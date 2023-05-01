use chrono::{DateTime, Utc};
use infinitree::{fields::QueryAction, Infinitree};
use itertools::Itertools;

use crate::Files;

#[derive(clap::Args, Debug, Clone, Default)]
pub struct ZfsSnapshotList {
    /// List of globs to match in the database
    pub globs: Vec<String>,
}

impl ZfsSnapshotList {
    pub fn list<'stash>(
        &'stash self,
        stash: &'stash Infinitree<Files>,
    ) -> impl Iterator<Item = (String, DateTime<Utc>)> + 'stash {
        let globs = if !self.globs.is_empty() {
            self.globs.clone()
        } else {
            vec!["*".into()]
        };

        iter(stash, globs.into_iter())
    }
}

type SnapshotIterator<'a> = Box<(dyn Iterator<Item = (String, DateTime<Utc>)> + Send + 'a)>;

fn iter<V: Iterator<Item = T>, T: AsRef<str>>(
    stash: &Infinitree<Files>,
    globs: V,
) -> SnapshotIterator {
    let matchers = globs
        .map(|glob| glob::Pattern::new(glob.as_ref()).unwrap())
        .collect::<Vec<glob::Pattern>>();

    use QueryAction::{Skip, Take};
    Box::new(
        stash
            .iter(stash.index().zfs_snapshots(), move |snapname| {
                if matchers.iter().any(|m| m.matches(snapname)) {
                    Take
                } else {
                    Skip
                }
            })
            .unwrap()
            .filter_map(|(name, snap)| snap.map(|s| (name, DateTime::<Utc>::from(s.as_ref()))))
            .sorted_by_key(|(_, time)| *time),
    )
}
