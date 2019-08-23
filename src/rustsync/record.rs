use log::*;
use std::collections::HashMap;
use std::{io, fs};
use std::io::Write;
use std::path::Path;

pub struct RecordWriter<T: io::Write> {
    writer: T,
}

impl<T: io::Write> RecordWriter<T> {
    pub fn new(writer: T) -> Self {
        Self {
            writer
        }
    }

    pub fn with_file_writer(file: impl AsRef<Path>) -> Result<RecordWriter<fs::File>, failure::Error> {
        let writer = fs::OpenOptions::new().create(true).write(true).open(file.as_ref())?;
        Ok(RecordWriter::new(writer))
    }
    pub fn write_field_slice(&mut self, field_type: u8, slice: &[u8]) -> Result<u32, failure::Error> {
        let header = ((slice.len() + 1) as u32).to_be_bytes();
        self.writer.write_all(&header)?;
        self.writer.write_all(&[field_type])?;
        self.writer.write_all(slice)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use rand;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use std::{io, fs};
    use std::io::{Read, Write, Seek};
    use tempfile;
    use std::time::Instant;
    use crate::develope::develope_data;

    #[test]
    fn t_signature_large_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let wr = RecordWriter::<fs::File>::with_file_writer("target/cc.sig")?;
        Ok(())
    }
}
