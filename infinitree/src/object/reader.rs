use super::{Result, WriteObject};
use crate::{
    backends::Backend,
    chunks::ChunkPointer,
    compress,
    crypto::{ChunkKey, CryptoProvider},
};

use std::sync::Arc;

pub trait Reader: Send + Clone {
    fn read_chunk(&mut self, pointer: ChunkPointer, target: &mut [u8]) -> Result<()>;
}

#[derive(Clone)]
pub struct AEADReader {
    backend: Arc<dyn Backend>,
    crypto: ChunkKey,
    buffer: WriteObject,
}

impl AEADReader {
    pub fn new(backend: Arc<dyn Backend>, crypto: ChunkKey) -> Self {
        AEADReader {
            backend,
            crypto,
            buffer: WriteObject::default(),
        }
    }
}

impl Reader for AEADReader {
    fn read_chunk(&mut self, pointer: ChunkPointer, target: &mut [u8]) -> Result<()> {
        let object = self.backend.read_object(&pointer.file)?;
        let mut cryptbuf: &mut [u8] = self.buffer.as_inner_mut();

        let buf =
            self.crypto
                .decrypt_chunk(&mut cryptbuf, object.as_inner(), object.id(), &pointer);
        compress::decompress_into(buf, target, 0)?;
        Ok(())
    }
}
