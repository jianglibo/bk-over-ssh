use serde::{Deserialize, Serialize};

use failure;
use log::*;
use ssh2::{self, Session};
use std::fs::File;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::TcpStream;
use std::path::Path;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SshClientParams {
    pub id_rsa: String,
    pub id_rsa_pub: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TestDirs {
    pub aatxt: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub host: String,
    pub username: String,
    pub test_dirs: TestDirs,
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
    let tcp = TcpStream::connect(&server.host).unwrap();
    let mut sess = Session::new().unwrap();
    sess.handshake(&tcp).unwrap();

    info!("{:?}", sess.auth_methods(&server.username).unwrap());
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
