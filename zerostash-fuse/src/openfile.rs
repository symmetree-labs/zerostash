use std::io::{Cursor, Seek, SeekFrom, Write};

pub struct OpenFile {
    pub open_file: Cursor<Vec<u8>>,
}

impl OpenFile {
    pub fn from_vec(vec: Vec<u8>) -> Self {
        Self {
            open_file: Cursor::new(vec),
        }
    }
    pub fn write_at(&mut self, offset: u64, data: Vec<u8>) -> anyhow::Result<u32, i32> {
        if let Err(e) = self.open_file.seek(SeekFrom::Start(offset)) {
            return Err(e.raw_os_error().unwrap());
        }

        let nwritten: u32 = match self.open_file.write(&data) {
            Ok(n) => n as u32,
            Err(e) => {
                return Err(e.raw_os_error().unwrap());
            }
        };

        if let Err(e) = self.open_file.seek(SeekFrom::Start(0)) {
            return Err(e.raw_os_error().unwrap());
        }

        Ok(nwritten)
    }
    pub fn get_len(&self) -> u64 {
        self.open_file.get_ref().len() as u64
    }
}
