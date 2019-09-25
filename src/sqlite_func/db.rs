use crate::data_shape::RemoteFileItem;
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use failure;
use r2d2;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use rusqlite::{types::Null, Connection, Result as SqliteResult, NO_PARAMS};

pub fn create_sqlite_pool(db_file: impl AsRef<str>) -> r2d2::Pool<SqliteConnectionManager> {
    let db_file = db_file.as_ref();
    let manager = SqliteConnectionManager::file(db_file);
    r2d2::Pool::new(manager).unwrap()
}

#[derive(Debug, Default)]
pub struct RemoteFileItemInDb {
    id: i64,
    path: String,
    sha1: Option<String>,
    len: i64,
    time_modified: Option<DateTime<Utc>>,
    time_created: Option<DateTime<Utc>>,
}

pub fn create_sqlite_database(pool: &r2d2::Pool<SqliteConnectionManager>) -> Result<usize, failure::Error> {
    let conn = pool.get().unwrap();
    let r = conn.execute(
        "CREATE TABLE remote_file_item (
                  id              INTEGER PRIMARY KEY,
                  path            TEXT NOT NULL UNIQUE,
                  sha1            TEXT,
                  len             INTEGER DEFAULT 0,
                  time_modified   TEXT,
                  time_created    TEXT
                  )",
        NO_PARAMS,
    )?;
    Ok(r)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    pub fn create_sqlite_database_1(conn: &Connection) -> Result<usize, failure::Error> {
    let r = conn.execute(
        "CREATE TABLE remote_file_item (
                  id              INTEGER PRIMARY KEY,
                  path            TEXT NOT NULL UNIQUE,
                  sha1            TEXT,
                  len             INTEGER DEFAULT 0,
                  time_modified   TEXT,
                  time_created    TEXT
                  )",
        NO_PARAMS,
    )?;
    Ok(r)
}


    #[test]
    fn t_create_database() -> Result<(), failure::Error> {
        let conn = Connection::open_in_memory()?;

        println!("auto_commit: {}", conn.is_autocommit());

        let r = create_sqlite_database_1(&conn)?;
        assert_eq!(r, 0, "first execution should return 0");

        if let Err(_err) = create_sqlite_database_1(&conn) {
            println!("already exists");
        } else {
            assert!(false, "should throw exception.");
        }

        // 2019-09-22 13:56:08.410951111 UTC
        let now = Utc::now();
        // 2019-09-22T14:07:21.722444951Z
        // https://sqlite.org/autoinc.html
        let me = RemoteFileItemInDb {
            id: 0,
            path: "abc".to_string(),
            sha1: None,
            len: 55,
            time_modified: None,
            time_created: Some(now),
        };

        let count = conn.execute(
            "INSERT INTO remote_file_item (path, time_created)
                  VALUES (?1, ?2)",
            // &[&me.path, &me.sha1 as &ToSql, &Null, &Null, &me.time_created as &ToSql],
            params![me.path, me.time_created],
        )?;

        assert_eq!(count, 1, "should effect one item.");
        let mut stmt = conn.prepare("SELECT id, path, time_created FROM remote_file_item")?;

        let person_iter = stmt.query_map(NO_PARAMS, |row| {
            Ok(RemoteFileItemInDb {
                id: row.get(0)?,
                path: row.get(1)?,
                time_created: row.get(2)?,
                ..RemoteFileItemInDb::default() // sha1: row.get(2)?,
                                                // len: row.get(3)?,
                                                // time_modified: row.get(4)?,
                                                // time_created: row.get(5)?,
            })
        })?;

        let ts_debug = format!("{:?}", now); // equal to to_rfc3339_opts.
        assert_ne!(ts_debug, now.to_rfc3339());
        assert_eq!(ts_debug, now.to_rfc3339_opts(SecondsFormat::Nanos, true));
        println!("{}", ts_debug);

        let now1 = DateTime::<Utc>::from_str(ts_debug.as_str());

        assert_eq!(Ok(now), now1);

        let c = person_iter
            .filter_map(|pp| match pp {
                Ok(pp) => Some(pp),
                Err(err) => {
                    println!("{:?}", err);
                    None
                }
            })
            .collect::<Vec<RemoteFileItemInDb>>();
        assert_eq!(c.len(), 1, "should exist one item.");

        assert_eq!(
            c.get(0).as_ref().unwrap().id,
            1,
            "id should automatically increased to 1."
        );

        assert_eq!(c.get(0).as_ref().unwrap().path, "abc");

        assert_eq!(
            c.get(0).as_ref().unwrap().time_created,
            Some(now),
            "archive from db should be same."
        );

        Ok(())
    }
}
