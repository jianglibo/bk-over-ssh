// use crate::actions;
use crate::data_shape::{server, AppConf};
// use crate::db_accesses::SqliteDbAccess;
use job_scheduler::{Job, JobScheduler};
// use r2d2_sqlite::SqliteConnectionManager;
use std::time::Duration;

use super::*;

pub fn client_push_loops(
    app_conf: &AppConf,
    server_yml: Option<&str>,
    follow_archive: bool,
    as_service: bool,
    open_db: bool,
) -> Result<(), failure::Error> {

        let servers = if let Some(server_yml) = server_yml {
        vec![app_conf.load_server_from_yml(server_yml, open_db)?]
    } else {
        app_conf.load_all_server_yml(false)
    };

    
    client_push_loop_by_spawn(servers, follow_archive, as_service)
}


/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn client_push_loop_by_spawn(
    servers: Vec<Server>,
    follow_archive: bool,
    as_service: bool,
) -> Result<(), failure::Error> {
    let handlers = servers
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
    server: Server,
    follow_archive: bool,
    as_service: bool,
) -> Option<thread::JoinHandle<()>> {
    if as_service {
        Some(thread::spawn(move || {
            if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS)
            {
                let mut sched = JobScheduler::new();
                sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                    match server.client_push_loop(follow_archive) {
                        Ok(_result) => {
                            // indicator.pb_finish();
                            // actions::write_dir_sync_result(&server, result.as_ref());
                            // archive when succeeded.
                            // if follow_archive {
                            //     server.archive_local().ok();
                            //     server.prune_backups().ok();
                            // }
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
        match server.client_push_loop(follow_archive) {
            Ok(_result) => {
                // indicator.pb_finish();
                // actions::write_dir_sync_result(&server, result.as_ref());
                // archive when succeeded.
                if follow_archive {
                    server.archive_local().ok();
                    server.prune_backups().ok();
                }
            }
            Err(err) => println!("client-push-loop failed {:?}", err),
        }
        None
    }
}


pub fn client_pull_loops(
    app_conf: &AppConf,
    server_yml: Option<&str>,
    follow_archive: bool,
    as_service: bool,
    open_db: bool,
) -> Result<(), failure::Error> {
    let servers = if let Some(server_yml) = server_yml {
        vec![app_conf.load_server_from_yml(server_yml, open_db)?]
    } else {
        app_conf.load_all_server_yml(false)
    };
    client_pull_loop_by_spawn(servers, follow_archive, as_service)
}


/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn client_pull_loop_by_spawn(
    server_indicator_pairs: Vec<Server>,
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
    server: Server,
    follow_archive: bool,
    as_service: bool,
) -> Option<thread::JoinHandle<()>> {
    if as_service {
        Some(thread::spawn(move || {
            if let Some(schedule_item) = server.find_cron_by_name(server::CRON_NAME_SYNC_PULL_DIRS)
            {
                let mut sched = JobScheduler::new();
                sched.add(Job::new(schedule_item.cron.parse().unwrap(), || {
                    match server.client_pull_loop() {
                        Ok(_result) => {
                            if follow_archive {
                                server.archive_local().ok();
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
            Ok(_result) => {
                if follow_archive {
                    server.archive_local().ok();
                    server.prune_backups().ok();
                }
            }
            Err(err) => println!("client-push-loop failed {:?}", err),
        }
        None
    }
}
