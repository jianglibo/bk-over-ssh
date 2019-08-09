use super::super::data_shape::FileItem;
use log::*;
use ssh2;
use std::fs::OpenOptions;
use std::io::prelude::{Read, Write};
use std::path::Path;

pub fn write_to_file<T: AsRef<str>>(
    from: &mut impl std::io::Read,
    to_file: T,
) -> Result<(), failure::Error> {
    let mut u8_buf = [0; 1024];
    let mut wf = OpenOptions::new()
        .create(true)
        .write(true)
        .open(to_file.as_ref())?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                wf.write_all(&u8_buf[..n])?;
            }
            _ => break,
        }
    }
    Ok(())
}

pub fn copy_a_file(session: &mut ssh2::Session, file_item: &mut FileItem) {
    let sftp = session.sftp().expect("should got sfpt instance.");
    if let Ok(mut file) = sftp.open(Path::new(file_item.remote_path)) {
        let mut buf = String::new();
        file.read_to_string(&mut buf).expect("msg: &str");
        assert_eq!(buf, "hello\nworld\n");
        assert_eq!(buf.len(), 12);
        info!("{:?}", buf);
    };
}
