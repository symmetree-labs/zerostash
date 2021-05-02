use super::{Result, Writer};
use crate::{chunks::ChunkPointer, crypto::CryptoDigest};

use flume as mpsc;

#[derive(Clone)]
pub struct RoundRobinBalancer<W> {
    enqueue: mpsc::Sender<W>,
    dequeue: mpsc::Receiver<W>,
    writers: usize,
}

impl<W: 'static + Writer> RoundRobinBalancer<W> {
    pub fn new(writer: W, writers: usize) -> Result<Self> {
        let (enqueue, dequeue) = mpsc::bounded(writers);

        for _ in 0..writers {
            enqueue.send(writer.clone()).unwrap();
        }

        Ok(RoundRobinBalancer {
            enqueue,
            dequeue,
            writers,
        })
    }

    pub fn write(&self, hash: &CryptoDigest, data: &[u8]) -> Result<ChunkPointer> {
        let mut writer = self.dequeue.recv().unwrap();
        let result = writer.write_chunk(hash, data);
        self.enqueue.send(writer).unwrap();

        result
    }

    pub fn flush(&self) -> Result<()> {
        for _ in 0..self.writers {
            let mut writer = self.dequeue.recv().unwrap();
            writer.flush().unwrap();
            self.enqueue.send(writer).unwrap();
        }

        Ok(())
    }
}
