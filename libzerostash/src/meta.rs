use crate::compress;
use crate::object::{ObjectId, WriteObject};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io::Cursor;

mod reader;
mod writer;

pub use reader::{ReadError, Reader};
pub use writer::{WriteError, Writer};

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn can_deserialize_fields() {
        use crate::backends;
        use crate::chunks::{self, ChunkPointer};
        use crate::crypto::{self, CryptoDigest};
        use crate::meta;
        use crate::object::ObjectId;

        use secrecy::Secret;
        use std::sync::Arc;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");

        let crypto = crypto::ObjectOperations::new(key);
        let storage = Arc::new(backends::test::InMemoryBackend::default());
        let oid = ObjectId::new(&crypto);
        let mut mw = meta::Writer::new(oid, storage.clone(), crypto.clone()).unwrap();

        let chunks = chunks::ChunkIndex::default();
        chunks
            .entry(CryptoDigest::default())
            .or_insert_with(|| ChunkPointer::default());

        mw.write_field("chunks", &chunks).await;
        mw.seal_and_store().await;

        let mut mr = meta::Reader::new(storage, crypto);
        let objects = mw.objects().get("chunks").unwrap();
        assert_eq!(objects.len(), 1);

        for id in objects.iter() {
            mr.open(&id).await.unwrap();
        }

        let mut chunks_restore = chunks::ChunkIndex::default();
        mr.read_into("chunks", &mut chunks_restore).await.unwrap();

        assert_eq!(chunks_restore.len(), 1);
    }
}
