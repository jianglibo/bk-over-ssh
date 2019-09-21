use crate::data_shape::Server;
use crate::ioutil::{SharedMpb};
use log::*;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc};
use std::{fs, io::Read, io::Write};

pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";

#[derive(Debug, Deserialize, Serialize)]
pub struct LogConf {
    pub log_file: String,
    pub verbose_modules: Vec<String>,
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
pub struct AppConf {
    data_dir: String,
    log_conf: LogConf,
    pub mail_conf: MailConf,
    #[serde(skip_deserializing)]
    pub config_file_path: Option<PathBuf>,
    #[serde(skip_deserializing)]
    data_dir_full_path: Option<PathBuf>,
    #[serde(skip_deserializing)]
    log_full_path: Option<PathBuf>,
}

impl AppConf {
    fn read_app_conf(file: impl AsRef<Path>) -> Result<Option<AppConf>, failure::Error> {
        if !file.as_ref().exists() {
            return Ok(None);
        }
        let file = file.as_ref();
        if let Ok(mut f) = fs::OpenOptions::new().read(true).open(file) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                match serde_yaml::from_str::<AppConf>(&buf) {
                    Ok(mut app_conf) => {
                        app_conf.config_file_path.replace(file.to_path_buf());
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
    #[allow(dead_code)]
    pub fn write_to_working_dir(&self) -> Result<(), failure::Error> {
        let ymld = serde_yaml::to_string(self)?;
        let path = env::current_dir()?.join(CONF_FILE_NAME);
        let mut file = fs::OpenOptions::new().write(true).create(true).open(path)?;
        write!(file, "{}", ymld)?;
        Ok(())
    }

    /// If no conf file provided, first look at the same directory as execuable, then current working directory.
    pub fn guess_conf_file(app_conf_file: Option<&str>) -> Result<Option<AppConf>, failure::Error> {
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

    pub fn get_data_dir(&self) -> &Path {
        self.data_dir_full_path
            .as_ref()
            .map(|pb| pb.as_path())
            .expect("data_dir_full_path should always available.")
    }

    pub fn validate_conf(&mut self) -> Result<(), failure::Error> {
        if self.data_dir.trim().is_empty() {
            self.data_dir = "data".to_string();
        } else {
            self.data_dir = self.data_dir.trim().to_string();
        }

        let mut path_buf = Path::new(&self.data_dir).to_path_buf();
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
            Ok(ab) => self.data_dir_full_path.replace(ab),
            Err(err) => bail!("path_buf {:?} canonicalize failed: {:?}", &path_buf, err),
        };

        self.log_full_path
            .replace(Path::new(&self.get_log_file()).to_path_buf());

        let servers_dir = self.get_servers_dir();
        if !servers_dir.exists() {
            if let Err(err) = fs::create_dir_all(&servers_dir) {
                bail!("create servers_dir {:?}, failed: {:?}", &servers_dir, err);
            }
        }
        Ok(())
    }

    pub fn get_servers_dir(&self) -> PathBuf {
        self.get_data_dir().join("servers")
    }

    pub fn load_server_yml(
        &self,
        yml_file_name: impl AsRef<str>,
        buf_len: Option<usize>,
        multi_bar: Option<SharedMpb>,
    ) -> Result<Server, failure::Error> {
        let server =
            Server::load_from_yml_with_app_config(&self, yml_file_name.as_ref(), buf_len, multi_bar)?;
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
    ) -> Vec<Server> {
        if let Ok(rd) = self.get_servers_dir().read_dir() {
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
            .map(|astr| self.load_server_yml(astr, buf_len, multi_bar.as_ref().map(Arc::clone)))
            .filter_map(|rr| match rr {
                Err(err) => {
                    warn!("load_server_yml failed: {:?}", err);
                    None
                }
                Ok(server) => Some(server),
            })
            .collect()
        } else {
            warn!("read_dir failed: {:?}", self.get_servers_dir());
            Vec::new()
        }
    }

    pub fn get_log_conf(&self) -> &LogConf {
        &self.log_conf
    }

    pub fn get_log_file(&self) -> String {
        let path = Path::new(&self.log_conf.log_file);
        if path.is_absolute() {
            self.log_conf.log_file.clone()
        } else {
            self.get_data_dir()
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
        let app_conf = serde_yaml::from_str::<AppConf>(&yml)?;
        assert_eq!(app_conf.get_servers_dir(), Path::new("abc"));

        let ymld = serde_yaml::to_string(&app_conf)?;
        println!("{}", ymld);

        assert_eq!(yml, ymld);
        Ok(())
    }
}
