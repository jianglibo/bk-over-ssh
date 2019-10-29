use crate::db_accesses::SqliteDbAccess;
use log::*;
use std::fs;

use crate::data_shape::AppConf;
use r2d2_sqlite::SqliteConnectionManager;

use super::*;

pub fn create_remote_db<'a>(
    app_conf: &AppConf<SqliteConnectionManager, SqliteDbAccess>,
    m: &'a clap::ArgMatches<'a>,
) -> Result<bool, failure::Error> {
    if let ("create-remote-db", Some(sub_matches)) = m.subcommand() {
        let (mut server, _indicator) =
            load_server_yml(&app_conf, sub_matches.value_of("server-yml"), false)?;
        let db_type = sub_matches.value_of("db-type").unwrap_or("sqlite");
        let force = sub_matches.is_present("force");
        server.connect()?;
        server.create_remote_db(db_type, force)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn create_db<'a>(
    app_conf: &mut AppConf<SqliteConnectionManager, SqliteDbAccess>,
    m: &'a clap::ArgMatches<'a>,
) -> Result<bool, failure::Error> {
    if let ("create-db", Some(sub_matches)) = m.subcommand() {
        let db_type = sub_matches.value_of("db-type").unwrap_or("sqlite");
        let force = sub_matches.is_present("force");
        if "sqlite" == db_type {
            // create server's db.
            if let Some(server_yml) = sub_matches.value_of("server-yml") {
                let (mut server, _indicator) =
                    load_server_yml_by_name(&app_conf, server_yml, false)?;
                if force {
                    info!("removing server side db file: {:?}", server.get_db_file());
                    fs::remove_file(server.get_db_file()).ok();
                }
                let sqlite_db_access = SqliteDbAccess::new(server.get_db_file());
                sqlite_db_access.create_database()?;
                server.set_db_access(sqlite_db_access);
            } else {
                if force {
                    info!(
                        "removing app side db file: {:?}",
                        app_conf.get_sqlite_db_file()
                    );
                    fs::remove_file(app_conf.get_sqlite_db_file()).ok();
                }
                let sqlite_db_access = SqliteDbAccess::new(app_conf.get_sqlite_db_file());
                sqlite_db_access.create_database()?;
                app_conf.set_db_access(sqlite_db_access);
            }
        } else {
            println!("unsupported database: {}", db_type);
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
