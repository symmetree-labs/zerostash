//! `ls` subcommand

use crate::prelude::*;
use abscissa_core::*;
use chrono::{DateTime, Utc};
use clap::Parser;

/// `ls` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Parser)]
pub struct Log {
    stash: String,
}

impl Runnable for Log {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.open_stash(&self.stash);
            stash.load_all().unwrap();

            for commit in stash.commit_list().iter() {
                let time: DateTime<Utc> = commit.metadata.time.into();
                let local_time = time.with_timezone(&chrono::Local);
                let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();

                println!(
                    "{:?}\t{}\t{}",
                    commit.id,
                    formatted_time,
                    commit
                        .metadata
                        .message
                        .as_ref()
                        .unwrap_or(&"No commit message".to_string())
                );
            }
        })
        .unwrap();
    }
}
