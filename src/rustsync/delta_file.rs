use super::{record, Block, Delta};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{fs, io, io::{Write, Read}};
use std::marker::PhantomData;

const TEN_MEGA_BYTES: usize = 10 * 1024 * 1024;

const LITERAL_FIELD_BYTE: u8 = 1;
const FROM_SOURCE_FIELD_BYTE: u8 = 2;


#[derive(Debug)]
pub struct LiteralReader<'lr, 'reader> {
    len: u64,
    reader: &'reader fs::File,
    phantom: PhantomData<&'lr bool>,
}

#[derive(Debug)]
pub enum BlockFile<'bf, 'reader> {
    FromSource(u64),
    Literal(LiteralReader<'bf, 'reader>),
}

impl<'bf, 'reader> Block for BlockFile<'bf, 'reader> {
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
pub struct DeltaFile<'df> {
    /// Description of the new file in terms of blocks.
    wr: Option<&'df mut record::RecordWriter<fs::File>>,
    rr: Option<&'df mut record::RecordReader<'df, fs::File>>,
    pending_file: Option<fs::File>,
    pending: Vec<u8>,
    window: usize,
    index: usize,
    pending_valve: usize,
    phantom: PhantomData<&'df bool>,
}

impl<'df> DeltaFile<'df> {
    pub fn create_delta_file(
        record_writer: &'df mut record::RecordWriter::<fs::File>,
        window: usize,
        pending_valve: Option<usize>,
    ) -> Result<Self, failure::Error> {
        // let mut wr = record::RecordWriter::<fs::File>::with_file_writer(file.as_ref())?;
        record_writer.write_field_slice(0_u8, &window.to_be_bytes())?;
        Ok(Self {
            wr: Some(record_writer),
            rr: None,
            pending_file: None,
            pending: Vec::new(),
            window,
            index: 0,
            pending_valve: pending_valve.unwrap_or(TEN_MEGA_BYTES),
            phantom: PhantomData,
        })
    }

    pub fn read_delta_file(
        record_reader: &'df mut record::RecordReader<'df, fs::File>,
    ) -> Result<Self, failure::Error> {
        // let mut rr = record::RecordReader::<fs::File>::with_file_reader(file.as_ref())?;
        // let record_reader = record::RecordReader::<fs::File>::with_file_reader(file)?;
        let header = record_reader.read_field_slice()?;
        ensure!(header.is_some(), "delta_file should has header record.");
        let (_, u8_vec) = header.unwrap();
        let mut ary = [0_u8; 8];
        ary.copy_from_slice(&u8_vec[..8]);
        let window = usize::from_be_bytes(ary);
        Ok(Self {
            wr: None,
            rr: Some(record_reader),
            pending_file: None,
            pending: Vec::new(),
            window,
            index: 0,
            pending_valve: TEN_MEGA_BYTES,
            phantom: PhantomData,
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

impl<'df, 'bf, 'reader> Delta<BlockFile<'bf, 'reader>> for DeltaFile<'df> {
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

    fn next_segment(&mut self) -> Result<Option<BlockFile<'bf, 'reader>>, failure::Error> {
        ensure!(self.rr.is_some(), "delta_file in rr mode, rr should'nt be None.");

        if let Some((field_type, field_len)) = self.rr.as_mut().unwrap().read_field_header()? {
           match field_type {
               FROM_SOURCE_FIELD_BYTE => {
                   if let Ok(Some(position)) = self.rr.as_mut().unwrap().read_u64() {
                    let block = BlockFile::FromSource(position);
                   Ok(Some(block))
                   } else {
                       bail!("read_u64 failed.")
                   }
               },
               LITERAL_FIELD_BYTE => {
                   let reader = self.rr.as_mut().unwrap().inner_reader();
                   let l = LiteralReader {
                       len: field_len,
                       reader,
                       phantom: PhantomData,
                   };
                   Ok(Some(BlockFile::Literal(l)))
               },
               _ => {
                   bail!("got unexpected field_type");
               }
           } 
        } else {
            Ok(None)
        }
    }

    fn finishup(&mut self) -> Result<(), failure::Error> {
        self.flush_pending()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
struct FontLoader(String);
struct Font<'a>(&'a str);

impl FontLoader {
    fn load(&self) -> Font {
        Font(&self.0)
    }
}

struct Window;

// struct Phi<'window> {
//     window: &'window Window,
//     loader: FontLoader,
//     font: Option<Font<'window>>,
// }

// impl<'window> Phi<'window> {
//     fn do_the_thing(&mut self) {
//         let font = self.loader.load();
//         self.font = Some(font);
//     }
// }

/// you cannot return a reference from owned object in the struct!!!!!!!!!!
struct Phi<'a> {
    window: &'a Window,
    loader: &'a FontLoader,
    font: Option<Font<'a>>,
}

impl<'a> Phi<'a> {
    fn do_the_thing(&mut self) {
        let font = self.loader.load();
        self.font = Some(font);
    }
}

}

