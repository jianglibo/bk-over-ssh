use crate::data_shape::{
    FileItemMap, FileItemProcessResult, Indicator, PbProperties, ProgressWriter, Server, SyncType,
};
use crate::db_accesses::DbAccess;
use crate::rustsync::{DeltaFileReader, DeltaReader, Signature};
use indicatif::ProgressStyle;
use log::*;
use r2d2;
use sha1::{Digest, Sha1};
use ssh2;
use std::ffi::OsStr;
use std::io::prelude::Write;
use std::path::Path;
use std::{fs, io, io::BufRead};
use std::fmt;
use std::error::Error as StdError;

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

pub fn copy_stream_to_file_with_cb<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf: &mut [u8],
    // mut counter: F,
    progress_bar: &Indicator,
) -> Result<u64, failure::Error> {
    let mut length = 0_u64;
    let path = to_file.as_ref();
    trace!("start copy_stream_to_file_with_cb: {:?}", path);
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(buf) {
            Ok(n) if n > 0 => {
                let nn = n as u64;
                length += nn;
                wf.write_all(&buf[..n])?;
                progress_bar.inc_pb(nn);
            }
            Ok(_) => {
                trace!("end copy_stream_to_file_with_cb when read zero byte.");
                break;
            }
            Err(err) => {
                trace!(
                    "end copy_stream_to_file_with_cb when catch error. {:?}",
                    err
                );
                break;
            }
        }
    }
    Ok(length)
}

#[allow(dead_code)]
pub fn copy_stream_to_file<T: AsRef<Path>, F: FnMut(u64) -> ()>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf_len: usize,
    mut counter: F,
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
                counter(n as u64);
            }
            _ => break,
        }
    }
    Ok(length)
}

pub fn copy_stream_to_file_return_sha1_with_cb<T: AsRef<Path>>(
    from: &mut impl std::io::Read,
    to_file: T,
    buf: &mut [u8],
    progress_bar: &Indicator,
) -> Result<(u64, String), failure::Error> {
    let mut length = 0_u64;
    let mut hasher = Sha1::new();
    let path = to_file.as_ref();
    trace!("start copy_stream_to_file_return_sha1_with_cb: {:?}", path);
    if let Some(pp) = path.parent() {
        if !pp.exists() {
            fs::create_dir_all(pp)?;
        }
    }
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(path)?;
    loop {
        match from.read(buf) {
            Ok(n) if n > 0 => {
                length += n as u64;
                wf.write_all(&buf[..n])?;
                hasher.input(&buf[..n]);
                // counter(length);
                progress_bar.inc_pb(length);
            }
            Ok(_) => {
                trace!("end copy_stream_to_file_return_sha1_with_cb when read zero byte");
                break;
            }
            Err(err) => {
                trace!(
                    "end copy_stream_to_file_return_sha1_with_cb catch error. {:?}",
                    err
                );
                break;
            }
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

#[allow(dead_code)]
pub fn copy_a_file_item_scp<'a>(
    session: &mut ssh2::Session,
    local_file_path: String,
    file_item: &FileItemMap<'a>,
    buf: &mut [u8],
    pb: &mut Indicator,
) -> FileItemProcessResult {
    match session.scp_recv(Path::new(file_item.get_relative_file_name().as_str())) {
        Ok((mut file, _stat)) => {
            if let Some(_r_sha1) = file_item.get_relative_item().get_sha1() {
                match copy_stream_to_file_return_sha1_with_cb(&mut file, &local_file_path, buf, pb)
                {
                    Ok((length, sha1)) => {
                        if length != file_item.get_relative_item().get_len() {
                            error!("length didn't match: {:?}", file_item);
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else if file_item.is_sha1_not_equal(&sha1) {
                            error!("sha1 didn't match: {:?}, local sha1: {:?}", file_item, sha1);
                            FileItemProcessResult::Sha1NotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Succeeded(
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
                match copy_stream_to_file_with_cb(&mut file, &local_file_path, buf, pb) {
                    Ok(length) => {
                        if length != file_item.get_relative_item().get_len() {
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Succeeded(
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
            error!("scp open failed: {:?}", err);
            FileItemProcessResult::ScpOpenFailed
        }
    }
}
// https://stackoverflow.com/questions/32300132/why-cant-i-store-a-value-and-a-reference-to-that-value-in-the-same-struct

pub fn copy_a_file_item_sftp<'a>(
    sftp: &ssh2::Sftp,
    local_file_path: String,
    file_item_map: &FileItemMap<'a>,
    buf: &mut [u8],
    pb: &Indicator,
) -> FileItemProcessResult {
    match sftp.open(Path::new(&file_item_map.get_relative_file_name())) {
        Ok(mut file) => {
            if let Some(_r_sha1) = file_item_map.get_relative_item().get_sha1() {
                match copy_stream_to_file_return_sha1_with_cb(&mut file, &local_file_path, buf, pb)
                {
                    Ok((length, sha1)) => {
                        if length != file_item_map.get_relative_item().get_len() {
                            error!("length didn't match: {:?}", file_item_map);
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else if file_item_map.is_sha1_not_equal(&sha1) {
                            error!("sha1 didn't match: {:?}, local sha1: {:?}", file_item_map, sha1);
                            FileItemProcessResult::Sha1NotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Succeeded(
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
                match copy_stream_to_file_with_cb(&mut file, &local_file_path, buf, pb) {
                    Ok(length) => {
                        if length != file_item_map.get_relative_item().get_len() {
                            FileItemProcessResult::LengthNotMatch(local_file_path)
                        } else {
                            FileItemProcessResult::Succeeded(
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

#[allow(dead_code)]
pub fn scp_upload_file_with_progress(
    session: &ssh2::Session,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    scp_file_with_progress(session, true, source, dest, message, pb)
}

pub fn scp_download_file_with_progress(
    session: &ssh2::Session,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    scp_file_with_progress(session, false, source, dest, message, pb)
}

fn scp_file_with_progress(
    session: &ssh2::Session,
    upload: bool,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    let dest = dest.as_ref();
    let source = source.as_ref();
    trace!(
        "scp a file source: {:?}, dest: {:?}, upload: {}",
        source,
        dest,
        upload
    );
    let (mut scp_channel, len) = if upload {
        let len = source.metadata()?.len();
        trace!("len: {}", len);
        let v = (session.scp_send(dest, 0o_0022, len, None)?, len);
        trace!("len: {}", len);
        v
    } else {
        let (channel, stat) = session.scp_recv(source)?;
        (channel, stat.size())
    };
    pb.alter_pb(PbProperties {
        reset: true,
        set_style: Some(ProgressStyle::default_bar().template("[{eta_precise}] {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {wide_msg}").progress_chars("#-")),
        set_message: Some(message.as_ref().to_owned()),
        set_length: Some(len),
        ..PbProperties::default()
    });

    if upload {
        let mut source_file = fs::OpenOptions::new().read(true).open(source)?;
        let mut dest_file = ProgressWriter::new(scp_channel, pb);
        io::copy(&mut source_file, &mut dest_file)?;
    } else {
        let w = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest)?;
        let mut dest_file = ProgressWriter::new(w, pb);
        io::copy(&mut scp_channel, &mut dest_file)?;
    };
    Ok(())
}

pub fn sftp_upload_file_with_progress(
    sftp: &ssh2::Sftp,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    sftp_file_with_progress(sftp, true, source, dest, message, pb)
}

#[allow(dead_code)]
pub fn sftp_download_file_with_progress(
    sftp: &ssh2::Sftp,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    sftp_file_with_progress(sftp, false, source, dest, message, pb)
}

fn sftp_file_with_progress(
    sftp: &ssh2::Sftp,
    upload: bool,
    source: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    message: impl AsRef<str>,
    pb: &Indicator,
) -> Result<(), failure::Error> {
    let dest = dest.as_ref();
    let source = source.as_ref();
    trace!(
        "sftp a file source: {:?}, dest: {:?}, upload: {}",
        source,
        dest,
        upload
    );
    let mut sftp_file = if upload {
        sftp.create(dest)?
    } else {
        sftp.open(source)?
    };

    let len = if upload {
        Some(source.metadata()?.len())
    } else {
        None
    };

    pb.alter_pb(PbProperties {
        reset: true,
        set_style: Some(ProgressStyle::default_bar().template("[{eta_precise}] {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {wide_msg}").progress_chars("#-")),
        set_message: Some(message.as_ref().to_owned()),
        set_length: len,
        ..PbProperties::default()
    });

    if upload {
        let mut source_file = fs::OpenOptions::new().read(true).open(source)?;
        let mut dest_file = ProgressWriter::new(sftp_file, pb);
        io::copy(&mut source_file, &mut dest_file)?;
    } else {
        let w = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest)?;
        let mut dest_file = ProgressWriter::new(w, pb);
        io::copy(&mut sftp_file, &mut dest_file)?;
    };
    Ok(())
}

pub fn invoke_remote_ssh_command_receive_progress(
    cmd: impl AsRef<str>,
    mut channel: ssh2::Channel,
    progress_bar: &Indicator,
) -> Result<(), failure::Error> {
    let cmd = cmd.as_ref();
    trace!("about to invoke command: {:?}", cmd);
    channel.exec(cmd)?;

    let buf_reader = io::BufReader::new(channel);
    buf_reader.lines().for_each(|line| {
        let line = line.ok().unwrap_or_else(|| "".to_string());
        if line.starts_with("size:") {
            let (_, d) = line.split_at(5);
            let i = d.parse::<u64>().ok().unwrap_or(!0);
                progress_bar.alter_pb(PbProperties {
        reset: true,
        set_style: Some(ProgressStyle::default_bar().template("[{eta_precise}] {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {wide_msg}").progress_chars("#-")),
        set_message: Some("start calculating delta file.".to_string()),
        set_length: Some(i),
        ..PbProperties::default()
    });
        } else {
            let i =  line.parse::<u64>().ok().unwrap_or(0);
            progress_bar.inc_pb(i);
        }
    });
    Ok(())
}

pub fn copy_a_file_item_rsync<'a, M, D>(
    server: &Server<M, D>,
    sftp: &ssh2::Sftp,
    local_file_path: String,
    file_item: &FileItemMap<'a>,
    progress_bar: &Indicator,
) -> Result<FileItemProcessResult, failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let remote_file_name = file_item.get_relative_file_name();
    trace!("start signature_a_file {}", &local_file_path);
    let mut sig =
        Signature::signature_a_file(&local_file_path, Some(server.server_yml.rsync.window), progress_bar)?;

    let local_sig_file_name = format!("{}.sig", local_file_path);
    sig.write_to_file(&local_sig_file_name)?;

    let remote_sig_file_name = format!("{}.sig", &remote_file_name);

    sftp_upload_file_with_progress(
        sftp,
        &local_sig_file_name,
        &remote_sig_file_name,
        format!("start upload signature file. {:?}", &local_sig_file_name),
        progress_bar,
    )?;

    let remote_delta_file_name = format!("{}.delta", &remote_file_name);
    let cmd = format!(
        "{} rsync delta-a-file --print-progress --new-file {} --sig-file {} --out-file {}",
        server.server_yml.remote_exec,
        &remote_file_name,
        &remote_sig_file_name,
        &remote_delta_file_name,
    );

    let channel: ssh2::Channel = server.create_channel()?;
    invoke_remote_ssh_command_receive_progress(cmd, channel, progress_bar)?;

    let local_delta_file_name = format!("{}.delta", local_file_path);

    scp_download_file_with_progress(
        server.get_ssh_session(),
        &remote_delta_file_name,
        &local_delta_file_name,
        format!("start download delta file. {:?}", &local_delta_file_name),
        progress_bar,
    )?;

    let local_delta_file = fs::OpenOptions::new()
        .read(true)
        .open(&local_delta_file_name)?;

    let mut delta_file = DeltaFileReader::<ssh2::File>::read_delta_stream(local_delta_file)?;

    let restore_path = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(format!("{}.restore", local_file_path))?;
    trace!("restore_path: {:?}", restore_path);
    let old_file = fs::OpenOptions::new().read(true).open(&local_file_path)?;
    delta_file.restore_seekable(restore_path, old_file)?;
    update_local_file_from_restored(&local_file_path)?;
    Ok(FileItemProcessResult::Succeeded(
        0,
        local_file_path,
        SyncType::Rsync,
    ))
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

#[derive(Debug)]
pub enum SftpException {
    NoSuchFile,
    OpenLocalFailed,
    CopyStreamFailed,
    Unknown,
}

impl fmt::Display for SftpException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = match self {
            SftpException::NoSuchFile => "no such file",
            SftpException::OpenLocalFailed => "open local failed",
            SftpException::CopyStreamFailed => "copy stream failed",
            SftpException::Unknown => "unknown",
        };
        write!(f, "{}", a)
    }
}

impl StdError for SftpException {
    fn description(&self) -> &str {
        "one of 4 possible sftp errors."
    }

    fn cause(&self) -> Option<&dyn StdError> {
        None
    }
}

pub fn copy_a_file_sftp(
    sftp: &ssh2::Sftp,
    local: impl AsRef<str>,
    remote: impl AsRef<str>,
) -> Result<(), SftpException> {
    let local = local.as_ref();
    let remote = remote.as_ref();
    match sftp.create(Path::new(remote)) {
        Ok(mut r_file) => match fs::File::open(local) {
            Ok(mut l_file) => match io::copy(&mut l_file, &mut r_file) {
                Ok(_) => Ok(()),
                Err(err) => {
                    error!("sftp copy failed: {:?}", err);
                    Err(SftpException::CopyStreamFailed)
                }
            },
            Err(err) => {
                error!("sftp open local file failed: {:?}", err);
                Err(SftpException::OpenLocalFailed)
            }
        },
        Err(err) => {
            error!(
                "copy from {:?} to {:?} failed. maybe remote file's parent dir didn't exists. err: {:?}",
                local,
                remote,
                err
            );
            // no such file
            if err.code() == 2 {
                Err(SftpException::NoSuchFile)
            } else {
                Err(SftpException::Unknown)
            }
        }
    }
}

pub fn copy_a_file_item<'a, M, D>(
    server: &Server<M, D>,
    sftp: &ssh2::Sftp,
    file_item: FileItemMap<'a>,
    buf: &mut [u8],
    pb: &mut Indicator,
) -> FileItemProcessResult
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    if let Some(local_file_path) = file_item.get_local_path_str() {
        let copy_result = match file_item.sync_type {
            SyncType::Sftp => copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf, pb),
            SyncType::Rsync => {
                if !Path::new(&local_file_path).exists() {
                    copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf, pb)
                } else {
                    match copy_a_file_item_rsync(
                        server,
                        sftp,
                        local_file_path.clone(),
                        &file_item,
                        pb,
                    ) {
                        Ok(r) => r,
                        Err(err) => {
                            error!("rsync file failed: {:?}, {:?}", file_item, err);
                            copy_a_file_item_sftp(sftp, local_file_path, &file_item, buf, pb)
                        }
                    }
                }
            }
        };

        if let FileItemProcessResult::Succeeded(_, _, _) = &copy_result {
            if let Err(err) = file_item.set_modified_as_remote() {
                warn!("set modified as remote failed: {}", err);
            } else {
                file_item.verify_modified_equal();
            }
        }

        copy_result
    } else {
        warn!("get_local_path_str failed: {:?}, ", file_item);
        FileItemProcessResult::GetLocalPathFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{copy_a_file_sftp, ssh_util};
    use crate::data_shape::{
        string_path, AppRole, FileItemMap, FileItemProcessResult, RelativeFileItem, Server, SyncType,
    };
    use crate::develope::tutil;
    use crate::log_util;
    use dotenv::dotenv;
    use std::env;
    use std::io::Read;
    use std::panic;
    use std::{fs, io};

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["actions::copy_file"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    fn copy_a_file<'a, M, D>(
        server: &mut Server<M, D>,
        local_base_dir: &'a Path,
        remote_base_dir: &'a str,
        remote_relative_path: &'a str,
        remote_file_len: u64,
        sync_type: SyncType,
        pb: &mut Indicator,
    ) -> Result<FileItemProcessResult, failure::Error>
    where
        M: r2d2::ManageConnection,
        D: DbAccess<M>,
    {
        let ri = RelativeFileItem::new(remote_relative_path, remote_file_len);
        let fi = FileItemMap::new(
            local_base_dir,
            remote_base_dir,
            ri,
            sync_type,
            true,
        );
        let sftp = server.get_ssh_session().sftp()?;
        let mut buf = vec![0; 8192];
        let app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        let mut another_server = tutil::load_demo_server_sqlite(&app_conf, None);
        another_server.connect()?;
        let r = copy_a_file_item(&another_server, &sftp, fi, &mut buf, pb);
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
    fn t_copy_executable() -> Result<(), failure::Error> {
        let tu = tutil::TestDir::new();
        let dir_str = tu.tmp_dir_str();
        let f_path_buf = tu.make_a_file_with_content("a.txt", "abc")?;
        let remote_dir_str = string_path::SlashPath::new(dir_str)
            .join("a/b/c/d/e.txt")
            .slash;
        dotenv().ok();
        let username = env::var("username")?;
        let password = env::var("password")?;
        let sess = ssh_util::create_ssh_session_password(
            "localhost:22",
            username.as_str(),
            password.as_str(),
        )?;
        let sftp = sess.sftp()?;
        eprintln!("copy {:?} to {:?}", f_path_buf, remote_dir_str);
        if let Err(err) = copy_a_file_sftp(
            &sftp,
            f_path_buf.to_str().expect("f_path_buf to_str failed."),
            remote_dir_str.as_str(),
        ) {
            if let SftpException::NoSuchFile = err {
                let mut channel = sess.channel_session()?;
                channel.exec("failed")?;
            } else {
                bail!("sftp failed: {:?}", err);
            }
        };

        assert!(Path::new(remote_dir_str.as_str()).exists());
        Ok(())
    }

    #[test]
    fn t_copy_a_file() -> Result<(), failure::Error> {
        log();
        let app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        let mut server = tutil::load_demo_server_sqlite(&app_conf, None);
        server.connect()?;
        server.server_yml.rsync.valve = 4;
        let test_dir1 = tutil::create_a_dir_and_a_file_with_content("xx.txt", "")?;
        let local_file_name = test_dir1.tmp_dir.path().join("yy.txt");
        let test_dir2 = tutil::create_a_dir_and_a_file_with_content("yy.txt", "hello")?;
        let remote_file_name = test_dir2.tmp_file_name_only()?;

        info!("{:?}, local: {:?}", remote_file_name, local_file_name);
        let mut indicator = Indicator::new(None);
        let r = copy_a_file(
            &mut server,
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            remote_file_name.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Sftp,
            &mut indicator,
        )?;
        assert!(
            if let FileItemProcessResult::Succeeded(_, _, SyncType::Sftp) = r {
                true
            } else {
                false
            },
            "by sftp should success."
        );
        assert!(local_file_name.exists());

        tutil::change_file_content(&local_file_name)?;
        let r = copy_a_file(
            &mut server,
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            test_dir2.tmp_file_name_only()?.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Rsync,
            &mut indicator,
        )?;

        tutil::change_file_content(&local_file_name)?;
        assert!(
            if let FileItemProcessResult::Succeeded(_, _, SyncType::Rsync) = r {
                true
            } else {
                false
            },
            "by rsync should success."
        );

        tutil::change_file_content(&local_file_name)?;
        let _r = copy_a_file(
            &mut server,
            test_dir1.tmp_dir_path(),
            test_dir2.tmp_dir_str(),
            test_dir2.tmp_file_name_only()?.as_str(),
            test_dir2.tmp_file_len()?,
            SyncType::Rsync,
            &mut indicator,
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
        (0..1000).for_each(|_i| {
            let mut u8_buf = [0_u8; 8192];
            u8_buf.last_mut().replace(&mut 1);
        });

        println!("array: {:?}", start.elapsed().as_millis());

        let start = std::time::Instant::now();
        (0..1000).for_each(|_i| {
            let mut u8_buf = vec![0_u8; 8192];
            u8_buf.last_mut().replace(&mut 1);
        });

        println!("vec: {:?}", start.elapsed().as_millis())
    }

    #[test]
    pub fn t_window_path() {
        let ps = "D:\\abc";
        let p = Path::new(ps);

        eprintln!("{:?}", p);
    }
}
