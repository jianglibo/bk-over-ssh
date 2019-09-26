use crate::actions::hash_file_sha1;
use chrono::{DateTime, Utc};
use failure;
use log::*;
use r2d2;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::{Path, PathBuf};

pub type SqlitePool = r2d2::Pool<SqliteConnectionManager>;

pub fn create_sqlite_pool(db_file: impl AsRef<Path>) -> SqlitePool {
    let db_file = db_file.as_ref();
    let manager = SqliteConnectionManager::file(db_file)
        .with_init(|c| c.execute_batch("PRAGMA foreign_keys=1;"));
    r2d2::Pool::new(manager).unwrap()
}

#[derive(Debug, Default)]
pub struct RemoteFileItemInDb {
    id: i64,
    path: String,
    sha1: Option<String>,
    len: i64,
    modified: Option<DateTime<Utc>>,
    created: Option<DateTime<Utc>>,
    dir_id: i64,
}

impl RemoteFileItemInDb {
    pub fn from_path(
        base_path: impl AsRef<Path>,
        path: PathBuf,
        skip_sha1: bool,
        dir_id: i64,
    ) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 {
                    hash_file_sha1(&path)
                } else {
                    Option::<String>::None
                };
                let relative_o = path.strip_prefix(&base_path).ok().and_then(|p| p.to_str());
                if let Some(relative) = relative_o {
                    return Some(Self {
                        id: 0,
                        path: relative.to_string(),
                        sha1,
                        len: metadata.len() as i64,
                        modified: metadata.modified().ok().map(Into::into),
                        created: metadata.created().ok().map(Into::into),
                        dir_id,
                    });
                } else {
                    error!("RemoteFileItem path name to_str() failed. {:?}", path);
                }
                // }
            }
            Err(err) => {
                error!("RemoteFileItem from_path failed: {:?}, {:?}", path, err);
            }
        }
        None
    }
}

pub fn insert_directory(pool: SqlitePool, path: impl AsRef<str>) -> Result<i64, failure::Error> {
    let conn = pool.get().unwrap();
    let count = conn.execute(
        "INSERT INTO directory (path) VALUES (?1)",
        params![path.as_ref()],
    )?;
    if count != 1 {
        bail!("insert directory failed, execute return not eq to 1.");
    }
    Ok(conn.last_insert_rowid())
}

pub fn insert_remote_file_item(pool: SqlitePool, rfi: RemoteFileItemInDb) {
    let conn = pool.get().unwrap();
    match conn.execute(
            "INSERT INTO directory (path, sha1, len, time_modified, time_created, dir_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![rfi.path, rfi.sha1, rfi.len, rfi.modified, rfi.created, rfi.dir_id],
        ) {
            Ok(count) => {
                if count != 1 {
                    error!("insert directory failed, execute return not eq to 1. {:?}", rfi);
                }
            },
            Err(e) => error!("insert remote file failed: {:?}, {:?}", rfi, e)
        }
}

pub fn create_sqlite_database(pool: SqlitePool) -> Result<(), failure::Error> {
    let conn = pool.get().unwrap();
    conn.execute_batch(
        "BEGIN;
            CREATE TABLE directory (
                id  INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE
            );
             CREATE TABLE remote_file_item (
                   id              INTEGER PRIMARY KEY,
                   path            TEXT NOT NULL,
                   sha1            TEXT,
                   len             INTEGER DEFAULT 0,
                   time_modified   TEXT,
                   time_created    TEXT,
                   dir_id          INTEGER NOT NULL,
                   FOREIGN KEY(dir_id) REFERENCES directory(id)
                   );
             CREATE UNIQUE INDEX dir_path ON remote_file_item (path, dir_id);
                COMMIT;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use std::str::FromStr;
    use rusqlite::{NO_PARAMS};
    use chrono::{DateTime, SecondsFormat, Utc};

    pub fn create_sqlite_database_1(pool: &SqlitePool) -> Result<usize, failure::Error> {
        let r = pool.get().unwrap().execute(
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
        let pool = tutil::create_sqlite_mem_pool();
        create_sqlite_database(pool.clone())?;

        let insert_result = pool.get().unwrap().execute(
            "INSERT INTO remote_file_item (path, time_created, dir_id)
                  VALUES (?1, ?2, ?3)",
            params!["abc", Utc::now(), 66],
        );

        assert!(insert_result.is_err(), "insert should be failed.");

        let last_row_id = pool.get().unwrap().last_insert_rowid();
        let c = pool.get().unwrap();
        let mut stmt = c.prepare("SELECT id, path, dir_id, time_created FROM remote_file_item")?;
        let rows: Vec<(String, i64, i64)> = stmt
            .query_map(NO_PARAMS, |row| Ok((row.get(1)?, row.get(0)?, row.get(2)?)))?
            .filter_map(|r| r.ok())
            .collect();
        rows.iter().for_each(|p| println!("{:?}", p));

        assert!(last_row_id > 0);
        Ok(())
    }

    #[test]
    fn t_create_database_1() -> Result<(), failure::Error> {
        let pool = tutil::create_sqlite_mem_pool();

        println!("auto_commit: {}", pool.get().unwrap().is_autocommit());

        let r = create_sqlite_database_1(&pool)?;
        assert_eq!(r, 0, "first execution should return 0");

        if let Err(_err) = create_sqlite_database_1(&pool) {
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
            modified: None,
            created: Some(now),
            dir_id: 0,
        };

        let count = pool.get().unwrap().execute(
            "INSERT INTO remote_file_item (path, time_created)
                  VALUES (?1, ?2)",
            // &[&me.path, &me.sha1 as &ToSql, &Null, &Null, &me.time_created as &ToSql],
            params![me.path, me.created],
        )?;

        assert_eq!(count, 1, "should effect one item.");
        let c = pool.get().unwrap();
        let mut stmt = c.prepare("SELECT id, path, time_created FROM remote_file_item")?;

        let person_iter = stmt.query_map(NO_PARAMS, |row| {
            Ok(RemoteFileItemInDb {
                id: row.get(0)?,
                path: row.get(1)?,
                created: row.get(2)?,
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
            c.get(0).as_ref().unwrap().created,
            Some(now),
            "archive from db should be same."
        );

        Ok(())
    }
}
