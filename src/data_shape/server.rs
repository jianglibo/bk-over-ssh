use crate::actions::{copy_a_file_item, SyncDirReport};
use crate::data_shape::{
    load_remote_item_owned, string_path, AppConf, FileItem, FileItemProcessResult,
    FileItemProcessResultStats, RemoteFileItemOwned, SyncType,
};
use glob::Pattern;
use log::*;
use serde::Deserialize;
use ssh2;
use std::io::prelude::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, io, io::Seek};
use tar::{Archive, Builder};
use walkdir::WalkDir;

#[derive(Deserialize, Debug)]
pub enum AuthMethod {
    Password,
    Agent,
    IdentityFile,
}

#[derive(Deserialize, Default, Debug)]
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
    pub fn new(
        remote_dir: impl AsRef<str>,
        local_dir: impl AsRef<str>,
        includes: Vec<impl AsRef<str>>,
        excludes: Vec<impl AsRef<str>>,
    ) -> Self {
        let mut o = Self {
            remote_dir: remote_dir.as_ref().to_string(),
            local_dir: local_dir.as_ref().to_string(),
            includes: includes.iter().map(|s| s.as_ref().to_string()).collect(),
            excludes: excludes.iter().map(|s| s.as_ref().to_string()).collect(),
            ..Directory::default()
        };
        o.compile_patterns()
            .expect("directory pattern should compile.");
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
pub struct PruneStrategy {
    pub yearly: u8,
    pub monthly: u8,
    pub weekly: u8,
    pub daily: u8,
    pub hourly: u8,
    pub minutely: u8,
}

#[derive(Deserialize)]
pub struct Server {
    pub id_rsa: String,
    pub id_rsa_pub: String,
    pub auth_method: AuthMethod,
    pub host: String,
    pub port: u16,
    pub remote_exec: String,
    pub file_list_file: String,
    pub remote_server_yml: String,
    pub username: String,
    pub rsync_valve: u64,
    pub directories: Vec<Directory>,
    pub prune_strategy: PruneStrategy,
    #[serde(skip)]
    _tcp_stream: Option<TcpStream>,
    #[serde(skip)]
    session: Option<ssh2::Session>,
    #[serde(skip)]
    report_dir: Option<PathBuf>,
    #[serde(skip)]
    tar_dir: Option<PathBuf>,
}

impl Server {
    #[allow(dead_code)]
    pub fn copy_a_file(
        &mut self,
        local: impl AsRef<str>,
        remote: impl AsRef<str>,
    ) -> Result<(), failure::Error> {
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

    pub fn get_dir_sync_report_file(&self) -> PathBuf {
        self.report_dir
            .as_ref()
            .expect("report_dir of server should always exists.")
            .join("sync_dir_report.json")
    }

    pub fn load_from_yml(
        servers_dir: impl AsRef<Path>,
        data_dir: impl AsRef<str>,
        name: impl AsRef<str>,
    ) -> Result<Server, failure::Error> {
        trace!("got server yml name: {:?}", name.as_ref());
        let mut server_yml_path = Path::new(name.as_ref()).to_path_buf();
        if (server_yml_path.is_absolute() || name.as_ref().starts_with('/'))
            && !server_yml_path.exists()
        {
            bail!(
                "server yml file does't exist, please create one: {:?}",
                server_yml_path
            );
        } else {
            server_yml_path = servers_dir.as_ref().join(name.as_ref());
            if !server_yml_path.exists() {
                let bytes = include_bytes!("../server_template.yaml");
                let mut f = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&server_yml_path)?;
                f.write_all(&bytes[..])?;
                bail!(
                    "server yml file does't exist and had created one for you: {:?}",
                    server_yml_path
                );
            }
        }
        let mut f = fs::OpenOptions::new().read(true).open(&server_yml_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let mut server: Server = serde_yaml::from_str(&buf)?;
        let data_dir = data_dir.as_ref();
        let maybe_local_server_base_dir = Path::new(data_dir).join(&server.host);
        let report_dir = Path::new(data_dir).join("report").join(&server.host);
        let tar_dir = Path::new(data_dir).join("tar").join(&server.host);
        if !report_dir.exists() {
            fs::create_dir_all(&report_dir)?;
        }
        if !tar_dir.exists() {
            fs::create_dir_all(&tar_dir)?;
        }
        server.report_dir = Some(report_dir);
        server.tar_dir = Some(tar_dir);

        server.directories.iter_mut().try_for_each(|d| {
            d.compile_patterns().unwrap();
            trace!("origin directory: {:?}", d);
            let ld = d.local_dir.trim();
            if ld.is_empty() || ld == "~" || ld == "null" {
                let mut splited = d.remote_dir.trim().rsplitn(3, &['/', '\\'][..]);
                let mut s = splited.next().expect("remote_dir should has dir name.");
                if s.is_empty() {
                    s = splited.next().expect("remote_dir should has dir name.");
                }
                d.local_dir = s.to_string();
                info!("local_dir is empty. change to {}", s);
            } else {
                d.local_dir = ld.to_string();
            }

            let dpath = Path::new(&d.local_dir);
            if dpath.is_absolute() {
                bail!("the local_dir of a server can't be absolute. {:?}", dpath);
            } else {
                let ld_path = maybe_local_server_base_dir.join(&d.local_dir);
                d.local_dir = ld_path
                    .to_str()
                    .expect("local_dir to_str should success.")
                    .to_string();
                if ld_path.exists() {
                    fs::create_dir_all(ld_path)?;
                }
                Ok(())
            }
        })?;
        trace!(
            "loaded server: {:?}",
            server
                .directories
                .iter()
                .map(|d| format!("{}, {}", d.local_dir, d.remote_dir))
                .collect::<Vec<String>>()
        );
        Ok(server)
    }

    fn tar_local(&self) -> Result<(), failure::Error> {
        if let Some(tf) = self.tar_dir.as_ref() {
            let file = fs::File::create("foo.tar").unwrap();
            let mut a = Builder::new(file);
            a.append_dir_all("bardir", ".").unwrap();

            for dir in self.directories.iter() {
                let d_path = Path::new(&dir.local_dir);
                if let Some(d_path_name) = d_path.file_name() {
                    a.append_dir_all(d_path_name, d_path).unwrap();
                } else {
                    error!("dir.local_dir get file_name failed: {:?}", d_path);
                }
            }
        } else {
            error!("empty tar_dir in the server.");
        }
        Ok(())
    }

    pub fn load_from_yml_with_app_config(
        app_conf: &AppConf,
        name: impl AsRef<str>,
    ) -> Result<Server, failure::Error> {
        Server::load_from_yml(app_conf.get_servers_dir(), app_conf.get_data_dir(), name)
    }

    pub fn is_connected(&self) -> bool {
        // self._tcp_stream.is_some() && self.session.is_some()
        self.session.is_some()
    }
    #[allow(dead_code)]
    pub fn get_ssh_session(&mut self) -> &ssh2::Session {
        self.connect().expect("ssh connection should be created.");
        self.session.as_ref().expect("session should be created.")
    }

    fn create_ssh_session(&self) -> Result<(Option<TcpStream>, ssh2::Session), failure::Error> {
        let url = format!("{}:{}", self.host, self.port);
        trace!("connecting to: {}", url);
        let tcp = TcpStream::connect(&url)?;
        if let Some(mut sess) = ssh2::Session::new() {
            // sess.set_tcp_stream(tcp);
            sess.handshake(&tcp)?;
            match self.auth_method {
                AuthMethod::Agent => {
                    let mut agent = sess.agent()?;
                    agent.connect()?;
                    agent.list_identities()?;

                    for id in agent.identities() {
                        match id {
                            Ok(identity) => {
                                trace!("start authenticate with pubkey.");
                                if let Err(err) = agent.userauth(&self.username, &identity) {
                                    warn!("ssh agent authentication failed. {:?}", err);
                                } else {
                                    break;
                                }
                            }
                            Err(err) => warn!("can't get key from ssh agent {:?}.", err),
                        }
                    }
                }
                AuthMethod::IdentityFile => {
                    trace!(
                        "about authenticate to {:?} with IdentityFile: {:?}",
                        url,
                        self.id_rsa_pub,
                    );
                    sess.userauth_pubkey_file(
                        &self.username,
                        Some(Path::new(&self.id_rsa_pub)),
                        Path::new(&self.id_rsa),
                        None,
                    )?;
                }
                AuthMethod::Password => {
                    bail!("password authentication not supported.");
                }
            }
            // Ok((None, sess))
            Ok((Some(tcp), sess))
        } else {
            bail!("Session::new failed.");
        }
    }

    pub fn connect(&mut self) -> Result<(), failure::Error> {
        if !self.is_connected() {
            let (tcp, sess) = self.create_ssh_session()?;
            if tcp.is_some() {
                self._tcp_stream.replace(tcp.unwrap());
            }
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
        contents.clear();
        channel.stderr().read_to_string(&mut contents)?;
        trace!("list-local-files stderr: {:?}", contents);
        if !contents.is_empty() {
            bail!("list-local-files return error: {:?}", contents);
        }
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
    fn _start_sync<R: Read>(
        &self,
        file_item_lines: R,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        let sftp = self.session.as_ref().unwrap().sftp()?;
        let result = io::BufReader::new(file_item_lines).lines().map(|line_r|{
            match line_r {
                Ok(line) => {
                    if line.starts_with('{') {
                        trace!("got item line {}", line);
                        if let (Some(rd), Some(ld)) =
                            (current_remote_dir.as_ref(), current_local_dir)
                        {
                            match serde_json::from_str::<RemoteFileItemOwned>(&line) {
                                Ok(remote_item) => {
                                    let sync_type = if self.rsync_valve > 0 && remote_item.get_len() > self.rsync_valve {
                                        SyncType::Rsync
                                    } else {
                                        SyncType::Sftp
                                    };
                                    let local_item = FileItem::new(
                                        ld,
                                        rd.as_str(),
                                        remote_item,
                                        sync_type,
                                    );
                                    if local_item.had_changed() {
                                        copy_a_file_item(&self, &sftp, local_item)
                                    } else {
                                        FileItemProcessResult::Skipped(local_item.get_local_path_str().expect("get_local_path_str should has some at thia point."))
                                    }
                                }
                                Err(err) => {
                                    error!("deserialize line failed: {}, {:?}", line, err);
                                    FileItemProcessResult::DeserializeFailed(line)
                                }
                            }
                        } else {
                            FileItemProcessResult::SkipBecauseNoBaseDir
                        }
                    } else {
                        // it's a directory line.
                        trace!("got directory line, it's a remote represent of path, be careful: {:?}", line);
                        if let Some(rd) = self.directories.iter().find(|d| string_path::path_equal(&d.remote_dir, &line)) {
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
                FileItemProcessResult::Successed(fl, _,_) => {
                    accu.bytes_transfered += fl;
                    accu.successed += 1;
                   },
                FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
                FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
                FileItemProcessResult::ReadLineFailed => accu.read_line_failed += 1,
            };
            accu
        });
        Ok(result)
    }

    pub fn sync_dirs(&mut self, skip_sha1: bool) -> Result<SyncDirReport, failure::Error> {
        let start = Instant::now();
        self.connect()?;
        let rs = self.start_sync(skip_sha1, Option::<fs::File>::None)?;
        Ok(SyncDirReport::new(start.elapsed(), rs))
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
    use crate::develope::tutil;
    use crate::log_util;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::server"],
            Some(vec!["ssh2"]),
        ).expect("init log should success.");
    }

    fn load_server_yml() -> Server {
        Server::load_from_yml("data/servers", "data", "localhost.yml").unwrap()
    }

    #[test]
    fn t_rsplitn() {
        let s = "a/b/c\\d/c0";
        assert_eq!(s.rsplitn(2, &['/', '\\'][..]).next(), Some("c0"));
        assert_eq!(s.rsplitn(20, &['/', '\\'][..]).next(), Some("c0"));

        let s = "a/b/c\\d\\c0";
        assert_eq!(s.rsplitn(2, &['/', '\\'][..]).next(), Some("c0"));
        assert_eq!(s.rsplitn(20, &['/', '\\'][..]).next(), Some("c0"));

        let s = "a/b/c\\d\\c0\\";
        assert!(s.ends_with(&['/', '\\'][..]));
        // assert_eq!(s.rsplitn(2, &['/', '\\'][..]).next(), Some("c0"));
        // assert_eq!(s.rsplitn(20, &['/', '\\'][..]).next(), Some("c0"));
    }

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let server = load_server_yml();
        assert_eq!(
            server.directories[0].excludes,
            vec!["*.log".to_string(), "*.bak".to_string()]
        );
        Ok(())
    }

    #[test]
    fn t_connect_server() -> Result<(), failure::Error> {
        log();
        let mut server = load_server_yml();
        info!("start connecting...");
        server.connect()?;
        assert!(server.is_connected());
        Ok(())
    }

    #[test]
    fn t_download_dirs() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let d = Path::new("target/adir");
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
        assert!(!d.exists());
        let mut server = load_server_yml();
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
        log();
        let mut server = load_server_yml();
        let stats = server.sync_dirs(true)?;
        info!("result {:?}", stats);
        Ok(())
    }

    #[test]
    fn t_tar() -> Result<(), failure::Error> {
        use tar::{Archive, Builder};
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::server"],
            Some(vec!["ssh2"]),
        )?;
        let test_dir = tutil::create_a_dir_and_a_file_with_content("a", "cc")?;

        let tar_dir = tutil::TestDir::new();
        let tar_file_path = tar_dir.tmp_dir_path().join("aa.tar");
        let tar_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&tar_file_path)?;
        {
            let mut a = Builder::new(tar_file);
            let p = test_dir.tmp_file_str()?;
            info!("file path: {:?}", p);
            a.append_file("xx.xx", &mut fs::File::open(&p)?)?;
        }

        let file = fs::File::open(&tar_file_path)?;
        let mut a = Archive::new(file);
        let file = a.entries()?.next().expect("tar entry should have one.");
        let file = file.unwrap();
        info!("{:?}", file.header());
        assert_eq!(file.header().path()?, Path::new("xx.xx"));
        Ok(())
    }
}
