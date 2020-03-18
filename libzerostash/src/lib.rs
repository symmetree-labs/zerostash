#![deny(clippy::all)]

#[macro_use]
extern crate serde_derive;

pub mod backends;
pub mod chunks;
pub mod compress;
pub mod crypto;
pub mod files;
pub mod meta;
pub mod objects;
pub mod stash;

pub mod rollsum;
pub mod splitter;

pub use crypto::StashKey;
pub use stash::Stash;

// Use block size of 4MiB for now
pub const BLOCK_SIZE: usize = 4 * 1024 * 1024;
