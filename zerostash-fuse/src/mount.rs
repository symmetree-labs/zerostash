//! `mount` subcommand

use std::ffi::OsStr;

use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use infinitree::object::AEADReader;
use infinitree::object::Pool;
use infinitree::object::PoolRef;
use infinitree::object::Reader;
use tracing::debug;
use zerostash_files::directory::Dir;
use zerostash_files::store::index_buf;

use std::io::Result;

use fuse_mt::*;
use infinitree::Infinitree;
use nix::libc;
use zerostash_files::{restore, Entry, Files};

use crate::chunks::ChunkStack;
use crate::chunks::ChunkStackCache;
use crate::openfile::OpenFile;

pub async fn mount(
    stash: Infinitree<Files>,
    options: &restore::Options,
    mountpoint: &str,
    threads: usize,
) -> anyhow::Result<()> {
    let stash = Arc::new(Mutex::new(stash));
    let (tx, finished) = mpsc::sync_channel(2);
    let destroy_tx = tx.clone();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let stash_clone = Arc::clone(&stash);
    tokio::spawn(async move {
        auto_commit(stash_clone).await;
    });

    let filesystem = ZerostashFS::open(stash, options, destroy_tx, threads).unwrap();
    let fuse_args = [OsStr::new("-o"), OsStr::new("fsname=zerostash")];

    let fs = fuse_mt::FuseMT::new(filesystem, 1);

    // Mount the filesystem.
    let handle = spawn_mount(fs, mountpoint, &fuse_args[..])?;

    // Wait until we are done.
    finished.recv().expect("Could not receive from channel.");

    // Ensure the filesystem is unmounted.
    handle.join();

    Ok(())
}

async fn auto_commit(stash: Arc<Mutex<Infinitree<Files>>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(180));

    interval.tick().await;
    loop {
        interval.tick().await;

        let mut stash_guard = stash.lock().unwrap();
        let _ = stash_guard.commit("Fuse commit");
        let _ = stash_guard.backend().sync();
        debug!("Committed Changes!");
    }
}

pub struct ZerostashFS {
    pub commit_timestamp: SystemTime,
    pub destroy_tx: mpsc::SyncSender<()>,
    pub stash: Arc<Mutex<Infinitree<Files>>>,
    pub chunks_cache: scc::HashMap<PathBuf, ChunkStackCache>,
    pub threads: usize,
}

impl ZerostashFS {
    pub fn open(
        stash: Arc<Mutex<Infinitree<Files>>>,
        _options: &restore::Options,
        destroy_tx: mpsc::SyncSender<()>,
        threads: usize,
    ) -> Result<Self> {
        stash.lock().unwrap().load_all().unwrap();

        let commit_timestamp = stash
            .lock()
            .unwrap()
            .commit_list()
            .last()
            .unwrap()
            .metadata
            .time;

        Ok(ZerostashFS {
            commit_timestamp,
            destroy_tx,
            stash,
            chunks_cache: scc::HashMap::new(),
            threads,
        })
    }
}

impl FilesystemMT for ZerostashFS {
    fn destroy(&self) {
        debug!("destroy and commit");

        let mut stash = self.stash.lock().unwrap();
        let _ = stash.commit("Fuse commit");
        let _ = stash.backend().sync();
        self.destroy_tx
            .send(())
            .expect("Could not send signal on channel.");
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        if self
            .stash
            .lock()
            .unwrap()
            .index()
            .directories
            .contains(&path.to_path_buf())
        {
            Ok((TTL, DIR_ATTR))
        } else {
            let path_string = strip_path(path).to_str().unwrap();
            match self.stash.lock().unwrap().index().files.get(path_string) {
                Some(metadata) => {
                    let fuse = file_to_fuse(&metadata, self.commit_timestamp);
                    Ok((TTL, fuse))
                }
                None => Err(libc::ENOENT),
            }
        }
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, _flags: u32) -> ResultOpen {
        debug!("opendir");
        Ok((0, 0))
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);

        let entries = self
            .stash
            .lock()
            .unwrap()
            .index()
            .directories
            .get(path)
            .unwrap_or_default();
        let transformed_entries = transform(entries.to_vec());

        Ok(transformed_entries)
    }

    fn open(&self, _req: RequestInfo, path: &Path, _flags: u32) -> ResultOpen {
        debug!("open: {:?}", path);
        Ok((0, 0))
    }

    fn read(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        offset: u64,
        size: u32,
        callback: impl FnOnce(ResultSlice<'_>) -> CallbackResult,
    ) -> CallbackResult {
        debug!("read: {:?} {:#x} @ {:#x}", path, size, offset);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();
        let metadata = self
            .stash
            .lock()
            .unwrap()
            .index()
            .files
            .get(path_string)
            .unwrap();
        let file_size = metadata.size as usize;
        let offset = offset as usize;

        if offset > file_size {
            return callback(Err(libc::EINVAL));
        }

        let size = size as usize;
        let sort_chunks = || {
            let mut chunks = metadata.chunks.clone();
            chunks.sort_by(|(a, _), (b, _)| a.cmp(b));
            chunks
        };
        let mut obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();

        {
            let mut chunks = self
                .chunks_cache
                .entry(real_path.to_path_buf())
                .or_insert_with(|| ChunkStackCache::new(sort_chunks()));
            let chunks = chunks.get_mut();

            if chunks.last_read_offset == offset {
                let end = size.min(file_size - offset);
                if chunks.buf.len() < end {
                    loop {
                        if chunks.read_next(file_size, &mut obj_reader).is_err() {
                            return callback(Err(libc::EINVAL));
                        }

                        if chunks.buf.len() >= end {
                            break;
                        }
                    }
                }
                let ret_buf = chunks.split_buf(end);
                chunks.set_current_read(offset + end);
                return callback(Ok(&ret_buf));
            }
        }

        let mut chunks = ChunkStack::new(sort_chunks(), offset);

        loop {
            if chunks
                .read_next(file_size, offset, &mut obj_reader)
                .is_err()
            {
                return callback(Err(libc::EINVAL));
            }

            if chunks.is_full(size, file_size, offset) {
                let from = chunks.start.unwrap();
                let to = chunks.end.unwrap();
                return callback(Ok(&chunks.buf[from..to]));
            }
        }
    }

    fn write(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        offset: u64,
        data: Vec<u8>,
        _flags: u32,
    ) -> ResultWrite {
        debug!("write: {:?} {:#x} @ {:#x}", path, data.len(), offset);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();

        let entry = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let files = &index.files;
            files.get(path_string).unwrap()
        };

        let obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();
        let mut buf: Vec<u8> = vec![0; entry.size as usize];
        read_chunks_into_buf(&mut buf, obj_reader, &entry);

        let mut open_file = OpenFile::from_vec(buf);
        let nwritten = match open_file.write_at(offset, data) {
            Ok(nwritten) => nwritten,
            Err(e) => return Err(e),
        };
        let entry_len = open_file.get_len();

        let entry = Arc::clone(&entry);
        let new_entry = Entry {
            size: entry_len,
            chunks: Vec::new(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..*entry
        };

        let stash = self.stash.lock().unwrap();
        let index = stash.index();
        let hasher = stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            stash.storage_writer().unwrap(),
        )
        .unwrap();

        index_buf(
            open_file.open_file,
            new_entry,
            hasher,
            &index,
            &balancer,
            path_string.to_string(),
        );

        Ok(nwritten)
    }

    fn truncate(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>, size: u64) -> ResultEmpty {
        debug!("truncate {:?}: size {}", path, size);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();

        let entry = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let files = &index.files;
            files.get(path_string).unwrap()
        };

        let obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();
        let mut buf: Vec<u8> = vec![0; entry.size as usize];
        read_chunks_into_buf(&mut buf, obj_reader, &entry);

        buf.truncate(size as usize);
        let len = buf.len() as u64;
        let open_file = Cursor::new(buf);

        let stash = self.stash.lock().unwrap();
        let index = stash.index();
        let hasher = stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            stash.storage_writer().unwrap(),
        )
        .unwrap();

        let new_entry = Entry {
            size: len,
            chunks: Vec::new(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..*entry
        };

        index_buf(
            open_file,
            new_entry,
            hasher,
            &index,
            &balancer,
            path_string.to_string(),
        );

        Ok(())
    }

    fn release(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
    ) -> ResultEmpty {
        debug!("release {:?}", path);

        let real_path = strip_path(path);
        self.chunks_cache.remove(real_path);

        Ok(())
    }
}

const TTL: Duration = Duration::from_secs(1);

const DIR_ATTR: FileAttr = FileAttr {
    size: 0,
    blocks: 0,
    atime: SystemTime::UNIX_EPOCH,
    mtime: SystemTime::UNIX_EPOCH,
    ctime: SystemTime::UNIX_EPOCH,
    crtime: SystemTime::UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o444,
    nlink: 1,
    uid: 1000,
    gid: 1000,
    rdev: 0,
    flags: 0,
};

fn transform(entries: Vec<Dir>) -> Vec<DirectoryEntry> {
    let mut vec = vec![];
    for entry in entries.iter() {
        let new_entry = DirectoryEntry {
            name: entry.path.file_name().unwrap().into(),
            kind: match entry.file_type {
                zerostash_files::FileType::Directory => fuse_mt::FileType::Directory,
                _ => fuse_mt::FileType::RegularFile,
            },
        };
        vec.push(new_entry);
    }
    vec
}

fn read_chunks_into_buf(buf: &mut [u8], mut reader: PoolRef<AEADReader>, entry: &Arc<Entry>) {
    for (start, cp) in entry.chunks.iter() {
        let start = *start as usize;
        reader.read_chunk(cp, &mut buf[start..]).unwrap();
    }
}

fn file_to_fuse(file: &Arc<Entry>, atime: SystemTime) -> FileAttr {
    let mtime = UNIX_EPOCH
        + Duration::from_secs(file.unix_secs as u64)
        + Duration::from_nanos(file.unix_nanos as u64);
    FileAttr {
        size: file.size,
        blocks: 1,
        atime,
        mtime,
        ctime: mtime,
        crtime: SystemTime::UNIX_EPOCH,
        kind: FileType::RegularFile,
        perm: 0o444,
        nlink: 1,
        gid: file
            .unix_gid
            .unwrap_or_else(|| nix::unistd::getgid().into()),
        uid: file
            .unix_uid
            .unwrap_or_else(|| nix::unistd::getuid().into()),
        rdev: 0,
        flags: 0,
    }
}

fn strip_path(path: &Path) -> &Path {
    path.strip_prefix("/").unwrap()
}
