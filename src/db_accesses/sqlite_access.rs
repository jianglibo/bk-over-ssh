use super::{DbAccess, RemoteFileItemInDb};
use failure;
use log::*;
use r2d2;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Row, NO_PARAMS};
use std::path::Path;

pub type SqlitePool = r2d2::Pool<SqliteConnectionManager>;

#[derive(Debug)]
pub struct SqliteDbAccess(SqlitePool);

impl Clone for SqliteDbAccess {
    fn clone(&self) -> SqliteDbAccess {
        SqliteDbAccess(self.0.clone())
    }
}

impl SqliteDbAccess {
    pub fn new(db_file: impl AsRef<Path>) -> Self {
        let db_file = db_file.as_ref();
        let manager = SqliteConnectionManager::file(db_file)
            .with_init(|c| c.execute_batch("PRAGMA foreign_keys=1;"));
        let pool = r2d2::Pool::new(manager).unwrap();
        Self(pool)
    }
    #[allow(dead_code)]
    pub fn new_mem() -> Self {
        let manager = SqliteConnectionManager::memory()
            .with_init(|c| c.execute_batch("PRAGMA foreign_keys=1;"));
        let pool = r2d2::Pool::new(manager).unwrap();
        Self(pool)
    }

    pub fn get_pool(&self) -> &SqlitePool {
        &self.0
    }
}

fn map_to_file_item(row: &Row) -> Result<RemoteFileItemInDb, rusqlite::Error> {
    Ok(RemoteFileItemInDb {
        id: row.get(0)?,
        path: row.get(1)?,
        sha1: row.get(2)?,
        len: row.get(3)?,
        modified: row.get(4)?,
        created: row.get(5)?,
        dir_id: row.get(6)?,
        changed: row.get(7)?,
    })
}

impl DbAccess<SqliteConnectionManager> for SqliteDbAccess {
    fn insert_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error> {
        let conn = self.get_pool().get().unwrap();
        let path = path.as_ref();
        if let Err(err) = conn.execute("INSERT INTO directory (path) VALUES (?1)", params![path]) {
            if let rusqlite::Error::SqliteFailure(_, Some(desc)) = &err {
                if desc.contains("UNIQUE constraint failed") {
                    warn!("{}", err);
                    return self.find_directory(path);
                } else {
                    bail!("{}", err);
                }
            } else {
                bail!("{}", err);
            }
        }
        Ok(conn.last_insert_rowid())
    }

    fn find_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error> {
        let conn = self.0.get().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM directory where path = ?1")?;
        Ok(stmt.query_row(params![path.as_ref()], |row| row.get(0))?)
    }

    fn find_remote_file_item(
        &self,
        dir_id: i64,
        path: impl AsRef<str>,
    ) -> Result<RemoteFileItemInDb, failure::Error> {
        let conn = self.0.get().unwrap();
        let path = path.as_ref();
        let r = match conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id, changed FROM remote_file_item where dir_id = :dir_id and path = :path") {
            Ok(mut stmt) => {
        match stmt.query_row_named(
            named_params! {
                ":dir_id": dir_id,
                ":path": path,
            },
            map_to_file_item
        ){
            Ok(r) => r,
            Err(e) => {
                trace!("find file failed: {:?}", e);
                bail!("find file failed: {:?}", e);
            }
        }

            }
            Err(e) => {
                trace!("prepare find stmt failed: {:?}", e);
                bail!("prepare find stmt failed: {:?}", e);

            }
        };
        Ok(r)
    }

    fn iterate_all_file_items<P>(&self, processor: P) -> (usize, usize)
    where
        P: Fn(RemoteFileItemInDb) -> (),
    {
        let conn = self.0.get().unwrap();
        // let mut stmt = conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id FROM directory where dir_id = :dir_id and path = :path")?;
        (0, 0)
    }

    fn iterate_files_by_directory<F>(&self, mut processor: F) -> Result<(), failure::Error>
    where
        F: FnMut((Option<RemoteFileItemInDb>, Option<String>)) -> (),
    {
        let conn = self.get_pool().get().unwrap();
        let mut stmt = conn.prepare("SELECT id, path FROM directory")?;
        let dirs = stmt.query_map(NO_PARAMS, |row| Ok((row.get(0)?, row.get(1)?)))?;

        for dir in dirs {
            let dir = dir?;
            let dir_id: i64 = dir.0;
            let path = dir.1;
            processor((None, Some(path)));
            let mut stmt = conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id, changed FROM remote_file_item where dir_id = :dir_id")?;
            let files = stmt.query_map_named(
                named_params! {
                    ":dir_id": dir_id
                },
                map_to_file_item,
            )?;

            files
                .filter_map(|fi| fi.ok())
                .for_each(|fi| processor((Some(fi), None)));
        }

        Ok(())
    }

    fn count_directory(&self) -> Result<u64, failure::Error> {
        let conn = self.0.get().unwrap();
        let mut stmt = conn.prepare("SELECT count(id) FROM directory")?;
        let i: i64 = stmt.query_row(NO_PARAMS, |row| row.get(0))?;
        Ok(i as u64)
    }

    fn count_remote_file_item(&self, changed: Option<bool>) -> Result<u64, failure::Error> {
        let conn = self.0.get().unwrap();
        let i: i64 = if let Some(b) = changed {
            let mut stmt =
                conn.prepare("SELECT count(id) FROM remote_file_item WHERE changed = :changed")?;
            stmt.query_row_named(
                named_params! {
                    ":changed": b
                },
                |row| row.get(0),
            )?
        } else {
            let mut stmt = conn.prepare("SELECT count(id) FROM remote_file_item")?;
            stmt.query_row(NO_PARAMS, |row| row.get(0))?
        };
        Ok(i as u64)
    }

    fn insert_or_update_remote_file_item(&self, rfi: RemoteFileItemInDb) {
        let conn = self.0.get().unwrap();
        if let Ok(rfi_db) = self.find_remote_file_item(rfi.dir_id, rfi.path.as_str()) {
            if rfi_db.len != rfi.len || rfi_db.sha1 != rfi.sha1 || rfi_db.modified != rfi.modified {
                let sql_mark_changed = "UPDATE remote_file_item SET len = :len, sha1 = :sha1, time_modified = :modified, changed = :changed where id = :id";
                if let Ok(mut stmt) = conn.prepare(sql_mark_changed) {
                    if let Err(err) = stmt.execute_named(named_params! {
                        ":len": rfi.len,
                        ":sha1": rfi.sha1,
                        ":modified": rfi.modified,
                        ":id": rfi_db.id,
                        ":changed": true,
                    }) {
                        error!("update remote file item failed: {:?}", err);
                    } else {
                        trace!("update changed item successfully.");
                    }
                } else {
                    error!("prepare find stmt failed: {:?}", sql_mark_changed);
                }
            } else {
                // make changed unchanged.
                if rfi_db.changed {
                    let sql_unmark_changed =
                        "UPDATE remote_file_item SET changed = :changed where id = :id";
                    if let Ok(mut stmt) = conn.prepare(sql_unmark_changed) {
                        if let Err(err) = stmt.execute_named(named_params! {
                            ":id": rfi_db.id,
                            ":changed": false,
                        }) {
                            error!("update remote file item failed: {:?}", err);
                        } else {
                            trace!("update changed item successfully.");
                        }
                    } else {
                        error!("prepare find stmt failed: {:?}", sql_unmark_changed);
                    }
                }
                trace!("unchanged file item.");
            }
        } else {
            match conn.execute(
            "INSERT INTO remote_file_item (path, sha1, len, time_modified, time_created, dir_id, changed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rfi.path, rfi.sha1, rfi.len, rfi.modified, rfi.created, rfi.dir_id, rfi.changed],
        ) {
            Ok(count) => {
                if count != 1 {
                    error!("insert remote file failed, execute return not eq to 1. {:?}", rfi);
                }
            },
            Err(e) => error!("insert remote file failed: {:?}, {:?}", rfi, e)
        }
        }
    }

    fn create_database(&self) -> Result<(), failure::Error> {
        let conn = self.0.get().unwrap();
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
                   changed         BOOLEAN,
                   dir_id          INTEGER NOT NULL,
                   FOREIGN KEY(dir_id) REFERENCES directory(id)
                   );
             CREATE UNIQUE INDEX dir_path ON remote_file_item (path, dir_id);
                COMMIT;",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::{load_remote_item_to_sqlite, Directory};
    use crate::develope::tutil;
    use crate::log_util;
    use std::fs;
    use std::io::{Read, Write};

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["db_accesses::sqlite_access"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_directory_to_db() -> Result<(), failure::Error> {
        log();
        let a_file = "abc_20130110120009.tar";
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20130101010155.tar", "abc")?;
        t_dir.make_a_file_with_content(a_file, "abc")?;
        t_dir.make_a_file_with_content("abc_20130117120009.tar", "abc")?;
        let dir = Directory {
            remote_dir: t_dir.tmp_dir_str().to_owned(),
            ..Default::default()
        };

        let db_dir = tutil::TestDir::new();
        let db_file = db_dir.tmp_dir_path().join("db.db");
        // let db_access = SqliteDbAccess::new_mem();
        let db_access = SqliteDbAccess::new(db_file);
        db_access.create_database()?;

        load_remote_item_to_sqlite(&dir, &db_access, false)?;

        assert_eq!(db_access.count_directory()?, 1);
        assert_eq!(db_access.count_remote_file_item(None)?, 3);

        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .open(t_dir.get_file_path(a_file))?;
            f.write_all(b"abc")?;
        }
        {
            let mut f = fs::OpenOptions::new()
                .read(true)
                .open(t_dir.get_file_path(a_file))?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            assert_eq!(buf.as_str(), "abc")
        }

        load_remote_item_to_sqlite(&dir, &db_access, false)?;

        assert_eq!(db_access.count_directory()?, 1);
        assert_eq!(db_access.count_remote_file_item(Some(true))?, 1);
        assert_eq!(db_access.count_remote_file_item(Some(false))?, 2);

        Ok(())
    }
}
