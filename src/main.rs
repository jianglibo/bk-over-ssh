#[macro_use]
extern crate derive_builder;
#[macro_use]
extern crate failure;

#[macro_use]
extern crate clap;

extern crate askama;
extern crate rand;
extern crate time;

#[macro_use]
extern crate itertools;
#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

mod actions;
mod command;
mod data_shape;
mod db_accesses;
mod develope;
mod log_util;
mod mail;
mod protocol;
mod rustsync;

#[macro_use]
extern crate rusqlite;

use clap::App;
use clap::ArgMatches;
use clap::Shell;
use command::db_cmd;
use db_accesses::SqliteDbAccess;
use indicatif::MultiProgress;
use log::*;
use mail::send_test_mail;
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::{fs, io, io::Read, io::Write};

use data_shape::{AppConf, AppRole};

/// we change mini_app_conf value here.
fn main() -> Result<(), failure::Error> {
    let yml = load_yaml!("17_yaml.yml");
    let app = App::from_yaml(yml);
    let m: ArgMatches = app.get_matches();
    let app1 = App::from_yaml(yml);
    let console_log = m.is_present("console-log");
    let verbose = if m.is_present("vv") {
        "vv"
    } else if m.is_present("v") {
        "v"
    } else {
        ""
    };
    let skip_sha1 = !m.is_present("enable-sha1");

    if let ("pong", Some(_sub_matches)) = m.subcommand() {
        let mut stdin = io::stdin();
        let mut stdout = io::stdout();
        let mut buf = [0; 8];
        stdin.read_exact(&mut buf)?;
        let len = u64::from_be_bytes(buf);
        info!("got ping: {}", len);
        stdout.write_all(&buf)?;
        return Ok(());
    }

    // When in server-receive-loop no configuration file is required.
    if let ("server-receive-loop", Some(_sub_matches)) = m.subcommand() {
        let log_file = "data/server-receive-loop.log";
        let log_file_path = Path::new(log_file);
        if let Some(parent) = log_file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        log_util::setup_logger_for_this_app(console_log, log_file, Vec::<String>::new(), verbose)?;
        if let Err(err) = command::server_loop::server_receive_loop() {
            error!("server-receive-loop caught error: {:?}", err);
        }
        return Ok(());
    }

    // When in server-send-loop no configuration file is required.
    if let ("server-send-loop", Some(_sub_matches)) = m.subcommand() {
        let log_file = "data/server-send-loop.log";
        let log_file_path = Path::new(log_file);
        if let Some(parent) = log_file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        log_util::setup_logger_for_this_app(console_log, log_file, Vec::<String>::new(), verbose)?;
        if let Err(err) = command::server_loop::server_send_loop(skip_sha1) {
            error!("server-send-loop caught error: {:?}", err);
        }
        return Ok(());
    }

    let app_role = if let ("client-push-loop", Some(_)) = m.subcommand() {
        Some(AppRole::ActiveLeaf)
    } else if let ("client-pull-loop", Some(_)) = m.subcommand() {
        Some(AppRole::PullHub)
    } else if let ("copy-executable", Some(_)) = m.subcommand() {
        Some(AppRole::PullHub)
    } else {
        None
    };

    let conf = m.value_of("conf");
    let mut app_conf = AppConf::guess_conf_file(conf, app_role.as_ref())?;
    // we always open db connection unless no-db parameter provided.
    // let mut app_conf = match command::process_app_config(conf, app_role.as_ref(), false) {
    //     Ok(app_conf) => app_conf,
    //     Err(err) => {
    //         eprintln!("parse app config failed: {}", err);
    //         bail!("parse app config failed: {}", err);
    //     }
    // };

    if let Some(aii) = m.value_of("app-instance-id") {
        app_conf.set_app_instance_id(aii);
    }

    app_conf.mini_app_conf.skip_sha1 = skip_sha1;
    app_conf.mini_app_conf.as_service = m.is_present("as-service");

    if !m.is_present("no-pb") && !app_conf.mini_app_conf.as_service {
        app_conf.mini_app_conf.show_pb = true;
        app_conf
            .progress_bar
            .replace(Arc::new(MultiProgress::new()));
    }
    if let Some(buf_len) = m.value_of("buf-len") {
        app_conf.mini_app_conf.buf_len = Some(buf_len.parse()?);
    }

    app_conf.mini_app_conf.console_log = console_log;
    app_conf.mini_app_conf.verbose = !verbose.is_empty();

    log_util::setup_logger_for_this_app(
        console_log,
        app_conf.log_full_path.as_path(),
        app_conf.get_log_conf().get_verbose_modules(),
        verbose,
    )?;

    if db_cmd::create_db(&mut app_conf, &m)? {
        return Ok(());
    }

    let no_db = m.is_present("no-db");

    if !no_db {
        let sqlite_db_access = SqliteDbAccess::new(app_conf.get_sqlite_db_file());
        app_conf.set_db_access(sqlite_db_access);
    }

    let delay = m.value_of("delay");
    if let Some(delay) = delay {
        command::delay_exec(delay);
    }
    if let Err(err) = main_entry(app1, &mut app_conf, &m, console_log) {
        error!("{:?}", err);
        eprintln!("{:?}", err);
    }
    Ok(())
}

fn main_entry<'a>(
    mut app1: App,
    app_conf: &mut AppConf,
    m: &'a clap::ArgMatches<'a>,
    _console_log: bool,
) -> Result<(), failure::Error> {
    // let no_db = m.is_present("no-db");
    match m.subcommand() {
        ("client-push-loop", Some(sub_matches)) => {
            app_conf.mini_app_conf.app_role.replace(AppRole::ActiveLeaf);
            let server_yml = sub_matches.value_of("server-yml");
            if server_yml.is_none() {
                app_conf.progress_bar.take();
            }
            let archive_after_sync = sub_matches.is_present("archive");
            command::client_push_loops(&app_conf, server_yml,archive_after_sync, app_conf.mini_app_conf.as_service, false)?;
        }
        ("client-pull-loop", Some(sub_matches)) => {
            app_conf.mini_app_conf.app_role.replace(AppRole::PullHub);
            let server_yml = sub_matches.value_of("server-yml");
            if server_yml.is_none() {
                app_conf.progress_bar.take();
            }
            let archive_after_sync = sub_matches.is_present("archive");
            command::client_pull_loops(&app_conf, server_yml, archive_after_sync, app_conf.mini_app_conf.as_service, false)?;
        }
        ("send-test-mail", Some(sub_matches)) => {
            let to = sub_matches.value_of("to").unwrap();
            send_test_mail(&app_conf.get_mail_conf(), to)?;
        }
        ("polling-file", Some(sub_matches)) => {
            command::misc::polling_file(sub_matches)?;
        }
        ("copy-executable", Some(sub_matches)) => {
            let server_yml = sub_matches.value_of("server-yml").expect("server-yml should be present");
            let mut server = app_conf.load_server_from_yml(server_yml, false)?;

            let executable = sub_matches
                .value_of("executable")
                .expect("executable paramter missing");
            server.connect()?;
            let remote = server.get_remote_exec();
            server.copy_a_file(executable, remote.as_str())?;
            eprintln!(
                "copy from {} to {} {} succeeded.",
                executable,
                server.get_host(),
                remote
            );
        }
        ("copy-server-yml", Some(sub_matches)) => {
            let server_yml = sub_matches.value_of("server-yml").expect("server-yml should be present");
            let mut server = app_conf.load_server_from_yml(server_yml, false)?;
            let remote = server.get_remote_server_yml();
            let local = server
                .yml_location
                .as_ref()
                .and_then(|pb| pb.to_str())
                .unwrap()
                .to_string();
            server.connect()?;
            server.copy_a_file(&local, &remote)?;
            println!(
                "copy from {} to {} {} succeeded.",
                local,
                server.get_host(),
                remote
            );
        }
        ("demo-server-yml", Some(sub_matches)) => {
            if let Some(out) = sub_matches.value_of("out") {
                let mut f = fs::OpenOptions::new().create(true).write(true).open(out)?;
                f.write_all(command::SERVER_TEMPLATE_BYTES)?;
                println!("write demo server yml to file done. {}", out);
            } else {
                io::stdout().write_all(command::SERVER_TEMPLATE_BYTES)?;
                println!();
            }
        }
        ("list-server-yml", Some(_sub_matches)) => {
            println!(
                "list files name under directory: {:?}",
                app_conf.servers_conf_dir
            );
            for entry in app_conf.servers_conf_dir.read_dir()? {
                if let Ok(ery) = entry {
                    println!("{:?}", ery.file_name());
                } else {
                    warn!("read servers_conf_dir entry failed.");
                }
            }
        }
        ("archive-local", Some(sub_matches)) => {
            command::archive_local(
                app_conf,
                sub_matches.value_of("server-yml"),
                sub_matches.is_present("prune"),
                sub_matches.is_present("prune-only"),
            )?;
        }
        ("verify-server-yml", Some(sub_matches)) => {
            let server_yml = sub_matches.value_of("server-yml").expect("server-yml should be present");
            let server = app_conf.load_server_from_yml(server_yml, false)?;
            command::misc::verify_server_yml(server)?;
        }
        ("completions", Some(sub_matches)) => {
            let shell = sub_matches.value_of("shell_name").unwrap();
            app1.gen_completions_to(
                "bk-over-ssh",
                shell.parse::<Shell>().unwrap(),
                &mut io::stdout(),
            );
        }
        ("rsync", Some(sub_matches)) => command::rsync::rsync_cmd_line(sub_matches)?,
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
        let sig_file_name = command::rsync::signature(old_file_name, None, None)?;
        eprintln!("sig_file_name: {:?}", sig_file_name);
        assert!(PathBuf::from(&sig_file_name).exists());
        let delta_file = command::rsync::delta_a_file(old_file_name, None, None, true)?;
        eprintln!("delta_file {:?}", delta_file);
        assert!(PathBuf::from(&delta_file).exists());
        let restored = command::rsync::restore_a_file(old_file_name, None, None)?;
        assert_eq!(
            hash_file_sha1(restored),
            hash_file_sha1(old_file_name.unwrap())
        );
        Ok(())
    }
}