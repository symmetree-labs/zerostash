#[macro_use]
extern crate serde_derive;

use infinitree::*;

mod files;
pub use files::*;
pub mod rollsum;
pub mod splitter;
mod stash;

pub use stash::restore;
pub use stash::store;

type ChunkIndex = fields::VersionedMap<Digest, ChunkPointer>;
type FileIndex = fields::VersionedMap<String, Entry>;
type SnapshotIndex = fields::VersionedMap<String, Vec<u8>>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    pub files: FileIndex,
    pub snapshots: SnapshotIndex,
}
