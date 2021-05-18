#![deny(clippy::all)]

#[macro_use]
extern crate serde_derive;

pub mod backends;
pub mod chunks;
pub mod compress;
pub mod crypto;
pub mod meta;
pub mod object;
pub mod stash;

pub mod index;

pub use crate::crypto::StashKey;
pub use crate::index::Index;
pub use crate::object::ObjectId;
pub use crate::stash::Stash;

pub use async_trait::async_trait;

// Use block size of 4MiB for now
pub const BLOCK_SIZE: usize = 4 * 1024 * 1024;
