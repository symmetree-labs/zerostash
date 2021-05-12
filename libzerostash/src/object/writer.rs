use super::{ObjectId, Result, WriteObject};
use crate::{
    backends::Backend,
    chunks::{ChunkPointer, RawChunkPointer},
    compress,
    crypto::{CryptoDigest, CryptoProvider, Random},
};

use std::{io::Write, sync::Arc};

pub trait Writer: Send + Clone {
    fn write_chunk(&mut self, hash: &CryptoDigest, data: &[u8]) -> Result<ChunkPointer>;
    fn flush(&mut self) -> Result<()>;
}

pub struct AEADWriter<C> {
    backend: Arc<dyn Backend>,
    crypto: C,
    object: WriteObject,
}

impl<C> AEADWriter<C>
where
    C: CryptoProvider,
{
    pub fn new(backend: Arc<dyn Backend>, crypto: C) -> AEADWriter<C> {
        let mut object = WriteObject::default();
        object.reset_id(&crypto);

        AEADWriter {
            backend,
            crypto,
            object,
        }
    }
}

impl<C> Clone for AEADWriter<C>
where
    C: Random + Clone,
{
    fn clone(&self) -> AEADWriter<C> {
        let mut object = self.object.clone();
        object.set_id(ObjectId::new(&self.crypto));

        AEADWriter {
            object,
            backend: self.backend.clone(),
            crypto: self.crypto.clone(),
        }
    }
}

impl<C> Writer for AEADWriter<C>
where
    C: CryptoProvider + Sync,
{
    fn write_chunk(&mut self, hash: &CryptoDigest, data: &[u8]) -> Result<ChunkPointer> {
        let mut compressed = compress::block(&data)?;
        let size = compressed.len();
        let mut offs = self.object.position();
        if offs + size > self.object.capacity() {
            self.flush()?;
            offs = self.object.position();
        }

        let tag = self
            .crypto
            .encrypt_chunk(&self.object, hash, &mut compressed);

        self.object.write_all(&compressed)?;

        Ok(Arc::new(RawChunkPointer {
            offs: offs as u32,
            size: size as u32,
            file: *self.object.id(),
            hash: *hash,
            tag,
        }))
    }

    fn flush(&mut self) -> Result<()> {
        self.object.finalize(&self.crypto);
        self.backend.write_object(&self.object)?;

        self.object.reset_id(&self.crypto);
        self.object.reset_cursor();

        Ok(())
    }
}