use crate::data_shape::{FileItem, FileItemProcessResult, RemoteFileItem, Server, SyncType};
use crate::rustsync::{DeltaFileReader, DeltaReader, Signature};
use log::*;
use sha1::{Digest, Sha1};
use ssh2;
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

pub fn copy_stream_to_file<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
) -> Result<u64, failure::Error> {
    let mut u8_buf = [0; 1024];
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
            }
            _ => break,
        }
    }
    Ok(length)
}

pub fn copy_stream_to_file_return_sha1<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
) -> Result<(u64, String), failure::Error> {
    let mut u8_buf = [0; 1024];
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
            }
            _ => break,
        }
    }
    ensure!(path.exists(), "write_stream_to_file should be done.");
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

#[allow(dead_code)]
pub fn copy_a_file<'a>(
    session: &mut ssh2::Session,
    remote_file_path: &'a str,
    local_file_path: &'a str,
    sync_type: SyncType,
) -> Result<FileItemProcessResult, failure::Error> {
    let ri = RemoteFileItem::new(remote_file_path);
    let fi = FileItem::standalone(Path::new(local_file_path), None, &ri, sync_type);
    let sftp = session.sftp()?;
    let server = Server::load_from_yml("localhost")?;
    let r = copy_a_file_item(&server, &sftp, fi);
    Ok(r)
}

pub fn copy_a_file_item_sftp<'a>(
    sftp: &ssh2::Sftp,
    local_file_path: String,
    file_item: &FileItem<'a>,
) -> FileItemProcessResult {
    match sftp.open(Path::new(&file_item.get_remote_path())) {
        Ok(mut file) => {
            if let Some(_r_sha1) = file_item.get_remote_item().get_sha1() {
                match copy_stream_to_file_return_sha1(&mut file, &local_file_path) {
                    Ok((length, sha1)) => {
                        if length != file_item.get_remote_item().get_len() {
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else if file_item.is_sha1_not_equal(&sha1) {
                            error!("sha1 didn't match: {:?}, local sha1: {:?}", file_item, sha1);
                            FileItemProcessResult::Sha1NotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Successed
                        }
                    }
                    Err(err) => {
                        error!("write_stream_to_file failed: {:?}", err);
                        FileItemProcessResult::CopyFailed(local_file_path)
                    }
                }
            } else {
                match copy_stream_to_file(&mut file, &local_file_path) {
                    Ok(length) => {
                        if length != file_item.get_remote_item().get_len() {
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Successed
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
        server.remote_exec,
        &remote_path,
        &remote_sig_file_path,
        &delta_file_name,
    );
    let mut channel: ssh2::Channel = server.create_channel()?;
    channel.exec(cmd.as_str())?;
    let mut chout = String::new();
    channel.read_to_string(&mut chout)?;
    trace!("delta-a-file output: {:?}", chout);
    let file = sftp.open(Path::new(&delta_file_name))?;
    let mut delta_file = DeltaFileReader::<ssh2::File>::read_delta_stream(file)?;
    let restore_path = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(format!("{}.restore", local_file_path))?;
    let old_file = fs::OpenOptions::new().read(true).open(&local_file_path)?;
    delta_file.restore_seekable(restore_path, old_file)?;
    update_local_file_from_restored(local_file_path)?;
    Ok(FileItemProcessResult::Successed)
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
) -> FileItemProcessResult {
    if let Some(local_file_path) = file_item.get_local_path_str() {
        match file_item.sync_type {
            SyncType::Sftp => copy_a_file_item_sftp(sftp, local_file_path, &file_item),
            SyncType::Rsync => {
                if !Path::new(&local_file_path).exists() {
                    copy_a_file_item_sftp(sftp, local_file_path, &file_item)
                } else {
                    match copy_a_file_item_rsync(server, sftp, local_file_path.clone(), &file_item)
                    {
                        Ok(r) => r,
                        Err(err) => {
                            error!("rsync file failed: {:?}, {:?}", file_item, err);
                            copy_a_file_item_sftp(sftp, local_file_path, &file_item)
                        }
                    }
                }
            }
        }
    } else {
        FileItemProcessResult::GetLocalPathFailed
    }
}

#[cfg(test)]
mod tests {
    use super::{copy_a_file, visit_dirs, Path, SyncType};
    use crate::develope::develope_data;
    use crate::log_util;
    use std::io::prelude::*;
    use std::{fs, io};

    #[test]
    fn t_visit() {
        let mut _count = 0_u64;
        visit_dirs(Path::new("e:\\"), &|entry| {
            // count += 1;
            println!("{:?}", entry);
        })
        .expect("success");
    }

    #[test]
    fn t_copy_a_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let (_tcp, mut sess, dev_env) = develope_data::connect_to_ubuntu();
        let lpn = "not_in_git/xx.txt";
        let lp = Path::new(lpn);

        if lp.exists() {
            fs::remove_file(lp)?;
        }
        copy_a_file(
            &mut sess,
            dev_env.servers.ubuntu18.test_dirs.aatxt.as_str(),
            lpn,
            SyncType::Sftp,
        )?;
        assert!(Path::new(lp).exists());
        Ok(())
    }

    #[test]
    fn t_copy_to_stdout() -> Result<(), failure::Error> {
        let mut f = fs::OpenOptions::new().open("fixtures/qrcode.png")?;
        // let mut buf = io::BufReader::new(f);
        let mut u8_buf = [0; 1024];
        let len = f.read(&mut u8_buf)?;
        // let copied_length = io::copy(&mut buf, &mut io::sink())?;
        io::sink().write_all(&u8_buf[..len])?;
        // assert_eq!(copied_length, 55);
        Ok(())
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
