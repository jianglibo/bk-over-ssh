use crate::actions::hash_file_sha1;
use log::*;
use std::path::Path;
use std::time::SystemTime;
use super::{FullPathFileItemError};

pub struct FileMeta {
    pub sha1: Option<String>,
    pub len: u64,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

pub fn get_file_meta(file_path: impl AsRef<Path>, skip_sha1: bool) -> Result<FileMeta, failure::Error> {
    let lp = file_path.as_ref();
    if !lp.exists() {
        bail!(FullPathFileItemError::NotExist(lp.to_path_buf()));
    }
    match lp.metadata() {
        Ok(mt) => {
            let len = mt.len();
            let modified = mt
                .modified()
                .ok()
                .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let created = mt
                .created()
                .ok()
                .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let sha1 = if !skip_sha1 {
                hash_file_sha1(&lp)
            } else {
                None
            };
            Ok(FileMeta {
                len,
                sha1,
                modified,
                created,
            })
        }
        Err(err) => {
            error!("read meta failed: {}", err);
            bail!(FullPathFileItemError::Meta(err));
        }
    }
}
