use crate::data_shape::Server;
use crate::ioutil::SharedMpb;
use crate::db_accesses::{DbAccess};
use log::{trace, warn};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io::Read, io::Write};
use std::marker::PhantomData;

pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";

#[derive(Debug, Deserialize, Serialize)]
pub struct LogConf {
    pub log_file: String,
    verbose_modules: Vec<String>,
}

impl LogConf {
    pub fn get_verbose_modules(&self) -> &Vec<String> {
        &self.verbose_modules
    }
}

#[derive(Debug, Deserialize, Serialize)]
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
    pub db_access: Option<D>,
    _m: PhantomData<M>,
}

impl<M, D> AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub fn set_db_access(&mut self, db_access: D) {
        self.db_access.replace(db_access);
    }

    pub fn get_inner(&self) -> &AppConfYml {
        &self.inner
    }
    pub fn get_db_access(&self) -> Option<&D> {
        self.db_access.as_ref()
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

    pub fn lock_working_file(&self) -> Result<(), failure::Error> {
        let lof = self.data_dir_full_path.as_path().join("working.lock");
        if lof.exists() {
            bail!("create lock file failed: {:?}, if you can sure app isn't running, you can delete it manually.", lof);
        } else {
            fs::OpenOptions::new().write(true).create(true).open(lof)?;
        }
        Ok(())
    }
    pub fn unlock_working_file(&self) {
        let lof = self.data_dir_full_path.as_path().join("working.lock");
        if lof.exists() {
            if let Err(err) = fs::remove_file(lof.as_path()) {
                warn!("remove lock file failed: {:?}, {:?}", lof, err);
            }
        }
    }

    // pub fn create_sqlite_pool(&self) {
    //     let db_file = self.data_dir_full_path.as_path().join("db.db");
    //     create_sqlite_pool(db_file)
    // }

    pub fn load_server_yml(
        &self,
        yml_file_name: impl AsRef<str>,
        buf_len: Option<usize>,
        multi_bar: Option<SharedMpb>,
    ) -> Result<Server<M, D>, failure::Error> {
        let server = Server::<M, D>::load_from_yml_with_app_config(
            &self,
            yml_file_name.as_ref(),
            buf_len,
            multi_bar,
        )?;
        println!(
            "load server yml from: {}",
            server
                .yml_location
                .as_ref()
                .map_or("O", |b| b.to_str().unwrap_or("O"))
        );
        Ok(server)
    }

    pub fn load_all_server_yml(
        &self,
        buf_len: Option<usize>,
        multi_bar: Option<SharedMpb>,
    ) -> Vec<Server<M, D>> {
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
            .map(|astr| {
                self.load_server_yml(
                    astr,
                    buf_len,
                    multi_bar.as_ref().map(Arc::clone),
                )
            })
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
        println!("{}", ymld);

        assert_eq!(yml, ymld);
        Ok(())
    }
}
