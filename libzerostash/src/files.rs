use crate::chunks::ChunkPointer;
use crate::meta::{FieldReader, FieldWriter, MetaObjectField};

use failure::Error;
use parking_lot::RwLock;

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

#[derive(Hash, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub unix_secs: u64,
    pub unix_nanos: u32,
    pub unix_perm: u32,
    pub unix_uid: u32,
    pub unix_gid: u32,

    pub size: u64,
    pub readonly: bool,
    pub name: String,

    pub chunks: Vec<(u64, Arc<ChunkPointer>)>,
}

impl Entry {
    #[cfg(windows)]
    pub fn from_file(file: &fs::File, path: impl AsRef<Path>) -> Result<Entry, Error> {
        let path = path.as_ref();
        let metadata = file.metadata()?;
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        Ok(File {
            unix_secs,
            unix_nanos,
            unix_perm: 0,
            unix_uid: 0,
            unix_gid: 0,

            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            name: path.as_ref().to_str().unwrap().to_string(),

            chunks: Vec::new(),
        })
    }

    #[cfg(unix)]
    pub fn from_file(file: &fs::File, path: impl AsRef<Path>) -> Result<Entry, Error> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = file.metadata()?;
        let perms = metadata.permissions();
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        Ok(Entry {
            unix_secs,
            unix_nanos,
            unix_perm: perms.mode(),
            unix_uid: metadata.uid(),
            unix_gid: metadata.gid(),

            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            name: path.as_ref().to_str().unwrap().to_string(),

            chunks: Vec::new(),
        })
    }
}

fn to_unix_mtime(m: &fs::Metadata) -> Result<(u64, u32), Error> {
    let mtime = m.modified()?.duration_since(UNIX_EPOCH)?;
    Ok((mtime.as_secs(), mtime.subsec_nanos()))
}

pub trait FileIndex: Clone + Send {
    fn len(&self) -> usize;
    fn for_each(&self, iter: impl FnMut(Arc<Entry>));
    fn to_vec(self) -> Vec<Arc<Entry>>;
    fn has_changed(&self, file: &Entry) -> bool;
    fn push(&mut self, file: Entry);
}

#[derive(Clone, Default)]
pub struct HashMapFileIndex(Arc<RwLock<HashSet<Arc<Entry>>>>);

impl MetaObjectField for HashMapFileIndex {
    type Item = Entry;

    fn serialize(&self, mw: &mut impl FieldWriter) {
        self.for_each(|f| mw.write_next(f));
    }

    fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        let mut map = self.0.write();
        while let Ok(file) = mw.read_next() {
            map.insert(Arc::new(file));
        }
    }
}

impl FileIndex for HashMapFileIndex {
    fn for_each(&self, mut iter: impl FnMut(Arc<Entry>)) {
        for f in self.0.read().iter() {
            iter(f.clone());
        }
    }

    fn has_changed(&self, file: &Entry) -> bool {
        !self.0.read().contains(file)
    }

    fn push(&mut self, file: Entry) {
        self.0.write().insert(Arc::new(file));
    }

    fn len(&self) -> usize {
        self.0.read().len()
    }

    fn to_vec(self) -> Vec<Arc<Entry>> {
        self.0.write().drain().collect()
    }
}
