use super::{Decoder, FieldReader, Header};
use crate::{
    backends::{Backend, BackendError},
    compress,
    crypto::{CryptoProvider, IndexKey},
    deserialize_from_slice,
    object::{BlockBuffer, Object, ObjectId},
    Deserializer,
};

use serde::de::DeserializeOwned;
use thiserror::Error;

use std::{
    io::{self, Cursor},
    sync::Arc,
};

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
    #[error("No more data can be read")]
    EndOfList,
}
pub type Result<T> = std::result::Result<T, ReadError>;

#[derive(Clone)]
pub struct Reader {
    inner: Arc<Object<BlockBuffer>>,
    header: Option<Header>,
    backend: Arc<dyn Backend>,
    crypto: IndexKey,
}

impl Reader {
    pub(crate) fn new(backend: Arc<dyn Backend>, crypto: IndexKey) -> Self {
        Reader {
            inner: Arc::default(),
            header: None,
            backend,
            crypto,
        }
    }

    pub(crate) fn open(&mut self, id: &ObjectId) -> Result<Header> {
        let obj = self.backend.read_object(id)?;

        let buffer = Arc::make_mut(&mut self.inner);
        buffer.reset_cursor();
        buffer.set_id(*id);

        self.header = None;
        self.crypto
            .decrypt_object_into(buffer.as_inner_mut(), obj.as_inner(), obj.id());

        self.header =
            deserialize_from_slice(buffer.as_inner()).map_err(|_| ReadError::InvalidHeader)?;
        self.header.clone().ok_or(ReadError::NoHeader)
    }

    pub(crate) fn field(&self, name: &str) -> Result<Decoder> {
        match self.header {
            None => Err(ReadError::NoHeader),
            Some(ref header) => {
                let frame_start = header.get_offset(name).ok_or(ReadError::NoField)? as usize;

                let buffer: &[u8] = self.inner.as_inner();
                let decompress =
                    compress::destream(Cursor::new(buffer[frame_start..header.end()].to_vec()));

                let reader = Deserializer::new(decompress);
                Ok(reader)
            }
        }
    }

    pub fn transaction(&self, field: impl Into<String>, id: &ObjectId) -> Result<Transaction> {
        Transaction::new(self.clone(), field.into(), id)
    }
}

pub struct Transaction {
    reader: Reader,
    field: String,
    decoder: Decoder,
    header: Header,
}

impl Transaction {
    fn new(mut reader: Reader, field: String, id: &ObjectId) -> Result<Self> {
        let header = reader.open(id)?;
        let decoder = reader.field(&field)?;

        Ok(Self {
            reader,
            field,
            header,
            decoder,
        })
    }
}

impl FieldReader for Transaction {
    fn read_next<T: DeserializeOwned>(
        &mut self,
    ) -> std::result::Result<T, Box<dyn std::error::Error>> {
        let next = self.decoder.read_next();
        match next {
            Ok(val) => Ok(val),
            Err(_) => {
                let next_object = &self
                    .header
                    .next_object(&self.field)
                    .ok_or(ReadError::EndOfList)?;
                self.header = self.reader.open(next_object)?;
                self.decoder = self.reader.field(&self.field)?;

                self.decoder.read_next()
            }
        }
    }
}

impl AsRef<Self> for Reader {
    fn as_ref(&self) -> &Self {
        self
    }
}
