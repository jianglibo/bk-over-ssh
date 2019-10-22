use super::{
    load_remote_item, load_remote_item_to_sqlite, rolling_files, string_path, AppRole, AuthMethod,
    Directory, FileItem, FileItemProcessResult, FileItemProcessResultStats, Indicator, MiniAppConf,
    PbProperties, ProgressWriter, PruneStrategy, RemoteFileItem, ScheduleItem, SyncType,
};
use crate::actions::{channel_util, copy_a_file_item, SyncDirReport};
use crate::db_accesses::{scheduler_util, DbAccess};
use bzip2::write::BzEncoder;
use bzip2::Compression;
use chrono::Local;
use indicatif::ProgressStyle;
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use ssh2;
use std::ffi::OsString;
use std::io::prelude::{BufRead, Read};
use std::marker::PhantomData;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use std::{fs, io, io::Seek};
use tar::Builder;

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum CompressionImpl {
    Bzip2,
}

#[derive(Deserialize, Serialize)]
pub struct RsyncConfig {
    pub window: usize,
    pub valve: u64,
    pub sig_ext: String,
    pub delta_ext: String,
}

#[derive(Deserialize, Serialize)]
pub struct ServerYml {
    pub id_rsa: String,
    pub id_rsa_pub: Option<String>,
    pub auth_method: AuthMethod,
    pub host: String,
    pub port: u16,
    pub rsync: RsyncConfig,
    pub remote_exec: String,
    pub file_list_file: String,
    pub remote_server_yml: String,
    pub username: String,
    pub password: String,
    pub directories: Vec<Directory>,
    pub prune_strategy: PruneStrategy,
    pub archive_prefix: String,
    pub archive_postfix: String,
    pub compress_archive: Option<CompressionImpl>,
    pub buf_len: usize,
    pub use_db: bool,
    pub skip_sha1: bool,
    pub sql_batch_size: usize,
    pub schedules: Vec<ScheduleItem>,
    pub exclude_by_sql: Vec<String>,
    #[serde(skip)]
    pub yml_location: Option<PathBuf>,
}

pub struct Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub server_yml: ServerYml,
    session: Option<ssh2::Session>,
    my_dir: PathBuf,
    reports_dir: PathBuf,
    archives_dir: PathBuf,
    working_dir: PathBuf,
    // directories_dir: PathBuf,
    pub yml_location: Option<PathBuf>,
    pub db_access: Option<D>,
    _m: PhantomData<M>,
    app_conf: MiniAppConf,
}

unsafe impl<M, D> Sync for Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
}

unsafe impl<M, D> Send for Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
}

impl<M, D> Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub fn new(
        app_conf: MiniAppConf,
        my_dir: PathBuf,
        mut server_yml: ServerYml,
    ) -> Result<Self, failure::Error> {
        let reports_dir = my_dir.join("reports");
        let archives_dir = my_dir.join("archives");
        let working_dir = my_dir.join("working");
        let directories_dir = my_dir.join("directories");

        if !archives_dir.exists() {
            fs::create_dir_all(&archives_dir).expect("archives_dir should create.");
        }
        if !reports_dir.exists() {
            fs::create_dir_all(&reports_dir).expect("reports_dir should create.");
        }
        if !working_dir.exists() {
            fs::create_dir_all(&working_dir).expect("working_dir should create.");
        }
        if !directories_dir.exists() {
            fs::create_dir_all(&directories_dir).expect("directories_dir should create.");
        }

        match app_conf.app_role {
            AppRole::PullHub => {
                server_yml.directories.iter_mut().try_for_each(|d| {
                    d.compile_patterns().expect("compile_patterns should succeeded.");
                    d.normalize_pull_hub_sync(directories_dir.as_path())
                })?;
            }
            AppRole::ActiveLeaf => {
                server_yml.directories.iter_mut().try_for_each(|d| {
                    d.compile_patterns().expect("compile_patterns should succeeded.");
                    d.normalize_active_leaf_sync(directories_dir.as_path())
                })?;
            }
            AppRole::PassiveLeaf => { // compiling patterns is enough.
                server_yml.directories.iter_mut().try_for_each(|d| {
                    d.compile_patterns()
                })?;
            }
            _ => {
                bail!("unexpected app_role: {:?}", app_conf.app_role);
            }
        }

        Ok(Self {
            server_yml,
            db_access: None,
            session: None,
            my_dir,
            reports_dir,
            archives_dir,
            working_dir,
            yml_location: None,
            app_conf,
            _m: PhantomData,
        })
    }

    pub fn set_db_access(&mut self, db_access: D) {
        // if let Err(err) = db_access.create_database() {
        //     warn!("create database failed: {:?}", err);
        // }
        self.db_access.replace(db_access);
    }

    pub fn get_db_file(&self) -> PathBuf {
        self.my_dir.join("db.db")
    }

    pub fn get_host(&self) -> &str {
        self.server_yml.host.as_str()
    }

    pub fn get_port(&self) -> u16 {
        self.server_yml.port
    }

    /// From the view of the server, it's an out direction.
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

    // pub fn pb_finish(&self) {
    //     self.pb.pb_finish();
    // }

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
        self.reports_dir.join("sync_dir_report.json")
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

    /// get archives_dir , archive_prefix, archive_postfix from server yml configuration file.
    fn next_archive_file(&self) -> PathBuf {
        rolling_files::get_next_file_name(
            &self.archives_dir,
            &self.server_yml.archive_prefix,
            &self.server_yml.archive_postfix,
        )
    }

    /// current tar file will not compress.
    fn current_archive_file_path(&self) -> PathBuf {
        self.archives_dir.join(format!(
            "{}{}",
            self.server_yml.archive_prefix, self.server_yml.archive_postfix,
        ))
    }

    fn archive_internal(&self, pb: &mut Indicator) -> Result<PathBuf, failure::Error> {
        let total_size = self.count_local_dirs_size();

        let style = ProgressStyle::default_bar()
            .template(
                "{spinner} {bytes_per_sec} {decimal_bytes:>8}/{decimal_total_bytes:8}  {percent:>4}% {wide_msg}",
            )
            .progress_chars("#-");

        pb.active_pb_total().alter_pb(PbProperties {
            set_style: Some(style),
            set_length: Some(total_size),
            ..PbProperties::default()
        });
        let cur_archive_path = self.current_archive_file_path();
        trace!("open file to write archive: {:?}", cur_archive_path);

        let writer_c = || {
            fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(cur_archive_path.as_path())
        };

        let writer: Box<dyn io::Write> = if let Some(ref sm) = self.server_yml.compress_archive {
            match sm {
                CompressionImpl::Bzip2 => {
                    let w = BzEncoder::new(writer_c()?, Compression::Best);
                    let w = ProgressWriter::new(w, pb);
                    Box::new(w)
                }
            }
        } else {
            let w = ProgressWriter::new(writer_c()?, pb);
            Box::new(w)
        };

        let mut archive = Builder::new(writer);

        for dir in self.server_yml.directories.iter() {
            let len = dir.count_total_size();
            let d_path = Path::new(&dir.local_dir);
            if let Some(d_path_name) = d_path.file_name() {
                if d_path.exists() {
                    pb.set_message_pb_total(format!(
                        "[{}] processing directory: {:?}",
                        self.get_host(),
                        d_path_name
                    ));
                    archive.append_dir_all(d_path_name, d_path)?;
                    pb.inc_pb_total(len);
                } else {
                    warn!("unexist directory: {:?}", d_path);
                }
            } else {
                error!("dir.local_dir get file_name failed: {:?}", d_path);
            }
        }
        archive.finish()?;
        Ok(cur_archive_path)
    }

    fn archive_out(&self, pb: &mut Indicator) -> Result<PathBuf, failure::Error> {
        let cur_archive_path = self.current_archive_file_path();
        let cur_archive_name = cur_archive_path.as_os_str();

        let style = ProgressStyle::default_bar().template("{spinner} {wide_msg}");

        pb.active_pb_total().alter_pb(PbProperties {
            set_style: Some(style),
            enable_steady_tick: Some(200),
            set_message: None,
            ..PbProperties::default()
        });

        for dir in self.server_yml.directories.iter() {
            pb.set_message(format!(
                "archive directory: {:?}, using out util: {}",
                dir.local_dir,
                self.app_conf.archive_cmd.get(0).unwrap()
            ));
            let archive_cmd = self
                .app_conf
                .archive_cmd
                .iter()
                .map(|s| {
                    if s == "archive_file_name" {
                        cur_archive_name.to_owned()
                    } else if s == "files_and_dirs" {
                        OsString::from(&dir.local_dir)
                    } else {
                        OsString::from(s)
                    }
                })
                .collect::<Vec<OsString>>();

            let output = if cfg!(target_os = "windows") {
                let mut c = Command::new("cmd");
                c.arg("/C");
                for seg in archive_cmd {
                    c.arg(seg);
                }
                c.output().expect("failed to execute process")
            } else {
                let mut c = Command::new("sh");
                c.arg("-c");
                for seg in archive_cmd {
                    c.arg(seg);
                }
                c.output().expect("failed to execute process")
            };
            trace!("archive_cmd output: {:?}", output);
        }
        Ok(cur_archive_path)
    }

    pub fn archive_local(&self, pb: &mut Indicator) -> Result<(), failure::Error> {
        if self.check_skip_cron("archive-local") {
            info!(
                "start archive_local on server: {} at: {}",
                self.get_host(),
                Local::now()
            );
            let cur = if self.app_conf.archive_cmd.is_empty() {
                self.archive_internal(pb)?
            } else {
                self.archive_out(pb)?
            };
            let nf = self.next_archive_file();
            trace!("move fie to {:?}", nf);
            fs::rename(cur, nf)?;
        }
        Ok(())
    }
    pub fn prune_backups(&self) -> Result<(), failure::Error> {
        rolling_files::do_prune_dir(
            &self.server_yml.prune_strategy,
            &self.archives_dir,
            &self.server_yml.archive_prefix,
            &self.server_yml.archive_postfix,
        )?;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub fn get_ssh_session(&self) -> &ssh2::Session {
        self.session.as_ref().expect("session should be created.")
    }

    fn create_ssh_session(&self) -> Result<ssh2::Session, failure::Error> {
        let url = format!("{}:{}", self.get_host(), self.get_port());
        trace!("connecting to: {}", url);
        let tcp = TcpStream::connect(&url)?;
        if let Ok(mut sess) = ssh2::Session::new() {
            sess.set_tcp_stream(tcp);
            sess.handshake()?;
            match self.server_yml.auth_method {
                AuthMethod::Agent => {
                    let mut agent = sess.agent()?;
                    agent.connect()?;
                    agent.list_identities()?;

                    for id in agent.identities() {
                        match id {
                            Ok(identity) => {
                                trace!("start authenticate with public key.");
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
                    )
                    .expect("userauth_pubkey_file should succeeded.");
                }
                AuthMethod::Password => {
                    sess.userauth_password(&self.server_yml.username, &self.server_yml.password)
                        .expect("userauth_password should succeeded.");
                }
            }
            Ok(sess)
        } else {
            bail!("Session::new failed.");
        }
    }

    pub fn connect(&mut self) -> Result<(), failure::Error> {
        if !self.is_connected() {
            let sess = self.create_ssh_session()?;
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

    pub fn is_skip_sha1(&self) -> bool {
        if !self.app_conf.skip_sha1 {
            // if force not to skip.
            false
        } else {
            self.server_yml.skip_sha1
        }
    }

    /// We temporarily save file list file at 'file_list_file' property of server.yml.
    pub fn list_remote_file_sftp(&self) -> Result<PathBuf, failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let app_role = match self.app_conf.app_role {
            AppRole::PullHub => AppRole::PassiveLeaf,
            _ => bail!("list_remote_file_sftp: unsupported app role. {:?}", self.app_conf.app_role),
        };
        let cmd = format!(
            "{} {} --app-role {} list-local-files {} --out {}",
            self.server_yml.remote_exec,
            if self.is_skip_sha1() {
                ""
            } else {
                "--enable-sha1"
            },
            app_role.to_str(),
            self.server_yml.remote_server_yml,
            self.server_yml.file_list_file,
        );
        trace!("invoking list remote files by sftp command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let out_op = channel_util::get_stdout_eprintln_stderr(&mut channel, true);

        trace!("exec output: {:?}", out_op);

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

    pub fn create_remote_db(
        &self,
        db_type: impl AsRef<str>,
        force: bool,
        server_yml: Option<&str>,
    ) -> Result<(), failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let db_type = db_type.as_ref();
        let cmd = format!(
            "{} create-db {} --db-type {}{}",
            self.server_yml.remote_exec,
            if let Some(server_yml) = server_yml {
                server_yml
            } else {
                ""
            },
            db_type,
            if force { " --force" } else { "" },
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let mut contents = String::new();
        if let Err(err) = channel.read_to_string(&mut contents) {
            bail!("is remote exec executable? {:?}", err);
        }
        if !contents.is_empty() {
            trace!("create_remote_db output: {:?}", contents);
        }
        contents.clear();
        channel.stderr().read_to_string(&mut contents)?;
        if !contents.is_empty() {
            trace!("create_remote_db stderr: {:?}", contents);
            eprintln!("create_remote_db stderr: {:?}", contents);
        }
        Ok(())
    }

    pub fn confirm_remote_sync(&self) -> Result<(), failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let app_role = match self.app_conf.app_role {
            AppRole::PullHub => AppRole::PassiveLeaf,
            _ => bail!("there is no need to confirm remote sync."),
        };
        let cmd = format!(
            "{} --app-role {} confirm-local-sync {}",
            self.server_yml.remote_exec,
            app_role.to_str(),
            self.server_yml.remote_server_yml,
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        channel_util::get_stdout_eprintln_stderr(&mut channel, true);
        Ok(())
    }

    pub fn confirm_local_sync(&self) -> Result<(), failure::Error> {
        let confirm_num = if let Some(db_access) = self.db_access.as_ref() {
            db_access.confirm_all()?
        } else {
            0
        };
        println!("{}", confirm_num);
        Ok(())
    }
    /// This method only used for command line list remote files. We use list_remote_file_sftp in actual sync task.
    pub fn list_remote_file_exec(&self, no_db: bool) -> Result<PathBuf, failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} {} --app-role {} {} list-local-files {}",
            self.server_yml.remote_exec,
            if self.is_skip_sha1() {
                ""
            } else {
                "--enable-sha1"
            },
            self.app_conf.app_role.to_str(),
            if no_db { " --no-db" } else { "" },
            self.server_yml.remote_server_yml,
        );
        info!("invoking list remote files command: {:?}", cmd);
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
    pub fn prepare_file_list(&self) -> Result<(), failure::Error> {
        if self.get_working_file_list_file().exists() {
            eprintln!(
                "uncompleted list file exists: {:?} continue processing",
                self.get_working_file_list_file()
            );
        } else {
            self.list_remote_file_sftp()?;
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
        pb: &mut Indicator,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        self.prepare_file_list()?;
        let working_file = &self.get_working_file_list_file();
        let rb = io::BufReader::new(fs::File::open(working_file)?);
        self.start_sync(rb, pb)
    }

    /// Do not try to return item stream from this function. consume it locally, pass in function to alter the behavior.
    fn start_sync<R: BufRead>(
        &self,
        file_item_lines: R,
        pb: &mut Indicator,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        let mut consume_count = 0u64;
        let count_and_len_op = if pb.is_some() {
            self.get_pb_count_and_len().ok()
        } else {
            None
        };
        let total_count = count_and_len_op.map(|cl| cl.0).unwrap_or_default();

        // if let Some((pb_total, pb_item)) = self.pb.as_ref() {
        // // let style = ProgressStyle::default_bar().template("[{eta_precise}] {prefix:.bold.dim} {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {spinner} {wide_msg}").progress_chars("#-");
        // let style = ProgressStyle::default_bar().template("{prefix} {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {percent}% {eta}").progress_chars("#-");
        // pb_total.set_length(count_and_len_op.map(|cl| cl.1).unwrap_or(0u64));
        // pb_total.set_style(style);

        //     let style = ProgressStyle::default_bar().template("{bytes_per_sec:10} {decimal_bytes:>8}/{decimal_total_bytes:8} {spinner} {percent:>4}% {eta:5} {wide_msg}").progress_chars("#-");
        //     pb_item.set_style(style);
        // }
        pb.active_pb_total().alter_pb(PbProperties {
            set_style: Some(ProgressStyle::default_bar().template("{prefix} {bytes_per_sec} {decimal_bytes}/{decimal_total_bytes} {bar:30.cyan/blue} {percent}% {eta}").progress_chars("#-")),
            set_length: count_and_len_op.map(|cl| cl.1),
            ..PbProperties::default()
        });

        pb.active_pb_item().alter_pb(PbProperties {
            set_style: Some(ProgressStyle::default_bar().template("{bytes_per_sec:10} {decimal_bytes:>8}/{decimal_total_bytes:8} {spinner} {percent:>4}% {eta:5} {wide_msg}").progress_chars("#-")),
            ..PbProperties::default()
        });

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
                    if let (Some(rd), Some(local_dir)) =
                        (current_remote_dir.as_ref(), current_local_dir)
                    {
                        match serde_json::from_str::<RemoteFileItem>(&line) {
                            Ok(remote_item) => {
                                let remote_len = remote_item.get_len();
                                let sync_type = if self.server_yml.rsync.valve > 0
                                    && remote_item.get_len() > self.server_yml.rsync.valve
                                {
                                    SyncType::Rsync
                                } else {
                                    SyncType::Sftp
                                };
                                let local_item = FileItem::new(
                                    local_dir,
                                    rd.as_str(),
                                    remote_item,
                                    sync_type,
                                    &self.app_conf.app_role,
                                );
                                consume_count += 1;

                                pb.active_pb_item().alter_pb(PbProperties {
                                    set_length: Some(remote_len),
                                    set_message: Some(
                                        local_item.get_remote_item().get_path().to_owned(),
                                    ),
                                    reset: true,
                                    ..PbProperties::default()
                                });

                                let mut skipped = false;
                                // if use_db all received item are changed.
                                // let r = if self.server_yml.use_db || local_item.had_changed() { // even use_db still check change or not.
                                let r = if local_item.had_changed() {
                                    trace!("file had changed. start copy_a_file_item.");
                                    copy_a_file_item(&self, &sftp, local_item, &mut buff, pb)
                                } else {
                                    skipped = true;
                                    FileItemProcessResult::Skipped(
                                        local_item.get_local_path_str().expect(
                                            "get_local_path_str should has some at this point.",
                                        ),
                                    )
                                };
                                if pb.is_some() {
                                    pb.active_pb_total().alter_pb(PbProperties {
                                        set_prefix: Some(format!(
                                            "[{}] {}/{} ",
                                            self.get_host(),
                                            total_count - consume_count,
                                            total_count
                                        )),
                                        inc: Some(remote_len),
                                        ..PbProperties::default()
                                    });
                                    if skipped {
                                        pb.active_pb_item().inc_pb_item(remote_len);
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
                    let found_directory = self
                        .server_yml
                        .directories
                        .iter()
                        .find(|d| string_path::path_equal(&d.remote_dir, &line));

                    if let Some(found_directory) = found_directory {
                        current_remote_dir = Some(line.clone());
                        current_local_dir = Some(Path::new(found_directory.local_dir.as_str()));
                        FileItemProcessResult::Directory(line)
                    } else {
                        // we compare the remote dir line with this server_yml.directories's remote dir
                        error!(
                            "this is line from remote: {:?}, this is all remote_dir in configuration file: {:?}, no one matches.",
                            line, self.server_yml
                                .directories
                                .iter()
                                .map(|d| d.local_dir.as_str())
                                .collect::<Vec<&str>>()
                        );
                        current_remote_dir = None;
                        current_local_dir = None;
                        FileItemProcessResult::NoCorrespondedLocalDir(line)
                    }
                }
            })
            .fold(FileItemProcessResultStats::default(), |mut accu, item| {
                match item {
                    FileItemProcessResult::DeserializeFailed(_) => accu.deserialize_failed += 1,
                    FileItemProcessResult::Skipped(_) => accu.skipped += 1,
                    FileItemProcessResult::NoCorrespondedLocalDir(_) => {
                        accu.no_corresponded_local_dir += 1
                    }
                    FileItemProcessResult::Directory(_) => accu.directory += 1,
                    FileItemProcessResult::LengthNotMatch(_) => accu.length_not_match += 1,
                    FileItemProcessResult::Sha1NotMatch(_) => accu.sha1_not_match += 1,
                    FileItemProcessResult::CopyFailed(_) => accu.copy_failed += 1,
                    FileItemProcessResult::SkipBecauseNoBaseDir => {
                        accu.skip_because_no_base_dir += 1
                    }
                    FileItemProcessResult::Succeeded(fl, _, _) => {
                        accu.bytes_transferred += fl;
                        accu.succeeded += 1;
                    }
                    FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
                    FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
                    FileItemProcessResult::ScpOpenFailed => accu.scp_open_failed += 1,
                };
                accu
            });
        Ok(result)
    }

    pub fn count_local_dirs_size(&self) -> u64 {
        self.server_yml
            .directories
            .iter()
            .map(|d| d.count_total_size())
            .sum()
    }

    fn check_skip_cron(&self, cron_name: &str) -> bool {
        self.app_conf.skip_cron
            || if let Some(si) = self
                .server_yml
                .schedules
                .iter()
                .find(|it| it.name == cron_name)
            {
                match scheduler_util::need_execute(
                    self.db_access.as_ref(),
                    self.yml_location.as_ref().unwrap().to_str().unwrap(),
                    &si.name,
                    &si.cron,
                ) {
                    (true, None) => true,
                    (false, Some(dt)) => {
                        eprintln!(
                            "cron time didn't meet yet. next execution scheduled at: {:?}",
                            dt
                        );
                        false
                    }
                    (_, _) => false,
                }
            } else {
                error!("Can't find cron item with name: {}", cron_name);
                false
            }
    }

    pub fn sync_pull_dirs(
        &self,
        pb: &mut Indicator,
    ) -> Result<Option<SyncDirReport>, failure::Error> {
        if self.check_skip_cron("sync-pull-dirs") {
            info!(
                "start sync_pull_dirs on server: {} at: {}",
                self.get_host(),
                Local::now()
            );
            let start = Instant::now();
            let started_at = Local::now();
            let rs = self.start_sync_working_file_list(pb)?;
            self.remove_working_file_list_file();
            self.confirm_remote_sync()?;
            Ok(Some(SyncDirReport::new(start.elapsed(), started_at, rs)))
        } else {
            Ok(None)
        }
    }

    pub fn load_dirs<O: io::Write>(&self, out: &mut O) -> Result<(), failure::Error> {
        if self.db_access.is_some() && self.server_yml.use_db {
            let db_access = self.db_access.as_ref().unwrap();
            for one_dir in self.server_yml.directories.iter() {
                trace!("start load directory: {:?}", one_dir);
                load_remote_item_to_sqlite(
                    one_dir,
                    db_access,
                    self.is_skip_sha1(),
                    self.server_yml.sql_batch_size,
                    self.server_yml.rsync.sig_ext.as_str(),
                    self.server_yml.rsync.delta_ext.as_str(),
                )?;
                trace!("load_remote_item_to_sqlite done.");
                for sql in self.server_yml.exclude_by_sql.iter() {
                    db_access.exclude_by_sql(sql)?;
                }
                trace!("exclude_by_sql done.");
                db_access.iterate_files_by_directory_changed_or_unconfirmed(|fi_db_or_path| {
                    match fi_db_or_path {
                        (Some(fi_db), None) => {
                            if fi_db.changed || !fi_db.confirmed {
                                match serde_json::to_string(&RemoteFileItem::from(fi_db)) {
                                    Ok(line) => {
                                        writeln!(out, "{}", line).ok();
                                    }
                                    Err(err) => error!("serialize item line failed: {:?}", err),
                                }
                            }
                        }
                        (None, Some(path)) => {
                            if let Err(err) = writeln!(out, "{}", path) {
                                error!("write path failed: {:?}", err);
                            }
                        }
                        _ => {}
                    }
                })?;
            }
        } else {
            for one_dir in self.server_yml.directories.iter() {
                load_remote_item(one_dir, out, self.is_skip_sha1())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_accesses::{DbAccess, SqliteDbAccess};
    use crate::develope::tutil;
    use crate::log_util;
    use bzip2::write::{BzDecoder, BzEncoder};
    use bzip2::Compression;
    use glob::Pattern;
    // use indicatif::MultiProgress;
    use std::fs;
    use std::io::{self};
    // use std::sync::Arc;
    // use std::thread;
    // use std::time::Duration;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec![
                "data_shape::server",
                "data_shape::app_conf",
                "action::copy_file",
            ],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log();
        let app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        let server = tutil::load_demo_server_sqlite(&app_conf, None);
        assert_eq!(
            server.server_yml.directories[0].excludes,
            vec!["*.log".to_string(), "*.bak".to_string()]
        );
        Ok(())
    }

    #[test]
    fn t_connect_server() -> Result<(), failure::Error> {
        log();
        let app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        let mut server = tutil::load_demo_server_sqlite(&app_conf, None);
        info!("start connecting...");
        server.connect()?;
        assert!(server.is_connected());
        Ok(())
    }

    /// pull downed files were saved in the ./data/pull-servers-data directory.
    /// remote generated file_list_file was saved in the 'file_list_file' property of server.yml point to.
    /// This test also involved compiled executable so remember to compile the app if result is not expected.
    #[test]
    fn t_sync_pull_dirs() -> Result<(), failure::Error> {
        log();
        let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        app_conf.mini_app_conf.skip_cron = true;

        assert!(app_conf.mini_app_conf.app_role == AppRole::PullHub);
        let mut server = tutil::load_demo_server_sqlite(&app_conf, None);

        // it's useless. because the db file is from remote server's perspective.
        let db_file = server.get_db_file();
        if db_file.exists() {
            fs::remove_file(db_file.as_path())?;
        }

        let remote_db_path = Path::new("./target/debug/data/passive-leaf-data/127.0.0.1/db.db");

        if remote_db_path.exists() {
            fs::remove_file(remote_db_path)?;
        }

        let sqlite_db_access = SqliteDbAccess::new(db_file);
        sqlite_db_access.create_database()?;
        server.set_db_access(sqlite_db_access);

        let a_dir_string = server
            .server_yml
            .directories
            .iter()
            .find(|d| d.remote_dir.ends_with("a-dir"))
            .expect("should have a directory who's remote_dir end with 'a-dir'")
            .local_dir
            .clone();
        let a_dir = Path::new(a_dir_string.as_str());
        if a_dir.exists() {
            info!("remove directory: {:?}", a_dir);
            fs::remove_dir_all(a_dir)?;
        }

        // let mb = Arc::new(MultiProgress::new());

        // let mb1 = Arc::clone(&mb);
        // let mb2 = Arc::clone(&mb);

        // let t = thread::spawn(move || {
        //     thread::sleep(Duration::from_millis(200));
        //     if let Err(err) = mb1.join() {
        //         warn!("join account failure. {:?}", err);
        //     }
        // });
        // let mut indicator = Indicator::new(Some(mb2));
        let mut indicator = Indicator::new(None);
        server.connect()?;
        let stats = server.sync_pull_dirs(&mut indicator)?;
        indicator.pb_finish();
        info!("result {:?}", stats);
        info!("a_dir is {:?}", a_dir);
        let cc_txt = a_dir.join("b").join("c c").join("c c .txt");
        info!("cc_txt is {:?}", cc_txt);
        assert!(cc_txt.exists());
        assert!(a_dir.join("b").join("b.txt").exists());
        assert!(a_dir.join("b b").join("b b.txt").exists());
        assert!(a_dir.join("a.txt").exists());
        assert!(a_dir.join("qrcode.png").exists());
        // t.join().unwrap();
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

    #[test]
    fn t_from_path() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/a-dir".to_string(),
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 8);
        tutil::print_cursor_lines(&mut cur);

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/a-dir".to_string(),
            includes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.excludes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 2); // one dir line, one file line.

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/a-dir".to_string(),
            excludes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 7, "if exclude 1 file there should 7 left.");

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: "fixtures/a-dir".to_string(),
            excludes: vec!["**/Tomcat6/logs/**".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        load_remote_item(&one_dir, &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 7, "if exclude logs file there should 7 left.");

        Ok(())
    }
    #[test]
    fn t_main_password() {
        // Connect to the local SSH server
        let tcp = TcpStream::connect("127.0.0.1:22").unwrap();
        let mut sess = ssh2::Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake().unwrap();

        sess.userauth_password("Administrator", "pass.")
            .expect("should authenticate succeeded.");
        assert!(sess.authenticated(), "should authenticate succeeded.");
    }
}
