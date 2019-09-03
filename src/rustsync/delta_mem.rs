use super::{DeltaReader, DeltaWriter};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockVec {
    FromSource(u64),
    Literal(usize, Vec<u8>),
}

#[derive(Default, Debug, PartialEq)]
/// The result of comparing two files
pub struct DeltaMem {
    /// Description of the new file in terms of blocks.
    blocks: Vec<BlockVec>,
    pending: Vec<u8>,
    window: usize,
    index: usize,
}

impl DeltaMem {
    #[allow(dead_code)]
    pub fn new(window: usize) -> Self {
        Self {
            blocks: Vec::new(),
            pending: Vec::new(),
            window,
            index: 0,
        }
    }
}

impl DeltaWriter for DeltaMem {
    fn push_from_source(&mut self, position: u64) -> Result<(), failure::Error> {
        if !self.pending.is_empty() {
            // We've reached the end of the file, and have never found
            // a matching block again.
            let v = std::mem::replace(&mut self.pending, Vec::new());
            let b = BlockVec::Literal(0, v);
            self.blocks.push(b);
        }
        self.blocks.push(BlockVec::FromSource(position));
        Ok(())
    }
    fn push_byte(&mut self, byte: u8) -> Result<(), failure::Error> {
        self.pending.push(byte);
        Ok(())
    }

    fn finishup(&mut self) -> Result<(), failure::Error> {
        if !self.pending.is_empty() {
            // We've reached the end of the file, and have never found
            // a matching block again.
            let b = BlockVec::Literal(0, std::mem::replace(&mut self.pending, Vec::new()));
            self.blocks.push(b);
        }
        Ok(())
    }
}
impl DeltaReader for DeltaMem {
    fn block_count(&mut self) -> Result<(usize, usize), failure::Error> {
        Ok(self.blocks.iter().fold((0, 0), |acc, block| match block {
            BlockVec::FromSource(i) => (acc.0 + 1, acc.1),
            BlockVec::Literal(_, _) => (acc.0, acc.1 + 1),
        }))
    }

    fn restore(&mut self, mut out: impl io::Write, old: &[u8]) -> Result<(), failure::Error> {
        for block in self.blocks.iter() {
            match block {
                BlockVec::FromSource(i) => {
                    let i = *i as usize;
                    if i + self.window <= old.len() {
                        out.write_all(&old[i..i + self.window])?;
                    } else {
                        out.write_all(&old[i..])?;
                    }
                }
                BlockVec::Literal(_, v) => {
                    out.write_all(v.as_slice())?;
                }
            }
        }
        Ok(())
    }

    fn restore_seekable(
        &mut self,
        mut out: impl io::Write,
        mut old: impl io::Read + io::Seek,
    ) -> Result<(), failure::Error> {
        let mut buf_v = vec![0_u8; self.window];
        for block in self.blocks.iter() {
            match block {
                BlockVec::FromSource(i) => {
                    DeltaMem::restore_from_source_seekable(
                        *i,
                        &mut buf_v[..],
                        self.window,
                        &mut out,
                        &mut old,
                    )?;
                }
                BlockVec::Literal(_, v) => {
                    out.write_all(v.as_slice())?;
                }
            }
        }
        Ok(())
    }
}
