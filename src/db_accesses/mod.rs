// placeholders
mod scheduler_util;
pub mod sqlite_access;

use crate::actions::hash_file_sha1;
use chrono::{DateTime, Utc};
use log::*;
use r2d2;
use std::path::{Path, PathBuf};

pub use sqlite_access::SqliteDbAccess;

#[derive(Debug, Default)]
pub struct RemoteFileItemInDb {
    pub id: i64,
    pub path: String,
    pub sha1: Option<String>,
    pub len: i64,
    pub modified: Option<DateTime<Utc>>,
    pub created: Option<DateTime<Utc>>,
    pub dir_id: i64,
    pub changed: bool,
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
                        changed: false,
                    });
                } else {
                    error!("RemoteFileItem path name to_str() failed. {:?}", path);
                }
            }
            Err(err) => {
                error!("RemoteFileItem metadata failed: {:?}, {:?}", path, err);
            }
        }
        None
    }
}

pub trait DbAccess<M>: Send + Sync + Clone
where
    M: r2d2::ManageConnection,
{
    fn insert_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error>;
    fn insert_or_update_remote_file_items(&self, rfis: Vec<RemoteFileItemInDb>);
    fn insert_or_update_remote_file_item(&self, rfi: RemoteFileItemInDb);
    fn one_file_item(&self) -> Result<RemoteFileItemInDb, failure::Error>;
    fn find_remote_file_item(
        &self,
        dir_id: i64,
        path: impl AsRef<str>,
    ) -> Result<RemoteFileItemInDb, failure::Error>;
    fn create_database(&self) -> Result<(), failure::Error>;
    fn find_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error>;
    fn count_directory(&self) -> Result<u64, failure::Error>;
    fn count_remote_file_item(&self, changed: Option<bool>) -> Result<u64, failure::Error>;
    fn iterate_all_file_items<P>(&self, processor: P) -> (usize, usize)
    where
        P: Fn(RemoteFileItemInDb) -> ();
    fn iterate_files_by_directory<F>(&self, processor: F) -> Result<(), failure::Error>
    where
        F: FnMut((Option<RemoteFileItemInDb>, Option<String>)) -> ();
    
    fn find_next_execute(&self, server_yml_path: impl AsRef<str>, task_name: impl AsRef<str>) -> Option<bool>;
    fn insert_next_execute(&self, server_yml_path: impl AsRef<str>, task_name: impl AsRef<str>, time_execution: DateTime<Utc>);
}
