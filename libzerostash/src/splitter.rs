use crate::crypto::{CryptoDigest, DigestFn};
use crate::rollsum::Rollsum;

use blake2::Digest;
use failure::Error;

use std::marker::PhantomData;

pub struct FileSplitter<'file, RS> {
    data: &'file [u8],
    hasher: DigestFn,
    cur: usize,
    _rs: PhantomData<RS>,
}

impl<'file, RS> FileSplitter<'file, RS>
where
    RS: Rollsum,
{
    pub fn new(data: &'file [u8]) -> Result<FileSplitter<'file, RS>, Error> {
        Ok(FileSplitter {
            data,
            hasher: DigestFn::default(),
            _rs: PhantomData,
            cur: 0,
        })
    }
}

impl<'file, RS> Iterator for FileSplitter<'file, RS>
where
    RS: Rollsum,
{
    type Item = (u64, CryptoDigest, &'file [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur >= self.data.len() {
            return None;
        }

        let start = self.cur;
        let end = RS::new().find_offset(&self.data[start..]);
        let data = &self.data[start..start + end];
        self.cur += end;

        self.hasher.input(data);

        let mut digest = CryptoDigest::default();
        digest.clone_from_slice(self.hasher.result_reset().as_slice());

        Some((start as u64, digest, data))
    }
}

#[cfg(test)]
mod tests {
    extern crate test;
    const PATH: &'static str = "tests/data/10k_random_blob";

    #[bench]
    fn bench_chunk_iter(b: &mut test::Bencher) {
        use super::FileSplitter;
        use crate::rollsum::SeaSplit;
        use memmap::MmapOptions;
        use std::fs::File;

        let file = File::open(PATH).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        b.iter(|| {
            FileSplitter::<SeaSplit>::new(&mmap)
                .unwrap()
                .map(|(_, _, c)| c.len())
                .sum::<usize>()
        });

        println!("asdf");
    }

    #[test]
    fn check_chunk_iterator_sum() {
        use super::FileSplitter;
        use crate::rollsum::SeaSplit;
        use memmap::MmapOptions;
        use std::fs::File;

        let file = File::open(PATH).unwrap();
        let metadata = file.metadata().unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };

        let size: usize = FileSplitter::<SeaSplit>::new(&mmap)
            .unwrap()
            .map(|(_, _, c)| c.len())
            .sum();
        assert_eq!(size as u64, metadata.len());
    }
}
