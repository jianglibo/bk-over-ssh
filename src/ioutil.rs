use std::io;
use std::sync::{Arc, Mutex};
use indicatif::{MultiProgress};
use crate::data_shape::{FileItem, Server};

pub type SharedMpb = Arc<MultiProgress>;

// pub type ArcedMultiBar = Arc<Mutex<MultiBar<io::Stdout>>>;
// pub type ArcedMultiBar = Arc<MultiBar<io::Stdout>>;

// pub fn copy<R: ?Sized, W: ?Sized>(
//     reader: &mut R,
//     writer: &mut W,
//     pb: &mut ProgressBar<io::Stdout>,
// ) -> io::Result<u64>
// where
//     R: io::Read,
//     W: io::Write,
// {
//     let mut buf = [0u8; 8 * 1024];

//     let mut written = 0;
//     loop {
//         let len = match reader.read(&mut buf) {
//             Ok(0) => return Ok(written),
//             Ok(len) => len,
//             Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
//             Err(e) => return Err(e),
//         };
//         pb.add(len as u64);
//         writer.write_all(&buf[..len])?;
//         written += len as u64;
//     }
// }

// pub fn map_multibar_to_pb_unit_bytes(
//     multi_bar: &mut ArcedMultiBar,
//     message: &str,
//     total: u64,
// ) -> FileItemPb {
//         let mut pb = multi_bar.create_bar(total);
//         pb.set_units(Units::Bytes);
//         pb.message(message);
//         pb
// }
