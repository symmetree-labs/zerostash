use infinitree::{
    object::{AEADReader, PoolRef, Reader},
    ChunkPointer,
};
use std::{iter::Peekable, sync::Arc};

type Chunk = (u64, Arc<ChunkPointer>);

struct ChunksIter {
    pub chunks: Peekable<Box<dyn Iterator<Item = Chunk> + Send + Sync>>,
}

impl ChunksIter {
    fn new(chunks: impl Iterator<Item = Chunk> + Send + Sync + 'static) -> Self {
        let chunks: Box<dyn Iterator<Item = Chunk> + Send + Sync> = Box::new(chunks);
        Self {
            chunks: chunks.peekable(),
        }
    }

    fn peek_next_offset(&mut self, file_size: usize) -> usize {
        let arc = (file_size as u64, Arc::new(ChunkPointer::default()));
        let (chunk_offset, _) = self.chunks.peek().unwrap_or(&arc);
        *chunk_offset as usize
    }

    fn get_next(&mut self) -> Option<(usize, Arc<ChunkPointer>)> {
        self.chunks
            .next()
            .map(|(offset, pointer)| (offset as usize, pointer))
    }
}

#[derive(Debug)]
pub enum ChunkDataError {
    NullChunkPointer,
}

pub struct ChunkStackCache {
    chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub last_read_offset: usize,
}

impl ChunkStackCache {
    pub fn new(chunks: Vec<Chunk>) -> Self {
        let chunks = ChunksIter::new(chunks.into_iter());
        Self {
            chunks,
            buf: Default::default(),
            last_read_offset: Default::default(),
        }
    }

    pub fn set_current_read(&mut self, val: usize) {
        self.last_read_offset = val;
    }

    pub fn split_buf(&mut self, end: usize) -> Vec<u8> {
        let mut ret_buf = self.buf.split_off(end);
        std::mem::swap(&mut self.buf, &mut ret_buf);
        ret_buf
    }

    #[inline(always)]
    pub fn read_next(
        &mut self,
        file_size: usize,
        objectreader: &mut PoolRef<AEADReader>,
    ) -> anyhow::Result<(), ChunkDataError> {
        let Some((c_offset, pointer)) = self.chunks.get_next() else {
            return Err(ChunkDataError::NullChunkPointer);
        };

        let next_c_offset = self.chunks.peek_next_offset(file_size);

        let len = next_c_offset - c_offset;
        self.buf.extend(std::iter::repeat(0).take(len));
        let start = self.buf.len() - len;
        objectreader
            .read_chunk(&pointer, &mut self.buf[start..])
            .unwrap();

        Ok(())
    }
}

pub struct ChunkStack {
    chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl ChunkStack {
    pub fn new(chunks: Vec<Chunk>, offset: usize) -> Self {
        let index = match chunks.binary_search_by(|a| a.0.cmp(&(offset as u64))) {
            Ok(v) => v,
            Err(v) => v - 1,
        };
        let chunks = ChunksIter::new(chunks.into_iter().skip(index));

        Self {
            chunks,
            buf: Default::default(),
            start: None,
            end: None,
        }
    }

    #[inline(always)]
    pub fn read_next(
        &mut self,
        file_size: usize,
        offset: usize,
        objectreader: &mut PoolRef<AEADReader>,
    ) -> anyhow::Result<(), ChunkDataError> {
        let Some((c_offset, pointer)) = self.chunks.get_next() else {
            return Err(ChunkDataError::NullChunkPointer);
        };

        let next_c_offset = self.chunks.peek_next_offset(file_size);

        if self.start.is_none() {
            self.start = Some(offset - c_offset);
        }

        let len = next_c_offset - c_offset;
        self.buf.extend(std::iter::repeat(0).take(len));
        let start = self.buf.len() - len;
        objectreader
            .read_chunk(&pointer, &mut self.buf[start..])
            .unwrap();

        Ok(())
    }

    #[inline(always)]
    pub fn is_full(&mut self, size: usize, file_size: usize, offset: usize) -> bool {
        if let Some(from) = self.start {
            if self.buf[from..].len() >= size.min(file_size - offset) {
                self.end = Some(self.buf.len().min(from + size));
                return true;
            }
        }
        false
    }
}
