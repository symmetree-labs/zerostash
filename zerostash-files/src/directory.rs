use core::fmt;
use std::path::PathBuf;

use crate::FileType;

#[derive(Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Dir {
    pub path: PathBuf,
    pub file_type: FileType,
    pub inside: PathBuf,
}

impl fmt::Debug for Dir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            .field("path", &self.path)
            .field("inside", &self.inside)
            .field("type", &self.file_type)
            .finish()
    }
}

impl Dir {
    pub fn new(path: PathBuf, file_type: FileType) -> Self {
        let parent = path.parent().unwrap().to_path_buf();
        Self {
            path,
            file_type,
            inside: parent,
        }
    }
}
