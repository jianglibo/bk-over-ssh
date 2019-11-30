pub mod error;
pub mod exchange;

pub use error::HeaderParseError;
pub use exchange::{StringMessage, TransferType, U64Message};
use log::*;
use ssh2;
use std::convert::TryInto;
use std::fs;
use std::io::{self, Cursor, Read, StdinLock, StdoutLock, Write};
use std::path::Path;

/// Only this method aware of underlying reader!!!
fn read_inner(
    underlying_reader: &mut impl Read,
    remains: &mut Vec<u8>,
    buf: &mut [u8],
) -> io::Result<usize> {
    let remains_len = remains.len();
    if remains_len > 0 {
        let buf_len = buf.len();
        if remains_len >= buf_len {
            let mut new_remains = remains.split_off(buf_len);
            buf.copy_from_slice(&remains[..]);
            remains.clear();
            remains.append(&mut new_remains);
            Ok(buf.len())
        } else {
            buf[..remains_len].copy_from_slice(&remains[..]);
            remains.clear();
            Ok(remains_len)
        }
    } else {
        underlying_reader.read(buf)
    }
}

pub trait MessageHub: Read + Write {
    fn get_remains(&mut self) -> &mut Vec<u8>;

    fn read_one_byte(&mut self) -> Result<u8, HeaderParseError> {
        let mut buf = [0; 1];
        match self.read_exact(&mut buf) {
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                error!("{:?}", err);
                Ok(0)
            }
            Err(err) => Err(HeaderParseError::Io(err)),
            Ok(_) => Ok(buf[0]),
        }
    }
    fn read_type_byte(&mut self) -> Result<TransferType, HeaderParseError> {
        trace!("start read_type_byte");
        let v = match self.read_one_byte() {
            Ok(b) => Ok(TransferType::from_u8(b)?),
            Err(err) => {
                error!("{:?}", err);
                Err(err)
            }
        };
        trace!("end read_type_byte {:?}.", v);
        v
    }
    /// must unware of the underlying reader.
    fn read_nbytes(&mut self, buf: &mut [u8], how_much: u64) -> Result<Vec<u8>, HeaderParseError> {
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
                    let at_to_end = result.split_off(
                        how_much
                            .try_into()
                            .expect("how_much convert from u64 to usize."),
                    );
                    self.get_remains().splice(0..0, at_to_end.iter().cloned());
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

    fn copy_to_file(
        &mut self,
        buf: &mut [u8],
        len: u64,
        file_path: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let mut count = len;
        let file_path = file_path.as_ref();
        let parent = file_path
            .parent()
            .expect("copy_to_file should has a parent.");
        if !parent.exists() {
            fs::create_dir_all(&parent)?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(file_path)?;
        loop {
            let readed = self.read(buf)?;
            if readed == 0 {
                break;
            }
            if count >= readed as u64 {
                f.write_all(&buf[..readed])?;
            } else {
                let mut new_remains = (&buf[count as usize..readed]).to_vec();
                self.get_remains().append(&mut new_remains);
                f.write_all(&buf[..count as usize])?;
                break;
            }
            count -= readed as u64;
        }
        Ok(())
    }

    fn write_and_flush(&mut self, bytes: &[u8]) -> io::Result<()> {
        // trace!("start send bytes: {:?}", bytes);
        self.write_all(bytes)?;
        self.flush()
        // trace!("end send bytes.");
        // r
    }

    fn write_error_message(&mut self, message: impl AsRef<str>) -> io::Result<()> {
        let string_message = StringMessage::new(message);
        self.write_and_flush(&string_message.as_string_error_sent_bytes())
    }

    fn write_transfer_type_only(&mut self, transfer_type: TransferType) -> io::Result<()> {
        self.write_and_flush(&[transfer_type.to_u8()])
    }
}

pub struct SshChannelMessageHub {
    channel: ssh2::Channel,
    remains: Vec<u8>,
}

impl SshChannelMessageHub {
    pub fn new(channel: ssh2::Channel) -> Self {
        Self {
            channel,
            remains: Vec::new(),
        }
    }
}

impl MessageHub for SshChannelMessageHub {
    fn get_remains(&mut self) -> &mut Vec<u8> {
        &mut self.remains
    }
}

impl Write for SshChannelMessageHub {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.channel.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.channel.flush()
    }
}

impl Read for SshChannelMessageHub {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_inner(&mut self.channel, &mut self.remains, buf)
    }
}

pub struct StdInOutMessageHub<'a> {
    stdin: StdinLock<'a>,
    stdout: StdoutLock<'a>,
    remains: Vec<u8>,
}

impl<'a> StdInOutMessageHub<'a> {
    pub fn new(stdin: StdinLock<'a>, stdout: StdoutLock<'a>) -> Self {
        Self {
            stdin,
            stdout,
            remains: Vec::new(),
        }
    }
}

impl<'a> MessageHub for StdInOutMessageHub<'a> {
    fn get_remains(&mut self) -> &mut Vec<u8> {
        &mut self.remains
    }
}

impl<'a> Write for StdInOutMessageHub<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stdout.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

impl<'a> Read for StdInOutMessageHub<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_inner(&mut self.stdin, &mut self.remains, buf)
    }
}

pub struct CursorMessageHub<'a> {
    cursor: &'a mut Cursor<Vec<u8>>,
    remains: Vec<u8>,
}

impl<'a> CursorMessageHub<'a> {
    #[allow(dead_code)]
    pub fn new(cursor: &'a mut Cursor<Vec<u8>>) -> Self {
        CursorMessageHub {
            cursor,
            remains: Vec::new(),
        }
    }
}

impl<'a> MessageHub for CursorMessageHub<'a> {
    fn get_remains(&mut self) -> &mut Vec<u8> {
        &mut self.remains
    }
}

impl<'a> Write for CursorMessageHub<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.cursor.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.cursor.flush()
    }
}

impl<'a> Read for CursorMessageHub<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_inner(&mut self.cursor, &mut self.remains, buf)
    }
}

#[cfg(test)]
mod tests {
    use failure;

    #[test]
    fn t_vec_splice() -> Result<(), failure::Error> {
        let mut v = vec![1, 2, 3];
        let new = [7, 8];
        let u: Vec<_> = v.splice(0..0, new.iter().cloned()).collect();
        assert_eq!(v, &[7, 8, 1, 2, 3]);
        assert_eq!(u.len(), 0);

        Ok(())
    }
}
