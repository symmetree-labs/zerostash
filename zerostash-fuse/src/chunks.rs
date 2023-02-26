use infinitree::{
    object::{AEADReader, PoolRef, Reader},
    ChunkPointer,
};
use std::{iter::Skip, sync::Arc, vec::IntoIter};

type Chunks = Skip<IntoIter<(u64, Arc<ChunkPointer>)>>;

pub struct ChunksIter {
    pub chunks: std::iter::Peekable<Chunks>,
}

impl ChunksIter {
    fn new(chunks: Chunks) -> Self {
        let chunks = chunks.peekable();
        Self { chunks }
    }

    fn peek_next_offset(&mut self, file_size: usize) -> usize {
        let arc = (file_size as u64, Arc::new(ChunkPointer::default()));
        let (chunk_offset, _) = self.chunks.peek().unwrap_or(&arc);
        *chunk_offset as usize
    }

    fn get_next(&mut self) -> Option<(usize, Arc<ChunkPointer>)> {
        let (c_offset, pointer) = match self.chunks.next() {
            Some((o, p)) => (o as usize, p),
            None => return None,
        };
        Some((c_offset, pointer))
    }
}

#[derive(Debug)]
pub enum ChunkDataError {
    NullChunkPointer,
}

pub struct ChunkStackCache {
    pub chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub last_read_offset: usize,
}

impl ChunkStackCache {
    pub fn new(chunks: Vec<(u64, Arc<ChunkPointer>)>) -> Self {
        let chunks = ChunksIter::new(chunks.into_iter().skip(0));
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
        let (c_offset, pointer) = match self.chunks.get_next() {
            Some(chunk) => chunk,
            None => return Err(ChunkDataError::NullChunkPointer),
        };
        let next_c_offset = self.chunks.peek_next_offset(file_size);

        let mut temp_buf = vec![0; next_c_offset - c_offset];
        objectreader.read_chunk(&pointer, &mut temp_buf).unwrap();
        self.buf.append(&mut temp_buf);

        Ok(())
    }
}

pub struct ChunkStack {
    pub chunks: ChunksIter,
    pub buf: Vec<u8>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl ChunkStack {
    pub fn new(chunks: Vec<(u64, Arc<ChunkPointer>)>, offset: usize) -> Self {
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
        let (c_offset, pointer) = match self.chunks.get_next() {
            Some(chunk) => chunk,
            None => return Err(ChunkDataError::NullChunkPointer),
        };
        let next_c_offset = self.chunks.peek_next_offset(file_size);

        if self.start.is_none() {
            self.start = Some(offset - c_offset);
        }
        let mut temp_buf = vec![0; next_c_offset - c_offset];
        objectreader.read_chunk(&pointer, &mut temp_buf).unwrap();
        self.buf.append(&mut temp_buf);

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
