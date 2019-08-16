use log::*;
use ssh2::{self, Session};
use std::net::TcpStream;
use std::path::Path;

pub fn create_connected_session(
    host_url: &str,
    username: &str,
    id_rsa: &str,
    id_rsa_pub: Option<&str>,
) -> (TcpStream, Session) {
    let tcp = TcpStream::connect(host_url).expect("tpc connect should success.");
    let mut sess = Session::new().expect("new session should created.");
    sess.handshake(&tcp).expect("handshake should success.");

    info!("{:?}", sess.auth_methods(username).expect("should print auth_methods."));
    sess.userauth_pubkey_file(
        &username,
        id_rsa_pub.map(|p| Path::new(p)),
        Path::new(id_rsa),
        None,
    )
    .expect("login should success.");
    assert!(sess.authenticated());
    (tcp, sess)
}