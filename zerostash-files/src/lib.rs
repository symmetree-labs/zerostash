#[macro_use]
extern crate serde_derive;

use infinitree::{
    index::{ChunkIndex, QueryAction},
    *,
};

use std::path::Path;

mod files;
pub mod rollsum;
pub mod splitter;
mod stash;

use files::FileSet;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    #[infinitree(strategy = "infinitree::index::SparseField")]
    pub files: FileSet,
}

impl Files {
    fn list<'a>(
        &'a self,
        stash: &Infinitree<Files>,
        glob: &'a [impl AsRef<str>],
    ) -> stash::restore::FileIterator<'a> {
        let matchers = glob
            .iter()
            .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
            .collect::<Vec<glob::Pattern>>();

        use QueryAction::{Skip, Take};
        Box::new(
            stash
                .query(self.files(), move |fname| {
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
