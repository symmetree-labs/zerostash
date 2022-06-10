//! `wipe` subcommand

use crate::prelude::*;

#[derive(Command, Debug, Clone)]
pub struct Wipe {
    stash: String,
}

#[async_trait]
impl AsyncRunnable for Wipe {
    /// Start the application.
    async fn run(&self) {
        use crate::config::Backend::*;

        let config = APP.config();
        let path = match config.resolve_stash(&self.stash) {
            None => self.stash.clone(),
            Some(stash) => match &stash.backend {
                Filesystem { path } => path.clone(),
                _ => {
                    println!("Wipe: Non-local backend found, skipping...");
                    return;
                }
            },
        };

        std::fs::remove_dir_all(path).expect("Error while wiping stash...");
    }
}
