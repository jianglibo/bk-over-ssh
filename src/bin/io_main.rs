extern crate ssh_client_demo;
use std::{fs, io};
// use std::io::prelude::{Read, Write};

// https://www.reddit.com/r/rust/comments/6hoayo/how_do_i_write_to_stdout_without_line_buffering/?st=jzh2qp1x&sh=7c002b76

// extern crate kernel32;
// extern crate winapi;
// use std::os::windows::io::FromRawHandle;
// let h = kernel32::GetStdHandle(winapi::winbase::STD_OUTPUT_HANDLE);
// let stdout = File::from_raw_handle(h);


pub fn main() -> Result<(), failure::Error> {
    let mut f = fs::OpenOptions::new().read(true).open("fixtures/qrcode.png")?;
    io::copy(&mut f, &mut io::stdout())?;
    Ok(())
}