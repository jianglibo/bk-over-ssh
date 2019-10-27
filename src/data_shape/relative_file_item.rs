use super::Directory;
use crate::actions::hash_file_sha1;
use crate::data_shape::SlashPath;
use crate::db_accesses::{DbAccess, RelativeFileItemInDb};
use itertools::Itertools;
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use std::io;
use std::iter::Iterator;
use std::path::{PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Deserialize, Serialize)]
pub struct RelativeFileItem {
    path: String,
    sha1: Option<String>,
    len: u64,
    modified: Option<u64>,
    created: Option<u64>,
    changed: bool,
    confirmed: bool,
}

impl RelativeFileItem {
    pub fn from_path(base_path: &SlashPath, path: PathBuf, skip_sha1: bool) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 {
                    hash_file_sha1(&path)
                } else {
                    Option::<String>::None
                };

                return Some(Self {
                    path: base_path.strip_prefix(path.as_path()),
                    sha1,
                    len: metadata.len(),
                    modified: metadata
                        .modified()
                        .ok()
                        .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    created: metadata
                        .created()
                        .ok()
                        .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    changed: false,
                    confirmed: false,
                });
            }
            Err(err) => {
                error!("RelativeFileItem from_path failed: {:?}, {:?}", path, err);
            }
        }
        None
    }
}

impl std::convert::From<RelativeFileItemInDb> for RelativeFileItem {
    fn from(rfidb: RelativeFileItemInDb) -> Self {
        RelativeFileItem {
            path: rfidb.path,
            sha1: rfidb.sha1,
            len: rfidb.len as u64,
            modified: rfidb.modified.map(|dt| dt.timestamp() as u64),
            created: rfidb.created.map(|dt| dt.timestamp() as u64),
            changed: rfidb.changed,
            confirmed: rfidb.confirmed,
        }
    }
}

impl RelativeFileItem {
    #[allow(dead_code)]
    pub fn new(relative_path: &str, len: u64) -> Self {
        Self {
            path: relative_path.to_owned(),
            sha1: None,
            len,
            created: None,
            modified: None,
            changed: false,
            confirmed: false,
        }
    }

    pub fn get_modified(&self) -> Option<u64> {
        self.modified
    }

    pub fn get_path(&self) -> &str {
        self.path.as_str()
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1.as_ref().map(|s| s.as_str())
    }
}

/// this function will walk over the directory, for every file checking it's metadata and compare to corepsonding item in the db.
/// for new and changed items mark changed field to true.
/// for unchanged items, if the status in db is changed chang to unchanged.
/// So after invoking this method all changed item will be marked, at the same time, metadata of items were updated too, this means you cannot regenerate the same result if the task is interupted.
/// To avoid this kind of situation, add a confirm field to the table. when the taks is done, we chang the confirm field to true.
/// Now we get the previous result by select the unconfirmed items.
pub fn load_remote_item_to_sqlite<M, D>(
    directory: &Directory,
    db_access: &D,
    skip_sha1: bool,
    sql_batch_size: usize,
    sig_ext: &str,
    delta_ext: &str,
) -> Result<(), failure::Error>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    trace!(
        "load_remote_item_to_sqlite, skip_sha1: {}, sql_batch_size: {}",
        skip_sha1,
        sql_batch_size
    );
    // let base_path = directory.get_remote_canonicalized_dir_str()?;
    let base_path = directory.remote_dir.as_str();
    // let dir_id = db_access.insert_directory(base_path.as_str())?;
    let dir_id = db_access.insert_directory(base_path)?;

    if sql_batch_size > 1 {
        // WalkDir::new(&base_path)
        WalkDir::new(&directory.remote_dir.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| d.path().canonicalize().ok())
            .filter_map(|d| directory.match_path(d))
            // .filter_map(|d| RelativeFileItemInDb::from_path(&base_path, d, skip_sha1, dir_id))
            .filter_map(|d| {
                RelativeFileItemInDb::from_path(&directory.remote_dir, d, skip_sha1, dir_id)
            })
            .filter(|rfi| !(rfi.path.ends_with(sig_ext) || rfi.path.ends_with(delta_ext)))
            .filter_map(|rfi| db_access.insert_or_update_relative_file_item(rfi, true))
            .map(|(rfi, da)| rfi.to_sql_string(&da))
            .chunks(sql_batch_size)
            .into_iter()
            .for_each(|ck| {
                trace!("start batch insert.");
                db_access.execute_batch(ck);
                trace!("end batch insert.");
            });
    } else {
        let _c = WalkDir::new(&base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| d.path().canonicalize().ok())
            .filter_map(|d| directory.match_path(d))
            .filter_map(|d| {
                RelativeFileItemInDb::from_path(&directory.remote_dir, d, skip_sha1, dir_id)
            })
            // .filter_map(|d| RelativeFileItemInDb::from_path(&base_path, d, skip_sha1, dir_id))
            .filter_map(|rfi| db_access.insert_or_update_relative_file_item(rfi, false))
            .count();
    }
    Ok(())
}

pub fn load_remote_item<O>(
    directory: &Directory,
    out: &mut O,
    skip_sha1: bool,
) -> Result<(), failure::Error>
where
    O: io::Write,
{
    trace!("load_remote_item, skip_sha1: {}", skip_sha1);
    // let base_path = directory.get_remote_canonicalized_dir_str()?;
    // writeln!(out, "{}", base_path)?;
    // WalkDir::new(&base_path)
    WalkDir::new(directory.remote_dir.as_path())
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|d| d.file_type().is_file())
        .filter_map(|d| d.path().canonicalize().ok())
        .filter_map(|d| directory.match_path(d))
        .filter_map(|d| RelativeFileItem::from_path(&directory.remote_dir, d, skip_sha1))
        .for_each(|rfi| match serde_json::to_string(&rfi) {
            Ok(line) => {
                if let Err(err) = writeln!(out, "{}", line) {
                    error!("write item line failed: {:?}, {:?}", err, line);
                }
            }
            Err(err) => {
                error!("serialize item line failed: {:?}", err);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use failure;

    #[test]
    fn t_deserialize_item() -> Result<(), failure::Error> {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::relative_file_item"],
            Some(vec!["ssh2"]),
            "",
        )?;
        let item = RelativeFileItem {
            path: "b b\\b b.txt".to_string(),
            sha1: None,
            len: 5,
            modified: Some(554),
            created: Some(666),
            changed: false,
            confirmed: false,
        };

        // let item: RelativeFileItem = RelativeFileItem::from(&item_owned);

        info!("item: {:?}", item);

        let s = serde_json::to_string(&item)?;
        info!("item str: {}", s);

        // let item: RelativeFileItem = serde_json::from_str(&s)?;
        // assert_eq!(item.get_len(), 5);

        let s = r##"{"path":"qrcode.png","sha1":null,"len":6044,"created":1567834936,"modified":1567834936}"##;
        let fi: RelativeFileItem = serde_json::from_str(s)?;
        assert_eq!(fi.get_len(), 6044);

        let s = r##"{"path":"b b\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RelativeFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b/b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi = serde_json::from_str::<RelativeFileItem>(s)?;
        assert_eq!(fi.get_len(), 5);

        Ok(())
    }
}
