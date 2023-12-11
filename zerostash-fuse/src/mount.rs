//! `mount` subcommand

use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsStr,
    io::Result,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fuse_mt::*;
use infinitree::{
    object::{AEADReader, AEADWriter, Pool, PoolRef, Reader, Writer},
    Infinitree, BLOCK_SIZE,
};
use nix::libc;
use scc::ebr::{AtomicShared, Guard, Shared, Tag};
use tokio::{runtime::Handle, task::JoinHandle};
use tracing::debug;
use zerostash_files::{Entry, FileType, Files, Node};

use crate::chunks::ChunkStack;
use crate::chunks::ChunkStackCache;

const MAX_BUFFER_SIZE: usize = infinitree::BLOCK_SIZE;
use zerostash_files::rollsum::CHUNK_SIZE_LIMIT;

pub async fn mount(
    stash: Infinitree<Files>,
    mountpoint: &str,
    threads: usize,
    read_write: bool,
) -> anyhow::Result<()> {
    let stash = Arc::new(stash);

    if read_write {
        stash.load(stash.index().chunks()).unwrap();
        let stash_clone = Arc::clone(&stash);
        tokio::spawn(async move {
            auto_commit(stash_clone).await;
        });
    }

    let mount_type = if read_write { "rw" } else { "ro" };

    let filesystem = ZerostashFs::open(stash, threads, read_write).unwrap();
    let fs = fuse_mt::FuseMT::new(filesystem, 1);

    // Mount the filesystem.
    let handle = spawn_mount(
        fs,
        mountpoint,
        &[
            OsStr::new(mount_type),
            OsStr::new("nodev"),
            OsStr::new("nosuid"),
            OsStr::new("noatime"),
            OsStr::new("fsname=zerostash"),
        ],
    )?;

    // Wait until we are done.
    tokio::signal::ctrl_c().await?;

    // Ensure the filesystem is unmounted.
    handle.join();

    Ok(())
}

async fn auto_commit(stash: Arc<Infinitree<Files>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(180));

    loop {
        interval.tick().await;

        _ = stash.commit("Fuse commit");
        _ = stash.backend().sync();
        debug!("Committed Changes!");
    }
}

pub struct ZerostashFs {
    commit_timestamp: SystemTime,
    stash: Arc<Infinitree<Files>>,
    writer: Option<Pool<AEADWriter>>,
    chunks_cache: scc::HashMap<PathBuf, ChunkStackCache>,
    open_handles: scc::HashMap<u64, OpenFileHandle>,
    runtime: Handle,
}

struct OpenFileHandle {
    // temporary, need to rewrite read() impl
    #[allow(unused)]
    entry: AtomicShared<Entry>,
    writer: Option<JoinHandle<AtomicShared<Entry>>>,
    write_queue: flume::Sender<WriteOp>,
}

enum OpenMode {
    Read,
    Write,
    ReadWrite,
}

enum WriteOp {
    Write(WriteData),
    Flush,
    Close,
}

struct WriteData {
    offset: u64,
    buf: VecDeque<u8>,
}

impl From<u32> for OpenMode {
    fn from(value: u32) -> Self {
        let flags = value as i32;

        if flags & libc::O_RDWR > 0 {
            return OpenMode::ReadWrite;
        }

        if flags & libc::O_WRONLY > 0 {
            return OpenMode::Write;
        }

        // TODO: not sure if this is sound. let's revisit later.
        OpenMode::Read
    }
}

impl OpenFileHandle {
    fn new(parent: &ZerostashFs, entry: Arc<Entry>, mode: OpenMode) -> Self {
        let (write_queue, write_queue_r) = flume::bounded::<WriteOp>(128);
        let (commit_queue, commit_queue_r) = flume::bounded::<WriteOp>(128);

        let shared_entry = AtomicShared::new((*entry).clone());

        let writer = match (mode, parent.writer.as_ref()) {
            (OpenMode::Read, _) => None,
            (_, None) => None,
            (OpenMode::Write | OpenMode::ReadWrite, Some(pool)) => {
                let merger_task = {
                    let ingester = IngestChanges {
                        write_queue_r,
                        commit_queue,
                    };

                    parent.runtime.spawn(ingester.start())
                };

                let commit_task = {
                    let committer = CommitChanges {
                        commit_queue_r,
                        shared_entry: shared_entry.clone(Ordering::Relaxed, &Guard::new()),
                        pool: pool.clone(),
                        entry: (*entry).clone(),
                        reader: parent.stash.storage_reader().unwrap(),
                        hasher: parent.stash.hasher().unwrap(),
                    };
                    parent.runtime.spawn(committer.start())
                };

                Some({
                    let entry = shared_entry.clone(Ordering::Relaxed, &Guard::new());
                    parent.runtime.spawn(async move {
                        _ = tokio::join!(merger_task, commit_task);
                        entry
                    })
                })
            }
        };

        Self {
            writer,
            write_queue,
            entry: shared_entry,
        }
    }
}

struct CommitChanges {
    commit_queue_r: flume::Receiver<WriteOp>,
    reader: PoolRef<AEADReader>,
    hasher: infinitree::Hasher,
    pool: Pool<AEADWriter>,

    entry: Entry,
    shared_entry: AtomicShared<Entry>,
}

impl CommitChanges {
    async fn start(mut self) {
        let mut basebuf = Vec::with_capacity(BLOCK_SIZE);

        loop {
            let (offset, mut buf) = match self.commit_queue_r.recv_async().await {
                Ok(WriteOp::Write(WriteData { offset, buf })) => (offset, buf),
                Ok(WriteOp::Flush) => {
                    self.shared_entry.swap(
                        (Some(Shared::new(self.entry.clone())), Tag::None),
                        Ordering::SeqCst,
                    );
                    continue;
                }
                _ => {
                    self.shared_entry.swap(
                        (Some(Shared::new(self.entry.clone())), Tag::None),
                        Ordering::SeqCst,
                    );
                    break;
                }
            };

            let chunk = self.find_base_chunk(offset);
            let Some((mut base_offset, mut ptr)) = chunk else {
                self.write_new_chunk_for_offset(buf.make_contiguous(), offset);
                self.entry.size += buf.len() as u64;
                continue;
            };

            let mut write_start = (offset - base_offset) as usize;
            let mut write_end = write_start + buf.len();
            loop {
                let chunk_end = self.reader.read_chunk(&ptr, &mut basebuf).unwrap().len();

                if write_end <= chunk_end {
                    basebuf[write_start..write_end].copy_from_slice(buf.make_contiguous());

                    self.write_new_chunk_for_offset(&basebuf[..chunk_end], base_offset);
                } else {
                    let rest = buf.split_off(chunk_end - write_start);

                    basebuf[write_start..chunk_end].copy_from_slice(buf.make_contiguous());

                    self.write_new_chunk_for_offset(&basebuf[..chunk_end], base_offset);

                    buf = rest;

                    let next = self.entry.chunks.range(base_offset + 1..).next();
                    if let Some((offs, p)) = next {
                        write_start = 0;
                        write_end -= chunk_end;
                        base_offset = *offs;
                        ptr = Arc::clone(p);
                    } else {
                        self.write_new_chunk_for_offset(buf.make_contiguous(), chunk_end as u64);

                        break;
                    }
                }
            }
        }
    }

    fn write_new_chunk_for_offset(&mut self, slice: &[u8], offset: u64) {
        let digest = self.hasher.reset().update(slice).finalize();
        let pointer = self.pool.write_chunk(digest.as_bytes(), slice).unwrap();
        self.entry.chunks.insert(offset, pointer.into());
    }

    fn find_base_chunk(&self, offset: u64) -> Option<(u64, Arc<infinitree::ChunkPointer>)> {
        let mut iter = self.entry.chunks.iter().peekable();
        loop {
            match (iter.next(), iter.peek()) {
                (Some((offs_a, chunk)), Some((offs_b, _)))
                    if *offs_a < offset && offset < **offs_b =>
                {
                    break Some((offs_a, chunk));
                }
                (Some(_), Some(_)) => continue,
                (Some((offs, chunk)), None) => break Some((offs, chunk)),
                _ => break None,
            }
        }
        .map(|(o, c)| (*o, Arc::clone(c)))
    }
}

struct IngestChanges {
    write_queue_r: flume::Receiver<WriteOp>,
    commit_queue: flume::Sender<WriteOp>,
}

impl IngestChanges {
    async fn start(self) {
        let mut write_cache: BTreeMap<u64, VecDeque<u8>> = BTreeMap::new();

        loop {
            // while let Ok((offset, new_buf)) = write_queue_r.recv_async().await {
            match self.write_queue_r.recv_async().await {
                Ok(WriteOp::Write(WriteData {
                    offset,
                    buf: mut new_buf,
                })) => {
                    let patch_into = write_cache
                        .iter()
                        .find(|(k, v)| **k < offset && offset < (v.len() as u64))
                        .map(|(k, _)| *k);

                    if let Some((file_offset, buf)) =
                        patch_into.zip(patch_into.and_then(|k| write_cache.get_mut(&k)))
                    {
                        // start byte of `new_buf` within the existing buffer
                        let offs = (offset - file_offset) as usize;

                        let end = offs + new_buf.len();
                        if end < buf.len() {
                            // replace the fully contained segment
                            buf.make_contiguous()[offs..end]
                                .copy_from_slice(new_buf.make_contiguous());
                        } else {
                            buf.truncate(offs);
                            buf.append(&mut new_buf);
                        }
                    } else {
                        write_cache.insert(offset, new_buf);
                    }

                    self.flush_write_cache(false, &mut write_cache);
                }
                Ok(WriteOp::Flush) => {
                    self.flush_write_cache(true, &mut write_cache);
                }
                Ok(WriteOp::Close) => {
                    self.flush_write_cache(true, &mut write_cache);
                    break;
                }
                Err(_) => todo!(),
            }
        }
    }

    fn flush_write_cache(&self, remove_all: bool, write_cache: &mut BTreeMap<u64, VecDeque<u8>>) {
        let mut sized_list = write_cache
            .iter()
            .map(|(k, v)| (v.len(), *k))
            .collect::<Vec<_>>();

        let remove_all =
            remove_all || sized_list.iter().map(|(size, _)| *size).sum::<usize>() > MAX_BUFFER_SIZE;

        sized_list.retain(|(size, offset)| {
            if remove_all || size > &CHUNK_SIZE_LIMIT {
                let (offset, buf) = write_cache.remove_entry(offset).unwrap();
                self.commit_queue
                    .send(WriteOp::Write(WriteData { offset, buf }))
                    .unwrap();
                false
            } else {
                true
            }
        });
    }
}

impl ZerostashFs {
    pub fn open(stash: Arc<Infinitree<Files>>, threads: usize, read_write: bool) -> Result<Self> {
        stash.load(stash.index().tree()).unwrap();

        let commit_timestamp = match stash.commit_list().last() {
            Some(last) => last.metadata.time,
            None => panic!("stash is empty"),
        };

        let writer = if read_write {
            Some(
                Pool::new(
                    NonZeroUsize::new(threads).unwrap(),
                    stash.storage_writer().unwrap(),
                )
                .expect("Failed to open stash for writing"),
            )
        } else {
            None
        };

        Ok(ZerostashFs {
            commit_timestamp,
            stash,
            writer,
            open_handles: scc::HashMap::new(),
            chunks_cache: scc::HashMap::new(),
            runtime: Handle::current(),
        })
    }

    fn new_handle(&self, entry: Arc<Entry>, flags: OpenMode) -> u64 {
        let mut val = rand::random();
        while self.open_handles.contains(&val) {
            val = rand::random();
        }

        _ = self
            .open_handles
            .insert(val, OpenFileHandle::new(self, entry, flags));
        val
    }
}

impl FilesystemMT for ZerostashFs {
    fn destroy(&self) {
        debug!("destroy and commit");

        if self.writer.is_some() {
            self.runtime.block_on(async {
                _ = self.stash.commit("Fuse commit");
                _ = self.stash.backend().sync();
            });
        }
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        let path_str = path.to_str().unwrap();

        let node = {
            let index = self.stash.index();
            let tree = &index.tree;
            tree.node_by_path(path_str)
        };

        let Ok(Some(node)) = node else {
            return Err(libc::ENOENT);
        };

        match node.as_ref() {
            Node::File { refs: _, entry } => {
                Ok((TTL, file_to_fuse(entry.as_ref(), self.commit_timestamp)))
            }
            Node::Directory { entries: _ } => Ok((TTL, DIR_ATTR)),
        }
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, flags: u32) -> ResultOpen {
        debug!("opendir");
        Ok((0, flags))
    }

    fn open(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        debug!("open: {:?}", path);

        if self.writer.is_none() && flags & (libc::O_RDWR | libc::O_WRONLY) as u32 > 0 {
            return Err(libc::EROFS);
        }

        let path_str = path.to_str().unwrap();
        let node = {
            let index = self.stash.index();
            let tree = &index.tree;
            tree.file(path_str)
        };

        let Ok(Some(node)) = node else {
            return Err(libc::ENOENT);
        };

        Ok((self.new_handle(node, flags.into()), flags))
    }

    fn release(
        &self,
        _req: RequestInfo,
        path: &Path,
        fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
    ) -> ResultEmpty {
        debug!("release {:?}", path);

        let Some((_, handle)) = self.open_handles.remove(&fh) else {
            return Err(libc::EINVAL);
        };

        if let Some(background) = handle.writer {
            handle.write_queue.send(WriteOp::Close).unwrap();
            let path_str = path.to_str().unwrap();
            let new_entry = self.runtime.block_on(background).unwrap();
            let guard = Guard::new();
            let new_entry_deref = new_entry.load(Ordering::Relaxed, &guard).as_ref().unwrap();

            self.stash
                .index()
                .tree
                .update_file(path_str, new_entry_deref.clone())
                .unwrap();
        }

        Ok(())
    }

    fn fsync(&self, _req: RequestInfo, _path: &Path, fh: u64, _datasync: bool) -> ResultEmpty {
        let Some(entry) = self.open_handles.get(&fh) else {
            return Err(libc::EINVAL);
        };

        let handle = entry.get();
        if handle.writer.is_some() {
            handle.write_queue.send(WriteOp::Flush).unwrap();
        }

        Ok(())
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);

        let path_str = path.to_str().unwrap();
        let node = {
            let index = self.stash.index();
            let tree = &index.tree;
            tree.node_by_path(path_str)
        };

        let Ok(Some(node)) = node else {
            return Err(libc::ENOENT);
        };

        let Node::Directory { entries } = node.as_ref() else {
            return Err(libc::ENOENT);
        };

        let index = self.stash.index();

        let mut vec: Vec<DirectoryEntry> = vec![];

        let mut current = entries.first_entry();
        while let Some(entry) = current {
            if let Some(node) = index.tree.node_by_ref(entry.get()) {
                let kind = if node.is_dir() {
                    fuse_mt::FileType::Directory
                } else {
                    fuse_mt::FileType::RegularFile
                };
                let directory_entry = DirectoryEntry {
                    name: entry.key().clone().into(),
                    kind,
                };
                vec.push(directory_entry);
            }
            current = entry.next();
        }

        Ok(vec)
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

        let entry = {
            let index = &self.stash.index();
            let tree = &index.tree;
            let Ok(Some(entry)) = tree.file(path_string) else {
                return callback(Err(libc::EINVAL));
            };
            entry
        };

        let file_size = entry.size as usize;
        let offset = offset as usize;

        if offset > file_size {
            return callback(Err(libc::EINVAL));
        }

        let size = size as usize;
        let sort_chunks = || entry.chunks.clone().into_iter().collect::<Vec<_>>();
        let mut obj_reader = self.stash.storage_reader().unwrap();

        self.runtime.block_on(async {
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
        })
    }

    fn write(
        &self,
        _req: RequestInfo,
        path: &Path,
        fh: u64,
        offset: u64,
        data: Vec<u8>,
        _flags: u32,
    ) -> ResultWrite {
        debug!("write: {:?} {:#x} @ {:#x}", path, data.len(), offset);

        let Some(handle) = self.open_handles.get(&fh) else {
            return Err(libc::EINVAL);
        };

        let Ok(size) = data.len().try_into() else {
            return Err(libc::EINVAL);
        };

        if handle.get().writer.is_none() {
            return Err(libc::EINVAL);
        }

        handle
            .get()
            .write_queue
            .send(WriteOp::Write(WriteData {
                offset,
                buf: data.into(),
            }))
            .unwrap();

        Ok(size)
    }

    fn truncate(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>, size: u64) -> ResultEmpty {
        debug!("truncate {:?}: size {}", path, size);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();

        let entry = {
            let tree = &self.stash.index().tree;
            let Ok(Some(entry)) = tree.file(path_string) else {
                return Err(libc::EINVAL);
            };
            entry
        };

        if entry.size == size {
            return Ok(());
        }

        if entry.size < size {
            self.stash
                .index()
                .tree
                .update_file(
                    path_string,
                    Entry {
                        size,
                        ..entry.as_ref().clone()
                    },
                )
                .unwrap();

            return Ok(());
        }

        let mut chunks = entry.chunks.clone();
        let Some(last_chunk_start) = chunks
            .range(size..)
            .next()
            .or(entry.chunks.last_key_value())
            .map(|(offs, _)| *offs)
        else {
            unreachable!();
        };

        let rest = chunks.split_off(&last_chunk_start);
        let Some((_, last_chunk)) = rest.first_key_value() else {
            unreachable!();
        };

        let truncated_chunk = {
            let mut reader = self.stash.storage_reader().unwrap();
            // i'm assuming we're not so good at compression that this
            // isn't enough?
            let mut buf: Vec<u8> = vec![0; last_chunk.size() * 16];
            reader.read_chunk(last_chunk, &mut buf).unwrap();

            buf.truncate((size - last_chunk_start) as usize);
            buf
        };

        let hash = *self
            .stash
            .hasher()
            .unwrap()
            .update(&truncated_chunk)
            .finalize()
            .as_bytes();

        let index = self.stash.index();
        let mut writer = self.writer.as_ref().unwrap().clone();
        chunks.insert(
            last_chunk_start,
            index.chunks.insert_with(hash, move || {
                writer.write_chunk(&hash, &truncated_chunk).unwrap()
            }),
        );

        let new_entry = Entry {
            chunks,
            size,
            ..entry.as_ref().clone()
        };

        index.tree.update_file(path_string, new_entry).unwrap();

        Ok(())
    }

    fn rename(
        &self,
        _req: RequestInfo,
        parent: &Path,
        name: &OsStr,
        newparent: &Path,
        newname: &OsStr,
    ) -> ResultEmpty {
        debug!(
            "rename: {:?}/{:?} -> {:?}/{:?}",
            parent, name, newparent, newname
        );

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();
        let new_path = newparent.join(newname);
        let new_path_str = strip_path(&new_path).to_str().unwrap().to_string();
        let index = self.stash.index();
        let tree = &index.tree;

        if tree.move_node(&path_str, &new_path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn mkdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr, _mode: u32) -> ResultEntry {
        debug!("mkdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let index = self.stash.index();
        let tree = &index.tree;

        if tree.insert_directory(path.to_str().unwrap()).is_err() {
            return Err(libc::EIO);
        }

        Ok((TTL, DIR_ATTR))
    }

    fn rmdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("rmdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;

        if tree.remove(&path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn unlink(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("unlink: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;

        if tree.remove(&path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn create(
        &self,
        req: RequestInfo,
        parent: &Path,
        name: &OsStr,
        mode: u32,
        flags: u32,
    ) -> ResultCreate {
        debug!("create {:?}/{:?}", parent, name);
        let real_path = parent.join(name);
        let path_string = strip_path(&real_path).to_str().unwrap();

        let now = SystemTime::now();
        let unix = now.duration_since(UNIX_EPOCH).unwrap();
        let name = name.to_str().unwrap().to_string();

        let entry = Arc::new(Entry {
            unix_secs: unix.as_secs() as i64,
            unix_nanos: unix.as_nanos() as u32,
            unix_perm: Some(mode),
            unix_uid: Some(req.uid),
            unix_gid: Some(req.gid),
            readonly: None,
            file_type: FileType::File,
            size: 0,
            name,
            chunks: Default::default(),
        });

        let attr = file_to_fuse(&entry, SystemTime::now());

        let index = self.stash.index();
        let tree = &index.tree;
        if tree
            .insert_file(path_string, entry.as_ref().clone())
            .is_err()
        {
            return Err(libc::EIO);
        }

        let fh = self.new_handle(entry, flags.into());

        Ok(CreatedEntry {
            ttl: TTL,
            attr,
            fh,
            flags,
        })
    }

    fn chmod(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>, mode: u32) -> ResultEmpty {
        debug!("chmod: {:?} {:#o}", path, mode);
        let path_string = strip_path(path).to_str().unwrap().to_string();

        let mut index = self.stash.index().clone();

        let tree = &mut index.tree;
        let Ok(Some(entry)) = tree.file(&path_string) else {
            return Err(libc::EINVAL);
        };

        let new_entry = Entry {
            unix_perm: Some(mode),
            chunks: entry.chunks.clone(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..entry.as_ref().clone()
        };

        if tree.update_file(&path_string, new_entry).is_err() {
            return Err(libc::ENOENT);
        }

        Ok(())
    }

    fn chown(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: Option<u64>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> ResultEmpty {
        debug!("chown {:?} to {:?}:{:?}", path, uid, gid);
        let path_string = strip_path(path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;
        let Ok(Some(entry)) = tree.file(&path_string) else {
            return Err(libc::EINVAL);
        };

        let new_entry = Entry {
            unix_uid: Some(uid.unwrap_or_else(|| {
                entry
                    .unix_uid
                    .unwrap_or_else(|| nix::unistd::getuid().into())
            })),
            unix_gid: Some(gid.unwrap_or_else(|| {
                entry
                    .unix_gid
                    .unwrap_or_else(|| nix::unistd::getgid().into())
            })),
            chunks: entry.chunks.clone(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..entry.as_ref().clone()
        };

        if tree.update_file(&path_string, new_entry).is_err() {
            return Err(libc::ENOENT);
        }

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
    kind: fuse_mt::FileType::Directory,
    perm: 0o777,
    nlink: 1,
    uid: 1000,
    gid: 1000,
    rdev: 0,
    flags: 0,
};

fn file_to_fuse(file: &Entry, atime: SystemTime) -> FileAttr {
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
        kind: match_filetype(file.file_type.clone()),
        perm: (file.unix_perm.unwrap() & 0o777) as u16,
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

fn match_filetype(file_type: FileType) -> fuse_mt::FileType {
    match file_type {
        FileType::File => fuse_mt::FileType::RegularFile,
        FileType::Symlink(_) => fuse_mt::FileType::Symlink,
        FileType::Directory => panic!("Must be a file!"),
    }
}
