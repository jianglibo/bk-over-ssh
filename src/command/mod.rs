pub mod misc;
pub mod sync_dirs;
pub mod archives;
pub mod rsync;


use crate::db_accesses::{DbAccess, SqliteDbAccess};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration};
use std::{fs, io::Write};

use crate::data_shape::{AppConf, Indicator, Server, CONF_FILE_NAME, AppRole};
use r2d2_sqlite::SqliteConnectionManager;

pub use sync_dirs::{sync_pull_dirs, sync_push_dirs};
pub use archives::{archive_local};

pub fn wait_progress_bar_finish(jh: Option<thread::JoinHandle<()>>) {
    if let Some(t) = jh {
        t.join().expect("wait_progress_bar_finish should succeeded.");
    }
}


pub fn join_multi_bars(multi_bar: Option<Arc<MultiProgress>>) -> Option<thread::JoinHandle<()>> {
    if let Some(mb) = multi_bar {
        Some(thread::spawn(move || {
            mb.join().expect("join_multi_bars should succeeded.");
        }))
    } else {
        None
    }
}

pub fn delay_exec(delay: &str) {
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

pub fn load_server_yml(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    open_db: bool,
) -> Result<(Server<SqliteConnectionManager, SqliteDbAccess>, Indicator), failure::Error> {
    load_server_yml_by_name(app_conf, server_yml.expect("server-yml should exist."), open_db)
}

// pub fn load_this_server_yml(
//     app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
//     open_db: bool,
// ) -> Result<(Server<SqliteConnectionManager, SqliteDbAccess>, Indicator), failure::Error> {
//         let mut s = app_conf.load_this_server_yml()?;
//         if open_db {
//             let sqlite_db_access = SqliteDbAccess::new(s.0.get_db_file());
//             s.0.set_db_access(sqlite_db_access);
//         }
//     Ok(s)
// }

pub fn load_all_server_yml(app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>, open_db: bool) -> Vec<(Server<SqliteConnectionManager, SqliteDbAccess>, Indicator)> {
    app_conf.load_all_server_yml().into_iter().map(|mut s|{
        if open_db {
            let sqlite_db_access = SqliteDbAccess::new(s.0.get_db_file());
            s.0.set_db_access(sqlite_db_access);
        }
        s
    }).collect()
}


pub fn load_server_yml_by_name(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    name: &str,
    open_db: bool,
) -> Result<(Server<SqliteConnectionManager, SqliteDbAccess>, Indicator), failure::Error> {
    let mut s = app_conf.load_server_yml(name)?;
    if open_db {
        let sqlite_db_access = SqliteDbAccess::new(s.0.get_db_file());
        s.0.set_db_access(sqlite_db_access);
    }
    Ok(s)
}

pub fn process_app_config<M, D>(
    conf: Option<&str>,
    app_role_op: Option<AppRole>,
    re_try: bool,
) -> Result<AppConf<M, D>, failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let message_pb = ProgressBar::new_spinner();
    message_pb.enable_steady_tick(200);
    let app_conf = match AppConf::guess_conf_file(conf, app_role_op.as_ref().cloned().unwrap_or(AppRole::PullHub)) {
        Ok(cfg) => {
            if let Some(cfg) = cfg {
                cfg
            } else if !re_try {
                let bytes = include_bytes!("../app_config_demo.yml");
                let path = env::current_exe()?
                    .parent()
                    .expect("current_exec's parent folder should exists.")
                    .join(CONF_FILE_NAME);
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&path)?;
                file.write_all(bytes)?;
                message_pb.finish_and_clear();
                return process_app_config(conf, app_role_op, true);
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