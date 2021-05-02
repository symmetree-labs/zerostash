use crate::crypto::{CryptoDigest, Tag};
use crate::meta::{FieldReader, FieldWriter, MetaObjectField};
use crate::object::{ObjectError, ObjectId};

use async_trait::async_trait;
use dashmap::DashMap;

use std::sync::Arc;

#[derive(Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct RawChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: CryptoDigest,
    pub tag: Tag,
}

pub type ChunkPointer = Arc<RawChunkPointer>;
pub type ChunkIndex = DashMap<CryptoDigest, ChunkPointer>;

#[derive(Clone, Default)]
pub struct ChunkStore(Arc<ChunkIndex>);

impl ChunkStore {
    pub fn index(&self) -> &ChunkIndex {
        &self.0
    }

    pub fn get(&self, digest: &CryptoDigest) -> Option<ChunkPointer> {
        self.0.get(digest).map(|r| r.value().clone())
    }

    pub fn push(
        &self,
        digest: CryptoDigest,
        store: impl Fn() -> Result<ChunkPointer, ObjectError>,
    ) -> Result<ChunkPointer, ObjectError> {
        self.0
            .entry(digest)
            .or_try_insert_with(store)
            .map(|r| r.value().clone())
    }
}

#[async_trait]
impl MetaObjectField for ChunkStore {
    type Item = (CryptoDigest, ChunkPointer);

    fn key() -> String {
        "chunks".to_string()
    }

    async fn serialize(&self, mw: &mut impl FieldWriter) {
        for r in self.0.iter() {
            mw.write_next((r.key(), r.value())).await;
        }
    }

    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        while let Ok((hash, pointer)) = mw.read_next().await {
            self.0.insert(hash, pointer);
        }
    }
}
