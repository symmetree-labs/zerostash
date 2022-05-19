#![allow(unused)]

use seahash::SeaHasher;
use std::hash::Hasher;

const ROLLSUM_CHAR_OFFSET: u32 = 31;
const BLOBBITS: u32 = (16);
const BLOBSIZE: u32 = (1 << BLOBBITS);
const WINDOWBITS: u32 = (6);
const WINDOWSIZE: u32 = (1 << WINDOWBITS);
const CHUNK_SIZE_LIMIT: usize = 256 * 1024;

pub trait Rollsum {
    fn new() -> Self;
    fn find_offset(&mut self, buf: &[u8]) -> usize;
}

#[derive(Default)]
pub struct SeaSplit(SeaHasher);

impl Rollsum for SeaSplit {
    fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    fn find_offset(&mut self, buf: &[u8]) -> usize {
        self.0 = SeaHasher::default();
        let mut last = 0;

        // On occasion a too high value for step size can produce
        // chunks larger than a single object
        for limit in (0..buf.len()).step_by(64) {
            self.0.write(&buf[last..limit]);
            let output = self.0.finish();

            if (output & ((u64::from(BLOBSIZE)) - 1)) == ((u64::from(BLOBSIZE)) - 1)
                || limit >= CHUNK_SIZE_LIMIT
            {
                return limit + 1;
            } else {
                last = limit;
            }
        }
        buf.len()
    }
}

pub struct BupSplit {
    s1: u32,
    s2: u32,
    window: [u8; WINDOWSIZE as usize],
    wofs: usize,
}

impl BupSplit {
    #[inline]
    pub fn add(&mut self, drop: u8, add: u8) {
        self.s1 = self.s1.wrapping_add(u32::from(add.wrapping_sub(drop)));
        self.s2 = self.s2.wrapping_add(
            self.s1
                .wrapping_sub((WINDOWSIZE * (u32::from(drop) + ROLLSUM_CHAR_OFFSET)) as u32),
        );
    }

    #[inline]
    pub fn roll(&mut self, ch: u8) {
        self.add(self.window[self.wofs], ch);
        self.window[self.wofs] = ch;
        self.wofs = (self.wofs + 1) % (WINDOWSIZE as usize);
    }

    #[inline]
    pub fn digest(&self) -> u32 {
        (self.s1 << 16) | (self.s2 & 0xffff)
    }
}

impl Rollsum for BupSplit {
    fn new() -> Self {
        BupSplit {
            s1: WINDOWSIZE * ROLLSUM_CHAR_OFFSET,
            s2: WINDOWSIZE * (WINDOWSIZE - 1) * ROLLSUM_CHAR_OFFSET,
            wofs: 0,
            window: [0; WINDOWSIZE as usize],
        }
    }

    fn find_offset(&mut self, buf: &[u8]) -> usize {
        for (i, v) in buf.iter().enumerate() {
            self.roll(*v);

            if (self.s2 & (BLOBSIZE - 1)) == (BLOBSIZE - 1) {
                return i + 1;
            }
        }
        buf.len()
    }
}

#[cfg(test)]
mod tests {
    const SELFTEST_SIZE: usize = 100_000;
    use super::WINDOWSIZE;

    fn rollsum_sum(buf: &[u8], ofs: usize, len: usize) -> u32 {
        use super::{BupSplit, Rollsum};
        let mut r = BupSplit::new();
        for count in ofs..len {
            r.roll(buf[count]);
        }
        r.digest()
    }

    fn setup() -> [u8; SELFTEST_SIZE] {
        let mut buf = [0; SELFTEST_SIZE];
        getrandom::getrandom(&mut buf).unwrap();

        buf
    }

    #[test]
    #[ignore]
    fn bupsplit_selftest() {
        let buf = setup();

        let sum1a = rollsum_sum(&buf, 0, SELFTEST_SIZE);
        let sum1b = rollsum_sum(&buf, 2, SELFTEST_SIZE);
        let sum2a = rollsum_sum(
            &buf,
            SELFTEST_SIZE - (WINDOWSIZE * 5 / 2) as usize,
            SELFTEST_SIZE - WINDOWSIZE as usize,
        );
        let sum2b = rollsum_sum(&buf, 0, SELFTEST_SIZE - WINDOWSIZE as usize);
        let sum3a = rollsum_sum(&buf, 0, (WINDOWSIZE + 3) as usize);
        let sum3b = rollsum_sum(&buf, 3, (WINDOWSIZE + 3) as usize);

        assert_ne!(sum1a, sum1b);
        assert_ne!(sum2a, sum2b);
        assert_ne!(sum3a, sum3b);
    }
}
