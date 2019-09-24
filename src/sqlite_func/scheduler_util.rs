use cron::Schedule;
use log::*;
use rusqlite::{types::Null, Connection, Result as SqliteResult, NO_PARAMS};
use chrono::{DateTime, Datelike, Utc, SecondsFormat};
use std::str::FromStr;

// let expression = "0   30   9,12,15     1,15       May-Aug  Mon,Wed,Fri  2018/2";

/// let's say next execution time as NT.
/// if now() < NT, return false
/// if now() > NT, select db by NT:
///     if exists, return false.
///     if not exists: insert NT to db, and return true.
pub fn need_execute(conn: &Connection, expression: impl AsRef<str>) -> bool {
    let now = Utc::now();
    let expression = expression.as_ref();
    let schedule = Schedule::from_str(expression).unwrap();
    if let Some(dt) = schedule.upcoming(Utc).take(1).next() {
        if dt > now {
            true
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

}
