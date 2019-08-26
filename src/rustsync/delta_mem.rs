use super::{Block, Delta};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum BlockVec {
    FromSource(u64),
    Literal(usize, Vec<u8>),
}

impl Block for BlockVec {
    fn is_from_source(&self) -> bool {
        if let BlockVec::FromSource(i) = self {
            true
        } else {
            false
        }

        // fn from_source(&self) -> Option<u64> {
        //     if let BlockVec::FromSource(i) = self {
        //         Some(*i)
        //     } else {
        //         None
        //     }
        // }

        // fn next_bytes(&mut self, len: usize) -> Result<Option<&[u8]>, failure::Error> {
        //     if let BlockVec::Literal(idx, v8) = self {
        //         if *idx < v8.len() {
        //             let old_idx = *idx;
        //             let upper_limit = old_idx + len;
        //             *idx = upper_limit;
        //             if upper_limit < v8.len() {
        //                 Ok(Some(&v8[old_idx..upper_limit]))
        //             } else {
        //                 Ok(Some(&v8[old_idx..]))
        //             }
        //         } else {
        //             Ok(None)
        //         }
        //     } else {
        //         bail!("call get_bytes on from source block.");
        //     }
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
        let buf = &mut buf_v[..];
        for block in self.blocks.iter() {
            match block {
                BlockVec::FromSource(i) => {
                    old.seek(io::SeekFrom::Start(*i as u64))?;
                    // fill the buffer from r.
                    let mut n = 0;
                    loop {
                        let r = old.read(&mut buf[n..self.window])?;
                        if r == 0 {
                            break;
                        }
                        n += r
                    }
                    // write the buffer to w.
                    let mut m = 0;
                    while m < n {
                        m += out.write(&buf[m..n])?;
                    }
                }
                BlockVec::Literal(_, v) => {
                    out.write_all(v.as_slice())?;
                }
            }
        }
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
