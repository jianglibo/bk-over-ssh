use crate::actions;
use crate::data_shape::{server, AppConf, Indicator, Server};
use crate::db_accesses::{SqliteDbAccess, CountItemParam};
use job_scheduler::{Job, JobScheduler};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::prelude::*;
use std::time::Duration;
use log::*;

use super::*;

pub type ServerAndIndicatorSqlite = (Server<SqliteConnectionManager, SqliteDbAccess>, Indicator);

/// When force parameter is true, the db file will be deleted and that means all file will be upload again.
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

    if app_conf.mini_app_conf.as_service {
        push_by_spawn(server_indicator_pairs, force)?;
    } else {
        push_by_par_iter(server_indicator_pairs, force)?;
    }

    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}


fn push_one_server(
    server: &mut Server<SqliteConnectionManager, SqliteDbAccess>,
    indicator: &mut Indicator,
    force: bool,
) {
    let mut need_reset = force && server.get_db_file().exists();
    if !need_reset {
        let rfs = server.count_remote_files().unwrap_or(0);
        if let Some(db_access) = server.db_access.as_ref() {
            if let Ok(c) = db_access.count_relative_file_item(CountItemParam::default()) {
                if c != rfs {
                    need_reset = true;
                }
            } else {
                error!("count_relative_file_item failed");
            }
        }
    }
    if need_reset {
        server.db_access.take();
        fs::remove_file(server.get_db_file()).expect("should remove db_file.");
        let sqlite_db_access = SqliteDbAccess::new(server.get_db_file());
        sqlite_db_access
            .create_database()
            .expect("should recreate database.");
        server.set_db_access(sqlite_db_access);
    }

    match server.sync_push_dirs(indicator) {
        Ok(result) => {
            indicator.pb_finish();
            actions::write_dir_sync_result(&server, result.as_ref());
        }
        Err(err) => println!("sync-push-dirs failed: {:?}", err),
    }
}

fn push_by_par_iter(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    force: bool,
) -> Result<(), failure::Error> {
    server_indicator_pairs
        .into_par_iter()
        .for_each(|(mut server, mut indicator)| {
            push_one_server(&mut server, &mut indicator, force);
        });
    Ok(())
}

fn push_by_spawn_do(mut pair: ServerAndIndicatorSqlite, force: bool) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Some(schedule_item) = pair.0.find_cron_by_name(server::CRON_NAME_SYNC_PUSH_DIRS) {
            let mut sched = JobScheduler::new();
            sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                push_one_server(&mut pair.0, &mut pair.1, force);
            }));

            eprintln!("entering sched ticking.");
            loop {
                sched.tick();
                thread::sleep(Duration::from_millis(500));
            }
        }
    })
}

fn push_by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    force: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| push_by_spawn_do(pair, force))
        .collect::<Vec<thread::JoinHandle<_>>>();

    for child in handlers {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
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
        pull_by_spawn(server_indicator_pairs, follow_archive)?;
    } else {
        pull_by_par_iter(server_indicator_pairs, follow_archive)?;
    }
    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

fn pull_by_par_iter(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    server_indicator_pairs
        .into_par_iter()
        .for_each(|(mut server, mut indicator)| {
            match server.sync_pull_dirs(&mut indicator, false) {
                Ok(result) => {
                    indicator.pb_finish();
                    actions::write_dir_sync_result(&server, result.as_ref());
                    // archive when succeeded.
                    if follow_archive {
                        server.archive_local(&mut indicator).ok();
                        server.prune_backups().ok();
                    }
                }
                Err(err) => println!("sync-pull-dirs failed: {:?}", err),
            }
        });
    Ok(())
}

fn pull_by_spawn_do(
    pair: ServerAndIndicatorSqlite,
    follow_archive: bool,
) -> thread::JoinHandle<()> {
    let (mut server, mut indicator) = pair;
    thread::spawn(move || {
        if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS) {
            let mut sched = JobScheduler::new();
            sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                match server.sync_pull_dirs(&mut indicator, true) {
                    Ok(result) => {
                        indicator.pb_finish();
                        actions::write_dir_sync_result(&server, result.as_ref());
                        // archive when succeeded.
                        if follow_archive {
                            server.archive_local(&mut indicator).ok();
                            server.prune_backups().ok();
                        }
                    }
                    Err(err) => println!("sync-pull-dirs failed: {:?}", err),
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

/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn pull_by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| pull_by_spawn_do(pair, follow_archive))
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
    if !app_conf.mini_app_conf.as_service {
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
    }
    Ok((progress_bar_join_handler, server_indicator_pairs))
}
