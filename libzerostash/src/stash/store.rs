use crate::chunks::ChunkStore;
use crate::files::{self, FileStore};
use crate::objects::ObjectStore;
use crate::rollsum::SeaSplit;
use crate::splitter::FileSplitter;

use ignore::{DirEntry, WalkBuilder};
use memmap2::MmapOptions;
use tokio::{fs, sync::mpsc, task};

use std::path::Path;

type Sender = mpsc::Sender<DirEntry>;
type Receiver = mpsc::Receiver<DirEntry>;

#[allow(unused)]
pub async fn recursive(
    max_file_handles: usize,
    chunkindex: &mut ChunkStore,
    fileindex: &mut FileStore,
    objectstore: &mut (impl ObjectStore + 'static),
    path: impl AsRef<Path>,
) {
    let (mut sender, receiver) = mpsc::channel(max_file_handles);

    let handle = task::spawn(process_file_loop(
        receiver,
        chunkindex.clone(),
        fileindex.clone(),
        objectstore.clone(),
    ));

    process_path(0, sender, path);

    handle.await;
}

async fn process_file_loop(
    mut r: Receiver,
    chunkindex: ChunkStore,
    mut fileindex: FileStore,
    mut objectstore: impl ObjectStore,
) {
    while let Some(file) = r.recv().await {
        let path = file.path();

        if file
            .path()
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            println!(
                "skipping because contains parent {:?}",
                path.to_string_lossy()
            );
            continue;
        }

        let osfile = fs::File::open(path).await;
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

        for (start, hash, data) in FileSplitter::<SeaSplit>::new(&mmap) {
            let chunkptr = {
                let store_fn = objectstore.store_chunk(&hash, data);
                chunkindex.push(hash, store_fn).await.unwrap()
            };

            entry.chunks.push((start, chunkptr));
        }

        fileindex.push(entry);
    }

    objectstore.flush().await.unwrap();
}

/// if `threads == 0`, it chooses the number of threads automatically using heuristics
fn process_path(threads: usize, sender: Sender, path: impl AsRef<Path>) {
    let walker = WalkBuilder::new(path)
        .threads(threads)
        .standard_filters(false)
        .build();

    for result in walker {
        if result.is_err() {
            continue;
        }

        let entry = result.unwrap();
        if !entry.path().is_file() {
            continue;
        }

        let tx = sender.clone();
        task::spawn(async move {
            tx.send(entry).await.unwrap();
        });
    }
    // walker.run(|| {
    //     let tx = sender.clone();
    //     Box::new(move |result| {
    //     })
    // });
}

#[cfg(test)]
mod tests {
    const PATH_100: &str = "tests/data/100_random_1k";

    #[test]
    fn test_stats_add_up() {
        use crate::chunks::*;
        use crate::files::*;
        use crate::objects::*;
        use crate::stash::store;

        let mut cs = ChunkStore::default();
        let mut fs = FileStore::default();
        let mut s = NullStorage::default();

        store::recursive(4, &mut cs, &mut fs, &mut s, PATH_100);

        assert_eq!(100, fs.index().len());
        assert_eq!(
            1_024_000u64,
            fs.index().iter().map(|f| f.key().size).sum::<u64>()
        );
    }
}
