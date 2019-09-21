use crate::data_shape::{FileItem, FileItemProcessResult, Server, SyncType};
use crate::rustsync::{DeltaFileReader, DeltaReader, Signature};
use indicatif::HumanBytes;
use indicatif::ProgressBar;
use log::*;
use sha1::{Digest, Sha1};
use ssh2;
use std::convert::TryInto;
use std::ffi::OsStr;
use std::io::prelude::Write;
use std::path::Path;
use std::{fs, io, io::Read};

#[allow(dead_code)]
pub fn copy_file_to_stream(
    mut to: &mut impl std::io::Write,
    from_file: impl AsRef<Path>,
) -> Result<(), failure::Error> {
    let path = from_file.as_ref();
    let mut rf = fs::OpenOptions::new().open(path)?;
    io::copy(&mut rf, &mut to)?;
    Ok(())
}

pub fn copy_stream_to_file_with_cb<T: AsRef<Path>, F: FnMut(u64) -> ()>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf_len: usize,
    mut cb: Option<F>,
) -> Result<u64, failure::Error> {
    let  u8_buf = &mut vec![0; buf_len];
    let mut length = 0_u64;
    let path = to_file.as_ref();
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                let nn = n as u64;
                length += nn;
                wf.write_all(&u8_buf[..n])?;
                if let Some(cb) = cb.as_mut() {
                    cb(nn);
                }
            }
            _ => break,
        }
    }
    Ok(length)
}

#[allow(dead_code)]
pub fn copy_stream_to_file<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf_len: usize,
    pb_op: Option<&ProgressBar>,
) -> Result<u64, failure::Error> {
    let u8_buf = &mut vec![0; buf_len];
    let mut length = 0_u64;
    let path = to_file.as_ref();
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                length += n as u64;
                wf.write_all(&u8_buf[..n])?;
                if let Some(pb) = pb_op.as_ref() {
                    let message = format!("{}/{}", HumanBytes(length), "");
                    pb.set_message(message.as_str())
                }
            }
            _ => break,
        }
    }
    Ok(length)
}

pub fn copy_stream_to_file_return_sha1_with_cb<T: AsRef<Path>, F: FnMut(u64) -> ()>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf_len: usize,
    mut cb: Option<F>,
) -> Result<(u64, String), failure::Error> {
    let u8_buf = &mut vec![0; buf_len];
    let mut length = 0_u64;
    let mut hasher = Sha1::new();
    let path = to_file.as_ref();
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                length += n as u64;
                wf.write_all(&u8_buf[..n])?;
                hasher.input(&u8_buf[..n]);
                if let Some(cb) = cb.as_mut() {
                    cb(length);
                }
            }
            _ => break,
        }
    }
    ensure!(path.exists(), "write_stream_to_file should be done.");
    Ok((length, format!("{:X}", hasher.result())))
}

#[allow(dead_code)]
pub fn copy_stream_to_file_return_sha1<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf_len: usize,
    mut fi_pb_op: Option<&ProgressBar>,
) -> Result<(u64, String), failure::Error> {
    let u8_buf = &mut vec![0; buf_len];
    let mut length = 0_u64;
    let mut hasher = Sha1::new();
    let path = to_file.as_ref();
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                length += n as u64;
                wf.write_all(&u8_buf[..n])?;
                hasher.input(&u8_buf[..n]);
                if let Some(mypb) = fi_pb_op.as_mut() {
                    mypb.inc(n.try_into().unwrap())
                }
            }
            _ => break,
        }
    }
    ensure!(path.exists(), "write_stream_to_file should be done.");
    if let Some(fpo) = fi_pb_op {
        fpo.finish();
    }
    Ok((length, format!("{:X}", hasher.result())))
}

#[allow(dead_code)]
pub fn write_str_to_file(
    content: impl AsRef<str>,
    to_file: impl AsRef<OsStr>,
) -> Result<(), failure::Error> {
    let mut wf = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(to_file.as_ref())?;
    wf.write_all(content.as_ref().as_bytes())?;
    Ok(())
}

pub fn hash_file_sha1(file_name: impl AsRef<Path>) -> Option<String> {
    // let start = Instant::now();
    let file_r = fs::File::open(file_name.as_ref());
    match file_r {
        Ok(mut file) => {
            let mut hasher = Sha1::new();
            let n_r = io::copy(&mut file, &mut hasher);
            match n_r {
                Ok(_n) => {
                    let hash = hasher.result();
                    // println!("Bytes processed: {}", n);
                    let r = format!("{:x}", hash);
                    // println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
                    Some(r)
                }
                Err(err) => {
                    error!(
                        "hash_file_sha1 copy stream failed: {:?}, {:?}",
                        file_name.as_ref(),
                        err
                    );
                    None
                }
            }
        }
        Err(err) => {
            error!("hash_file_sha1 failed: {:?}, {:?}", file_name.as_ref(), err);
            None
        }
    }
}

// one possible implementation of walking a directory only visiting files
// https://doc.rust-lang.org/std/fs/fn.read_dir.html
#[allow(dead_code)]
pub fn visit_dirs(dir: &Path, cb: &dyn Fn(&fs::DirEntry)) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

// https://stackoverflow.com/questions/32300132/why-cant-i-store-a-value-and-a-reference-to-that-value-in-the-same-struct

pub fn copy_a_file_item_sftp<'a, F: FnMut(u64) -> ()>(
    sftp: &ssh2::Sftp,
    local_file_path: String,
    file_item: &FileItem<'a>,
    buf_len: usize,
    cb: Option<F>,
) -> FileItemProcessResult {
    match sftp.open(Path::new(&file_item.get_remote_path())) {
        Ok(mut file) => {
            if let Some(_r_sha1) = file_item.get_remote_item().get_sha1() {
                match copy_stream_to_file_return_sha1_with_cb(&mut file, &local_file_path, buf_len, cb) {
                    Ok((length, sha1)) => {
                        if length != file_item.get_remote_item().get_len() {
                            error!("length didn't match: {:?}", file_item);
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else if file_item.is_sha1_not_equal(&sha1) {
                            error!("sha1 didn't match: {:?}, local sha1: {:?}", file_item, sha1);
                            FileItemProcessResult::Sha1NotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Successed(
                                length,
                                local_file_path,
                                SyncType::Sftp,
                            )
                        }
                    }
                    Err(err) => {
                        error!("write_stream_to_file failed: {:?}", err);
                        FileItemProcessResult::CopyFailed(local_file_path)
                    }
                }
            } else {
                match copy_stream_to_file_with_cb(&mut file, &local_file_path, buf_len, cb) {
                    Ok(length) => {
                        if length != file_item.get_remote_item().get_len() {
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Successed(
                                length,
                                local_file_path,
                                SyncType::Sftp,
                            )
                        }
                    }
                    Err(err) => {
                        error!("write_stream_to_file failed: {:?}", err);
                        FileItemProcessResult::CopyFailed(local_file_path)
                    }
                }
            }
        }
        Err(err) => {
            error!("sftp open failed: {:?}", err);
            FileItemProcessResult::SftpOpenFailed
        }
    }
}

pub fn copy_a_file_item_rsync<'a>(
    server: &Server,
    sftp: &ssh2::Sftp,
    local_file_path: String,
    file_item: &FileItem<'a>,
) -> Result<FileItemProcessResult, failure::Error> {
    let remote_path = file_item.get_remote_path();
    let mut sig = Signature::signature_a_file(&local_file_path, Some(4096))?;
    let remote_sig_file_path = format!("{}.sig", &remote_path);
    let sig_file = sftp.create(Path::new(&remote_sig_file_path))?;
    sig.write_to_stream(sig_file)?;
    let delta_file_name = format!("{}.delta", &remote_path);
    let cmd = format!(
        "{} rsync delta-a-file --new-file {} --sig-file {} --out-file {}",
        server.remote_exec, &remote_path, &remote_sig_file_path, &delta_file_name,
    );
    trace!("about to invoke command: {:?}", cmd);
    let mut channel: ssh2::Channel = server.create_channel()?;
    channel.exec(cmd.as_str())?;
    let mut ch_stderr = channel.stderr();
    let mut chout = String::new();
    ch_stderr.read_to_string(&mut chout)?;
    trace!(
        "after invoke delta command, there maybe err in channel: {:?}",
        chout
    );
    channel.read_to_string(&mut chout)?;
    trace!(
        "delta-a-file output: {:?}, delta_file_name: {:?}",
        chout,
        delta_file_name
    );
    if let Ok(file) = sftp.open(Path::new(&delta_file_name)) {
        let mut delta_file = DeltaFileReader::<ssh2::File>::read_delta_stream(file)?;
        let restore_path = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(format!("{}.restore", local_file_path))?;
        trace!("restore_path: {:?}", restore_path);
        let old_file = fs::OpenOptions::new().read(true).open(&local_file_path)?;
        delta_file.restore_seekable(restore_path, old_file)?;
        update_local_file_from_restored(&local_file_path)?;
        Ok(FileItemProcessResult::Successed(
            0,
            local_file_path,
            SyncType::Rsync,
        ))
    } else {
        bail!("sftp open delta_file: {:?} failed.", delta_file_name);
    }
}

fn update_local_file_from_restored(local_file_path: impl AsRef<str>) -> Result<(), failure::Error> {
    let old_tmp = format!("{}.old.tmp", local_file_path.as_ref());
    let old_tmp_path = Path::new(&old_tmp);
    if old_tmp_path.exists() {
        fs::remove_file(old_tmp_path)?;
    }
    fs::rename(local_file_path.as_ref(), &old_tmp)?;
    let restored = format!("{}.restore", local_file_path.as_ref());
    fs::rename(&restored, local_file_path.as_ref())?;
    if old_tmp_path.exists() {
        fs::remove_file(&old_tmp_path)?;
    }
    Ok(())
}

pub fn copy_a_file_item<'a>(
    server: &Server,
    sftp: &ssh2::Sftp,
    file_item: FileItem<'a>,
    buf_len: usize,
    pb_op: Option<&ProgressBar>,
) -> FileItemProcessResult {
    let mut received = 0_u64;
    if let Some(local_file_path) = file_item.get_local_path_str() {
        let cb = if pb_op.is_some() {
            Some(|length| {
                received += length;
                let message = format!(
                    "{}/{} - {}",
                    HumanBytes(received),
                    HumanBytes(file_item.get_remote_item().get_len()),
                    file_item.get_remote_item().get_path()
                );
                pb_op.unwrap().set_message(message.as_str());
                pb_op.unwrap().inc(length);
            })
        } else {
            None
        };
        let copy_result = match file_item.sync_type {
            SyncType::Sftp => copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf_len, cb),
            SyncType::Rsync => {
                if !Path::new(&local_file_path).exists() {
                    copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf_len, cb)
                } else {
                    match copy_a_file_item_rsync(server, sftp, local_file_path.clone(), &file_item)
                    {
                        Ok(r) => r,
                        Err(err) => {
                            error!("rsync file failed: {:?}, {:?}", file_item, err);
                            copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf_len, cb)
                        }
                    }
                }
            }
        };

        if let FileItemProcessResult::Successed(_, _, _) = &copy_result {
            if let Err(err) = file_item.set_modified_as_remote() {
                warn!("set modified as remote failed: {:?}", err);
            } else {
                file_item.verify_modified_equal();
            }
        }

        copy_result
    } else {
        FileItemProcessResult::GetLocalPathFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::{
        FileItem, FileItemProcessResult, RemoteFileItemOwned, Server, SyncType,
    };
    use crate::develope::tutil;
    use crate::log_util;
    use std::panic;
    use std::{fs, io};

    fn copy_a_file<'a>(
        session: &ssh2::Session,
        local_base_dir: &'a Path,
        remote_base_dir: &'a str,
        remote_relative_path: &'a str,
        remote_file_len: u64,
        sync_type: SyncType,
    ) -> Result<FileItemProcessResult, failure::Error> {
        let ri = RemoteFileItemOwned::new(remote_relative_path, remote_file_len);
        let fi = FileItem::new(local_base_dir, remote_base_dir, ri, sync_type);
        let sftp = session.sftp()?;
        let mut server = Server::load_from_yml("servers", "data", "localhost.yml", None, None)?;
        server.connect()?;
        let r = copy_a_file_item(&server, &sftp, fi, 8192, None);
        Ok(r)
    }

    #[test]
    fn t_visit() {
        let mut _count = 0_u64;
        visit_dirs(Path::new("e:\\"), &|entry| {
            println!("{:?}", entry);
        })
        .expect("success");
    }

    #[test]
    fn t_copy_a_file() -> Result<(), failure::Error> {
        log_util::setup_test_logger_only_self(vec!["actions::copy_file"]);

        let mut server = Server::load_from_yml("servers", "data", "localhost",None, None)?;
        server.connect()?;
        server.rsync_valve = 4;
        let test_dir1 = tutil::create_a_dir_and_a_filename("xx.txt")?;
        let local_file_name = test_dir1.tmp_dir.path().join("yy.txt");
        let test_dir2 = tutil::create_a_dir_and_a_file_with_content("yy.txt", "hello")?;
        let remote_file_name = test_dir2.tmp_file_name_only()?;

        info!("{:?}, local: {:?}", remote_file_name, local_file_name);
        // let result = panic::catch_unwind(|| {
        let r = copy_a_file(
            server.get_ssh_session(),
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            remote_file_name.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Sftp,
        )?;
        assert!(
            if let FileItemProcessResult::Successed(_, _, SyncType::Sftp) = r {
                true
            } else {
                false
            }
        );
        assert!(local_file_name.exists());
        // });

        tutil::change_file_content(&local_file_name)?;
        let r = copy_a_file(
            server.get_ssh_session(),
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            test_dir2.tmp_file_name_only()?.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Rsync,
        )?;

        tutil::change_file_content(&local_file_name)?;
        assert!(
            if let FileItemProcessResult::Successed(_, _, SyncType::Rsync) = r {
                true
            } else {
                false
            }
        );

        tutil::change_file_content(&local_file_name)?;
        let _r = copy_a_file(
            server.get_ssh_session(),
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            test_dir2.tmp_file_name_only()?.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Rsync,
        )?;
        Ok(())
    }

    #[test]
    fn t_copy_to_stdout() -> Result<(), failure::Error> {
        let mut f = fs::OpenOptions::new().open("fixtures/qrcode.png")?;
        // let mut buf = io::BufReader::new(f);
        let mut u8_buf = [0; 8192];
        let len = f.read(&mut u8_buf)?;
        // let copied_length = io::copy(&mut buf, &mut io::sink())?;
        io::sink().write_all(&u8_buf[..len])?;
        // assert_eq!(copied_length, 55);
        Ok(())
    }

    #[test]
    fn t_buff_init() {
        let start = std::time::Instant::now();
        (0..1000).for_each(|i|{
            let mut u8_buf = [0_u8; 8192];
            u8_buf.last_mut().replace(&mut 1);
        });

        println!("array: {:?}", start.elapsed().as_millis());

        let start = std::time::Instant::now();
        (0..1000).for_each(|i|{
            let mut u8_buf = vec![0_u8; 8192];
            u8_buf.last_mut().replace(&mut 1);
        });

        println!("vec: {:?}", start.elapsed().as_millis())
    }
}

// fn hash_file_2(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();

//     let mut hasher = DefaultHasher::new();

//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut buffer = [0; 1024];
//     let mut total = 0_usize;
//     loop {
//         let n = file.read(&mut buffer[..])?;
//         if n == 0 {
//             break
//         } else {
//             hasher.write(&buffer[..n]);
//             total += n;
//         }
//     }
//     let hash = hasher.finish();
//     println!("Bytes processed: {}", total);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }

// fn hash_file_1(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();
//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut hasher = Sha224::new();
//     let mut buffer = [0; 1024];
//     let mut total = 0_usize;
//     loop {
//         let n = file.read(&mut buffer[..])?;
//         if n == 0 {
//             break
//         } else {
//             hasher.input(&buffer[..n]);
//             total += n;
//         }
//     }
//     let hash = hasher.result();
//     println!("Bytes processed: {}", total);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }

// fn hash_file(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();
//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut hasher = Sha224::new();
//     let n = io::copy(&mut file, &mut hasher)?;
//     let hash = hasher.result();
//     println!("Bytes processed: {}", n);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }
