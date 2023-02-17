#[macro_use]
extern crate serde_derive;

use zerostash_fuse::dir::Dir;
use infinitree::*;
use std::path::PathBuf;

pub mod directory;
pub use directory::*;
mod files;
pub use files::*;
mod zfs_snapshots;
pub use zfs_snapshots::*;
pub mod rollsum;
pub mod splitter;
mod stash;

pub use stash::list_snapshots::ZfsSnapshotList;
pub use stash::restore;
pub use stash::store;

type ChunkIndex = fields::VersionedMap<Digest, ChunkPointer>;
type FileIndex = fields::VersionedMap<String, Entry>;
type ZfsIndex = fields::VersionedMap<String, ZfsSnapshot>;
type DirectoryIndex = fields::VersionedMap<PathBuf, Vec<Dir>>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    pub files: FileIndex,
    pub zfs_snapshots: ZfsIndex,
    pub directories: DirectoryIndex,
}
