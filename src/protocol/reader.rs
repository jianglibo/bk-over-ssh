use super::{HeaderParseError, TransferType};
use std::convert::TryInto;
use std::io::{self, Read};

pub struct ProtocolReader<'a, T>
where
    T: io::Read,
{
    inner: &'a mut T,
    pub remains: Vec<u8>,
}

impl<'a, T> ProtocolReader<'a, T>
where
    T: io::Read,
{
    pub fn new(inner: &'a mut T) -> ProtocolReader<'a, T> {
        ProtocolReader {
            inner,
            remains: Vec::new(),
        }
    }

    pub fn read_one_byte(&mut self) -> Result<u8, HeaderParseError> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf).map_err(HeaderParseError::Io)?;
        Ok(buf[0])
    }
    pub fn read_type_byte(&mut self) -> Result<TransferType, HeaderParseError> {
        TransferType::from_u8(self.read_one_byte()?)
    }

    pub fn read_nbytes(
        &mut self,
        buf: &mut [u8],
        how_much: u64,
    ) -> Result<Vec<u8>, HeaderParseError> {
        let mut read_count = 0_u64;
        let mut result = Vec::new();
        loop {
            let readed = self.read(buf).map_err(HeaderParseError::Io)?;
            if readed == 0 {
                break;
            }
            result.extend_from_slice(&buf[..readed]);
            read_count += readed as u64;
            if read_count >= how_much {
                if read_count > how_much {
                    let mut new_remains = result.split_off(
                        how_much
                            .try_into()
                            .expect("how_much convert from u64 to usize."),
                    );
                    if self.remains.is_empty() {
                        self.remains = new_remains;
                    } else {
                        // if not empty, all bytes read is from sefl.remains, put back to it.
                        new_remains.append(&mut self.remains);
                        self.remains = new_remains;
                    }
                }
                break;
            }
        }
        if read_count < how_much {
            Err(HeaderParseError::InsufficientBytes(how_much, read_count))
        } else {
            Ok(result)
        }
    }
}

impl<'a, T> Read for ProtocolReader<'a, T>
where
    T: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remains_len = self.remains.len();
        if remains_len > 0 {
            let buf_len = buf.len();
            if remains_len >= buf_len {
                let new_remains = self.remains.split_off(buf_len);
                buf.copy_from_slice(&self.remains[..]);
                self.remains = new_remains;
                Ok(buf.len())
            } else {
                buf[..remains_len].copy_from_slice(&self.remains[..]);
                self.remains.clear();
                Ok(remains_len)
            }
        } else {
            self.inner.read(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use failure;
    use std::io::{self, Cursor, Read, StdinLock};

    fn get_pr<'a, 'b: 'a>(
        stdin_handler: &'a mut std::io::StdinLock<'b>,
    ) -> ProtocolReader<'a, StdinLock<'b>> {
        ProtocolReader {
            inner: stdin_handler,
            remains: b"hello".to_vec(),
        }
    }

    #[test]
    fn t_read_nbytes() -> Result<(), failure::Error> {
        let mut curor = Cursor::new(b" world".to_vec());
        let mut pr = ProtocolReader {
            inner: &mut curor,
            remains: b"hello".to_vec(),
        };

        let mut buf = [0; 2];
        let result = pr.read_nbytes(&mut buf, 8)?;
        assert_eq!(&result[..], b"hello wo");
        assert_eq!(pr.remains.len(), 1);
        assert_eq!(&pr.remains[..], b"r");
        assert_eq!(curor.position(), 4); // 'r' is in the remains.

        let mut curor = Cursor::new(b" world".to_vec());
        let mut pr = ProtocolReader {
            inner: &mut curor,
            remains: b"hello".to_vec(),
        };

        assert!(
            pr.read_nbytes(&mut buf, 100).is_err(),
            "insufficient bytes."
        );

        Ok(())
    }

    #[test]
    fn t_protocol_cursor() -> Result<(), failure::Error> {
        let mut curor = Cursor::new(b" world".to_vec());
        let mut pr = ProtocolReader {
            inner: &mut curor,
            remains: b"hello".to_vec(),
        };

        let mut buf = [0; 2];
        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 2);
        assert_eq!(&buf, b"he");
        assert_eq!(pr.remains.len(), 3);
        assert_eq!(&pr.remains[..], b"llo");

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 2);
        assert_eq!(&buf, b"ll");
        assert_eq!(pr.remains.len(), 1);
        assert_eq!(&pr.remains[..], b"o");

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 1);
        assert_eq!(&buf[..1], b"o");
        assert_eq!(pr.remains.len(), 0);
        assert_eq!(&pr.remains[..], b"");

        let mut buf = [0; 10];
        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 6);
        assert_eq!(&buf[..6], b" world");
        assert_eq!(pr.remains.len(), 0);
        assert_eq!(&pr.remains[..], b"");

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 0);

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 0);

        Ok(())
    }

    #[test]
    fn t_protocol_stdinlock() -> Result<(), failure::Error> {
        let stdin = io::stdin();
        let mut stdin_handler = stdin.lock();

        let mut pr = get_pr(&mut stdin_handler);

        let mut buf = [0; 2];
        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 2);
        assert_eq!(&buf, b"he");
        assert_eq!(pr.remains.len(), 3);
        assert_eq!(&pr.remains[..], b"llo");

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 2);
        assert_eq!(&buf, b"ll");
        assert_eq!(pr.remains.len(), 1);
        assert_eq!(&pr.remains[..], b"o");

        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 1);
        assert_eq!(&buf[..1], b"o");
        assert_eq!(pr.remains.len(), 0);
        assert_eq!(&pr.remains[..], b"");

        let mut pr = get_pr(&mut stdin_handler);
        let mut buf = [0; 5];
        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 5);
        assert_eq!(&buf, b"hello");
        assert_eq!(pr.remains.len(), 0);
        assert_eq!(&pr.remains[..], b"");

        let mut pr = get_pr(&mut stdin_handler);
        let mut buf = [0; 15];
        let readed = pr.read(&mut buf)?;
        assert_eq!(readed, 5);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(pr.remains.len(), 0);
        assert_eq!(&pr.remains[..], b"");
        Ok(())
    }
}
