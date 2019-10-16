use crate::db_accesses::{SqliteDbAccess};
use rayon::prelude::*;

use crate::data_shape::{AppConf, Indicator, Server};
use r2d2_sqlite::SqliteConnectionManager;
use crate::actions;

use super::*;

pub fn sync_pull_dirs(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    server_yml: Option<&str>,
) -> Result<(), failure::Error> {
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
    //     match server.sync_pull_dirs(&mut indicator) {
    //         Ok(result) => {
    //             indicator.pb_finish();
    //             actions::write_dir_sync_result(&server, result.as_ref());
    //             if console_log {
    //                 let result_yml = serde_yaml::to_string(&result)
    //                     .expect("SyncDirReport should deserialize success.");
    //                 println!("{}:\n{}", server.get_host(), result_yml);
    //             }
    //         }
    //         Err(err) => println!("sync-pull-dirs failed: {:?}", err),
    //     }
    //     })
    // }).collect::<Vec<thread::JoinHandle<_>>>();

    // for child in handlers {
    //     // Wait for the thread to finish. Returns a result.
    //     let _ = child.join();
    // }

    servers.into_par_iter().for_each(|(server, mut indicator)| {
        match server.sync_pull_dirs(&mut indicator) {
            Ok(result) => {
                indicator.pb_finish();
                actions::write_dir_sync_result(&server, result.as_ref());
            }
            Err(err) => println!("sync-pull-dirs failed: {:?}", err),
        }
    });
    wait_progress_bar_finish(t);
    Ok(())
}