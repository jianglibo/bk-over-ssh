use super::server::Directory;
use crate::actions::hash_file_sha1;
use crate::db_accesses::{RemoteFileItemInDb, DbAccess};
use log::*;
use serde::{Deserialize, Serialize};
use std::io;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;
use r2d2;

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItem {
    path: String,
    sha1: Option<String>,
    len: u64,
    modified: Option<u64>,
    created: Option<u64>,
}

impl RemoteFileItem {
    pub fn from_path(base_path: impl AsRef<Path>, path: PathBuf, skip_sha1: bool) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 {
                    hash_file_sha1(&path)
                } else {
                    Option::<String>::None
                };
                // if let Some(sha1) = hash_file_sha1(&path) {
                let relative_o = path.strip_prefix(&base_path).ok().and_then(|p| p.to_str());
                if let Some(relative) = relative_o {
                    return Some(Self {
                        path: relative.to_string(),
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

impl RemoteFileItem {
    #[allow(dead_code)]
    pub fn new(relative_path: &str, len: u64) -> Self {
        Self {
            path: relative_path.to_owned(),
            sha1: None,
            len,
            created: None,
            modified: None,
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

pub fn load_remote_item_to_sqlite<M, D>(
    directory: &Directory,
    db_access: &D,
    skip_sha1: bool,
) -> Result<(), failure::Error> where
    M: r2d2::ManageConnection, 
    D: DbAccess<M>, {
    if let Some(base_path) = directory.get_remote_canonicalized_dir_str() {
        let dir_id = db_access.insert_directory(base_path.as_str())?;
        WalkDir::new(&base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| d.path().canonicalize().ok())
            .filter_map(|d| directory.match_path(d))
            .filter_map(|d| RemoteFileItemInDb::from_path(&base_path, d, skip_sha1, dir_id))
            .for_each(|rfi| {
                db_access.insert_or_update_remote_file_item(rfi);
            });
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
    if let Some(base_path) = directory.get_remote_canonicalized_dir_str() {
        writeln!(out, "{}", base_path)?;
        WalkDir::new(&base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| d.path().canonicalize().ok())
            .filter_map(|d| directory.match_path(d))
            .filter_map(|d| RemoteFileItem::from_path(&base_path, d, skip_sha1))
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
    }
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
            vec!["data_shape::remote_file_item"],
            Some(vec!["ssh2"]),
            ""
        )?;
        let item = RemoteFileItem {
            path: "b b\\b b.txt".to_string(),
            sha1: None,
            len: 5,
            modified: Some(554),
            created: Some(666),
        };

        // let item: RemoteFileItem = RemoteFileItem::from(&item_owned);

        info!("item: {:?}", item);

        let s = serde_json::to_string(&item)?;
        info!("item str: {}", s);

        // let item: RemoteFileItem = serde_json::from_str(&s)?;
        // assert_eq!(item.get_len(), 5);

        let s = r##"{"path":"qrcode.png","sha1":null,"len":6044,"created":1567834936,"modified":1567834936}"##;
        let fi: RemoteFileItem = serde_json::from_str(s)?;
        assert_eq!(fi.get_len(), 6044);

        let s = r##"{"path":"b b\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b\\\\\\\\b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi_r = serde_json::from_str::<RemoteFileItem>(s);
        assert!(fi_r.is_ok());

        let s = r##"{"path":"b b/b b.txt","sha1":null,"len":5,"created":1565607566,"modified":1565607566}"##;
        let fi = serde_json::from_str::<RemoteFileItem>(s)?;
        assert_eq!(fi.get_len(), 5);

        Ok(())
    }
}
