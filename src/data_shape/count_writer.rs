use std::io::{self, Write};

pub struct CountWriter<T, F> where T: Write, F: FnMut(u64) -> (), {
    w: T,
    c: F,
}

impl<T, F> CountWriter<T, F> where T: Write, F: FnMut(u64) -> (), {
    pub fn new(w: T, c: F) -> Self {
        Self { w, c }
    }
}

impl<T, F> Write for CountWriter<T, F> where T: Write, F: FnMut(u64) -> (), {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let num = self.w.write(buf)?;
        (self.c)(num as u64);
        Ok(num)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
     }
}