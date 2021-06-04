use super::{Backend, Result};
use crate::object::{Object, ObjectId, ReadBuffer, ReadObject, WriteObject};

use lru::LruCache;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

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
            read_lru: Arc::new(Mutex::new(LruCache::new(550))),
        })
    }

    pub fn path(&self) -> &Path {
        &self.target
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

    fn delete(&self, objects: &[ObjectId]) -> Result<()> {
        for id in objects {
            fs::remove_file(self.target.join(id.to_string()))?;
        }

        Ok(())
    }
}
