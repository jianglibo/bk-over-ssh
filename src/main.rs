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
mod ioutil;
mod log_util;
mod rustsync;

use crate::rustsync::DeltaWriter;
use std::borrow::Cow::{self, Borrowed, Owned};

use clap::App;
use clap::ArgMatches;
use clap::Shell;
use log::*;
use pbr::{MultiBar, ProgressBar, Units};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::config::OutputStreamType;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::{Cmd, CompletionType, Config, Context, EditMode, Editor, Helper, KeyPress};
use std::env;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, io, io::Write};
use rayon::prelude::*;

use data_shape::{AppConf, MyPb, Server, CONF_FILE_NAME};

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

fn parse_server_yml<'a>(sub_sub_matches: &'a clap::ArgMatches<'a>) -> &'a str {
    sub_sub_matches.value_of("server-yml").unwrap()
}

fn demonstrate_pbr() -> Result<(), failure::Error> {
    let count = 1000;
    let mut pb = ProgressBar::new(count);
    pb.format("╢▌▌░╟");
    for _ in 0..count {
        pb.inc();
        thread::sleep(Duration::from_millis(200));
    }
    pb.finish_print("done");
    Ok(())
}

fn process_app_config(conf: Option<&str>, console_log: bool, re_try: bool) -> Result<AppConf, failure::Error> {
    let mut app_conf = match AppConf::guess_conf_file(conf) {
        Ok(cfg) => {
            if let Some(cfg) = cfg {
                cfg
            } else if !re_try {
                let bytes = include_bytes!("app_config_demo.yml");
                let path = env::current_exe()?
                    .parent()
                    .expect("current_exe's parent folder should exists.")
                    .join(CONF_FILE_NAME);
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&path)?;
                file.write_all(bytes)?;
                process_app_config(conf, console_log, true)?
            } else {
                bail!("re_try read app_conf failed!");
            }
        }
        Err(err) => {
            bail!("Read app configuration file failed:{:?}, {:?}", conf, err);
        }
    };

    println!(
        "using configuration file: {}",
        app_conf
            .config_file_path
            .as_ref()
            .expect("configuration file should exist at this point.")
            .to_str()
            .unwrap_or("O")
    );

    app_conf.validate_conf()?;
    log_util::setup_logger_for_this_app(
        console_log,
        app_conf.get_log_file().as_str(),
        &app_conf.get_log_conf().verbose_modules,
    )?;

    Ok(app_conf)
}

fn sync_dirs<'a>(
    app_conf: &AppConf,
    sub_matches: &'a clap::ArgMatches<'a>,
) -> Result<(), failure::Error> {
    let mut servers: Vec<Server> = Vec::new();
    if sub_matches.value_of("server-yml").is_some() {
        let server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
        servers.push(server);
    } else {
        servers.append(&mut app_conf.load_all_server_yml());
    }

    let skip_sha1 = sub_matches.is_present("skip-sha1");
    let no_pb = sub_matches.is_present("no-pb");

    if servers.is_empty() {
        println!("found no server yml!");
    } else {
        println!(
            "found {} server yml files. start processing...",
            servers.len()
        );
    }
    if no_pb {
        for mut server in servers {
            if let Err(err) = server.prepare_file_list(skip_sha1) {
                warn!("prepare_file_list failed: {:?}", err);
                println!("prepare_file_list failed: {:?}", err);
            } else {
                match server.sync_dirs(skip_sha1, None) {
                    Ok(result) => {
                        actions::write_dir_sync_result(&server, &result);
                        println!("{:?}", result);
                    }
                    Err(err) => println!("sync-dirs failed: {:?}", err),
                }
            }
        }
    } else {
        let mut mb = MultiBar::new();
        let server_pbs: Vec<(Server, MyPb)> = servers
            .into_iter()
            .filter_map(|mut s| {
                if let Err(err) = s.prepare_file_list(skip_sha1) {
                    warn!("prepare_file_list failed: {:?}", err);
                    println!("prepare_file_list failed: {:?}", err);
                    None
                } else {
                    Some(s)
                }
            })
            .filter_map(|s| match s.get_pb_data() {
                Err(err) => {
                    warn!("get_pb_data failed: {:?}", err);
                    None
                }
                Ok(pb_data) => {
                    let mut pb = mb.create_bar(pb_data.1);
                    pb.set_units(Units::Bytes);
                    let mypb = MyPb {
                        file_count: pb_data.0,
                        pb,
                    };
                    Some((s, mypb))
                }
            })
            .collect();

        let t = thread::spawn(move || {
            mb.listen();
        });

        server_pbs.into_par_iter().for_each(|server_pb| {
            let (mut server, pb) = server_pb;
            match server.sync_dirs(skip_sha1, Some(pb)) {
                Ok(result) => {
                    actions::write_dir_sync_result(&server, &result);
                    println!("{:?}", result);
                }
                Err(err) => println!("sync-dirs failed: {:?}", err),
            }
        });

        t.join().unwrap();
    }
    Ok(())
}

fn main() -> Result<(), failure::Error> {
    let yml = load_yaml!("17_yaml.yml");
    let app = App::from_yaml(yml);
    let m: ArgMatches = app.get_matches();

    let mut app1 = App::from_yaml(yml);

    let conf = m.value_of("conf");
    let console_log = m.is_present("console-log");

    let app_conf = process_app_config(conf, console_log, false)?;

    match m.subcommand() {
        ("pbr", Some(_sub_matches)) => {
            demonstrate_pbr()?;
        }
        ("sync-dirs", Some(sub_matches)) => {
            sync_dirs(&app_conf, sub_matches)?;
        }
        ("copy-executable", Some(sub_matches)) => {
            let mut server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            let executable = sub_matches.value_of("executable").unwrap();
            let remote = server.remote_exec.clone();
            server.copy_a_file(executable, &remote)?;
            println!(
                "copy from {} to {} {} successed.",
                executable,
                server.host.as_str(),
                remote
            );
        }
        ("copy-server-yml", Some(sub_matches)) => {
            let mut server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            let remote = server.remote_server_yml.clone();
            let local = server
                .yml_location
                .as_ref()
                .and_then(|pb| pb.to_str())
                .unwrap()
                .to_string();
            server.copy_a_file(&local, &remote)?;
            println!(
                "copy from {} to {} {} successed.",
                local,
                server.host.as_str(),
                remote
            );
        }
        ("demo-server-yml", Some(sub_matches)) => {
            let bytes = include_bytes!("server_template.yaml");

            if let Some(out) = sub_matches.value_of("out") {
                let mut f = fs::OpenOptions::new().create(true).write(true).open(out)?;
                f.write_all(&bytes[..])?;
                println!("write demo server yml to file done. {}", out);
            } else {
                io::stdout().write_all(&bytes[..])?;
                println!();
            }
        }
        ("list-server-yml", Some(_sub_matches)) => {
            println!(
                "list files name under directory: {:?}",
                app_conf.get_servers_dir()
            );
            for entry in app_conf.get_servers_dir().read_dir()? {
                if let Ok(ery) = entry {
                    println!("{:?}", ery.file_name());
                } else {
                    warn!("read servers_dir entry failed.");
                }
            }
        }
        ("archive-local", Some(sub_matches)) => {
            let server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            server.tar_local()?;
        }
        ("list-remote-files", Some(sub_matches)) => {
            let skip_sha1 = sub_matches.is_present("skip-sha1");
            let start = Instant::now();
            let mut server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            server.list_remote_file_exec(skip_sha1)?;

            if let Some(out) = sub_matches.value_of("out") {
                let mut out = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(out)?;
                let mut rf = fs::OpenOptions::new()
                    .read(true)
                    .open(&server.get_working_file_list_file())?;
                io::copy(&mut rf, &mut out)?;
            } else {
                let mut rf = fs::OpenOptions::new()
                    .read(true)
                    .open(&server.get_working_file_list_file())?;
                io::copy(&mut rf, &mut io::stdout())?;
            }

            println!("time costs: {:?}", start.elapsed().as_secs());
        }
        ("list-local-files", Some(sub_matches)) => {
            let skip_sha1 = sub_matches.is_present("skip-sha1");
            let server = &app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            if let Some(out) = sub_matches.value_of("out") {
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
        ("verify-server-yml", Some(sub_matches)) => {
            let mut server = app_conf.load_server_yml(parse_server_yml(sub_matches))?;
            println!(
                "found server configuration yml at: {:?}",
                server.yml_location.as_ref().unwrap()
            );
            println!("server content: {}", serde_yaml::to_string(&server)?);
            if let Err(err) = server.stats_remote_exec() {
                println!(
                    "CAN'T FIND SERVER SIDE EXEC. {:?}\n{:?}",
                    server.remote_exec, err
                );
            } else {
                let rp = server.remote_server_yml.clone();
                match server.get_remote_file_content(&rp) {
                    Ok(content) => {
                        let ss: Server = serde_yaml::from_str(content.as_str())?;
                        if !server.dir_equals(&ss) {
                            println!(
                                "SERVER DIRS DIDN'T EQUAL TO.\nlocal: {:?} vs remote: {:?}",
                                server.directories, ss.directories
                            );
                        } else {
                            println!("SERVER SIDE CONFIGURATION IS OK!");
                        }
                    }
                    Err(err) => println!("got error: {:?}", err),
                }
            }
        }
        ("completions", Some(sub_matches)) => {
            let shell = sub_matches.value_of("shell_name").unwrap();
            app1.gen_completions_to(
                "bk-over-ssh",
                shell.parse::<Shell>().unwrap(),
                &mut io::stdout(),
            );
        }
        ("rsync", Some(sub_matches)) => match sub_matches.subcommand() {
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

                let mut dr = rustsync::DeltaFileReader::<fs::File>::read_delta_file(delta_file)?;
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

            (_, _) => {
                println!("please add --help to view usage help.");
            }
        },
        ("repl", Some(_)) => {
            main_client();
        }
        ("print-env", Some(_)) => {
            for (key, value) in env::vars_os() {
                println!("{:?}: {:?}", key, value);
            }
            println!("current exec: {:?}", env::current_exe());
        }
        ("print-conf", Some(_)) => {
            println!(
                "The configuration file is located at: {:?}, content:\n{}",
                app_conf.config_file_path.as_ref().unwrap(),
                serde_yaml::to_string(&app_conf)?
            );
        }
        (_, _) => unimplemented!(), // for brevity
    }
    Ok(())
}
