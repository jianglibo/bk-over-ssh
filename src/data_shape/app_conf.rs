use crate::data_shape::{string_path, Server, ServerYml};
use crate::db_accesses::{SqliteDbAccess};
use indicatif::MultiProgress;
use log::*;
use log::{trace, warn};
use serde::{Deserialize, Serialize};
use std::env;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::{fs, io::Read, io::Write};
use r2d2_sqlite::SqliteConnectionManager;

pub const APP_CONFIG_BYTES: &[u8] = include_bytes!("../app_config_demo.yml");
pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";

pub const PULL_CONF: &str = "pull-conf";
pub const PULL_DATA: &str = "pull-data";

pub const RECEIVE_SERVERS_CONF: &str = "receive-servers-conf";
// pub const RECEIVE_SERVERS_DATA: &str = "receive-servers-data";

// even passive leaf may have mulitple configuration file. For example when mulitple PullHubs pull this server.
pub const PASSIVE_LEAF_CONF: &str = "passive-leaf-conf";
// pub const PASSIVE_LEAF_DATA: &str = "passive-leaf-data";

// even active leaf may have multiple configuration file. For example push to mulitple ReceiveHubs.
pub const PUSH_CONF: &str = "push-conf";
pub const PUSH_DATA: &str = "push-data";

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct LogConf {
    pub log_file: String,
    verbose_modules: Vec<String>,
}

impl LogConf {
    pub fn get_verbose_modules(&self) -> &Vec<String> {
        &self.verbose_modules
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum AppRole {
    PullHub,
    ActiveLeaf,
}

impl AppRole {
    #[allow(dead_code)]
    pub fn to_str(&self) -> &str {
        match &self {
            AppRole::PullHub => "pull_hub",
            AppRole::ActiveLeaf => "active_leaf",
        }
    }
}

impl FromStr for AppRole {
    type Err = &'static str;

    fn from_str(role_name: &str) -> Result<Self, Self::Err> {
        match role_name {
            "pull_hub" => Ok(AppRole::PullHub),
            "active_leaf" => Ok(AppRole::ActiveLeaf),
            _rn => Err("unexpected role name"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct MailConf {
    pub from: String,
    pub username: String,
    pub password: String,
    pub hostname: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfYml {
    app_instance_id: String,
    data_dir: String,
    log_conf: LogConf,
    pub mail_conf: MailConf,
    archive_cmd: Vec<String>,
}

impl Default for AppConfYml {
    fn default() -> Self {
        Self {
            app_instance_id: "an-pull-hub-instance".to_string(),
            data_dir: "data".to_string(),
            // role: AppRole::PullHub,
            mail_conf: MailConf::default(),
            log_conf: LogConf::default(),
            archive_cmd: Vec::new(),
        }
    }
}

/// The data dir is a fixed path relative to the exec file no matter the role of the app.
///
fn guess_data_dir(data_dir: impl AsRef<str>) -> Result<PathBuf, failure::Error> {
    let mut path_buf = {
        let data_dir = data_dir.as_ref();
        let data_dir = if data_dir.is_empty() {
            "data"
        } else {
            data_dir
        };
        PathBuf::from(data_dir)
    };

    if !&path_buf.is_absolute() {
        path_buf = env::current_exe()?
            .parent()
            .expect("current_exe parent should exists.")
            .join(path_buf);
    }
    if !&path_buf.exists() {
        if let Err(err) = fs::create_dir_all(&path_buf) {
            bail!("create data_dir {:?}, failed: {:?}", &path_buf, err);
        }
    }
    match path_buf.canonicalize() {
        Ok(ab) => Ok(ab),
        Err(err) => bail!("path_buf {:?} canonicalize failed: {:?}", &path_buf, err),
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct MiniAppConf {
    pub buf_len: Option<usize>,
    pub skip_sha1: bool,
    pub archive_cmd: Vec<String>,
    pub app_instance_id: String,
    pub app_role: Option<AppRole>,
    pub verbose: bool,
    pub console_log: bool,
    pub as_service: bool,
    pub show_pb: bool,
}

#[derive(Debug, Serialize)]
pub struct AppConf {
    inner: AppConfYml,
    pub config_file_path: PathBuf,
    pub data_dir_full_path: PathBuf,
    pub log_full_path: PathBuf,
    pub servers_conf_dir: PathBuf,
    #[serde(skip)]
    pub db_access: Option<SqliteDbAccess>,
    #[serde(skip)]
    _m: PhantomData<SqliteConnectionManager>,
    #[serde(skip)]
    lock_file: Option<fs::File>,
    #[serde(skip)]
    pub progress_bar: Option<Arc<MultiProgress>>,
    pub mini_app_conf: MiniAppConf,
}

#[derive(Debug, Fail)]
pub enum ReadAppConfException {
    #[fail(display = "read app conf file failed: {:?}", _0)]
    ReadAppConfFileFailed(PathBuf),
    #[fail(display = "serde deserialize failed: {:?}", _0)]
    SerdeDeserializeFailed(PathBuf),
    #[fail(display = "guess data dir failed.")]
    GuessDataDirFailed,
    #[fail(display = "create server conf dir failed.")]
    CreateServersConfDirFailed,
    #[fail(display = "guess app conf name failed.")]
    GuessAppConfNameFailed,
}

pub fn demo_app_conf(data_dir: &str, app_role: AppRole) -> AppConf {
    let servers_conf_dir_name = match app_role {
        AppRole::PullHub => PULL_CONF,
        AppRole::ActiveLeaf => PUSH_CONF,
    };

    AppConf {
        inner: AppConfYml::default(),
        config_file_path: Path::new("abc").to_path_buf(),
        data_dir_full_path: PathBuf::from(data_dir),
        log_full_path: PathBuf::from(data_dir).join("out.log"),
        servers_conf_dir: PathBuf::from("data").join(servers_conf_dir_name),
        _m: PhantomData,
        db_access: None,
        lock_file: None,
        progress_bar: None,
        mini_app_conf: MiniAppConf {
            app_instance_id: "demo-app-instance-id".to_string(),
            skip_sha1: true,
            buf_len: None,
            archive_cmd: Vec::new(),
            app_role: Some(app_role),
            verbose: false,
            console_log: false,
            as_service: false,
            show_pb: false,
        },
    }
}

impl AppConf {
    pub fn set_db_access(&mut self, db_access: SqliteDbAccess) {
        // if let Err(err) = db_access.create_database() {
        //     warn!("create database failed: {:?}", err);
        // }
        self.db_access.replace(db_access);
    }

    pub fn get_sqlite_db_file(&self) -> PathBuf {
        self.data_dir_full_path.join("db.db")
    }

    #[allow(dead_code)]
    pub fn get_inner(&self) -> &AppConfYml {
        &self.inner
    }
    #[allow(dead_code)]
    pub fn get_db_access(&self) -> Option<&SqliteDbAccess> {
        self.db_access.as_ref()
    }

    pub fn set_app_instance_id(&mut self, app_instance_id: impl AsRef<str>) {
        let s = app_instance_id.as_ref().to_string();
        self.inner.app_instance_id = s.clone();
        self.mini_app_conf.app_instance_id = s;
    }

    /// If parse app configuration file failed, move the failed configuration file to bak. recreate a fresh new one.
    /// full_data_dir is default to ./data, servers_conf_dir is default to full_data_dir/(app_role). This means app_role is a must to find server configuration file.
    /// If we can fall back to find configuration file under full_data_dir then app_rols will bacame optional.
    fn read_app_conf(
        file: impl AsRef<Path>,
        app_role: Option<&AppRole>,
    ) -> Result<AppConf, failure::Error> {
        let file = file.as_ref();
        if let Ok(mut f) = fs::OpenOptions::new().read(true).open(file) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                match serde_yaml::from_str::<AppConfYml>(&buf) {
                    Ok(app_conf_yml) => {
                        let data_dir_full_path =
                            if let Ok(gdd) = guess_data_dir(app_conf_yml.data_dir.trim()) {
                                gdd
                            } else {
                                bail!(ReadAppConfException::GuessDataDirFailed);
                            };

                        let log_full_path = {
                            let log_file = &app_conf_yml.log_conf.log_file;
                            let path = Path::new(log_file);
                            if path.is_absolute() {
                                log_file.clone()
                            } else {
                                data_dir_full_path
                                    .as_path()
                                    .join(path)
                                    .to_str()
                                    .expect("log_file should be a valid string.")
                                    .to_string()
                            }
                        };

                        let log_full_path = Path::new(&log_full_path).to_path_buf();

                        let servers_conf_dir = if let Some(app_role) = app_role {
                            match app_role {
                                AppRole::PullHub => {
                                    data_dir_full_path.as_path().join(PULL_CONF)
                                }
                                AppRole::ActiveLeaf => {
                                    data_dir_full_path.as_path().join(PUSH_CONF)
                                }
                            }
                        } else {
                            data_dir_full_path.clone()
                        };

                        trace!("found servers conf dir: {:?}", servers_conf_dir);

                        if !servers_conf_dir.exists() {
                            if let Err(err) = fs::create_dir_all(&servers_conf_dir) {
                                error!(
                                    "create servers_conf_dir {:?}, failed: {:?}",
                                    &servers_conf_dir, err
                                );
                                bail!(ReadAppConfException::CreateServersConfDirFailed);
                            }
                        }

                        let archive_cmd = app_conf_yml.archive_cmd.clone();
                        let app_instance_id = app_conf_yml.app_instance_id.clone();

                        let app_conf = AppConf {
                            inner: app_conf_yml,
                            config_file_path: file.to_path_buf(),
                            data_dir_full_path,
                            log_full_path,
                            servers_conf_dir,
                            db_access: None,
                            _m: PhantomData,
                            lock_file: None,
                            progress_bar: None,
                            mini_app_conf: MiniAppConf {
                                app_instance_id,
                                skip_sha1: true,
                                buf_len: None,
                                archive_cmd,
                                app_role: app_role.cloned(),
                                verbose: false,
                                console_log: false,
                                as_service: false,
                                show_pb: false,
                            },
                        };
                        Ok(app_conf)
                    }
                    Err(err) => {
                        error!("deserialize failed: {:?}, {:?}", file, err);
                        bail!(ReadAppConfException::SerdeDeserializeFailed(
                            file.to_path_buf(),
                        ))
                    }
                }
            } else {
                error!("read_to_string failure: {:?}", file);
                bail!(ReadAppConfException::ReadAppConfFileFailed(
                    file.to_path_buf(),
                ))
            }
        } else {
            error!("open conf file failed: {:?}", file);
            bail!(ReadAppConfException::ReadAppConfFileFailed(
                file.to_path_buf(),
            ))
        }
    }

    pub fn get_mail_conf(&self) -> &MailConf {
        &self.inner.mail_conf
    }
    #[allow(dead_code)]
    pub fn write_to_working_dir(&self) -> Result<(), failure::Error> {
        let yml_serialized = serde_yaml::to_string(&self.inner)?;
        let path = env::current_dir()?.join(CONF_FILE_NAME);
        let mut file = fs::OpenOptions::new().write(true).create(true).open(path)?;
        write!(file, "{}", yml_serialized)?;
        Ok(())
    }

    /// If no conf file provided, first look at the same directory as executable, then current working directory.
    /// the app role must be known at this point.
    pub fn guess_conf_file(
        app_conf_file: Option<&str>,
        app_role: Option<&AppRole>,
    ) -> Result<AppConf, failure::Error> {
        let app_conf_file = if let Some(app_conf_file) = app_conf_file {
            if Path::new(app_conf_file).exists() {
                Some(PathBuf::from(app_conf_file))
            } else {
                None
            }
        } else {
            None
        };

        let app_conf_file = if let Some(app_conf_file) = app_conf_file {
            app_conf_file
        } else if let Ok(current_exe) = env::current_exe() {
            current_exe
                .parent()
                .expect("current_exe's parent should exist")
                .join(CONF_FILE_NAME)
        } else if let Ok(current_dir) = env::current_dir() {
            current_dir.join(CONF_FILE_NAME)
        } else {
            bail!(ReadAppConfException::GuessAppConfNameFailed);
        };
        if !app_conf_file.exists() {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(app_conf_file.as_path())?;
                file.write_all(APP_CONFIG_BYTES)?;
        }
        AppConf::read_app_conf(app_conf_file, app_role)
    }
    
    #[allow(dead_code)]
    pub fn lock_working_file(&mut self) -> Result<(), failure::Error> {
        let lof = self.data_dir_full_path.as_path().join("working.lock");
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

    // pub fn load_server_yml(
    //     &self,
    //     yml_file_name: impl AsRef<str>,
    // ) -> Result<(Server, Indicator), failure::Error> {
    //     let server = self.load_server_from_yml(yml_file_name.as_ref())?;
    //     if self.mini_app_conf.verbose {
    //         eprintln!(
    //             "load server yml from: {:?}",
    //             server.yml_location.as_ref().map(|pb| pb.as_os_str())
    //         );
    //     }
    //     let indicator = Indicator::new(self.progress_bar.clone());
    //     Ok((server, indicator))
    // }

    /// load all .yml file under servers directory.
    pub fn load_all_server_yml(&self, open_db: bool) -> Vec<Server> {
        trace!("servers_conf_dir is: {:?}", self.servers_conf_dir);
        if let Ok(read_dir) = self.servers_conf_dir.read_dir() {
            read_dir
                .filter_map(|ery| match ery {
                    Err(err) => {
                        warn!("read_dir entry return error: {:?}", err);
                        None
                    }
                    Ok(entry) => Some(entry.file_name().into_string()),
                })
                .filter_map(|from_os_string| match from_os_string {
                    Err(err) => {
                        warn!("osstring to_string failed: {:?}", err);
                        None
                    }
                    Ok(astr) => Some(astr),
                })
                .filter(|s|s.ends_with(".yml") || s.ends_with(".yaml"))
                .map(|astr| self.load_server_from_yml(astr, open_db))
                .filter_map(|rr| match rr {
                    Err(err) => {
                        warn!("load_server_yml failed: {:?}", err);
                        None
                    }
                    Ok(server) => Some(server),
                })
                .collect()
        } else {
            warn!("read_dir failed: {:?}", self.servers_conf_dir);
            Vec::new()
        }
    }

    pub fn get_log_conf(&self) -> &LogConf {
        &self.inner.log_conf
    }

    /// Why load_server_yml from app_conf?
    /// Because there is a servers_conf_dir item here.
    /// and my_dir is crucial.
    pub fn load_server_from_yml(
        &self,
        name: impl AsRef<str>,
        open_db: bool,
    ) -> Result<Server, failure::Error> {
        let name = name.as_ref();
        let mut server_yml_path = Path::new(name).to_path_buf();
        if (server_yml_path.is_absolute() || name.starts_with('/')) && !server_yml_path.exists() {
            // create the directories above this server yml file.
            if server_yml_path.is_absolute() {
                let sp = string_path::SlashPath::new(name)
                    .parent()
                    .expect("server yml's parent directory should exist");
                fs::create_dir_all(Path::new(sp.slash.as_str()))?;
            }
            bail!(
                "server yml file doesn't exist, please create one: {:?}",
                server_yml_path
            );
        } else {
            if !(name.contains('/') || name.contains('\\')) {
                server_yml_path = self.servers_conf_dir.as_path().join(name);
            }
            if !server_yml_path.exists() {
                bail!("server yml file doesn't exist: {:?}", server_yml_path);
            }
        }
        trace!("got server yml at: {:?}", server_yml_path);
        let mut f = fs::OpenOptions::new().read(true).open(&server_yml_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let server_yml: ServerYml = match serde_yaml::from_str(&buf) {
            Ok(server_yml) => server_yml,
            Err(err) => {
                bail!("parse yml file: {:?} failed: {}", server_yml_path, err);
            }
        };

        let conf_data_dir = self.data_dir_full_path.as_path();

        let data_dir_with_app_role = if let Some(app_role) = self.mini_app_conf.app_role.as_ref() {
            match app_role {
                AppRole::PullHub => conf_data_dir.join(PULL_DATA),
                AppRole::ActiveLeaf => conf_data_dir.join(PUSH_DATA),
            }
        } else {
            conf_data_dir.to_path_buf()
        };

        trace!("conf_data_dir: {:?}", conf_data_dir);
        trace!("data_dir_with_app_role: {:?}", data_dir_with_app_role);

        if !data_dir_with_app_role.exists() {
            fs::create_dir_all(&data_dir_with_app_role)?;
        }

        // Server's my_dir is composed of the 'data' dir and the host name of the server.
        // But for passive_leaf which host name it is?
        // We must use command line app_instance_id to override the value in the app_conf_yml,
        // Then use app_instance_id as my_dir.
        let my_dir = data_dir_with_app_role.join(&server_yml.host);
        // let my_dir = if let Some(app_role) = self.mini_app_conf.app_role.as_ref() {
        //     match app_role {
        //         _ => data_dir_with_app_role.join(&server_yml.host),
        //     }
        // } else {
        //     data_dir_with_app_role.join(&server_yml.host)
        // };

        let mut server = Server::new(self.mini_app_conf.clone(), my_dir, server_yml)?;

        server.lock_working_file()?;

        if let Some(bl) = self.mini_app_conf.buf_len {
            server.server_yml.buf_len = bl;
        }

        let ab = server_yml_path.canonicalize()?;
        trace!("server_yml_path: {:?}", server_yml_path);
        server.yml_location.replace(ab);

        trace!(
            "loaded server, directory pairs, [from_dir, to_dir]: {:?}",
            server
                .server_yml
                .directories
                .iter()
                .map(|dir| format!("{}, {}", dir.from_dir, dir.to_dir))
                .collect::<Vec<String>>()
        );
        Ok(server)
    }

    #[allow(dead_code)]
    fn get_log_file(data_dir: &Path, inner: &AppConfYml) -> String {
        let log_file = &inner.log_conf.log_file;
        let path = Path::new(log_file);
        if path.is_absolute() {
            log_file.clone()
        } else {
            data_dir
                .join(path)
                .to_str()
                .expect("log_file should be a valid string.")
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use std::process::Command;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::app_conf"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_app_conf_deserd() -> Result<(), failure::Error> {
        let yml = r##"---
app_instance_id: demo-app-instance-id
archive_cmd: 
  - C:/Program Files/7-Zip/7z.exe
  - a
  - archive_file_name
  - files_and_dirs
data_dir: data
log_conf:
  log_file: output.log
  verbose_modules: []
    # - data_shape::server
mail_conf:
  from: xxx@gmail.com
  username: xxx@gmail.com
  password: password
  hostname: xxx.example.com
  port: 587"##;
        let app_conf_yml = serde_yaml::from_str::<AppConfYml>(&yml)?;
        assert_eq!(
            app_conf_yml.archive_cmd,
            vec![
                "C:/Program Files/7-Zip/7z.exe",
                "a",
                "archive_file_name",
                "files_and_dirs"
            ]
        );

        log();
        // create a directory of 3 files.
        let a_file = "a_file.tar";
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20130101010155.tar", "abc")?;
        t_dir.make_a_file_with_content(a_file, "abc")?;
        t_dir.make_a_file_with_content("b.tar", "abc")?;

        let t_dir_name = t_dir.tmp_dir_str();

        let target_dir = tutil::TestDir::new();

        let archive_path = target_dir.tmp_dir_path().join("aa.7z");
        let archive_file_name = archive_path
            .to_str()
            .expect("archive name to str should success.");

        let archive_cmd = app_conf_yml
            .archive_cmd
            .iter()
            .map(|s| {
                if s == "archive_file_name" {
                    archive_file_name.to_owned()
                } else if s == "files_and_dirs" {
                    t_dir_name.to_owned()
                } else {
                    s.to_owned()
                }
            })
            .collect::<Vec<String>>();

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
        eprintln!("output: {:?}", output);
        assert!(
            archive_path.metadata()?.len() > 0,
            "archived aa.7z should have a length great than 0."
        );
        Ok(())
    }
}
