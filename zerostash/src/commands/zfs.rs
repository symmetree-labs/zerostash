use crate::prelude::AsyncRunnable;
use async_trait::async_trait;
use clap::Parser;

mod commit;
mod destroy;
mod extract;
mod ls;

#[derive(Debug, Parser)]
pub enum Zfs {
    /// Add a ZFS snapshot to the stash
    Commit(commit::ZfsCommit),

    /// Extracts a snapshot to stdout
    Extract(extract::ZfsExtract),

    /// Remove a snapshot from the stash
    Destroy(destroy::ZfsDestroy),

    /// List Snapshots in a stash
    Ls(ls::ZfsLs),
}

#[async_trait]
impl AsyncRunnable for Zfs {
    async fn run(&self) {
        use Zfs::*;
        match self {
            Commit(c) => c.run().await,
            Extract(e) => e.run().await,
            Destroy(d) => d.run().await,
            Ls(l) => l.run().await,
        }
    }
}
