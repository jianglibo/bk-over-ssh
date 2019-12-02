use crate::actions;
use crate::data_shape::{server, AppConf};
use crate::db_accesses::SqliteDbAccess;
use job_scheduler::{Job, JobScheduler};
use r2d2_sqlite::SqliteConnectionManager;
use std::time::Duration;

use super::*;

pub fn client_push_loops(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<(), failure::Error> {
    client_push_loops_follow_archive(app_conf, server_yml, false)
}

pub fn client_push_loops_follow_archive(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    let (progress_bar_join_handler, server_indicator_pairs) =
        load_server_indicator_pairs(app_conf, server_yml)?;

    client_push_loop_by_spawn(
        server_indicator_pairs,
        follow_archive,
        app_conf.mini_app_conf.as_service,
    )?;

    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn client_push_loop_by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
    as_service: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| client_push_loop_by_spawn_do(pair, follow_archive, as_service))
        .filter_map(|i| i)
        .collect::<Vec<thread::JoinHandle<_>>>();

    for child in handlers {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
    Ok(())
}

fn client_push_loop_by_spawn_do(
    pair: ServerAndIndicatorSqlite,
    follow_archive: bool,
    as_service: bool,
) -> Option<thread::JoinHandle<()>> {
    let (server, mut indicator) = pair;
    if as_service {
        Some(thread::spawn(move || {
            if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS)
            {
                let mut sched = JobScheduler::new();
                sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                    match server.client_push_loop() {
                        Ok(result) => {
                            indicator.pb_finish();
                            actions::write_dir_sync_result(&server, result.as_ref());
                            // archive when succeeded.
                            if follow_archive {
                                server.archive_local(&mut indicator).ok();
                                server.prune_backups().ok();
                            }
                        }
                        Err(err) => println!("client-push-loop failed: {:?}", err),
                    }
                }));

                eprintln!("entering sched ticking.");
                loop {
                    sched.tick();
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }))
    } else {
        match server.client_push_loop() {
            Ok(result) => {
                indicator.pb_finish();
                actions::write_dir_sync_result(&server, result.as_ref());
                // archive when succeeded.
                if follow_archive {
                    server.archive_local(&mut indicator).ok();
                    server.prune_backups().ok();
                }
            }
            Err(err) => println!("client-push-loop failed {:?}", err),
        }
        None
    }
}


pub fn client_pull_loops(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<(), failure::Error> {
    client_pull_loops_follow_archive(app_conf, server_yml, false)
}

pub fn client_pull_loops_follow_archive(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    let (progress_bar_join_handler, server_indicator_pairs) =
        load_server_indicator_pairs(app_conf, server_yml)?;

    client_pull_loop_by_spawn(
        server_indicator_pairs,
        follow_archive,
        app_conf.mini_app_conf.as_service,
    )?;

    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn client_pull_loop_by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
    as_service: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| client_pull_loop_by_spawn_do(pair, follow_archive, as_service))
        .filter_map(|i| i)
        .collect::<Vec<thread::JoinHandle<_>>>();

    for child in handlers {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
    Ok(())
}

fn client_pull_loop_by_spawn_do(
    pair: ServerAndIndicatorSqlite,
    follow_archive: bool,
    as_service: bool,
) -> Option<thread::JoinHandle<()>> {
    let (server, mut indicator) = pair;
    if as_service {
        Some(thread::spawn(move || {
            if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS)
            {
                let mut sched = JobScheduler::new();
                sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                    match server.client_pull_loop() {
                        Ok(result) => {
                            indicator.pb_finish();
                            actions::write_dir_sync_result(&server, result.as_ref());
                            // archive when succeeded.
                            if follow_archive {
                                server.archive_local(&mut indicator).ok();
                                server.prune_backups().ok();
                            }
                        }
                        Err(err) => println!("client-push-loop failed: {:?}", err),
                    }
                }));

                eprintln!("entering sched ticking.");
                loop {
                    sched.tick();
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }))
    } else {
        match server.client_pull_loop() {
            Ok(result) => {
                indicator.pb_finish();
                actions::write_dir_sync_result(&server, result.as_ref());
                // archive when succeeded.
                if follow_archive {
                    server.archive_local(&mut indicator).ok();
                    server.prune_backups().ok();
                }
            }
            Err(err) => println!("client-push-loop failed {:?}", err),
        }
        None
    }
}
