use crate::actions;
use crate::data_shape::{server, AppConf, Indicator, Server};
use crate::db_accesses::SqliteDbAccess;
use job_scheduler::{Job, JobScheduler};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::prelude::*;
use std::time::Duration;

use super::*;

pub type ServerAndIndicatorSqlite = (Server<SqliteConnectionManager, SqliteDbAccess>, Indicator);

pub fn sync_push_dirs(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    force: bool,
) -> Result<(), failure::Error> {
    if app_conf.mini_app_conf.app_role != AppRole::ActiveLeaf {
        bail!("only when app-role is ActiveLeaf can call sync_push_dirs");
    }
    let (progress_bar_join_handler, server_indicator_pairs) =
        load_server_indicator_pairs(app_conf, server_yml)?;

    server_indicator_pairs
        .into_par_iter()
        .for_each(|(mut server, mut indicator)| {
            if force && server.get_db_file().exists() {
                server.db_access.take();
                fs::remove_file(server.get_db_file()).expect("should remove db_file.");
                let sqlite_db_access = SqliteDbAccess::new(server.get_db_file());
                sqlite_db_access
                    .create_database()
                    .expect("should recreate database.");
                server.set_db_access(sqlite_db_access);
            }
            match server.sync_push_dirs(&mut indicator) {
                Ok(result) => {
                    indicator.pb_finish();
                    actions::write_dir_sync_result(&server, result.as_ref());
                }
                Err(err) => println!("sync-push-dirs failed: {:?}", err),
            }
        });
    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

pub fn sync_pull_dirs(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<(), failure::Error> {
    sync_pull_dirs_follow_archive(app_conf, server_yml, false)
}

pub fn sync_pull_dirs_follow_archive(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    if app_conf.mini_app_conf.app_role != AppRole::PullHub {
        bail!("only when app-role is PullHub can call sync_pull_dirs");
    }
    let (progress_bar_join_handler, server_indicator_pairs) =
        load_server_indicator_pairs(app_conf, server_yml)?;
    if app_conf.mini_app_conf.as_service {
        by_spawn(server_indicator_pairs, follow_archive)?;
    } else {
        by_par_iter(server_indicator_pairs, follow_archive)?;
    }
    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

fn by_par_iter(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    server_indicator_pairs
        .into_par_iter()
        .for_each(|(server, mut indicator)| {
            match server.sync_pull_dirs(&mut indicator, false) {
                Ok(result) => {
                    indicator.pb_finish();
                    actions::write_dir_sync_result(&server, result.as_ref());
                }
                Err(err) => println!("sync-pull-dirs failed: {:?}", err),
            }
            if follow_archive {
                server.archive_local(&mut indicator).ok();
                server.prune_backups().ok();
            }
        });
    Ok(())
}

fn by_spawn_do(pair: ServerAndIndicatorSqlite, follow_archive: bool) -> thread::JoinHandle<()> {
    let (server, mut indicator) = pair;
    thread::spawn(move || {
        if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS) {
            let mut sched = JobScheduler::new();
            sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                match server.sync_pull_dirs(&mut indicator, true) {
                    Ok(result) => {
                        indicator.pb_finish();
                        actions::write_dir_sync_result(&server, result.as_ref());
                    }
                    Err(err) => println!("sync-pull-dirs failed: {:?}", err),
                }
                if follow_archive {
                    server.archive_local(&mut indicator).ok();
                    server.prune_backups().ok();
                }
            }));

            eprintln!("entering sched ticking.");
            loop {
                sched.tick();
                thread::sleep(Duration::from_millis(500));
            }
        }
    })
}

fn by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| by_spawn_do(pair, follow_archive))
        .collect::<Vec<thread::JoinHandle<_>>>();

    for child in handlers {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
    Ok(())
}

fn load_server_indicator_pairs(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<
    (
        Option<thread::JoinHandle<()>>,
        Vec<ServerAndIndicatorSqlite>,
    ),
    failure::Error,
> {
    let mut server_indicator_pairs: Vec<ServerAndIndicatorSqlite> = Vec::new();
    if let Some(server_yml) = server_yml {
        let server = load_server_yml_by_name(app_conf, server_yml, true)?;
        server_indicator_pairs.push(server);
    } else {
        server_indicator_pairs.append(&mut load_all_server_yml(app_conf, true));
    }
    // all progress bars already create from here on.
    let progress_bar_join_handler = join_multi_bars(app_conf.progress_bar.clone());

    if server_indicator_pairs.is_empty() {
        println!("found no server yml!");
    } else {
        println!(
            "found {} server yml files. start processing...",
            server_indicator_pairs.len()
        );
    }
    server_indicator_pairs
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
    Ok((progress_bar_join_handler, server_indicator_pairs))
}
