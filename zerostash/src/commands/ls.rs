//! `ls` subcommand

use crate::prelude::*;
use abscissa_core::{
    terminal::{stderr, stdout, StandardStream},
    *,
};
use chrono::{DateTime, Utc};
use clap::Parser;
use humansize::{file_size_opts, FileSize};
use std::{io::Write, sync::Arc};
use termcolor::{Color, ColorSpec, WriteColor};
use zerostash_files::*;

/// `ls` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Parser)]
pub struct Ls {
    stash: String,

    #[clap(short = 'l', long)]
    list: bool,

    #[clap(short = 'H', long)]
    human_readable: bool,

    #[clap(flatten)]
    options: zerostash_files::restore::Options,
}

impl Runnable for Ls {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.open_stash(&self.stash);

            stash.load_all().unwrap();
            let count = self
                .options
                .list(&stash)
                .map(match self.list {
                    false => self.print_simple(),
                    true => self.print_list(),
                })
                .count();

            writeln!(stderr().lock(), "Total entries: {}", count).unwrap();
        })
        .unwrap();
    }
}

impl Ls {
    fn print_simple(&self) -> Box<dyn Fn(Arc<Entry>)> {
        Box::new(|entry: Arc<Entry>| println!("{}", entry.name))
    }

    fn print_list(&self) -> Box<dyn Fn(Arc<Entry>)> {
        let human_readable = self.human_readable;

        Box::new(move |entry: Arc<Entry>| {
            let time: DateTime<Utc> = entry.as_ref().into();
            let local_time = time.with_timezone(&chrono::Local);
            let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();

            let size = if human_readable {
                format!(
                    "{:<8}",
                    entry.size.file_size(file_size_opts::BINARY).unwrap()
                )
            } else {
                format!("{:<8}", entry.size)
            };

            let mode = if let Some(mode) = entry.unix_perm {
                format!("{:o}", mode)
            } else if entry.readonly.unwrap_or_default() {
                "ro".into()
            } else {
                "rw".into()
            };

            let owner = get_uid(entry.unix_uid);
            let group = get_gid(entry.unix_gid);

            let file_color = match entry.file_type {
                FileType::File => ColorSpec::new(),
                FileType::Directory => ColorSpec::new()
                    .set_fg(Some(Color::Red))
                    .set_bold(true)
                    .clone(),
                FileType::Symlink(_) => ColorSpec::new()
                    .set_fg(Some(Color::Blue))
                    .set_bold(true)
                    .clone(),
            };

            print(stdout(), ColorSpec::new(), mode);
            print(stdout(), ColorSpec::new(), owner);
            print(stdout(), ColorSpec::new(), group);
            print(stdout(), ColorSpec::new(), size);
            print(stdout(), ColorSpec::new(), formatted_time);
            print(
                stdout(),
                file_color,
                if let FileType::Symlink(ref target) = entry.file_type {
                    format!("{} -> {:?}", entry.name, target)
                } else {
                    entry.name.clone()
                },
            );
            writeln!(stdout().lock()).unwrap();
        })
    }
}

#[cfg(unix)]
fn get_uid(uid: Option<u32>) -> String {
    use nix::unistd::{Uid, User};
    uid.and_then(|uid| User::from_uid(Uid::from_raw(uid)).ok())
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| "---".to_string())
}

#[cfg(unix)]
fn get_gid(gid: Option<u32>) -> String {
    use nix::unistd::{Gid, Group};
    gid.and_then(|gid| Group::from_gid(Gid::from_raw(gid)).ok())
        .flatten()
        .map(|g| g.name)
        .unwrap_or_else(|| "---".to_string())
}

#[cfg(windows)]
fn get_uid(uid: Option<u32>) -> String {
    uid.map(|id| format!("{}", id)).unwrap_or("---".to_string())
}

#[cfg(windows)]
fn get_gid(gid: Option<u32>) -> String {
    gid.map(|id| format!("{}", id)).unwrap_or("---".to_string())
}

fn print(channel: &StandardStream, color: ColorSpec, msg: impl AsRef<str>) {
    let mut s = channel.lock();
    s.reset().unwrap();
    s.set_color(&color).unwrap();
    write!(s, "{}\t", msg.as_ref()).unwrap();
    s.reset().unwrap();
}
