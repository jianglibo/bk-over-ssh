use serde::{Serialize, Deserialize};

    use std::fs::OpenOptions;
    use std::io::prelude::*;
    use std::fs::File;
    use std::io::BufReader;
    use std::net::TcpStream;
    use std::path::Path;
    use failure;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SshClient {
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
    pub ssh_client: SshClient,
    pub servers: Servers,
}

pub fn load_env() -> DevelopeEnv {
    let file = File::open("develope_env.yaml").expect("develope_env.yaml should exist in project root folder.");
    let mut buf_reader = BufReader::new(file);
    serde_yaml::from_reader(buf_reader).expect("load develope_data should success.")
}