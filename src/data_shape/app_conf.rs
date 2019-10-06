use crate::data_shape::{string_path, Indicator, Server, ServerYml};
use crate::db_accesses::DbAccess;
use indicatif::MultiProgress;
use log::{trace, warn};
use serde::{Deserialize, Serialize};
use std::env;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io::Read, io::Write};

pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";

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

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum AppRole {
    Controller,
    Leaf,
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
    data_dir: String,
    log_conf: LogConf,
    pub mail_conf: MailConf,
    role: AppRole,
}

impl Default for AppConfYml {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
            role: AppRole::Controller,
            mail_conf: MailConf::default(),
            log_conf: LogConf::default(),
        }
    }
}

fn guesss_data_dir(data_dir: impl AsRef<str>) -> Result<PathBuf, failure::Error> {
    let data_dir = data_dir.as_ref();
    let data_dir = if data_dir.is_empty() {
        "data"
    } else {
        data_dir
    };

    let mut path_buf = Path::new(data_dir).to_path_buf();

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
    pub skip_cron: bool,
    pub skip_sha1: bool,
}

#[derive(Debug, Serialize)]
pub struct AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    inner: AppConfYml,
    pub config_file_path: PathBuf,
    pub data_dir_full_path: PathBuf,
    pub log_full_path: PathBuf,
    pub servers_dir: PathBuf,
    #[serde(skip)]
    pub db_access: Option<D>,
    #[serde(skip)]
    _m: PhantomData<M>,
    #[serde(skip)]
    lock_file: Option<fs::File>,
    #[serde(skip)]
    pub progress_bar: Option<Arc<MultiProgress>>,
    pub mini_app_conf: MiniAppConf,
}

pub fn demo_app_conf<M, D>(data_dir: &str) -> AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    AppConf {
        inner: AppConfYml::default(),
        config_file_path: Path::new("abc").to_path_buf(),
        data_dir_full_path: PathBuf::from(data_dir),
        log_full_path: PathBuf::from(data_dir).join("out.log"),
        servers_dir: PathBuf::from("data").join("servers"),
        _m: PhantomData,
        db_access: None,
        lock_file: None,
        progress_bar: None,
        mini_app_conf: MiniAppConf {
            skip_sha1: true,
            skip_cron: false,
            buf_len: None,
        },
    }
}

impl<M, D> AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub fn set_db_access(&mut self, db_access: D) {
        self.db_access.replace(db_access);
    }

    pub fn get_sqlite_db_file(&self) -> PathBuf {
        self.data_dir_full_path.join("db.db")
    }

    #[allow(dead_code)]
    pub fn get_inner(&self) -> &AppConfYml {
        &self.inner
    }
    pub fn get_db_access(&self) -> Option<&D> {
        self.db_access.as_ref()
    }

    pub fn skip_cron(&mut self) {
        self.mini_app_conf.skip_cron = true;
    }

    pub fn not_skip_sha1(&mut self) {
        self.mini_app_conf.skip_sha1 = false;
    }

    fn read_app_conf(file: impl AsRef<Path>) -> Result<Option<AppConf<M, D>>, failure::Error> {
        if !file.as_ref().exists() {
            return Ok(None);
        }
        let file = file.as_ref();
        if let Ok(mut f) = fs::OpenOptions::new().read(true).open(file) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                match serde_yaml::from_str::<AppConfYml>(&buf) {
                    Ok(app_conf_yml) => {
                        let data_dir_full_path = guesss_data_dir(app_conf_yml.data_dir.trim())?;

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
                        let servers_dir = data_dir_full_path.as_path().join("servers");

                        if !servers_dir.exists() {
                            if let Err(err) = fs::create_dir_all(&servers_dir) {
                                bail!("create servers_dir {:?}, failed: {:?}", &servers_dir, err);
                            }
                        }

                        let app_conf = AppConf {
                            inner: app_conf_yml,
                            config_file_path: file.to_path_buf(),
                            data_dir_full_path,
                            log_full_path,
                            servers_dir,
                            db_access: None,
                            _m: PhantomData,
                            lock_file: None,
                            progress_bar: None,
                            mini_app_conf: MiniAppConf {
                                skip_sha1: true,
                                skip_cron: false,
                                buf_len: None,
                            },
                        };
                        Ok(Some(app_conf))
                    }
                    Err(err) => bail!("deserialize failed: {:?}, {:?}", file, err),
                }
            } else {
                bail!("read_to_string failure: {:?}", file);
            }
        } else {
            bail!("open conf file failed: {:?}", file);
        }
    }

    pub fn get_mail_conf(&self) -> &MailConf {
        &self.inner.mail_conf
    }
    #[allow(dead_code)]
    pub fn write_to_working_dir(&self) -> Result<(), failure::Error> {
        let ymld = serde_yaml::to_string(&self.inner)?;
        let path = env::current_dir()?.join(CONF_FILE_NAME);
        let mut file = fs::OpenOptions::new().write(true).create(true).open(path)?;
        write!(file, "{}", ymld)?;
        Ok(())
    }

    /// If no conf file provided, first look at the same directory as execuable, then current working directory.
    pub fn guess_conf_file(
        app_conf_file: Option<&str>,
    ) -> Result<Option<AppConf<M, D>>, failure::Error> {
        if let Some(af) = app_conf_file {
            return AppConf::read_app_conf(af);
        } else {
            if let Ok(current_exe) = env::current_exe() {
                if let Some(pp) = current_exe.parent() {
                    let cf = pp.join(CONF_FILE_NAME);
                    trace!("found configuration file: {:?}", &cf);
                    if let Some(af) = AppConf::read_app_conf(&cf)? {
                        // if it returned None, continue searching.
                        return Ok(Some(af));
                    }
                }
            }

            if let Ok(current_dir) = env::current_dir() {
                let cf = current_dir.join(CONF_FILE_NAME);
                trace!("found configuration file: {:?}", &cf);
                return AppConf::read_app_conf(&cf);
            }
        }
        bail!("read app_conf failed.")
    }

    pub fn lock_working_file(&mut self) -> Result<(), failure::Error> {
        let lof = self.data_dir_full_path.as_path().join("working.lock");
        if lof.exists() {
            if fs::remove_file(lof.as_path()).is_err() {
                eprintln!("create lock file failed: {:?}, if you can sure app isn't running, you can delete it manually.", lof);
            }
        } else {
            self.lock_file
                .replace(fs::OpenOptions::new().write(true).create(true).open(lof)?);
        }
        Ok(())
    }

    pub fn load_server_yml(
        &self,
        yml_file_name: impl AsRef<str>,
    ) -> Result<(Server<M, D>, Indicator), failure::Error> {
        let server = self.load_server_from_yml(yml_file_name.as_ref())?;
        eprintln!(
            "load server yml from: {}",
            server
                .yml_location
                .as_ref()
                .map_or("O", |b| b.to_str().unwrap_or("O"))
        );
        let indicator = Indicator::new(self.progress_bar.clone());
        Ok((server, indicator))
    }

    pub fn load_all_server_yml(&self) -> Vec<(Server<M, D>, Indicator)> {
        if let Ok(rd) = self.servers_dir.read_dir() {
            rd.filter_map(|ery| match ery {
                Err(err) => {
                    warn!("read_dir entry return error: {:?}", err);
                    None
                }
                Ok(entry) => Some(entry.file_name().into_string()),
            })
            .filter_map(|ossr| match ossr {
                Err(err) => {
                    warn!("osstring to_string failed: {:?}", err);
                    None
                }
                Ok(astr) => Some(astr),
            })
            .map(|astr| self.load_server_yml(astr))
            .filter_map(|rr| match rr {
                Err(err) => {
                    warn!("load_server_yml failed: {:?}", err);
                    None
                }
                Ok(server) => Some(server),
            })
            .collect()
        } else {
            warn!("read_dir failed: {:?}", self.servers_dir);
            Vec::new()
        }
    }

    pub fn get_log_conf(&self) -> &LogConf {
        &self.inner.log_conf
    }

    pub fn load_server_from_yml(
        &self,
        name: impl AsRef<str>,
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
            if !(name.contains('/') || name.contains('\\')) {
                server_yml_path = self.servers_dir.as_path().join(name);
            }
            if !server_yml_path.exists() {
                bail!("server yml file does't exist: {:?}", server_yml_path);
            }
        }
        let mut f = fs::OpenOptions::new().read(true).open(&server_yml_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let server_yml: ServerYml = serde_yaml::from_str(&buf)?;

        let data_dir = self.data_dir_full_path.as_path();
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

        // let multi_bar = self.progress_bar.clone();

        let mut server = Server::new(
            self.mini_app_conf.clone(),
            server_yml,
            self.db_access.clone(),
            report_dir,
            tar_dir,
            working_dir,
        );
        //  {
        //     server_yml,
        //     multi_bar,
        //     db_access: self.db_access.clone(),
        //     // pb: Indicator::new(multi_bar.as_ref().cloned()),
        //     report_dir,
        //     session: None,
        //     tar_dir,
        //     working_dir,
        //     yml_location: None,
        //     app_conf: &self,
        //     _m: PhantomData,
        // };

        if let Some(bl) = self.mini_app_conf.buf_len {
            server.server_yml.buf_len = bl;
        }

        // if let Some(mb) = server.multi_bar.as_ref() {
        //     let pb_total = ProgressBar::new(!0);
        //     let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
        //     pb_total.set_style(ps);
        //     let pb_total = mb.add(pb_total);

        //     let pb_item = ProgressBar::new(!0);
        //     let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
        //     pb_item.set_style(ps);
        //     let pb_item = mb.add(pb_item);

        //     server.pb = Some((pb_total, pb_item));
        // }

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

                if d.local_dir.starts_with(string_path::VERBATIM_PREFIX) {
                    trace!("local dir start with VERBATIM_PREFIX, stripping it.");
                    d.local_dir = d.local_dir.split_at(4).1.to_string();
                }
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

    #[test]
    fn t_app_conf_deserd() -> Result<(), failure::Error> {
        let yml = r##"---
servers_dir: abc"##;
        let app_conf = serde_yaml::from_str::<AppConfYml>(&yml)?;
        // assert_eq!(app_conf.get_servers_dir(), Path::new("abc"));

        let ymld = serde_yaml::to_string(&app_conf)?;
        eprintln!("{}", ymld);

        assert_eq!(yml, ymld);
        Ok(())
    }
}
