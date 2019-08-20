use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};
use walkdir::WalkDir;

pub struct RemoteFileItemLineOwned {
    path: String,
    sha1: Option<String>,
    len: u64,
    modified: Option<u64>,
    created: Option<u64>,
}

impl RemoteFileItemLineOwned {
    pub fn from_path(base_path: impl AsRef<Path>, path: PathBuf) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                if let Some(sha1) = hash_file_sha1(&path) {
                    let relative_o = path.strip_prefix(&base_path).ok().and_then(|p| p.to_str());
                    if let Some(relative) = relative_o {
                        return Some(Self {
                            path: relative.to_string(),
                            sha1: Some(sha1),
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
                }
            }
            Err(err) => {
                error!("RemoteFileItem from_path failed: {:?}, {:?}", path, err);
            }
        }
        None
    }
}

impl<'a> std::convert::From<&'a RemoteFileItemLineOwned> for RemoteFileItemLine<'a> {
    fn from(rfio: &'a RemoteFileItemLineOwned) -> Self {
        Self {
            path: rfio.path.as_str(),
            sha1: rfio.sha1.as_ref().map(|ss| ss.as_str()),
            len: rfio.len,
            modified: rfio.modified.clone(),
            created: rfio.created.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItemLine<'a> {
    path: &'a str,
    sha1: Option<&'a str>,
    len: u64,
    created: Option<u64>,
    modified: Option<u64>,
}

impl<'a> RemoteFileItemLine<'a> {
    #[allow(dead_code)]
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            sha1: None,
            len: 0_u64,
            created: None,
            modified: None,
        }
    }

    pub fn get_path(&self) -> &'a str {
        self.path
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1
    }
}

fn load_remote_item_owned<O: io::Write>(dir_path: impl AsRef<Path>, out: &mut O) {
    let bp = Path::new(dir_path.as_ref()).canonicalize();
    match bp {
        Ok(base_path) => {
            if let Some(path_str) = base_path.to_str() {
                if let Err(err) = write!(out, "{}\n", path_str) {
                    error!("{}", err);
                    return;
                }
            } else {
                error!("base_path to_str failed: {:?}", base_path);
                return;
            }
            WalkDir::new(&base_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_file())
                .filter_map(|d| d.path().canonicalize().ok())
                .filter_map(|d| RemoteFileItemLineOwned::from_path(&base_path, d))
                .for_each(|owned| {
                    let it = RemoteFileItemLine::from(&owned);
                    match serde_json::to_string(&it) {
                        Ok(line) => {
                            if let Err(err) = write!(out, "{}\n", line) {
                                error!("write item line failed: {:?}, {:?}", err, line);
                            }
                        }
                        Err(err) => {
                            error!("serialize item line failed: {:?}", err);
                        }
                    }
                });
        }
        Err(err) => {
            error!("load_dir resolve path failed: {:?}", err);
        }
    }
}

pub fn load_dirs<'a>(
    dirs: impl Iterator<Item = &'a str>,
    out: &'a str,
) -> Result<(), failure::Error> {
    let mut wf = fs::OpenOptions::new().create(true).write(true).open(out)?;

    for one_dir in dirs {
        load_remote_item_owned(one_dir, &mut wf);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use failure;

    #[test]
    fn t_from_path() -> Result<(), failure::Error> {
        log_util::setup_logger(vec![""], vec![]);
        let dirs = vec!["fixtures/adir"].into_iter();
        load_dirs(dirs, "fixtures/linux_remote_item_dir.txt")?;
        Ok(())
    }
}
