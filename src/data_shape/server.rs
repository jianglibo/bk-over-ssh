use super::{
    app_conf, rolling_files, AppRole, AuthMethod, Directory, FileItemDirectories, FileItemMap,
    FileItemProcessResult, FileItemProcessResultStats, Indicator, MiniAppConf, PbProperties,
    PrimaryFileItem, ProgressWriter, PruneStrategy, RelativeFileItem, ScheduleItem, SlashPath,
    SyncType, ClientPushProgressBar,
};
use crate::actions::{copy_a_file_item, copy_a_file_sftp, copy_file, ssh_util, SyncDirReport};
use crate::db_accesses::{scheduler_util, DbAccess};
use crate::protocol::{MessageHub, SshChannelMessageHub, StringMessage, TransferType, U64Message};
use base64;
use bzip2::write::BzEncoder;
use bzip2::Compression;
use chrono::Local;
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use ssh2;
use std::ffi::OsString;
use std::io::prelude::{BufRead, Read};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use std::{fs, io, io::Seek, io::Write};
use tar::Builder;
use indicatif::{ProgressStyle};

pub const CRON_NAME_SYNC_PULL_DIRS: &str = "sync-pull-dirs";
pub const CRON_NAME_SYNC_PUSH_DIRS: &str = "sync-push-dirs";

const FILE_LIST_FILE_NAME: &str = "file_list_file.txt";

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
}

fn accumulate_file_process(
    mut accu: FileItemProcessResultStats,
    item: FileItemProcessResult,
) -> FileItemProcessResultStats {
    match item {
        FileItemProcessResult::DeserializeFailed(_) => accu.deserialize_failed += 1,
        FileItemProcessResult::Skipped(_) => accu.skipped += 1,
        FileItemProcessResult::NoCorrespondedLocalDir(_) => accu.no_corresponded_local_dir += 1,
        FileItemProcessResult::Directory(_) => accu.directory += 1,
        FileItemProcessResult::LengthNotMatch(_) => accu.length_not_match += 1,
        FileItemProcessResult::Sha1NotMatch(_) => accu.sha1_not_match += 1,
        FileItemProcessResult::CopyFailed(_) => accu.copy_failed += 1,
        FileItemProcessResult::SkipBecauseNoBaseDir => accu.skip_because_no_base_dir += 1,
        FileItemProcessResult::Succeeded(fl, _, _) => {
            accu.bytes_transferred += fl;
            accu.succeeded += 1;
        }
        FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
        FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
        FileItemProcessResult::ScpOpenFailed => accu.scp_open_failed += 1,
        FileItemProcessResult::MayBeNoParentDir(_) => (),
    };
    accu
}

pub fn push_a_file_item_sftp(
    sftp: &ssh2::Sftp,
    file_item_primary: PrimaryFileItem,
    buf: &mut [u8],
    progress_bar: &mut Indicator,
) -> FileItemProcessResult {
    progress_bar.init_item_pb_style_1(
        file_item_primary.get_local_path().as_str(),
        file_item_primary.relative_item.get_len(),
    );
    trace!(
        "staring create remote file: {}.",
        file_item_primary.get_remote_path()
    );
    match sftp.create(file_item_primary.get_remote_path().as_path()) {
        Ok(mut ssh_file) => {
            let local_file_path = file_item_primary.get_local_path();
            trace!(
                "coping {} to {}.",
                local_file_path,
                file_item_primary.get_remote_path()
            );
            match local_file_path.get_local_file_reader() {
                Ok(mut local_reader) => {
                    match copy_file::copy_stream_with_pb(
                        &mut local_reader,
                        &mut ssh_file,
                        buf,
                        progress_bar,
                    ) {
                        Ok(length) => {
                            if length != file_item_primary.get_relative_item().get_len() {
                                FileItemProcessResult::LengthNotMatch(local_file_path.get_slash())
                            } else {
                                FileItemProcessResult::Succeeded(
                                    length,
                                    local_file_path.get_slash(),
                                    SyncType::Sftp,
                                )
                            }
                        }
                        Err(err) => {
                            error!("write_stream_to_file failed: {:?}", err);
                            FileItemProcessResult::CopyFailed(local_file_path.get_slash())
                        }
                    }
                }
                Err(_err) => FileItemProcessResult::GetLocalPathFailed,
            }
        }
        Err(err) => {
            error!("sftp create failed: {:?}", err);
            if err.code() == 2 {
                error!("sftp create failed return code 2.");
                FileItemProcessResult::MayBeNoParentDir(file_item_primary)
            } else {
                FileItemProcessResult::SftpOpenFailed
            }
        }
    }
}

pub struct Server<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub server_yml: ServerYml,
    session: Option<ssh2::Session>,
    // for passive_leaf node, it's dependent on invoking parameter of app_instance_id.
    my_dir: PathBuf,
    reports_dir: PathBuf,
    archives_dir: PathBuf,
    working_dir: PathBuf,
    pub yml_location: Option<PathBuf>,
    pub db_access: Option<D>,
    _m: PhantomData<M>,
    app_conf: MiniAppConf,
    lock_file: Option<fs::File>,
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
    /// Server's my_dir is composed of the 'data' dir and the host name of the server.
    /// But for passive_leaf which host name it is?
    /// We must use command line app_instance_id to override the value in the app_conf_yml,
    /// Then use app_instance_id as my_dir.
    pub fn new(
        app_conf: MiniAppConf,
        my_dir: PathBuf,
        mut server_yml: ServerYml,
    ) -> Result<Self, failure::Error> {
        let reports_dir = my_dir.join("reports");
        let archives_dir = my_dir.join("archives");
        let working_dir = my_dir.join("working");
        let directories_dir = my_dir.join("directories");

        if !my_dir.exists() {
            fs::create_dir_all(&my_dir).expect("my_dir should create.");
        }

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

        server_yml.directories.iter_mut().for_each(|d| {
            d.compile_patterns()
                .expect("compile_patterns should succeeded.")
        });

        if let Some(app_role) = app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => {
                    server_yml
                        .directories
                        .iter_mut()
                        .try_for_each(|d| d.normalize_pull_hub_sync(directories_dir.as_path()))?;
                }
                AppRole::ActiveLeaf => {
                    let remote_home = server_yml.remote_exec.as_str();
                    server_yml.directories.iter_mut().try_for_each(|d| {
                        d.normalize_active_leaf_sync(
                            directories_dir.as_path(),
                            app_conf.app_instance_id.as_str(),
                            remote_home,
                        )
                    })?;
                }
                AppRole::ReceiveHub => {
                    server_yml.directories.iter_mut().try_for_each(|d| {
                        d.normalize_receive_hub_sync(directories_dir.as_path())
                    })?;
                }
                _ => (),
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
            lock_file: None,
        })
    }
    /// Lock the server, preventing server from concurrently executing.
    pub fn lock_working_file(&mut self) -> Result<(), failure::Error> {
        let lof = self.working_dir.join("working.lock");
        trace!("start locking file: {:?}", lof);
        if lof.exists() {
            if fs::remove_file(lof.as_path()).is_err() {
                eprintln!("create lock file failed: {:?}, if you can sure app isn't running, you can delete it manually.", lof);
            }
        } else {
            self.lock_file
                .replace(fs::OpenOptions::new().write(true).create(true).open(&lof)?);
        }
        trace!("locked!");
        Ok(())
    }

    pub fn count_local_files(&self) -> u64 {
        self.server_yml
            .directories
            .iter()
            .filter_map(|dir| dir.count_local_files(self.app_conf.app_role.as_ref()).ok())
            .count() as u64
    }

    pub fn count_local_dir_files(&self) -> u64 {
        self.server_yml
            .directories
            .iter()
            .map(|dir| dir.count_local_dir_files())
            .sum()
    }
    /// For app_role is ReceiveHub, remote exec is from user's home directory.
    pub fn get_remote_exec(&self) -> SlashPath {
        SlashPath::new(&self.server_yml.remote_exec)
    }

    pub fn count_remote_files(&self) -> Result<u64, failure::Error> {
        let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::ActiveLeaf => AppRole::ReceiveHub,
                _ => bail!(
                    "create_remote_files: unsupported app role. {:?}",
                    self.app_conf.app_role
                ),
            }
        } else {
            bail!("no app_role whne count_remote_files");
        };
        let mut channel: ssh2::Channel = self.create_channel()?;
        let cmd = format!(
            "{} {} {} --app-instance-id {} --app-role {}  count-local-files {}",
            self.get_remote_exec(),
            if self.app_conf.console_log {
                "--console-log"
            } else {
                ""
            },
            if self.app_conf.verbose { "--vv" } else { "" },
            self.app_conf.app_instance_id,
            app_role.to_str(),
            self.get_remote_server_yml(),
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let ss = ssh_util::get_stdout_eprintln_stderr(&mut channel, true);
        Ok(ssh_util::parse_scalar_value(ss)
            .unwrap_or_else(|| "0".to_string())
            .parse()?)
    }

    /// The passive leaf side of the application will try to find the configuration in the passive-leaf-conf folder which located in the same folder as the executable.
    /// The server yml file should located in the data/passive-leaf-conf/{self.app_conf.app_instance_id}.yml
    pub fn get_remote_server_yml(&self) -> String {
        let conf_folder = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => app_conf::PASSIVE_LEAF_CONF,
                AppRole::ActiveLeaf => app_conf::RECEIVE_SERVERS_CONF,
                _ => panic!(
                    "get_remote_server_yml got unsupported app role. {:?}",
                    self.app_conf.app_role
                ),
            }
        } else {
            panic!("no app_role when get_remote_server_yml");
        };
        let yml = format!(
            "/data/{}/{}.yml",
            conf_folder, self.app_conf.app_instance_id
        );
        self.get_remote_exec()
            .parent()
            .expect("the remote executable's parent directory should exist")
            .join(yml)
            .slash
    }

    /// located in the data/passive-leaf-data/{self.app_conf.app_instance_id}/file_list_file.txt
    pub fn get_passive_leaf_file_list_file(&self) -> String {
        let yml = format!(
            "/data/{}/{}/{}",
            app_conf::PASSIVE_LEAF_DATA,
            self.app_conf.app_instance_id,
            FILE_LIST_FILE_NAME,
        );
        self.get_remote_exec()
            .parent()
            .expect("the remote executable's parent directory should exist")
            .join(yml)
            .slash
    }

    pub fn get_active_leaf_file_list_file(&self) -> PathBuf {
        self.my_dir.join(FILE_LIST_FILE_NAME)
    }

    pub fn set_db_access(&mut self, db_access: D) {
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
        &self,
        local: impl AsRef<str>,
        remote: impl AsRef<str>,
    ) -> Result<(), failure::Error> {
        let sftp = self.session.as_ref().expect("is ssh connected?").sftp()?;
        Ok(copy_a_file_sftp(&sftp, local, remote)?)
    }

    pub fn dir_equals(&self, directories: &[Directory]) -> bool {
        let ss: Vec<&SlashPath> = self
            .server_yml
            .directories
            .iter()
            .map(|d| &d.remote_dir)
            .collect();
        let ass: Vec<&SlashPath> = directories.iter().map(|d| &d.remote_dir).collect();
        ss == ass
    }

    pub fn stats_remote_exec(&mut self) -> Result<ssh2::FileStat, failure::Error> {
        self.get_server_file_stats(self.get_remote_exec().as_str())
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
            let d_path = &dir.local_dir.as_path();
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
                        dir.local_dir.get_os_string()
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

    /// Archive need not to schedule standalone.
    /// Because of conflict with the sync operation.
    pub fn archive_local(&self, pb: &mut Indicator) -> Result<(), failure::Error> {
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
        let username = self.server_yml.username.as_str();
        match self.server_yml.auth_method {
            AuthMethod::Agent => ssh_util::create_ssh_session_agent(url.as_str(), username),
            AuthMethod::IdentityFile => ssh_util::create_ssh_session_identity_file(
                url.as_str(),
                username,
                self.server_yml.id_rsa.as_str(),
                self.server_yml.id_rsa_pub.as_ref().map(|ds| ds.as_str()),
            ),
            AuthMethod::Password => ssh_util::create_ssh_session_password(
                url.as_str(),
                username,
                self.server_yml.password.as_str(),
            ),
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
        let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => AppRole::PassiveLeaf,
                _ => bail!(
                    "list_remote_file_sftp: unsupported app role. {:?}",
                    self.app_conf.app_role
                ),
            }
        } else {
            bail!("no app_role when list_remote_file_sftp");
        };
        let cmd = format!(
            "{} {} --app-instance-id {} --app-role {} list-local-files {} --out {}",
            self.get_remote_exec(),
            if self.is_skip_sha1() {
                ""
            } else {
                "--enable-sha1"
            },
            self.app_conf.app_instance_id,
            app_role.to_str(),
            self.get_remote_server_yml(),
            self.get_passive_leaf_file_list_file(),
        );
        trace!("invoking list remote files by sftp command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        let (std_out, std_err) =
            ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);

        let sftp = self.session.as_ref().unwrap().sftp()?;

        if std_err.find("server yml file doesn").is_some()
            || std_out.find("server yml file doesn").is_some()
        {
            // now copy server yml to remote.
            let yml_location = self
                .yml_location
                .as_ref()
                .expect("yml_location should exist")
                .to_str()
                .expect("yml_location to_str should succeeded.");
            if let Err(err) = copy_a_file_sftp(&sftp, yml_location, self.get_remote_server_yml()) {
                bail!("sftp copy failed: {:?}", err);
            }

            // execute cmd again.
            let mut channel: ssh2::Channel = self.create_channel()?;
            channel.exec(cmd.as_str())?;
            ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
        }

        let mut f = sftp.open(Path::new(&self.get_passive_leaf_file_list_file().as_str()))?;

        let working_file = self.get_working_file_list_file();
        let mut wf = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&working_file)?;
        io::copy(&mut f, &mut wf)?;
        Ok(working_file)
    }

    pub fn create_remote_dir(&self, dir: &str) -> Result<(), failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let dir = base64::encode(dir);
        let cmd = format!(
            "{} {} {} mkdir {}",
            self.get_remote_exec(),
            if self.app_conf.console_log {
                "--console-log"
            } else {
                ""
            },
            if self.app_conf.verbose { "--vv" } else { "" },
            dir,
        );
        info!("invoking remote command: {}", cmd);
        channel.exec(cmd.as_str())?;
        ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
        Ok(())
    }

    pub fn create_remote_db(
        &self,
        db_type: impl AsRef<str>,
        force: bool,
    ) -> Result<(), failure::Error> {
        let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => AppRole::PassiveLeaf,
                _ => bail!(
                    "create_remote_db: unsupported app role. {:?}",
                    self.app_conf.app_role
                ),
            }
        } else {
            bail!("no app_role when create_remote_db");
        };
        let mut channel: ssh2::Channel = self.create_channel()?;
        let db_type = db_type.as_ref();
        let cmd = format!(
            "{} {} {} --app-instance-id {} --app-role {}  create-db {} --db-type {}{}",
            self.get_remote_exec(),
            if self.app_conf.console_log {
                "--console-log"
            } else {
                ""
            },
            if self.app_conf.verbose { "--vv" } else { "" },
            self.app_conf.app_instance_id,
            app_role.to_str(),
            self.get_remote_server_yml(),
            db_type,
            if force { " --force" } else { "" },
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
        Ok(())
    }

    pub fn confirm_remote_sync(&self) -> Result<(), failure::Error> {
        let mut channel: ssh2::Channel = self.create_channel()?;
        let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => AppRole::PassiveLeaf,
                _ => bail!("there is no need to confirm remote sync."),
            }
        } else {
            bail!("no app_role when confirm_remote_sync");
        };
        let cmd = format!(
            "{} --app-instance-id {} --app-role {} confirm-local-sync {}",
            self.get_remote_exec(),
            self.app_conf.app_instance_id,
            app_role.to_str(),
            self.get_remote_server_yml(),
        );
        info!("invoking remote command: {:?}", cmd);
        channel.exec(cmd.as_str())?;
        ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
        Ok(())
    }

    pub fn confirm_local_sync(&self) -> Result<(), failure::Error> {
        trace!("confirm sync, db file is: {:?}", self.get_db_file());
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
            "{} {} --app-instance-id {} --app-role {} {} list-local-files {}",
            self.get_remote_exec(),
            if self.is_skip_sha1() {
                ""
            } else {
                "--enable-sha1"
            },
            self.app_conf.app_instance_id,
            self.app_conf
                .app_role
                .as_ref()
                .unwrap_or(&AppRole::PullHub)
                .to_str(),
            if no_db { " --no-db" } else { "" },
            self.get_remote_server_yml(),
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
                        match serde_json::from_str::<RelativeFileItem>(&buf) {
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

    fn init_total_progress_bar(
        &self,
        progress_bar: &mut Indicator,
        file_list_file: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let count_and_len_op = if progress_bar.is_some() {
            let mut wfb = io::BufReader::new(fs::File::open(file_list_file.as_ref())?);
            Some(self.count_and_len(&mut wfb))
        } else {
            None
        };
        let total_count = count_and_len_op.map(|cl| cl.0).unwrap_or_default();
        progress_bar.count_total = total_count;

        progress_bar.active_pb_total().alter_pb(PbProperties {
            set_style: Some(ProgressStyle::default_bar().template("{prefix} {bytes_per_sec: 11} {decimal_bytes:>11}/{decimal_total_bytes} {bar:30.cyan/blue} {percent}% {eta}").progress_chars("#-")),
            set_length: count_and_len_op.map(|cl| cl.1),
            ..PbProperties::default()
        });
        progress_bar.active_pb_item().alter_pb(PbProperties {
            set_style: Some(ProgressStyle::default_bar().template("{bytes_per_sec:10} {decimal_bytes:>8}/{decimal_total_bytes:8} {spinner} {percent:>4}% {eta:5} {wide_msg}").progress_chars("#-")),
            ..PbProperties::default()
        });
        Ok(())
    }

    fn start_pull_sync_working_file_list(
        &self,
        pb: &mut Indicator,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        self.prepare_file_list()?;
        let working_file = &self.get_working_file_list_file();
        let rb = io::BufReader::new(fs::File::open(working_file)?);
        self.start_pull_sync(rb, pb)
    }

    fn get_local_remote_pairs(&self) -> Vec<(SlashPath, SlashPath)> {
        self.server_yml
            .directories
            .iter()
            .map(|d| (d.local_dir.clone(), d.remote_dir.clone()))
            .collect()
    }
    /// First list changed file to file_list_file.
    /// Then for each line in file_list_file
    fn start_push_sync_working_file_list(
        &self,
        progress_bar: &mut Indicator,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        let file_list_file = self.get_active_leaf_file_list_file();

        {
            info!("start creating file_list_file: {:?}", file_list_file);
            let mut o = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(file_list_file.as_path())
                .expect("file list file should be created.");
            self.create_file_list_files(&mut o)?;
        }

        self.init_total_progress_bar(progress_bar, file_list_file.as_path())?;

        info!("start reading file_list_file: {:?}", file_list_file);
        let reader = fs::OpenOptions::new()
            .read(true)
            .open(file_list_file.as_path())?;

        let file_item_directories =
            FileItemDirectories::<io::BufReader<fs::File>>::from_file_reader(
                reader,
                self.get_local_remote_pairs(),
                AppRole::ActiveLeaf,
            );

        let sftp = self.session.as_ref().unwrap().sftp()?;
        let mut buff = vec![0_u8; self.server_yml.buf_len];

        let mut result = FileItemProcessResultStats::default();
        for file_item_dir in file_item_directories {
            result += file_item_dir
                .map(|item| {
                    let file_len = item.get_relative_item().get_len();
                    let push_result = push_a_file_item_sftp(&sftp, item, &mut buff, progress_bar);
                    progress_bar.tick_total_pb_style_1(self.get_host(), file_len);
                    if let FileItemProcessResult::MayBeNoParentDir(item) = push_result {
                        match self.create_remote_dir(
                            item.get_remote_path()
                                .parent()
                                .expect("slash path's parent directory should exist.")
                                .as_str(),
                        ) {
                            Ok(_) => {
                                info!("push_a_file_item_sftp again.");
                                push_a_file_item_sftp(&sftp, item, &mut buff, progress_bar)
                            }
                            Err(err) => {
                                error!("create_remote_dir failed: {:?}", err);
                                FileItemProcessResult::SftpOpenFailed
                            }
                        }
                    } else {
                        push_result
                    }
                })
                .fold(
                    FileItemProcessResultStats::default(),
                    accumulate_file_process,
                );
        }
        Ok(result)
    }

    /// Do not try to return item stream from this function.
    /// consume it locally, pass in function to alter the behavior.
    ///
    /// Take a reader as parameter, each line may be a directory name or a file name.
    /// the file names are relative to last read directory line.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    ///
    ///
    /// ```
    fn start_pull_sync<R: BufRead>(
        &self,
        file_item_lines: R,
        progress_bar: &mut Indicator,
    ) -> Result<FileItemProcessResultStats, failure::Error> {
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;

        self.init_total_progress_bar(progress_bar, self.get_working_file_list_file())?;

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
            })
            .map(|line| {
                if line.starts_with('{') {
                    trace!("got item line {}", line);
                    if let (Some(remote_dir), Some(local_dir)) =
                        (current_remote_dir.as_ref(), current_local_dir)
                    {
                        match serde_json::from_str::<RelativeFileItem>(&line) {
                            Ok(remote_item) => {
                                let remote_len = remote_item.get_len();
                                let sync_type = if self.server_yml.rsync.valve > 0
                                    && remote_item.get_len() > self.server_yml.rsync.valve
                                {
                                    SyncType::Rsync
                                } else {
                                    SyncType::Sftp
                                };
                                let file_item_map = FileItemMap::new(
                                    local_dir,
                                    remote_dir.as_str(),
                                    remote_item,
                                    sync_type,
                                    true,
                                );

                                progress_bar.init_item_pb_style_1(file_item_map.get_relative_item().get_path(), remote_len);

                                let mut skipped = false;
                                // if use_db all received item are changed.
                                // let r = if self.server_yml.use_db || local_item.had_changed() { // even use_db still check change or not.
                                let r = if file_item_map.had_changed() {
                                    trace!("file had changed. start copy_a_file_item.");
                                    copy_a_file_item(&self, &sftp, file_item_map, &mut buff, progress_bar)
                                } else {
                                    skipped = true;
                                    FileItemProcessResult::Skipped(
                                        file_item_map.get_local_path_str().expect(
                                            "get_local_path_str should has some at this point.",
                                        ),
                                    )
                                };

                                progress_bar.tick_total_pb_style_1(self.get_host(), remote_len);
                                    if skipped {
                                        progress_bar.active_pb_item().inc_pb_item(remote_len);
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
                        .find(|d| d.remote_dir.slash_equal_to(&line));

                    if let Some(found_directory) = found_directory {
                        current_remote_dir = Some(line.clone());
                        current_local_dir = Some(found_directory.local_dir.as_path());
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
            .fold(FileItemProcessResultStats::default(), accumulate_file_process);
        Ok(result)
    }

    pub fn count_local_dirs_size(&self) -> u64 {
        self.server_yml
            .directories
            .iter()
            .map(|d| d.count_total_size())
            .sum()
    }

    pub fn find_cron_by_name(&self, cron_name: &str) -> Option<ScheduleItem> {
        self.server_yml
            .schedules
            .iter()
            .find(|it| it.name.as_str() == cron_name)
            .cloned()
    }

    fn check_skip_cron(&self, cron_name: &str) -> bool {
        self.app_conf.skip_cron
            || if let Some(si) = self.find_cron_by_name(cron_name) {
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

    /// We can push files to multiple destinations simultaneously.
    pub fn sync_push_dirs(
        &self,
        progress_bar: &mut Indicator,
    ) -> Result<Option<SyncDirReport>, failure::Error> {
        if self.check_skip_cron("sync-push-dirs") {
            info!(
                "start sync_push_dirs on server: {} at: {}",
                self.get_host(),
                Local::now()
            );
            let start = Instant::now();
            let started_at = Local::now();

            let rs = self.start_push_sync_working_file_list(progress_bar)?;
            self.confirm_local_sync()?;
            Ok(Some(SyncDirReport::new(start.elapsed(), started_at, rs)))
        } else {
            Ok(None)
        }
    }
    /// If as_service is true, one must connect to server first, and then after executing task close the connection.
    pub fn sync_pull_dirs(
        &mut self,
        pb: &mut Indicator,
        as_service: bool,
    ) -> Result<Option<SyncDirReport>, failure::Error> {
        if as_service || self.check_skip_cron(CRON_NAME_SYNC_PULL_DIRS) {
            info!(
                "start sync_pull_dirs on server: {} at: {}",
                self.get_host(),
                Local::now()
            );
            if as_service {
                self.session.take();
                self.connect()?;
            }
            let start = Instant::now();
            let started_at = Local::now();
            let rs = self.start_pull_sync_working_file_list(pb)?;
            self.remove_working_file_list_file();
            self.confirm_remote_sync()?;
            if as_service {
                if let Some(sess) = self.session.as_mut() {
                    sess.disconnect(None, "", None).ok();
                }
                self.session.take();
            }
            Ok(Some(SyncDirReport::new(start.elapsed(), started_at, rs)))
        } else {
            Ok(None)
        }
    }

    pub fn client_push_loop(
        &self,
        pb: &mut Indicator,
        as_service: bool,
    ) -> Result<Option<SyncDirReport>, failure::Error> {

        let session = self.create_ssh_session()?;
        let mut channel: ssh2::Channel = session.channel_session()?;
        let cmd = format!(
            "{}{} server-receive-loop",
            self.server_yml.remote_exec,
            if self.app_conf.verbose { " --vv" } else { "" }
        );
        trace!("invoke remote: {}", cmd);
        channel.exec(&cmd).expect("start remote server-loop");

        let mut cppb = ClientPushProgressBar::new(self.count_local_dir_files());
        let mut message_hub = SshChannelMessageHub::new(channel);

        let server_yml = StringMessage::from_path(
            self.yml_location
                .as_ref()
                .expect("yml_location should exist.")
                .as_path(),
        );
        message_hub.write_and_flush(server_yml.as_server_yml_sent_bytes().as_slice())?;
        let mut changed = 0_u64;
        let mut unchanged = 0_u64;
        let mut has_errror = 0_u64;
        // after sent server_yml, will send push_primary_file_item repeatly, when finish sending follow a RepeatDone message.
        for dir in self.server_yml.directories.iter() {
            let push_file_items = dir.push_file_item_iter(
                &self.app_conf.app_instance_id,
                &dir.local_dir,
                self.app_conf.skip_sha1,
            );
            for fi in push_file_items {
                message_hub.write_and_flush(&fi.as_sent_bytes())?;
                match message_hub.read_type_byte().expect("read type byte.") {
                    TransferType::FileItemChanged => {
                        let change_message = StringMessage::parse(&mut message_hub)?;
                        trace!("changed file: {}.", change_message.content);
                        let file_len = fi.local_path.as_path().metadata()?.len();
                        cppb.push_one(file_len, fi.local_path.as_str());
                        let u64_message = U64Message::new(file_len);
                        message_hub.write_and_flush(&u64_message.as_start_send_bytes())?;
                        trace!("start send header sent.");
                        let mut buf = [0; 8192];
                        let mut file = fs::OpenOptions::new()
                            .read(true)
                            .open(fi.local_path.as_path())?;
                        trace!("start send file content.");
                        loop {
                            let readed = file.read(&mut buf)?;
                            if readed == 0 {
                                message_hub.flush()?;
                                break;
                            } else {
                                cppb.progress(readed);
                                message_hub.write_all(&buf[..readed])?;
                            }
                        }
                        changed += 1;
                        trace!("send file content done.");
                    }
                    TransferType::FileItemUnchanged => {
                        cppb.skip_one();
                        unchanged += 1;
                        trace!("unchanged file.");
                    }
                    TransferType::StringError => {
                        let ss = StringMessage::parse(&mut message_hub)?;
                        error!("string error: {:?}", ss.content);
                    }
                    i => error!("got unexpected transfer type {:?}", i),
                }
            }
        }
        message_hub.write_and_flush(&[TransferType::RepeatDone.to_u8()])?;
        info!("changed: {}, unchanged: {}", changed, unchanged);
        cppb.finish();
        Ok(None)
    }

    /// When in the role of AppRole::ActiveLeaf, it list changed files in the local disk.
    /// But it has no way to know the changes happen in the remote side, what if the remote file has been deleted? at that situation it should upload again.
    pub fn create_file_list_files<O: io::Write>(&self, out: &mut O) -> Result<(), failure::Error> {
        if self.db_access.is_some() && self.server_yml.use_db {
            let db_access = self.db_access.as_ref().unwrap();
            for one_dir in self.server_yml.directories.iter() {
                trace!("start load directory: {:?}", one_dir);
                one_dir.load_relative_item_to_sqlite(
                    self.app_conf.app_role.as_ref(),
                    db_access,
                    self.is_skip_sha1(),
                    self.server_yml.sql_batch_size,
                    self.server_yml.rsync.sig_ext.as_str(),
                    self.server_yml.rsync.delta_ext.as_str(),
                )?;
                trace!("load_relative_item_to_sqlite done.");
                for sql in self.server_yml.exclude_by_sql.iter() {
                    if let Err(err) = db_access.exclude_by_sql(sql) {
                        eprintln!("exclude_by_sql execution failed: {:?}", err);
                        error!("exclude_by_sql execution failed: {:?}", err);
                    }
                }
                trace!("exclude_by_sql done.");
                db_access.iterate_files_by_directory_changed_or_unconfirmed(|fi_db_or_path| {
                    match fi_db_or_path {
                        (Some(fi_db), None) => {
                            if fi_db.changed || !fi_db.confirmed {
                                match serde_json::to_string(&RelativeFileItem::from(fi_db)) {
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
                one_dir.load_relative_item(
                    self.app_conf.app_role.as_ref(),
                    out,
                    self.is_skip_sha1(),
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::{string_path::SlashPath, AppRole};
    use crate::db_accesses::{DbAccess, SqliteDbAccess};
    use crate::develope::tutil;
    use crate::log_util;
    use bzip2::write::{BzDecoder, BzEncoder};
    use bzip2::Compression;
    use glob::Pattern;
    use std::fs;
    use std::io::{self, Write};
    use std::net::TcpStream;
    use indicatif::{ProgressBar, ProgressStyle};

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec![
                "data_shape::server",
                "data_shape::app_conf",
                "protocol",
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
        let sess = server.get_ssh_session();
        let mut channel: ssh2::Channel = sess.channel_session().unwrap();
        let cmd = format!("{} cp abc", server.get_remote_exec());
        eprintln!("{}", cmd);
        channel.exec(&cmd).unwrap();
        let mut buf = vec![0; 8192];
        let mut ll = 0;
        channel.write_all(&buf[..5]).unwrap();
        let of = PathBuf::from("data/tt.png");
        let mut file = fs::OpenOptions::new().write(true).create(true).open(&of)?;
        loop {
            let readed = channel.read(&mut buf)?;
            if readed == 0 {
                break;
            } else {
                ll += readed;
                file.write_all(&buf[..readed])?;
            }
        }
        let mut s = String::new();
        channel.stderr().read_to_string(&mut s)?;
        eprintln!("{}", s);
        let f = PathBuf::from("fixtures/qrcode.png").metadata()?.len();

        assert_eq!(ll as u64, f);

        assert!(server.is_connected());
        Ok(())
    }

    #[test]
    fn t_client_loop() -> Result<(), failure::Error> {
        log();
        let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::ActiveLeaf);
        app_conf.mini_app_conf.skip_cron = true;
        app_conf.mini_app_conf.verbose = true;
        app_conf.mini_app_conf.console_log = true;

        assert!(app_conf.mini_app_conf.app_role == Some(AppRole::ActiveLeaf));
        let server = tutil::load_demo_server_sqlite(&app_conf, Some("localhost_2.yml"));

        let a_dir = SlashPath::from_path(
            dirs::home_dir()
                .expect("get home_dir")
                .join("directories")
                .join(&app_conf.mini_app_conf.app_instance_id)
                .join("a-dir")
                .as_path(),
        )
        .expect("get slash path from home_dir");

        let mut indicator = Indicator::new(None);

        server.client_push_loop(&mut indicator, false)?;

        info!("a_dir is {:?}", a_dir);
        let cc_txt = a_dir.join("b").join("c c").join("c c .txt");
        info!("cc_txt is {:?}", cc_txt);
        assert!(cc_txt.exists());
        assert!(a_dir.join("b").join("b.txt").exists());
        assert!(a_dir.join("b b").join("b b.txt").exists());
        assert!(a_dir.join("a.txt").exists());
        assert!(a_dir.join("qrcode.png").exists());
        Ok(())
    }

    #[test]
    fn t_sync_push_dirs() -> Result<(), failure::Error> {
        log();
        let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::ActiveLeaf);
        app_conf.mini_app_conf.skip_cron = true;
        app_conf.mini_app_conf.verbose = true;

        assert!(app_conf.mini_app_conf.app_role == Some(AppRole::ActiveLeaf));
        let mut server = tutil::load_demo_server_sqlite(&app_conf, None);

        let db_file = server.get_db_file();

        if db_file.exists() {
            fs::remove_file(db_file.as_path())?;
        }

        let sqlite_db_access = SqliteDbAccess::new(db_file);
        sqlite_db_access.create_database()?;
        server.set_db_access(sqlite_db_access);

        server.connect()?;
        let cc = server.count_remote_files()?;
        assert_eq!(cc, 1, "should have 1 files at server side.");
        let a_dir = &server
            .server_yml
            .directories
            .iter()
            .find(|d| d.remote_dir.ends_with("a-dir"))
            .expect("should have a directory who's remote_dir end with 'a-dir'")
            .remote_dir;

        if a_dir.exists() {
            info!("directories path: {:?}", a_dir.as_path());
            fs::remove_dir_all(a_dir.as_path())?;
        }

        let mut indicator = Indicator::new(None);
        let stats = server.sync_push_dirs(&mut indicator)?;
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
        app_conf.mini_app_conf.verbose = true;

        assert!(app_conf.mini_app_conf.app_role == Some(AppRole::PullHub));
        let mut server = tutil::load_demo_server_sqlite(&app_conf, None);

        // it's useless. because the db file is from remote server's perspective.
        let db_file = server.get_db_file();
        if db_file.exists() {
            fs::remove_file(db_file.as_path())?;
        }
        let remote_db_path =
            Path::new("./target/debug/data/passive-leaf-data/demo-app-instance-id/db.db");

        if remote_db_path.exists() {
            fs::remove_file(remote_db_path)?;
        }

        let sqlite_db_access = SqliteDbAccess::new(db_file);
        sqlite_db_access.create_database()?;
        server.set_db_access(sqlite_db_access);

        let a_dir = server
            .server_yml
            .directories
            .iter()
            .find(|d| d.remote_dir.ends_with("a-dir"))
            .expect("should have a directory who's remote_dir end with 'a-dir'")
            .local_dir
            .clone();

        let a_dir = a_dir.as_path();
        if a_dir.exists() {
            info!("remove directory: {:?}", a_dir);
            fs::remove_dir_all(a_dir)?;
        }

        let mut indicator = Indicator::new(None);
        server.connect()?;
        let stats = server.sync_pull_dirs(&mut indicator, false)?;
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
            remote_dir: SlashPath::new("fixtures/a-dir"),
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 8);
        tutil::print_cursor_lines(&mut cur);

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: SlashPath::new("fixtures/a-dir"),
            includes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.excludes_patterns.is_none());
        one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 2); // one dir line, one file line.

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: SlashPath::new("fixtures/a-dir"),
            excludes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
        let num = tutil::count_cursor_lines(&mut cur);
        assert_eq!(num, 7, "if exclude 1 file there should 7 left.");

        let mut cur = tutil::get_a_cursor_writer();
        let mut one_dir = Directory {
            remote_dir: SlashPath::new("fixtures/a-dir"),
            excludes: vec!["**/Tomcat6/logs/**".to_string()],
            ..Directory::default()
        };

        one_dir.compile_patterns()?;
        assert!(one_dir.includes_patterns.is_none());
        one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
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

    #[test]
    fn t_pb_in_action() -> Result<(), failure::Error> {
        let bar = ProgressBar::new(1000);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{prefix}[{elapsed_precise}] {bar:40.cyan/blue} {bytes:>7}/{total_bytes:7} {bytes_per_sec} {msg}")
                .progress_chars("##-"),
        );
        let three = std::time::Duration::from_millis(10);
        bar.set_message("a");
        for _ in 0..1000 {
            bar.inc(1);
            std::thread::sleep(three);
        }
        bar.reset();
        bar.set_length(2000);
        bar.set_message("hello b");
        for _ in 0..2000 {
            bar.inc(1);
            std::thread::sleep(three);
        }        
        bar.finish();
        Ok(())
    }
}
