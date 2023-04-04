//! `commit` subcommand

use crate::{migration::migration, prelude::*};

#[derive(Command, Debug)]
pub struct Commit {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: zerostash_files::store::Options,

    /// Commit message to include in the changeset
    #[clap(short = 'm', long)]
    message: Option<String>,
}

#[async_trait]
impl AsyncRunnable for Commit {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        migration(&mut stash);
        stash.load_all().unwrap();

        self.options
            .add_recursive(&stash, APP.get_worker_threads())
            .await
            .unwrap();

        stash
            .commit(self.message.clone())
            .expect("Failed to write metadata");
        stash.backend().sync().expect("Failed to write to storage");
    }
}
