//! `zfs extract` subcommand

use infinitree::Infinitree;
use zerostash_files::Files;

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsExtract {
    #[clap(flatten)]
    stash: StashArgs,

    /// The snapshot stored inside the stash
    #[clap(long)]
    snapshot: String,
}

#[async_trait]
impl AsyncRunnable for ZfsExtract {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        stash.load(stash.index().snapshots()).unwrap();

        extract_snapshot(&stash, &self.snapshot);
    }
}

fn extract_snapshot(stash: &Infinitree<Files>, snapshot: &str) {
    if let Some(stream) = stash.index().snapshots.get(snapshot) {
        let reader = stash.storage_reader().unwrap();
        stream.to_stdout(reader).expect("Failed to write to stdout");
    } else {
        panic!("Snapshot not stashed!");
    }
}
