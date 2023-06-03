use criterion::{criterion_group, criterion_main, Criterion};
use infinitree::{backends::test::*, crypto::UsernamePassword, Infinitree};
use memmap2::MmapOptions;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use zerostash_files::{rollsum::*, splitter::FileSplitter, Entry, Files, Tree};

criterion_group!(
    chunking,
    chunk_saturated_e2e,
    chunk_e2e,
    bup_rollsum,
    split_seasplit,
    split_bupsplit,
    tree_get,
    tree_fill,
    tree_insert_file,
    tree_insert_new,
    tree_rename,
    tree_remove,
    tree_rename_node,
);
criterion_main! {chunking}

const PATH: &str = "tests/data/10k_random_blob";
const PATH_100: &str = "tests/data/100_random_1k";
const SELFTEST_SIZE: usize = 100_000;

fn fill_tree(tree: &mut Tree, branches: usize, depth: usize, files: usize) {
    for branch in 1..=branches {
        let mut path = PathBuf::from(format!("{branch}"));
        for level in 1..=depth {
            path = path.join(format!("{level}"));
            _ = tree.insert_directory(path.to_str().unwrap());
            for file in 1..=files {
                let name = format!("{file}.txt");
                let entry = Entry {
                    name: name.clone(),
                    ..Entry::default()
                };
                _ = tree.insert_file(&format!("{}/{name}", path.to_str().unwrap()), entry);
            }
        }
    }
}

fn get_path(base: &str, depth: usize) -> String {
    let mut base = PathBuf::from(base);
    for level in 1..=depth {
        base = base.join(format!("{level}"));
    }
    base.to_str().unwrap().to_string()
}

fn tree_insert_new(c: &mut Criterion) {
    let mut tree = Tree::default();
    let path = format!("{}/1.txt", get_path("1", 10_000));

    let entry = Entry {
        name: "1.txt".to_string(),
        ..Default::default()
    };

    c.bench_function("tree insert new 10_000", |b| {
        b.iter(|| {
            _ = tree.insert_file(&path, entry.clone());
            tree = Tree::default();
        });
    });
}

fn tree_rename_node(c: &mut Criterion) {
    let mut tree = Tree::default();
    fill_tree(&mut tree, 1, 1_000, 5);

    c.bench_function("tree rename node", |b| {
        b.iter(|| {
            _ = tree.move_node("1/1/2", "1/1/renamed");
            _ = tree.move_node("1/1/renamed", "1/1/2");
        });
    });
}

fn tree_insert_file(c: &mut Criterion) {
    let mut tree = Tree::default();
    fill_tree(&mut tree, 50, 1_000, 0);
    let path = format!("{}/1.txt", get_path("1", 1_000));
    let entry = Entry {
        name: "1.txt".to_string(),
        ..Default::default()
    };
    c.bench_function("tree insert 50,1000,0", |b| {
        b.iter(|| tree.insert_file(&path, entry.clone()))
    });
}

fn tree_fill(c: &mut Criterion) {
    let mut tree = Tree::default();
    let mut group = c.benchmark_group("tree build");
    group.significance_level(0.05).sample_size(10);
    group.bench_function("tree build 500,100,20", |b| {
        b.iter(|| fill_tree(&mut tree, 500, 100, 20))
    });
    group.bench_function("tree build 1,100,2000", |b| {
        b.iter(|| fill_tree(&mut tree, 1, 100, 2_000))
    });
    group.finish();
}

fn tree_rename(c: &mut Criterion) {
    let tree = Tree::default();
    let path = format!("{}/1.txt", get_path("1", 1_000));
    let entry = Entry {
        name: "1.txt".to_string(),
        ..Default::default()
    };
    _ = tree.insert_file(&path, entry);
    c.bench_function("tree rename file depth 1_000", |b| {
        b.iter(|| tree.move_node(&path, "2.txt"))
    });
}

fn tree_get(c: &mut Criterion) {
    let mut tree = Tree::default();
    fill_tree(&mut tree, 50, 1_000, 10);
    let path = get_path("1", 1_000);
    c.bench_function("tree get 50,1000,10", |b| b.iter(|| tree.get(&path)));
}

fn tree_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree remove");
    group.significance_level(0.05).sample_size(10);

    let mut tree = Tree::default();

    let path = get_path("root", 0);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 1", |b| b.iter(|| tree.clone().remove("/root")));

    tree = Tree::default();
    let path = get_path("root", 10);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 10", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });

    tree = Tree::default();
    let path = get_path("root", 100);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 100", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });

    tree = Tree::default();
    let path = get_path("root", 1_000);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 1000", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });

    tree = Tree::default();
    let path = get_path("root", 5_000);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 5000", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });

    tree = Tree::default();
    let path = get_path("root", 10_000);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 10_000", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });

    tree = Tree::default();
    let path = get_path("root", 50_000);
    _ = tree.insert_directory(&path);

    group.bench_function("tree remove 50_000", |b| {
        b.iter(|| tree.clone().remove("/root"))
    });
}

fn set_test_cwd() {
    std::env::set_current_dir(format!(
        "{}/..",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    ))
    .unwrap();
}

fn rollsum_sum(buf: &[u8], ofs: usize, len: usize) -> u32 {
    let mut r = BupSplit::new();
    for b in buf.iter().skip(ofs).take(len) {
        r.roll(*b);
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
        let key = UsernamePassword::with_credentials(key.clone(), key).unwrap();
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
        let key = UsernamePassword::with_credentials(key.clone(), key).unwrap();
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
