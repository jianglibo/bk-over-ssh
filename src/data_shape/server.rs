use crate::actions::copy_a_file_item;
use crate::data_shape::{FileItemLine, RemoteFileItemLine};
use log::*;
use serde::Deserialize;
use ssh2;
use std::io::prelude::{BufRead, Read};
use std::net::TcpStream;
use std::path::Path;
use std::{fs, io};

#[derive(Deserialize)]
pub struct Directory {
    pub remote_dir: String,
    pub local_dir: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
}

#[derive(Deserialize)]
pub struct Server {
    pub id_rsa: String,
    pub id_rsa_pub: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub directories: Vec<Directory>,
    pub file_list_file: Option<String>,
    #[serde(skip)]
    _tcp_stream: Option<TcpStream>,
    #[serde(skip)]
    session: Option<ssh2::Session>,
}

impl Server {
    pub fn is_connected(&self) -> bool {
        self._tcp_stream.is_some() && self.session.is_some()
    }
    pub fn connect(&mut self) -> Result<(), failure::Error> {
        let url = format!("{}:{}", self.host, self.port);
        let tcp = TcpStream::connect(&url)?;
        if let Some(mut sess) = ssh2::Session::new() {
            sess.handshake(&tcp)?;
            sess.userauth_pubkey_file(
                &self.username,
                Some(Path::new(&self.id_rsa_pub)),
                Path::new(&self.id_rsa),
                None,
            )?;
            self._tcp_stream.replace(tcp);
            self.session.replace(sess);
        } else {
            bail!("Session::new failed.");
        }
        Ok(())
    }

    pub fn start_sync<R: Read>(&mut self, file_item_lines: Option<R>) -> Result<(), failure::Error> {
        if let Some(r) = file_item_lines {
            self._start_sync(r)
        } else if let Some(url) = self.file_list_file.as_ref().cloned() {
            if self.session.is_none() {
                bail!("please connect the server first.");
            }
        let sftp = self.session.as_mut().unwrap().sftp()?;
        // need write to a tmp file.
        let file: ssh2::File =  sftp.open(Path::new(&url))?;
            self._start_sync(file)
        } else {
            bail!("no file_list_file.");
        }
    }

    fn _start_sync<R: Read>(&mut self, file_item_lines: R) -> Result<(), failure::Error> {
        if self.session.is_none() {
            bail!("please connect the server first.");
        }
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        for line_r in io::BufReader::new(file_item_lines).lines() {
            match line_r {
                Ok(line) => {
                    if current_local_dir.is_none() {
                        if let Some(rd) = self.directories.iter().find(|d| d.remote_dir == line) {
                            current_remote_dir = Some(line);
                            current_local_dir = Some(Path::new(rd.local_dir.as_str()));
                        }
                    } else {
                        // let current_remote_file = current_remote_dir + line;
                        // let current_local_file = current_local_dir + line;
                        match serde_json::from_str::<RemoteFileItemLine>(&line) {
                            Ok(remote_item) => {
                                let local_item = FileItemLine::new(
                                    current_local_dir.unwrap(),
                                    current_remote_dir.as_ref().unwrap().as_str(),
                                    &remote_item,
                                );
                                copy_a_file_item(self.session.as_mut().unwrap(), local_item);
                            }
                            Err(err) => {
                                error!("deserialize line failed: {:?}, {:?}", line, err);
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("read line failed: {:?}", err);
                }
            }
        }
        Ok(())
    }
}

pub fn sync_dirs<P: AsRef<str>, R: Read>(name: P, from: Option<R>) -> Result<(), failure::Error> {
    let mut server = load_server(name)?;
    server.connect()?;
    server.start_sync(from)?;
    Ok(())
}

pub fn load_server(name: impl AsRef<str>) -> Result<Server, failure::Error> {
    let _name = name.as_ref();

    let full_name = if _name.ends_with(".yml") {
        _name.to_string()
    } else {
        format!("{}.yml", _name)
    };
    let mut server_path = std::env::current_exe()?
        .parent()
        .expect("executable's parent should exists.")
        .join("servers")
        .join(&full_name);
    if !server_path.exists() {
        server_path = std::env::current_dir()?.join("servers").join(&full_name);
    }
    info!("loading server configuration: {:?}", server_path);
    let mut f = fs::OpenOptions::new().read(true).open(server_path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
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
        assert_eq!(
            server.directories[0].excludes,
            vec!["*.log".to_string(), "*.bak".to_string()]
        );
        Ok(())
    }

    #[test]
    fn t_connect_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let mut server = load_server("localhost")?;
        server.connect()?;
        assert!(server.is_connected());
        Ok(())
    }

    #[test]
    fn t_download_dirs() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let d = Path::new("target/adir");
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
        assert!(!d.exists());
        let mut server = load_server("localhost")?;
        server.connect()?;
        let f = fs::OpenOptions::new().read(true).open("fixtures/linux_remote_item_dir.txt")?;
        server.start_sync(f)?;

        assert!(d.exists());
        Ok(())
    }

}
