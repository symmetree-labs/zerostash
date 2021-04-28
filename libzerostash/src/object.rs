use crate::{backends::BackendError, chunks::ChunkPointer, crypto::CryptoDigest};

use async_trait::async_trait;
use thiserror::Error;

use std::{io, sync::Arc};

mod store;
pub use store::{Storage, Writer};

mod object;
pub use object::{BlockBuffer, Object, ReadBuffer, ReadObject, WriteObject};

mod id;
pub use id::ObjectId;

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
}

pub type Result<T> = std::result::Result<T, ObjectError>;

pub mod test {
    use super::*;

    #[derive(Clone, Default)]
    pub struct NullStorage(Arc<tokio::sync::Mutex<usize>>);

    #[async_trait]
    impl Writer for NullStorage {
        async fn write_chunk(
            &mut self,
            _hash: &CryptoDigest,
            data: &[u8],
        ) -> Result<Arc<ChunkPointer>> {
            *self.0.lock().await += data.len();
            Ok(Arc::default())
        }

        async fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }
}
