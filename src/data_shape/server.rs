use crate::actions::copy_a_file_item;
use crate::data_shape::{load_remote_item_owned, FileItemLine, RemoteFileItemLine};
use log::*;
use serde::Deserialize;
use ssh2;
use std::io::prelude::{BufRead, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::{fs, io};
use tempfile;
use glob::Pattern;

#[derive(Deserialize, Default)]
pub struct Directory {
    pub remote_dir: String,
    pub local_dir: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    #[serde(skip)]
    pub includes_patterns: Option<Vec<Pattern>>,
    #[serde(skip)]
    pub excludes_patterns: Option<Vec<Pattern>>,
}

impl Directory {
    /// if has exclude items any matching will return None,
    /// if has include items, only matched item will return.
    /// if neither exclude nor include, mathes.
    pub fn match_path(&self, path: PathBuf) -> Option<PathBuf> {
        let mut no_exlucdes = false;
        if let Some(exclude_ptns) = self.excludes_patterns.as_ref() {
            if exclude_ptns.iter().any(|p|p.matches_path(&path)) {
                return None;
            }
        } else {
            no_exlucdes = true;
        }

        if let Some(include_ptns) = self.includes_patterns.as_ref() {
            if include_ptns.is_empty() {
                return Some(path);
            }
            if include_ptns.iter().any(|p|p.matches_path(&path)) {
                return Some(path);
            }
        } else if no_exlucdes {
            return Some(path);
        }
        None
    }

    pub fn compile_patterns(&mut self) -> Result<(), failure::Error> {
        if self.includes_patterns.is_none() {
            self.includes_patterns.replace(self.includes.iter().map(|s|Pattern::new(s).unwrap()).collect());
        }

        if self.excludes_patterns.is_none() {
            self.excludes_patterns.replace(self.excludes.iter().map(|s|Pattern::new(s).unwrap()).collect());
        }

        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Server {
    pub id_rsa: String,
    pub id_rsa_pub: String,
    pub host: String,
    pub port: u16,
    pub remote_exec: String,
    pub remote_server_yml: String,
    pub username: String,
    pub directories: Vec<Directory>,
    pub file_list_file: Option<String>,
    #[serde(skip)]
    _tcp_stream: Option<TcpStream>,
    #[serde(skip)]
    session: Option<ssh2::Session>,
}

impl Server {
    pub fn load_from_yml(name: impl AsRef<str>) -> Result<Server, failure::Error> {
        let name = name.as_ref();

        let server_path = if name.contains('\\') || name.contains('/') {
            Path::new(name).to_path_buf()
        } else {
            let name_only = if !name.ends_with(".yml") {
                format!("{}.yml", name)
            } else {
                name.to_string()
            };
            let sp = std::env::current_exe()?
                .parent()
                .expect("executable's parent should exists.")
                .join("servers")
                .join(&name_only);

            if sp.exists() {
                sp
            } else {
                std::env::current_dir()?.join("servers").join(&name_only)
            }
        };

        info!("loading server configuration: {:?}", server_path);
        let mut f = fs::OpenOptions::new().read(true).open(server_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let mut server: Server = serde_yaml::from_str(&buf)?;
        server.directories.iter_mut().for_each(|d|d.compile_patterns().unwrap());
        Ok(server)
    }

    pub fn is_connected(&self) -> bool {
        self._tcp_stream.is_some() && self.session.is_some()
    }

    pub fn get_ssh_session(&self) -> Result<(TcpStream, ssh2::Session), failure::Error> {
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
            Ok((tcp, sess))
        } else {
            bail!("Session::new failed.");
        }
    }

    pub fn connect(&mut self) -> Result<(), failure::Error> {
        let (tcp, sess) = self.get_ssh_session()?;
        self._tcp_stream.replace(tcp);
        self.session.replace(sess);
        Ok(())
    }

    fn get_file_list_file(&self, url: impl AsRef<str>) -> Result<impl io::Read, failure::Error> {
        let mut channel: ssh2::Channel = self.session.as_ref().unwrap().channel_session().unwrap();
        let cmd = format!(
            "{} rsync list-files --server-yml {}",
            self.remote_exec, self.remote_server_yml
        );
        channel.exec(cmd.as_str()).unwrap();
        let mut tmp = tempfile::tempfile()?;
        io::copy(&mut channel, &mut tmp)?;
        Ok(tmp)
    }

    pub fn start_sync<R: Read>(&self, file_item_lines: Option<R>) -> Result<(), failure::Error> {
        ensure!(self.is_connected(), "please connect the server first.");
        if let Some(r) = file_item_lines {
            self._start_sync(r)
        } else if let Some(url) = self.file_list_file.as_ref().cloned() {
            // let tmp = self.get_file_list_file(url)?;
            let sftp = self.session.as_ref().unwrap().sftp()?;
            let sftp_file: ssh2::File = sftp.open(Path::new(&url))?;
            self._start_sync(sftp_file)
        } else {
            bail!("no file_list_file.");
        }
    }

    fn _start_sync<R: Read>(&self, file_item_lines: R) -> Result<(), failure::Error> {
        if self.session.is_none() {
            bail!("please connect the server first.");
        }
        let mut current_remote_dir = Option::<String>::None;
        let mut current_local_dir = Option::<&Path>::None;
        for line_r in io::BufReader::new(file_item_lines).lines() {
            match line_r {
                Ok(line) => {
                    info!("got line: {:?}", line);
                    if current_local_dir.is_none() {
                        if let Some(rd) = self.directories.iter().find(|d| d.remote_dir == line) {
                            current_remote_dir = Some(line);
                            current_local_dir = Some(Path::new(rd.local_dir.as_str()));
                        }
                    } else {
                        let sftp = self.session.as_ref().unwrap().sftp()?;
                        match serde_json::from_str::<RemoteFileItemLine>(&line) {
                            Ok(remote_item) => {
                                let local_item = FileItemLine::new(
                                    current_local_dir.unwrap(),
                                    current_remote_dir.as_ref().unwrap().as_str(),
                                    &remote_item,
                                );
                                copy_a_file_item(&sftp, local_item);
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

    pub fn sync_dirs(&mut self) -> Result<(), failure::Error> {
        self.connect()?;
        self.start_sync(Option::<fs::File>::None)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn sync_file_item_lines<R: Read>(&mut self, from: R) -> Result<(), failure::Error> {
        self.connect()?;
        self.start_sync(Some(from))?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn load_dirs<'a, O: io::Write>(
        &self,
        out: &'a mut O,
        skip_sha1: bool,
    ) -> Result<(), failure::Error> {
        for one_dir in self.directories.iter() {
            load_remote_item_owned(one_dir, out, skip_sha1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;

    #[test]
    fn t_load_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let server = Server::load_from_yml("localhost")?;
        assert_eq!(
            server.directories[0].excludes,
            vec!["*.log".to_string(), "*.bak".to_string()]
        );
        Ok(())
    }

    #[test]
    fn t_connect_server() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let mut server = Server::load_from_yml("localhost")?;
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
        let mut server = Server::load_from_yml("localhost")?;
        server.connect()?;
        let f = fs::OpenOptions::new()
            .read(true)
            .open("fixtures/linux_remote_item_dir.txt")?;
        server.start_sync(Some(f))?;

        assert!(d.exists());
        Ok(())
    }

    #[test]
    fn t_sync_dirs() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let d = Path::new("target/adir");
        if d.exists() {
            fs::remove_dir_all(d)?;
        }
        assert!(!d.exists());
        let mut server = Server::load_from_yml("localhost")?;
        server.sync_dirs()?;
        assert!(d.exists());
        Ok(())
    }

}
