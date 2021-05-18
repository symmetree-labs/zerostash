#[macro_use]
extern crate serde_derive;

use libzerostash::*;
use libzerostash::{
    chunks::ChunkIndex,
    meta::{self, ReadError},
};

use std::path::Path;

mod files;
pub mod rollsum;
pub mod splitter;
mod stash;

use files::FileIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Default)]
pub struct FileStashIndex {
    chunks: ChunkIndex,
    files: files::FileIndex,
}

#[async_trait]
impl Index for FileStashIndex {
    async fn read_fields(
        &mut self,
        mut metareader: meta::Reader,
        start_object: ObjectId,
    ) -> Result<()> {
        let mut next_object = Some(start_object);

        while let Some(header) = match next_object {
            Some(ref o) => Some(metareader.open(o).await?),
            None => None,
        } {
            next_object = header.next_object();

            match metareader.read_into("files", &mut self.files).await {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };

            match metareader.read_into("chunks", &mut self.chunks).await {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };
        }

        Ok(())
    }

    async fn write_fields(&mut self, metawriter: &mut meta::Writer) -> Result<()> {
        metawriter.write_field("files", &self.files).await;
        metawriter.write_field("chunks", &self.chunks).await;
        metawriter.seal_and_store().await;

        Ok(())
    }
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

    pub fn chunks(&self) -> &ChunkIndex {
        &self.chunks
    }

    pub fn files(&self) -> &FileIndex {
        &self.files
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
