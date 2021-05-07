use crate::{
    backends::Backend,
    chunks, files,
    index::Index,
    meta::{self, ReadError},
    object::{self, ObjectId},
};
pub use crate::{crypto::StashKey, meta::ObjectIndex};

use async_trait::async_trait;

use std::path::Path;
use std::sync::Arc;

pub(crate) mod restore;
pub(crate) mod store;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Default)]
pub struct FileStashIndex {
    chunks: chunks::ChunkStore,
    files: files::FileStore,
}

#[async_trait]
impl Index for FileStashIndex {
    async fn read_fields(
        &mut self,
        mut metareader: meta::Reader,
        start_object: ObjectId,
    ) -> Result<()> {
        let mut next_object = Some(start_object);

        while let Some(header) = match next_object {
            Some(ref o) => Some(metareader.open(o).await?),
            None => None,
        } {
            next_object = header.next_object();

            match metareader.read_into(&mut self.files).await {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };

            match metareader.read_into(&mut self.chunks).await {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };
        }

        Ok(())
    }

    async fn write_fields(&mut self, metawriter: &mut meta::Writer) -> Result<()> {
        metawriter.write_field(&self.files).await;
        metawriter.write_field(&self.chunks).await;
        metawriter.seal_and_store().await;

        Ok(())
    }
}

impl FileStashIndex {
    fn chunks(&self) -> &chunks::ChunkStore {
        &self.chunks
    }

    fn files(&self) -> &files::FileStore {
        &self.files
    }
}

pub struct Stash<I> {
    backend: Arc<dyn Backend>,
    index: I,
    master_key: StashKey,
}

impl Stash<FileStashIndex> {
    pub fn new(backend: Arc<dyn Backend>, master_key: StashKey, index: FileStashIndex) -> Self {
        Stash {
            backend,
            index,
            master_key,
        }
    }

    pub async fn read(&mut self) -> Result<&Self> {
        let metareader =
            meta::Reader::new(self.backend.clone(), self.master_key.get_meta_crypto()?);
        let start_object = self.master_key.root_object_id()?;

        self.index.read_fields(metareader, start_object).await?;
        Ok(self)
    }

    pub fn list<'a>(&'a self, glob: &'a [impl AsRef<str>]) -> restore::FileIterator<'a> {
        let matchers = glob
            .iter()
            .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
            .collect::<Vec<glob::Pattern>>();
        let base_iter = self.file_index().into_iter().map(|r| r.key().clone());

        match glob.len() {
            i if i == 0 => Box::new(base_iter),
            _ => Box::new(base_iter.filter(move |f| matchers.iter().any(|m| m.matches(&f.name)))),
        }
    }

    pub async fn restore_by_glob(
        &mut self,
        threads: usize,
        pattern: &[impl AsRef<str>],
        target: impl AsRef<Path>,
    ) -> Result<()> {
        restore::from_iter(
            threads,
            self.list(pattern),
            self.backend.clone(),
            self.master_key.get_object_crypto()?,
            target,
        )
        .await;

        Ok(())
    }

    pub async fn add_recursive(&mut self, threads: usize, path: impl AsRef<Path>) -> Result<()> {
        let objstore =
            object::AEADWriter::new(self.backend.clone(), self.master_key.get_object_crypto()?);

        store::recursive(threads, &self.index, objstore, path).await;

        Ok(())
    }

    pub async fn commit(&mut self) -> Result<ObjectIndex> {
        let mut mw = meta::Writer::new(
            self.master_key.root_object_id()?,
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        )?;

        self.index.write_fields(&mut mw).await?;

        Ok(mw.objects().clone())
    }

    pub fn file_index(&self) -> &files::FileIndex {
        self.index.files().index()
    }

    pub fn chunk_index(&self) -> &chunks::ChunkIndex {
        self.index.chunks().index()
    }
}
