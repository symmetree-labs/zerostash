#![forbid(unsafe_code)]
#![deny(clippy::all)]

#[macro_use]
extern crate serde_derive;

pub mod backends;
pub(crate) mod compress;
pub mod index;
pub mod object;

mod chunks;
mod crypto;
mod tree;

pub use crate::backends::Backend;
pub use crate::index::Index;
pub use crate::object::ObjectId;

pub use chunks::ChunkPointer;
pub use crypto::{chunk_hash, ChunkKey, Digest, IndexKey, Key};
pub use tree::Infinitree;

pub use anyhow;
pub use infinitree_macros::Index;

// Use block size of 4MiB for now
const BLOCK_SIZE: usize = 4 * 1024 * 1024;