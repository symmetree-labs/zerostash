//! `mount` subcommand

use std::ffi::OsStr;

use std::mem;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::vec::IntoIter;

use infinitree::fields::VersionedMap;
use tracing::debug;
use zerostash_files::directory::Dir;

use std::io::Result;

use crate::prelude::*;
use fuse_mt::*;
use infinitree::object::{AEADReader, PoolRef, Reader};
use infinitree::{ChunkPointer, Infinitree};
use nix::libc;
use zerostash_files::{restore, Entry, Files};

#[derive(Command, Debug)]
pub struct Mount {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: restore::Options,

    /// The location the filesytem will be mounted on
    #[clap(short = 'T', long = "target")]
    mount_point: String,
}

#[cfg(unix)]
#[async_trait]
impl AsyncRunnable for Mount {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();

        if let Err(e) = mount(stash, &self.options, &self.mount_point) {
            panic!("Error = {}", e)
        }
    }
}

pub fn mount(
    stash: Infinitree<Files>,
    options: &restore::Options,
    mountpoint: &str,
) -> anyhow::Result<()> {
    let (tx, finished) = mpsc::sync_channel(2);
    let destroy_tx = tx.clone();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let filesystem = ZerostashFS::open(stash, options, destroy_tx).unwrap();
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

type Chunks = IntoIter<(u64, Arc<ChunkPointer>)>;

pub struct ChunksIter {
    pub chunks: std::iter::Peekable<Chunks>,
}

impl ChunksIter {
    fn new(chunks: Chunks) -> Self {
        let chunks = chunks.peekable();
        Self { chunks }
    }

    fn peek_next_offset(&mut self, file_size: usize) -> usize {
        let arc = (file_size as u64, Arc::new(ChunkPointer::default()));
        let (chunk_offset, _) = self.chunks.peek().unwrap_or(&arc);
        *chunk_offset as usize
    }

    fn get_next(&mut self) -> Option<(usize, Arc<ChunkPointer>)> {
        let (c_offset, pointer) = match self.chunks.next() {
            Some((o, p)) => (o as usize, p),
            None => return None,
        };
        Some((c_offset, pointer))
    }
}

#[derive(Debug)]
pub enum ChunkDataError {
    NullChunkPointer,
}

pub struct ChunkStackCache {
    pub chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub last_read_offset: usize,
}

impl ChunkStackCache {
    fn new(chunks: ChunksIter) -> Self {
        Self {
            chunks,
            buf: Default::default(),
            last_read_offset: Default::default(),
        }
    }

    fn set_current_read(&mut self, val: usize) {
        self.last_read_offset = val;
    }

    fn split_buf(&mut self, end: usize) -> Vec<u8> {
        let mut ret_buf = self.buf.split_off(end);
        mem::swap(&mut self.buf, &mut ret_buf);
        ret_buf
    }

    #[inline(always)]
    fn read_next(
        &mut self,
        file_size: usize,
        objectreader: &mut PoolRef<AEADReader>,
    ) -> anyhow::Result<(), ChunkDataError> {
        let (c_offset, pointer) = match self.chunks.get_next() {
            Some(chunk) => chunk,
            None => return Err(ChunkDataError::NullChunkPointer),
        };
        let next_c_offset = self.chunks.peek_next_offset(file_size);

        let mut temp_buf = vec![0; next_c_offset - c_offset];
        objectreader.read_chunk(&pointer, &mut temp_buf).unwrap();
        self.buf.append(&mut temp_buf);

        Ok(())
    }
}

pub struct ChunkStack {
    pub chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl ChunkStack {
    fn new(chunks: ChunksIter) -> Self {
        Self {
            chunks,
            buf: Default::default(),
            start: None,
            end: None,
        }
    }

    #[inline(always)]
    fn read_next(
        &mut self,
        file_size: usize,
        offset: usize,
        objectreader: &mut PoolRef<AEADReader>,
    ) -> anyhow::Result<(), ChunkDataError> {
        let (c_offset, pointer) = match self.chunks.get_next() {
            Some(chunk) => chunk,
            None => return Err(ChunkDataError::NullChunkPointer),
        };
        let next_c_offset = self.chunks.peek_next_offset(file_size);

        if !self.buf.is_empty() || offset < next_c_offset {
            if self.start.is_none() {
                self.start = Some(offset - c_offset);
            }
            let mut temp_buf = vec![0; next_c_offset - c_offset];
            objectreader.read_chunk(&pointer, &mut temp_buf).unwrap();
            self.buf.append(&mut temp_buf);
        }
        Ok(())
    }

    fn is_full(&mut self, size: usize, file_size: usize, offset: usize) -> bool {
        if let Some(from) = self.start {
            if self.buf[from..].len() >= size.min(file_size - offset) {
                self.end = Some(self.buf.len().min(from + size));
                return true;
            }
        }
        false
    }
}

pub struct ZerostashFS {
    pub commit_timestamp: SystemTime,
    pub destroy_tx: mpsc::SyncSender<()>,
    pub stash: Infinitree<Files>,
    pub chunks_cache: scc::HashMap<PathBuf, ChunkStackCache>,
}

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

pub fn match_filetype(file_type: &zerostash_files::FileType) -> FileType {
    match file_type {
        zerostash_files::FileType::File => FileType::RegularFile,
        zerostash_files::FileType::Symlink(_) => FileType::Symlink,
        zerostash_files::FileType::Directory => panic!("Didnt expect a directory!"),
    }
}

pub fn walk_dir_up(index: &VersionedMap<PathBuf, Mutex<Vec<Dir>>>, path: PathBuf) {
    if let Some(parent) = path.parent() {
        let dir = Dir::new(path.clone(), zerostash_files::FileType::Directory);
        match index.get(parent) {
            Some(parent_map) => {
                if !parent_map.lock().unwrap().contains(&dir) {
                    parent_map.lock().unwrap().push(dir);
                }
            }
            None => {
                index.insert(parent.to_path_buf(), Mutex::new(vec![dir]));
            }
        }
        walk_dir_up(index, parent.to_path_buf());
    }
}

impl ZerostashFS {
    pub fn open(
        stash: Infinitree<Files>,
        _options: &restore::Options,
        destroy_tx: mpsc::SyncSender<()>,
    ) -> Result<Self> {
        stash.load_all().unwrap();

        let commit_timestamp = stash.commit_list().last().unwrap().metadata.time;
        let mut temp_paths = vec![];

        {
            stash.index().directories.for_each(|k, _| {
                temp_paths.push(k.to_path_buf());
            });
            for k in temp_paths.iter() {
                let index = &stash.index().directories;
                walk_dir_up(index, k.to_path_buf());
            }
        }

        Ok(ZerostashFS {
            commit_timestamp,
            destroy_tx,
            stash,
            chunks_cache: scc::HashMap::new(),
        })
    }
}

const TTL: Duration = Duration::from_secs(1);

pub fn file_to_fuse(file: &Arc<Entry>, atime: SystemTime) -> FileAttr {
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

impl FilesystemMT for ZerostashFS {
    fn destroy(&self) {
        debug!("destroy");
        self.destroy_tx
            .send(())
            .expect("Could not send signal on channel.")
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        let real_path = strip_path(path);

        if self.stash.index().directories.contains(&path.to_path_buf()) {
            Ok((TTL, DIR_ATTR))
        } else {
            let path_string = real_path.to_str().unwrap();
            match self.stash.index().files.get(path_string) {
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

        let entries = self.stash.index().directories.get(path).unwrap_or_default();
        let entries = entries.lock().unwrap();
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
        let metadata = self.stash.index().files.get(path_string).unwrap();
        let file_size = metadata.size as usize;
        let offset = offset as usize;

        if offset > file_size {
            return callback(Err(libc::EINVAL));
        }

        let size = size as usize;
        let sort_chunks = || {
            let mut chunks = metadata.chunks.clone();
            chunks.sort_by(|(a, _), (b, _)| a.cmp(b));
            chunks.into_iter()
        };
        let mut obj_reader = self.stash.storage_reader().unwrap();

        {
            let mut chunks = self
                .chunks_cache
                .entry(real_path.to_path_buf())
                .or_insert_with(|| ChunkStackCache::new(ChunksIter::new(sort_chunks())));
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

        let mut chunks = ChunkStack::new(ChunksIter::new(sort_chunks()));

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

pub fn transform(entries: Vec<Dir>) -> Vec<DirectoryEntry> {
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
