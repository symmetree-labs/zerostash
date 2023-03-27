//! `zfs commit` subcommand

use infinitree::Infinitree;
use zerostash_files::{Files, Snapshot};

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsCommit {
    #[clap(flatten)]
    stash: StashArgs,

    /// Commit message to include in the changeset
    #[clap(short = 'm', long)]
    message: Option<String>,

    /// Snapshot name
    #[clap(long)]
    snapshot: String,
}

#[async_trait]
impl AsyncRunnable for ZfsCommit {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        stash.load(stash.index().snapshots()).unwrap();

        add_snapshot(&stash, self.snapshot.clone());

        stash
            .commit(self.message.clone())
            .expect("Failed to write metadata");
        stash.backend().sync().expect("Failed to write to storage");
    }
}

fn add_snapshot(stash: &Infinitree<Files>, snapshot: String) {
    let snapshots = &stash.index().snapshots;

    if snapshots.get(&snapshot).is_some() {
        panic!("Can't override existing Snapshot!");
    }

    let writer = stash.storage_writer().unwrap();
    let stream = Snapshot::from_stdin(writer).expect("Failed to capture Snapshot");

    snapshots.insert(snapshot, stream);
}
