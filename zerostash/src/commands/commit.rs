//! `commit` subcommand

use crate::prelude::*;

#[derive(Command, Debug, Clone)]
pub struct Commit {
    /// Stash path or alias
    stash: String,

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
        let mut stash = APP.open_stash(&self.stash);
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
