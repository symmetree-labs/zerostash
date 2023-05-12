//! `zfs ls` subcommand

use crate::prelude::*;
use abscissa_core::terminal::{stderr, stdout};
use chrono::{DateTime, Utc};
use termcolor::StandardStreamLock;
use std::io::Write;

type Printer = Box<dyn Fn(&mut StandardStreamLock<'_>, (String, DateTime<Utc>)) -> std::io::Result<()>>;

#[derive(Command, Debug)]
pub struct ZfsLs {
    #[clap(flatten)]
    stash: StashArgs,

    /// Use detailed output
    #[clap(short = 'l', long)]
    list: bool,

    #[clap(flatten)]
    options: zerostash_files::ZfsSnapshotList,
}

#[async_trait]
impl AsyncRunnable for ZfsLs {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        let printer = match self.list {
            false => self.print_simple(),
            true => self.print_list(),
        };

        let mut stdout = stdout().lock();
        let mut count = 0;
        for item in self.options.list(&stash) {
            count += 1;

            if printer(&mut stdout, item).is_err() {
                return;
            }
        }

        writeln!(stderr().lock(), "Total entries: {}", count).unwrap();
    }
}

impl ZfsLs {
    fn print_simple(&self) -> Printer {
        writeln!(stderr().lock(), "NAME").unwrap();
        Box::new(|stdout, (name, _)| writeln!(stdout, "{}", name))
    }

    fn print_list(&self) -> Printer {
        writeln!(stderr().lock(), "{:<25} TIME", "NAME").unwrap();
        Box::new(|stdout, (name, time)| {
            let local_time = time.with_timezone(&chrono::Local);
            let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();
            writeln!(stdout, "{:<25} {}", name, formatted_time)
        })
    }
}
