#[macro_use]
extern crate derive_builder;
#[macro_use]
extern crate failure;

#[macro_use]
extern crate clap;

extern crate rand;
// extern crate rustsync;

mod actions;
mod data_shape;
mod develope;
mod log_util;
mod rustsync;

use crate::rustsync::DeltaWriter;
use std::borrow::Cow::{self, Borrowed, Owned};

use clap::App;
use clap::ArgMatches;
use clap::Shell;
use log::*;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::config::OutputStreamType;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::{Cmd, CompletionType, Config, Context, EditMode, Editor, Helper, KeyPress};
use std::env;
use std::time::Instant;
use std::{fs, io, io::Write};

use data_shape::{AppConf, Server, CONF_FILE_NAME};

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

fn main_client() {
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
                    actions::hash_file_sha1(s2).expect("hash should success.");
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

fn main() -> Result<(), failure::Error> {

    let yml = load_yaml!("17_yaml.yml");
    let app = App::from_yaml(yml);
    let m: ArgMatches = app.get_matches();

    let mut app1 = App::from_yaml(yml);

    let conf = m.value_of("conf");

    let mut app_conf = match AppConf::guess_conf_file(conf) {
        Ok(cfg) => {
            if let Some(cfg) = cfg {
                cfg
            } else {
                let bytes = include_bytes!("app_config_demo.yml");
                let path = env::current_dir()?.join(CONF_FILE_NAME);
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&path)?;
                file.write_all(bytes)?;
                bail!("cann't find app configuration file and had created one for you in the current directory. {:?}", path);
            }
        }
        Err(err) => {
            bail!("read app configuration file failed: {:?}", err);
        }
    };

    app_conf.validate_conf()?;
    log_util::setup_logger_for_this_app(true, app_conf.get_log_file().as_str(), &app_conf.get_log_conf().verbose_modules)?;

    if m.is_present("print-conf") {
        println!("{:?}", app_conf);
        return Ok(());
    }

    match m.subcommand() {
        ("completions", Some(sub_matches)) => {
            let shell = sub_matches.value_of("shell_name").unwrap();
            app1.gen_completions_to(
                "ssh-client-demo",
                shell.parse::<Shell>().unwrap(),
                &mut io::stdout(),
            );
        }
        ("rsync", Some(sub_matches)) => {
            match sub_matches.subcommand() {
                ("sync-dirs", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches.value_of("server-yml").unwrap();
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");
                    match Server::load_from_yml_with_app_config(&app_conf, server_config_path) {
                        Ok(mut server) => {
                            match server.sync_dirs(skip_sha1) {
                                Ok(result) => {
                                    actions::write_dir_sync_result(&server, &result);
                                    println!("{:?}", result);
                                },
                                Err(err) => error!("sync-dirs failed: {:?}", err),
                            }
                        }
                        Err(err) => {
                            error!("load_from_yml failed: {:?}", err);
                        }
                    }
                }
                ("deploy-to-server", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches.value_of("server-yml").unwrap();
                    match Server::load_from_yml_with_app_config(&app_conf, server_config_path) {
                        Ok(server) => {
                            if ["127.0.0.1", "localhost"]
                                .iter()
                                .any(|&s| s == server.host.as_str())
                            {
                                bail!("no need to copy to self.");
                            }
                            // let rp = server.remote_server_yml.clone();
                            // let lpb = app_conf.get_servers_dir()
                            // let lp = lpb.to_str().expect("server yml should exists.");
                            // if let Err(err) = server.copy_a_file(lp, rp) {
                            //     bail!("copy_a_file failed: {:?}", err);
                            // }
                            // It's useless because of different of platform.
                            // if let Ok(current_exe) = env::current_exe() {
                            //     let rp = server.remote_exec.clone();
                            //     let lp = current_exe.to_str().expect("current_exe PathBuf to_str should success.");
                            //     server.copy_a_file(lp, rp)?;
                            // }
                        }
                        Err(err) => {
                            error!("load_from_yml failed: {:?}", err);
                        }
                    }
                }
                ("restore-a-file", Some(sub_sub_matches)) => {
                    let old_file = sub_sub_matches.value_of("old-file").unwrap();
                    let maybe_delta_file = sub_sub_matches.value_of("delta-file");
                    let maybe_out_file = sub_sub_matches.value_of("out-file");
                    let delta_file = if let Some(f) = maybe_delta_file {
                        f.to_string()
                    } else {
                        format!("{}.delta", old_file)
                    };

                    let out_file = if let Some(f) = maybe_out_file {
                        f.to_string()
                    } else {
                        format!("{}.restore", old_file)
                    };

                    let mut dr =
                        rustsync::DeltaFileReader::<fs::File>::read_delta_file(delta_file)?;
                    dr.restore_from_file_to_file(out_file, old_file)?;
                }
                ("delta-a-file", Some(sub_sub_matches)) => {
                    let new_file = sub_sub_matches.value_of("new-file").unwrap();
                    let maybe_sig_file = sub_sub_matches.value_of("sig-file");
                    let maybe_out_file = sub_sub_matches.value_of("out-file");
                    let sig_file = if let Some(f) = maybe_sig_file {
                        f.to_string()
                    } else {
                        format!("{}.sig", new_file)
                    };

                    let out_file = if let Some(f) = maybe_out_file {
                        f.to_string()
                    } else {
                        format!("{}.delta", new_file)
                    };

                    let sig = rustsync::Signature::load_signature_file(sig_file)?;

                    let new_file_input = fs::OpenOptions::new().read(true).open(new_file)?;
                    rustsync::DeltaFileWriter::<fs::File>::create_delta_file(
                        out_file, sig.window, None,
                    )?
                    .compare(&sig, new_file_input)?;
                }
                ("signature", Some(sub_sub_matches)) => {
                    let file = sub_sub_matches.value_of("file").unwrap();
                    let block_size: Option<usize> = sub_sub_matches
                        .value_of("block-size")
                        .and_then(|s| s.parse().ok());
                    let sig_file = format!("{}.sig", file);
                    let out = sub_sub_matches
                        .value_of("out")
                        .unwrap_or_else(|| sig_file.as_str());
                    let start = Instant::now();
                    match rustsync::Signature::signature_a_file(file, block_size) {
                        Ok(mut sig) => {
                            if let Err(err) = sig.write_to_file(out) {
                                error!("rsync signature write_to_file failed: {:?}", err);
                            }
                        }
                        Err(err) => {
                            error!("rsync signature failed: {:?}", err);
                        }
                    }
                    println!("time costs: {:?}", start.elapsed().as_secs());
                }
                ("list-remote-files", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches
                        .value_of("server-yml")
                        .expect("should load sever yml.");
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");
                    let start = Instant::now();
                    let mut server = Server::load_from_yml_with_app_config(&app_conf, server_config_path)?;

                    if let Some(out) = sub_sub_matches.value_of("out") {
                        let mut out = fs::OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .open(out)?;
                        server.list_remote_file_exec(&mut out, skip_sha1)?;
                    } else {
                        server.list_remote_file_exec(&mut io::stdout(), skip_sha1)?;
                    }

                    println!("time costs: {:?}", start.elapsed().as_secs());
                }
                ("list-local-files", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches
                        .value_of("server-yml")
                        .expect("should load server yml.");
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");

                    let server = Server::load_from_yml_with_app_config(&app_conf, server_config_path)?;

                    if let Some(out) = sub_sub_matches.value_of("out") {
                        let mut out = fs::OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .open(out)?;
                        server.load_dirs(&mut out, skip_sha1)?;
                    } else {
                        server.load_dirs(&mut io::stdout(), skip_sha1)?;
                    }
                }
                (_, _) => {
                    println!("please add --help to view usage help.");
                }
            }
        }
        ("repl", Some(_)) => {
            main_client();
        }
        (_, _) => unimplemented!(), // for brevity
    }
    Ok(())

    // if let Some(mode) = m.value_of("mode") {
    //     match mode {
    //         "vi" => println!("You are using vi"),
    //         "emacs" => println!("You are using emacs..."),
    //         _      => unreachable!()
    //     }
    // } else {
    //     println!("--mode <MODE> wasn't used...");
    // }
    // main_client();
}
