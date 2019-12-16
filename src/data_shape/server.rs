use super::{
    app_conf, rolling_files, AppRole, AuthMethod, Directory, FileChanged, FullPathFileItem,
    Indicator, MiniAppConf, PbProperties, ProgressWriter, PruneStrategy, ScheduleItem, SlashPath,
    TransferFileProgressBar,
};
use crate::actions::{copy_a_file_sftp, ssh_util};
use crate::db_accesses::SqliteDbAccess;
use crate::protocol::{MessageHub, SshChannelMessageHub, StringMessage, TransferType, U64Message};
use bzip2::write::BzEncoder;
use bzip2::Compression;
use chrono::Local;
use encoding_rs::*;
use indicatif::ProgressStyle;
use log::*;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use ssh2;
use std::ffi::OsString;
use std::io::prelude::Read;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io, io::Write};
use tar::Builder;

pub const CRON_NAME_SYNC_PULL_DIRS: &str = "sync-pull-dirs";

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
    pub possible_encoding: Vec<String>,
}

impl ServerYml {
    pub fn get_possible_encoding(&self) -> Vec<&Encoding> {
        self.possible_encoding
            .iter()
            .map(|s| s.to_uppercase())
            .filter_map(|ename| match ename.as_str() {
                "UTF8" => Some(UTF_8),
                "GBK" => Some(GBK),
                "SHIFT_JIS" => Some(SHIFT_JIS),
                _ => None,
            })
            .collect()
    }
}

pub struct Server {
    pub server_yml: ServerYml,
    session: Option<ssh2::Session>,
    // for passive_leaf node, it's dependent on invoking parameter of app_instance_id.
    my_dir: PathBuf,
    #[allow(dead_code)]
    reports_dir: PathBuf,
    archives_dir: PathBuf,
    working_dir: PathBuf,
    pub yml_location: Option<PathBuf>,
    pub db_access: Option<SqliteDbAccess>,
    _m: PhantomData<SqliteConnectionManager>,
    app_conf: MiniAppConf,
    lock_file: Option<fs::File>,
}

unsafe impl Sync for Server {}

unsafe impl Send for Server {}

impl Server {
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
                .expect("compile_patterns should succeeded.");
        });

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

    /// Server's my_dir is composed of the app_setting's 'data dir' and the server distinctness, app_instance_id o/r hostname.
    /// It varies on the role of the application.
    pub fn get_my_dir(&self) -> &Path {
        self.my_dir.as_path()
    }

    pub fn get_my_directories(&self) -> SlashPath {
        SlashPath::from_path(self.get_my_dir(), &vec![])
            .expect("my_dir should exists.")
            .join("directories")
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

    pub fn get_access_log(&self) -> Result<fs::File, failure::Error> {
        let cf = self.working_dir.join("sync.log");
        Ok(fs::OpenOptions::new().create(true).write(true).open(cf)?)
    }

    pub fn read_last_file_count(&self) -> u64 {
        let cf = self.working_dir.join("last_counting.txt");
        if cf.exists() {
            let mut f = fs::OpenOptions::new()
                .read(true)
                .open(cf.as_path())
                .expect("last_counting_file opened.");
            let mut s = String::new();
            f.read_to_string(&mut s)
                .expect("read last counting file to string.");
            s.trim()
                .parse::<u64>()
                .expect("last_counting_file content parse to u64.")
        } else {
            0
        }
    }

    pub fn write_last_file_count(&self, count: u64) {
        let cf = self.working_dir.join("last_counting.txt");
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(cf.as_path())
            .expect("last_counting_file opened.");
        write!(f, "{}", count).expect("wrote file count to file.");
    }

    /// For app_role is ReceiveHub, remote exec is from user's home directory.
    pub fn get_remote_exec(&self) -> SlashPath {
        SlashPath::new(&self.server_yml.remote_exec)
    }

    /// The passive leaf side of the application will try to find the configuration in the passive-leaf-conf folder which located in the same folder as the executable.
    /// The server yml file should located in the data/passive-leaf-conf/{self.app_conf.app_instance_id}.yml
    pub fn get_remote_server_yml(&self) -> String {
        let conf_folder = if let Some(app_role) = self.app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => app_conf::PASSIVE_LEAF_CONF,
                AppRole::ActiveLeaf => app_conf::RECEIVE_SERVERS_CONF,
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

    pub fn set_db_access(&mut self, db_access: SqliteDbAccess) {
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
            .map(|dir| &dir.to_dir)
            .collect();
        let ass: Vec<&SlashPath> = directories.iter().map(|dir| &dir.to_dir).collect();
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

    fn current_archive_file_slash_path(&self) -> SlashPath {
        SlashPath::from_path(self.current_archive_file_path().as_path(), &vec![])
            .expect("current_archive_file_path got.")
    }

    fn archive_internal(&self, pb: &mut Indicator) -> Result<PathBuf, failure::Error> {
        let total_size = self.count_from_dirs_size();

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
            let d_path = &dir.from_dir.as_path();
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
                error!("dir.from_dir get file_name failed: {:?}", d_path);
            }
        }
        archive.finish()?;
        Ok(cur_archive_path)
    }

    fn archive_out(&self, pb: &mut Indicator) -> Result<PathBuf, failure::Error> {
        let cur_archive_path = self.current_archive_file_slash_path();

        let style = ProgressStyle::default_bar().template("{spinner} {wide_msg}");

        pb.active_pb_total().alter_pb(PbProperties {
            set_style: Some(style),
            enable_steady_tick: Some(200),
            set_message: None,
            ..PbProperties::default()
        });

        let my_directories = self.get_my_directories();

        for dir in self.server_yml.directories.iter() {
            pb.set_message(format!(
                "archive directory: {:?}, using out util: {}",
                dir.from_dir,
                self.app_conf.archive_cmd.get(0).unwrap()
            ));
            let archive_cmd = self
                .app_conf
                .archive_cmd
                .iter()
                .map(|s| {
                    if s == "archive_file_name" {
                        cur_archive_path.get_os_string()
                    } else if s == "files_and_dirs" {
                        // let df = my_directories.join_another(&dir.get_to_dir_base("")); // use to path.
                        // df.get_os_string()
                        my_directories
                            .join_another(&dir.get_to_dir_base(""))
                            .get_os_string()
                    } else {
                        OsString::from(s)
                    }
                })
                .collect::<Vec<OsString>>();
            trace!("run archive_cmd: {:?}", archive_cmd);

            let mut c = self.get_archive_cmd();

            for seg in archive_cmd {
                c.arg(seg);
            }
            let output = c.output().expect("failed to execute process");

            // let output = if cfg!(target_os = "windows") {
            //     let mut c = Command::new("cmd");
            //     c.arg("/C");
            //     for seg in archive_cmd {
            //         c.arg(seg);
            //     }
            //     c.output().expect("failed to execute process")
            // } else {
            //     let mut c = Command::new("sh");
            //     c.arg("-c");
            //     for seg in archive_cmd {
            //         c.arg(seg);
            //     }
            //     c.output().expect("failed to execute process")
            // };
            trace!("archive_cmd output: {:?}", output);
        }
        Ok(cur_archive_path.as_path().to_path_buf())
    }

    #[cfg(unix)]
    fn get_archive_cmd(&self) -> Command {
        let mut c = Command::new("sh");
        c.arg("-c");
        c
    }

    #[cfg(windows)]
    fn get_archive_cmd(&self) -> Command {
        let mut c = Command::new("cmd");
        c.arg("/C");
        c
    }

    /// Archive need not to schedule standalone.
    /// Because of conflict with the sync operation.
    pub fn archive_local(&self) -> Result<(), failure::Error> {
        info!(
            "start archive_local on server: {} at: {}",
            self.get_host(),
            Local::now()
        );
        let mut pb = Indicator::new(None);
        let cur = if self.app_conf.archive_cmd.is_empty() {
            self.archive_internal(&mut pb)?
        } else {
            self.archive_out(&mut pb)?
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

    #[allow(dead_code)]
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

    pub fn count_from_dirs_size(&self) -> u64 {
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

    pub fn client_pull_loop(&self) -> Result<Option<(u64, u64)>, failure::Error> {
        let session = self.create_ssh_session()?;
        let mut channel: ssh2::Channel = session.channel_session()?;
        let cmd = format!(
            "{}{}{} server-send-loop",
            self.server_yml.remote_exec,
            if self.app_conf.verbose { " --vv" } else { "" },
            if self.app_conf.skip_sha1 {
                ""
            } else {
                "--enable-sha1"
            },
        );
        trace!("invoke remote: {}", cmd);
        channel.exec(&cmd).expect("start remote server-loop");

        let mut sync_log = self.get_access_log()?;

        let mut message_hub = SshChannelMessageHub::new(channel);

        let server_yml = StringMessage::from_path(
            self.yml_location
                .as_ref()
                .expect("yml_location should exist.")
                .as_path(),
        );
        message_hub.write_and_flush(server_yml.as_server_yml_sent_bytes().as_slice())?;

        let my_directories = SlashPath::from_path(self.get_my_dir(), &vec![])
            .expect("my_dir should exists.")
            .join("directories");
        trace!("save to my_directories: {:?}", my_directories);
        let file_count = self.read_last_file_count();
        let mut cppb = TransferFileProgressBar::new(file_count, self.app_conf.show_pb);

        let mut new_file_count = 0_u64;

        let mut last_df: Option<SlashPath> = None;
        let mut last_file_item: Option<FullPathFileItem> = None;
        let mut buf = vec![0; 8192];

        loop {
            let type_byte = match message_hub.read_type_byte() {
                Err(err) => {
                    error!("got error type byte: {}", err);
                    error!("last_df: {:?}", last_df);
                    error!("last_file_item: {:?}", last_file_item);
                    break;
                }
                Ok(type_byte) => type_byte,
            };

            match type_byte {
                TransferType::FileItem => {
                    new_file_count += 1;
                    let string_message = StringMessage::parse(&mut message_hub)?;
                    trace!("got file item: {}", string_message.content);
                    match serde_json::from_str::<FullPathFileItem>(&string_message.content) {
                        Ok(file_item) => {
                            let df = my_directories.join_another(&file_item.to_path); // use to path.
                            match file_item.changed(df.as_path()) {
                                FileChanged::NoChange => {
                                    message_hub.write_transfer_type_only(
                                        TransferType::FileItemUnchanged,
                                    )?;
                                    cppb.skip_one();
                                }
                                fc => {
                                    let string_message = StringMessage::new(format!("{:?}", fc));
                                    message_hub.write_and_flush(
                                        &string_message.as_string_sent_bytes_with_header(
                                            TransferType::FileItemChanged,
                                        ),
                                    )?;
                                    last_df.replace(df);
                                    last_file_item.replace(file_item);
                                }
                            }
                        }
                        Err(err) => {
                            // why send error message to server side?
                            // message_hub.write_error_message(format!("{:?}", err))?;
                            error!("{:?}", err);
                        }
                    };
                }
                TransferType::StartSend => {
                    let content_len = U64Message::parse(&mut message_hub)?;
                    // file item is from another side.
                    if let (Some(df), Some(file_item)) = (last_df.take(), last_file_item.take()) {
                        cppb.push_one(file_item.len, &file_item);
                        writeln!(sync_log, "{}", file_item.to_path).ok();
                        trace!("copy to file: {:?}", df.as_path());
                        match message_hub.copy_to_file(
                            &mut buf,
                            content_len.value,
                            df.as_path(),
                            Some(&cppb),
                        ) {
                            Err(err) => {
                                // why send error message to server side?
                                // message_hub.write_error_message(format!("{:?}", err))?;
                                //log at client side.
                                error!("copy_to_file got error {:?}", err);
                            }
                            Ok(()) => {
                                if let Some(md) = file_item.modified {
                                    let ft = filetime::FileTime::from_unix_time(md as i64, 0);
                                    filetime::set_file_mtime(df.as_path(), ft)?;
                                } else {
                                    // why send error message to server side?
                                    // message_hub.write_error_message(
                                    //     "push_primary_file_item has no modified value.",
                                    // )?;
                                    error!(
                                        "push_primary_file_item has no modified: {:?}",
                                        file_item
                                    )
                                }
                            }
                        }
                    } else {
                        error!("empty last_df.");
                    }
                }
                TransferType::StringError => {
                    // must read it or else the stream will stall.
                    let ss = StringMessage::parse(&mut message_hub)?;
                    error!("string error: {:?}", ss.content);
                }
                TransferType::RepeatDone | TransferType::Eof => {
                    info!("got eof, exiting.");
                    break;
                }
                t => {
                    error!("unhandled transfer type: {:?}", t);
                    panic!("unimplement transfer type.");
                }
            }
        }
        cppb.pb.finish_with_message("done.");
        self.write_last_file_count(new_file_count);
        sync_log.flush()?;
        message_hub.close()?;
        Ok(None)
    }

    pub fn client_push_loop(
        &self,
        _follow_archive: bool,
    ) -> Result<Option<(u64, u64)>, failure::Error> {
        let session = self.create_ssh_session()?;
        let mut channel: ssh2::Channel = session.channel_session()?;
        let cmd = format!(
            "{}{} server-receive-loop",
            self.server_yml.remote_exec,
            if self.app_conf.verbose { " --vv" } else { "" }
        );
        trace!("invoke remote: {}", cmd);
        channel.exec(&cmd).expect("start remote server-loop");
        let file_count = self.read_last_file_count();
        let mut cppb = TransferFileProgressBar::new(file_count, self.app_conf.show_pb);
        let mut message_hub = SshChannelMessageHub::new(channel);

        let mut new_file_count = 0_u64;

        let server_yml = StringMessage::from_path(
            self.yml_location
                .as_ref()
                .expect("yml_location should exist.")
                .as_path(),
        );
        message_hub.write_and_flush(server_yml.as_server_yml_sent_bytes().as_slice())?;
        let mut changed = 0_u64;
        let mut unchanged = 0_u64;
        let mut buf = [0; 8192];
        let possible_encoding = self.server_yml.get_possible_encoding();
        // after sent server_yml, will send push_primary_file_item repeatly, when finish sending follow a RepeatDone message.
        for dir in self.server_yml.directories.iter() {
            let push_file_items = dir.file_item_iter(
                &self.app_conf.app_instance_id,
                self.app_conf.skip_sha1,
                &possible_encoding,
            );
            for fi in push_file_items {
                new_file_count += 1;
                match fi {
                    Ok(fi) => {
                        message_hub.write_and_flush(&fi.as_sent_bytes())?;
                        match message_hub.read_type_byte().expect("read type byte.") {
                            TransferType::FileItemChanged => {
                                let change_message = StringMessage::parse(&mut message_hub)?;
                                trace!("changed file: {}.", change_message.content);
                                cppb.push_one(fi.len, &fi);
                                message_hub.copy_from_file(&mut buf, &fi, Some(&cppb))?;
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
                    Err(err) => {
                        error!("{:?}", err);
                    }
                }
            }
        }
        message_hub.write_and_flush(&[TransferType::RepeatDone.to_u8()])?;
        info!("changed: {}, unchanged: {}", changed, unchanged);
        cppb.pb.finish_with_message("done.");
        self.write_last_file_count(new_file_count);
        message_hub.close()?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::{string_path::SlashPath, AppRole};
    // use crate::db_accesses::{DbAccess, SqliteDbAccess};
    use crate::develope::tutil;
    use crate::log_util;
    use std::fs;
    use std::io::Write;

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
    fn t_client_pull_loop() -> Result<(), failure::Error> {
        log();
        let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
        app_conf.mini_app_conf.verbose = true;
        app_conf.mini_app_conf.console_log = true;

        assert!(app_conf.mini_app_conf.app_role == Some(AppRole::PullHub));
        let server = tutil::load_demo_server_sqlite(&app_conf, Some("localhost_2.yml"));

        eprintln!("{:?}", server.working_dir);
        eprintln!("{:?}", server.reports_dir);

        let a_dir = SlashPath::from_path(
            server
                .get_my_dir()
                .join("directories")
                .join("a-dir")
                .as_path(),
            &vec![],
        )
        .expect("get slash path from home_dir");

        server.client_pull_loop()?;

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
    fn t_client_push_loop() -> Result<(), failure::Error> {
        log();
        let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::ActiveLeaf);
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
            &vec![],
        )
        .expect("get slash path from home_dir");

        server.client_push_loop(false)?;

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
}
