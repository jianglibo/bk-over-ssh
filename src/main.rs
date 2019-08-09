#[macro_use]
extern crate derive_builder;
mod log_util;
mod develope_data;
mod data_shape;
mod actions;
use actions::write_to_file;

use std::fs::OpenOptions;
use std::io::prelude::*;
    use sha2::{Digest, Sha224};
    use sha1::{Sha1, Digest as Digest1};
    use std::{fs, io};
    use std::time::Instant;

use std::borrow::Cow::{self, Borrowed, Owned};

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::config::OutputStreamType;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::{Cmd, CompletionType, Config, Context, EditMode, Editor, Helper, KeyPress};

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

struct MyHelper {
    completer: FilenameCompleter,
    highlighter: MatchingBracketHighlighter,
    hinter: HistoryHinter,
    colored_prompt: String,
}

impl Completer for MyHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for MyHelper {
    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for MyHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned("\x1b[1m".to_owned() + hint + "\x1b[m")
    }

    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize) -> bool {
        self.highlighter.highlight_char(line, pos)
    }
}

impl Helper for MyHelper {}

fn hash_file_2(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let start = Instant::now();

    let mut hasher = DefaultHasher::new();

    let mut file = fs::File::open(file_name.as_ref())?;
    let mut buffer = [0; 1024];
    let mut total = 0_usize;
    loop {
        let n = file.read(&mut buffer[..])?;
        if n == 0 {
            break
        } else {
            hasher.write(&buffer[..n]);
            total += n;
        }
    }
    let hash = hasher.finish();
    println!("Bytes processed: {}", total);
    let r = format!("{:x}", hash);
    println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
    Ok(r)
}

fn hash_file_1(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let start = Instant::now();
    let mut file = fs::File::open(file_name.as_ref())?;
    let mut hasher = Sha224::new();
    let mut buffer = [0; 1024];
    let mut total = 0_usize;
    loop {
        let n = file.read(&mut buffer[..])?;
        if n == 0 {
            break
        } else {
            hasher.input(&buffer[..n]);
            total += n;
        }
    }
    let hash = hasher.result();
    println!("Bytes processed: {}", total);
    let r = format!("{:x}", hash);
    println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
    Ok(r)
}

fn hash_file_3(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let start = Instant::now();
    let mut file = fs::File::open(file_name.as_ref())?;
    let mut hasher = Sha1::new();
    let n = io::copy(&mut file, &mut hasher)?;
    let hash = hasher.result();
    println!("Bytes processed: {}", n);
    let r = format!("{:x}", hash);
    println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
    Ok(r)
}

fn hash_file(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let start = Instant::now();
    let mut file = fs::File::open(file_name.as_ref())?;
    let mut hasher = Sha224::new();
    let n = io::copy(&mut file, &mut hasher)?;
    let hash = hasher.result();
    println!("Bytes processed: {}", n);
    let r = format!("{:x}", hash);
    println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
    Ok(r)
}

fn main() {
    // env_logger::init();
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .output_stream(OutputStreamType::Stdout)
        .build();

    let h = MyHelper {
        completer: FilenameCompleter::new(),
        highlighter: MatchingBracketHighlighter::new(),
        hinter: HistoryHinter {},
        colored_prompt: "".to_owned(),
    };
    let mut rl = Editor::with_config(config);
    rl.set_helper(Some(h));
    rl.bind_sequence(KeyPress::Meta('N'), Cmd::HistorySearchForward);
    rl.bind_sequence(KeyPress::Meta('P'), Cmd::HistorySearchBackward);
    if rl.load_history("history.txt").is_err() {
        println!("No previous history.");
    }
    let mut count = 1;
    loop {
        let p = format!("{}> ", count);
        rl.helper_mut().unwrap().colored_prompt = format!("\x1b[1;32m{}\x1b[0m", p);
        let readline = rl.readline(&p);
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                println!("Line: {}", line);
                if line.starts_with("hash ") {
                    let (_s1, s2) = line.split_at(5);
                    hash_file_3(s2).expect("hash should success.");
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
        count += 1;
    }
    rl.save_history("history.txt").unwrap();
}



#[cfg(test)]
mod tests {

    use super::*;
    use failure;
    use log::*;
    use ssh2::{self, Session};
    use std::fs::OpenOptions;
    use std::io::prelude::*;
    use std::net::TcpStream;
    use std::path::Path;



    #[test]
    fn t_digest_file() -> Result<(), failure::Error> {
        let f = r"E:\backups\mysql\10.19.183.53\mysqls\mysql.27\hm-log-bin.000502";
        let r1 = hash_file(f)?;
        let r2 = hash_file_1(f)?;
        let r3 = hash_file_2(f)?;
        let r4 = hash_file_2(f)?;
        let r5 = hash_file_3(f)?;
        let r6 = hash_file_3(f)?;
        // let _r3 = hash_file(r"E:\data_20190429.tar.gz");
        assert_eq!(r1, r2);
        assert_eq!(r3, r4);
        assert_eq!(r5, r6);
        Ok(())
    }

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
