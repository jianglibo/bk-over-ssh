mod record;

use log::*;
use std::collections::HashMap;
use std::{io, fs};
use std::path::Path;
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
    chunks: HashMap<u32, HashMap<Blake2b, usize>>,
}

/// Create the "signature" of a file, essentially a content-indexed
/// map of blocks. The first step of the protocol is to run this
/// function on the "source" (the remote file when downloading, the
/// local file while uploading).
pub fn signature<R: io::Read, B: AsRef<[u8]> + AsMut<[u8]>>(
    mut r: R,
    mut buff: B,
) -> Result<Signature, std::io::Error> {
    let mut chunks = HashMap::new();

    let mut i = 0;
    let buff = buff.as_mut();
    let mut eof = false;
    while !eof {
        let mut j = 0;
        while j < buff.len() {
            // full fill the block. for the last block, maybe not full.
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
        blake2.clone_from_slice(
            blake2_rfc::blake2b::blake2b(BLAKE2_SIZE, &[], &readed_block).as_bytes(),
        );
        // println!("blake2: {:?}", blake2.len());
        chunks
            .entry(hash.hash())
            .or_insert_with(HashMap::new)
            .insert(Blake2b(blake2), i);

        i += readed_block.len()
    }

    Ok(Signature {
        window: buff.len(),
        chunks,
    })
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum Block {
    FromSource(u64),
    Literal(Vec<u8>),
}

struct State {
    result: Vec<Block>,
    oldest_byte_position: usize,
    block_content_len: usize,
    pending: Vec<u8>,
}

impl State {
    fn new() -> Self {
        State {
            result: Vec::new(),
            oldest_byte_position: 0,
            block_content_len: 1,
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
pub fn compare<R: io::Read, B: AsRef<[u8]> + AsMut<[u8]>>(
    sig: &Signature,
    mut r: R,
    mut buff: B,
) -> Result<Delta, std::io::Error> {
    let mut st = State::new();
    let buff = buff.as_mut();
    assert_eq!(buff.len(), sig.window);
    while st.block_content_len > 0 {
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
                st.oldest_byte_position = 0;
                st.block_content_len = j;
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
            info!("block_oldest: {:?}", st.oldest_byte_position);
            info!("block_len: {:?}", st.block_content_len);
            // The blocks are not equal. Move the hash by one byte
            // until finding an equal block.
            let oldest_u8 = buff[st.oldest_byte_position];
            hash.remove(st.block_content_len, oldest_u8);
            let r = r.read(&mut buff[st.oldest_byte_position..=st.oldest_byte_position])?;
            if r > 0 {
                // If there are still bytes to read, update the hash.
                hash.update(buff[st.oldest_byte_position]);
            } else if st.block_content_len > 0 {
                // Else, just shrink the window, so that the current
                // block's blake2 hash can be compared with the
                // signature.
                st.block_content_len -= 1;
            } else {
                // We're done reading the file.
                break;
            }
            st.pending.push(oldest_u8);
            info!("pending: {:?}", st.pending.len());
            st.oldest_byte_position = (st.oldest_byte_position + 1) % sig.window;
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

pub trait PendingStore {
    fn push_byte(&mut self, byte: u8);
    fn get_reader<R: io::Read>(&self) -> io::BufReader<R>;
}

fn matches(st: &mut State, sig: &Signature, block: &[u8], hash: &adler32::RollingAdler32) -> bool {
    if let Some(h) = sig.chunks.get(&hash.hash()) {
        let blake2 = {
            let mut b = blake2_rfc::blake2b::Blake2b::new(BLAKE2_SIZE);
            if st.oldest_byte_position + st.block_content_len > sig.window {
                b.update(&block[st.oldest_byte_position..]);
                b.update(&block[..(st.oldest_byte_position + st.block_content_len) % sig.window]);
            } else {
                b.update(&block[st.oldest_byte_position..st.oldest_byte_position + st.block_content_len])
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
pub fn restore<W: io::Write>(mut w: W, s: &[u8], delta: &Delta) -> Result<(), std::io::Error> {
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
pub fn restore_seek<W: io::Write, R: io::Read + io::Seek, B: AsRef<[u8]> + AsMut<[u8]>>(
    mut w: W,
    mut s: R,
    mut buf: B,
    delta: &Delta,
) -> Result<(), std::io::Error> {
    let buf = buf.as_mut();

    for d in delta.blocks.iter() {
        match *d {
            Block::FromSource(i) => {
                s.seek(io::SeekFrom::Start(i as u64))?;
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
                w.write_all(l)?;
            }
        }
    }
    Ok(())
}

pub fn signature_a_file(file_name: impl AsRef<Path>, buf_size: Option<usize>, out_file: Option<&str>) -> Result<Option<Signature>, failure::Error> {
    let f = fs::OpenOptions::new().read(true).open(file_name.as_ref())?;
    let mut br = io::BufReader::new(f); 
    let mut buf = vec![0_u8; buf_size.unwrap_or(2048)];
    let sig = signature(&mut br, &mut buf[..])?;
    if let Some(out) = out_file {
        let f = fs::OpenOptions::new().create(true).write(true).truncate(true).open(out)?;
        serde_json::to_writer_pretty(f, &sig)?;
        Ok(None)
    } else {
        Ok(Some(sig))
    }
}

pub fn write_signature_to_file(sig: &Signature, file_name: impl AsRef<Path>) -> Result<(), failure::Error> {
    let mut wr = record::RecordWriter::<fs::File>::with_file_writer(file_name.as_ref())?;
    wr.write_field_slice(0_u8, &sig.window.to_be_bytes())?;
    for (k, v) in sig.chunks.iter() {
        let u32_bytes = k.to_be_bytes();
        let mut v_bytes = Vec::new();
        v_bytes.extend_from_slice(&u32_bytes);
        for (kk, vv) in v.iter() {
            v_bytes.extend_from_slice(&kk.0);
            v_bytes.extend_from_slice(&vv.to_be_bytes());
        }
        wr.write_field_slice(1_u8, v_bytes.as_slice())?;
    }
    Ok(())
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
            // let modified = rand::thread_rng()
            //     .sample_iter(&Alphanumeric)
            //     .take(WINDOW * 100 + 8)
            //     .collect::<String>();
            let mut modified = source.clone();
            let index = WINDOW * index + 3;
            unsafe {
                modified.as_bytes_mut()[index] =
                    ((source.as_bytes()[index] as usize + 1) & 255) as u8
            }
            let block = [0; WINDOW];
            let source_sig = signature(source.as_bytes(), block).unwrap();
            // println!("source_sig: {:?}", source_sig);
            let comp = compare(&source_sig, modified.as_bytes(), block).unwrap();

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
    fn t_u32() -> Result<(), failure::Error> {
        let mut cursor = io::Cursor::<Vec<u8>>::new(Vec::new());
        assert_eq!(cursor.position(), 0);
        let a_u32 = 55_u32;
        cursor.write_all(&a_u32.to_be_bytes())?;
        assert_eq!(cursor.position(), 4);
        cursor.seek(io::SeekFrom::Start(0))?;
        let mut b = [0_u8; 4];
        cursor.read_exact(&mut b)?;
        let b_u32 = u32::from_be_bytes(b);
        assert_eq!(b_u32, a_u32);
        Ok(())
    }


    #[test]
    fn t_write_object() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let tf = tempfile::tempfile()?;
        let mut br = io::BufReader::new(tf);
        let mut buf: [u8; 32] = [0; 32];
        let len = br.read(&mut buf)?;
        assert_eq!(len, 0);
        // to_be_bytes from_be_bytes.
        let mut cursor = io::Cursor::new(vec![1,2 ,3]);
        let mut buf = [0; 1024];
        info!("{:?}", cursor);
        let size = cursor.read(&mut buf)?;
        info!("{:?}, position: {:?}", size, cursor.position());
        // assert_eq!(size, 15);
        // let mut v = Vec::<u8>::new();
        // let ios = io::IoSliceMut::new(&mut v);
        // let size = buff.read_vectored(&mut [ios])?;
        // assert_eq!(size, 15);
        Ok(())
    }

    #[test]
    fn t_signature_large_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let dev_env = develope_data::load_env();
        let start = Instant::now();
        signature_a_file(&dev_env.servers.ubuntu18.test_files.midum_binary_file, Some(2048), None)?;
        info!("time elapsed: {:?}", start.elapsed().as_secs());
        let start = Instant::now();
        signature_a_file(&dev_env.servers.ubuntu18.test_files.midum_binary_file, Some(4096), None)?;
        info!("time elapsed: {:?}", start.elapsed().as_secs());
        Ok(())
    }
}
