// https://stackoverflow.com/questions/27567849/what-makes-something-a-trait-object
// https://abronan.com/rust-trait-objects-box-and-rc/
mod delta_file;
mod delta_mem;
mod record;

use log::*;
use std::collections::HashMap;
use std::path::Path;
use std::{fs, io};

pub use delta_file::{DeltaFileWriter, DeltaFileReader};
// use tokio_io::{AsyncRead, AsyncWrite};
// use tokio_io::io::{write_all, WriteAll};
// use futures::{Async, Future, Poll};

use serde::{Deserialize, Serialize};

const BLAKE2_SIZE: usize = 32;

pub const WINDOW_FIELD_TYPE: u8 = 0;

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

impl Signature {
    #[allow(dead_code)]
    pub fn write_to_file(&mut self, file_name: impl AsRef<Path>) -> Result<(), failure::Error> {
        let writer = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(file_name.as_ref())?;
        self.write_to_stream(writer)
    }

    pub fn write_to_stream(&mut self, wr: impl io::Write) -> Result<(), failure::Error> {
        let mut wr = record::RecordWriter::new(wr);
        wr.write_field_usize(WINDOW_FIELD_TYPE, self.window)?;
        // u32(4) + blake2_bytes(32) + position(4 or 8);
        for (k, v) in self.chunks.iter() {
            let u32_bytes = k.to_be_bytes();
            let mut v_bytes = Vec::new();
            v_bytes.extend_from_slice(&u32_bytes);
            for (blake2_bytes, position) in v.iter() {
                v_bytes.extend_from_slice(&blake2_bytes.0);
                v_bytes.extend_from_slice(&position.to_be_bytes());
            }
            wr.write_field_slice(1_u8, v_bytes.as_slice())?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_signature_file(file_name: impl AsRef<Path>) -> Result<Signature, failure::Error> {
        let mut rr = record::RecordReader::<fs::File>::with_file_reader(file_name.as_ref())?;
        if let Some((_field_type, u8_vec)) = rr.read_field_slice()? {
            let mut chunks = HashMap::new();
            let _usize_size = std::mem::size_of::<usize>();
            let mut ary = [0_u8; 8];
            ary.copy_from_slice(&u8_vec[..8]);
            let window = usize::from_be_bytes(ary);
            while let Some((_field_type, u8_vec)) = rr.read_field_slice()? {
                let mut alder32_bytes = [0_u8; 4];
                alder32_bytes.copy_from_slice(&u8_vec[..4]);
                let alder32_value = u32::from_be_bytes(alder32_bytes);

                let mut i = 4;
                while i < u8_vec.len() {
                    let mut blake2_bytes = [0_u8; BLAKE2_SIZE];
                    blake2_bytes.copy_from_slice(&u8_vec[i..i + BLAKE2_SIZE]);
                    let mut position = [0_u8; 8];
                    position.copy_from_slice(&u8_vec[i + BLAKE2_SIZE..i + BLAKE2_SIZE + 8]);
                    let position_usize = usize::from_be_bytes(position);
                    chunks
                        .entry(alder32_value)
                        .or_insert_with(HashMap::new)
                        .insert(Blake2b(blake2_bytes), position_usize);
                    i += BLAKE2_SIZE + 8;
                }
            }
            let sig = Signature { window, chunks };
            Ok(sig)
        } else {
            bail!("damaged signature file.");
        }
    }

    /// Create the "signature" of a file, essentially a content-indexed
    /// map of blocks. The first step of the protocol is to run this
    /// function on the "source" (the remote file when downloading, the
    /// local file while uploading).
    pub fn signature<R: io::Read, B: AsRef<[u8]> + AsMut<[u8]>>(
        mut r: R,
        mut buff: B,
    ) -> Result<Signature, failure::Error> {
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

    pub fn signature_a_file(
        file_name: impl AsRef<Path>,
        buf_size: Option<usize>,
    ) -> Result<Signature, failure::Error> {
        let f = fs::OpenOptions::new().read(true).open(file_name.as_ref())?;
        let mut br = io::BufReader::new(f);
        let mut buf = vec![0_u8; buf_size.unwrap_or(2048)];
        Signature::signature(&mut br, &mut buf[..])
    }
}

struct State {
    // result: Vec<Block>,
    oldest_byte_position: usize,
    block_content_len: usize,
    // pending: Vec<u8>,
}

impl State {
    fn new() -> Self {
        State {
            // result: Vec::new(),
            oldest_byte_position: 0,
            block_content_len: 1,
            // pending: Vec::new(),
        }
    }
}

pub trait DeltaReader {
    fn block_count(&mut self) -> Result<(usize, usize), failure::Error>;
    
    fn restore_seekable(
        &mut self,
        out: impl io::Write,
        old: impl io::Read + io::Seek,
    ) -> Result<(), failure::Error>;

    fn restore(&mut self, out: impl io::Write, old: &[u8]) -> Result<(), failure::Error>;

    fn restore_from_source_seekable(
        source_position: u64,
        mut buf: impl AsRef<[u8]> + AsMut<[u8]>,
        window: usize,
        out: &mut impl io::Write,
        old: &mut (impl io::Read + io::Seek),
    ) -> Result<(), failure::Error> {
        let buf = buf.as_mut();
        old.seek(io::SeekFrom::Start(source_position))?;
        // fill the buffer from r.
        let mut n = 0;
        loop {
            let r = old.read(&mut buf[n..window])?;
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
        Ok(())
    }
}

pub trait DeltaWriter {
    fn push_from_source(&mut self, position: u64) -> Result<(), failure::Error>;
    fn push_byte(&mut self, byte: u8) -> Result<(), failure::Error>;
    fn finishup(&mut self) -> Result<(), failure::Error>;

    fn compare<R: io::Read>(
        &mut self,
        sig: &Signature,
        mut r: R,
    ) -> Result<(), failure::Error> {
        let mut st = State::new();
        let mut buff = vec![0_u8; sig.window];
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
                let matched: bool = if let Some(h) = sig.chunks.get(&hash.hash()) {
                    let blake2 = {
                        let mut b = blake2_rfc::blake2b::Blake2b::new(BLAKE2_SIZE);
                        if st.oldest_byte_position + st.block_content_len > sig.window {
                            b.update(&buff[st.oldest_byte_position..]);
                            b.update(
                                &buff[..(st.oldest_byte_position + st.block_content_len)
                                    % sig.window],
                            );
                        } else {
                            b.update(
                                &buff[st.oldest_byte_position
                                    ..st.oldest_byte_position + st.block_content_len],
                            )
                        }
                        b.finalize()
                    };
                    if let Some(&index) = h.get(blake2.as_bytes()) {
                        self.push_from_source(index as u64)?;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                if matched {
                    break;
                }
                // info!("block_oldest: {:?}", st.oldest_byte_position);
                // info!("block_len: {:?}", st.block_content_len);
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
                self.push_byte(oldest_u8)?;
                // info!("pending: {:?}", st.pending.len());
                st.oldest_byte_position = (st.oldest_byte_position + 1) % sig.window;
            }
            self.finishup()?;
        }
        Ok(())
    }
}

pub trait Delta {



}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use log::*;
    use rand;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use std::io;
    use std::io::{Read, Seek, Write};
    use std::time::Instant;
    use tempfile;
    // use tokio_core::reactor::Core;
    const WINDOW: usize = 32;
    #[test]
    fn basic() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
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
            let source_sig = Signature::signature(source.as_bytes(), block)?;
            // println!("source_sig: {:?}", source_sig);
            // let mut blocks = Vec::new();
            let mut delta = delta_mem::DeltaMem::new(WINDOW);
            delta.compare(&source_sig, modified.as_bytes())?;
            // let mut delta = compare(&source_sig, modified.as_bytes(), block, delta).unwrap();

            let mut restored = Vec::new();
            let source = std::io::Cursor::new(source.as_bytes());
            delta.restore_seekable(&mut restored, source)?;
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
        Ok(())
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
        log_util::setup_logger_empty();
        let tf = tempfile::tempfile()?;
        let mut br = io::BufReader::new(tf);
        let mut buf: [u8; 32] = [0; 32];
        let len = br.read(&mut buf)?;
        assert_eq!(len, 0);
        // to_be_bytes from_be_bytes.
        let mut cursor = io::Cursor::new(vec![1, 2, 3]);
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
        log_util::setup_logger_empty();
        let start = Instant::now();
        let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024*1024*4)?;
        let demo_file = test_dir.tmp_file_str()?;
        let mut sig = Signature::signature_a_file(&demo_file, Some(4096))?;

        let sig_out = "target/cc.sig";
        sig.write_to_file(sig_out)?;

        let demo_file_length = Path::new(&demo_file).metadata()?.len();
        let sig_out_length = Path::new(sig_out).metadata()?.len();
        info!(
            "sig file length: {:?}, origin: {:?}, percent: {:?}",
            sig_out_length,
            demo_file_length,
            (sig_out_length as f64 / demo_file_length as f64) * 100.0
        );
        info!("signature time elapsed: {:?}", start.elapsed().as_secs());
        let start = Instant::now();
        let new_sig = Signature::load_signature_file(sig_out)?;
        info!(
            "load signature time elapsed: {:?}",
            start.elapsed().as_secs()
        );
        assert_eq!(sig, new_sig);
        Ok(())
    }

    #[test]
    fn t_other() -> Result<(), failure::Error> {
        assert_eq!(std::mem::size_of::<usize>(), 8);
        let a = 5_u64;
        let b = 10_u64;
        // assert_eq!(0.5_f32, a as f32 / b as f32);

        let mut tf = tempfile::tempfile()?;
        tf.write_all(b"hello")?;

        assert_eq!(tf.metadata()?.len(), 5);
        enum Aenum {
            A(u8),
            B(u32),
        }

        impl Aenum {
            pub fn set_value(&mut self, _v: u8) {
                match self {
                    Self::A(_) => {}
                    Self::B(_) => {}
                }
            }
        }
        Ok(())
    }
}
