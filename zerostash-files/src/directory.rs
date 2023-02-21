use crate::files::FileType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct Dir {
    pub path: PathBuf,
    pub file_type: FileType,
}

impl Dir {
    pub fn new(path: PathBuf, file_type: FileType) -> Self {
        Self { path, file_type }
    }
}
