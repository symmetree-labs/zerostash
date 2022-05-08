use crate::{files, rollsum::SeaSplit, splitter::FileSplitter, Files};
use anyhow::Context;
use flume as mpsc;
use futures::{future::join_all, stream::futures_unordered::FuturesUnordered};
use ignore::{DirEntry, WalkBuilder};
use infinitree::{
    object::{Pool, Writer},
    Infinitree,
};
use memmap2::{Mmap, MmapOptions};
use std::path::PathBuf;
use tokio::{fs, io::AsyncReadExt, task};
use tracing::{error, trace, trace_span, warn, Instrument};

type Sender = mpsc::Sender<(PathBuf, files::Entry)>;
type Receiver = mpsc::Receiver<(PathBuf, files::Entry)>;

const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

#[derive(clap::Args, Debug, Default, Clone)]
pub struct Options {
    /// The paths to include in the commit. All changes (addition/removal) will be committed.
    pub paths: Vec<PathBuf>,

    #[clap(flatten)]
    pub preserve: files::PreserveMetadata,

    /// Force hashing all files even if size and modification time is the same
    #[clap(short = 'f', long)]
    pub force: bool,

    /// Ignore files larger than the given value in bytes.
    #[clap(short = 'm', long = "max-size")]
    pub max_size: Option<u64>,

    /// Do not cross file system boundaries during directory walk.
    #[clap(short = 'x', long = "same-file-system")]
    pub same_fs: bool,

    /// Ignore hidden files.
    #[clap(short = 'd', long = "ignore-hidden")]
    pub hidden: bool,

    /// Process ignore rules case insensitively.
    #[clap(short = 'i', long = "ignore-case-insensitive")]
    pub case_insensitive: bool,

    /// Respect ignore rules from parent directories (.gitignore and .ignore files)
    #[clap(
        short = 'P',
        long = "inherit-parent-ignore",
        default_value = "true",
        parse(try_from_str)
    )]
    pub parents: bool,

    /// Respect global gitignore rules (from `core.excludesFile` setting, or $XDG_CONFIG_HOME/git/ignore)
    #[clap(short = 'G', long = "git-global-ignore")]
    pub git_global: bool,

    /// Respect git .git/info/exclude files for git repositories.
    #[clap(short = 'E', long = "git-exclude")]
    pub git_exclude: bool,

    /// Respect .gitignore files for git repositories.
    #[clap(short = 'g', long = "git-gitignore")]
    pub git_ignore: bool,

    /// Respect .ignore files, which are equivalent to .gitignore without git.
    #[clap(short = 'I', long = "dot-ignore")]
    pub ignore: bool,

    /// Follow symbolic links.
    #[clap(short = 'l', long = "follow-links")]
    pub follow_links: bool,
}

impl Options {
    pub async fn add_recursive(
        &self,
        stash: &Infinitree<Files>,
        threads: usize,
    ) -> anyhow::Result<()> {
        let (sender, workers) = start_workers(stash, threads, self.force)?;
        let dir_walk = self.dir_walk()?;
        let mut current_file_list = vec![];

        for dir_entry in dir_walk {
            let (metadata, path) = match dir_entry {
                Ok(de) => (de.metadata(), de.path().to_owned()),
                Err(error) => {
                    warn!(%error, "failed to process file; skipping");
                    continue;
                }
            };

            let metadata = match metadata {
                Ok(md) if md.is_file() || md.is_symlink() => md,
                Err(error) => {
                    warn!(%error, ?path, "failed to get file metadata; skipping");
                    continue;
                }
                _ => continue,
            };

            let entry = match files::Entry::from_metadata(metadata, &path, &self.preserve) {
                Ok(e) => e,
                Err(error) => {
                    error!(%error, ?path, "failed to ingest file; aborting");
                    break;
                }
            };

            trace!(?path, "queued");
            current_file_list.push(entry.name.clone());
            sender.send((path, entry)).unwrap();
        }

        drop(sender);
        join_all(workers).await;

        let source_paths = self
            .paths
            .iter()
            .map(files::normalize_filename)
            .collect::<Result<Vec<_>, _>>()?;

        stash.index().files.retain(|k, _| {
            for path in source_paths.iter() {
                if k.starts_with(path) {
                    // if the current directory is part of the new commit, diff
                    return current_file_list.contains(k);
                }
            }

            // if it's unrelated, keep it in the index
            true
        });

        Ok(())
    }

    fn dir_walk(&self) -> anyhow::Result<impl Iterator<Item = Result<DirEntry, ignore::Error>>> {
        let mut paths = self.paths.iter();
        let mut builder = WalkBuilder::new(paths.next().context("no path available")?);

        for path in paths {
            builder.add(path);
        }

        builder.standard_filters(false);
        builder.max_filesize(self.max_size);
        builder.same_file_system(self.same_fs);
        builder.hidden(self.hidden);
        builder.ignore_case_insensitive(self.case_insensitive);
        builder.parents(self.parents);
        builder.git_exclude(self.git_exclude);
        builder.git_ignore(self.git_ignore);
        builder.git_global(self.git_global);
        builder.ignore(self.ignore);
        builder.follow_links(self.follow_links);

        Ok(builder.build())
    }
}

fn start_workers(
    stash: &Infinitree<Files>,
    threads: usize,
    force: bool,
) -> anyhow::Result<(Sender, Vec<task::JoinHandle<()>>)> {
    // make sure the input and output queues are generous
    let (sender, receiver) = mpsc::bounded(threads * 2);
    let balancer = Pool::new(threads, stash.object_writer()?)?;

    let workers = (0..threads)
        .map(|_| {
            task::spawn(process_file_loop(
                force,
                receiver.clone(),
                stash.index().clone(),
                balancer.clone(),
            ))
        })
        .collect::<Vec<_>>();

    Ok((sender, workers))
}

async fn process_file_loop(
    force: bool,
    r: Receiver,
    index: crate::Files,
    writer: Pool<impl Writer + Clone + 'static>,
) {
    let mut buf = Vec::with_capacity(MAX_FILE_SIZE);

    while let Ok((path, entry)) = r.recv_async().await {
        buf.clear();

        if !force {
            if let Some(in_store) = index.files.get(&entry.name) {
                if in_store.as_ref() == &entry {
                    trace!(?path, "already indexed, skipping");
                    continue;
                }
            }
        }

        let size = entry.size;
        if size == 0 || entry.file_type.is_symlink() {
            index.files.insert(entry.name.clone(), entry);
            continue;
        }

        let osfile = match fs::File::open(&path).await {
            Ok(f) => f,
            Err(error) => {
                warn!(%error, ?path, "failed to open file; skipping");
                continue;
            }
        };

        index_file(entry, osfile, &mut buf, path.clone(), &index, &writer)
            .instrument(trace_span!("index file", ?path, size))
            .await;
    }
}

async fn index_file(
    mut entry: files::Entry,
    mut osfile: fs::File,
    buf: &mut Vec<u8>,
    path: PathBuf,
    index: &crate::Files,
    writer: &Pool<impl Writer + Clone + 'static>,
) {
    let size = entry.size as usize;

    if size < MAX_FILE_SIZE {
        osfile.read_to_end(buf).await.unwrap();
    }

    let mut mmap = MmappedFile::new(size, osfile.into_std().await);

    let splitter = if size < MAX_FILE_SIZE {
        FileSplitter::<SeaSplit>::new(&buf[0..size])
    } else {
        FileSplitter::<SeaSplit>::new(mmap.open())
    };

    let chunks = splitter
        .map(|(start, hash, data)| {
            let mut writer = writer.clone();
            let data = data.to_vec();
            let chunkindex = index.chunks.clone();

            task::spawn(async move {
                let store = || writer.write_chunk(&hash, &data).unwrap();
                let ptr = chunkindex.insert_with(hash, store);
                (start, ptr)
            })
        })
        .collect::<FuturesUnordered<_>>();

    entry
        .chunks
        .extend(join_all(chunks).await.into_iter().map(Result::unwrap));

    trace!(?path, chunks = entry.chunks.len(), "indexed");

    index.files.insert(entry.name.clone(), entry);
}

struct MmappedFile {
    mmap: Option<Mmap>,
    len: usize,
    _file: std::fs::File,
}

impl MmappedFile {
    fn new(len: usize, _file: std::fs::File) -> Self {
        Self {
            mmap: None,
            len,
            _file,
        }
    }

    fn open(&mut self) -> &[u8] {
        self.mmap.get_or_insert(unsafe {
            MmapOptions::new()
                .len(self.len)
                .populate()
                .map(&self._file)
                .unwrap()
        })
    }
}
