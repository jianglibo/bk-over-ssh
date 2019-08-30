use super::{RemoteFileItem, RemoteFileItemDir, RemoteFileItemDirOwned};
use crate::actions::{copy_a_file_item};
use log::*;
use ssh2;
use std::iter::Iterator;
use std::path::{Path};
use std::io::prelude::Read;
use std::{io, fs};

#[derive(Debug)]
pub struct FileItemDir<'a> {
    pub base_dir: &'a Path,
    remote_dir: RemoteFileItemDir<'a>,
}

pub fn download_dirs<'a>(session: &mut ssh2::Session, json_file: impl AsRef<str> + 'a, out: impl AsRef<str> + 'a) -> Result<(), failure::Error> {
    let rf = fs::OpenOptions::new()
        .open(json_file.as_ref())?;
    let mut reader = io::BufReader::new(rf);
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    let rds: Vec<RemoteFileItemDir<'_>> = serde_json::from_str(content.as_str())?;
    for rd in rds {
        let fid = FileItemDir::new(Path::new(out.as_ref()), rd);
        fid.download_files(session);
    }
    Ok(())
}

impl<'a> FileItemDir<'a> {
    pub fn new(base_dir: &'a Path, mut remote_dir: RemoteFileItemDir<'a>) -> Self {
        remote_dir.fill_base_dir();
        Self {
            base_dir,
            remote_dir,
        }
    }

    pub fn download_files(&self, session: &mut ssh2::Session) -> (u64, u64) {
        self.remote_dir
            .get_items()
            .iter()
            .map(|ri| FileItem::new(self.base_dir, ri))
            .map(|fi| copy_a_file_item(session, fi))
            .fold((0_u64, 0_u64), |(mut successed, mut failed), fi| {
                if fi.fail_reason.is_some() {
                    error!("copy_a_file_item failed: {:?}", fi);
                    failed += 1;
                } else {
                    successed += 1;
                }
                (successed, failed)
            })
    }
}

#[derive(Debug)]
pub struct FileItem<'a> {
    pub remote_item: &'a RemoteFileItem<'a>,
    base_dir: &'a Path,
    sha1: Option<String>,
    len: u64,
    fail_reason: Option<String>,
}

impl<'a> FileItem<'a> {
    #[allow(dead_code)]
    pub fn standalone(file_path: &'a Path, remote_item: &'a RemoteFileItem) -> Self {
        Self {
            remote_item,
            base_dir: file_path,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
    pub fn new(base_dir: &'a Path, remote_item: &'a RemoteFileItem) -> Self {
        Self {
            base_dir,
            remote_item,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
}

impl<'a> FileItem<'a> {
    pub fn get_path(&self) -> Option<String> {
        let rp = self.remote_item.get_path();
        self
            .base_dir
            .join(&rp)
            .to_str().map(|s|s.to_string())
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn set_len(&mut self, len: u64) {
        self.len = len;
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1.as_ref().map(|s| s.as_str())
    }

    pub fn set_sha1(&mut self, sha1: impl AsRef<str>) {
        self.sha1.replace(sha1.as_ref().to_string());
    }

    pub fn set_fail_reason(&mut self, fail_reason: impl AsRef<str>) {
        self.fail_reason.replace(fail_reason.as_ref().to_string());
    }

    pub fn get_fail_reason(&self) -> Option<&String> {
        self.fail_reason.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{RemoteFileItem, RemoteFileItemDir};
    use super::*;
    use crate::actions::write_str_to_file;
    use crate::develope::develope_data;
    use crate::log_util;
    use log::*;
    use std::fs;
    use std::io::prelude::Read;

    #[test]
    fn new_file_item() {
        log_util::setup_logger(vec![""], vec![]);
        let rdo = RemoteFileItemDirOwned::load_dir("fixtures/adir");
        let rd: RemoteFileItemDir = (&rdo).into();
        let remote_item = rd
            .get_items()
            .iter()
            .find(|ri| ri.get_path().ends_with("鮮やか"))
            .expect("must have at least one.");
        let fi = FileItem::new(Path::new("not_in_git"), remote_item);
        assert_eq!(fi.get_path(), Some("not_in_git/鮮やか".to_string()));
    }

    #[test]
    fn t_from_path() {
        log_util::setup_logger(vec![""], vec![]);
        let rdo = RemoteFileItemDirOwned::load_dir("fixtures/adir");
        let rd: RemoteFileItemDir = (&rdo).into();
        let json_str = serde_json::to_string_pretty(&rd).expect("deserialize should success");
        info!("{:?}", json_str);
        write_str_to_file(json_str, "fixtures/linux_remote_item_dir.json")
            .expect("should success.");
        assert_eq!(rd.get_items().len(), 5_usize);
    }

    #[test]
    fn t_download_remote_dir() {
        log_util::setup_logger(vec![""], vec![]);
        let mut buffer = String::new();
        fs::File::open("fixtures/linux_remote_item_dir.json")
            .expect("success.")
            .read_to_string(&mut buffer)
            .expect("success.");
        let remote_dir =
            serde_json::from_str::<RemoteFileItemDir>(&buffer).expect("deserialize should success");
        info!("{:?}", remote_dir);
        let ldpn = "not_in_git/local_dir";
        let ldp = Path::new(ldpn);
        if ldp.exists() {
            fs::remove_dir_all(ldpn).expect("remove all files under this directory.");
        }
        let local_dir = FileItemDir::new(ldp, remote_dir);
        let (_tcp, mut sess, _dev_env) = develope_data::connect_to_ubuntu();
        let (successed, failed) = local_dir.download_files(&mut sess);
        info!("{:?}", local_dir);
        assert_eq!(failed, 0_u64);
        assert_eq!(successed, 5_u64);
    }

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
