use crate::{
    compress,
    object::{ObjectId, WriteObject},
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io::Cursor,
};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod fields;
mod header;
mod reader;
mod writer;

pub use fields::*;
pub use header::*;
pub use reader::{ReadError, Reader};
pub use writer::{WriteError, Writer};

type Encoder = compress::Encoder<WriteObject>;
type Decoder<'b> =
    serde_cbor::Deserializer<serde_cbor::de::IoRead<compress::Decoder<Cursor<&'b [u8]>>>>;
pub type ObjectIndex = HashMap<Field, HashSet<ObjectId>>;

#[async_trait]
pub trait Index {
    async fn read_fields(
        &mut self,
        metareader: reader::Reader,
        start_object: ObjectId,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn write_fields(
        &mut self,
        metareader: &mut writer::Writer,
    ) -> Result<(), Box<dyn std::error::Error>>;
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

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn can_deserialize_fields() {
        use crate::backends;
        use crate::chunks::{ChunkIndex, ChunkPointer};
        use crate::crypto::{self, CryptoDigest};
        use crate::object::ObjectId;

        use secrecy::Secret;
        use std::sync::Arc;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");

        let crypto = crypto::ObjectOperations::new(key);
        let storage = Arc::new(backends::test::InMemoryBackend::default());
        let oid = ObjectId::new(&crypto);
        let mut mw = super::Writer::new(oid, storage.clone(), crypto.clone()).unwrap();

        let chunks = ChunkIndex::default();
        chunks
            .entry(CryptoDigest::default())
            .or_insert_with(|| ChunkPointer::default());

        mw.write_field("chunks", &chunks).await;
        mw.seal_and_store().await;

        let mut mr = super::Reader::new(storage, crypto);
        let objects = mw.objects().get("chunks").unwrap();
        assert_eq!(objects.len(), 1);

        for id in objects.iter() {
            mr.open(&id).await.unwrap();
        }

        let mut chunks_restore = ChunkIndex::default();
        mr.read_into("chunks", &mut chunks_restore).await.unwrap();

        assert_eq!(chunks_restore.len(), 1);
    }
}
