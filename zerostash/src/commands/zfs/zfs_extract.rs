//! `zfs extract` subcommand

use std::io::Write;

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsExtract {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: zerostash_files::store::Options,

    /// The snapshot stored inside the stash
    #[clap(long)]
    snapshot: String,
}

#[async_trait]
impl AsyncRunnable for ZfsExtract {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        stash.load_all().unwrap();
        {
            let snapshots = &stash.index().snapshots;
            if let Some(stream) = snapshots.get(&self.snapshot) {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                lock.write_all(&stream).expect("Failed to write the stream")
            } else {
                panic!("Snapshot not stashed!");
            }
        }
    }
}
