//! `checkout` subcommand

use crate::prelude::*;
use zerostash_files::restore;

#[derive(Command, Debug, Clone)]
pub struct Checkout {
    stash: String,

    #[clap(flatten)]
    options: restore::Options,
}

#[async_trait]
impl AsyncRunnable for Checkout {
    /// Start the application.
    async fn run(&self) {
        let mut stash = APP.open_stash(&self.stash);
        stash.load_all().unwrap();

        self.options
            .from_iter(&stash, APP.get_worker_threads())
            .await
            .expect("Error extracting data");
    }
}
