use super::{record, Delta, WINDOW_FIELD_TYPE};
use log::*;
use std::convert::TryInto;
use std::path::Path;
use std::{
    fs, io,
    io::{Read, Write},
};

const TEN_MEGA_BYTES: usize = 10 * 1024 * 1024;

const LITERAL_FIELD_BYTE: u8 = 1;
const FROM_SOURCE_FIELD_BYTE: u8 = 2;

#[derive(Debug)]
/// The result of comparing two files
pub struct DeltaFile {
    wr: Option<record::RecordWriter<fs::File>>,
    rr: Option<record::RecordReader<fs::File>>,
    pending_file: Option<fs::File>,
    pending: Vec<u8>,
    window: usize,
    index: usize,
    pending_valve: usize,
}

impl DeltaFile {
    #[allow(dead_code)]
    pub fn create_delta_file(
        file: impl AsRef<Path>,
        window: usize,
        pending_valve: Option<usize>,
    ) -> Result<Self, failure::Error> {
        let mut wr = record::RecordWriter::<fs::File>::with_file_writer(file.as_ref())?;
        wr.write_field_usize(WINDOW_FIELD_TYPE, window)?;
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

    #[allow(dead_code)]
    pub fn read_delta_file(file: impl AsRef<Path>) -> Result<Self, failure::Error> {
        let mut rr = record::RecordReader::<fs::File>::with_file_reader(file.as_ref())?;
        let (_, window) = rr.read_field_usize()?.expect("should has window header.");
        info!("got window size from delta file: {}", window);
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
        let cur_wr = self
            .wr
            .as_mut()
            .expect("delta_file in wr mode, wr should'nt be None.");
        if let Some(pf) = self.pending_file.as_mut() {
            // write pending bytes to pending_file first. then write pending_file to record file.
            if !self.pending.is_empty() {
                pf.write_all(&self.pending[..])?;
            }
            cur_wr.write_field_from_file(LITERAL_FIELD_BYTE, pf)?;
            self.pending_file.take();
        } else if !self.pending.is_empty() {
            cur_wr.write_field_slice(LITERAL_FIELD_BYTE, &self.pending[..])?;
        }
        self.pending.clear();
        Ok(())
    }
}

impl Delta for DeltaFile {
    fn push_from_source(&mut self, position: u64) -> Result<(), failure::Error> {
        self.flush_pending()?;
        let wr = self
            .wr
            .as_mut()
            .expect("delta_file in wr mode, wr should'nt be None.");
        wr.write_field_u64(FROM_SOURCE_FIELD_BYTE, position)?;
        Ok(())
    }

    fn block_count(&mut self) -> Result<(usize, usize), failure::Error> {
        let cur_rr = self
            .rr
            .as_mut()
            .expect("delta_file in rr mode, rr should'nt be None.");
        let mut buf_v = vec![0_u8; self.window];
        let mut from_source_count = 0_usize;
        let mut literal_count = 0_usize;

        while let Some((field_type, mut field_len)) = cur_rr.read_field_header()? {
            info!(
                "got field_type: {:?}, field_len: {:?}",
                field_type, field_len
            );
            match field_type {
                FROM_SOURCE_FIELD_BYTE => {
                    if let Ok(Some(_)) = cur_rr.read_u64() {
                        from_source_count += 1;
                    } else {
                        bail!("read_u64 failed.")
                    }
                }
                LITERAL_FIELD_BYTE => {
                    let mut reader = cur_rr.inner_reader();
                    let window_u64: u64 =
                        self.window.try_into().expect("usize should convert to u64");

                    while field_len >= window_u64 {
                        reader.read_exact(&mut buf_v[..])?;
                        field_len -= window_u64;
                    }
                    if field_len > 0 {
                        let field_len_usize: usize =
                            field_len.try_into().expect("u64 should convert to usize.");
                        reader.read_exact(&mut buf_v[..field_len_usize])?;
                    }
                    literal_count += 1;
                }
                _ => {
                    bail!("got unexpected field_type");
                }
            }
        }
        Ok((from_source_count, literal_count))
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

    fn restore_seekable(
        &mut self,
        mut out: impl io::Write,
        mut old: impl io::Read + io::Seek,
    ) -> Result<(), failure::Error> {
        ensure!(
            self.rr.is_some(),
            "delta_file in rr mode, rr should'nt be None."
        );

        let mut buf_v = vec![0_u8; self.window];
        let cur_rr = self.rr.as_mut().expect("rr should exists.");

        while let Some((field_type, mut field_len)) = cur_rr.read_field_header()? {
            match field_type {
                FROM_SOURCE_FIELD_BYTE => {
                    if let Ok(Some(position)) = cur_rr.read_u64() {
                        DeltaFile::restore_from_source_seekable(
                            position,
                            &mut buf_v[..],
                            self.window,
                            &mut out,
                            &mut old,
                        )?;
                    } else {
                        bail!("read_u64 failed.")
                    }
                }
                LITERAL_FIELD_BYTE => {
                    let mut reader = cur_rr.inner_reader();
                    let window_u64: u64 =
                        self.window.try_into().expect("usize should convert to u64");

                    while field_len >= window_u64 {
                        reader.read_exact(&mut buf_v[..])?;
                        out.write_all(&buf_v[..])?;
                        field_len -= window_u64;
                    }
                    if field_len > 0 {
                        let field_len_usize: usize =
                            field_len.try_into().expect("u64 should convert to usize.");
                        reader.read_exact(&mut buf_v[..field_len_usize])?;
                        out.write_all(&buf_v[..field_len_usize])?;
                    }
                }
                _ => {
                    bail!("got unexpected field_type: {:?}", field_type);
                }
            }
        }
        Ok(())
    }
    fn restore(&mut self, mut _out: impl io::Write, mut _old: &[u8]) -> Result<(), failure::Error> {
        Ok(())
    }

    fn finishup(&mut self) -> Result<(), failure::Error> {
        self.flush_pending()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use crate::rustsync::{Signature};
    use rand;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    const WINDOW: usize = 32;

    #[test]
    fn t_delta_file_equal() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let source = vec![0_u8; 129]; // 129/WINDOW = 4 windows + 1 byte.
                                      // self.window = 4 + 1(field_type) +  8
                                      // 4 + 1 + 8 source_position = 13 * 4
                                      // 4 + 1 + 8 = 13
                                      // total size: 78
        let modified = source.clone();
        let buf = [0; WINDOW];
        let source_sig = Signature::signature(&source[..], buf)?;
        let delta_file = "target/cc.delta";
        DeltaFile::create_delta_file(delta_file, WINDOW, Some(10))?.compare(
            &source_sig,
            &modified[..],
            buf,
        )?;
        let delta_file_len = Path::new(delta_file).metadata()?.len();
        assert_eq!(delta_file_len, 78);

        let mut delta = DeltaFile::read_delta_file(delta_file)?;
        assert_eq!((5, 0), delta.block_count()?);

        let mut delta = DeltaFile::read_delta_file(delta_file)?;
        let mut restored = Vec::new();
        let source = std::io::Cursor::new(source);
        delta.restore_seekable(&mut restored, source)?;
        Ok(())
    }
    #[test]
    fn delta_file_basic() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        for index in 0..10 {
            let source = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(WINDOW * 10 + 8)
                .collect::<String>();
            let mut modified = source.clone();
            let index = WINDOW * index + 3;
            unsafe {
                modified.as_bytes_mut()[index] =
                    ((source.as_bytes()[index] as usize + 1) & 255) as u8
            }
            let buf = [0; WINDOW];
            let source_sig = Signature::signature(source.as_bytes(), buf)?;
            let delta_file = "target/cc.delta";
            DeltaFile::create_delta_file(delta_file, WINDOW, Some(3))?.compare(
                &source_sig,
                modified.as_bytes(),
                buf,
            )?;

            // compare(&source_sig, modified.as_bytes(), buf, delta)?;

            let mut delta = DeltaFile::read_delta_file(delta_file)?;
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
}
