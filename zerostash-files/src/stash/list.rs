use chrono::{DateTime, Utc};
use infinitree::{fields::QueryAction, Infinitree};
use itertools::Itertools;

use crate::Files;

#[derive(clap::Args, Debug, Clone, Default)]
pub struct List {
    /// List of globs to match in the database
    pub globs: Vec<String>,
}

impl List {
    pub fn list<'stash>(
        &'stash self,
        stash: &'stash Infinitree<Files>,
    ) -> impl Iterator<Item = (String, DateTime<Utc>)> + 'stash {
        let globs = if !self.globs.is_empty() {
            self.globs.clone()
        } else {
            vec!["*".into()]
        };

        iter(stash, globs)
    }
}

pub type SnapshotIterator<'a> = Box<(dyn Iterator<Item = (String, DateTime<Utc>)> + Send + 'a)>;

fn iter<V: AsRef<[T]>, T: AsRef<str>>(stash: &Infinitree<Files>, glob: V) -> SnapshotIterator {
    let matchers = glob
        .as_ref()
        .iter()
        .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
        .collect::<Vec<glob::Pattern>>();

    use QueryAction::{Skip, Take};
    Box::new(
        stash
            .iter(stash.index().snapshots(), move |snapname| {
                if matchers.iter().any(|m| m.matches(snapname)) {
                    Take
                } else {
                    Skip
                }
            })
            .unwrap()
            .filter(|(_, snap)| snap.is_some())
            .map(|(name, snap)| {
                let datetime: DateTime<Utc> = snap.unwrap().as_ref().into();
                (name, datetime)
            })
            .sorted_by(|(_, a_time), (_, b_time)| a_time.cmp(b_time)),
    )
}
