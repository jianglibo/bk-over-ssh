use super::{HeaderParseError, ProtocolReader};
use std::convert::TryInto;
use std::io::{self, Read, StdinLock};

#[derive(Debug)]
pub enum TransferType {
    CopyIn,
    CopyOut,
    RsyncIn,
    RsyncOut,
    ListFiles,
}

impl TransferType {
    pub fn from_u8(u8_value: u8) -> Result<TransferType, HeaderParseError> {
        match u8_value {
            1 => Ok(TransferType::CopyIn),
            2 => Ok(TransferType::CopyOut),
            3 => Ok(TransferType::RsyncIn),
            4 => Ok(TransferType::RsyncOut),
            5 => Ok(TransferType::ListFiles),
            i => Err(HeaderParseError::InvalidTransferType(i)),
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            TransferType::CopyIn => 1,
            TransferType::CopyOut => 2,
            TransferType::RsyncIn => 3,
            TransferType::RsyncOut => 4,
            TransferType::ListFiles => 5,
        }
    }
}

#[derive(Debug)]
pub struct CopyOutHeader {
    pub content_len: u64,
    pub offset: u64,
    pub full_file_name: String,
}

impl CopyOutHeader {
    pub fn new(content_len: u64, offset: u64, full_file_name: impl AsRef<str>) -> Self {
        Self {
            content_len,
            offset,
            full_file_name: full_file_name.as_ref().to_owned(),
        }
    }

    pub fn into_bytes(&mut self) -> Vec<u8> {
        let mut v = Vec::new();
        v.insert(0, TransferType::CopyOut.to_u8());
        v.append(&mut self.content_len.to_be_bytes().to_vec());
        v.append(&mut self.offset.to_be_bytes().to_vec());
        let file_name_len: u16 =  self.full_file_name.len().try_into().expect("file name length is in limit of u16");
        v.append(&mut file_name_len.to_be_bytes().to_vec());
        v.append(&mut self.full_file_name.as_bytes().to_vec());
        v
    }
}

#[allow(dead_code)]
pub fn parse_copy_out_header<T>(
    protocol_reader: &mut ProtocolReader<T>,
) -> Result<CopyOutHeader, HeaderParseError> where T : Read {
    let mut buf = [0; 1];
    protocol_reader.read_exact(&mut buf).map_err(HeaderParseError::Io)?;
    let type_num = *buf.get(0).expect("buf first byte.");
    let transfer_type = TransferType::from_u8(type_num)?;

    if let TransferType::CopyOut = transfer_type {
        let mut buf_u64 = [0; 8];

        protocol_reader
            .read_exact(&mut buf_u64)
            .map_err(HeaderParseError::Io)?;
        let content_len: u64 = u64::from_be_bytes(buf_u64);

        protocol_reader
            .read_exact(&mut buf_u64)
            .map_err(HeaderParseError::Io)?;
        let offset: u64 = u64::from_be_bytes(buf_u64);


        let mut buf_u16 = [0; 2];
        protocol_reader
            .read_exact(&mut buf_u16)
            .map_err(HeaderParseError::Io)?;
        let full_file_name_len = u16::from_be_bytes(buf_u16);

        let mut buf = [0;1024];
        let full_file_name_buf = protocol_reader.read_nbytes(&mut buf, full_file_name_len as u64)?;
        let full_file_name = String::from_utf8(full_file_name_buf)
            .map_err(|e| HeaderParseError::Utf8Error(e.utf8_error().valid_up_to()))?;
        Ok(CopyOutHeader {
            content_len,
            offset,
            full_file_name,
        })
    } else {
        Err(HeaderParseError::InvalidTransferType(type_num))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use failure;
    use std::io::{self, Read, StdinLock, Cursor, Write};

    #[test]
    fn t_parse_copy_out_header()  -> Result<(), failure::Error> {
        let mut curor = Cursor::new(Vec::new());
        let content_len = 288_u64;
        let offset = 5_u64;
        let file_name = "hello.txt";
        let file_name_len: u16 = file_name.len().try_into().expect("file name length is in limit of u16");
        curor.write_all(&[TransferType::CopyOut.to_u8()])?;
        curor.write_all(&content_len.to_be_bytes())?;
        curor.write_all(&offset.to_be_bytes())?;
        curor.write_all(&file_name_len.to_be_bytes())?;
        curor.write_all(file_name.as_bytes())?;

        curor.set_position(0);
        
        let mut pr = ProtocolReader::new(&mut curor);

        let hd = parse_copy_out_header(&mut pr)?;

        assert_eq!(hd.content_len, content_len);
        assert_eq!(hd.offset, offset);
        assert_eq!(hd.full_file_name, file_name);

        Ok(())
    }

    #[test]
    fn t_parse_copy_out_header_1()  -> Result<(), failure::Error> {
        let mut curor = Cursor::new(CopyOutHeader::new(288, 5, "hello.txt").into_bytes());
        curor.set_position(0);
        let mut pr = ProtocolReader::new(&mut curor);
        let hd = parse_copy_out_header(&mut pr)?;
        assert_eq!(hd.content_len, 288);
        assert_eq!(hd.offset, 5);
        assert_eq!(hd.full_file_name, "hello.txt");

        Ok(())
    }
}