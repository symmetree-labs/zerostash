#![deny(clippy::all)]

use infinitree::{backends, Infinitree, Key};
use zerostash_files::{restore, store, Files};

use std::{collections::HashMap, env::args, fs::metadata, time::Instant};

const MAX_OBJECT_LRU: usize = 64;

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

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    use tracing_subscriber::FmtSubscriber;
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::WARN)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

    let threads = std::thread::available_parallelism().unwrap().get();
    let path = args().nth(1).unwrap();
    let output = args().nth(2).unwrap();
    let restore_to = args().nth(3).unwrap();

    let key = "abcdef1234567890abcdef1234567890";

    let _ = std::fs::remove_dir_all(&output);
    let _ = std::fs::remove_dir_all(&restore_to);

    let _ = std::fs::create_dir(&restore_to);

    let key = || Key::from_credentials(&key, &key).unwrap();
    // i am really, truly sorry for this. there must be a better way,
    // but i can't be bothered to find it
    let (store_time, commit_time, ol, fl, cl, creuse_sum, creuse_cnt, ssize, tlen, tsize) = {
        let mut repo = Infinitree::<Files>::empty(
            backends::Directory::with_open_file_limit(&output, MAX_OBJECT_LRU).unwrap(),
            (key)(),
        )
        .unwrap();

        let store_start = Instant::now();
        store::Options {
            paths: vec![path.clone().into()],
            ..Default::default()
        }
        .add_recursive(&repo, threads)
        .await
        .unwrap();
        let store_time = store_start.elapsed();

        let commit_start = Instant::now();
        repo.commit(None).unwrap();
        let commit_time = commit_start.elapsed();

        let ol = repo.index_object_count();
        let fl = repo.index().files.len();
        let cl = repo.index().chunks.len();
        let (creuse_sum, creuse_cnt) = {
            let mut chunk_reuse = HashMap::new();
            repo.index().files.for_each(|_, f| {
                f.chunks
                    .iter()
                    .for_each(|(_, c)| *chunk_reuse.entry(*c.hash()).or_insert(0u32) += 1)
            });

            (
                chunk_reuse.values().sum::<u32>() as f64,
                chunk_reuse.len() as f64,
            )
        };

        let ssize = {
            let mut data_size = 0.0f64;
            repo.index()
                .files
                .for_each(|_, f| data_size += f.size as f64);
            data_size
        };

        let (tsize, tlen) = dir_stat(&output);

        (
            store_time,
            commit_time,
            ol,
            fl,
            cl,
            creuse_sum,
            creuse_cnt,
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
 * chunk reuse: {}/{} = {}
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
        creuse_sum,
        creuse_cnt,
        creuse_sum / creuse_cnt
    );

    {
        let mut repo: Infinitree<Files> = Infinitree::open(
            backends::Directory::with_open_file_limit(&output, MAX_OBJECT_LRU).unwrap(),
            (key)(),
        )
        .unwrap();

        let read_start = Instant::now();
        repo.load_all().unwrap();
        let read_time = read_start.elapsed();
        println!("repo open: {}", read_time.as_secs_f64());

        let restore_start = Instant::now();
        restore::Options {
            globs: vec!["*".into()],
            chdir: Some(restore_to.into()),
            ..Default::default()
        }
        .from_iter(&repo, threads)
        .await
        .unwrap();
        let restore_time = restore_start.elapsed();

        let total_time = restore_time.as_secs_f64();

        println!(
            r#"restore time: {}
throughput packed: {}
throughput unpacked: {}
"#,
            restore_time.as_secs_f64(),
            mb(tsize as f64) / total_time,
            mb(ssize as f64) / total_time
        );
    }
}
