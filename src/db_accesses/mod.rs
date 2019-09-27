// placeholders
pub mod sqlite_access;
mod scheduler_util;

use r2d2;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use crate::actions::hash_file_sha1;
use log::*;

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

pub trait DbAccess<M>: Clone where M: r2d2::ManageConnection, {
    fn insert_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error>;
    fn insert_remote_file_item(&self, rfi: RemoteFileItemInDb);
    fn create_database(&self) -> Result<(), failure::Error>;
}

