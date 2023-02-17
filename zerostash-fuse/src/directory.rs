use std::path::{Path, PathBuf};

use infinitree::fields::VersionedMap;

use crate::{dir::Dir, files::FileType};

pub fn walk_dir_up(index: &VersionedMap<PathBuf, Vec<Dir>>, path: PathBuf) {
    if let Some(parent) = path.parent() {
        let dir = Dir::new(path.clone(), FileType::Directory);
        match index.get(parent) {
            Some(parent_map) => {
                if !parent_map.contains(&dir) {
                    let mut vec = parent_map.to_vec();
                    vec.push(dir);
                    index.update_with(parent.to_path_buf(), |_| vec.to_vec());
                }
            }
            None => {
                index.insert(parent.to_path_buf(), vec![dir]);
            }
        }
        walk_dir_up(index, parent.to_path_buf());
    }
}

pub fn insert_directories(index: &VersionedMap<PathBuf, Vec<Dir>>, path: &Path, file: Dir) {
    let parent = path.parent().unwrap();
    match index.get(parent) {
        Some(parent_map) => {
            if !parent_map.contains(&file) {
                let mut vec = parent_map.to_vec();
                vec.push(file);
                index.update_with(parent.to_path_buf(), |_| vec.to_vec());
            }
        }
        None => {
            index.insert(parent.to_path_buf(), vec![file]);
        }
    }
}
