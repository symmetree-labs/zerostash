#[macro_use]
extern crate serde_derive;

use infinitree::{fields::QueryAction, *};

mod files;
pub mod rollsum;
pub mod splitter;
mod stash;

pub use stash::restore;
pub use stash::store;

type ChunkIndex = fields::VersionedMap<Digest, ChunkPointer>;
type FileSet = fields::VersionedMap<String, files::Entry>;

#[derive(Clone, Default, Index)]
pub struct Files {
    pub chunks: ChunkIndex,
    #[infinitree(strategy = "infinitree::fields::SparseField")]
    pub files: FileSet,
}

impl Files {
    pub fn list<'a>(
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
                    if matchers.iter().any(|m| m.matches(fname)) {
                        Take
                    } else {
                        Skip
                    }
                })
                .unwrap()
                .map(|(_, v)| v.unwrap()),
        )
    }
}
