use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
};

use criterion::{criterion_group, criterion_main, Criterion};
use fuse_mt::FuseMT;
use infinitree::{backends, crypto::UsernamePassword, Infinitree};
use zerostash_fuse::mount::ZerostashFS;

criterion_group!(benches, mount_starup);
criterion_main! {benches}

fn mount_starup(c: &mut Criterion) {
    c.bench_function("mount startup", |b| b.iter(mount));
}

fn mount() -> anyhow::Result<()> {
    let (tx, finished) = mpsc::sync_channel(2);

    let key = "abcde".to_string();
    let key = UsernamePassword::with_credentials(key.clone(), key).unwrap();

    let backend = backends::Directory::new(PathBuf::from("../tests/data/Mounting/Stash/")).unwrap();
    let stash = Infinitree::open(backend, key).unwrap();
    let fuse_args = [OsStr::new("-o"), OsStr::new("fsname=zerostash")];
    let options = zerostash_files::restore::Options::default();
    let filesystem = ZerostashFS::open(Arc::new(Mutex::new(stash)), &options, tx, 0).unwrap();
    let fs = FuseMT::new(filesystem, 1);
    fuse_mt::spawn_mount(fs, "../tests/data/Mounting/Target/", &fuse_args[..])
        .unwrap()
        .join();

    finished.recv().expect("Could not receive from channel.");

    Ok(())
}
