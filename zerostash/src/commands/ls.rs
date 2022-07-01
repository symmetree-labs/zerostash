//! `ls` subcommand

use crate::prelude::*;
use abscissa_core::terminal::{stderr, stdout};
use chrono::{DateTime, Utc};
use humansize::{file_size_opts, FileSize};
use std::{io::Write, sync::Arc};
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};
use zerostash_files::*;

#[derive(Command, Debug)]
pub struct Ls {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(short = 'l', long)]
    list: bool,

    #[clap(short = 'H', long)]
    human_readable: bool,

    #[clap(flatten)]
    options: zerostash_files::restore::Options,
}

#[async_trait]
impl AsyncRunnable for Ls {
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

impl Ls {
    fn print_simple(&self) -> Box<dyn Fn(Arc<Entry>)> {
        Box::new(|entry: Arc<Entry>| println!("{}", entry.name))
    }

    fn print_list(&self) -> Box<dyn Fn(Arc<Entry>)> {
        let human_readable = self.human_readable;
        let stdout = stdout();

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

            print(stdout, ColorSpec::new(), mode);
            print(stdout, ColorSpec::new(), owner);
            print(stdout, ColorSpec::new(), group);
            print(stdout, ColorSpec::new(), size);
            print(stdout, ColorSpec::new(), formatted_time);
            print(
                stdout,
                file_color,
                if let FileType::Symlink(ref target) = entry.file_type {
                    format!("{} -> {:?}", entry.name, target)
                } else {
                    entry.name.clone()
                },
            );
            writeln!(stdout.lock()).unwrap();
        })
    }
}

#[cfg(unix)]
fn get_uid(uid: Option<u32>) -> String {
    use nix::unistd::{Uid, User};
    match uid {
        Some(uid) => User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
            .unwrap_or_else(|| format!("{}", uid)),
        None => "---".to_string(),
    }
}

#[cfg(unix)]
fn get_gid(gid: Option<u32>) -> String {
    use nix::unistd::{Gid, Group};
    match gid {
        Some(gid) => Group::from_gid(Gid::from_raw(gid))
            .ok()
            .flatten()
            .map(|u| u.name)
            .unwrap_or_else(|| format!("{}", gid)),
        None => "---".to_string(),
    }
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
