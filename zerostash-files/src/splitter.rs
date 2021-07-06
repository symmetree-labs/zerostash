use crate::rollsum::Rollsum;
use infinitree::crypto::{chunk_hash, Digest};

use std::marker::PhantomData;

pub struct FileSplitter<'file, RS> {
    data: &'file [u8],
    cur: usize,
    _rs: PhantomData<RS>,
}

impl<'file, RS> FileSplitter<'file, RS>
where
    RS: Rollsum,
{
    pub fn new(data: &'file [u8]) -> FileSplitter<'file, RS> {
        FileSplitter {
            data,
            _rs: PhantomData,
            cur: 0,
        }
    }
}

impl<'file, RS> Iterator for FileSplitter<'file, RS>
where
    RS: Rollsum,
{
    type Item = (u64, Digest, &'file [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur >= self.data.len() {
            return None;
        }

        let start = self.cur;
        let end = RS::new().find_offset(&self.data[start..]);
        let data = &self.data[start..start + end];
        self.cur += end;

        Some((start as u64, chunk_hash(data), data))
    }
}

#[cfg(test)]
mod tests {
    const PATH: &str = "../tests/data/10k_random_blob";

    #[test]
    fn check_chunk_iterator_sum() {
        use super::FileSplitter;
        use crate::rollsum::SeaSplit;
        use memmap2::MmapOptions;
        use std::fs::File;

        let file = File::open(PATH).unwrap();
        let metadata = file.metadata().unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        let size: usize = FileSplitter::<SeaSplit>::new(&mmap)
            .map(|(_, _, c)| c.len())
            .sum();
        assert_eq!(size as u64, metadata.len());
    }
}
