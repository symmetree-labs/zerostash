//! `zfs ls` subcommand

use crate::prelude::*;
use abscissa_core::terminal::stderr;
use chrono::{DateTime, Utc};
use std::io::Write;

#[derive(Command, Debug)]
pub struct ZfsLs {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(short = 'l', long)]
    list: bool,

    #[clap(flatten)]
    options: zerostash_files::list::List,
}

#[async_trait]
impl AsyncRunnable for ZfsLs {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        let count = self
            .options
            .list(&stash)
            .map(match self.list {
                false => self.print_simple(),
                true => self.print_list(),
            })
            .count();

        writeln!(stderr().lock(), "Total entries: {}", count).unwrap();
    }
}

impl ZfsLs {
    fn print_simple(&self) -> Box<dyn Fn((String, DateTime<Utc>))> {
        writeln!(stderr().lock(), "NAME").unwrap();
        Box::new(|(name, _)| println!("{}", name))
    }

    fn print_list(&self) -> Box<dyn Fn((String, DateTime<Utc>))> {
        writeln!(stderr().lock(), "{:<25} TIME", "NAME").unwrap();
        Box::new(|(name, time)| {
            let local_time = time.with_timezone(&chrono::Local);
            let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();
            println!("{:<25} {}", name, formatted_time);
        })
    }
}
