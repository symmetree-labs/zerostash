use infinitree::Infinitree;
use zerostash_files::Files;

pub fn migration(stash: &mut Infinitree<Files>) {
    stash.load(stash.index().directory_tree()).unwrap();
    stash.load(stash.index().files()).unwrap();

    println!("Attempting migration!");

    let mut count = 0;

    stash.index().files.for_each(|k, v| {
        count += 1;
        let filename = path_to_filename(k);
        let mut entry = v.clone();
        entry.name = filename;
        let tree = &stash.index().directory_tree;
        tree.write().insert_file(k, entry);
    });

    if count != 0 {
        stash.index().files.retain(|_, _| false);

        stash
            .commit("Files migration")
            .expect("Failed to write metadata");
        stash.backend().sync().expect("Failed to write to storage");
    }

    println!("Migrated {} files", count);
}

fn path_to_filename(path: &str) -> String {
    path.split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap()
        .to_string()
}

#[cfg(test)]
mod tests {
    use crate::migration::path_to_filename;

    #[test]
    fn test_path_to_filename() {
        assert_eq!(
            path_to_filename("/path/to/random/test.rs"),
            "test.rs".to_string()
        );
    }
}
