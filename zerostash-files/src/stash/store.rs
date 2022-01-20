use crate::{files, rollsum::SeaSplit, splitter::FileSplitter, Files};
use infinitree::{
    object::{self, write_balancer::RoundRobinBalancer, Writer},
    Infinitree,
};

use anyhow::Context;
use flume as mpsc;
use futures::future::join_all;
use ignore::{DirEntry, WalkBuilder};
use memmap2::{Mmap, MmapOptions};
use tokio::{fs, io::AsyncReadExt, task};
use tracing::{error, trace, warn};

type Sender = mpsc::Sender<(fs::File, files::Entry)>;
type Receiver = mpsc::Receiver<(fs::File, files::Entry)>;

const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

#[derive(clap::Args, Debug, Default, Clone)]
pub struct Options {
    /// The paths to include in the commit. All changes (addition/removal) will be committed.
    pub paths: Vec<String>,

    /// Preserve permissions.
    #[clap(
        short = 'p',
        long = "preserve-permissions",
        default_value = "true",
        parse(try_from_str)
    )]
    pub preserve_permissions: bool,

    /// Preserve owner/gid information.
    #[clap(
        short = 'o',
        long = "preserve-ownership",
        default_value = "true",
        parse(try_from_str)
    )]
    pub preserve_ownership: bool,

    /// Ignore files larger than the given value in bytes.
    #[clap(short = 'm', long = "max-size")]
    pub max_size: Option<u64>,

    /// Do not cross file system boundaries during directory walk.
    #[clap(short = 'x', long = "same-file-system")]
    pub same_fs: bool,

    /// Ignore hidden files.
    #[clap(short = 'h', long = "ignore-hidden")]
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
        let (sender, workers) = start_workers(stash, threads)?;
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
                Ok(md) if md.is_file() => md,
                Err(error) => {
                    warn!(%error, ?path, "failed to stat file; skipping");
                    continue;
                }
                _ => continue,
            };

            let osfile = match fs::File::open(&path).await {
                Ok(f) => f,
                Err(error) => {
                    warn!(%error, ?path, "failed to open file; skipping");
                    continue;
                }
            };

            let entry = match files::Entry::from_metadata(
                metadata,
                &path,
                self.preserve_permissions,
                self.preserve_ownership,
            ) {
                Ok(e) => e,
                Err(error) => {
                    error!(%error, ?path, "failed to ingest file; aborting");
                    break;
                }
            };

            trace!(?path, "processed");
            current_file_list.push(entry.name.clone());
            sender.send((osfile, entry)).unwrap();
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
) -> anyhow::Result<(Sender, Vec<task::JoinHandle<()>>)> {
    // make sure the input and output queues are generous
    let (sender, receiver) = mpsc::bounded(threads * 2);
    let balancer = RoundRobinBalancer::new(stash.object_writer()?, threads)?;

    let workers = (0..threads)
        .map(|_| {
            task::spawn(process_file_loop(
                receiver.clone(),
                stash.index().clone(),
                balancer.clone(),
            ))
        })
        .collect::<Vec<_>>();

    Ok((sender, workers))
}

async fn process_file_loop(
    r: Receiver,
    index: crate::Files,
    mut writer: RoundRobinBalancer<impl object::Writer + Clone + 'static>,
) {
    let fileindex = &index.files;
    let chunkindex = &index.chunks;
    let mut buf = Vec::with_capacity(MAX_FILE_SIZE);

    while let Ok((mut osfile, mut entry)) = r.recv_async().await {
        buf.clear();

        if let Some(in_store) = fileindex.get(&entry.name) {
            if in_store.as_ref() == &entry {
                continue;
            }
        }

        if entry.size == 0 {
            fileindex.insert(entry.name.clone(), entry);
            continue;
        }

        if entry.size < MAX_FILE_SIZE as u64 {
            osfile.read_to_end(&mut buf).await.unwrap();
        }

        let size = entry.size as usize;
        let mut mmap = MmappedFile::new(size, osfile.into_std().await);

        let splitter = if size < MAX_FILE_SIZE {
            FileSplitter::<SeaSplit>::new(&buf[0..size])
        } else {
            FileSplitter::<SeaSplit>::new(mmap.open())
        };
        let chunks = splitter.map(|(start, hash, data)| {
            let store = || writer.write_chunk(&hash, data).unwrap();
            let ptr = chunkindex.insert_with(hash, store);
            (start, ptr)
        });

        entry.chunks.extend(chunks);

        fileindex.insert(entry.name.clone(), entry);
    }
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
