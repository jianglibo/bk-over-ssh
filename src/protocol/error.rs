use std::io;

#[derive(Debug, Fail)]
pub enum HeaderParseError {
    #[fail(display = "invalid transfer type: {}", _0)]
    InvalidTransferType(u8),
    #[fail(display = "{}", _0)]
    Io(#[fail(cause)] io::Error),
    #[fail(display = "Input was invalid UTF-8 at index {}", _0)]
    Utf8Error(usize),
    #[fail(display = "demanded {}, provided: {}", _0, _1)]
    InsufficientBytes(u64, u64),
}
