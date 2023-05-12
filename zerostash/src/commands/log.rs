//! `log` subcommand

use crate::prelude::*;
use chrono::{DateTime, Utc};

#[derive(Command, Debug)]
pub struct Log {
    #[clap(flatten)]
    stash: StashArgs,
}

#[async_trait]
impl AsyncRunnable for Log {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        let mut stdout = std::io::stdout().lock();

        for commit in stash.commit_list().iter() {
            let time: DateTime<Utc> = commit.metadata.time.into();
            let local_time = time.with_timezone(&chrono::Local);
            let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();

            if writeln!(
                stdout,
                "{:?}\t{}\t{}",
                commit.id,
                formatted_time,
                commit
                    .metadata
                    .message
                    .as_ref()
                    .unwrap_or(&"No commit message".to_string())
            )
            .is_err()
            {
                break;
            }
        }
    }
}
