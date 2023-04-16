//! `zfs destroy` subcommand

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsDestroy {
    #[clap(flatten)]
    stash: StashArgs,

    /// name of the stored snapshot
    #[clap(short = 'n', long)]
    name: String,
}

#[async_trait]
impl AsyncRunnable for ZfsDestroy {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        stash.load_all().unwrap();

        stash.index().snapshots.remove(self.name.clone());

        stash
            .commit(format!("Destroyed snapshot '{}'", self.name))
            .expect("failed to write metadata");
        stash.backend().sync().expect("failed to write to storage");
    }
}
