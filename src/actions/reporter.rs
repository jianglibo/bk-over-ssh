use crate::data_shape::{FileItemProcessResultStats, Server};
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::{fs, io::Write};
use crate::db_accesses::{DbAccess};
// fn ser_instant<S>(inst: &Instant, serer: S) -> Result<S::Ok, S::Error> where S: Serializer {
//     let s = format!("{}", date.format(FORMAT));
//     serializer.serialize_str(&s)
// }

// mod serde_instant {
//     use std::time::{Instant, SystemTime};
//     use serde::{self, Deserialize, Serializer, Deserializer};

//     const FORMAT: &'static str = "%Y-%m-%d %H:%M:%S";

//     pub fn serialize<S>(
//         date: &Instant,
//         serializer: S,
//     ) -> Result<S::Ok, S::Error>
//     where
//         S: Serializer,
//     {
//         let sec = date.duration_since(SystemTime::UNIX_EPOCH).as_secs();
//         serializer.serialize_u64(sec)
//     }

//     pub fn deserialize<'de, D>(
//         deserializer: D,
//     ) -> Result<Instant, D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         let s = String::deserialize(deserializer)?;
//         Utc.datetime_from_str(&s, FORMAT).map_err(serde::de::Error::custom)
//     }
// }

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncDirReport {
    duration: Duration,
    statistics: FileItemProcessResultStats,
}

impl SyncDirReport {
    pub fn new(duration: Duration, statistics: FileItemProcessResultStats) -> Self {
        Self {
            duration,
            statistics,
        }
    }
}

pub fn write_dir_sync_result<M, D>(server: &Server<M, D>, result: &SyncDirReport)
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let rp = &server.get_dir_sync_report_file();
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(rp)
    {
        Err(err) => error!("open report file failed: {:?}, {:?}", rp, err),
        Ok(mut out) => match serde_json::to_string(&result) {
            Err(err) => {
                error!("serialize reporter failed: {:?}", err);
            }
            Ok(s) => {
                if let Err(err) = writeln!(out, "{}", s) {
                    error!("write report failed: {:?}", err);
                }
            }
        },
    }
}
