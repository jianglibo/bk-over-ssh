use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use super::{RemoteFileItem, RemoteFileItemDir, RemoteFileItemDirOwned};
// use std::marker::PhantomData;

#[derive(Debug)]
pub struct FileItemDir<'a> {
    pub base_path: &'a str,
    remote_dir: RemoteFileItemDir<'a>,
    // items: Vec<FileItem<'a>>,
}

impl<'a> FileItemDir<'a> {
    pub fn new(base_path: &'static str, remote_dir: RemoteFileItemDir<'a>) -> Self {
        Self {
            base_path: base_path,
            remote_dir,
            // items: remote_dir
            //     ._items
            //     .iter()
            //     .map(|ri| FileItem::new(base_path.as_ref(), ri))
            //     .collect(),
        }
    }

    pub fn download_remote(&mut self) {

    }
}


#[derive(Debug)]
pub struct FileItem<'a> {
    pub remote_item: &'a RemoteFileItem<'a>,
    path: String,
    sha1: Option<String>,
    len: u64,
    fail_reason: Option<String>,
}

impl<'a> FileItem<'a> {

    pub fn standalone(file_path: impl AsRef<str> + 'a, remote_item: &'a RemoteFileItem) -> Self {
        Self {
            remote_item,
            path: file_path.as_ref().to_string(),
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
    pub fn new(local_dir: impl AsRef<str>, remote_item: &'a RemoteFileItem) -> Self {
        if let Some(path) = remote_item.calculate_local_path(local_dir) {
            Self {
                remote_item,
                path,
                sha1: None,
                len: 0_u64,
                fail_reason: None,
            }
        } else {
            Self {
                remote_item,
                path: String::from(""),
                sha1: None,
                len: 0_u64,
                fail_reason: Some("calculate_local_path failed.".to_string()),
            }
        }
    }
}

impl<'a> FileItem<'a> {
    pub fn get_path(&self) -> &str {
        self.path.as_str()
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn set_len(&mut self, len: u64) {
        self.len = len;
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1.as_ref().map(|s|s.as_str())
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
    use super::*;
    use crate::log_util;
    use crate::actions::write_str_to_file;
    use std::io::prelude::{Read};
    use log::*;
    use super::super::{RemoteFileItem, RemoteFileItemDir};


    #[test]
    fn new_file_item() {
        let rdo = RemoteFileItemDirOwned::load_path("fixtures/adir");
        let rd: RemoteFileItemDir = (&rdo).into();
        let remote_item = rd.get_items().iter()
            .find(|ri|ri.get_path().ends_with("鮮やか")).expect("must have at least one.");
        let fi = FileItem::new("not_in_git", remote_item);
        assert_eq!(fi.get_path(), "not_in_git/鮮やか");
    }

    #[test]
    fn t_from_path() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let rdo = RemoteFileItemDirOwned::load_path("fixtures/adir");
        let rd: RemoteFileItemDir = (&rdo).into();
        let json_str = serde_json::to_string_pretty(&rd).expect("deserialize should success");
        info!("{:?}", json_str);
        write_str_to_file(json_str, "fixtures/linux_remote_item_dir.json").expect("should success.");
        assert_eq!(rd.get_items().len(), 5_usize); 
    }

    #[test]
    fn t_download_remote_dir() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let mut buffer = String::new();
        std::fs::File::open("fixtures/linux_remote_item_dir.json").expect("success.").read_to_string(&mut buffer).expect("success.");
        let remote_dir = serde_json::from_str::<RemoteFileItemDir>(&buffer).expect("deserialize should success");
        info!("{:?}", remote_dir);
        let local_dir = FileItemDir::new("not_in_git/local_dir", remote_dir);
    }

    #[test]
    fn t_concat_str() {
        let a = "a";
        let b = "b";
        let c = [a, b].concat();
        assert_eq!(c, String::from("ab"));
        assert_eq!(a, "a");

        let aa = String::from("a");
        let bb = String::from("b");

        // let cc = [&aa, bb].concat();
        // assert_eq!(cc, String::from("ab"));
        // assert_eq!(aa, String::from("a"));
    }
}
