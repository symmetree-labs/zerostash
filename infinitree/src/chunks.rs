use crate::crypto::{Digest, Tag};
use crate::object::ObjectId;

use std::sync::Arc;

#[derive(Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct RawChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: Digest,
    pub tag: Tag,
}

pub type ChunkPointer = Arc<RawChunkPointer>;
pub type ChunkIndex = crate::index::Map<Digest, ChunkPointer>;
