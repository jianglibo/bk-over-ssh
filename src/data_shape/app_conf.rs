use log::*;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::{fs, io::Read, io::Write};

pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";

#[derive(Debug, Deserialize, Serialize)]
pub struct LogConf {
    pub console: bool,
    pub log_file: String,
    pub verbose_modules: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConf {
    data_dir: String,
    log_conf: LogConf,
}

// fn guess_servers_dir() -> Result<PathBuf, failure::Error> {
//     info!(
//         "try to find servers_dir in current_exe: {:?}",
//         env::current_exe()
//     );
//     if let Ok(current_exe) = env::current_exe() {
//         if let Some(current_dir) = current_exe.parent() {
//             let p = current_dir.join("servers");
//             if p.exists() {
//                 info!("found servers_dir: {:?}", p);
//                 return Ok(p);
//             }
//         }
//     }

//     info!(
//         "try to find servers_dir in current_dir: {:?}",
//         env::current_dir()
//     );
//     if let Ok(current_dir) = env::current_dir() {
//         let p = current_dir.join("servers");
//         if p.exists() {
//             info!("found servers_dir: {:?}", p);
//             return Ok(p);
//         }
//     }

//     bail!("cannot find servers_dir configuration item.");
// }

impl AppConf {
    fn read_app_conf(file: impl AsRef<Path>) -> Result<Option<AppConf>, failure::Error> {
        if !file.as_ref().exists() {
            return Ok(None);
        }
        if let Ok(mut f) = fs::OpenOptions::new().read(true).open(file.as_ref()) {
            let mut buf = String::new();
            if let Ok(_) = f.read_to_string(&mut buf) {
                match serde_yaml::from_str::<AppConf>(&buf) {
                    Ok(app_conf) => return Ok(Some(app_conf)),
                    Err(err) => bail!("deserialize failed: {:?}, {:?}", file.as_ref(), err),
                }
            } else {
                bail!("read_to_string failure: {:?}", file.as_ref());
            }
        } else {
            bail!("open conf file failed: {:?}", file.as_ref());
        }
    }

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
                    if let Ok(Some(af)) = AppConf::read_app_conf(pp.join(CONF_FILE_NAME)) {
                        return Ok(Some(af));
                    }
                }
            }

            if let Ok(current_dir) = env::current_dir() {
                let cf = current_dir.join(CONF_FILE_NAME);
                return AppConf::read_app_conf(cf);
            }
        }
        bail!("read app_conf failed.")
    }

    /// default to "data" folder in the working directory.
    pub fn get_data_dir(&self) -> &str {
        &self.data_dir
    }

    pub fn validate_conf(&mut self) -> Result<(), failure::Error> {
        if self.data_dir.trim().is_empty() {
            self.data_dir = "data".to_string();
        } else {
            self.data_dir = self.data_dir.trim().to_string();
        }

        let path = Path::new(&self.data_dir);
        if !path.exists() {
            fs::create_dir_all(path)?;
        }

        let servers_dir = self.get_servers_dir();
        if !servers_dir.exists() {
            fs::create_dir_all(servers_dir)?;
        }

        Ok(())
    }

    pub fn get_servers_dir(&self) -> PathBuf {
        Path::new(&self.data_dir).join("servers")
    }

    pub fn get_log_conf(&self) -> &LogConf {
        &self.log_conf
    }

    pub fn get_log_file(&self) -> String {
        let path = Path::new(&self.log_conf.log_file);
        if path.is_absolute() {
            self.log_conf.log_file.clone()
        } else {
            Path::new(&self.data_dir)
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
