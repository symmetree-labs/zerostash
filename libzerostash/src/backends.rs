use crate::object::{Object, ObjectId, ReadBuffer, ReadObject, WriteObject};

use lru::LruCache;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("IO error: {source}")]
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

#[derive(Clone)]
pub struct Directory {
    target: PathBuf,
    read_lru: Arc<Mutex<LruCache<ObjectId, Arc<ReadObject>>>>,
}

impl Directory {
    pub fn new(target: impl AsRef<Path>) -> Result<Directory> {
        std::fs::create_dir_all(&target)?;
        Ok(Directory {
            target: target.as_ref().into(),
            read_lru: Arc::new(Mutex::new(LruCache::new(100))),
        })
    }
}

impl Backend for Directory {
    fn write_object(&self, object: &WriteObject) -> Result<()> {
        let filename = self.target.join(object.id().to_string());
        fs::write(filename, object.as_inner())?;
        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
        let lru = {
            let mut lock = self.read_lru.lock().unwrap();
            lock.get(id).cloned()
        };

        match lru {
            Some(buffer) => Ok(buffer),
            None => {
                let filename = self.target.join(id.to_string());
                let file = fs::read(&filename)?;
                let obj = Arc::new(Object::with_id(*id, ReadBuffer::new(file)));

                self.read_lru.lock().unwrap().put(*id, obj.clone());

                Ok(obj)
            }
        }
    }
}

pub mod test {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Clone, Default)]
    pub struct InMemoryBackend(Arc<Mutex<HashMap<ObjectId, Arc<ReadObject>>>>);

    impl Backend for InMemoryBackend {
        fn write_object(&self, object: &WriteObject) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .insert(*object.id(), Arc::new(object.into()));
            Ok(())
        }

        fn read_object(&self, id: &ObjectId) -> Result<Arc<ReadObject>> {
            self.0
                .lock()
                .unwrap()
                .get(id)
                .ok_or(BackendError::NoObjectFound)
                .map(Arc::clone)
        }
    }

    #[derive(Clone, Default)]
    pub struct NullBackend(Arc<Mutex<usize>>);

    #[allow(clippy::len_without_is_empty)]
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
}
