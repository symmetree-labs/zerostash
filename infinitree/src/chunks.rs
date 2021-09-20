use crate::crypto::{Digest, Tag};
use crate::object::ObjectId;

#[derive(Clone, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub(crate) struct RawChunkPointer {
    pub offs: u32,
    pub size: u32,
    pub file: ObjectId,
    pub hash: Digest,
    pub tag: Tag,
}

#[derive(Clone, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub struct ChunkPointer(RawChunkPointer);

impl ChunkPointer {
    #[inline(always)]
    pub(crate) fn new(offs: u32, size: u32, file: ObjectId, hash: Digest, tag: Tag) -> Self {
        Self(RawChunkPointer {
            offs,
            size,
            file,
            hash,
            tag,
        })
    }

    #[inline(always)]
    pub(crate) fn as_raw(&self) -> &RawChunkPointer {
        &self.0
    }

    #[inline(always)]
    pub fn object_id(&self) -> &ObjectId {
        &self.0.file
    }

    #[inline(always)]
    pub fn hash(&self) -> &Digest {
        &self.0.hash
    }
}
