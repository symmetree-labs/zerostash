#![allow(unused)]

use crate::backends::Backend;
use crate::chunks::ChunkPointer;
use crate::compress;
use crate::crypto::CryptoProvider;
use crate::files::{self, FileIndex};
use crate::objects::*;

use crossbeam_utils::thread;
use failure::Error;
use itertools::Itertools;
use memmap::MmapOptions;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

type ThreadWork = (PathBuf, Arc<files::Entry>);

type Sender = crossbeam_channel::Sender<ThreadWork>;
type Receiver = crossbeam_channel::Receiver<ThreadWork>;

pub fn from_glob(
    pattern: &str,
    num_threads: usize,
    fileindex: &FileIndex,
    backend: &(impl Backend),
    crypto: impl CryptoProvider,
    target: impl AsRef<Path>,
) -> Result<(), Error> {
    let matcher = glob::Pattern::new(pattern)?;
    thread::scope(move |s| {
        // need to set up threads here and stuff
        let (sender, receiver) = crossbeam_channel::bounded::<ThreadWork>(2 * num_threads);

        for range in 0..(num_threads - 1) {
            let backend = backend.clone();
            let crypto = crypto.clone();
            let receiver = receiver.clone();

            s.spawn(move |_| process_packet_loop(receiver, backend, crypto));
        }

        for f in fileindex.into_iter() {
            let md = f.key();
            if !matcher.matches_with(&md.name, glob::MatchOptions::new()) {
                continue;
            }

            let path = get_path(&md.name);

            // if there's no parent, then the entire thing is root.
            // if what we're trying to extract is root, then what happens?
            let mut basedir = target.as_ref().to_owned();
            if let Some(parent) = path.parent() {
                // create the file and parent directory
                fs::create_dir_all(basedir.join(parent)).unwrap();
            }

            let filename = basedir.join(&path);

            sender.send((filename, md.clone()));
        }
    })
    .map(|_| ())
    .map_err(|e| format_err!("threads: {:?}", e))
}

fn process_packet_loop(r: Receiver, backend: impl Backend, crypto: impl CryptoProvider) {
    // Since resources here are all managed by RAII, and they all
    // implement Drop, we can simply go through the Arc<_>s,
    // mmap them, open the corresponding objects to extract details,
    // and everything will be cleaned up on Drop.
    //
    // In fact, every layer of these for loops is also managing a
    // corresponding resource.
    let mut buffer = WriteObject::default();

    // This loop is managing an mmap of a file that's written
    for (filename, metadata) in r.iter() {
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

        let object_ordered = metadata.chunks.iter().fold(HashMap::new(), |mut a, c| {
            a.entry(c.1.file).or_insert_with(Vec::new).push(c);
            a
        });

        let mut mmap = unsafe {
            MmapOptions::new()
                .len(metadata.size as usize)
                .map_mut(&fd)
                .expect("mmap")
        };

        // This loop manages the object we're reading from
        for (objectid, cs) in object_ordered.iter() {
            let object = backend.read_object(objectid).expect("object read");

            // This loop will extract & decrypt & decompress from the object
            for (i, (start, cp)) in cs.iter().enumerate() {
                let start = *start as usize;
                let mut target: &mut [u8] = buffer.buffer.as_mut();

                let len = crypto.decrypt_chunk(&mut target, &object, cp);
                compress::decompress_into(&mut mmap[start..], &target[..len]).unwrap();
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
