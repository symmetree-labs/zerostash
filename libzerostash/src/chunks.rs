use crate::crypto::CryptoDigest;
use crate::meta::{FieldReader, FieldWriter, MetaObjectField};
use crate::objects::ObjectId;

use failure::Error;
use parking_lot::RwLock;

use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;

#[derive(Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct ChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: CryptoDigest,
}

pub trait ChunkIndex: Clone + Send {
    fn new() -> Self;
    fn for_each(&self, iter: impl FnMut((&CryptoDigest, &Arc<ChunkPointer>)));
    fn to_vec(self) -> Vec<(CryptoDigest, Arc<ChunkPointer>)>;
    fn get_address(&self, digest: &CryptoDigest) -> Option<Arc<ChunkPointer>>;
    fn len(&self) -> usize;
    fn push(
        &self,
        digest: CryptoDigest,
        store: impl FnMut() -> Result<Arc<ChunkPointer>, Error>,
    ) -> Result<Arc<ChunkPointer>, Error>;
}

#[derive(Clone)]
pub struct RwLockIndex(Arc<RwLock<HashMap<CryptoDigest, Arc<ChunkPointer>>>>);
impl MetaObjectField for RwLockIndex {
    type Item = (CryptoDigest, Arc<ChunkPointer>);

    fn serialize(&self, mw: &mut impl FieldWriter) {
        self.for_each(|f| mw.write_next(f));
    }

    fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        let mut map = self.0.write();
        while let Ok((hash, pointer)) = mw.read_next() {
            map.insert(hash, pointer);
        }
    }
}

impl ChunkIndex for RwLockIndex {
    fn new() -> Self {
        RwLockIndex(Arc::default())
    }

    fn len(&self) -> usize {
        self.0.read().len()
    }

    fn for_each(&self, mut iter: impl FnMut((&CryptoDigest, &Arc<ChunkPointer>))) {
        for c in self.0.write().iter() {
            iter(c)
        }
    }

    fn to_vec(self) -> Vec<(CryptoDigest, Arc<ChunkPointer>)> {
        self.0.write().drain().collect()
    }

    #[inline]
    fn get_address(&self, digest: &CryptoDigest) -> Option<Arc<ChunkPointer>> {
        let root = self.0.read_recursive();
        root.get(digest).map(Arc::clone)
    }

    fn push(
        &self,
        digest: CryptoDigest,
        mut store: impl FnMut() -> Result<Arc<ChunkPointer>, Error>,
    ) -> Result<Arc<ChunkPointer>, Error> {
        if let Some(ptr) = self.get_address(&digest) {
            return Ok(ptr);
        }

        let mut dir = self.0.write();

        // be as lazy as possible in storing the object:
        // at this stage the store is locked, so it's still best to
        // release it asap
        match dir.entry(digest) {
            Entry::Occupied(e) => Ok(e.get().clone()),
            Entry::Vacant(e) => {
                let address = (store)()?;
                e.insert(address.clone());
                Ok(address)
            }
        }
    }
}
