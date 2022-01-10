#[macro_use]
extern crate serde_derive;

use infinitree::{fields::QueryAction, *};
use std::path::{Path, PathBuf};

mod files;
pub mod rollsum;
pub mod splitter;
mod stash;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type ChunkIndex = fields::VersionedMap<Digest, ChunkPointer>;
type FileSet = fields::VersionedMap<PathBuf, files::Entry>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    #[infinitree(strategy = "infinitree::fields::SparseField")]
    pub files: FileSet,
}

impl Files {
    fn list<'a>(
        &'a self,
        stash: &'a Infinitree<Files>,
        glob: &'a [impl AsRef<str>],
    ) -> stash::restore::FileIterator<'a> {
        let matchers = glob
            .iter()
            .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
            .collect::<Vec<glob::Pattern>>();

        use QueryAction::{Skip, Take};
        Box::new(
            stash
                .iter(self.files(), move |fname| {
                    if matchers.iter().any(|m| m.matches(&fname.to_string_lossy())) {
                        Take
                    } else {
                        Skip
                    }
                })
                .unwrap()
                .map(|(_, v)| v.unwrap()),
        )
    }

    pub async fn add_recursive(
        &self,
        stash: &Infinitree<Files>,
        threads: usize,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        stash::store::recursive(threads, &self, stash.object_writer()?, path).await;

        Ok(())
    }

    pub async fn restore_by_glob(
        &self,
        stash: &Infinitree<Files>,
        threads: usize,
        pattern: &[impl AsRef<str>],
        target: impl AsRef<Path>,
    ) -> Result<()> {
        stash::restore::from_iter(
            threads,
            self.list(stash, pattern),
            stash.object_reader()?,
            target,
        )
        .await;

        Ok(())
    }
}
