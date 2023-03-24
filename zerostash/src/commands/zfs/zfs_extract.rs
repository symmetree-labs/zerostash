//! `zfs extract` subcommand

use std::io::{Read, Write};

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
        stash.load_all().unwrap();
        {
            let snapshots = &stash.index().snapshots;
            if let Some(stream) = snapshots.get(&self.snapshot) {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                let reader = stash.storage_reader().unwrap();

                let mut stream = stream.open_reader(reader);
                let mut buf = Vec::default();
                stream.read_to_end(&mut buf).unwrap();

                lock.write_all(&buf).expect("Failed to write the stream")
            } else {
                panic!("Snapshot not stashed!");
            }
        }
    }
}
