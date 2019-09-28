use crate::actions::{copy_a_file_item, SyncDirReport};
use crate::data_shape::{
    load_remote_item, load_remote_item_to_sqlite, rolling_files, string_path, AppConf, FileItem,
    FileItemProcessResult, FileItemProcessResultStats, RemoteFileItem, SyncType,
};
use crate::db_accesses::DbAccess;
use crate::ioutil::SharedMpb;
use crate::rustsync::{DeltaFileReader, DeltaReader, Signature};
use bzip2::write::BzEncoder;
use bzip2::Compression;
use glob::Pattern;
use indicatif::{ProgressBar, ProgressStyle};
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use ssh2;
use std::io::prelude::{BufRead, Read};
use std::marker::PhantomData;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, io, io::Seek};
use tar::Builder;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum ServerRole {
    PureClient,
    PureServer,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum AuthMethod {
    Password,
    Agent,
    IdentityFile,
}

#[derive(Deserialize, Serialize, Default, Debug)]
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
    pub fn get_remote_dir(&self) -> &str {
        self.remote_dir.as_str()
    }

    pub fn get_remote_canonicalized_dir_str(&self) -> Option<String> {
        let bp = Path::new(self.get_remote_dir()).canonicalize();
        match bp {
            Ok(base_path) => {
                if let Some(path_str) = base_path.to_str() {
                    return Some(path_str.to_owned());
                } else {
                    error!("base_path to_str failed: {:?}", base_path);
                }
            }
            Err(err) => {
                error!("load_dir resolve path failed: {:?}", err);
            }
        }
        None
    }
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
    /// if has includes get includes first.
    /// if has excludes exclude files.
    pub fn match_path(&self, path: PathBuf) -> Option<PathBuf> {
        let has_includes = self.includes_patterns.is_some();
        let keep_file = if has_includes {
            self.includes_patterns
                .as_ref()
                .unwrap()
                .iter()
                .any(|ptn| ptn.matches_path(&path))
        } else {
            true
        };

        if !keep_file {
            return None;
        }

        let has_exlucdes = self.excludes_patterns.is_some();

        let keep_file = if has_exlucdes {
            !self
                .excludes_patterns
                .as_ref()
                .unwrap()
                .iter()
                .any(|p| p.matches_path(&path))
        } else {
            true
        };

        if keep_file {
            Some(path)
        } else {
            None
        }
    }
    /// When includes is empyt, includes_patterns will be None, excludes is the same.
    pub fn compile_patterns(&mut self) -> Result<(), failure::Error> {
        if self.includes_patterns.is_none() && !self.includes.is_empty() {
            self.includes_patterns.replace(
                self.includes
                    .iter()
                    .map(|s| Pattern::new(s).unwrap())
                    .collect(),
            );
        }

        if self.excludes_patterns.is_none() && !self.excludes.is_empty() {
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

#[derive(Builder, Deserialize, Serialize, Debug)]
#[builder(setter(into))]
pub struct PruneStrategy {
    #[builder(default = "1")]
    pub yearly: u8,
    #[builder(default = "1")]
    pub monthly: u8,
    #[builder(default = "0")]
    pub weekly: u8,
    #[builder(default = "1")]
    pub daily: u8,
    #[builder(default = "1")]
    pub hourly: u8,
    #[builder(default = "1")]
    pub minutely: u8,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum CompressionImpl {
    Bzip2,
}

#[derive(Deserialize, Serialize)]
pub struct ServerYml {
    pub id_rsa: String,
    pub id_rsa_pub: Option<String>,
    pub auth_method: AuthMethod,
    pub host: String,
    pub port: u16,
    pub remote_exec: String,
    pub file_list_file: String,
    pub remote_server_yml: String,
    pub username: String,
    pub rsync_valve: u64,
    pub directories: Vec<Directory>,
    pub role: ServerRole,
    pub prune_strategy: PruneStrategy,
    pub archive_prefix: String,
    pub archive_postfix: String,
    pub compress_archive: Option<CompressionImpl>,
    pub buf_len: usize,
    pub rsync_window: usize,
    pub use_db: bool,
    #[serde(skip)]
    pub yml_location: Option<PathBuf>,
}

pub struct Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub server_yml: ServerYml,
    _tcp_stream: Option<TcpStream>,
    session: Option<ssh2::Session>,
    report_dir: PathBuf,
    tar_dir: PathBuf,
    working_dir: PathBuf,
    pub yml_location: Option<PathBuf>,
    pub multi_bar: Option<SharedMpb>,
    pub pb: Option<(ProgressBar, ProgressBar)>,
    pub db_access: Option<D>,
    _m: PhantomData<M>,
}

impl<M, D> Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub fn get_host(&self) -> &str {
        self.server_yml.host.as_str()
    }

    pub fn get_port(&self) -> u16 {
        self.server_yml.port
    }

    #[allow(dead_code)]
    pub fn copy_a_file(
        &mut self,
        local: impl AsRef<str>,
        remote: impl AsRef<str>,
    ) -> Result<(), failure::Error> {
        self.connect()?;
        let local = local.as_ref();
        let remote = remote.as_ref();
        let sftp: ssh2::Sftp = self.session.as_ref().unwrap().sftp()?;

        if let Ok(mut r_file) = sftp.create(Path::new(remote)) {
            let mut l_file = fs::File::open(local)?;
            io::copy(&mut l_file, &mut r_file)?;
        } else {
            bail!(
                "copy from {:?} to {:?} failed. maybe remote file's parent dir didn't exists.",
                local,
                remote
            );
        }
        Ok(())
    }

    pub fn pb_finish(&self) {
        if let Some((pb_total, pb_item)) = self.pb.as_ref() {
            pb_total.finish();
            // pb_item.finish();
            pb_item.finish_and_clear();
        }
    }

    pub fn dir_equals(&self, directories: &[Directory]) -> bool {
        let ss: Vec<&String> = self
            .server_yml
            .directories
            .iter()
            .map(|d| &d.remote_dir)
            .collect();
        let ass: Vec<&String> = directories.iter().map(|d| &d.remote_dir).collect();
        ss == ass
    }

    pub fn stats_remote_exec(&mut self) -> Result<ssh2::FileStat, failure::Error> {
        let re = &self.server_yml.remote_exec.clone();
        self.get_server_file_stats(&re)
    }

    fn get_server_file_stats(
        &mut self,
        remote: impl AsRef<Path>,
    ) -> Result<ssh2::FileStat, failure::Error> {
        let sftp: ssh2::Sftp = self
            .session
            .as_ref()
            .expect("server should already connected.")
            .sftp()?;
        let stat = match sftp.stat(remote.as_ref()) {
            Ok(stat) => stat,
            Err(err) => {
                bail!("stat sftp file {:?} failed, {:?}", remote.as_ref(), err);
            }
        };
        Ok(stat)
    }

    pub fn get_remote_file_content(
        &mut self,
        remote: impl AsRef<Path>,
    ) -> Result<String, failure::Error> {
        let sftp: ssh2::Sftp = self.session.as_ref().unwrap().sftp()?;
        let content = match sftp.open(remote.as_ref()) {
            Ok(mut r_file) => {
                let mut content = String::new();
                r_file.read_to_string(&mut content)?;
                content
            }
            Err(err) => {
                bail!("open sftp file {:?} failed, {:?}", remote.as_ref(), err);
            }
        };
        Ok(content)
    }
    /// Test purpose.
    #[allow(dead_code)]
    pub fn replace_directories(&mut self, directories: Vec<Directory>) {
        self.server_yml.directories = directories;
    }

    pub fn get_dir_sync_report_file(&self) -> PathBuf {
        self.report_dir.join("sync_dir_report.json")
    }

    pub fn get_working_file_list_file(&self) -> PathBuf {
        self.working_dir.join("file_list_working.txt")
    }

    pub fn remove_working_file_list_file(&self) {
        let wf = self.get_working_file_list_file();
        if let Err(err) = fs::remove_file(&wf) {
            error!(
                "delete working file list file failed: {:?}, {:?}",
                self.get_working_file_list_file(),
                err
            );
        }
    }

    pub fn load_from_yml_with_app_config(
        app_conf: &AppConf<M, D>,
        name: impl AsRef<str>,
        buf_len: Option<usize>,
        multi_bar: Option<SharedMpb>,
    ) -> Result<Server<M, D>, failure::Error> {
        Server::<M, D>::load_from_yml(
            app_conf.servers_dir.as_path(),
            app_conf.data_dir_full_path.as_path(),
            name,
            buf_len,
            multi_bar,
            app_conf.get_db_access().cloned(),
        )
    }

    pub fn load_from_yml(
        servers_dir: impl AsRef<Path>,
        data_dir: impl AsRef<Path>,
        name: impl AsRef<str>,
        buf_len: Option<usize>,
        multi_bar: Option<SharedMpb>,
        db_access: Option<D>,
    ) -> Result<Server<M, D>, failure::Error> {
        let name = name.as_ref();
        trace!("got server yml name: {:?}", name);
        let mut server_yml_path = Path::new(name).to_path_buf();
        if (server_yml_path.is_absolute() || name.starts_with('/')) && !server_yml_path.exists() {
            bail!(
                "server yml file does't exist, please create one: {:?}",
                server_yml_path
            );
        } else {
            if !name.contains('/') {
                server_yml_path = servers_dir.as_ref().join(name);
            }
            if !server_yml_path.exists() {
                bail!(
                    "server yml file does't exist and had created one for you: {:?}",
                    server_yml_path
                );
            }
        }
        let mut f = fs::OpenOptions::new().read(true).open(&server_yml_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let server_yml: ServerYml = serde_yaml::from_str(&buf)?;

        let data_dir = data_dir.as_ref();
        let maybe_local_server_base_dir = data_dir.join(&server_yml.host);
        let report_dir = data_dir.join("report").join(&server_yml.host);
        let tar_dir = data_dir.join("tar").join(&server_yml.host);
        let working_dir = data_dir.join("working").join(&server_yml.host);

        if !report_dir.exists() {
            fs::create_dir_all(&report_dir)?;
        }
        if !tar_dir.exists() {
            fs::create_dir_all(&tar_dir)?;
        }

        if !working_dir.exists() {
            fs::create_dir_all(&working_dir)?;
        }

        let mut server = Server {
            server_yml,
            multi_bar,
            db_access,
            pb: None,
            _tcp_stream: None,
            report_dir,
            session: None,
            tar_dir,
            working_dir,
            yml_location: None,
            _m: PhantomData,
        };

        if let Some(buf_len) = buf_len {
            server.server_yml.buf_len = buf_len;
        }

        if let Some(mb) = server.multi_bar.as_ref() {
            let pb_total = ProgressBar::new(!0);
            let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
            pb_total.set_style(ps);
            let pb_total = mb.add(pb_total);

            let pb_item = ProgressBar::new(!0);
            let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
            pb_item.set_style(ps);
            let pb_item = mb.add(pb_item);

            server.pb = Some((pb_total, pb_item));
        }

        let ab = server_yml_path.canonicalize()?;
        server.yml_location.replace(ab);

        server.server_yml.directories.iter_mut().try_for_each(|d| {
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
                trace!("local_dir is empty. change to {}", s);
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
                .server_yml
                .directories
                .iter()
                .map(|d| format!("{}, {}", d.local_dir, d.remote_dir))
                .collect::<Vec<String>>()
        );
        Ok(server)
    }

    /// get tar_dir , archive_prefix, archive_postfix from server yml configuration file.
    fn next_tar_file(&self) -> PathBuf {
        rolling_files::get_next_file_name(
            &self.tar_dir,
            &self.server_yml.archive_prefix,
            &self.server_yml.archive_postfix,
        )
    }

    fn get_archive_writer_file(&self) -> Result<impl io::Write, failure::Error> {
        Ok(fs::File::create(self.next_tar_file())?)
    }

    fn get_archive_writer_bzip2(&self) -> Result<impl io::Write, failure::Error> {
        let nf = self.next_tar_file();
        trace!("got next file: {:?}", nf);
        let out = fs::File::create(nf)?;
        Ok(BzEncoder::new(out, Compression::Best))
    }
    /// use dyn trait.
    #[allow(dead_code)]
    fn get_archive_writer(
        &self,
        tar_dir: impl AsRef<Path>,
    ) -> Result<Box<dyn io::Write>, failure::Error> {
        let next_fn = rolling_files::get_next_file_name(
            tar_dir.as_ref(),
            &self.server_yml.archive_prefix,
            &self.server_yml.archive_postfix,
        );
        let file: Box<dyn io::Write> = if let Some(ref sm) = self.server_yml.compress_archive {
            match sm {
                CompressionImpl::Bzip2 => {
                    let out = fs::File::create(next_fn)?;
                    Box::new(BzEncoder::new(out, Compression::Best))
                }
            }
        } else {
            Box::new(fs::File::create(next_fn)?)
        };
        Ok(file)
    }

    fn do_tar_local(&self, writer: impl io::Write) -> Result<(), failure::Error> {
        let mut archive = Builder::new(writer);

        for dir in self.server_yml.directories.iter() {
            let d_path = Path::new(&dir.local_dir);
            if let Some(d_path_name) = d_path.file_name() {
                if d_path.exists() {
                    archive.append_dir_all(d_path_name, d_path)?;
                } else {
                    warn!("unexist directory: {:?}", d_path);
                }
            } else {
                error!("dir.local_dir get file_name failed: {:?}", d_path);
            }
        }
        archive.finish()?;

        Ok(())
    }

    /// tar xjf(bzip2) or tar xzf(gzip).
    pub fn tar_local(&self) -> Result<(), failure::Error> {
        if let Some(ref zm) = self.server_yml.compress_archive {
            match zm {
                CompressionImpl::Bzip2 => self.do_tar_local(self.get_archive_writer_bzip2()?)?,
            };
        } else {
            self.do_tar_local(self.get_archive_writer_file()?)?;
        }
        Ok(())
    }

    pub fn prune_backups(&self) -> Result<(), failure::Error> {
        rolling_files::do_prune_dir(
            &self.server_yml.prune_strategy,
            &self.tar_dir,
            &self.server_yml.archive_prefix,
            &self.server_yml.archive_postfix,
        )?;
        Ok(())
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
        let url = format!("{}:{}", self.get_host(), self.get_port());
        trace!("connecting to: {}", url);
        let tcp = TcpStream::connect(&url)?;
        if let Some(mut sess) = ssh2::Session::new() {
            // sess.set_tcp_stream(tcp);
            sess.handshake(&tcp)?;
            match self.server_yml.auth_method {
                AuthMethod::Agent => {
                    let mut agent = sess.agent()?;
                    agent.connect()?;
                    agent.list_identities()?;

                    for id in agent.identities() {
                        match id {
                            Ok(identity) => {
                                trace!("start authenticate with pubkey.");
                                if let Err(err) =
                                    agent.userauth(&self.server_yml.username, &identity)
                                {
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
                        self.server_yml.id_rsa_pub,
                    );
                    sess.userauth_pubkey_file(
                        &self.server_yml.username,
                        self.server_yml.id_rsa_pub.as_ref().map(Path::new),
                        Path::new(&self.server_yml.id_rsa),
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
            if let Some(tcp) = tcp {
                self._tcp_stream.replace(tcp);
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

    pub fn list_remote_file_sftp(&self, skip_sha1: bool) -> Result<PathBuf, failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} list-local-files {} --out {}{}",
            self.server_yml.remote_exec,
            self.server_yml.remote_server_yml,
            self.server_yml.file_list_file,
            if skip_sha1 { " --skip-sha1" } else { "" }
        );
        trace!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let mut contents = String::new();
        if let Err(err) = channel.read_to_string(&mut contents) {
            bail!("is remote exec executable? {:?}", err);
        }
        trace!("list-local-files output: {:?}", contents);
        contents.clear();
        channel.stderr().read_to_string(&mut contents)?;
        trace!("list-local-files stderr: {:?}", contents);
        if !contents.is_empty() {
            bail!("list-local-files return error: {:?}", contents);
        }
        let sftp = self.session.as_ref().unwrap().sftp()?;
        let mut f = sftp.open(Path::new(&self.server_yml.file_list_file))?;

        let working_file = self.get_working_file_list_file();
        let mut wf = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&working_file)?;
        io::copy(&mut f, &mut wf)?;
        Ok(working_file)
    }

    pub fn create_remote_db(&self, db_type: impl AsRef<str>) -> Result<(), failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let db_type = db_type.as_ref();
        let cmd = format!(
            "{} create-db --db-type {}",
            self.server_yml.remote_exec,
            db_type,
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let mut contents = String::new();
        if let Err(err) = channel.read_to_string(&mut contents) {
            bail!("is remote exec executable? {:?}", err);
        }
        trace!("create_remote_db output: {:?}", contents);
        contents.clear();
        channel.stderr().read_to_string(&mut contents)?;
        trace!("create_remote_db stderr: {:?}", contents);
        Ok(())
    }

    pub fn list_remote_file_exec(&mut self, skip_sha1: bool, no_db: bool) -> Result<PathBuf, failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} {} list-local-files {}{}",
            self.server_yml.remote_exec,
            if no_db {" --no-db"} else {""},
            self.server_yml.remote_server_yml,
            if skip_sha1 { " --skip-sha1" } else { "" }
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let working_file = self.get_working_file_list_file();
        let mut wf = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&working_file)?;
        io::copy(&mut channel, &mut wf)?;
        Ok(working_file)
    }

    fn count_and_len(&self, input: &mut (impl io::BufRead + Seek)) -> (u64, u64) {
        let mut count_and_len = (0u64, 0u64);
        loop {
            let mut buf = String::new();
            match input.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_length) => {
                    if buf.starts_with('{') {
                        match serde_json::from_str::<RemoteFileItem>(&buf) {
                            Ok(remote_item) => {
                                count_and_len.0 += 1;
                                count_and_len.1 += remote_item.get_len();
                            }
                            Err(err) => {
                                error!("deserialize cursor line failed: {}, {:?}", buf, err);
                            }
                        };
                    }
                }
                Err(err) => {
                    error!("read line from cursor failed: {:?}", err);
                    break;
                }
            };
        }
        count_and_len
    }

    /// Preparing file list includes invoking remote command to collect file list and downloading to local.
    pub fn prepare_file_list(&self, skip_sha1: bool) -> Result<(), failure::Error> {
        if self.get_working_file_list_file().exists() {
            println!(
                "uncompleted list file exists: {:?} continue processing",
                self.get_working_file_list_file()
            );
        } else {
            self.list_remote_file_sftp(skip_sha1)?;
        }
        Ok(())
    }

    pub fn get_pb_count_and_len(&self) -> Result<(u64, u64), failure::Error> {
        let working_file = &self.get_working_file_list_file();
        let mut wfb = io::BufReader::new(fs::File::open(working_file)?);
        Ok(self.count_and_len(&mut wfb))
    }

    fn start_sync_working_file_list(
        &self,
        skip_sha1: bool,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        self.prepare_file_list(skip_sha1)?;
        let working_file = &self.get_working_file_list_file();
        let rb = io::BufReader::new(fs::File::open(working_file)?);
        self.start_sync(rb)
    }

    /// Do not try to return item stream from this function. consume it locally, pass in function to alter the behavior.
    fn start_sync<R: BufRead>(
        &self,
        file_item_lines: R,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        let mut consume_count = 0u64;
        let count_and_len_op = self.pb.as_ref().map(|_pb| {
            self.get_pb_count_and_len()
                .expect("get_pb_count_and_len should success.")
        });
        let total_count = count_and_len_op.map(|cl| cl.0).unwrap_or_default();

        if let Some((pb_total, pb_item)) = self.pb.as_ref() {
            // let style = ProgressStyle::default_bar().template("[{eta_precise}] {prefix:.bold.dim} {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {spinner} {wide_msg}").progress_chars("#-");
            let style = ProgressStyle::default_bar().template("{prefix} {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {percent}% {eta}").progress_chars("#-");
            pb_total.set_length(count_and_len_op.map(|cl| cl.1).unwrap_or(0u64));
            pb_total.set_style(style);

            let style = ProgressStyle::default_bar().template("{bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {spinner} {percent}% {eta} {wide_msg}").progress_chars("#-");
            pb_item.set_style(style);
        }

        let sftp = self.session.as_ref().unwrap().sftp()?;
        let mut buff = vec![0_u8; self.server_yml.buf_len];

        let result = file_item_lines
            .lines()
            .filter_map(|li| match li {
                Err(err) => {
                    error!("read line failed: {:?}", err);
                    None
                }
                Ok(line) => Some(line),
            }) /*.collect::<Vec<String>>().into_par_iter()*/
            .map(|line| {
                if line.starts_with('{') {
                    trace!("got item line {}", line);
                    if let (Some(rd), Some(ld)) = (current_remote_dir.as_ref(), current_local_dir) {
                        match serde_json::from_str::<RemoteFileItem>(&line) {
                            Ok(remote_item) => {
                                let remote_len = remote_item.get_len();
                                let sync_type = if self.server_yml.rsync_valve > 0
                                    && remote_item.get_len() > self.server_yml.rsync_valve
                                {
                                    SyncType::Rsync
                                } else {
                                    SyncType::Sftp
                                };
                                let local_item =
                                    FileItem::new(ld, rd.as_str(), remote_item, sync_type);
                                consume_count += 1;
                                if let Some((_pb_total, pb_item)) = self.pb.as_ref() {
                                    pb_item.reset();
                                    pb_item.set_length(remote_len);
                                    pb_item.set_message(local_item.get_remote_item().get_path());
                                }
                                let mut skiped = false;
                                let r = if local_item.had_changed() {
                                    trace!("file had changed. start copy_a_file_item.");
                                    copy_a_file_item(
                                        &self,
                                        &sftp,
                                        local_item,
                                        &mut buff,
                                        self.pb.as_ref(),
                                    )
                                } else {
                                    skiped = true;
                                    FileItemProcessResult::Skipped(
                                        local_item.get_local_path_str().expect(
                                            "get_local_path_str should has some at thia point.",
                                        ),
                                    )
                                };
                                if let Some((pb_total, pb_item)) = self.pb.as_ref() {
                                    let prefix = format!(
                                        "[{}] {}/{} ",
                                        self.get_host(),
                                        total_count - consume_count,
                                        total_count
                                    );
                                    pb_total.set_prefix(prefix.as_str());
                                    pb_total.inc(remote_len); // no matter skiped or processed.
                                    if skiped {
                                        pb_item.inc(remote_len);
                                    }
                                }
                                r
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
                    trace!(
                        "got directory line, it's a remote represent of path, be careful: {:?}",
                        line
                    );
                    let k = self
                        .server_yml
                        .directories
                        .iter()
                        .find(|d| string_path::path_equal(&d.remote_dir, &line));

                    if let Some(rd) = k {
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
            })
            .fold(FileItemProcessResultStats::default(), |mut accu, item| {
                match item {
                    FileItemProcessResult::DeserializeFailed(_) => accu.deserialize_failed += 1,
                    FileItemProcessResult::Skipped(_) => accu.skipped += 1,
                    FileItemProcessResult::NoCorresponedLocalDir(_) => {
                        accu.no_corresponed_local_dir += 1
                    }
                    FileItemProcessResult::Directory(_) => accu.directory += 1,
                    FileItemProcessResult::LengthNotMatch(_) => accu.length_not_match += 1,
                    FileItemProcessResult::Sha1NotMatch(_) => accu.sha1_not_match += 1,
                    FileItemProcessResult::CopyFailed(_) => accu.copy_failed += 1,
                    FileItemProcessResult::SkipBecauseNoBaseDir => {
                        accu.skip_because_no_base_dir += 1
                    }
                    FileItemProcessResult::Successed(fl, _, _) => {
                        accu.bytes_transfered += fl;
                        accu.successed += 1;
                    }
                    FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
                    FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
                };
                accu
            });
        Ok(result)
    }

    pub fn sync_dirs(&mut self, skip_sha1: bool) -> Result<SyncDirReport, failure::Error> {
        let start = Instant::now();
        self.connect()?;
        let rs = self.start_sync_working_file_list(skip_sha1)?;
        self.remove_working_file_list_file();
        self.pb_finish();
        Ok(SyncDirReport::new(start.elapsed(), rs))
    }

    pub fn load_dirs<O: io::Write>(
        &self,
        out: &mut O,
        skip_sha1: bool,
    ) -> Result<(), failure::Error> {
        if self.db_access.is_some() && self.server_yml.use_db {
            for one_dir in self.server_yml.directories.iter() {
                load_remote_item_to_sqlite(
                    one_dir,
                    self.db_access.as_ref().unwrap(),
                    skip_sha1,
                )?;
            }
        } else {
            for one_dir in self.server_yml.directories.iter() {
                load_remote_item(one_dir, out, skip_sha1)?;
            }
        }
        Ok(())
    }

    //     pub fn copy_a_file_item_rsync<'a>(&self,
    //     sftp: &ssh2::Sftp,
    //     local_file_path: String,
    //     file_item: &FileItem<'a>,
    // ) -> Result<FileItemProcessResult, failure::Error> {
    //     let remote_path = file_item.get_remote_path();
    //     trace!("start signature_a_file {}", &local_file_path);
    //     let mut sig = Signature::signature_a_file(&local_file_path, Some(self.server_yml.rsync_window), self.pb.is_some())?;
    //     let remote_sig_file_path = format!("{}.sig", &remote_path);
    //     let sig_file = sftp.create(Path::new(&remote_sig_file_path))?;
    //     sig.write_to_stream(sig_file)?;
    //     let delta_file_name = format!("{}.delta", &remote_path);
    //     let cmd = format!(
    //         "{} rsync delta-a-file --new-file {} --sig-file {} --out-file {}",
    //         self.server_yml.remote_exec, &remote_path, &remote_sig_file_path, &delta_file_name,
    //     );
    //     trace!("about to invoke command: {:?}", cmd);
    //     let mut channel: ssh2::Channel = self.create_channel()?;
    //     channel.exec(cmd.as_str())?;
    //     let mut ch_stderr = channel.stderr();
    //     let mut chout = String::new();
    //     ch_stderr.read_to_string(&mut chout)?;
    //     trace!(
    //         "after invoke delta command, there maybe err in channel: {:?}",
    //         chout
    //     );
    //     channel.read_to_string(&mut chout)?;
    //     trace!(
    //         "delta-a-file output: {:?}, delta_file_name: {:?}",
    //         chout,
    //         delta_file_name
    //     );
    //     if let Ok(file) = sftp.open(Path::new(&delta_file_name)) {
    //         let mut delta_file = DeltaFileReader::<ssh2::File>::read_delta_stream(file)?;
    //         let restore_path = fs::OpenOptions::new()
    //             .create(true)
    //             .truncate(true)
    //             .write(true)
    //             .open(format!("{}.restore", local_file_path))?;
    //         trace!("restore_path: {:?}", restore_path);
    //         let old_file = fs::OpenOptions::new().read(true).open(&local_file_path)?;
    //         delta_file.restore_seekable(restore_path, old_file)?;
    //         self.update_local_file_from_restored(&local_file_path)?;
    //         Ok(FileItemProcessResult::Successed(
    //             0,
    //             local_file_path,
    //             SyncType::Rsync,
    //         ))
    //     } else {
    //         bail!("sftp open delta_file: {:?} failed.", delta_file_name);
    //     }
    // }

    // fn update_local_file_from_restored(&self, local_file_path: impl AsRef<str>) -> Result<(), failure::Error> {
    //     let old_tmp = format!("{}.old.tmp", local_file_path.as_ref());
    //     let old_tmp_path = Path::new(&old_tmp);
    //     if old_tmp_path.exists() {
    //         fs::remove_file(old_tmp_path)?;
    //     }
    //     fs::rename(local_file_path.as_ref(), &old_tmp)?;
    //     let restored = format!("{}.restore", local_file_path.as_ref());
    //     fs::rename(&restored, local_file_path.as_ref())?;
    //     if old_tmp_path.exists() {
    //         fs::remove_file(&old_tmp_path)?;
    //     }
    //     Ok(())
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_accesses::SqliteDbAccess;
    use crate::develope::tutil;
    use crate::log_util;
    use bzip2::write::{BzDecoder, BzEncoder};
    use bzip2::Compression;
    use indicatif::MultiProgress;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::server"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    fn load_server_yml() -> Server<SqliteConnectionManager, SqliteDbAccess> {
        Server::<SqliteConnectionManager, SqliteDbAccess>::load_from_yml(
            "data/servers",
            "data",
            "localhost.yml",
            None,
            None,
            None,
        )
        .unwrap()
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
    }

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log();
        let server = load_server_yml();
        assert_eq!(
            server.server_yml.directories[0].excludes,
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
    fn t_sync_dirs() -> Result<(), failure::Error> {
        log();
        let mut server = load_server_yml();
        let mb = Arc::new(MultiProgress::new());

        let mb1 = Arc::clone(&mb);
        let mb2 = Arc::clone(&mb);

        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            if let Err(err) = mb1.join() {
                warn!("join account failure. {:?}", err);
            }
        });

        server.multi_bar.replace(mb2);
        let stats = server.sync_dirs(true)?;

        info!("result {:?}", stats);
        t.join().unwrap();
        Ok(())
    }

    #[test]
    fn t_bzip2() -> Result<(), failure::Error> {
        log();
        let vc = ['c'; 10000];
        let s: String = vc.iter().collect();
        let test_dir = tutil::create_a_dir_and_a_file_with_content("a", &s)?;

        let out = test_dir.open_an_empty_file_for_write("xx.bzip2")?;

        let mut encoder = BzEncoder::new(out, Compression::Best);

        let mut read_in = test_dir.open_a_file_for_read("a")?;

        io::copy(&mut read_in, &mut encoder)?;
        encoder.try_finish()?;
        test_dir.assert_file_exists("xx.bzip2");
        let of = test_dir.get_file_path("xx.bzip2");
        info!(
            "len: {}, ratio: {:?}",
            of.metadata().expect("sg").len(),
            (encoder.total_out() as f64 / encoder.total_in() as f64) * 100_f64
        );

        let out = test_dir.open_an_empty_file_for_write("b.txt")?;

        let mut decoder = BzDecoder::new(out);

        let mut read_in = test_dir.open_a_file_for_read("xx.bzip2")?;

        io::copy(&mut read_in, &mut decoder)?;
        decoder.try_finish()?;

        assert_eq!(
            test_dir.get_file_path("b.txt").metadata()?.len(),
            10000,
            "len should equal to file before compress."
        );

        Ok(())
    }

    #[test]
    fn t_tar() -> Result<(), failure::Error> {
        use tar::{Archive, Builder};
        log();
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

    #[test]
    fn t_glob() -> Result<(), failure::Error> {
        log();
        let ptn1 = Pattern::new("a/")?;
        assert!(!ptn1.matches("xa/bc"));
        let ptn1 = Pattern::new("?a/*")?;
        assert!(ptn1.matches("xa/bc"));

        let ptn1 = Pattern::new("**/a/b/**")?;
        assert!(ptn1.matches("x/a/b/c"));

        let ptn1 = Pattern::new("**/c/a/**")?;
        let p1 = Path::new("xy/c/a/3.txt");

        assert!(ptn1.matches_path(p1));

        Ok(())
    }

    // #[test]
    // fn t_load_from_path_into_db() -> Result<(), failure::Error> {
    //     let mut one_dir = Directory {
    //         remote_dir: "fixtures/adir".to_string(),
    //         ..Directory::default()
    //     };
    //     let pool = tutil::create_sqlite_mem_pool();
    //     create_sqlite_database(pool.clone())?;
    //     load_remote_item_to_sqlite(&one_dir, pool, true)?;
    //     Ok(())
    // }

    #[test]
    fn t_from_path() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/adir".to_string(),
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 8);
        tutil::print_cursor_lines(&mut cur);

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/adir".to_string(),
            includes: vec!["**/fixtures/adir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.excludes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 2); // one dir line, one file line.

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/adir".to_string(),
            excludes: vec!["**/fixtures/adir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 7, "if exlude 1 file there should 7 left.");

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/adir".to_string(),
            excludes: vec!["**/Tomcat6/logs/**".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 7, "if exlude logs file there should 7 left.");

        Ok(())
    }
}
