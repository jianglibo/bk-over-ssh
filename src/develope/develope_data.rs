use serde::{Deserialize, Serialize};

use log::*;
use ssh2::{self, Session};
use std::fs::File;
use std::{io::BufReader, io, io::BufRead, io::Seek};
use std::net::TcpStream;
use std::path::Path;

#[allow(dead_code)]
pub fn get_a_cursor_writer() -> io::Cursor<Vec<u8>> {
        let v = Vec::<u8>::new();
        io::Cursor::new(v)
}

#[allow(dead_code)]
pub fn count_cursor_lines(mut cursor: io::Cursor<Vec<u8>>) -> usize {
    cursor.seek(io::SeekFrom::Start(0)).unwrap();
    io::BufReader::new(cursor).lines().count()
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SshClientParams {
    pub id_rsa: String,
    pub id_rsa_pub: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TestDirs {
    pub aatxt: String,
    pub linux_remote_item_dir: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TestFiles {
    pub big_binary_file: String,
    pub midum_binary_file: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub host: String,
    pub username: String,
    pub test_dirs: TestDirs,
    pub test_files: TestFiles,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Servers {
    pub ubuntu18: Server,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct DevelopeEnv {
    pub ssh_client: SshClientParams,
    pub servers: Servers,
}

#[allow(dead_code)]
pub fn load_env() -> DevelopeEnv {
    let file = File::open("develope_env.yaml")
        .expect("develope_env.yaml should exist in project root folder.");
    let buf_reader = BufReader::new(file);
    serde_yaml::from_reader(buf_reader).expect("load develope_data should success.")
}

#[allow(dead_code)]
pub fn create_connected_session(
    server: &Server,
    ssh_client: &SshClientParams,
) -> (TcpStream, Session) {
    info!("{:?}", server);
    let tcp = TcpStream::connect(&server.host).expect("tpc connect should success.");
    let mut sess = Session::new().expect("new session should created.");
    sess.handshake(&tcp).expect("handshake should success.");

    info!("{:?}", sess.auth_methods(&server.username).expect("should print auth_methods."));
    sess.userauth_pubkey_file(
        &server.username,
        ssh_client.id_rsa_pub.as_ref().map(|p| Path::new(p)),
        Path::new(&ssh_client.id_rsa),
        None,
    )
    .expect("login should success.");
    assert!(sess.authenticated());
    (tcp, sess)
}

#[allow(dead_code)]
pub fn connect_to_ubuntu() -> (TcpStream, Session, DevelopeEnv) {
    let env = load_env();
    let ts = create_connected_session(&env.servers.ubuntu18, &env.ssh_client);
    (ts.0, ts.1, env)
}
