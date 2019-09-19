use pbr::{MultiBar, Pipe, ProgressBar, Units};
use std::io;
use std::sync::{Arc, Mutex};
use crate::data_shape::{FileItem, FileItemPb, Server};

pub fn copy<R: ?Sized, W: ?Sized>(
    reader: &mut R,
    writer: &mut W,
    pb: &mut ProgressBar<io::Stdout>,
) -> io::Result<u64>
where
    R: io::Read,
    W: io::Write,
{
    let mut buf = [0u8; 8 * 1024];

    let mut written = 0;
    loop {
        let len = match reader.read(&mut buf) {
            Ok(0) => return Ok(written),
            Ok(len) => len,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        pb.add(len as u64);
        writer.write_all(&buf[..len])?;
        written += len as u64;
    }
}

pub fn map_multibar_to_pb_unit_bytes(
    multi_bar: Option<&Arc<Mutex<MultiBar<io::Stdout>>>>,
    server: &Server,
    file_item: &FileItem,
) -> Option<FileItemPb> {
    let total = file_item.get_remote_item().get_len();
    multi_bar.map(|mb| {
        let mut pb = mb.lock().unwrap().create_bar(total);
        pb.set_units(Units::Bytes);
        FileItemPb {
            hostname: server.host.clone(),
            filename: file_item.get_remote_item().get_path().to_string(),
            pb,
        }
    })
}
