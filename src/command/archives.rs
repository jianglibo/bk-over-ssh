
use log::*;
use rayon::prelude::*;

use crate::data_shape::{AppConf};

pub fn archive_local(
    app_conf: &AppConf,
    server_yml: Option<&str>,
    prune: bool,
    prune_only: bool,
) -> Result<(), failure::Error>
{
    let servers = if let Some(server_yml) = server_yml {
        let server = app_conf.load_server_from_yml(server_yml, false)?;
        vec![server]
    } else {
        app_conf.load_all_server_yml(false)
    };

    if servers.is_empty() {
        println!("found no server yml!");
    } else {
        println!(
            "found {} server yml files. start processing...",
            servers.len()
        );
    }
    #[allow(clippy::suspicious_map)]
    servers
        .into_par_iter()
        .map(|server| {
            if prune {
                if let Err(err) = server.archive_local() {
                    error!("{:?}", err);
                    eprintln!("{:?}", err);
                }
                if let Err(err) = server.prune_backups() {
                    error!("{:?}", err);
                    eprintln!("{:?}", err);
                }
            } else if prune_only {
                if let Err(err) = server.prune_backups() {
                    error!("{:?}", err);
                    eprintln!("{:?}", err);
                }
            } else if let Err(err) = server.archive_local() {
                error!("{:?}", err);
                eprintln!("{:?}", err);
            }
        })
        .count();
    Ok(())
}