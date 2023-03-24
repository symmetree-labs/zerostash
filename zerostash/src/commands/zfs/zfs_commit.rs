//! `zfs commit` subcommand

use std::io::{Read, Write};

use infinitree::object::BufferedSink;

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
        let mut buf = Vec::default();
        std::io::stdin()
            .read_to_end(&mut buf)
            .expect("Failed to read the stream");

        let mut stash = self.stash.open();

        let mut sink = BufferedSink::new(stash.storage_writer().unwrap());
        sink.write_all(&buf).unwrap();
        let stream = sink.finish().unwrap();
        {
            let snapshots = &stash.index().snapshots;
            if snapshots
                .update_with(self.snapshot.clone(), |_v| stream.clone())
                .is_none()
            {
                snapshots.insert(self.snapshot.clone(), stream);
            }
        }

        stash
            .commit(self.message.clone())
            .expect("Failed to write metadata");
        stash.backend().sync().expect("Failed to write to storage");
    }
}
