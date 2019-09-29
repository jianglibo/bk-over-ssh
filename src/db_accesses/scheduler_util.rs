use cron::Schedule;
use log::*;
use rusqlite::{types::Null, Connection, Result as SqliteResult, NO_PARAMS};
use chrono::{DateTime, Datelike, Utc, SecondsFormat};
use std::str::FromStr;
use r2d2;
use crate::db_accesses::DbAccess;

// let expression = "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2";

/// let's say next execution time as NT.
/// if now() < NT, return false
/// if now() > NT, select db by NT:
///     if exists, return false, this execution has done.
///     if not exists: insert NT to db, and return true.
pub fn need_execute<M, D>(db_access: D, server_yml_path: impl AsRef<str>, task_name: impl AsRef<str>, expression: impl AsRef<str>) -> bool where
    M: r2d2::ManageConnection,
    D: DbAccess<M> {
    let now = Utc::now();
    let expression = expression.as_ref();
    let schedule = Schedule::from_str(expression).unwrap();
    if let Some(dt) = schedule.upcoming(Utc).take(1).next() {
        if dt > now {
            let done: Option<bool> = db_access.find_next_execute(server_yml_path.as_ref(), task_name.as_ref());

            if let Some(_done) = done {
                false
            } else {
                db_access.insert_next_execute(server_yml_path.as_ref(), task_name.as_ref(), dt);
                true
            }
        } else {
            false
        }
    } else {
        error!("Can't retrive next execution time: {}", expression);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use crate::develope::tutil;

    pub fn create_database(conn: &Connection) -> Result<usize, failure::Error> {
        let r = conn.execute(
            "CREATE TABLE scheduler_done (
                  id              INTEGER PRIMARY KEY,
                  time_execution   TEXT
                  )",
            NO_PARAMS,
        )?;
        Ok(r)
    }

    #[test]
    fn t_dt_compare() {
        let past = Utc::now();
        thread::sleep(Duration::from_millis(3));
        let now = Utc::now();
        assert!(now > past);
    }

    #[test]
    fn t_need_execute() -> Result<(), failure::Error> {
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;

        let conn = Connection::open_in_memory()?;
        create_database(&conn)?;

        let now = Utc::now();
        let count = conn.execute(
            "INSERT INTO scheduler_done (time_execution)
                  VALUES (?1)",
            params![now],
        )?;
        assert_eq!(count, 1, "should insert one.");

        let mut stmt = conn
        .prepare("SELECT id, time_execution from scheduler_done")?;
        let r: DateTime<Utc> = stmt.query_row(NO_PARAMS, |row|{
            row.get(1)
        })?;
        assert_eq!(r, now);

        let mut stmt = conn
        .prepare("SELECT count(*) from scheduler_done where time_execution = ?1")?;

        let r: u8 = stmt.query_row(params![now], |row|{
            row.get(0)
        })?;
        assert_eq!(r, 1);

        let c = conn.execute("DELETE FROM scheduler_done", NO_PARAMS)?;

        assert_eq!(c, 1);

        // stmt.query_and_then(NO_PARAMS, |row|
        //     row.get(1)
        // )?.for_each(|r| {
        //     if let Ok(i) = r {
        //         let i: DateTime<Utc> = i;
        //         let b = now == i;
        //         assert!(b);
        //     } else {
        //         panic!("nnn.");
        //     }
        // });

        Ok(())
    }

    #[test]
    fn t_n_sort() {
        let mut vs = vec!["data_20190826.tar.gz", "data_20190923.tar.gz", "data_20190901.tar.gz"];
        vs.sort();
        assert_eq!(Some("data_20190826.tar.gz").as_ref(), vs.first());
        assert_eq!(Some("data_20190923.tar.gz").as_ref(), vs.last());

        let mut vs = vec!["data_20190826.tar.gz", "data_20190923.tar.gz", "data_20190901.tar.gz"];
        vs.reverse(); // reverse just reverse, don't sort.
        assert_eq!(Some("data_20190901.tar.gz").as_ref(), vs.first());
        assert_eq!(Some("data_20190826.tar.gz").as_ref(), vs.last());
    }

}
