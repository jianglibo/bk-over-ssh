use log::*;
use std::convert::TryInto;
use std::path::Path;
use std::{fs, io, io::Seek};

#[derive(Debug)]
pub struct RecordWriter<T> {
    writer: T,
}

#[derive(Debug)]
pub struct RecordReader<T> {
    reader: T,
    source_len: Option<u64>,
}

impl<T> RecordReader<T>
where
    T: io::Read,
{
    pub fn new(reader: T, source_len: Option<u64>) -> Self {
        Self { reader, source_len }
    }

    pub fn inner_reader(&mut self) -> &mut T {
        &mut self.reader
    }

    pub fn with_file_reader(
        file: impl AsRef<Path>,
    ) -> Result<RecordReader<fs::File>, failure::Error> {
        let p = file.as_ref();
        let len = p.metadata()?.len();
        let reader = fs::OpenOptions::new().read(true).open(p)?;
        Ok(RecordReader::new(reader, Some(len)))
    }

    pub fn read_field_usize(&mut self) -> Result<Option<(u8, usize)>, failure::Error> {
        if let Some((field_type, u8_vec)) = self.read_field_slice()? {
            let mut ary = [0_u8; 8];
            ary.copy_from_slice(&u8_vec[..8]);
            Ok(Some((field_type, usize::from_be_bytes(ary))))
        } else {
            Ok(None)
        }
    }

    /// return the type_field(1 byte) and the content Vec<u8>.
    pub fn read_field_slice(&mut self) -> Result<Option<(u8, Vec<u8>)>, failure::Error> {
        let mut buf = [0_u8; 4];
        if self.read_exact(&mut buf)? {
            let record_len = u32::from_be_bytes(buf);
            if self.read_exact(&mut buf[0..=0])? {
                let field_type = buf[0];
                let mut buf = vec![0_u8; (record_len - 1).try_into()?];
                if self.read_exact(&mut buf)? {
                    return Ok(Some((field_type, buf)));
                }
            }
        }
        Ok(None)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<bool, failure::Error> {
        let mut n = 0;
        let count = buf.len();
        loop {
            let r = self.reader.read(&mut buf[n..])?;
            if r == 0 {
                break;
            }
            n += r;
            if count == n {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// read field length and field_type, return the field_type and (field length - 1(it's the field_type byte.))
    /// even not be readed, inscreasing the readed_len too which indicate the end of the file.
    pub fn read_field_header(&mut self) -> Result<Option<(u8, u64)>, failure::Error> {
        let mut buf = [0_u8; 4];
        if self.read_exact(&mut buf)? {
            let record_len = u32::from_be_bytes(buf);
            if self.read_exact(&mut buf[0..=0])? {
                let field_type = buf[0];
                info!(
                    "read_field_header, field_type: {:?}, record_len: {:?}",
                    field_type, record_len
                );
                return Ok(Some((field_type, (record_len - 1).into())));
            }
        }
        Ok(None)
    }
    /// read exact 8 bytes, and parse to u64.
    pub fn read_u64(&mut self) -> Result<Option<u64>, failure::Error> {
        let mut buf = [0_u8; 8];
        if self.read_exact(&mut buf)? {
            Ok(Some(u64::from_be_bytes(buf)))
        } else {
            Ok(None)
        }
    }
}

impl<T> RecordWriter<T>
where
    T: io::Write,
{
    pub fn new(writer: T) -> Self {
        Self { writer }
    }

    #[allow(dead_code)]
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
    pub fn write_field_usize(
        &mut self,
        field_type: u8,
        an_usize: usize,
    ) -> Result<u32, failure::Error> {
        let slice = &an_usize.to_be_bytes();
        self.write_field_slice(field_type, slice)
    }

    /// will first write a four bytes u32 represent the length of field_type(1) + slice.len().
    pub fn write_field_u64(&mut self, field_type: u8, an_u64: u64) -> Result<u32, failure::Error> {
        self.write_field_slice(field_type, &an_u64.to_be_bytes())
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
    /// will first write a four bytes u32 represent the length of field_type(1) + file.metadata()?.len().
    pub fn write_field_from_file(
        &mut self,
        field_type: u8,
        file: &mut fs::File,
    ) -> Result<u32, failure::Error> {
        let len: u32 = file
            .metadata()?
            .len()
            .try_into()
            .expect("u64 may convert to u32.");
        info!("write file with length: {:?}", len);
        let header = (len + 1).to_be_bytes();
        self.writer.write_all(&header)?;
        self.writer.write_all(&[field_type])?;
        file.seek(io::SeekFrom::Start(0))?;
        io::copy(file, &mut self.writer)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use std::fs;

    #[test]
    fn t_signature_large_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let _wr = RecordWriter::<fs::File>::with_file_writer("target/cc.sig")?;
        Ok(())
    }
}
