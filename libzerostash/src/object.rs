use crate::{
    backends::BackendError,
    chunks::ChunkPointer,
    compress::{CompressError, DecompressError},
    crypto::CryptoDigest,
};

use async_trait::async_trait;
use thiserror::Error;

use std::{io, sync::Arc};

mod reader;
pub use reader::{AEADReader, Reader};

mod writer;
pub use writer::{AEADWriter, Writer};

mod object;
pub use object::{BlockBuffer, Object, ReadBuffer, ReadObject, WriteObject};

mod id;
pub use id::ObjectId;

pub mod write_balancer;

#[derive(Error, Debug)]
pub enum ObjectError {
    #[error("IO error")]
    Io {
        #[from]
        source: io::Error,
    },
    #[error("Backend error")]
    Backend {
        #[from]
        source: BackendError,
    },
    #[error("Compress failed")]
    Compress {
        #[from]
        source: CompressError,
    },
    #[error("Decompress failed")]
    Decompress {
        #[from]
        source: DecompressError,
    },
}

pub type Result<T> = std::result::Result<T, ObjectError>;

pub mod test {
    use super::*;

    #[derive(Clone, Default)]
    pub struct NullStorage(Arc<std::sync::Mutex<usize>>);

    #[async_trait]
    impl Writer for NullStorage {
        fn write_chunk(&mut self, _hash: &CryptoDigest, data: &[u8]) -> Result<ChunkPointer> {
            *self.0.lock().unwrap() += data.len();
            Ok(Arc::default())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }
}
