use crate::actions::copy_a_file_item;
use crate::data_shape::{
    load_remote_item_owned, FileItem, FileItemProcessResult, FileItemProcessResultStats,
    RemoteFileItem, SyncType,
};
use glob::Pattern;
use log::*;
use serde::Deserialize;
use ssh2;
use std::io::prelude::{BufRead, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::{fs, io, io::Seek};
// use tempfile;

#[derive(Deserialize, Default)]
pub struct Directory {
    pub remote_dir: String,
    pub local_dir: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    #[serde(skip)]
    pub includes_patterns: Option<Vec<Pattern>>,
    #[serde(skip)]
    pub excludes_patterns: Option<Vec<Pattern>>,
}

impl Directory {

    /// for test purpose.
    #[allow(dead_code)]
    pub fn new(remote_dir: impl AsRef<str>, local_dir: impl AsRef<str>, includes: Vec<impl AsRef<str>>, excludes: Vec<impl AsRef<str>>) -> Self {
        let mut o = Self {
            remote_dir: remote_dir.as_ref().to_string(),
            local_dir: local_dir.as_ref().to_string(),
            includes: includes.iter().map(|s|s.as_ref().to_string()).collect(),
            excludes: excludes.iter().map(|s|s.as_ref().to_string()).collect(),
            ..Directory::default()
        };
        o.compile_patterns().expect("directory pattern should compile.");
        o
    }
    /// if has exclude items any matching will return None,
    /// if has include items, only matched item will return.
    /// if neither exclude nor include, mathes.
    pub fn match_path(&self, path: PathBuf) -> Option<PathBuf> {
        let mut no_exlucdes = false;
        if let Some(exclude_ptns) = self.excludes_patterns.as_ref() {
            if exclude_ptns.iter().any(|p| p.matches_path(&path)) {
                return None;
            }
        } else {
            no_exlucdes = true;
        }

        if let Some(include_ptns) = self.includes_patterns.as_ref() {
            if include_ptns.is_empty() {
                return Some(path);
            }
            if include_ptns.iter().any(|p| p.matches_path(&path)) {
                return Some(path);
            }
        } else if no_exlucdes {
            return Some(path);
        }
        None
    }

    pub fn compile_patterns(&mut self) -> Result<(), failure::Error> {
        if self.includes_patterns.is_none() {
            self.includes_patterns.replace(
                self.includes
                    .iter()
                    .map(|s| Pattern::new(s).unwrap())
                    .collect(),
            );
        }

        if self.excludes_patterns.is_none() {
            self.excludes_patterns.replace(
                self.excludes
                    .iter()
                    .map(|s| Pattern::new(s).unwrap())
                    .collect(),
            );
        }

        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Server {
    pub id_rsa: String,
    pub id_rsa_pub: String,
    pub host: String,
    pub port: u16,
    pub remote_exec: String,
    pub file_list_file: String,
    pub remote_server_yml: String,
    pub username: String,
    pub rsync_valve: u64,
    pub directories: Vec<Directory>,
    #[serde(skip)]
    _tcp_stream: Option<TcpStream>,
    #[serde(skip)]
    session: Option<ssh2::Session>,
}

impl Server {
    pub fn copy_a_file(&mut self, local: impl AsRef<str>, remote: impl AsRef<str>) -> Result<(), failure::Error> {
        self.connect()?;
        let sftp: ssh2::Sftp = self.session.as_ref().unwrap().sftp()?;

        info!("copy from {:?} to {:?}", local.as_ref(), remote.as_ref());
        let mut r_file = sftp.create(Path::new(remote.as_ref()))?;
        let mut l_file = fs::File::open(local.as_ref())?;
        io::copy(&mut l_file, &mut r_file)?;
        Ok(())
    }
    /// Test purpose.
    #[allow(dead_code)]
    pub fn replace_directories(&mut self, directories: Vec<Directory>) {
        self.directories = directories;
    }

    pub fn guess_server_yml_file(name: impl AsRef<str>) -> Result<PathBuf, failure::Error> {
        let name = name.as_ref();

        let server_path = if name.contains('\\') || name.contains('/') {
            Path::new(name).to_path_buf()
        } else {
            let name_only = if !name.ends_with(".yml") {
                format!("{}.yml", name)
            } else {
                name.to_string()
            };
            let sp = std::env::current_exe()?
                .parent()
                .expect("executable's parent should exists.")
                .join("servers")
                .join(&name_only);

            if sp.exists() {
                sp
            } else {
                std::env::current_dir()?.join("servers").join(&name_only)
            }
        };
        Ok(server_path)
    }
    pub fn load_from_yml(name: impl AsRef<str>) -> Result<Server, failure::Error> {
        let server_path = Server::guess_server_yml_file(name)?;
        info!("loading server configuration: {:?}", server_path);
        let mut f = fs::OpenOptions::new().read(true).open(server_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let mut server: Server = serde_yaml::from_str(&buf)?;
        server
            .directories
            .iter_mut()
            .for_each(|d| d.compile_patterns().unwrap());
        Ok(server)
    }

    pub fn is_connected(&self) -> bool {
        self._tcp_stream.is_some() && self.session.is_some()
    }

    pub fn get_ssh_session(&mut self) -> &ssh2::Session {
        self.connect().expect("ssh connection should be created.");
        self.session.as_ref().expect("session should be created.")
    }


    fn create_ssh_session(&self) -> Result<(TcpStream, ssh2::Session), failure::Error> {
        let url = format!("{}:{}", self.host, self.port);
        let tcp = TcpStream::connect(&url)?;
        if let Some(mut sess) = ssh2::Session::new() {
            sess.handshake(&tcp)?;
            sess.userauth_pubkey_file(
                &self.username,
                Some(Path::new(&self.id_rsa_pub)),
                Path::new(&self.id_rsa),
                None,
            )?;
            Ok((tcp, sess))
        } else {
            bail!("Session::new failed.");
        }
    }

    pub fn connect(&mut self) -> Result<(), failure::Error> {
        if !self.is_connected() {
            let (tcp, sess) = self.create_ssh_session()?;
            self._tcp_stream.replace(tcp);
            self.session.replace(sess);
        }
        Ok(())
    }

    pub fn create_channel(&self) -> Result<ssh2::Channel, failure::Error> {
        Ok(self
            .session
            .as_ref()
            .expect("should already connected.")
            .channel_session()
            .expect("a channel session."))
    }

    pub fn list_remote_file_sftp(
        &mut self,
        out: &mut impl io::Write,
        skip_sha1: bool,
    ) -> Result<(), failure::Error> {
        self.connect()?;
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} rsync list-local-files --server-yml {} --out {}{}",
            self.remote_exec,
            self.remote_server_yml,
            self.file_list_file,
            if skip_sha1 { " --skip-sha1" } else { "" }
        );
        trace!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let mut contents = String::new();
        channel.read_to_string(&mut contents)?;
        trace!("list-local-files output: {:?}", contents);
        let sftp = self.session.as_ref().unwrap().sftp()?;
        let mut f = sftp.open(Path::new(&self.file_list_file))?;
        io::copy(&mut f, out)?;
        Ok(())
    }

    pub fn list_remote_file_exec(
        &mut self,
        out: &mut impl io::Write,
        skip_sha1: bool,
    ) -> Result<(), failure::Error> {
        self.connect()?;
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} rsync list-local-files --server-yml {}{}",
            self.remote_exec,
            self.remote_server_yml,
            if skip_sha1 { " --skip-sha1" } else { "" }
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        io::copy(&mut channel, out)?;
        Ok(())
    }

    pub fn start_sync<R: Read>(
        &mut self,
        skip_sha1: bool,
        file_item_lines: Option<R>,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        self.connect()?;
        if let Some(r) = file_item_lines {
            self._start_sync(r)
        } else {
            let mut cursor = io::Cursor::new(Vec::<u8>::new());
            self.list_remote_file_sftp(&mut cursor, skip_sha1)?;
            cursor.seek(io::SeekFrom::Start(0))?;
            self._start_sync(cursor)
        }
    }

    /// Do not try to return item stream from this function. consume it locally, pass in function to alter the behavior.
    fn _start_sync<R: Read>(&self, file_item_lines: R) -> Result<FileItemProcessResultStats, failure::Error> {
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        let sftp = self.session.as_ref().unwrap().sftp()?;
        let result = io::BufReader::new(file_item_lines).lines().map(|line_r|{
            match line_r {
                Ok(line) => {
                    // info!("got line: {:?}", line);
                    if line.starts_with('{') {
                        if let (Some(rd), Some(ld)) =
                            (current_remote_dir.as_ref(), current_local_dir)
                        {
                            match serde_json::from_str::<RemoteFileItem>(&line) {
                                Ok(remote_item) => {
                                    let sync_type = if self.rsync_valve > 0 && remote_item.get_len() > self.rsync_valve {
                                        SyncType::Rsync
                                    } else {
                                        SyncType::Sftp
                                    };
                                    let local_item = FileItem::new(
                                        ld,
                                        rd.as_str(),
                                        &remote_item,
                                        sync_type,
                                    );
                                    if local_item.had_changed() {
                                        copy_a_file_item(&self, &sftp, local_item)
                                    } else {
                                        FileItemProcessResult::Skipped(local_item.get_local_path_str().expect("get_local_path_str should has some at thia point."))
                                    }
                                }
                                Err(err) => {
                                    error!("deserialize line failed: {:?}, {:?}", line, err);
                                    FileItemProcessResult::DeserializeFailed(line)
                                }
                            }
                        } else {
                            FileItemProcessResult::SkipBecauseNoBaseDir
                        }
                    } else {
                        // it's a directory line.
                        if let Some(rd) = self.directories.iter().find(|d| d.remote_dir == line) {
                            current_remote_dir = Some(line.clone());
                            current_local_dir = Some(Path::new(rd.local_dir.as_str()));
                            FileItemProcessResult::Directory(line)
                        } else {
                            // cannot find corepsonding local_dir, skipping following lines.
                            error!("can't find corepsonding local_dir: {:?}", line);
                            current_remote_dir = None;
                            current_local_dir = None;
                            FileItemProcessResult::NoCorresponedLocalDir(line)
                        }
                    }
                }
                Err(err) => {
                    error!("read line failed: {:?}", err);
                    FileItemProcessResult::ReadLineFailed
                }
            }
        }).fold(FileItemProcessResultStats::default(), |mut accu, item|{
            match item {
                FileItemProcessResult::DeserializeFailed(_) => accu.deserialize_failed += 1,
                FileItemProcessResult::Skipped(_) => accu.skipped += 1,
                FileItemProcessResult::NoCorresponedLocalDir(_) => accu.no_corresponed_local_dir += 1,
                FileItemProcessResult::Directory(_) => accu.directory += 1,
                FileItemProcessResult::LengthNotMatch(_) => accu.length_not_match += 1,
                FileItemProcessResult::Sha1NotMatch(_) => accu.sha1_not_match += 1,
                FileItemProcessResult::CopyFailed(_) => accu.copy_failed += 1,
                FileItemProcessResult::SkipBecauseNoBaseDir => accu.skip_because_no_base_dir += 1,
                FileItemProcessResult::Successed(_,_) => accu.successed += 1,
                FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
                FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
                FileItemProcessResult::ReadLineFailed => accu.read_line_failed += 1,
            };
            accu
        });
        Ok(result)
    }

    pub fn sync_dirs(&mut self, skip_sha1: bool) -> Result<FileItemProcessResultStats, failure::Error> {
        self.connect()?;
        self.start_sync(skip_sha1, Option::<fs::File>::None)
    }

    #[allow(dead_code)]
    pub fn sync_file_item_lines<R: Read>(
        &mut self,
        skip_sha1: bool,
        from: R,
    ) -> Result<(), failure::Error> {
        self.connect()?;
        self.start_sync(skip_sha1, Some(from))?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn load_dirs<'a, O: io::Write>(
        &self,
        out: &'a mut O,
        skip_sha1: bool,
    ) -> Result<(), failure::Error> {
        for one_dir in self.directories.iter() {
            load_remote_item_owned(one_dir, out, skip_sha1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let server = Server::load_from_yml("localhost")?;
        assert_eq!(
            server.directories[0].excludes,
            vec!["*.log".to_string(), "*.bak".to_string()]
        );
        Ok(())
    }

    #[test]
    fn t_connect_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let mut server = Server::load_from_yml("localhost")?;
        server.connect()?;
        assert!(server.is_connected());
        Ok(())
    }

    #[test]
    fn t_download_dirs() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let d = Path::new("target/adir");
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
        assert!(!d.exists());
        let mut server = Server::load_from_yml("localhost")?;
        server.connect()?;
        let f = fs::OpenOptions::new()
            .read(true)
            .open("fixtures/linux_remote_item_dir.txt")?;
        server.start_sync(true, Some(f))?;

        assert!(d.exists());
        Ok(())
    }

    #[test]
    fn t_sync_dirs() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let d = Path::new("target/adir");
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
        assert!(!d.exists());
        let mut server = Server::load_from_yml("localhost")?;
        let stats = server.sync_dirs(true)?;
        assert_eq!(stats.successed, 1);
        assert!(d.exists());
        Ok(())
    }
}
