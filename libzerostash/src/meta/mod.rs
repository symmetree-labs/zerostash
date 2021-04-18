use crate::compress;
use crate::objects::{ObjectId, WriteObject};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io::Cursor;

type Encoder = compress::Encoder<WriteObject>;
type Decoder<'b> =
    serde_cbor::Deserializer<serde_cbor::de::IoRead<compress::Decoder<Cursor<&'b [u8]>>>>;
pub type ObjectIndex = HashMap<Field, HashSet<ObjectId>>;

// Header size max 512b
const HEADER_SIZE: usize = 512;

mod reader;
mod writer;

pub use reader::{ReadError, Reader};
pub use writer::{WriteError, Writer};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum MetaObjectHeader {
    V1 {
        next_object: Option<ObjectId>,
        offsets: Vec<FieldOffset>,
        end: usize,
    },
}

impl MetaObjectHeader {
    fn new(
        next_object: Option<ObjectId>,
        offsets: impl AsRef<[FieldOffset]>,
        end: usize,
    ) -> MetaObjectHeader {
        MetaObjectHeader::V1 {
            offsets: offsets.as_ref().to_vec(),
            next_object,
            end,
        }
    }

    pub fn next_object(&self) -> Option<ObjectId> {
        match self {
            MetaObjectHeader::V1 {
                ref next_object, ..
            } => *next_object,
        }
    }

    pub fn fields(&self) -> Vec<Field> {
        match self {
            MetaObjectHeader::V1 { ref offsets, .. } => {
                offsets.iter().map(FieldOffset::as_field).collect()
            }
        }
    }

    fn end(&self) -> usize {
        match self {
            MetaObjectHeader::V1 { ref end, .. } => *end,
        }
    }

    fn get_offset(&self, field: &str) -> Option<u32> {
        match self {
            MetaObjectHeader::V1 { ref offsets, .. } => {
                for fo in offsets.iter() {
                    if fo.as_field() == field {
                        return Some(fo.into());
                    }
                }
                None
            }
        }
    }
}

#[async_trait]
pub trait MetaObjectField {
    type Item: DeserializeOwned;

    fn key() -> String;
    async fn serialize(&self, mw: &mut impl FieldWriter);
    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>);

    fn as_offset(&self, offs: u32) -> FieldOffset {
        FieldOffset(offs, Self::key())
    }
}

#[async_trait]
pub trait FieldWriter: Send {
    async fn write_next(&mut self, obj: impl Serialize + Send + 'async_trait);
}

#[async_trait]
pub trait FieldReader<T>: Send {
    async fn read_next(&mut self) -> Result<T, Box<dyn Error>>;
}

#[async_trait]
impl<'b, T> FieldReader<T> for Decoder<'b>
where
    T: DeserializeOwned,
{
    async fn read_next(&mut self) -> Result<T, Box<dyn Error>> {
        Ok(T::deserialize(self)?)
    }
}

pub type Field = String;
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FieldOffset(u32, String);

impl From<&FieldOffset> for u32 {
    fn from(fo: &FieldOffset) -> u32 {
        fo.0
    }
}

impl FieldOffset {
    pub fn new(offs: u32, f: Field) -> Self {
        FieldOffset(offs, f)
    }

    fn as_field(&self) -> Field {
        self.1.to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn can_deserialize_fields() {
        use crate::backends;
        use crate::chunks::{self, ChunkPointer};
        use crate::crypto::{self, CryptoDigest};
        use crate::meta::{self, MetaObjectField};
        use crate::objects::ObjectId;

        use secrecy::Secret;
        use std::sync::Arc;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");

        let crypto = crypto::ObjectOperations::new(key);
        let storage = Arc::new(backends::test::InMemoryBackend::default());
        let oid = ObjectId::new(&crypto);
        let mut mw = meta::Writer::new(oid, storage.clone(), crypto.clone()).unwrap();

        let chunks = chunks::ChunkStore::default();
        chunks
            .push(CryptoDigest::default(), || {
                Ok(Arc::new(ChunkPointer::default()))
            })
            .unwrap();

        mw.write_field(&chunks);
        mw.seal_and_store();

        let mut mr = meta::Reader::new(storage, crypto);
        let objects = mw.objects().get(&chunks::ChunkStore::key()).unwrap();
        assert_eq!(objects.len(), 1);

        for id in objects.iter() {
            mr.open(&id).unwrap();
        }

        let mut chunks_restore = chunks::ChunkStore::default();
        mr.read_into(&mut chunks_restore).unwrap();

        assert_eq!(chunks_restore.index().len(), 1);
    }
}
