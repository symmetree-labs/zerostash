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
    pub async fn new(writer: W, writers: usize) -> Result<Self> {
        let (enqueue, dequeue) = mpsc::bounded(writers);

        for _ in 0..writers {
            enqueue.send_async(writer.clone()).await.unwrap();
        }

        Ok(RoundRobinBalancer {
            enqueue,
            dequeue,
            writers,
        })
    }

    pub async fn write(&self, hash: &CryptoDigest, data: &[u8]) -> Result<ChunkPointer> {
        let mut writer = self.dequeue.recv_async().await.unwrap();
        let result = writer.write_chunk(hash, data).await;
        self.enqueue.send_async(writer).await.unwrap();

        result
    }

    pub async fn flush(&self) -> Result<()> {
        for _ in 0..self.writers {
            let mut writer = self.dequeue.recv_async().await.unwrap();
            writer.flush().await.unwrap();
            self.enqueue.send_async(writer).await.unwrap();
        }

        Ok(())
    }
}
