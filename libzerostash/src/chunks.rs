use crate::crypto::{CryptoDigest, Tag};
use crate::meta::{FieldReader, FieldWriter, MetaObjectField};
use crate::objects::{ObjectError, ObjectId};

use async_trait::async_trait;
use dashmap::{mapref::entry::Entry, DashMap};
use std::{future::Future, marker::Unpin, sync::Arc};

#[derive(Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct ChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: CryptoDigest,
    pub tag: Tag,
}

pub type ChunkIndex = DashMap<CryptoDigest, Arc<ChunkPointer>>;

#[derive(Clone, Default)]
pub struct ChunkStore(Arc<ChunkIndex>);
impl ChunkStore {
    pub fn index(&self) -> &ChunkIndex {
        &self.0
    }

    pub async fn push(
        &self,
        digest: CryptoDigest,
        store: (impl Future<Output = Result<Arc<ChunkPointer>, ObjectError>> + Unpin),
    ) -> Result<Arc<ChunkPointer>, ObjectError> {
        // do a simple check to ensure we don't write-lock straight away
        if let Some(ptr) = self.0.get(&digest) {
            return Ok(ptr.clone());
        }

        // be as lazy as possible in storing the object:
        // at this stage the store is locked, so it's still best to
        // release it asap
        match self.0.entry(digest) {
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
        for f in self.0.iter() {
            mw.write_next((f.key(), f.value())).await;
        }
    }

    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        while let Ok((hash, pointer)) = mw.read_next().await {
            self.0.insert(hash, pointer);
        }
    }
}
