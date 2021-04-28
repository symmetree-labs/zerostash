use crate::backends::{Backend, BackendError};
use crate::compress;
use crate::crypto::CryptoProvider;
use crate::meta::{MetaObjectField, MetaObjectHeader, ObjectIndex};
use crate::object::{BlockBuffer, Object, ObjectId};

use thiserror::Error;

use std::io::{self, Cursor};
use std::sync::Arc;

#[derive(Error, Debug)]
pub enum ReadError {
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
    #[error("Failed to decode header")]
    InvalidHeader,
    #[error("No field found in header")]
    NoField,
    #[error("No header found in object")]
    NoHeader,
}
pub type Result<T> = std::result::Result<T, ReadError>;

pub struct Reader<C> {
    inner: Object<BlockBuffer>,
    header: Option<MetaObjectHeader>,
    objects: ObjectIndex,
    backend: Arc<dyn Backend>,
    crypto: C,
}

impl<C> Reader<C>
where
    C: CryptoProvider,
{
    pub fn new(backend: Arc<dyn Backend>, crypto: C) -> Reader<C> {
        Reader {
            inner: Object::default(),
            objects: ObjectIndex::default(),
            header: None,
            backend,
            crypto,
        }
    }

    pub async fn open(&mut self, id: &ObjectId) -> Result<MetaObjectHeader> {
        let obj = self.backend.read_object(id).await?;

        self.inner.reset_cursor();
        self.inner.set_id(*id);
        self.crypto.decrypt_object_into(&mut self.inner, &obj);

        let mut de = serde_cbor::Deserializer::from_slice(self.inner.as_ref()).into_iter();
        self.header = de.next().ok_or(ReadError::InvalidHeader)?.ok();

        self.header.clone().ok_or(ReadError::NoHeader)
    }

    pub async fn read_into<F: MetaObjectField>(&mut self, store: &mut F) -> Result<()> {
        match self.header {
            None => Err(ReadError::NoHeader),
            Some(ref header) => {
                let frame_start = header.get_offset(&F::key()).ok_or(ReadError::NoField)? as usize;

                let buffer: &[u8] = self.inner.as_ref();
                let decompress =
                    compress::destream(Cursor::new(&buffer[frame_start..header.end()]))?;

                let mut reader = serde_cbor::Deserializer::from_reader(decompress);

                store.deserialize(&mut reader).await;
                Ok(())
            }
        }
    }
}
