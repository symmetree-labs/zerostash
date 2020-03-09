use crate::objects::{Object, ObjectId, ReadBuffer, ReadObject, WriteObject};

use lru::LruCache;
use memmap::{Mmap, MmapOptions};
use thiserror::Error;

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("IO error")]
    Io {
        #[from]
        source: io::Error,
    },
    #[error("No object found")]
    NoObjectFound,
    #[error("Can't create object")]
    Create,
}

pub type Result<T> = std::result::Result<T, BackendError>;

pub trait Backend: Send + Sync {
    fn write_object(&self, object: &WriteObject) -> Result<()>;
    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>>;
}

pub struct MmappedFile {
    pub _file: fs::File,
    pub mmap: Mmap,
}

impl AsRef<[u8]> for MmappedFile {
    fn as_ref(&self) -> &[u8] {
        self.mmap.as_ref()
    }
}

#[derive(Clone)]
pub struct Directory {
    target: Arc<PathBuf>,
    read_lru: Arc<Mutex<LruCache<ObjectId, Arc<ReadObject>>>>,
}

impl Directory {
    pub fn new(target: impl AsRef<Path>) -> Directory {
        fs::create_dir_all(&target).expect("dir");
        Directory {
            target: Arc::new(target.as_ref().into()),
            read_lru: Arc::new(Mutex::new(LruCache::new(100))),
        }
    }
}

impl Backend for Directory {
    fn write_object(&self, object: &WriteObject) -> Result<()> {
        let size = object.buffer.as_ref().len();

        let filename = self.target.join(object.id.to_string());

        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(filename)?;

        file.set_len(size as u64)?;

        {
            let mut mmap = unsafe { MmapOptions::new().len(size).map_mut(&file)? };
            mmap.copy_from_slice(object.buffer.as_ref());
        }

        file.flush()?;
        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
        let mut lru = self.read_lru.lock().unwrap();

        match lru.get(id) {
            Some(mmap) => Ok(mmap.clone()),
            None => {
                let filename = self.target.join(id.to_string());
                let file = fs::OpenOptions::new().read(true).open(filename)?;
                let mmap = unsafe { MmapOptions::new().map(&file)? };

                let obj = Arc::new(Object::with_id(
                    *id,
                    ReadBuffer::new(MmappedFile { _file: file, mmap }),
                ));
                lru.put(*id, obj.clone());

                Ok(obj)
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct InMemoryBackend(Arc<Mutex<HashMap<ObjectId, Arc<ReadObject>>>>);

impl Backend for InMemoryBackend {
    fn write_object(&self, object: &WriteObject) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(object.id, Arc::new(object.into()));
        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| BackendError::NoObjectFound)
            .map(Arc::clone)
    }
}

#[derive(Clone, Default)]
pub struct NullBackend(Arc<Mutex<usize>>);

impl NullBackend {
    pub fn len(&self) -> usize {
        *self.0.lock().unwrap()
    }
}

impl Backend for NullBackend {
    fn write_object(&self, _object: &WriteObject) -> Result<()> {
        *self.0.lock().unwrap() += 1;
        Ok(())
    }

    fn read_object(&self, _id: &ObjectId) -> Result<Arc<ReadObject>> {
        unimplemented!();
    }
}
