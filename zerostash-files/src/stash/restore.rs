#![allow(unused)]

use crate::files::{self, FileSet};
use infinitree::{
    backends::Backend,
    object::{self, WriteObject},
};

use flume as mpsc;
use futures::future::join_all;
use itertools::Itertools;
use memmap2::MmapOptions;
use tokio::task;

use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::UNIX_EPOCH,
};

type ThreadWork = (PathBuf, Arc<files::Entry>);

type Sender = mpsc::Sender<ThreadWork>;
type Receiver = mpsc::Receiver<ThreadWork>;

pub type FileIterator<'a> = Box<(dyn Iterator<Item = Arc<files::Entry>> + 'a)>;

pub async fn from_iter(
    max_file_handles: usize,
    iter: FileIterator<'_>,
    objreader: impl object::Reader + Clone + 'static,
    target: impl AsRef<Path>,
) {
    let (mut sender, receiver) = mpsc::bounded(2 * max_file_handles);

    let workers = (0..max_file_handles)
        .map(|_| task::spawn(process_packet_loop(receiver.clone(), objreader.clone())))
        .collect::<Vec<_>>();

    for md in iter {
        let path = get_path(&md.name);

        // if there's no parent, then the entire thing is root.
        // if what we're trying to extract is root, then what happens?
        let mut basedir = target.as_ref().to_owned();
        if let Some(parent) = path.parent() {
            // create the file and parent directory
            fs::create_dir_all(basedir.join(parent)).unwrap();
        }

        let filename = basedir.join(&path);

        if sender.send_async((filename, md.clone())).await.is_err() {
            println!("internal process crashed");
            return;
        }
    }

    drop(sender);
    join_all(workers).await;
}

async fn process_packet_loop(mut r: Receiver, mut objreader: impl object::Reader + 'static) {
    // Since resources here are all managed by RAII, and they all
    // implement Drop, we can simply go through the Arc<_>s,
    // mmap them, open the corresponding objects to extract details,
    // and everything will be cleaned up on Drop.
    //
    // In fact, every layer of these for loops is also managing a
    // corresponding resource.
    let mut buffer = WriteObject::default();

    // This loop is managing an mmap of a file that's written
    while let Ok((filename, metadata)) = r.recv_async().await {
        if metadata.size == 0 {
            continue;
        }
        let fd = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(filename)
            .unwrap();
        fd.set_len(metadata.size).unwrap();

        let object_ordered = metadata
            .chunks
            .iter()
            .cloned()
            .fold(HashMap::new(), |mut a, c| {
                a.entry(*c.1.object_id()).or_insert_with(Vec::new).push(c);
                a
            });

        let mut mmap = unsafe {
            MmapOptions::new()
                .len(metadata.size as usize)
                .map_mut(&fd)
                .expect("mmap")
        };

        // This loop manages the object we're reading from
        for (objectid, cs) in object_ordered.into_iter() {
            // This loop will extract & decrypt & decompress from the object
            for (i, (start, cp)) in cs.into_iter().enumerate() {
                let start = start as usize;
                objreader.read_chunk(cp, &mut mmap[start..]).unwrap();
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
