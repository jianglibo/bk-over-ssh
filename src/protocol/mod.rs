pub mod error;
pub mod exchange;
pub mod reader;

pub use error::HeaderParseError;
pub use exchange::{CopyOutHeader, StringMessage, TransferType, U64Message};
use log::*;
pub use reader::ProtocolReader;
use ssh2;
use std::convert::TryInto;
use std::io::{self, Read, Write};

pub trait MessageHub<R, W>
where
    R: Read,
    W: Write,
{
    fn get_reader(&mut self) -> &mut R;
    fn get_writer(&mut self) -> &mut W;
    fn get_remains(&mut self) -> &mut Vec<u8>;

    fn read_one_byte(&mut self) -> Result<u8, HeaderParseError> {
        let mut buf = [0; 1];
        match self.get_reader().read_exact(&mut buf) {
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

    fn read_nbytes(&mut self, buf: &mut [u8], how_much: u64) -> Result<Vec<u8>, HeaderParseError> {
        let mut read_count = 0_u64;
        let mut result = Vec::new();
        loop {
            let readed = self.get_reader().read(buf).map_err(HeaderParseError::Io)?;
            if readed == 0 {
                break;
            }
            result.extend_from_slice(&buf[..readed]);
            read_count += readed as u64;
            if read_count >= how_much {
                if read_count > how_much {
                    let new_remains = result.split_off(
                        how_much
                            .try_into()
                            .expect("how_much convert from u64 to usize."),
                    );
                    self.get_remains().splice(0..0, new_remains.iter().cloned());
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

pub struct SshChannelMessageHub {
    channel: ssh2::Channel,
    remains: Vec<u8>,
}

impl MessageHub<ssh2::Channel, ssh2::Channel> for SshChannelMessageHub {
    fn get_reader(&mut self) -> &mut ssh2::Channel {
        &mut self.channel
    }

    fn get_writer(&mut self) -> &mut ssh2::Channel {
        &mut self.channel
    }

    fn get_remains(&mut self) -> &mut Vec<u8> {
        &mut self.remains
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
