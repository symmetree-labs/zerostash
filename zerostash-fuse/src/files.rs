use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
}

impl Default for FileType {
    fn default() -> Self {
        Self::File
    }
}
