#[macro_use]
extern crate derive_builder;
#[macro_use]
extern crate failure;

#[macro_use]
extern crate clap;

extern crate askama;
extern crate rand;
// extern crate rustsync;
extern crate time;

#[macro_use]
extern crate itertools;

#[macro_use]
extern crate lazy_static;

mod actions;
mod data_shape;
mod db_accesses;
mod develope;
mod ioutil;
mod log_util;
mod mail;
mod rustsync;

#[macro_use]
extern crate rusqlite;

use crate::rustsync::DeltaWriter;
// use std::borrow::Cow::{self, Borrowed, Owned};

use clap::App;
use clap::ArgMatches;
use clap::Shell;
use db_accesses::{DbAccess, SqliteDbAccess};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::*;
use mail::send_test_mail;
use rayon::prelude::*;
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, io, io::BufRead, io::Write};

use actions::SyncDirReport;
use data_shape::{AppConf, Server, ServerYml, CONF_FILE_NAME};
use r2d2_sqlite::SqliteConnectionManager;

fn demonstrate_pbr() -> Result<(), failure::Error> {
    let multi_bar = Arc::new(MultiProgress::new());

    let multi_bar1 = Arc::clone(&multi_bar);

    let count = 100;

    let mut i = 0;

    let _ = thread::spawn(move || loop {
        i += 1;
        println!("{}", i);
        multi_bar1.join_and_clear().unwrap();
        thread::sleep(Duration::from_millis(5));
    });

    let pb1 = multi_bar.add(ProgressBar::new(1_000_000));
    pb1.set_style(
        ProgressStyle::default_bar()
            // .template("[{eta_precise}] {prefix:.bold.dim} {bar:40.cyan/blue} {pos:>7}/{len:7} {wide_msg}")
            .template("[{eta_precise}] {prefix:.bold.dim} {spinner} {bar:40.cyan/blue}  {decimal_bytes}/{decimal_total_bytes}  {bytes:>7}/{bytes_per_sec}/{total_bytes:7} {wide_msg}")
            .progress_chars("##-"),
    );
    // pb1.format_state(); format_style list all possible value.
    pb1.set_message("hello message.");
    pb1.set_prefix(&format!("[{}/?]", 33));

    let pb2 = multi_bar.add(ProgressBar::new(count));

    for _ in 0..count {
        pb1.inc(1000);
        pb2.inc(1);
        thread::sleep(Duration::from_millis(200));
    }
    pb1.finish();
    pb2.finish();

    if let Err(err) = multi_bar.join_and_clear() {
        println!("join_and_clear failed: {:?}", err);
    }
    Ok(())
}

fn join_multi_bars(multi_bar: Option<Arc<MultiProgress>>) -> Option<thread::JoinHandle<()>> {
    if let Some(mb) = multi_bar {
        Some(thread::spawn(move || {
            mb.join().unwrap();
        }))
    } else {
        None
    }
}

fn wait_progresss_bar_finish(jh: Option<thread::JoinHandle<()>>) {
    if let Some(t) = jh {
        t.join().unwrap();
    }
}

fn process_app_config<M, D>(
    conf: Option<&str>,
    re_try: bool,
) -> Result<AppConf<M, D>, failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let message_pb = ProgressBar::new_spinner();
    message_pb.enable_steady_tick(200);
    let app_conf = match AppConf::guess_conf_file(conf) {
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
                message_pb.finish_and_clear();
                return process_app_config(conf, true);
            } else {
                bail!("re_try read app_conf failed!");
            }
        }
        Err(err) => {
            bail!("Read app configuration file failed:{:?}, {:?}", conf, err);
        }
    };

    let message = format!(
        "using configuration file: {:?}",
        app_conf.config_file_path.as_path()
    );

    message_pb.set_message(message.as_str());

    message_pb.finish_and_clear();
    Ok(app_conf)
}

fn delay_exec(delay: &str) {
    let delay = delay.parse::<u64>().expect("delay must be an integer.");
    let style = ProgressStyle::default_bar()
        .template("{bar:40} counting down {wide_msg}.")
        .progress_chars("##-");
    let pb = ProgressBar::new(delay).with_style(style);
    thread::spawn(move || loop {
        pb.inc(1);
        let message = format!("{}", delay - pb.position());
        pb.set_message(message.as_str());
        thread::sleep(Duration::from_secs(1));
        if pb.position() >= delay {
            pb.finish_and_clear();
            break;
        }
    });
    thread::sleep(Duration::from_secs(delay));
}

fn sync_dirs<'a, M, D>(
    app_conf: &AppConf<M, D>,
    sub_matches: &'a clap::ArgMatches<'a>,
    console_log: bool,
) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let mut servers: Vec<Server<M, D>> = Vec::new();

    if sub_matches.value_of("server-yml").is_some() {
        let server = app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
        servers.push(server);
    } else {
        servers.append(&mut app_conf.load_all_server_yml());
    }

    // all progress bars already create from here on.

    let t = join_multi_bars(app_conf.progress_bar.clone());

    if servers.is_empty() {
        println!("found no server yml!");
    } else {
        println!(
            "found {} server yml files. start processing...",
            servers.len()
        );
    }
    servers.iter_mut().filter_map(|s|{
        if let Err(err) = s.connect() {
            eprintln!("{:?}", err);
            None
        } else {
            Some(s)
        }
        }).count();

        servers.into_par_iter().for_each(|server| {
        match server.sync_dirs() {
            Ok(result) => {
                actions::write_dir_sync_result(&server, result.as_ref());
                if console_log {
                    let result_yml = serde_yaml::to_string(&result)
                        .expect("SyncDirReport should deserialize success.");
                    println!("{}:\n{}", server.get_host(), result_yml);
                }
            }
            Err(err) => println!("sync-dirs failed: {:?}", err),
        }
    });

    servers.iter().for_each(|sv|sv.pb_finish());
    wait_progresss_bar_finish(t);
    Ok(())
}

fn main() -> Result<(), failure::Error> {
    let yml = load_yaml!("17_yaml.yml");
    let app = App::from_yaml(yml);
    let m: ArgMatches = app.get_matches();
    let app1 = App::from_yaml(yml);
    let console_log = m.is_present("console-log");

    let conf = m.value_of("conf");
    // we always open db connection unless no-db parameter provided.
    let mut app_conf = process_app_config::<SqliteConnectionManager, SqliteDbAccess>(conf, false)?;
    if m.is_present("skip-cron") {
        app_conf.skip_cron();
    }
    if m.is_present("enable-sha1") {
        app_conf.not_skip_sha1();
    }
    if !m.is_present("no-pb") {
        app_conf
            .progress_bar
            .replace(Arc::new(MultiProgress::new()));
    }
    if let Some(buf_len) = m.value_of("buf-len") {
        app_conf.buf_len = Some(buf_len.parse()?);
    }

    let verbose = if m.is_present("vv") {
        "vv"
    } else if m.is_present("v") {
        "v"
    } else {
        ""
    };

    log_util::setup_logger_for_this_app(
        console_log,
        app_conf.log_full_path.as_path(),
        app_conf.get_log_conf().get_verbose_modules(),
        verbose,
    )?;

    if let ("create-remote-db", Some(sub_matches)) = m.subcommand() {
        let mut server = app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
        let db_type = sub_matches.value_of("db-type").unwrap_or("sqlite");
        let force = sub_matches.is_present("force");
        server.connect()?;
        return server.create_remote_db(db_type, force);
    }

    if let ("create-db", Some(sub_matches)) = m.subcommand() {
        let db_type = sub_matches.value_of("db-type").unwrap_or("sqlite");
        let force = sub_matches.is_present("force");
        if "sqlite" == db_type {
            let mut app_conf =
                process_app_config::<SqliteConnectionManager, SqliteDbAccess>(conf, false)?;
            if force {
                fs::remove_file(app_conf.get_sqlite_db_file())?;
            }
            let sqlite_db_access = SqliteDbAccess::new(app_conf.get_sqlite_db_file());
            app_conf.set_db_access(sqlite_db_access);
            if let Some(da) = app_conf.get_db_access() {
                if let Err(err) = da.create_database() {
                    eprintln!("{}", err.find_root_cause());
                }
            }
        } else {
            println!("unsupported database: {}", db_type);
        }
        return Ok(());
    }

    let no_db = m.is_present("no-db");
    if !no_db {
        let sqlite_db_access = SqliteDbAccess::new(app_conf.get_sqlite_db_file());
        app_conf.set_db_access(sqlite_db_access);
    }

    app_conf.lock_working_file()?;
    let delay = m.value_of("delay");
    if let Some(delay) = delay {
        delay_exec(delay);
    }
    if let Err(err) = main_entry(app1, &app_conf, &m, console_log) {
        error!("{:?}", err);
        eprintln!("{:?}", err);
    }
    // wait_progresss_bar_finish(t);
    Ok(())
}

fn main_entry<'a, M, D>(
    app1: App,
    app_conf: &AppConf<M, D>,
    m: &'a clap::ArgMatches<'a>,
    console_log: bool,
) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    if let ("sync-dirs", Some(sub_matches)) = m.subcommand() {
        return sync_dirs(&app_conf, sub_matches, console_log);
    }
    main_entry_1(app1, app_conf, m, console_log)
}

fn main_entry_1<'a, M, D>(
    mut app1: App,
    app_conf: &AppConf<M, D>,
    m: &'a clap::ArgMatches<'a>,
    _console_log: bool,
) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let no_db = m.is_present("no-db");
    match m.subcommand() {
        ("pbr", Some(_sub_matches)) => {
            demonstrate_pbr()?;
        }
        ("send-test-mail", Some(sub_matches)) => {
            let to = sub_matches.value_of("to").unwrap();
            send_test_mail(&app_conf.get_mail_conf(), to)?;
        }
        ("copy-executable", Some(sub_matches)) => {
            let mut server =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            let executable = sub_matches.value_of("executable").unwrap();
            let remote = server.server_yml.remote_exec.clone();
            server.copy_a_file(executable, &remote)?;
            println!(
                "copy from {} to {} {} successed.",
                executable,
                server.get_host(),
                remote
            );
        }
        ("print-report", Some(sub_matches)) => {
            let server = app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            let fbuf = io::BufReader::new(
                fs::OpenOptions::new()
                    .read(true)
                    .open(server.get_dir_sync_report_file())?,
            );

            if let Some(line) = fbuf.lines().last() {
                let line = line?;
                let sdr: SyncDirReport = serde_json::from_str(line.as_str())?;
                println!("{}", serde_yaml::to_string(&sdr)?);
            } else {
                println!("empty report file.");
            }
        }
        ("copy-server-yml", Some(sub_matches)) => {
            let mut server =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            let remote = server.server_yml.remote_server_yml.clone();
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
                server.get_host(),
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
                app_conf.servers_dir
            );
            for entry in app_conf.servers_dir.read_dir()? {
                if let Ok(ery) = entry {
                    println!("{:?}", ery.file_name());
                } else {
                    warn!("read servers_dir entry failed.");
                }
            }
        }
        ("archive-local", Some(sub_matches)) => {
            let mut servers: Vec<Server<M, D>> = Vec::new();

            if sub_matches.value_of("server-yml").is_some() {
                let server =
                    app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
                servers.push(server);
            } else {
                servers.append(&mut app_conf.load_all_server_yml());
            }

            // all progress bars already create from here on.

            let t = join_multi_bars(app_conf.progress_bar.clone());

            if servers.is_empty() {
                println!("found no server yml!");
            } else {
                println!(
                    "found {} server yml files. start processing...",
                    servers.len()
                );
            }
            let prune_op = sub_matches.value_of("prune");
            let prune_only_op = sub_matches.value_of("prune-only");

            servers.into_par_iter().map(|server| {
                if prune_op.is_some() {
                    if let Err(err) = server.tar_local() {
                        error!("{:?}", err);
                        eprintln!("{:?}", err);
                    }
                    if let Err(err) = server.prune_backups() {
                        error!("{:?}", err);
                        eprintln!("{:?}", err);
                    }
                } else if prune_only_op.is_some() {
                    if let Err(err) = server.prune_backups() {
                        error!("{:?}", err);
                        eprintln!("{:?}", err);
                    }
                } else if let Err(err) = server.tar_local() {
                    error!("{:?}", err);
                    eprintln!("{:?}", err);
                }
            }).count();

            // servers.for_each(|sv| sv.pb_finish());
            wait_progresss_bar_finish(t);
        }
        ("list-remote-files", Some(sub_matches)) => {
            let start = Instant::now();
            let mut server =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            server.connect()?;
            server.list_remote_file_exec(no_db)?;

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
            let mut server =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            if no_db {
                server.server_yml.use_db = false;
            }
            if let Some(out) = sub_matches.value_of("out") {
                let mut out = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(out)?;
                server.load_dirs(&mut out)?;
            } else {
                server.load_dirs(&mut io::stdout())?;
            }
        }
        ("verify-server-yml", Some(sub_matches)) => {
            let mut server =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            eprintln!(
                "found server configuration yml at: {:?}",
                server.yml_location.as_ref().unwrap()
            );
            eprintln!(
                "server content: {}",
                serde_yaml::to_string(&server.server_yml)?
            );
            server.connect()?;
            if let Err(err) = server.stats_remote_exec() {
                eprintln!(
                    "CAN'T FIND SERVER SIDE EXEC. {:?}\n{:?}",
                    server.server_yml.remote_exec, err
                );
            } else {
                let rp = server.server_yml.remote_server_yml.clone();
                match server.get_remote_file_content(&rp) {
                    Ok(content) => {
                        let ss: ServerYml = serde_yaml::from_str(content.as_str())?;
                        if !server.dir_equals(&ss.directories) {
                            eprintln!(
                                "SERVER DIRS DIDN'T EQUAL TO.\nlocal: {:?} vs remote: {:?}",
                                server.server_yml.directories, ss.directories
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
                match rustsync::Signature::signature_a_file(file, block_size, true) {
                    Ok(mut sig) => {
                        if let Err(err) = sig.write_to_file(out) {
                            error!("rsync signature write_to_file failed: {:?}", err);
                        }
                    }
                    Err(err) => {
                        error!("rsync signature failed: {:?}", err);
                    }
                }
                eprintln!("time costs: {:?}", start.elapsed().as_secs());
            }

            (_, _) => {
                println!("please add --help to view usage help.");
            }
        },
        // ("repl", Some(_)) => {
        //     main_client();
        // }
        ("print-env", Some(_)) => {
            for (key, value) in env::vars_os() {
                println!("{:?}: {:?}", key, value);
            }
            println!("current exec: {:?}", env::current_exe());
        }
        ("print-conf", Some(_)) => {
            println!(
                "The configuration file is located at: {:?}, content:\n{}",
                app_conf.config_file_path,
                serde_yaml::to_string(&app_conf)?
            );
        }
        (_, _) => unimplemented!(), // for brevity
    }
    Ok(())
}

// struct MyHelper {
//     completer: FilenameCompleter,
//     highlighter: MatchingBracketHighlighter,
//     hinter: HistoryHinter,
//     colored_prompt: String,
// }

// impl Completer for MyHelper {
//     type Candidate = Pair;

//     fn complete(
//         &self,
//         line: &str,
//         pos: usize,
//         ctx: &Context<'_>,
//     ) -> Result<(usize, Vec<Pair>), ReadlineError> {
//         self.completer.complete(line, pos, ctx)
//     }
// }

// impl Hinter for MyHelper {
//     fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
//         self.hinter.hint(line, pos, ctx)
//     }
// }

// impl Highlighter for MyHelper {
//     fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
//         &'s self,
//         prompt: &'p str,
//         default: bool,
//     ) -> Cow<'b, str> {
//         if default {
//             Borrowed(&self.colored_prompt)
//         } else {
//             Borrowed(prompt)
//         }
//     }

//     fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
//         Owned("\x1b[1m".to_owned() + hint + "\x1b[m")
//     }

//     fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
//         self.highlighter.highlight(line, pos)
//     }

//     fn highlight_char(&self, line: &str, pos: usize) -> bool {
//         self.highlighter.highlight_char(line, pos)
//     }
// }

// impl Helper for MyHelper {}

// fn main_client() {
//     // env_logger::init();
//     let config = Config::builder()
//         .history_ignore_space(true)
//         .completion_type(CompletionType::List)
//         .edit_mode(EditMode::Emacs)
//         .output_stream(OutputStreamType::Stdout)
//         .build();

//     let h = MyHelper {
//         completer: FilenameCompleter::new(),
//         highlighter: MatchingBracketHighlighter::new(),
//         hinter: HistoryHinter {},
//         colored_prompt: "".to_owned(),
//     };
//     let mut rl = Editor::with_config(config);
//     rl.set_helper(Some(h));
//     rl.bind_sequence(KeyPress::Meta('N'), Cmd::HistorySearchForward);
//     rl.bind_sequence(KeyPress::Meta('P'), Cmd::HistorySearchBackward);
//     if rl.load_history("history.txt").is_err() {
//         println!("No previous history.");
//     }
//     let mut count = 1;
//     loop {
//         let p = format!("{}> ", count);
//         rl.helper_mut().unwrap().colored_prompt = format!("\x1b[1;32m{}\x1b[0m", p);
//         let readline = rl.readline(&p);
//         match readline {
//             Ok(line) => {
//                 rl.add_history_entry(line.as_str());
//                 println!("Line: {}", line);
//                 if line.starts_with("hash ") {
//                     let (_s1, s2) = line.split_at(5);
//                     actions::hash_file_sha1(s2).expect("hash should success.");
//                 }
//             }
//             Err(ReadlineError::Interrupted) => {
//                 println!("CTRL-C");
//                 break;
//             }
//             Err(ReadlineError::Eof) => {
//                 println!("CTRL-D");
//                 break;
//             }
//             Err(err) => {
//                 println!("Error: {:?}", err);
//                 break;
//             }
//         }
//         count += 1;
//     }
//     rl.save_history("history.txt").unwrap();
// }
