use super::{Block, Delta};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockVec {
    FromSource(u64),
    Literal(usize, Vec<u8>),
}

impl Block for BlockVec {
    fn from_source(&self) -> Option<u64> {
        if let BlockVec::FromSource(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    fn next_bytes(&mut self, len: usize) -> Result<Option<&[u8]>, failure::Error> {
        if let BlockVec::Literal(idx, v8) = self {
            if *idx < v8.len() {
                let old_idx = *idx;
                let upper_limit = old_idx + len;
                *idx = upper_limit;
                if upper_limit < v8.len() {
                    Ok(Some(&v8[old_idx..upper_limit]))
                } else {
                    Ok(Some(&v8[old_idx..]))
                }
            } else {
                Ok(None)
            }
        } else {
            bail!("call get_bytes on from source block.");
        }
    }

    // fn get_bytes(&self) -> Result<&[u8], failure::Error> {
    // }
}

#[derive(Default, Debug, PartialEq)]
/// The result of comparing two files
pub struct DeltaMem {
    /// Description of the new file in terms of blocks.
    blocks: Vec<BlockVec>,
    pending: Vec<u8>,
    /// Size of the window.
    window: usize,
    index: usize,
}

impl DeltaMem {
    pub fn new(window: usize) -> Self {
        Self {
            blocks: Vec::new(),
            pending: Vec::new(),
            window,
            index: 0,
        }
    }
}

impl Delta<BlockVec> for DeltaMem {
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

    fn window(&self) -> usize {
        self.window
    }

    fn next_segment(&mut self) -> Result<Option<BlockVec>, failure::Error> {
        // let t = self.blocks.get_mut(self.index);
        // self.index += 1;
        Ok(Some(self.blocks.remove(0)))
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
