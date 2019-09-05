use log::*;
use std::env;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct LogConf {
    pub console: bool,
    pub log_file_name: String,
    pub verbose_modules: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConf {
    servers_dir: PathBuf,
    log_conf: Option<LogConf>,
}

fn guess_servers_dir() -> Result<PathBuf, failure::Error> {
    info!("try to find servers_dir in current_exe: {:?}", env::current_exe());
        if let Ok(current_exe) = env::current_exe() {
        if let Some(current_dir) = current_exe.parent() {
            let p = current_dir.join("servers");
            if p.exists() {
                info!("found servers_dir: {:?}", p);
                return Ok(p);
            }
        }
    }

    info!("try to find servers_dir in current_dir: {:?}", env::current_dir());
    if let Ok(current_dir) = env::current_dir() {
        let p = current_dir.join("servers");
        if p.exists() {
            info!("found servers_dir: {:?}", p);
            return Ok(p);
        }
    }



    bail!("cannot find servers_dir configuration item.");
}

impl AppConf {
    pub fn new(servers_dir_op: Option<impl AsRef<str>>) -> Result<Self, failure::Error> {
        let servers_dir = if let Some(servers_dir) = servers_dir_op {
            info!("got servers_dir from parameter: {}", servers_dir.as_ref());
            Path::new(servers_dir.as_ref()).to_path_buf()
        } else {
            guess_servers_dir()?
        };
        Ok(Self {
            servers_dir,
            log_conf: None,
        })
    }

    pub fn get_servers_dir(&self) -> &Path {
        self.servers_dir.as_path()
    }

    pub fn get_log_conf(&self) -> Option<&LogConf> {
        self.log_conf.as_ref()
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