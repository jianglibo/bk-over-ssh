#[cfg(test)]
mod tests {
    use super::super::super::log_util;
    use super::super::develope_data;
    use crate::actions::{copy_a_file, write_to_file};
    use crate::data_shape::{FileItem, RemoteFileItem};
    use failure;
    use log::*;
    use ssh2::{self, Session};
    use std::ffi::OsStr;
    use std::io::prelude::*;
    use std::net::TcpStream;
    use std::path::Path;

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

    #[test]
    fn t_main_pubkey() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (tcp, sess, dev_env) = develope_data::connect_to_ubuntu();
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
        let (tcp, sess, dev_env) = develope_data::connect_to_ubuntu();
        info!("{:?}", tcp);
        let (mut remote_file, stat) = sess
            .scp_recv(Path::new(&dev_env.servers.ubuntu18.test_dirs.aatxt))
            .unwrap();
        println!("remote file size: {}", stat.size());
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents).unwrap();
        info!("{:?}", contents);
    }

    #[test]
    fn t_sftp_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (tcp, sess, dev_env) = develope_data::connect_to_ubuntu();
        info!("{:?}", tcp);
        let sftp = sess.sftp().expect("should got sfpt instance.");

        let mut file: ssh2::File =
            sftp.open(Path::new(&dev_env.servers.ubuntu18.test_dirs.aatxt))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        assert_eq!(buf, "hello\nworld\n");
        assert_eq!(buf.len(), 12);
        info!("{:?}", buf);
        Ok(())
    }

    #[test]
    fn t_sftp_resume_file() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let (_tcp, mut sess, dev_env) = develope_data::connect_to_ubuntu();
        let file_item = FileItem::new(RemoteFileItem::new(dev_env.servers.ubuntu18.test_dirs.aatxt.as_str()));
            // FileItemBuilder::default()
            // .sha1("58853E8A5E8272B1012F9A52A80758B27BD0D3CB")
            // .remote_path(dev_env.servers.ubuntu18.test_dirs.aatxt.as_str())
            // .len(12_u64)
            // .build()
            // .expect("should create file item.");
        let file_item = copy_a_file(&mut sess, file_item);
        info!("{:?}", file_item);
        assert_eq!(file_item.len, 12);
        assert_eq!(file_item.len, file_item.remote_item.len);
        assert_eq!(file_item.remote_item.sha1.map(str::to_string), file_item.sha1);
        assert_eq!(file_item.get_local_path(), Some(OsStr::new("aa.txt")));
        Ok(())
    }

    #[test]
    fn t_channel_1() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");

        let (_tcp, sess, _dev_env) = develope_data::connect_to_ubuntu();
        let mut channel: ssh2::Channel = sess.channel_session().unwrap();
        channel.exec("ls").unwrap();
        write_to_file(&mut channel, "not_in_git/t.txt")?;
        Ok(())
    }

    #[test]
    fn t_load_env() {
        let develope_env = develope_data::load_env();
        assert!(develope_env
            .servers
            .ubuntu18
            .test_dirs
            .aatxt
            .contains("aa.txt"));
    }
}
