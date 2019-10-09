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
#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

mod actions;
mod data_shape;
mod db_accesses;
mod develope;
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
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, io, io::BufRead, io::Write};

use actions::SyncDirReport;
use data_shape::{AppConf, CountReadr, Indicator, Server, ServerYml, CONF_FILE_NAME};
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

fn sync_dirs<M, D>(app_conf: &AppConf<M, D>, server_yml: Option<&str>) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let mut servers: Vec<(Server<M, D>, Indicator)> = Vec::new();

    if let Some(server_yml) = server_yml {
        let server = app_conf.load_server_yml(server_yml)?;
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
    servers
        .iter_mut()
        .filter_map(|s| {
            if let Err(err) = s.0.connect() {
                eprintln!("{:?}", err);
                None
            } else {
                Some(s)
            }
        })
        .count();

    // let handlers = servers.into_iter().map(|(server, mut indicator)| {
    //     thread::spawn(move || {
    //     match server.sync_dirs(&mut indicator) {
    //         Ok(result) => {
    //             indicator.pb_finish();
    //             actions::write_dir_sync_result(&server, result.as_ref());
    //             if console_log {
    //                 let result_yml = serde_yaml::to_string(&result)
    //                     .expect("SyncDirReport should deserialize success.");
    //                 println!("{}:\n{}", server.get_host(), result_yml);
    //             }
    //         }
    //         Err(err) => println!("sync-dirs failed: {:?}", err),
    //     }
    //     })
    // }).collect::<Vec<thread::JoinHandle<_>>>();

    // for child in handlers {
    //     // Wait for the thread to finish. Returns a result.
    //     let _ = child.join();
    // }

    servers.into_par_iter().for_each(|(server, mut indicator)| {
        match server.sync_dirs(&mut indicator) {
            Ok(result) => {
                indicator.pb_finish();
                actions::write_dir_sync_result(&server, result.as_ref());
            }
            Err(err) => println!("sync-dirs failed: {:?}", err),
        }
    });
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
    let mut app_conf =
        match process_app_config::<SqliteConnectionManager, SqliteDbAccess>(conf, false) {
            Ok(app_conf) => app_conf,
            Err(err) => {
                eprintln!("parse app config failed: {:?}", err);
                bail!("parse app config failed: {:?}", err);
            }
        };

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
        app_conf.mini_app_conf.buf_len = Some(buf_len.parse()?);
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
        let (mut server, _indicator) =
            app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
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
    Ok(())
}

fn main_entry<'a, M, D>(
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
        ("run-round", Some(_sub_matches)) => {
            sync_dirs(&app_conf, None)?;
            archive_local(&app_conf, None, Some("prune"), None)?;
        }
        ("sync-dirs", Some(sub_matches)) => {
            sync_dirs(&app_conf, sub_matches.value_of("server-yml"))?;
        }
        ("pbr", Some(_sub_matches)) => {
            demonstrate_pbr()?;
        }
        ("send-test-mail", Some(sub_matches)) => {
            let to = sub_matches.value_of("to").unwrap();
            send_test_mail(&app_conf.get_mail_conf(), to)?;
        }
        ("polling-file", Some(sub_matches)) => {
            let file = sub_matches.value_of("file").unwrap();
            let period = sub_matches
                .value_of("period")
                .unwrap_or("3")
                .parse::<u64>()
                .ok()
                .unwrap_or(3);
            let path = Path::new(file);
            let mut last_len = 0;
            let mut count = 0;
            loop {
                if count > 1 {
                    break;
                }
                thread::sleep(Duration::from_secs(period));
                let ln = path.metadata()?.len();
                if ln == last_len {
                    count += 1;
                } else {
                    last_len = ln;
                }
                println!("{}", ln);
            }
        }
        ("copy-executable", Some(sub_matches)) => {
            let (mut server, _indicator) =
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
            let (server, _indicator) =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
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
            let (mut server, _indicator) =
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
            archive_local(
                app_conf,
                sub_matches.value_of("server-yml"),
                sub_matches.value_of("prune"),
                sub_matches.value_of("prune-only"),
            )?;
        }
        ("confirm-remote-sync", Some(sub_matches)) => {
            let start = Instant::now();
            let (mut server, _indicator) =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            server.connect()?;
            server.confirm_remote_sync()?;
            eprintln!("time costs: {:?}", start.elapsed().as_secs());
        }
        ("confirm-local-sync", Some(sub_matches)) => {
            let (server, _indicator) =
                app_conf.load_server_yml(sub_matches.value_of("server-yml").unwrap())?;
            server.confirm_local_sync()?;
        }
        ("list-remote-files", Some(sub_matches)) => {
            let start = Instant::now();
            let (mut server, _indicator) =
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
            let (mut server, _indicator) =
                if let Some(server_yml) = sub_matches.value_of("server-yml") {
                    app_conf.load_server_yml(server_yml)?
                } else {
                    let mut all_yml = app_conf.load_all_server_yml();
                    if all_yml.is_empty() {
                        bail!("no server-yml found");
                    } else {
                        all_yml.remove(0)
                    }
                };
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
            let (mut server, _indicator) =
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
                restore_a_file(
                    sub_sub_matches.value_of("old-file"),
                    sub_sub_matches.value_of("delta-file"),
                    sub_sub_matches.value_of("out-file"),
                )?;
            }
            ("delta-a-file", Some(sub_sub_matches)) => {
                delta_a_file(
                    sub_sub_matches.value_of("new-file"),
                    sub_sub_matches.value_of("sig-file"),
                    sub_sub_matches.value_of("out-file"),
                    sub_sub_matches.is_present("print-progress"),
                )?;
            }
            ("signature", Some(sub_sub_matches)) => {
                signature(
                    sub_sub_matches.value_of("file"),
                    sub_sub_matches.value_of("block-size"),
                    sub_sub_matches.value_of("out"),
                )?;
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

fn archive_local<M, D>(
    app_conf: &AppConf<M, D>,
    server_yml: Option<&str>,
    prune_op: Option<&str>,
    prune_only_op: Option<&str>,
) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let mut servers: Vec<(Server<M, D>, Indicator)> = Vec::new();

    if let Some(server_yml) = server_yml {
        let server = app_conf.load_server_yml(server_yml)?;
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
    servers
        .into_par_iter()
        .map(|(server, mut indicator)| {
            if prune_op.is_some() {
                if let Err(err) = server.archive_local(&mut indicator) {
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
            } else if let Err(err) = server.archive_local(&mut indicator) {
                error!("{:?}", err);
                eprintln!("{:?}", err);
            }
            indicator.pb_finish();
        })
        .count();

    wait_progresss_bar_finish(t);
    Ok(())
}

fn restore_a_file(
    old_file: Option<&str>,
    maybe_delta_file: Option<&str>,
    maybe_out_file: Option<&str>,
) -> Result<String, failure::Error> {
    let old_file = old_file.unwrap();
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
    dr.restore_from_file_to_file(&out_file, old_file)?;
    Ok(out_file)
}

fn delta_a_file(
    new_file: Option<&str>,
    maybe_sig_file: Option<&str>,
    maybe_out_file: Option<&str>,
    print_progress: bool,
) -> Result<String, failure::Error> {
    let new_file = new_file.unwrap();
    if print_progress {
        let new_file_length = Path::new(new_file).metadata()?.len();
        println!("size:{}", new_file_length);
    }
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

    let new_file_input = io::BufReader::new(fs::OpenOptions::new().read(true).open(new_file)?);
    if print_progress {
        let mut sum = 0;
        let f = |num| {
            sum += num;
            if sum > 5_0000 || num == 0 {
                println!("{:?}", sum);
                sum = 0;
            }
        };
        let nr = CountReadr::new(new_file_input, f);
        rustsync::DeltaFileWriter::<fs::File>::create_delta_file(&out_file, sig.window, None)?
            .compare(&sig, nr)?;
    } else {
        rustsync::DeltaFileWriter::<fs::File>::create_delta_file(&out_file, sig.window, None)?
            .compare(&sig, new_file_input)?;
    }
    Ok(out_file)
}

fn signature(
    file: Option<&str>,
    block_size: Option<&str>,
    out: Option<&str>,
) -> Result<String, failure::Error> {
    let file = file.unwrap();
    let block_size: Option<usize> = block_size.and_then(|s| s.parse().ok());
    let sig_file = format!("{}.sig", file);
    let out = out.unwrap_or_else(|| sig_file.as_str());
    let start = Instant::now();
    let indicator = Indicator::new(None);
    match rustsync::Signature::signature_a_file(file, block_size, &indicator) {
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
    Ok(out.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::hash_file_sha1;
    use crate::develope::tutil;
    use std::path::PathBuf;

    #[test]
    fn t_sig_delta_restore() -> Result<(), failure::Error> {
        let tu = tutil::TestDir::new();
        let old_file_path = tu.make_a_file_with_len("abc", 100_000)?;
        let old_file_name = old_file_path.as_path().to_str();
        let sig_file_name = signature(old_file_name, None, None)?;
        eprintln!("sig_file_name: {:?}", sig_file_name);
        assert!(PathBuf::from(&sig_file_name).exists());
        let delta_file = delta_a_file(old_file_name, None, None, true)?;
        eprintln!("delta_file {:?}", delta_file);
        assert!(PathBuf::from(&delta_file).exists());
        let restored = restore_a_file(old_file_name, None, None)?;
        assert_eq!(
            hash_file_sha1(restored),
            hash_file_sha1(old_file_name.unwrap())
        );
        Ok(())
    }
}
