use crate::prelude::AsyncRunnable;
use async_trait::async_trait;
use clap::Parser;
use std::boxed::Box;

use super::{ZfsCommit, ZfsDestroy, ZfsExtract, ZfsLs};

#[derive(Debug, Parser)]
pub enum ZfsCommand {
    /// Add a ZFS snapshot to the stash
    Commit(ZfsCommit),

    /// Extracts a snapshot to stdout
    Extract(ZfsExtract),

    /// Remove a snapshot from the stash
    Destroy(ZfsDestroy),

    /// List Snapshots in a stash
    Ls(ZfsLs),
}

#[async_trait]
impl AsyncRunnable for ZfsCommand {
    async fn run(&self) {
        use ZfsCommand::*;
        match self {
            Commit(c) => c.run().await,
            Extract(e) => e.run().await,
            Destroy(d) => d.run().await,
            Ls(l) => l.run().await,
        }
    }
}
