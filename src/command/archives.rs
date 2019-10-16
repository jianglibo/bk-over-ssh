
use crate::db_accesses::{SqliteDbAccess};
use log::*;
use rayon::prelude::*;

use crate::data_shape::{AppConf, Indicator, Server};
use r2d2_sqlite::SqliteConnectionManager;
use super::*;

pub fn archive_local(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
    prune_op: Option<&str>,
    prune_only_op: Option<&str>,
) -> Result<(), failure::Error>
{
    let mut servers: Vec<(Server<SqliteConnectionManager, SqliteDbAccess>, Indicator)> = Vec::new();

    if let Some(server_yml) = server_yml {
        let server = load_server_yml_by_name(app_conf, server_yml, true)?;
        servers.push(server);
    } else {
        servers.append(&mut load_all_server_yml(app_conf, true));
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

    wait_progress_bar_finish(t);
    Ok(())
}