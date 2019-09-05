#[cfg(test)]
mod tests {
    use super::super::super::log_util;
    use crate::actions::{copy_stream_to_file_return_sha1};
    use failure;
    use log::*;
    use ssh2::{self, Session};
    use std::io::prelude::*;
    use std::net::TcpStream;
    use std::path::Path;
    use walkdir::WalkDir;
    use crate::develope::tutil;
    use crate::data_shape::{Server};

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
    fn t_scp_file() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();

        let mut server = Server::load_from_yml("localhost")?;
        let sess = server.get_ssh_session();
        let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024*1024*4)?;
        let file = test_dir.tmp_file_str();
        let (mut remote_file, stat) = sess
            .scp_recv(Path::new(&file))
            .unwrap();
        println!("remote file size: {}", stat.size());
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents).unwrap();
        info!("{:?}", contents);
        Ok(())
    }

    #[test]
    fn t_sftp_file() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let mut server = Server::load_from_yml("localhost")?;
        let sess = server.get_ssh_session();
        let test_dir =tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024*1024*4)?; 
        let file = test_dir.tmp_file_str();
        let sftp = sess.sftp().expect("should got sfpt instance.");

        let mut file: ssh2::File =
            sftp.open(Path::new(&file))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        assert_eq!(buf, "hello\nworld");
        assert_eq!(buf.len(), 11);
        info!("{:?}", buf);
        Ok(())
    }

    #[test]
    fn t_sftp_resume_file() -> Result<(), failure::Error> {
        // log_util::setup_logger(vec![""], vec![]);
        // let (_tcp, mut sess, _dev_env) = develope_data::connect_to_ubuntu();
        // let rdo = RemoteFileItemDirOwned::load_dir("fixtures/adir");
        // let rd: RemoteFileItemDir = (&rdo).into();
        // let remote_item = rd
        //     .get_items()
        //     .iter()
        //     .find(|ri| ri.get_path().ends_with("鮮やか"))
        //     .expect("must have at least one.");
        // let file_item = FileItem::new(Path::new("not_in_git"), &remote_item);
        // let file_item = copy_a_file_item(&mut sess, file_item);
        // info!("{:?}", file_item);
        // assert_eq!(file_item.get_len(), 11);
        // assert_eq!(file_item.get_len(), file_item.remote_item.get_len());
        // assert_eq!(file_item.remote_item.get_sha1(), file_item.get_sha1());
        // assert_eq!(file_item.get_path(), Some("aa.txt".to_string()));
        Ok(())
    }

    #[test]
    fn t_channel_1() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();

        let mut server = Server::load_from_yml("localhost")?;
        let sess = server.get_ssh_session();
        let mut channel: ssh2::Channel = sess.channel_session().unwrap();
        channel.exec("ls").unwrap();
        copy_stream_to_file_return_sha1(&mut channel, "not_in_git/t.txt")?;
        Ok(())
    }

    #[test]
    fn t_walkdir() {
        let base_path = Path::new("f:/迅雷下载")
            .canonicalize()
            .expect("should open dir to walk.");
        WalkDir::new(&base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| d.path().canonicalize().ok())
            .filter_map(|d| {
                d.strip_prefix(&base_path).ok().map(|d| d.to_path_buf())
                // .map(|dd| dd.to_str().map(|s| s.to_string()))
            })
            .for_each(|d| println!("{:?}", d.to_str()));
        // assert_eq!(WalkDir::new("f:/").into_iter().filter_map(|e| e.ok()).count(), 33);
    }

    #[test]
    fn t_components() {
        let p = Path::new("./fixtures/a/b b/")
            .canonicalize()
            .expect("success");
        println!("{:?}", p);

        let rp = Path::new("fixtures").canonicalize().expect("success");

        let pp = p.strip_prefix(rp).expect("success");

        println!("{:?}", pp.as_os_str());
    }

    #[test]
    fn t_env_base() -> Result<(), failure::Error> {
        use std::env;
        let c_path = env::current_dir()?;
        let e_path = env::current_exe()?;
        assert_eq!(c_path, e_path);
        Ok(())
    }
}
