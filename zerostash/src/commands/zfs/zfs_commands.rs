use std::{boxed::Box, future::Future, marker::Send, pin::Pin};

use clap::Parser;

use crate::prelude::AsyncRunnable;

use super::{ZfsCommit, ZfsDestroy, ZfsExtract, ZfsLs};

/// Zerostash Subcommands
/// Subcommands need to be listed in an enum.
#[derive(Debug, Parser)]
pub enum ZerostashZfs {
    /// Add a ZFS snapshot to the stash
    Commit(ZfsCommit),

    /// Extracts a snapshot to stdout
    Extract(ZfsExtract),

    /// Remove a snapshot from the stash
    Destroy(ZfsDestroy),

    /// List Snapshots in a stash
    Ls(ZfsLs),
}

pub fn match_zfs_cmd<'a>(cmd: &'a ZerostashZfs) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    match cmd {
        ZerostashZfs::Commit(cmd) => cmd.run(),
        ZerostashZfs::Extract(cmd) => cmd.run(),
        ZerostashZfs::Destroy(cmd) => cmd.run(),
        ZerostashZfs::Ls(cmd) => cmd.run(),
    }
}
