use crate::db_accesses::DbAccess;
use chrono::{DateTime, Local};
use cron::Schedule;
use r2d2;
use std::str::FromStr;
use log::*;

fn upcomming(expression: impl AsRef<str>) -> DateTime<Local> {
    let schedule = Schedule::from_str(expression.as_ref()).unwrap();
    schedule
        .upcoming(Local)
        .take(1)
        .next()
        .expect("Can't retrive next execution time")
}

// let expression = "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2";
///

/// let's say next execution time as NT.
///
/// Below is linux crontab format:
/// min hour day month dayOfWeek Command
/// min : Minute (0-59)
/// hour : Hour (0-23)
/// day : Day of month (1-31)  
/// month : Month (1-12)
/// dayOfWeek : Day of week (0 - 7) [Sunday = 0 or 7]
/// Command: command to run as cron job.

#[allow(dead_code)]
#[allow(clippy::let_and_return)]
pub fn need_execute<M, D>(
    db_access: Option<&D>,
    server_yml_path: impl AsRef<str>,
    task_name: impl AsRef<str>,
    expression: impl AsRef<str>,
) -> (bool, Option<DateTime<Local>>)
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    if db_access.is_none() {
        return (true, None);
    }

    let db_access = db_access.unwrap();
    let expression = expression.as_ref();
    let server_yml_path = server_yml_path.as_ref();
    let task_name = task_name.as_ref();
    let next_execute_in_db = db_access.find_next_execute(server_yml_path, task_name);
    let now = Local::now();
    trace!("now: {:?}, find next_execute_in_db: {:?}", now, next_execute_in_db);
    let b = match next_execute_in_db {
        Some((row_id, next_execute_in_db, true)) => {
            // find in db and already done.
            let next_execute = upcomming(expression);
            if next_execute > next_execute_in_db {
                // delete old one.
                db_access.delete_next_execute(row_id).expect("delete_next_execute should success.");
                //insert next_execute to db.
                db_access.insert_next_execute(server_yml_path, task_name, next_execute);
            }
            (false, Some(next_execute))
        }
        Some((row_id, next_execute_in_db, false)) => {
            // prevent multiple invoking.
            if now > next_execute_in_db {
                // update done status.
                db_access
                    .update_next_execute_done(row_id)
                    .expect("update_next_execute_done should success.");
                (true, None)
            } else {
                trace!("time isn't up yet. do nothing.");
                (false, Some(next_execute_in_db))
            }
        }
        None => {
            // insert next_execute to db.
            let next_execute = upcomming(expression);
            db_access.insert_next_execute(server_yml_path, task_name, next_execute);
            (false, Some(next_execute))
        }
    };
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use chrono::{self, offset::TimeZone, Datelike, Timelike, Utc};
    use std::thread;
    use std::time::Duration;
    use rusqlite::{params, NO_PARAMS};

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
        let adt = Utc.ymd(2014, 7, 8).and_hms(9, 10, 11);
        eprintln!("adt: {:?}", adt.minute());
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        // min hour day month dayOfWeek Command
        // let expression = "0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2"; parser's spec.
        let now = Utc::now();
        let dt = now + chrono::Duration::seconds(2);
        let expression = format!(
            "{}/6 {} {} {} {} {} {}",
            dt.second(),
            dt.minute(),
            dt.hour(),
            dt.day(),
            "*",
            "*",
            "*"
        );
        eprintln!("expression: {}", expression);
        assert!(
            !need_execute(Some(&db_access), "a.yml", "d", &expression).0,
            "not happened"
        ); // because time is not up. no need.
        thread::sleep(Duration::from_secs(3));
        assert!(
            need_execute(Some(&db_access), "a.yml", "d", &expression).0,
            "happen"
        ); // time is up, needed.
        assert_eq!(db_access.count_next_execute()?, 1, "done set to true."); // done and deleted.
        assert!(
            !need_execute(Some(&db_access), "a.yml", "d", &expression).0,
            "happened"
        ); // insert next_execute and delete previous next_execute.

        assert_eq!(db_access.count_next_execute()?, 1); // already insert new one.
        let ne = db_access.find_next_execute("a.yml", "d").unwrap();
        assert!(!ne.2, "not executed yet.");

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

        let mut stmt = conn.prepare("SELECT id, time_execution from schedule_done")?;
        let r: DateTime<Utc> = stmt.query_row(NO_PARAMS, |row| row.get(1))?;
        assert_eq!(r, now);

        let mut stmt =
            conn.prepare("SELECT count(*) from schedule_done where time_execution = ?1")?;

        let r: u8 = stmt.query_row(params![now], |row| row.get(0))?;
        assert_eq!(r, 1);

        let c = conn.execute("DELETE FROM schedule_done", NO_PARAMS)?;

        assert_eq!(c, 1);
        Ok(())
    }

    #[test]
    fn t_n_sort() {
        let mut vs = vec![
            "data_20190826.tar.gz",
            "data_20190923.tar.gz",
            "data_20190901.tar.gz",
        ];
        vs.sort();
        assert_eq!(Some("data_20190826.tar.gz").as_ref(), vs.first());
        assert_eq!(Some("data_20190923.tar.gz").as_ref(), vs.last());

        let mut vs = vec![
            "data_20190826.tar.gz",
            "data_20190923.tar.gz",
            "data_20190901.tar.gz",
        ];
        vs.reverse(); // reverse just reverse, don't sort.
        assert_eq!(Some("data_20190901.tar.gz").as_ref(), vs.first());
        assert_eq!(Some("data_20190826.tar.gz").as_ref(), vs.last());
    }
}
