use super::{record, Block, Delta};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{fs, io, io::Write};

const TEN_MEGA_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockFile {
    FromSource(u64),
    Literal(usize),
}

impl Block for BlockFile {
    fn from_source(&self) -> Option<u64> {
        if let BlockFile::FromSource(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    fn next_bytes(&mut self, len: usize) -> Result<Option<&[u8]>, failure::Error> {
        Ok(None)
        // if let BlockFile::Literal(v8) = self {
        //     Ok(&v8[..])
        // } else {
        //     bail!("call get_bytes on from source block.");
        // }
    }
}

#[derive(Debug)]
/// The result of comparing two files
pub struct DeltaFile {
    /// Description of the new file in terms of blocks.
    wr: Option<record::RecordWriter<fs::File>>,
    rr: Option<record::RecordReader<fs::File>>,
    pending_file: Option<fs::File>,
    pending: Vec<u8>,
    window: usize,
    index: usize,
    pending_valve: usize,
}

impl<'a> DeltaFile {
    pub fn create_delta_file(
        file: impl AsRef<Path>,
        window: usize,
        pending_valve: Option<usize>,
    ) -> Result<Self, failure::Error> {
        let mut wr = record::RecordWriter::<fs::File>::with_file_writer(file.as_ref())?;
        wr.write_field_slice(0_u8, &window.to_be_bytes())?;
        Ok(Self {
            wr: Some(wr),
            rr: None,
            pending_file: None,
            pending: Vec::new(),
            window,
            index: 0,
            pending_valve: pending_valve.unwrap_or(TEN_MEGA_BYTES),
        })
    }

    pub fn read_delta_file(
        file: impl AsRef<Path>,
    ) -> Result<Self, failure::Error> {
        let mut rr = record::RecordReader::<fs::File>::with_file_reader(file.as_ref())?;
        let header = rr.read_field_slice()?;
        ensure!(header.is_some(), "delta_file should has header record.");
        let (_, u8_vec) = header.unwrap();
        let mut ary = [0_u8; 8];
        ary.copy_from_slice(&u8_vec[..8]);
        let window = usize::from_be_bytes(ary);
        Ok(Self {
            wr: None,
            rr: Some(rr),
            pending_file: None,
            pending: Vec::new(),
            window,
            index: 0,
            pending_valve: TEN_MEGA_BYTES,
        })
    }

    fn flush_pending(&mut self) -> Result<(), failure::Error> {
        ensure!(self.wr.is_some(), "delta_file in wr mode, wr should'nt be None.");
        if let Some(pf) = self.pending_file.as_mut() {
            if !self.pending.is_empty() {
                pf.write_all(&self.pending[..])?;
            }
            self.wr.as_mut().unwrap().write_field_from_file(1_u8, pf)?;
            self.pending_file.take();
        } else if !self.pending.is_empty() {
            self.wr.as_mut().unwrap().write_field_slice(1_u8, &self.pending[..])?;
        }
        self.pending.clear();
        Ok(())
    }
}

impl Delta<BlockFile> for DeltaFile {
    fn push_from_source(&mut self, position: u64) -> Result<(), failure::Error> {
        ensure!(self.wr.is_some(), "delta_file in wr mode, wr should'nt be None.");
        self.flush_pending()?;
        self.wr.as_mut().unwrap().write_field_slice(2_u8, &position.to_be_bytes())?;
        Ok(())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), failure::Error> {
        self.pending.push(byte);

        if self.pending.len() > self.pending_valve {
            if self.pending_file.is_none() {
                self.pending_file = tempfile::tempfile().ok();
            }
            if let Some(tf) = self.pending_file.as_mut() {
                tf.write_all(&self.pending[..])?;
                self.pending.clear();
            }
        }
        Ok(())
    }

    fn window(&self) -> usize {
        self.window
    }

    fn next_segment(&mut self) -> Result<Option<&mut BlockFile>, failure::Error> {
        ensure!(self.rr.is_some(), "delta_file in rr mode, rr should'nt be None.");

        if let Some(field) = self.rr.as_mut().unwrap().read_field_slice()? {
            // TODO read_field_header()
        }
        // let t = self.blocks.get(self.index);
        // self.index += 1;
        Ok(None)
    }

    fn finishup(&mut self) -> Result<(), failure::Error> {
        self.flush_pending()?;
        Ok(())
    }
}
