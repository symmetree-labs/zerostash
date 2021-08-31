use crate::{
    backends::BackendError,
    compress::{CompressError, DecompressError},
    crypto::{Random, Tag},
    BLOCK_SIZE,
};

use thiserror::Error;

use std::{io, mem::size_of};

mod reader;
pub use reader::{AEADReader, Reader};

mod writer;
pub use writer::{AEADWriter, Writer};

pub mod serializer;

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
    #[error("Chunk too large to be written: {size}, max: {max_size}")]
    ChunkTooLarge { max_size: usize, size: usize },
    #[error("Serialize failed")]
    Serialize { source: Box<dyn std::error::Error> },
    #[error("Deserialize failed")]
    Deserialize { source: Box<dyn std::error::Error> },
}

pub type Result<T> = std::result::Result<T, ObjectError>;

pub type WriteObject = Object<BlockBuffer>;
pub type ReadObject = Object<ReadBuffer>;

#[derive(Clone)]
pub struct BlockBuffer(Box<[u8]>);
pub struct ReadBuffer(ReadBufferInner);
type ReadBufferInner = Box<dyn AsRef<[u8]> + Send + Sync + 'static>;

impl<RO> From<RO> for WriteObject
where
    RO: AsRef<ReadObject>,
{
    fn from(rwr: RO) -> WriteObject {
        let rw = rwr.as_ref();

        Object::with_id(
            rw.id,
            BlockBuffer(rw.buffer.as_ref().to_vec().into_boxed_slice()),
        )
    }
}

impl<WO> From<WO> for ReadObject
where
    WO: AsRef<WriteObject>,
{
    fn from(rwr: WO) -> ReadObject {
        let rw = rwr.as_ref();

        Object::with_id(
            rw.id,
            ReadBuffer(Box::new(rw.buffer.clone()) as ReadBufferInner),
        )
    }
}

impl ReadBuffer {
    pub fn new(buf: impl AsRef<[u8]> + Send + Sync + 'static) -> ReadBuffer {
        ReadBuffer(Box::new(buf) as ReadBufferInner)
    }
}

impl AsRef<[u8]> for ReadBuffer {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref().as_ref()
    }
}

impl Default for BlockBuffer {
    #[inline]
    fn default() -> BlockBuffer {
        BlockBuffer(vec![0; BLOCK_SIZE].into_boxed_slice())
    }
}

impl AsMut<[u8]> for BlockBuffer {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}

impl AsRef<[u8]> for BlockBuffer {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

pub struct Object<T> {
    id: ObjectId,
    buffer: T,
    capacity: usize,
    cursor: usize,
}

impl<T> Object<T> {
    pub fn new(buffer: T) -> Self {
        Object {
            id: ObjectId::default(),
            cursor: 0,
            capacity: BLOCK_SIZE,
            buffer,
        }
    }

    pub fn reset_id(&mut self, random: &impl Random) {
        self.id.reset(random)
    }
}

impl<T> Object<T> {
    #[inline(always)]
    pub fn id(&self) -> &ObjectId {
        &self.id
    }

    #[inline(always)]
    pub fn set_id(&mut self, id: ObjectId) {
        self.id = id;
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline(always)]
    pub fn position(&self) -> usize {
        self.cursor
    }

    #[inline(always)]
    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
    }

    pub fn reserve_tag(&mut self) {
        self.capacity = BLOCK_SIZE - size_of::<Tag>();
    }
}

impl<T> Object<T>
where
    T: AsRef<[u8]>,
{
    #[inline(always)]
    pub fn as_inner(&self) -> &[u8] {
        self.buffer.as_ref()
    }

    pub fn with_id(id: ObjectId, buffer: T) -> Object<T> {
        let mut object = Object {
            id: ObjectId::default(),
            cursor: 0,
            capacity: buffer.as_ref().len(),
            buffer,
        };
        object.set_id(id);
        object
    }
}

impl<T> Object<T>
where
    T: AsMut<[u8]>,
{
    #[inline(always)]
    pub fn as_inner_mut(&mut self) -> &mut [u8] {
        self.buffer.as_mut()
    }

    #[inline]
    pub fn clear(&mut self) {
        for i in self.buffer.as_mut().iter_mut() {
            *i = 0;
        }
    }

    #[inline(always)]
    pub fn tail_mut(&mut self) -> &mut [u8] {
        &mut self.buffer.as_mut()[self.cursor..self.capacity]
    }

    pub fn position_mut(&mut self) -> &mut usize {
        &mut self.cursor
    }

    #[inline(always)]
    pub fn write_tag(&mut self, buf: &[u8]) {
        self.buffer.as_mut()[self.capacity..].copy_from_slice(buf);
    }

    #[inline(always)]
    pub fn write_head(&mut self, buf: &[u8]) {
        self.buffer.as_mut()[..buf.len()].copy_from_slice(buf);
    }

    #[inline(always)]
    pub fn finalize(&mut self, random: &impl Random) {
        random.fill(&mut self.buffer.as_mut()[self.cursor..])
    }
}

impl<T> io::Write for Object<T>
where
    T: AsMut<[u8]>,
{
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let ofs = self.cursor;
        let len = buf.len();

        self.buffer.as_mut()[ofs..(ofs + len)].copy_from_slice(buf);
        self.cursor += len;

        Ok(len)
    }

    #[inline(always)]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T> io::Read for Object<T>
where
    T: AsRef<[u8]>,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = buf.len() + self.cursor;
        let inner = self.as_inner();

        if end > inner.len() {
            Err(io::Error::from(io::ErrorKind::UnexpectedEof))
        } else {
            buf.copy_from_slice(&inner[self.cursor..end]);
            self.cursor = end;
            Ok(buf.len())
        }
    }
}

impl<T> io::Seek for Object<T> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        use io::SeekFrom::*;

        let umax = self.capacity as u64;
        let imax = self.capacity as i64;

        match pos {
            Start(s) => match s {
                s if s > umax => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                s => {
                    self.cursor = s as usize;
                    Ok(self.cursor as u64)
                }
            },
            End(e) => match e {
                e if e < 0 => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                e if e > imax => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                e => {
                    self.cursor = self.capacity - e as usize;
                    Ok(self.cursor as u64)
                }
            },
            Current(c) => {
                let new_pos = self.cursor as i64 + c;

                match new_pos {
                    p if p < 0 => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                    p if p > imax => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                    p => {
                        self.cursor = p as usize;
                        Ok(self.cursor as u64)
                    }
                }
            }
        }
    }
}

impl<T> Clone for Object<T>
where
    T: Clone,
{
    fn clone(&self) -> Object<T> {
        Object {
            id: self.id,
            buffer: self.buffer.clone(),
            capacity: self.capacity,
            cursor: self.cursor,
        }
    }
}

impl<T> Default for Object<T>
where
    T: Default + AsRef<[u8]>,
{
    fn default() -> Object<T> {
        let buffer = T::default();
        Object {
            id: ObjectId::default(),
            cursor: 0,
            capacity: buffer.as_ref().len(),
            buffer,
        }
    }
}

impl<T> AsRef<[u8]> for Object<T>
where
    T: AsRef<[u8]>,
{
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        &self.buffer.as_ref()[..self.capacity]
    }
}

impl<T> AsMut<[u8]> for Object<T>
where
    T: AsMut<[u8]>,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buffer.as_mut()[..self.capacity]
    }
}

impl<T> AsRef<Object<T>> for Object<T> {
    #[inline(always)]
    fn as_ref(&self) -> &Object<T> {
        self
    }
}

#[cfg(test)]
pub mod test {
    use crate::{ChunkPointer, Digest};

    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    pub struct NullStorage(Arc<std::sync::Mutex<usize>>);

    impl Writer for NullStorage {
        fn write_chunk(&mut self, _hash: &Digest, data: &[u8]) -> Result<ChunkPointer> {
            *self.0.lock().unwrap() += data.len();
            Ok(ChunkPointer::default())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }
}
