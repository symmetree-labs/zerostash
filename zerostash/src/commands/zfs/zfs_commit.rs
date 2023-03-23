//! `zfs commit` subcommand

use std::io::Read;

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsCommit {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: zerostash_files::store::Options,

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
        let mut stream = Vec::new();
        std::io::stdin().read_to_end(&mut stream).unwrap();

        let mut stash = self.stash.open();
        stash.load_all().unwrap();

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
