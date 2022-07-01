use criterion::{criterion_group, criterion_main, Criterion};
use infinitree::{backends::test::*, keys::UsernamePassword, Infinitree};
use memmap2::MmapOptions;
use std::fs::File;
use std::sync::Arc;
use zerostash_files::{rollsum::*, splitter::FileSplitter, Files};

criterion_group!(
    chunking,
    chunk_saturated_e2e,
    chunk_e2e,
    bup_rollsum,
    split_seasplit,
    split_bupsplit
);
criterion_main! {chunking}

const PATH: &str = "tests/data/10k_random_blob";
const PATH_100: &str = "tests/data/100_random_1k";
const SELFTEST_SIZE: usize = 100_000;

fn set_test_cwd() {
    std::env::set_current_dir(format!(
        "{}/..",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    ))
    .unwrap();
}

fn rollsum_sum(buf: &[u8], ofs: usize, len: usize) -> u32 {
    let mut r = BupSplit::new();
    for count in ofs..len {
        r.roll(buf[count]);
    }
    r.digest()
}

fn bup_rollsum(c: &mut Criterion) {
    c.bench_function("bup rollsum", |b| {
        let mut buf = [0; SELFTEST_SIZE];
        getrandom::getrandom(&mut buf).unwrap();

        b.iter(|| {
            rollsum_sum(&buf, 0, SELFTEST_SIZE);
        });
    });
}

fn chunk_saturated_e2e(c: &mut Criterion) {
    c.bench_function("end-to-end chunking, saturated chunks list", |b| {
        let key = "abcdef1234567890abcdef1234567890".to_string();
        let key =
            UsernamePassword::with_credentials(key.clone().into(), key.clone().into()).unwrap();
        let repo = Infinitree::<Files>::empty(Arc::new(NullBackend::default()), key).unwrap();

        let basic_rt = tokio::runtime::Runtime::new().unwrap();
        let options = zerostash_files::store::Options {
            paths: vec![PATH_100.into()],
            ..Default::default()
        };

        set_test_cwd();
        // first build up the file index
        basic_rt.block_on(options.add_recursive(&repo, 4)).unwrap();

        b.iter(|| {
            basic_rt.block_on(options.add_recursive(&repo, 4)).unwrap();
        })
    });
}

fn chunk_e2e(c: &mut Criterion) {
    c.bench_function("end-to-end chunking", |b| {
        let key = "abcdef1234567890abcdef1234567890".to_string();
        let key =
            UsernamePassword::with_credentials(key.clone().into(), key.clone().into()).unwrap();
        let repo = Infinitree::<Files>::empty(Arc::new(NullBackend::default()), key).unwrap();
        let options = zerostash_files::store::Options {
            paths: vec![PATH_100.into()],
            ..Default::default()
        };

        let basic_rt = tokio::runtime::Runtime::new().unwrap();

        set_test_cwd();

        b.iter(|| {
            basic_rt.block_on(options.add_recursive(&repo, 4)).unwrap();
        })
    });
}

fn split_seasplit(c: &mut Criterion) {
    c.bench_function("chunking with seasplit", |b| {
        set_test_cwd();
        let file = File::open(PATH).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        let hasher = infinitree::Hasher::new();

        b.iter(|| {
            FileSplitter::<SeaSplit>::new(&mmap, hasher.clone())
                .map(|(_, _, c)| c.len())
                .sum::<usize>()
        });
    });
}

fn split_bupsplit(c: &mut Criterion) {
    c.bench_function("chunking with bupsplit", |b| {
        set_test_cwd();
        let file = File::open(PATH).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        let hasher = infinitree::Hasher::new();

        b.iter(|| {
            FileSplitter::<BupSplit>::new(&mmap, hasher.clone())
                .map(|(_, _, c)| c.len())
                .sum::<usize>()
        });
    });
}
