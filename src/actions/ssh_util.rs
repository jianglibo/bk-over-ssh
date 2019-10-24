use log::*;
use ssh2;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;

fn get_sess_pre_authentication(url: &str) -> Result<ssh2::Session, failure::Error> {
    trace!("connecting to: {}", url);
    let tcp = TcpStream::connect(&url)?;
    let mut sess = ssh2::Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;
    Ok(sess)
}

pub fn create_ssh_session_agent(
    url: &str,
    username: &str,
) -> Result<ssh2::Session, failure::Error> {
    let sess = get_sess_pre_authentication(url)?;
    let mut agent = sess.agent()?;
    agent.connect()?;
    agent.list_identities()?;

    for id in agent.identities() {
        match id {
            Ok(identity) => {
                trace!("start authenticate with public key.");
                if let Err(err) = agent.userauth(username, &identity) {
                    warn!("ssh agent authentication failed. {:?}", err);
                } else {
                    break;
                }
            }
            Err(err) => warn!("can't get key from ssh agent {:?}.", err),
        }
    }
    Ok(sess)
}

pub fn create_ssh_session_identity_file(
    url: &str,
    username: &str,
    id_rsa: &str,
    id_rsa_pub: Option<&str>,
) -> Result<ssh2::Session, failure::Error> {
    let sess = get_sess_pre_authentication(url)?;
    trace!(
        "about authenticate to {:?} with IdentityFile: {:?}",
        url,
        id_rsa_pub,
    );
    sess.userauth_pubkey_file(
        username,
        id_rsa_pub.as_ref().map(Path::new),
        Path::new(id_rsa),
        None,
    )
    .expect("userauth_pubkey_file should succeeded.");
    Ok(sess)
}
pub fn create_ssh_session_password(
    url: &str,
    username: &str,
    password: &str,
) -> Result<ssh2::Session, failure::Error> {
    let sess = get_sess_pre_authentication(url)?;
    sess.userauth_password(username, password)
        .expect("userauth_password should succeeded.");
    Ok(sess)
}

pub fn get_stdout_eprintln_stderr(
    channel: &mut ssh2::Channel,
    eprint_stdout: bool,
) -> (String, String) {
    let mut s = String::new();
    let std_out = if let Err(err) = channel.read_to_string(&mut s) {
        eprintln!("read channel stdout failure: {:?}", err);
        "".to_string()
    } else {
        if eprint_stdout {
            eprintln!("std_out: {}", s);
        }
        s
    };

    let mut s = String::new();
    let std_err = if let Err(err) = channel.stderr().read_to_string(&mut s) {
        eprintln!("read channel stderr failure: {:?}", err);
        "".to_string()
    } else {
        eprintln!("std_err: {}", s);
        s
    };

    (std_out, std_err)
}
