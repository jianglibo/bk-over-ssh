use std::io::{self, Read};
use crate::data_shape::{Indicator};
use sha1::{Digest, Sha1};

#[allow(dead_code)]
pub struct Sha1Reader<'a, T> where T: Read {
    r: T,
    c: &'a Indicator,
    hasher: Option<sha1::Sha1>,
    length: usize,
}

#[allow(dead_code)]
impl<'a, T> Sha1Reader<'a, T> where T: Read {
    pub fn new(r: T, c: &'a Indicator) -> Self {
        Sha1Reader { r, c, hasher: Some(Sha1::new()), length: 0}
    }

    pub fn get_sha1(&mut self) -> String {
        format!("{:X}", self.hasher.take().unwrap().result())
    }

    pub fn get_length(&mut self) -> u64 {
        self.length as u64
    }
}

impl<'a, T> Read for Sha1Reader<'a, T> where T: Read {

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let i = self.r.read(buf)?;
        self.length += i;
        self.c.inc_pb(i as u64);
        Ok(i)
    }
}