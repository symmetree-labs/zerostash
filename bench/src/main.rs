#![deny(clippy::all)]
#![feature(test)]

use libzerostash::stash::{Stash, StashKey};
use libzerostash::{backends, chunks::ChunkIndex, files::FileIndex, objects};

use std::collections::{HashMap, HashSet};
use std::env::args;
use std::fs::metadata;
use std::sync::Arc;
use std::time::Instant;

fn mb(m: f64) -> f64 {
    m / 1024.0 / 1024.0
}

fn dir_stat(path: &str) -> (u64, usize) {
    use walkdir::WalkDir;
    let lens = WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|f| metadata(f.path()).unwrap().len())
        .collect::<Vec<_>>();

    (lens.iter().sum::<u64>(), lens.len())
}

pub fn main() {
    let threads = args().nth(1).unwrap();
    let path = args().nth(2).unwrap();
    let output = args().nth(3).unwrap();
    let restore_to = args().nth(4).unwrap();

    let key = "abcdef1234567890abcdef1234567890";

    let (store_time, commit_time, ol, fl, cl, creuse, ssize, tlen, tsize) = {
        let key = StashKey::open_stash(&key, &key).unwrap();
        let mut repo = Stash::new(backends::Directory::new(&output), key);

        let store_start = Instant::now();
        repo.add_recursive(threads.parse().unwrap(), &path).unwrap();
        let store_time = store_start.elapsed();

        let commit_start = Instant::now();
        let mobjects = repo.commit().unwrap();
        let commit_time = commit_start.elapsed();

        let objects = mobjects
            .values()
            .flatten()
            .collect::<HashSet<&objects::ObjectId>>();

        let ol = objects.len();
        let fl = repo.file_index().len();
        let cl = repo.chunk_index().len();
        let creuse = {
            let mut chunk_reuse = HashMap::new();
            repo.file_index().for_each(|f| {
                f.chunks
                    .iter()
                    .for_each(|(_, c)| *chunk_reuse.entry(c.hash).or_insert(0u32) = c.size)
            });

            chunk_reuse.values().sum::<u32>() as f64 / chunk_reuse.len() as f64
        };

        let ssize = {
            let mut data_size = 0.0f64;
            repo.file_index().for_each(|f| data_size += f.size as f64);
            data_size
        };

        let (tsize, tlen) = dir_stat(&output);

        (
            store_time,
            commit_time,
            ol,
            fl,
            cl,
            creuse,
            ssize,
            tlen,
            tsize,
        )
    };

    let total_time = (store_time + commit_time).as_secs_f64();

    println!(
        r#"stats for path ({}), seconds: {}
 * files: {},
 * chunks: {},
 * data size: {}
 * throughput: {}
 * objects: {}
 * output size: {}
 * compression ratio: {}
 * meta dump time: {}
 * meta object count: {}
 * chunk reuse: {}
"#,
        // * storage for chunks: {}
        path,
        store_time.as_secs_f64(),
        fl,
        cl,
        mb(ssize),
        mb(ssize) / total_time,
        tlen,
        mb(tsize as f64),
        tsize as f64 / ssize,
        commit_time.as_secs_f64(),
        ol,
        creuse
    );

    {
        let key = StashKey::open_stash(&key, &key).unwrap();
        let mut repo = Stash::new(backends::Directory::new(&output), key);

        let read_start = Instant::now();
        repo.read().unwrap();
        let read_time = read_start.elapsed();

        let restore_start = Instant::now();
        repo.restore_by_glob(threads.parse().unwrap(), "*", restore_to)
            .unwrap();
        let restore_time = restore_start.elapsed();

        let total_time = (read_time + restore_time).as_secs_f64();

        println!(
            r#"read time: {}
restore time: {}
throughput packed: {}
throughput unpacked: {}
"#,
            read_time.as_secs_f64(),
            restore_time.as_secs_f64(),
            mb(tsize as f64) / total_time,
            mb(ssize as f64) / total_time
        );
    }
}
