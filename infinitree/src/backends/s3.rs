use super::{Backend, BackendError, Context, Directory, Result};
use crate::object::{Object, ObjectId, ReadBuffer, ReadObject, WriteObject};

use dashmap::DashMap;
use lru::LruCache;
use parking_lot::RwLock;
use rusoto_core::Region;
use rusoto_s3::{GetObjectRequest, PutObjectOutput, PutObjectRequest, S3Client, S3};
use tokio::{
    runtime,
    task::{self, JoinHandle},
};

use std::{
    convert::TryFrom,
    fs::{self, read_dir, DirEntry},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

type TaskHandle = tokio::task::JoinHandle<std::result::Result<PutObjectOutput, anyhow::Error>>;

#[derive(Clone)]
pub struct InMemoryS3 {
    client: S3Client,
    bucket: String,
    handles: Arc<RwLock<Vec<(ObjectId, TaskHandle)>>>,
}

impl InMemoryS3 {
    pub fn new(region: Region, bucket: String) -> Result<Self> {
        let client = S3Client::new(region);

        Ok(Self {
            client,
            bucket,
            handles: Arc::default(),
        })
    }
}

impl Backend for InMemoryS3 {
    fn write_object(&self, object: &WriteObject) -> Result<()> {
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        let body = Some(object.as_inner().to_vec().into());
        let key = object.id().to_string();

        self.handles.write().push((
            *object.id(),
            task::spawn(async move {
                client
                    .put_object(PutObjectRequest {
                        bucket,
                        key,
                        body,
                        ..Default::default()
                    })
                    .await
                    .context("Failed to write object")
            }),
        ));

        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
        let object: std::result::Result<Vec<u8>, BackendError> = {
            let client = self.client.clone();
            let bucket = self.bucket.clone();
            let key = id.to_string();

            runtime::Handle::current().block_on(async move {
                let s3obj = client
                    .get_object(GetObjectRequest {
                        bucket,
                        key,
                        ..GetObjectRequest::default()
                    })
                    .await
                    .context("Failed to fetch object")?;

                let mut buf = vec![];
                tokio::io::copy(
                    &mut s3obj
                        .body
                        .context("No body for retrieved object")?
                        .into_async_read(),
                    &mut buf,
                )
                .await?;
                Ok(buf)
            })
        };

        Ok(Arc::new(Object::with_id(*id, ReadBuffer::new(object?))))
    }

    fn delete(&self, _objects: &[ObjectId]) -> Result<()> {
        Ok(())
    }
}

struct FileAccess {
    atime: SystemTime,
    id: ObjectId,
    path: PathBuf,
}

impl FileAccess {
    fn new(id: ObjectId, path: impl AsRef<Path>) -> Self {
        let mut path = path.as_ref().to_owned();
        path.push(id.to_string());

        Self {
            id,
            path,
            atime: SystemTime::now(),
        }
    }

    fn delete(self) {
        fs::remove_file(self.path).unwrap();
    }
}

impl From<DirEntry> for FileAccess {
    fn from(direntry: DirEntry) -> Self {
        let atime = direntry.metadata().unwrap().accessed().unwrap();
        let path = direntry.path();
        let id = ObjectId::try_from(path.file_name().unwrap().to_str().unwrap()).unwrap();

        Self { atime, id, path }
    }
}

#[derive(Clone)]
pub struct Cache<Upstream> {
    file_list: Arc<tokio::sync::RwLock<LruCache<ObjectId, FileAccess>>>,
    in_flight: Arc<DashMap<ObjectId, TaskHandle>>,
    handles: Arc<DashMap<ObjectId, JoinHandle<()>>>,

    size_limit: NonZeroUsize,
    upstream: Upstream,
    directory: Directory,
}

impl<Upstream> Cache<Upstream> {
    pub fn new(
        local: impl AsRef<Path>,
        size_limit: NonZeroUsize,
        upstream: Upstream,
    ) -> Result<Self> {
        let local = PathBuf::from(local.as_ref());
        let mut file_list = read_dir(&local)?
            .filter_map(|de| match de {
                Ok(de) => match de.file_type().map(|ft| ft.is_file()) {
                    Ok(true) => Some(de),
                    _ => None,
                },
                _ => None,
            })
            .map(FileAccess::from)
            .collect::<Vec<_>>();

        // we want to insert files in access time order so that we can
        // always drop the least recently used from the cache.
        //
        // many filesystems will flat out ignore atime and we fall
        // back to ctime. we're rolling on a best effort basis here.
        //
        // this also makes sense since when an object gets used, it's
        // bumped in the lru, therefore it's not "old" anymore as far
        // as the running process is concerned.
        //
        // to actually maintain a lru between processes would require
        // dumping the lru, which complicates the logic and
        // produces additional metadata in the local cache that may
        // make sense to be protected (?). idk, good enough.

        file_list.sort_by(|a, b| a.atime.cmp(&b.atime));

        let mut files = LruCache::unbounded();
        for file in file_list {
            files.put(file.id, file);
        }

        Ok(Self {
            upstream,
            size_limit,
            directory: Directory::new(local)?,
            in_flight: Arc::default(),
            file_list: Arc::new(tokio::sync::RwLock::new(files)),
            handles: Arc::default(),
        })
    }

    async fn make_space_for_object(&self) -> Result<Vec<ObjectId>> {
        let mut evicted = vec![];

        // due to the async-icity of this, we don't want to sit on a
        // read-lock for the entire scope of this function
        while self.file_list.read().await.len() * crate::BLOCK_SIZE >= self.size_limit.into() {
            // unwrap won't blow up, because if it is `None`, that
            // implies `files.len()` is 0, while `size_limit` is
            // non-zero, therefore we won't enter the loop
            let id = *self.file_list.read().await.peek_lru().unwrap().0;
            if let Some((_, future)) = self.in_flight.remove(&id) {
                // can't start deleting objects during a pending
                // up-stream transaction
                future.await.context("In-flight transaction failed")??;
            }

            let file = self.file_list.write().await.pop(&id).unwrap();

            file.delete();
            evicted.push(id);
        }

        Ok(evicted)
    }

    async fn add_new_object(&self, obj: &WriteObject) -> Result<Vec<ObjectId>> {
        if self.file_list.write().await.get(obj.id()).is_none() {
            let evicted = self.make_space_for_object().await?;

            self.directory.write_object(obj)?;

            self.file_list
                .write()
                .await
                .put(*obj.id(), FileAccess::new(*obj.id(), self.directory.path()));

            return Ok(evicted);
        }

        Ok(vec![])
    }
}

impl<Upstream: 'static + Backend + Clone> Backend for Cache<Upstream> {
    fn write_object(&self, object: &WriteObject) -> Result<()> {
        let cache = self.clone();
        let object = object.clone();

        self.handles.insert(
            *object.id(),
            task::spawn(async move {
                let _ = cache.add_new_object(&object).await;
                cache.upstream.write_object(&object).unwrap();
            }),
        );

        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
        let cache = self.clone();

        runtime::Handle::current().block_on(async move {
            match cache.file_list.write().await.get(id) {
                Some(_) => cache.directory.read_object(id),
                None => {
                    let object = cache.upstream.read_object(id);
                    if let Ok(ref obj) = object {
                        self.add_new_object(&obj.into()).await?;
                    }

                    object
                }
            }
        })
    }

    fn preload(&self, _objects: &[ObjectId]) -> Result<()> {
        Ok(())
    }

    fn delete(&self, _objects: &[ObjectId]) -> Result<()> {
        Ok(())
    }

    fn sync(&self) -> Result<()> {
        Ok(())
    }
}
