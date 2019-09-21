#[cfg(test)]
mod tests {
    use super::super::super::log_util;
    use crate::actions::copy_stream_to_file_return_sha1;
    use crate::data_shape::Server;
    use crate::develope::tutil;
    use askama::Template;
    use failure;
    use ssh2::{self, Session};
    use std::io::prelude::*;
    use std::net::TcpStream;
    use std::path::Path;
    use walkdir::WalkDir; // bring trait in scope

    #[derive(Template)] // this will generate the code...
    #[template(path = "hello.html")] // using the template in this path, relative
    struct HelloTemplate<'a> {
        // the name of the struct can be anything
        name: &'a str, // the field name should match the variable name
                       // in your template
    }

    #[derive(Template)]
    #[template(source = "hello.html", ext = "txt")]
    struct SourceTpl1<'a> {
        name: &'a str,
    }

    #[derive(Template)]
    #[template(source = "{{ name }}", ext = "txt")]
    struct SourceTpl2<'a> {
        name: &'a str,
    }

    #[test]
    fn t_source_tpl1() {
        let hello = SourceTpl1 { name: "world" };
        assert_eq!(hello.render().unwrap(), "hello.html");
    }

    #[test]
    fn t_source_tpl2() {
        let hello = SourceTpl2 { name: "world" };
        assert_eq!(hello.render().unwrap(), "world");
    }

    #[derive(Template)]
    #[template(source = "{% if 1 == 1 %} {{ name }} {% endif %}", ext = "txt")]
    struct SourceTpl3<'a> {
        name: &'a str,
    }

    #[test]
    fn t_source_tpl3() {
        let hello = SourceTpl3 { name: "world" };
        assert_eq!(hello.render().unwrap(), " world ");
    }

    #[derive(Template)]
    #[template(source = "{% if 1 == 1 -%} {{ name }} {%- endif %}", ext = "txt")]
    struct SourceTpl4<'a> {
        name: &'a str,
    }

    #[test]
    fn t_source_tpl4() {
        let hello = SourceTpl4 { name: "world" };
        assert_eq!(hello.render().unwrap(), "world");
    }

    #[test]
    fn t_hello_html() {
        let hello = HelloTemplate { name: "world" }; // instantiate your struct
        assert_eq!(hello.render().unwrap(), "Hello, world!");
    }

    #[test]
    fn t_main_password() {
        // Connect to the local SSH server
        let _tcp = TcpStream::connect("127.0.0.1:22").unwrap();
        let sess = Session::new().unwrap();
        // sess.handshake(&tcp).unwrap();

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

    fn load_server_yml() -> Server {
        Server::load_from_yml("data/servers", "data", "localhost.yml", None, None).unwrap()
    }

    #[test]
    fn t_scp_file() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();

        let mut server = load_server_yml();
        let sess = server.get_ssh_session();
        let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024)?;
        let file = test_dir.tmp_file_str()?;
        let (mut remote_file, stat) = sess.scp_recv(Path::new(&file)).unwrap();
        println!("remote file size: {}", stat.size());
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents).unwrap();
        assert_eq!(stat.size(), 1024);
        assert_eq!(contents.len(), 1024);
        Ok(())
    }

    #[test]
    fn t_sftp_file() -> Result<(), failure::Error> {
        log_util::setup_test_logger_only_self(vec!["develope::knowledge"]);
        let mut server = load_server_yml();
        let sess = server.get_ssh_session();
        let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024)?;
        let file = test_dir.tmp_file_str()?;
        let sftp = sess.sftp().expect("should got sfpt instance.");

        let mut file: ssh2::File = sftp.open(Path::new(&file))?;
        let mut buf = Vec::<u8>::new();
        file.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 1024);
        Ok(())
    }

    #[test]
    fn t_sftp_resume_file() -> Result<(), failure::Error> {
        // log_util::setup_logger(vec![""], vec![]);
        // let (_tcp, mut sess, _dev_env) = develope_data::connect_to_ubuntu();
        // let rdo = RemoteFileItemOwnedDirOwned::load_dir("fixtures/adir");
        // let rd: RemoteFileItemOwnedDir = (&rdo).into();
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

        let mut server = load_server_yml();
        let sess = server.get_ssh_session();
        let mut channel: ssh2::Channel = sess.channel_session().unwrap();
        channel.exec("ls").unwrap();
        copy_stream_to_file_return_sha1(&mut channel, "not_in_git/t.txt", 8192, None)?;
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

    #[test]
    fn t_dereference_destructure() -> Result<(), failure::Error> {
        let mut mut_value = 3;

        match mut_value {
            ref mut r => {
                *r += 1;
                println!("{}", r);
            }
        }
        let x = Some("foo".to_string());
        x.map(|_| ()).ok_or_else(|| failure::err_msg("abc"))
    }

    #[test]
    fn t_lettre() -> Result<(), failure::Error> {
        use lettre::smtp::authentication::{Credentials, Mechanism};
        use lettre::smtp::extension::ClientId;
        use lettre::smtp::ConnectionReuseParameters;
        use lettre::{SmtpClient, Transport};
        use lettre_email::{mime::TEXT_PLAIN, Email};
        use std::path::Path;

        let email = Email::builder()
            // Addresses can be specified by the tuple (email, alias)
            .to(("jianglibo@hotmail.com", "Firstname Lastname"))
            // ... or by an address only
            .from("jlbfine@qq.com")
            .subject("Hi, Hello world")
            .text("Hello world.")
            .attachment_from_file(Path::new("Cargo.toml"), None, &TEXT_PLAIN)
            .unwrap()
            .build()
            .unwrap();

        // Open a local connection on port 25
        // let mut mailer = SmtpClient::new_unencrypted_localhost().unwrap().transport();
        let mut mailer = SmtpClient::new_simple("smtp.qq.com")
            .unwrap()
            // Set the name sent during EHLO/HELO, default is `localhost`
            // .hello_name(ClientId::Domain("my.hostname.tld".to_string()))
            // Add credentials for authentication
            .credentials(Credentials::new(
                "jlbfine@qq.com".to_string(),
                "emnbsygyqacibgjh".to_string(),
            ))
            // Enable SMTPUTF8 if the server supports it
            .smtp_utf8(true)
            // Configure expected authentication mechanism
            .authentication_mechanism(Mechanism::Plain)
            // Enable connection reuse
            .connection_reuse(ConnectionReuseParameters::ReuseUnlimited)
            .transport();

        // Send the email
        let result = mailer.send(email.into());

        if result.is_ok() {
            println!("Email sent");
        } else {
            println!("Could not send email: {:?}", result);
        }

        assert!(result.is_ok());
        Ok(())
    }
}

#[cfg(test)]
mod tests1 {
    use rusqlite::types::ToSql;
    use rusqlite::{Connection, Result, NO_PARAMS};
    use time::Timespec;

    #[derive(Debug)]
    struct Person {
        id: i32,
        name: String,
        time_created: Timespec,
        data: Option<Vec<u8>>,
    }

    #[test]
    fn t_sqlite_() -> Result<()> {
        let conn = Connection::open_in_memory()?;

        conn.execute(
            "CREATE TABLE person (
                  id              INTEGER PRIMARY KEY,
                  name            TEXT NOT NULL,
                  time_created    TEXT NOT NULL,
                  data            BLOB
                  )",
            NO_PARAMS,
        )?;
        let me = Person {
            id: 0,
            name: "Steven".to_string(),
            time_created: time::get_time(),
            data: None,
        };
        conn.execute(
            "INSERT INTO person (name, time_created, data)
                  VALUES (?1, ?2, ?3)",
            &[&me.name as &ToSql, &me.time_created, &me.data],
        )?;

        let mut stmt = conn.prepare("SELECT id, name, time_created, data FROM person")?;
        let person_iter = stmt.query_map(NO_PARAMS, |row| {
            Ok(Person {
                id: row.get(0)?,
                name: row.get(1)?,
                time_created: row.get(2)?,
                data: row.get(3)?,
            })
        })?;

        for person in person_iter {
            println!("Found person {:?}", person.unwrap());
        }
        Ok(())
    }
}
