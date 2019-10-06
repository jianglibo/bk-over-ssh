use std::io::{self, Write};
use crate::data_shape::{Indicator};

pub struct CountWriter<'a, T> where T: Write {
    w: T,
    c: &'a Indicator,
}

impl<'a, T> CountWriter<'a, T> where T: Write {
    pub fn new(w: T, c: &'a Indicator) -> Self {
        Self { w, c }
    }
}

impl<'a, T> Write for CountWriter<'a, T> where T: Write {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let num = self.w.write(buf)?;
        self.c.inc_pb(num as u64);
        Ok(num)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
     }
}