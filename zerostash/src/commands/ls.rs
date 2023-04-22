//! `ls` subcommand

use crate::prelude::*;
use abscissa_core::terminal::{stderr, stdout};
use chrono::{DateTime, Utc};
use humansize::{format_size, BINARY};
use std::{io::Write, sync::Arc, writeln};
use termcolor::{Color, ColorSpec, StandardStreamLock, WriteColor};
use zerostash_files::*;

type Printer = Box<dyn Fn(&mut StandardStreamLock<'_>, String, Arc<Entry>) -> std::io::Result<()>>;

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
        stash.load(stash.index().tree()).unwrap();
        let printer = match self.list {
            false => self.print_simple(),
            true => self.print_list(),
        };

        let mut stdout = stdout().lock();
        let mut count = 0;
        for item in self.options.list(&stash) {
            let (path, entry) = (item.0, item.1);
            count += 1;

            if printer(&mut stdout, path, entry).is_err() {
                return;
            }
        }

        _ = writeln!(stderr().lock(), "Total entries: {count}");
    }
}

impl Ls {
    fn print_simple(&self) -> Printer {
        Box::new(|stdout, path, _| writeln!(stdout, "{}", path))
    }

    fn print_list(&self) -> Printer {
        let human_readable = self.human_readable;
        Box::new(move |stdout, path, entry| {
            let time: DateTime<Utc> = entry.as_ref().into();
            let local_time = time.with_timezone(&chrono::Local);
            let formatted_time = local_time.format("%Y %b %e %H:%M:%S").to_string();

            let size = if human_readable {
                format!("{:<8}", format_size(entry.size, BINARY))
            } else {
                format!("{:<8}", entry.size)
            };

            let mode = if let Some(mode) = entry.unix_perm {
                format!("{mode:o}")
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

            print(stdout, ColorSpec::new(), mode)?;
            print(stdout, ColorSpec::new(), owner)?;
            print(stdout, ColorSpec::new(), group)?;
            print(stdout, ColorSpec::new(), size)?;
            print(stdout, ColorSpec::new(), formatted_time)?;
            print(
                stdout,
                file_color,
                if let FileType::Symlink(ref target) = entry.file_type {
                    format!("{} -> {:?}", path, target)
                } else {
                    path
                },
            )?;

            writeln!(stdout)
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
            .unwrap_or_else(|| format!("{uid}")),
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
            .unwrap_or_else(|| format!("{gid}")),
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

fn print(
    s: &mut StandardStreamLock<'_>,
    color: ColorSpec,
    msg: impl AsRef<str>,
) -> std::io::Result<()> {
    s.reset()?;
    s.set_color(&color)?;
    write!(s, "{}\t", msg.as_ref())?;
    s.reset()?;

    Ok(())
}
