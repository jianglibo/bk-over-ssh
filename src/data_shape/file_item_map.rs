use super::{string_path, RelativeFileItem, PrimaryFileItem};
use crate::actions::hash_file_sha1;
use filetime;
use log::*;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug)]
pub enum SyncType {
    Sftp,
    Rsync,
}

#[derive(Debug)]
pub enum FileItemProcessResult {
    DeserializeFailed(String),
    Skipped(String),
    NoCorrespondedLocalDir(String),
    Directory(String),
    LengthNotMatch(String),
    Sha1NotMatch(String),
    CopyFailed(String),
    SkipBecauseNoBaseDir,
    Succeeded(u64, String, SyncType),
    GetLocalPathFailed,
    SftpOpenFailed,
    ScpOpenFailed,
    MayBeNoParentDir(PrimaryFileItem),
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileItemProcessResultStats {
    pub deserialize_failed: u64,
    pub skipped: u64,
    pub no_corresponded_from_dir: u64,
    pub directory: u64,
    pub length_not_match: u64,
    pub sha1_not_match: u64,
    pub copy_failed: u64,
    pub skip_because_no_base_dir: u64,
    pub succeeded: u64,
    pub get_local_path_failed: u64,
    pub sftp_open_failed: u64,
    pub scp_open_failed: u64,
    pub read_line_failed: u64,
    pub bytes_transferred: u64,
}

impl FileItemProcessResultStats {
    fn add_fields(&self, other: Self) -> Self {
        Self {
            deserialize_failed: self.deserialize_failed + other.deserialize_failed,
            skipped: self.skipped + other.skipped,
            no_corresponded_from_dir: self.no_corresponded_from_dir
                + other.no_corresponded_from_dir,
            directory: self.directory + other.directory,
            length_not_match: self.length_not_match + other.length_not_match,
            sha1_not_match: self.sha1_not_match + other.sha1_not_match,
            copy_failed: self.copy_failed + other.copy_failed,
            skip_because_no_base_dir: self.skip_because_no_base_dir
                + other.skip_because_no_base_dir,
            succeeded: self.succeeded + other.succeeded,
            get_local_path_failed: self.get_local_path_failed + other.get_local_path_failed,
            sftp_open_failed: self.sftp_open_failed + other.sftp_open_failed,
            scp_open_failed: self.scp_open_failed + other.scp_open_failed,
            read_line_failed: self.read_line_failed + other.read_line_failed,
            bytes_transferred: self.bytes_transferred + other.bytes_transferred,
        }
    }
}

impl Add for FileItemProcessResultStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.add_fields(other)
    }
}

impl AddAssign for FileItemProcessResultStats {
    fn add_assign(&mut self, other: Self) {
        *self = self.add_fields(other);
    }
}
/// FileItemMap hold a relative_file_item and local_base and remote_base.
/// So to calculate the local dir by relative_item's path.
/// it's been constructed by reading the stateful directory and fileitem lines.
#[derive(Debug)]
pub struct FileItemMap<'a> {
    pub relative_item: RelativeFileItem,
    local_base_dir: &'a Path,
    remote_base_dir: Option<&'a str>,
    pub sync_type: SyncType,
    pub download: bool,
}

impl<'a> FileItemMap<'a> {
    pub fn new(
        local_base_dir: &'a Path,
        remote_base_dir: &'a str,
        relative_item: RelativeFileItem,
        sync_type: SyncType,
        download: bool,
    ) -> Self {
        FileItemMap {
            local_base_dir,
            relative_item,
            remote_base_dir: Some(remote_base_dir),
            sync_type,
            download,
        }
    }

    pub fn had_changed(&self) -> bool {
        let lp = self.get_local_path();
        let ri = &self.relative_item;
        if !lp.exists() {
            return true;
        }
        if let Ok(mt) = lp.metadata() {
            if mt.len() != ri.get_len() {
                return true;
            }
            if mt
                .modified()
                .ok()
                .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                != ri.get_modified()
            {
                return true;
            }
            if ri.get_sha1().is_some() {
                let sha1 = hash_file_sha1(&lp);
                if ri.get_sha1() != sha1.as_ref().map(String::as_str) {
                    return true;
                }
            }
        } else {
            return true; // If cannot get the metadata think it as of changed.
        }
        false
    }

    pub fn is_sha1_not_equal(&self, local_sha1: impl AsRef<str>) -> bool {
        Some(local_sha1.as_ref().to_ascii_uppercase())
            != self.relative_item.get_sha1().map(str::to_ascii_uppercase)
    }

    pub fn get_relative_item(&self) -> &RelativeFileItem {
        &self.relative_item
    }
    pub fn get_local_path(&self) -> PathBuf {
        let rp = self.relative_item.get_path();
        self.local_base_dir.join(&rp)
    }

    /// If remote path is absolute then remote path will returned.
    pub fn get_local_path_str(&self) -> Option<String> {
        let rp = self.relative_item.get_path();
        self.local_base_dir.join(&rp).to_str().map(str::to_string)
    }

    pub fn get_remote_path_str(&self) -> String {
        if let Some(rbd) = self.remote_base_dir {
            string_path::join_path(rbd, self.relative_item.get_path())
        } else {
            self.relative_item.get_path().to_string()
        }
    }

    pub fn modified_secs(&self) -> Result<u64, failure::Error> {
        Ok(self
            .get_local_path()
            .metadata()?
            .modified()?
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs())
    }

    pub fn verify_modified_equal(&self) {
        if let Some(rmd) = self.relative_item.get_modified() {
            if let Ok(md) = self.modified_secs() {
                if rmd != md {
                    warn!("modified not equal, local: {:?}, remote: {:?}", md, rmd);
                    return;
                } else {
                    return;
                }
            } else {
                warn!(
                    "can't verify modified time. {:?}",
                    self.set_modified_as_remote()
                );
            }
        }
    }

    pub fn set_modified_as_remote(&self) -> Result<(), failure::Error> {
        if let Some(md) = self.relative_item.get_modified() {
            let ft = filetime::FileTime::from_unix_time(md as i64, 0);
            filetime::set_file_mtime(self.get_local_path(), ft)?;
        } else {
            bail!("remote_item has no modified value.");
        }
        Ok(())
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
