use infinitree::object::{AEADReader, AEADWriter, BufferedSink, PoolRef};
use std::io::{self, Read, Write};

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
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub stream: infinitree::object::Stream,
}

impl Snapshot {
    pub fn from_stdin(writer: AEADWriter) -> Result<Snapshot, SnapshotError> {
        let mut buf = Vec::default();
        std::io::stdin().read_to_end(&mut buf)?;

        let mut sink = BufferedSink::new(writer);
        sink.write_all(&buf)?;
        let stream = sink.finish()?;
        Ok(Self { stream })
    }

    pub fn to_stdout(&self, reader: PoolRef<AEADReader>) -> Result<(), SnapshotError> {
        let mut lock = std::io::stdout().lock();
        let mut buf = Vec::default();

        let mut stream = self.stream.open_reader(reader);
        stream.read_to_end(&mut buf)?;
        lock.write_all(&buf)?;

        Ok(())
    }
}
