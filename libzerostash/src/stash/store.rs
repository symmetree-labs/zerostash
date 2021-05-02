use crate::files::{self, FileStore};
use crate::object;
use crate::rollsum::SeaSplit;
use crate::splitter::FileSplitter;
use crate::{chunks::ChunkStore, object::write_balancer::RoundRobinBalancer};

use flume as mpsc;
use futures::future::join_all;
use ignore::{DirEntry, WalkBuilder};
use memmap2::MmapOptions;
use tokio::{fs, task};

use std::path::Path;

type Sender = mpsc::Sender<DirEntry>;
type Receiver = mpsc::Receiver<DirEntry>;

#[allow(unused)]
pub async fn recursive(
    worker_count: usize,
    chunkindex: &mut ChunkStore,
    fileindex: &mut FileStore,
    objectstore: impl object::Writer + 'static,
    path: impl AsRef<Path>,
) {
    // make sure the input and output queues are generous
    let (mut sender, receiver) = mpsc::bounded(worker_count * 2);
    let balancer = RoundRobinBalancer::new(objectstore, worker_count * 2).unwrap();

    let workers = (0..worker_count)
        .map(|_| {
            task::spawn(process_file_loop(
                receiver.clone(),
                chunkindex.clone(),
                fileindex.clone(),
                balancer.clone(),
            ))
        })
        .collect::<Vec<_>>();

    // it's probably not a good idea to have walker threads compete
    // with workers, so we don't need to scale this up so aggressively
    walk_path(worker_count / 2, sender, path);

    join_all(workers).await;

    balancer.flush().unwrap();
}

async fn process_file_loop(
    r: Receiver,
    chunkindex: ChunkStore,
    mut fileindex: FileStore,
    writer: RoundRobinBalancer<impl object::Writer + 'static>,
) {
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

        let osfile = osfile.unwrap();
        let metadata = osfile.metadata().await.unwrap();
        let mut entry = files::Entry::from_metadata(metadata, path).unwrap();

        if !fileindex.has_changed(&entry) {
            continue;
        }

        if entry.size == 0 {
            fileindex.push(entry);
            continue;
        }

        let mmap = unsafe {
            // avoid an unnecessary fstat() by passing `len`
            // directly from the previous call
            MmapOptions::new()
                .len(entry.size as usize)
                .populate()
                .map(&osfile.into_std().await)
                .unwrap()
        };

        let splitter = FileSplitter::<SeaSplit>::new(&mmap);
        let chunks = splitter.map(|(start, hash, data)| {
            let writer = writer.clone();
            let chunkindex = chunkindex.clone();
            let data = data.to_vec();

            task::spawn_blocking(move || {
                let store = || writer.write(&hash, &data);
                (start, chunkindex.push(hash, store).unwrap())
            })
        });

        entry
            .chunks
            .extend(join_all(chunks).await.into_iter().map(Result::unwrap));

        fileindex.push(entry);
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
        use crate::chunks::*;
        use crate::files::*;
        use crate::object::test::*;
        use crate::stash::store;

        let mut cs = ChunkStore::default();
        let mut fs = FileStore::default();
        let mut s = NullStorage::default();

        store::recursive(2, &mut cs, &mut fs, s, PATH_100).await;

        assert_eq!(100, fs.index().len());
        assert_eq!(
            1_024_000u64,
            fs.index().iter().map(|f| f.key().size).sum::<u64>()
        );
    }
}
