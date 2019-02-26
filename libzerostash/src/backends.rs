use crate::objects::{BlockBuffer, Object, ObjectId, WriteObject};

use failure::Error;
use lru::LruCache;
use memmap::{Mmap, MmapOptions};

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub trait Backend: Clone + Send {
    type Buffer: AsRef<[u8]>;

    fn write_object(&self, object: &WriteObject) -> Result<(), Error>;
    fn read_object(&self, id: &ObjectId) -> Result<Arc<Object<Self::Buffer>>, Error>;
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
    read_lru: Arc<Mutex<LruCache<ObjectId, Arc<Object<MmappedFile>>>>>,
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
    type Buffer = MmappedFile;

    fn write_object(&self, object: &WriteObject) -> Result<(), Error> {
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
            mmap.copy_from_slice(object.as_ref());
        }

        file.flush()?;
        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<Object<Self::Buffer>>, Error> {
        let mut lru = self.read_lru.lock().unwrap();

        match lru.get(id) {
            Some(mmap) => Ok(mmap.clone()),
            None => {
                let filename = self.target.join(id.to_string());
                let file = fs::OpenOptions::new().read(true).open(filename)?;
                let mmap = unsafe { MmapOptions::new().map(&file)? };

                let obj = Arc::new(Object::with_id(*id, MmappedFile { _file: file, mmap }));
                lru.put(*id, obj.clone());

                Ok(obj)
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct InMemoryBackend(Arc<Mutex<HashMap<ObjectId, WriteObject>>>);

impl Backend for InMemoryBackend {
    type Buffer = BlockBuffer;

    fn write_object(&self, object: &WriteObject) -> Result<(), Error> {
        self.0.lock().unwrap().insert(object.id, object.clone());
        Ok(())
    }

    fn read_object(&self, id: &ObjectId) -> Result<Arc<Object<Self::Buffer>>, Error> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| format_err!("invalid"))
            .map(Object::clone)
            .map(Arc::new)
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
    type Buffer = Vec<u8>;

    fn write_object(&self, _object: &WriteObject) -> Result<(), Error> {
        *self.0.lock().unwrap() += 1;
        Ok(())
    }

    fn read_object(&self, _id: &ObjectId) -> Result<Arc<Object<Self::Buffer>>, Error> {
        unimplemented!();
    }
}
