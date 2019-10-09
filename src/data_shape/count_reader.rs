use std::io::{self, Read};

pub struct CountReadr<T, F> where T: Read, F: FnMut(usize) -> () {
    r: T,
    f: F,
}

impl<T, F> CountReadr<T, F> where T: Read, F: FnMut(usize) -> () {
    pub fn new(r: T, f: F) -> Self {
        Self { r, f }
    }
}

impl<T, F> Read for CountReadr<T, F> where T: Read, F: FnMut(usize) -> () {

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let i = self.r.read(buf)?;
        (self.f)(i);
        Ok(i)
    }
}