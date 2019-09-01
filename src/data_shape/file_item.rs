use super::string_path;
use super::RemoteFileItem;
use crate::actions::hash_file_sha1;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub enum FileItemProcessResult {
    DeserializeFailed(String),
    Skipped(String),
    NoCorresponedLocalDir(String),
    Directory(String),
    LengthNotMatch(String),
    Sha1NotMatch(String),
    CopyFailed(String),
    SkipBecauseNoBaseDir,
    Successed,
    GetLocalPathFailed,
    SftpOpenFailed,
    ReadLineFailed,
}


#[derive(Debug, Default)]
pub struct FileItemProcessResultStats {
    pub deserialize_failed: u64,
    pub skipped: u64,
    pub no_corresponed_local_dir: u64,
    pub directory: u64,
    pub length_not_match: u64,
    pub sha1_not_match: u64,
    pub copy_failed: u64,
    pub skip_because_no_base_dir: u64,
    pub successed: u64,
    pub get_local_path_failed: u64,
    pub sftp_open_failed: u64,
    pub read_line_failed: u64,
}

#[derive(Debug)]
pub struct FileItem<'a> {
    pub remote_item: &'a RemoteFileItem<'a>,
    base_dir: &'a Path,
    remote_base_dir: Option<&'a str>,
}

impl<'a> FileItem<'a> {
    #[allow(dead_code)]
    pub fn standalone(
        file_path: &'a Path,
        remote_base_dir: Option<&'a str>,
        remote_item: &'a RemoteFileItem,
    ) -> Self {
        Self {
            remote_item,
            base_dir: file_path,
            remote_base_dir,
        }
    }

    pub fn new(
        base_dir: &'a Path,
        remote_base_dir: &'a str,
        remote_item: &'a RemoteFileItem,
    ) -> Self {
        Self {
            base_dir,
            remote_item,
            remote_base_dir: Some(remote_base_dir),
        }
    }

    pub fn had_changed(&self) -> bool {
        let lp = self.get_local_path();
        let ri = self.remote_item;
        if !lp.exists() {
            return true;
        }
        if let Ok(mt) = lp.metadata() {
            if mt.len() != ri.get_len() {
                return true;
            }
            // if mt
            //     .modified()
            //     .ok()
            //     .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
            //     .map(|d| d.as_secs())
            //     != ri.get_modified()
            // {
            //     return true;
            // }
            if ri.get_sha1().is_some() {
                let sha1 = hash_file_sha1(&lp);
                if ri.get_sha1() != sha1.as_ref().map(String::as_str) {
                    return true;
                }
            }
        } else {
            return true; // If cannnot get the metadata think it as of changed.
        }
        false
    }

    pub fn is_sha1_not_equal(&self, local_sha1: impl AsRef<str>) -> bool {
        Some(local_sha1.as_ref().to_ascii_uppercase()) != self.remote_item.get_sha1().map(str::to_ascii_uppercase)
    }

    pub fn get_remote_item(&self) -> &RemoteFileItem {
        self.remote_item
    }
    pub fn get_local_path(&self) -> PathBuf {
        let rp = self.remote_item.get_path();
        self.base_dir.join(&rp)
    }

    pub fn get_local_path_str(&self) -> Option<String> {
        let rp = self.remote_item.get_path();
        self.base_dir.join(&rp).to_str().map(str::to_string)
    }

    pub fn get_remote_path(&self) -> String {
        if let Some(rbd) = self.remote_base_dir {
            string_path::join_path(rbd, self.remote_item.get_path())
        } else {
            self.remote_item.get_path().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_join_path() {
        let p1 = Path::new("not_in_git");
        let p2 = p1.join("鮮やか");
        assert_eq!(p2, Path::new("not_in_git/鮮やか"));
    }

    #[test]
    fn t_concat_str() {
        let a = "a";
        let b = "b";
        let c = [a, b].concat();
        assert_eq!(c, String::from("ab"));
        assert_eq!(a, "a");

        let _aa = String::from("a");
        let _bb = String::from("b");

        // let cc = [&aa, bb].concat();
        // assert_eq!(cc, String::from("ab"));
        // assert_eq!(aa, String::from("a"));
    }
}
