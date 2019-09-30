use cron::Schedule;
use log::*;
use rusqlite::{types::Null, Connection, Result as SqliteResult, NO_PARAMS};
use chrono::{DateTime, Datelike, Utc, SecondsFormat};
use std::str::FromStr;
use r2d2;
use crate::db_accesses::DbAccess;

// let expression = "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2";
/// 

/// let's say next execution time as NT.
/// if now() < NT, return false
/// if now() > NT, select db by NT:
///     if exists, return false, this execution has done.
///     if not exists: insert NT to db, and return true.
/// Below is linux crontab format:
/// min hour day month dayOfWeek Command
/// min : Minute (0-59)
/// hour : Hour (0-23)
/// day : Day of month (1-31)  
/// month : Month (1-12)
/// dayOfWeek : Day of week (0 - 7) [Sunday = 0 or 7]
/// Command: command to run as cron job.

pub fn need_execute<M, D>(db_access: D, server_yml_path: impl AsRef<str>, task_name: impl AsRef<str>, expression: impl AsRef<str>) -> bool where
    M: r2d2::ManageConnection,
    D: DbAccess<M> {
    let now = Utc::now();
    let expression = expression.as_ref();
    let schedule = Schedule::from_str(expression).unwrap();
    if let Some(dt) = schedule.upcoming(Utc).take(1).next() { // because last upcoming event had gone!!!
        eprintln!("next time: {:?}", dt);
        // must now greater than dt.
        if now > dt {
            let done_exists: Option<bool> = db_access.find_next_execute(server_yml_path.as_ref(), task_name.as_ref());
            if done_exists.is_some() {
                eprintln!("alreay exists in db.");
                false
            } else {
                eprintln!("insert item.");
                db_access.insert_next_execute(server_yml_path.as_ref(), task_name.as_ref(), dt);
                true
            }
        } else {
            eprintln!("time is no up.");
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
    use chrono::{self, Utc, Datelike, Timelike};
    use crate::log_util;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["db_accesses::scheduler_util"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_dt_compare() {
        let past = Utc::now();
        thread::sleep(Duration::from_millis(3));
        let now = Utc::now();
        assert!(now > past);
    }

    #[test]
    fn t_test_one_task() -> Result<(), failure::Error> {
        log();
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        // min hour day month dayOfWeek Command
        // let expression = "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2"; parser's spec.
        let dt = Utc::now() + chrono::Duration::seconds(3);
        eprintln!("dt after 3 seconds: {:?}", dt);
        let expression = format!("{} {} {} {} {} {} {}",dt.second(), dt.minute() + 1, dt.hour(), dt.day(), "*", "*", "*");
        eprintln!("expression: {}", expression);
        assert!(!need_execute(db_access.clone(), "a.yml", "d", &expression), "not happened"); // because time is not up.
        thread::sleep(Duration::from_secs(5));
        assert!(need_execute(db_access.clone(), "a.yml", "d", &expression), "happen"); // time is up, but 
        assert!(!need_execute(db_access.clone(), "a.yml", "d", &expression), "happened");

        Ok(())

    }

    #[test]
    fn t_need_execute() -> Result<(), failure::Error> {
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        let conn = db_access.get_pool().get().unwrap();

        let now = Utc::now();
        let count = conn.execute(
            "INSERT INTO schedule_done (server_yml_path, task_name, time_execution)
                  VALUES (?1, ?2, ?3)",
            params!["ss.yml", "sync-dir", now],
        )?;
        assert_eq!(count, 1, "should insert one.");

        let mut stmt = conn
        .prepare("SELECT id, time_execution from schedule_done")?;
        let r: DateTime<Utc> = stmt.query_row(NO_PARAMS, |row|{
            row.get(1)
        })?;
        assert_eq!(r, now);

        let mut stmt = conn
        .prepare("SELECT count(*) from schedule_done where time_execution = ?1")?;

        let r: u8 = stmt.query_row(params![now], |row|{
            row.get(0)
        })?;
        assert_eq!(r, 1);

        let c = conn.execute("DELETE FROM schedule_done", NO_PARAMS)?;

        assert_eq!(c, 1);
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
