//! Zerostash
//!
//! Application based on the [Abscissa] framework.
//!
//! [Abscissa]: https://github.com/iqlusioninc/abscissa

// Tip: Deny warnings with `RUSTFLAGS="-D warnings"` environment variable in CI

#![forbid(unsafe_code)]
#![deny(
    arithmetic_overflow,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms,
    trivial_casts,
    unused_crate_dependencies,
    unused_lifetimes,
    unused_qualifications
)]

pub mod migration;
pub mod application;
pub mod commands;
pub mod config;
pub mod error;
pub mod keygen;
pub mod prelude;
#[cfg(feature = "fuse")]
pub use zerostash_fuse;

// These dependencies are required for the e2e benchmark
#[cfg(test)]
use tokio as _;
#[cfg(test)]
use tracing as _;
#[cfg(test)]
use tracing_subscriber as _;
#[cfg(test)]
use walkdir as _;

#[cfg(unix)]
use dirs as _;
#[cfg(windows)]
use nix as _;
#[cfg(windows)]
use xdg as _;
