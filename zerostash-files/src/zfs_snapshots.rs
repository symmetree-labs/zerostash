use chrono::{DateTime, Utc};
use infinitree::object::{AEADReader, AEADWriter, BufferedSink, PoolRef};
use std::{
    io::{self, Read, Write},
    process::{ChildStdin, ChildStdout},
    time::{Duration, SystemTime},
};

#[derive(thiserror::Error, Debug)]
pub enum SnapshotError {
    #[error("IO error: {source}")]
    IO {
        #[from]
        source: io::Error,
    },

    #[error("Object error: {source}")]
    Object {
        #[from]
        source: infinitree::object::ObjectError,
    },

    #[error("System time error: {source}")]
    Time {
        #[from]
        source: std::time::SystemTimeError,
    },
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ZfsSnapshot {
    pub stream: infinitree::object::Stream,
    pub creation_time_secs: u64,
    pub creation_time_nanos: u128,
}

impl From<&ZfsSnapshot> for DateTime<Utc> {
    fn from(e: &ZfsSnapshot) -> Self {
        let duration = Duration::new(
            e.creation_time_secs,
            e.creation_time_nanos.try_into().unwrap_or(0),
        );
        let systime = SystemTime::UNIX_EPOCH + duration;
        DateTime::from(systime)
    }
}

impl ZfsSnapshot {
    pub fn from_stdout(
        writer: AEADWriter,
        stdin: &mut ChildStdout,
    ) -> Result<ZfsSnapshot, SnapshotError> {
        let mut sink = BufferedSink::with_chunk_size(writer, 4_100_000);
        let mut buf = vec![0; 1_000_000];

        loop {
            let read_amount = stdin.read(&mut buf)?;
            if read_amount == 0 {
                break;
            }
            sink.write_all(&buf[..read_amount])?;
        }

        let stream = sink.finish()?;

        let since_epoch = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let creation_time_secs = since_epoch.as_secs();
        let creation_time_nanos = since_epoch.as_nanos();

        Ok(Self {
            stream,
            creation_time_secs,
            creation_time_nanos,
        })
    }

    pub fn to_stdin(
        &self,
        reader: PoolRef<AEADReader>,
        lock: &mut ChildStdin,
    ) -> Result<(), SnapshotError> {
        let mut stream = self.stream.open_reader(reader);
        let mut buf = vec![0; 1_000_000];

        loop {
            let read_amount = stream.read(&mut buf)?;
            if read_amount == 0 {
                break;
            }
            lock.write_all(&buf[..read_amount])?;
        }

        Ok(())
    }
}
