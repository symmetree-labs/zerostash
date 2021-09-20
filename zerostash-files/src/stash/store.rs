use crate::{files, rollsum::SeaSplit, splitter::FileSplitter};
use infinitree::object::{self, write_balancer::RoundRobinBalancer, Writer};

use flume as mpsc;
use futures::future::join_all;
use ignore::{DirEntry, WalkBuilder};
use memmap2::{Mmap, MmapOptions};
use tokio::{fs, io::AsyncReadExt, task};

use std::path::Path;

type Sender = mpsc::Sender<DirEntry>;
type Receiver = mpsc::Receiver<DirEntry>;

const MAX_FILE_SIZE: usize = 16 * 1024 * 1024;

#[allow(unused)]
pub async fn recursive(
    worker_count: usize,
    index: &crate::Files,
    objectstore: impl object::Writer + Clone + 'static,
    path: impl AsRef<Path>,
) {
    // make sure the input and output queues are generous
    let (mut sender, receiver) = mpsc::bounded(worker_count * 2);
    let mut balancer = RoundRobinBalancer::new(objectstore, worker_count).unwrap();

    let workers = (0..worker_count)
        .map(|_| {
            task::spawn(process_file_loop(
                receiver.clone(),
                index.clone(),
                balancer.clone(),
            ))
        })
        .collect::<Vec<_>>();

    // it's probably not a good idea to have walker threads compete
    // with workers, so we don't need to scale this up so aggressively
    walk_path(worker_count / 4, sender, path);

    join_all(workers).await;

    balancer.flush().unwrap();
}

async fn process_file_loop(
    r: Receiver,
    index: crate::Files,
    writer: RoundRobinBalancer<impl object::Writer + Clone + 'static>,
) {
    let fileindex = &index.files;
    let chunkindex = &index.chunks;
    let mut buf = Vec::with_capacity(MAX_FILE_SIZE);

    while let Ok(file) = r.recv_async().await {
        let path = file.path().to_owned();

        if path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            println!(
                "skipping because contains parent {:?}",
                path.to_string_lossy()
            );
            continue;
        }

        let osfile = fs::File::open(&path).await;
        if osfile.is_err() {
            println!("skipping {}: {}", path.display(), osfile.unwrap_err());
            continue;
        }

        let mut osfile = osfile.unwrap();
        let metadata = osfile.metadata().await.unwrap();
        let mut entry = files::Entry::from_metadata(metadata, path.clone()).unwrap();

        if let Some(in_store) = fileindex.get(&path) {
            if in_store.as_ref() == &entry {
                continue;
            }
        }

        if entry.size == 0 {
            fileindex.insert(path, entry);
            continue;
        }

        if entry.size < MAX_FILE_SIZE as u64 {
            osfile.read_to_end(&mut buf).await.unwrap();
        }

        let size = entry.size as usize;
        let mut mmap = MmappedFile::new(size, osfile.into_std().await);

        let splitter = if size < MAX_FILE_SIZE {
            FileSplitter::<SeaSplit>::new(&buf[0..size])
        } else {
            FileSplitter::<SeaSplit>::new(mmap.open())
        };
        let chunks = splitter.map(|(start, hash, data)| {
            let mut writer = writer.clone();
            let chunkindex = chunkindex.clone();
            let data = data.to_vec();

            task::spawn_blocking(move || {
                let store = || writer.write_chunk(&hash, &data).unwrap();
                let ptr = chunkindex.insert_with(hash, store);
                (start, ptr)
            })
        });

        entry
            .chunks
            .extend(join_all(chunks).await.into_iter().map(Result::unwrap));

        fileindex.insert(path, entry);
    }
}

struct MmappedFile {
    mmap: Option<Mmap>,
    len: usize,
    _file: std::fs::File,
}

impl MmappedFile {
    fn new(len: usize, _file: std::fs::File) -> Self {
        Self {
            mmap: None,
            len,
            _file,
        }
    }

    fn open(&mut self) -> &[u8] {
        self.mmap.get_or_insert(unsafe {
            MmapOptions::new()
                .len(self.len)
                .populate()
                .map(&self._file)
                .unwrap()
        })
    }
}

/// if `threads == 0`, it chooses the number of threads automatically using heuristics
fn walk_path(threads: usize, sender: Sender, path: impl AsRef<Path>) {
    let walker = WalkBuilder::new(path)
        .threads(threads)
        .standard_filters(false)
        .build_parallel();

    walker.run(|| {
        let tx = sender.clone();
        Box::new(move |result| {
            use ignore::WalkState::*;

            if result.is_err() {
                return Continue;
            }

            let entry = result.unwrap();
            if !entry.path().is_file() {
                return Continue;
            }

            tx.send(entry).unwrap();

            Continue
        })
    });

    println!("all paths done");
}

#[cfg(test)]
mod tests {
    const PATH_100: &str = "tests/data/100_random_1k";

    // need a multi-threaded scheduler for anything involving
    // `store::recursive`
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_stats_add_up() {
        use crate::stash::store;
        use crate::*;
        use libzerostash::object::test::*;

        let mut index = Files::default();
        let s = NullStorage::default();

        std::env::set_current_dir("..").unwrap();
        store::recursive(2, &mut index, s, PATH_100).await;

        assert_eq!(100, index.files.len());
        assert_eq!(
            1_024_000u64,
            index.files.iter().map(|f| f.key().size).sum::<u64>()
        );
    }
}
