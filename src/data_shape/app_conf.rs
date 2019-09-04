use log::*;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct AppConf {
    servers_dir: PathBuf,
}

fn guess_servers_dir() -> Result<PathBuf, failure::Error> {
    info!("try to find servers_dir in current_dir");
    let p = if let Ok(current_dir) = env::current_dir() {
        let p = current_dir.join("servers");
        if p.exists() {
            info!("found servers_dir: {:?}", p);
            p
        } else {
            Path::new("").to_path_buf()
        }
    } else {
        Path::new("").to_path_buf()
    };
    Ok(p)
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
            servers_dir: servers_dir,
        })
    }

    pub fn get_servers_dir(&self) -> &Path {
        self.servers_dir.as_path()
    }
}
