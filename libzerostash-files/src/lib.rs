#[macro_use]
extern crate serde_derive;

use libzerostash::chunks::ChunkIndex;
use libzerostash::*;

use std::path::Path;

mod files;
pub mod rollsum;
pub mod splitter;
mod stash;

use files::FileIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Default, Index)]
pub struct FileStashIndex {
    chunks: ChunkIndex,
    files: FileIndex,
}

impl FileStashIndex {
    pub fn list<'a>(&'a self, glob: &'a [impl AsRef<str>]) -> stash::restore::FileIterator<'a> {
        let matchers = glob
            .iter()
            .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
            .collect::<Vec<glob::Pattern>>();
        let base_iter = self.files.iter().map(|r| r.clone());

        match glob.len() {
            i if i == 0 => Box::new(base_iter),
            _ => Box::new(base_iter.filter(move |f| matchers.iter().any(|m| m.matches(&f.name)))),
        }
    }

    pub async fn add_recursive(
        &self,
        stash: &Stash<FileStashIndex>,
        threads: usize,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        stash::store::recursive(threads, &self, stash.object_writer()?, path).await;

        Ok(())
    }

    pub async fn restore_by_glob(
        &self,
        stash: &Stash<FileStashIndex>,
        threads: usize,
        pattern: &[impl AsRef<str>],
        target: impl AsRef<Path>,
    ) -> Result<()> {
        stash::restore::from_iter(threads, self.list(pattern), stash.object_reader()?, target)
            .await;

        Ok(())
    }
}
