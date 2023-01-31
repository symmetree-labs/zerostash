#[macro_use]
extern crate serde_derive;

use std::path::PathBuf;

use directory::Dir;
use infinitree::*;

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
type ParentPaths = fields::VersionedMap<usize, Vec<PathBuf>>;
type BasePath = fields::VersionedMap<usize, PathBuf>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    pub files: FileIndex,
    pub zfs_snapshots: ZfsIndex,
    pub directories: DirectoryIndex,
    pub upmost_parents: ParentPaths,
    pub base_path: BasePath,
}
