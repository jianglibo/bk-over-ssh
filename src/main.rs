mod log_util;
use std::fs::OpenOptions;
use std::io::prelude::*;


fn main() {
    println!("Hello, world!");
}

fn write_to_file<T: AsRef<str>>(from: &mut impl std::io::Read, to_file: T) -> Result<(), failure::Error> {
    let mut u8_buf = [0; 1024];
    let mut wf = OpenOptions::new().create(true).write(true).open(to_file.as_ref())?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                wf.write(&u8_buf[..n])?;
                // println!("The bytes: {:?}", &u8_buf[..n]);
            }
            _ => break
        }
    }    
    Ok(())
}

#[cfg(test)]
mod tests {

    use ssh2::{Session, self};
    use std::net::TcpStream;
    use std::path::Path;
    use log::*;
    use super::*;
    use std::io::prelude::*;
    use failure;
    use std::fs::OpenOptions;


    const RSA_PUB: &str = "C:/Users/Administrator/.ssh/i51.pub";
    const RSA_PRI: &str = "C:/Users/Administrator/.ssh/i51";

    #[test]
    fn t_main_password() {
        // Connect to the local SSH server
        let tcp = TcpStream::connect("127.0.0.1:22").unwrap();
        let mut sess = Session::new().unwrap();
        sess.handshake(&tcp).unwrap();

        sess.userauth_password("administrator", "apassword")
            .unwrap();
        assert!(sess.authenticated());
    }

    fn create_connected_session() -> (TcpStream, Session) {
        // Connect to the local SSH server
        let tcp = TcpStream::connect("10.19.183.51:8122").unwrap();
        let mut sess = Session::new().unwrap();
        sess.handshake(&tcp).unwrap();

        info!("{:?}", sess.auth_methods("root").unwrap());
        sess.userauth_pubkey_file("root", Some(Path::new(RSA_PUB)), Path::new(RSA_PRI), None).expect("login success.");
        assert!(sess.authenticated());
        (tcp, sess)
    }

    #[test]
    fn t_main_pubkey() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (_, sess) = create_connected_session();
        assert!(sess.authenticated());
    }

    #[test]
    fn t_main_agent_inspect() {
        // Almost all APIs require a `Session` to be available
        let sess = Session::new().unwrap();
        let mut agent = sess.agent().unwrap();

        // Connect the agent and request a list of identities
        agent.connect().unwrap();
        agent.list_identities().unwrap();

        for identity in agent.identities() {
            let identity = identity.unwrap(); // assume no I/O errors
            println!("{}", identity.comment());
            let pubkey = identity.blob();
            println!("{:?}", pubkey);
        }
    }
    #[test]
    fn t_scp_file() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (tcp, sess) = create_connected_session();
        info!("{:?}", tcp);
        let (mut remote_file, stat) = sess.scp_recv(Path::new("/root/t.txt")).unwrap();
        println!("remote file size: {}", stat.size());
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents).unwrap();
        info!("{:?}", contents);
    }

    #[test]
    fn t_sftp_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (tcp, sess) = create_connected_session();
        info!("{:?}", tcp);
        let sftp = sess.sftp().expect("should got sfpt instance.");

        let mut file: ssh2::File = sftp.open(Path::new("/root/t.txt"))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        info!("{:?}", buf);
        Ok(())
    }

    #[test]
    fn t_channel_1() -> Result<(), failure::Error>  {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");

        let (_tcp, sess) = create_connected_session();
        let mut channel: ssh2::Channel = sess.channel_session().unwrap();
        channel.exec("ls").unwrap();

        write_to_file(&mut channel, "not_in_git/t.txt")?;
        Ok(())
    }
}
