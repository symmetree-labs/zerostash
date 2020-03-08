use crate::backends::*;
use crate::compress;
use crate::crypto::CryptoProvider;
use crate::meta::{Field, MetaObjectField, MetaObjectHeader, ObjectIndex};
use crate::objects::{BlockBuffer, Object, ObjectId};

use thiserror::Error;

use std::borrow::Borrow;
use std::io::{self, Cursor};

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

pub struct Reader<B, C> {
    inner: Object<BlockBuffer>,
    header: Option<MetaObjectHeader>,
    objects: ObjectIndex,
    backend: B,
    crypto: C,
}

impl<B, C> Reader<B, C>
where
    B: Backend,
    C: CryptoProvider,
{
    pub fn new(backend: B, crypto: C) -> Reader<B, C> {
        Reader {
            inner: Object::default(),
            objects: ObjectIndex::default(),
            header: None,
            backend,
            crypto,
        }
    }

    pub fn open(&mut self, id: &ObjectId) -> Result<MetaObjectHeader> {
        let obj = self.backend.read_object(id)?;

        self.inner.reset_cursor();
        self.inner.set_id(*id);
        self.crypto.decrypt_object_into(&mut self.inner, &obj);

        let mut de = serde_cbor::Deserializer::from_slice(self.inner.as_ref()).into_iter();
        self.header = de.next().ok_or_else(|| ReadError::InvalidHeader)?.ok();

        self.header.clone().ok_or_else(|| ReadError::NoHeader)
    }

    pub fn read_into(
        &mut self,
        field: impl Borrow<Field>,
        store: &mut impl MetaObjectField,
    ) -> Result<()> {
        let field = field.borrow();

        match self.header {
            None => Err(ReadError::NoHeader),
            Some(ref header) => {
                let frame_start = header
                    .get_offset(&field)
                    .ok_or_else(|| ReadError::NoField)? as usize;

                let buffer: &[u8] = self.inner.as_ref();
                let decompress =
                    compress::destream(Cursor::new(&buffer[frame_start..header.end()]))?;

                let mut reader = serde_cbor::Deserializer::from_reader(decompress);

                store.deserialize(&mut reader);
                Ok(())
            }
        }
    }
}
