use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};
use walkdir::WalkDir;
use super::server::Directory;

pub struct RemoteFileItemLineOwned {
    path: String,
    sha1: Option<String>,
    len: u64,
    modified: Option<u64>,
    created: Option<u64>,
}

impl RemoteFileItemLineOwned {
    pub fn from_path(base_path: impl AsRef<Path>, path: PathBuf, skip_sha1: bool) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 { hash_file_sha1(&path) } else { Option::<String>::None };
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

pub fn load_remote_item_owned<O: io::Write>(directory: &Directory, out: &mut O, skip_sha1: bool) -> Result<(), failure::Error> {
    let bp = Path::new(directory.remote_dir.as_str()).canonicalize();
    match bp {
        Ok(base_path) => {
            if let Some(path_str) = base_path.to_str() {
                writeln!(out, "{}", path_str)?;
            } else {
                bail!("base_path to_str failed: {:?}", base_path);
            }
            WalkDir::new(&base_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_file())
                .filter_map(|d| d.path().canonicalize().ok())
                .filter_map(|d| {
                    directory.match_path(d)
                })
                .filter_map(|d| RemoteFileItemLineOwned::from_path(&base_path, d, skip_sha1))
                .for_each(|owned| {
                    let it = RemoteFileItemLine::from(&owned);
                    match serde_json::to_string(&it) {
                        Ok(line) => {
                            if let Err(err) = writeln!(out, "{}", line) {
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
    Ok(())
}

pub fn load_dirs<'a, O: io::Write>(
    dirs: impl Iterator<Item = &'a str>,
    out: &'a mut O,
    skip_sha1: bool,
) -> Result<(), failure::Error> {
    for one_dir in dirs {
        let directory = Directory {
            remote_dir: one_dir.to_string(),
            ..Directory::default()
        };
        load_remote_item_owned(&directory, out, skip_sha1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use crate::develope::develope_data;
    use failure;

    const L_DIR: &str = "fixtures/linux_remote_item_dir.txt";

    #[test]
    fn t_from_path() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let dirs = vec!["fixtures/adir"].into_iter();
        let mut cur = develope_data::get_a_cursor_writer();
        load_dirs(dirs, &mut cur, true)?;
        let num = develope_data::count_cursor_lines(cur);
        assert_eq!(num, 6);
        Ok(())
    }

    #[test]
    fn t_from_path_to_path() -> Result<(), failure::Error> {
        log_util::setup_logger_empty();
        let dirs = vec!["F:/"].into_iter();
        let mut out = fs::OpenOptions::new().create(true).truncate(true).write(true).open(L_DIR)?;
        load_dirs(dirs, &mut out, true)?;
        Ok(())
    }
}
