#![deny(clippy::all)]

#[macro_use]
extern crate serde_derive;

pub mod backends;
pub mod chunks;
pub(crate) mod compress;
pub mod crypto;
pub mod index;
pub mod object;
pub mod stash;

pub use crate::crypto::StashKey;
pub use crate::index::Index;
pub use crate::object::ObjectId;
pub use crate::stash::Stash;

pub use anyhow;
pub use infinitree_macros::Index;

// Use block size of 4MiB for now
const BLOCK_SIZE: usize = 4 * 1024 * 1024;
