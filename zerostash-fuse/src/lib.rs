#![deny(unused_crate_dependencies)]
pub mod mount;
pub mod chunks;
pub mod openfile;

#[cfg(test)]
use criterion as _;
