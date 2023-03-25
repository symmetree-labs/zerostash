//! `zfs destroy` subcommand

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsDestroy {
    #[clap(flatten)]
    stash: StashArgs,

    /// The snapshot stored inside the stash
    #[clap(long)]
    snapshot: String,
}

#[async_trait]
impl AsyncRunnable for ZfsDestroy {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        stash.load_all().unwrap();

        stash.index().snapshots.remove(self.snapshot.clone());

        stash
            .commit(format!("Destroyed snapshot '{}'", self.snapshot))
            .expect("Failed to write metadata");
        stash.backend().sync().expect("Failed to write to storage");
    }
}
