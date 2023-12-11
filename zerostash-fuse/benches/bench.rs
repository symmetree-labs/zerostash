use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use criterion::{criterion_group, criterion_main, Criterion};
use fuse_mt::FuseMT;
use infinitree::{backends, crypto::UsernamePassword, Infinitree};
use zerostash_fuse::mount::ZerostashFs;

criterion_group!(benches, mount_starup);
criterion_main! {benches}

fn mount_starup(c: &mut Criterion) {
    c.bench_function("mount startup", |b| b.iter(mount));
}

async fn mount() -> anyhow::Result<()> {
    let key = "abcde".to_string();
    let key = UsernamePassword::with_credentials(key.clone(), key).unwrap();

    let backend = backends::Directory::new(PathBuf::from("../tests/data/Mounting/Stash/")).unwrap();
    let stash = Infinitree::open(backend, key).unwrap();
    let fuse_args = [OsStr::new("-o"), OsStr::new("fsname=zerostash")];
    let filesystem = ZerostashFs::open(Arc::new(Mutex::new(stash)), 0, false).unwrap();
    let fs = FuseMT::new(filesystem, 1);
    let handle =
        fuse_mt::spawn_mount(fs, "../tests/data/Mounting/Target/", &fuse_args[..]).unwrap();

    handle.join();

    Ok(())
}
