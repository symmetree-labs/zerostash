use super::{Backend, Result};
use crate::object::{Object, ObjectId, ReadBuffer, ReadObject, WriteObject};

use lru::LruCache;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(feature = "mmap")]
struct MmappedFile {
    mmap: memmap2::Mmap,
    _file: std::fs::File,
}

#[cfg(feature = "mmap")]
impl MmappedFile {
    fn new(len: usize, _file: std::fs::File) -> Result<Self> {
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(len)
                .populate()
                .map(&_file)?
        };
        Ok(Self { mmap, _file })
    }
}

#[cfg(feature = "mmap")]
impl AsRef<[u8]> for MmappedFile {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        self.mmap.as_ref()
    }
}

#[cfg(feature = "mmap")]
#[inline(always)]
fn get_buf(filename: impl AsRef<Path>) -> Result<ReadBuffer> {
    let mmap = MmappedFile::new(crate::BLOCK_SIZE, fs::File::open(filename)?)?;
    Ok(ReadBuffer::new(mmap))
}

#[cfg(not(feature = "mmap"))]
#[inline(always)]
fn get_buf(filename: impl AsRef<Path>) -> Result<ReadBuffer> {
    Ok(ReadBuffer::new(fs::read(&filename)?))
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
            read_lru: Arc::new(Mutex::new(LruCache::new(256))),
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
                let obj = Arc::new(Object::with_id(
                    *id,
                    get_buf(self.target.join(id.to_string()))?,
                ));

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
