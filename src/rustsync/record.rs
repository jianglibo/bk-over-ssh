use log::*;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Write;
use std::path::Path;
use std::{fs, io};

#[derive(Debug)]
pub struct RecordWriter<T> {
    writer: T,
}

#[derive(Debug)]
pub struct RecordReader<'a, T: 'a> {
    reader: &'a T,
    source_len: Option<u64>,
    readed_len: u64,
}

impl<'a, T> RecordReader<'a, T>
where
    T: io::Read,
{
    pub fn new(reader: &'a T, source_len: Option<u64>) -> Self {
        Self {
            reader,
            source_len,
            readed_len: 0,
        }
    }

    pub fn inner_reader(&self) -> &T {
        &self.reader
    }

    pub fn with_file_reader(
        file: &'a fs::File,
    ) -> Result<RecordReader<fs::File>, failure::Error> {
        // let p = file.as_ref();
        let len = file.metadata()?.len();
        // let reader = fs::OpenOptions::new().read(true).open(p)?;
        Ok(RecordReader::new(file, Some(len)))
    }

    pub fn read_field_slice(&mut self) -> Result<Option<(u8, Vec<u8>)>, failure::Error> {
        if self.source_len > Some(self.readed_len) {
            let mut buf = [0_u8; 4];
            self.reader.read_exact(&mut buf)?;
            let record_len = u32::from_be_bytes(buf);
            self.reader.read_exact(&mut buf[0..=0])?;
            let field_type = buf[0];
            let mut buf = vec![0_u8; (record_len - 1).try_into()?];
            self.reader.read_exact(&mut buf)?;
            self.readed_len += 4 + u64::from(record_len);
            Ok(Some((field_type, buf)))
        } else {
            Ok(None)
        }
    }

    /// read field length and field_type, return the field_type and (field length - 1(it's the field_type byte.))
    /// even not be readed, inscreasing the readed_len too which indicate the end of the file.
    pub fn read_field_header(&mut self) -> Result<Option<(u8, u64)>, failure::Error> {
        if self.source_len > Some(self.readed_len) {
            let mut buf = [0_u8; 4];
            self.reader.read_exact(&mut buf)?;
            let record_len = u32::from_be_bytes(buf);
            self.reader.read_exact(&mut buf[0..=0])?;
            let field_type = buf[0];
            self.readed_len += 4 + u64::from(record_len);
            Ok(Some((field_type, (record_len - 1).into())))
        } else {
            Ok(None)
        }
    }

    pub fn read_u64(&mut self) -> Result<Option<u64>, failure::Error> {
        let mut buf = [0_u8; 8];
        self.reader.read_exact(&mut buf)?;
        self.readed_len += 8;
        Ok(Some(u64::from_be_bytes(buf)))
    }
}

impl<T> RecordWriter<T>
where
    T: io::Write,
{
    pub fn new(writer: T) -> Self {
        Self { writer }
    }

    pub fn with_file_writer(
        file: impl AsRef<Path>,
    ) -> Result<RecordWriter<fs::File>, failure::Error> {
        let writer = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(file.as_ref())?;
        Ok(RecordWriter::new(writer))
    }

    /// will first write a four bytes u32 represent the length of field_type(1) + slice.len().
    pub fn write_field_slice(
        &mut self,
        field_type: u8,
        slice: &[u8],
    ) -> Result<u32, failure::Error> {
        let header = ((slice.len() + 1) as u32).to_be_bytes();
        self.writer.write_all(&header)?;
        self.writer.write_all(&[field_type])?;
        self.writer.write_all(slice)?;
        Ok(0)
    }

    pub fn write_field_from_file(
        &mut self,
        field_type: u8,
        file: &mut fs::File,
    ) -> Result<u32, failure::Error> {
        let len: u32 = file.metadata()?.len().try_into().unwrap();
        let header = len.to_be_bytes();
        self.writer.write_all(&header)?;
        self.writer.write_all(&[field_type])?;
        io::copy(file, &mut self.writer)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::develope_data;
    use crate::log_util;
    use rand;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use std::io::{Read, Seek, Write};
    use std::time::Instant;
    use std::{fs, io};
    use tempfile;

    #[test]
    fn t_signature_large_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let wr = RecordWriter::<fs::File>::with_file_writer("target/cc.sig")?;
        Ok(())
    }
}
