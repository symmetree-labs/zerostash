//! `commit` subcommand

use crate::prelude::*;
use clap::Parser;

#[derive(Command, Parser, Debug)]
pub struct Commit {
    /// Stash path or alias
    stash: String,

    #[clap(flatten)]
    options: zerostash_files::store::Options,

    /// Commit message to include in the changeset
    #[clap(short = 'm', long)]
    message: Option<String>,
}

impl Runnable for Commit {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.open_stash(&self.stash);
            stash.load_all().unwrap();

            self.options
                .add_recursive(&stash, APP.get_worker_threads())
                .await
                .unwrap();

            stash
                .commit(self.message.clone())
                .expect("Failed to write metadata");
        })
        .unwrap();
    }
}
