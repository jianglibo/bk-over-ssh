use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use log::*;
// use tokio_io::{AsyncRead, AsyncWrite};
// use tokio_io::io::{write_all, WriteAll};
// use futures::{Async, Future, Poll};

use serde::{Deserialize, Serialize};

const BLAKE2_SIZE: usize = 32;

#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Blake2b([u8; BLAKE2_SIZE]);

impl std::borrow::Borrow<[u8]> for Blake2b {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
/// The "signature" of the file, which is essentially a
/// content-indexed description of the blocks in the file.
pub struct Signature {
    pub window: usize,
    chunks: HashMap<u32, HashMap<Blake2b, usize>>
}


/// Create the "signature" of a file, essentially a content-indexed
/// map of blocks. The first step of the protocol is to run this
/// function on the "source" (the remote file when downloading, the
/// local file while uploading).
pub fn signature<R: Read, B: AsRef<[u8]>+AsMut<[u8]>>(mut r: R, mut buff: B) -> Result<Signature, std::io::Error> {
    let mut chunks = HashMap::new();

    let mut i = 0;
    let buff = buff.as_mut();
    let mut eof = false;
    while !eof {
        let mut j = 0;
        while j < buff.len() { // full fill the block. for the last block, maybe not full.
            let r = r.read(&mut buff[j..])?;
            if r == 0 {
                eof = true;
                break;
            }
            j += r
        }
        let readed_block = &buff[..j];
        let hash = adler32::RollingAdler32::from_buffer(readed_block);
        let mut blake2 = [0; BLAKE2_SIZE];
        blake2.clone_from_slice(blake2_rfc::blake2b::blake2b(BLAKE2_SIZE, &[], &readed_block).as_bytes());
        // println!("block: {:?}, blake2: {:?}", block, blake2);
        chunks
            .entry(hash.hash())
            .or_insert(HashMap::new())
            .insert(Blake2b(blake2), i);

        i += readed_block.len()
    }

    Ok(Signature {
        window: buff.len(),
        chunks
    })
}


#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum Block {
    FromSource(u64),
    Literal(Vec<u8>),
}

struct State {
    result: Vec<Block>,
    block_oldest: usize,
    block_len: usize,
    pending: Vec<u8>,
}

impl State {
    fn new() -> Self {
        State {
            result: Vec::new(),
            block_oldest: 0,
            block_len: 1,
            pending: Vec::new(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
/// The result of comparing two files
pub struct Delta {
    /// Description of the new file in terms of blocks.
    pub blocks: Vec<Block>,
    /// Size of the window.
    pub window: usize,
}

/// Compare a signature with an existing file. This is the second step
/// of the protocol, `r` is the local file when downloading, and the
/// remote file when uploading.
///
/// `block` must be a buffer the same size as `sig.window`.
pub fn compare<R: Read, B:AsRef<[u8]>+AsMut<[u8]>>(sig: &Signature, mut r: R, mut buff: B) -> Result<Delta, std::io::Error> {
    let mut st = State::new();
    let buff = buff.as_mut();
    assert_eq!(buff.len(), sig.window);
    while st.block_len > 0 {
        let mut hash = {
            let mut j = 0;
            let readed_block = {
                while j < sig.window {
                    let r = r.read(&mut buff[..])?;
                    if r == 0 {
                        break;
                    }
                    j += r
                }
                st.block_oldest = 0;
                st.block_len = j;
                &buff[..j]
            };
            adler32::RollingAdler32::from_buffer(readed_block)
        };

        // Starting from the current block (with hash `hash`), find
        // the next block with a hash that appears in the signature.
        loop {
            if matches(&mut st, sig, &buff, &hash) {
                break;
            }
            info!("block_oldest: {:?}", st.block_oldest);
            info!("block_len: {:?}", st.block_len);
            // The blocks are not equal. Move the hash by one byte
            // until finding an equal block.
            let oldest = buff[st.block_oldest];
            hash.remove(st.block_len, oldest);
            let r = r.read(&mut buff[st.block_oldest..st.block_oldest + 1])?;
            if r > 0 {
                // If there are still bytes to read, update the hash.
                hash.update(buff[st.block_oldest]);
            } else if st.block_len > 0 {
                // Else, just shrink the window, so that the current
                // block's blake2 hash can be compared with the
                // signature.
                st.block_len -= 1;
            } else {
                // We're done reading the file.
                break;
            }
            st.pending.push(oldest);
            info!("pending: {:?}", st.pending.len());
            st.block_oldest = (st.block_oldest + 1) % sig.window;
        }
        if !st.pending.is_empty() {
            // We've reached the end of the file, and have never found
            // a matching block again.
            st.result.push(Block::Literal(std::mem::replace(
                &mut st.pending,
                Vec::new(),
            )))
        }
    }
    Ok(Delta {
        blocks: st.result,
        window: sig.window,
    })
}

/// Compare a signature with an existing file. This is the second step
/// of the protocol, `r` is the local file when downloading, and the
/// remote file when uploading.
///
/// `block` must be a buffer the same size as `sig.window`.
pub fn compare_1<R: Read, B:AsRef<[u8]>+AsMut<[u8]>>(sig: &Signature, mut r: R, mut buff: B) -> Result<Delta, std::io::Error> {
    let mut st = State::new();
    let buff = buff.as_mut();
    assert_eq!(buff.len(), sig.window);
    while st.block_len > 0 {
        let mut hash = {
            let mut j = 0;
            let readed_block = {
                while j < sig.window {
                    let r = r.read(&mut buff[..])?;
                    if r == 0 {
                        info!("read breaked.");
                        break;
                    }
                    j += r
                }
                st.block_oldest = 0;
                st.block_len = j;
                &buff[..j]
            };
            adler32::RollingAdler32::from_buffer(readed_block)
        };

        // Starting from the current block (with hash `hash`), find
        // the next block with a hash that appears in the signature.
        loop {
            if matches(&mut st, sig, &buff, &hash) {
                break;
            }
            info!("block_oldest: {:?}", st.block_oldest);
            info!("block_len: {:?}", st.block_len);
            // The blocks are not equal. Move the hash by one byte
            // until finding an equal block.
            let oldest = buff[st.block_oldest];
            hash.remove(st.block_len, oldest);
            let r = r.read(&mut buff[st.block_oldest..st.block_oldest + 1])?;
            if r > 0 {
                // If there are still bytes to read, update the hash.
                hash.update(buff[st.block_oldest]);
            } else if st.block_len > 0 {
                // Else, just shrink the window, so that the current
                // block's blake2 hash can be compared with the
                // signature.
                st.block_len -= 1;
            } else {
                // We're done reading the file.
                break;
            }
            st.pending.push(oldest);
            info!("pending: {:?}", st.pending.len());
            st.block_oldest = (st.block_oldest + 1) % sig.window;
        }
        if !st.pending.is_empty() {
            // We've reached the end of the file, and have never found
            // a matching block again.
            st.result.push(Block::Literal(std::mem::replace(
                &mut st.pending,
                Vec::new(),
            )))
        }
    }
    info!("result len: {:?}", st.result.len());
    Ok(Delta {
        blocks: st.result,
        window: sig.window,
    })
}



fn matches(st: &mut State, sig: &Signature, block: &[u8], hash: &adler32::RollingAdler32) -> bool {
    if let Some(h) = sig.chunks.get(&hash.hash()) {
        let blake2 = {
            let mut b = blake2_rfc::blake2b::Blake2b::new(BLAKE2_SIZE);
            if st.block_oldest + st.block_len > sig.window {
                b.update(&block[st.block_oldest..]);
                b.update(&block[..(st.block_oldest + st.block_len) % sig.window]);
            } else {
                b.update(&block[st.block_oldest..st.block_oldest + st.block_len])
            }
            b.finalize()
        };

        if let Some(&index) = h.get(blake2.as_bytes()) {
            // Matching hash found! If we have non-matching
            // material before the match, add it.
            if !st.pending.is_empty() {
                st.result.push(Block::Literal(std::mem::replace(
                    &mut st.pending,
                    Vec::new(),
                )));
            }
            st.result.push(Block::FromSource(index as u64));
            return true;
        }
    }
    false
}

/// Restore a file, using a "delta" (resulting from
/// [`compare`](fn.compare.html))
pub fn restore<W: Write>(mut w: W, s: &[u8], delta: &Delta) -> Result<(), std::io::Error> {
    for d in delta.blocks.iter() {
        match *d {
            Block::FromSource(i) => {
                let i = i as usize;
                if i + delta.window <= s.len() {
                    w.write(&s[i..i + delta.window])?
                } else {
                    w.write(&s[i..])?
                }
            }
            Block::Literal(ref l) => w.write(l)?,
        };
    }
    Ok(())
}

/// Same as [`restore`](fn.restore.html), except that this function
/// uses a seekable, readable stream instead of the entire file in a
/// slice.
///
/// `buf` must be a buffer the same size as `sig.window`.
pub fn restore_seek<W: Write, R: Read + Seek, B: AsRef<[u8]>+AsMut<[u8]>>(
    mut w: W,
    mut s: R,
    mut buf: B,
    delta: &Delta,
) -> Result<(), std::io::Error> {
    let buf = buf.as_mut();

    for d in delta.blocks.iter() {
        match *d {
            Block::FromSource(i) => {
                s.seek(SeekFrom::Start(i as u64))?;
                // fill the buffer from r.
                let mut n = 0;
                loop {
                    let r = s.read(&mut buf[n..delta.window])?;
                    if r == 0 {
                        break;
                    }
                    n += r
                }
                // write the buffer to w.
                let mut m = 0;
                while m < n {
                    m += w.write(&buf[m..n])?;
                }
            }
            Block::Literal(ref l) => {
                w.write(l)?;
            }
        }
    }
    Ok(())
}



#[cfg(test)]
mod tests {
    use rand;
    use super::*;
    use rand::Rng;
    use std::{io};
    use rand::distributions::Alphanumeric;
    use crate::log_util;
    use tempfile;
    // use tokio_core::reactor::Core;
    const WINDOW: usize = 32;
    #[test]
    fn basic() {
        log_util::setup_logger(vec![""], vec![]);
        for index in 0..10 {
            let source = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(WINDOW * 10 + 8)
                .collect::<String>();
            let modified = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(WINDOW * 100 + 8)
                .collect::<String>();
            // let mut modified = source.clone();
            // let index = WINDOW * index + 3;
            // unsafe {
            //     modified.as_bytes_mut()[index] =
            //         ((source.as_bytes()[index] as usize + 1) & 255) as u8
            // }
            let block = [0; WINDOW];
            let source_sig = signature(source.as_bytes(), block).unwrap();
            // println!("source_sig: {:?}", source_sig);
            let comp = compare_1(&source_sig, modified.as_bytes(), block).unwrap();

            let mut restored = Vec::new();
            let source = std::io::Cursor::new(source.as_bytes());
            restore_seek(&mut restored, source, [0; WINDOW], &comp).unwrap();
            if &restored[..] != modified.as_bytes() {
                for i in 0..10 {
                    let a = &restored[i * WINDOW..(i + 1) * WINDOW];
                    let b = &modified.as_bytes()[i * WINDOW..(i + 1) * WINDOW];
                    println!("{:?}\n{:?}\n", a, b);
                    if a != b {
                        println!(">>>>>>>>");
                    }
                }
                panic!("different");
            }
        }
    }

    #[test]
    fn t_write_object() -> Result<(), failure::Error> {
        let tf = tempfile::tempfile()?;
        let mut br = io::BufReader::new(tf);
        let mut buf: [u8; 32] = [0; 32];
        let len = br.read(&mut buf)?;
        assert_eq!(len, 0);
        // to_be_bytes from_be_bytes.
        Ok(())
    }
}