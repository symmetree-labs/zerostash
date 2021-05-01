use crate::crypto::{CryptoDigest, Tag};
use crate::meta::{FieldReader, FieldWriter, MetaObjectField};
use crate::object::{ObjectError, ObjectId};

use async_trait::async_trait;
// use dashmap::{mapref::entry::Entry, DashMap};
use tokio::sync::RwLock;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    future::Future,
    marker::Unpin,
    sync::Arc,
};

#[derive(Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct ChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: CryptoDigest,
    pub tag: Tag,
}

// pub type ChunkIndex = DashMap<CryptoDigest, Arc<ChunkPointer>>;
pub type ChunkIndex = RwLock<BTreeMap<CryptoDigest, Arc<ChunkPointer>>>;

#[derive(Clone)]
pub struct ChunkStore(Arc<ChunkIndex>);

impl Default for ChunkStore {
    fn default() -> Self {
        ChunkStore(Arc::new(RwLock::new(BTreeMap::default())))
    }
}

impl ChunkStore {
    pub fn index(&self) -> &ChunkIndex {
        &self.0
    }

    pub async fn get(&self, digest: &CryptoDigest) -> Option<Arc<ChunkPointer>> {
        self.0.read().await.get(digest).map(Clone::clone)
    }

    pub async fn push(
        &self,
        digest: CryptoDigest,
        store: (impl Future<Output = Result<Arc<ChunkPointer>, ObjectError>> + Unpin),
    ) -> Result<Arc<ChunkPointer>, ObjectError> {
        // be as lazy as possible in storing the object:
        // at this stage the store is locked, so it's still best to
        // release it asap
        let mut map = self.0.write().await;
        match map.entry(digest) {
            Entry::Occupied(e) => Ok(e.get().clone()),
            Entry::Vacant(e) => {
                let address = store.await?;
                e.insert(address.clone());
                Ok(address)
            }
        }
    }
}

#[async_trait]
impl MetaObjectField for ChunkStore {
    type Item = (CryptoDigest, Arc<ChunkPointer>);

    fn key() -> String {
        "chunks".to_string()
    }

    async fn serialize(&self, mw: &mut impl FieldWriter) {
        for record in self.0.read().await.iter() {
            mw.write_next(record).await;
        }
    }

    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        let mut map = self.0.write().await;
        while let Ok((hash, pointer)) = mw.read_next().await {
            map.insert(hash, pointer);
        }
    }
}
