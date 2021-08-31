use super::{Result, Writer};
use crate::{crypto::Digest, ChunkPointer};

use flume as mpsc;

#[derive(Clone)]
pub struct RoundRobinBalancer<W> {
    enqueue: mpsc::Sender<W>,
    dequeue: mpsc::Receiver<W>,
    writers: usize,
}

impl<W: 'static + Writer + Clone> RoundRobinBalancer<W> {
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
}

impl<W: 'static + Writer> Writer for RoundRobinBalancer<W> {
    fn write_chunk(&mut self, hash: &Digest, data: &[u8]) -> Result<ChunkPointer> {
        let mut writer = self.dequeue.recv().unwrap();
        let result = writer.write_chunk(hash, data);
        self.enqueue.send(writer).unwrap();

        result
    }

    fn flush(&mut self) -> Result<()> {
        for _ in 0..self.writers {
            let mut writer = self.dequeue.recv().unwrap();
            writer.flush()?;
            self.enqueue.send(writer).unwrap();
        }

        Ok(())
    }
}
