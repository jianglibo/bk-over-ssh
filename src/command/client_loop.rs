use crate::actions;
use crate::data_shape::{server, AppConf, Indicator, Server};
use crate::db_accesses::{SqliteDbAccess, CountItemParam};
use job_scheduler::{Job, JobScheduler};
use r2d2_sqlite::SqliteConnectionManager;
use rayon::prelude::*;
use std::time::Duration;
use log::*;

use super::*;

pub fn client_loops(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<(), failure::Error> {
    client_loops_follow_archive(app_conf, server_yml, false)
}

pub fn client_loops_follow_archive(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    if app_conf.mini_app_conf.app_role != AppRole::PullHub {
        bail!("only when app-role is PullHub can call sync_pull_dirs");
    }
    let (progress_bar_join_handler, server_indicator_pairs) =
        load_server_indicator_pairs(app_conf, server_yml)?;

    client_loop_by_spawn(server_indicator_pairs, follow_archive)?;

    wait_progress_bar_finish(progress_bar_join_handler);
    Ok(())
}

/// If invoking with parameter as-service, this branch will be called.
/// Because it is a long running thread, We should choose to connect to server when schedule time is meet.
/// and disconnect from server when task is done.
fn client_loop_by_spawn(
    server_indicator_pairs: Vec<ServerAndIndicatorSqlite>,
    follow_archive: bool,
) -> Result<(), failure::Error> {
    let handlers = server_indicator_pairs
        .into_iter()
        .map(|pair| client_loop_by_spawn_do(pair, follow_archive))
        .collect::<Vec<thread::JoinHandle<_>>>();

    for child in handlers {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
    Ok(())
}

fn client_loop_by_spawn_do(
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