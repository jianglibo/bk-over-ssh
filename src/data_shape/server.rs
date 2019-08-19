use serde::{Deserialize};
use std::{fs, io};
use std::io::prelude::Read;
use log::*;

#[derive(Debug, Deserialize)]
pub struct Directory {
    pub remote_dir: String,
    pub local_dir: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub id_rsa: String,
    pub id_rsa_pub: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub directories: Vec<Directory>,
}

pub fn load_server(name: impl AsRef<str>) -> Result<Server, failure::Error> {
    let _name = name.as_ref();

    let full_name = if _name.ends_with(".yml") {
        _name.to_string()
    } else {
        format!("{}.yml", _name)
    };
    let mut server_path = std::env::current_exe()?.parent().expect("executable's parent should exists.").join("servers").join(&full_name);
    if !server_path.exists() {
        server_path = std::env::current_dir()?.join("servers").join(&full_name);
    }
    info!("loading server configuration: {:?}", server_path);
    let mut f = fs::OpenOptions::new().read(true).open(server_path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    info!("server content: {:?}", buf);
    Ok(serde_yaml::from_str(&buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let server = load_server("localhost")?;
        assert_eq!(server.port, 2222_u16);
        assert_eq!(server.directories[0].excludes, vec!["*.log".to_string(), "*.bak".to_string()]);
        Ok(())
    }
}

