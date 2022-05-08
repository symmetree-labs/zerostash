#![allow(unused)]

use crate::{files, FileSet, Files};
use flume as mpsc;
use futures::future::join_all;
use infinitree::{
    backends::Backend,
    object::{self, WriteObject},
    Infinitree,
};
use itertools::Itertools;
use memmap2::MmapOptions;
use std::{
    collections::{HashMap, HashSet},
    env,
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::{fs, task};
use tracing::{error, trace};

type ThreadWork = (PathBuf, Arc<files::Entry>);

type Sender = mpsc::Sender<ThreadWork>;
type Receiver = mpsc::Receiver<ThreadWork>;

pub type FileIterator<'a> = Box<(dyn Iterator<Item = Arc<files::Entry>> + 'a)>;

#[derive(clap::Args, Debug, Clone, Default)]
pub struct Options {
    /// List of globs to match in the database
    pub globs: Vec<String>,

    #[clap(flatten)]
    pub preserve: files::PreserveMetadata,

    /// Ignore errors
    #[clap(short = 'f', long)]
    pub force: bool,

    /// Ignore files larger than the given value in bytes.
    #[clap(short = 'M', long = "max-size")]
    pub max_size: Option<u64>,

    /// Ignore files smaller than the given value in bytes.
    #[clap(short = 'm', long = "min-size")]
    pub min_size: Option<u64>,

    /// Change directory before restore operation.
    #[clap(short = 'c', long = "chdir")]
    pub chdir: Option<PathBuf>,

    /// Call chroot(PATH) before restore operation. It is executed before --chdir if specified.
    /// Note that the source needs to be inside the chroot, or on the network!
    #[cfg(target_family = "unix")]
    #[clap(short = 'C', long = "chroot")]
    pub chroot: Option<PathBuf>,
}

impl Options {
    pub fn list(&self, stash: &Infinitree<Files>) {
        let index = stash.index();
        let globs = if !self.globs.is_empty() {
            self.globs.clone()
        } else {
            vec!["*".into()]
        };
        let iter = index.list(stash, &globs);

        for md in iter {
            if let Some(max) = self.max_size {
                if max > md.size {
                    continue;
                }
            }

            if let Some(min) = self.min_size {
                if min < md.size {
                    continue;
                }
            }

            println!("{}", md.name);
        }
    }

    pub async fn from_iter(
        &self,
        stash: &Infinitree<Files>,
        threads: usize,
    ) -> anyhow::Result<u64> {
        self.setup_env()?;
        let globs = if !self.globs.is_empty() {
            self.globs.clone()
        } else {
            vec!["*".into()]
        };

        let (sender, workers) = self.start_workers(stash, threads)?;
        let index = stash.index();
        let iter = index.list(stash, &globs);

        for md in iter {
            if let Some(max) = self.max_size {
                if max > md.size {
                    continue;
                }
            }

            if let Some(min) = self.min_size {
                if min < md.size {
                    continue;
                }
            }

            let path = get_path(&md.name);

            trace!(?path, "queued");
            sender.send_async((path, md)).await.unwrap();
        }

        drop(sender);
        join_all(workers).await;

        Ok(0)
    }

    #[cfg(unix)]
    fn setup_env(&self) -> anyhow::Result<()> {
        if let Some(ref path) = self.chroot {
            std::os::unix::fs::chroot(path).unwrap();
        }

        if let Some(ref path) = self.chdir {
            env::set_current_dir(path)?;
        }

        Ok(())
    }

    #[cfg(windows)]
    fn setup_env(&self) -> anyhow::Result<()> {
        if let Some(ref path) = self.chdir {
            env::set_current_dir(path)?;
        }

        Ok(())
    }

    fn start_workers(
        &self,
        stash: &Infinitree<Files>,
        threads: usize,
    ) -> anyhow::Result<(Sender, Vec<task::JoinHandle<()>>)> {
        let (mut sender, receiver) = mpsc::bounded(threads);
        let workers = (0..threads)
            .map(|_| {
                task::spawn(process_packet_loop(
                    self.force,
                    self.preserve.clone(),
                    receiver.clone(),
                    stash.object_reader().unwrap(),
                ))
            })
            .collect::<Vec<_>>();
        Ok((sender, workers))
    }
}

async fn process_packet_loop(
    force: bool,
    preserve: files::PreserveMetadata,
    mut r: Receiver,
    mut objreader: impl object::Reader + 'static,
) {
    // Since resources here are all managed by RAII, and they all
    // implement Drop, we can simply go through the Arc<_>s,
    // mmap them, open the corresponding objects to extract details,
    // and everything will be cleaned up on Drop.
    //
    // In fact, every layer of these for loops is also managing a
    // corresponding resource.
    let mut buffer = WriteObject::default();

    // This loop is managing an mmap of a file that's written
    while let Ok((path, metadata)) = r.recv_async().await {
        match metadata.restore_to(&path, &preserve) {
            Ok(Some(fd)) => {
                let mut mmap = unsafe {
                    MmapOptions::new()
                        .len(metadata.size as usize)
                        .map_mut(&fd)
                        .expect("mmap")
                };

                for (start, cp) in metadata.chunks.iter() {
                    let start = *start as usize;
                    objreader.read_chunk(cp, &mut mmap[start..]).unwrap();
                }

                trace!(?path, "restored");
            }
            Ok(None) => {
                trace!(?path, file_type = ?metadata.file_type, "no chunks restored for file");
            }
            Err(error) => {
                error!(%error, ?path, "failed to restore file");

                if !force {
                    panic!("error while restoring file");
                }
            }
        }
    }
}

fn get_path(filename: impl AsRef<Path>) -> PathBuf {
    let path = filename.as_ref();
    let mut cs = path.components();

    if let Some(std::path::Component::RootDir) = cs.next() {
        cs.as_path().to_owned()
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_removes_root() {
        use super::*;

        assert_eq!(Path::new("home/a/b"), get_path("/home/a/b").as_path());
        assert_eq!(Path::new("./a/b"), get_path("./a/b").as_path());
    }
}
