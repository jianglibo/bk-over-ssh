use crate::actions::hash_file_sha1;
use log::*;
use std::path::Path;
use std::time::SystemTime;

pub struct FileMeta {
    pub sha1: Option<String>,
    pub len: u64,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

pub fn get_file_meta(file_path: impl AsRef<Path>, skip_sha1: bool) -> Option<FileMeta> {
    let lp = file_path.as_ref();
    if !lp.exists() {
        return None;
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
            Some(FileMeta {
                len,
                sha1,
                modified,
                created,
            })
        }
        Err(err) => {
            error!("read meta failed: {}", err);
            None
        }
    }
}
