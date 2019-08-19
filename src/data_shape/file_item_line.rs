use super::RemoteFileItemLine;
use crate::actions::copy_a_file_item;
use log::*;
use ssh2;
use std::io::prelude::Read;
use std::iter::Iterator;
use std::path::Path;
use std::{fs, io};

// pub fn download_dirs<'a>(session: &mut ssh2::Session, json_file: impl AsRef<str> + 'a, out: impl AsRef<str> + 'a) -> Result<(), failure::Error> {
//     let rf = fs::OpenOptions::new()
//         .open(json_file.as_ref())?;
//     let mut reader = io::BufReader::new(rf);
//     let mut content = String::new();
//     reader.read_to_string(&mut content)?;
//     let rds: Vec<RemoteFileItemDir<'_>> = serde_json::from_str(content.as_str())?;
//     for rd in rds {
//         let fid = FileItemDir::new(Path::new(out.as_ref()), rd);
//         fid.download_files(session);
//     }
//     Ok(())
// }

#[derive(Debug)]
pub struct FileItemLine<'a> {
    pub remote_item: &'a RemoteFileItemLine<'a>,
    base_dir: &'a Path,
    sha1: Option<String>,
    len: u64,
    fail_reason: Option<String>,
}

impl<'a> FileItemLine<'a> {
    #[allow(dead_code)]
    pub fn standalone(file_path: &'a Path, remote_item: &'a RemoteFileItemLine) -> Self {
        Self {
            remote_item,
            base_dir: file_path,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
    pub fn new(base_dir: &'a Path, remote_item: &'a RemoteFileItemLine) -> Self {
        Self {
            base_dir,
            remote_item,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
}

impl<'a> FileItemLine<'a> {
    pub fn get_path(&self) -> Option<String> {
        let rp = self.remote_item.get_path();
        self.base_dir.join(&rp).to_str().map(|s| s.to_string())
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
    use super::super::RemoteFileItemLine;
    use super::*;
    use crate::actions::write_str_to_file;
    use crate::develope::develope_data;
    use crate::log_util;
    use log::*;
    use std::fs;
    use std::io::prelude::Read;

    #[test]
    fn new_file_item() {
        // log_util::setup_logger(vec![""], vec![]);
        // let rdo = RemoteFileItemDirOwned::load_dir("fixtures/adir");
        // let rd: RemoteFileItemDir = (&rdo).into();
        // let remote_item = rd
        //     .get_items()
        //     .iter()
        //     .find(|ri| ri.get_path().ends_with("鮮やか"))
        //     .expect("must have at least one.");
        // let fi = FileItem::new(Path::new("not_in_git"), remote_item);
        // assert_eq!(fi.get_path(), Some("not_in_git/鮮やか".to_string()));
    }

    #[test]
    fn t_from_path() {
        // log_util::setup_logger(vec![""], vec![]);
        // let rdo = RemoteFileItemDirOwned::load_dir("fixtures/adir");
        // let rd: RemoteFileItemDir = (&rdo).into();
        // let json_str = serde_json::to_string_pretty(&rd).expect("deserialize should success");
        // info!("{:?}", json_str);
        // write_str_to_file(json_str, "fixtures/linux_remote_item_dir.json")
        //     .expect("should success.");
        // assert_eq!(rd.get_items().len(), 5_usize);
    }

    #[test]
    fn t_download_remote_dir() {
        // log_util::setup_logger(vec![""], vec![]);
        // let mut buffer = String::new();
        // fs::File::open("fixtures/linux_remote_item_dir.json")
        //     .expect("success.")
        //     .read_to_string(&mut buffer)
        //     .expect("success.");
        // let remote_dir =
        //     serde_json::from_str::<RemoteFileItemDir>(&buffer).expect("deserialize should success");
        // info!("{:?}", remote_dir);
        // let ldpn = "not_in_git/local_dir";
        // let ldp = Path::new(ldpn);
        // if ldp.exists() {
        //     fs::remove_dir_all(ldpn).expect("remove all files under this directory.");
        // }
        // let local_dir = FileItemDir::new(ldp, remote_dir);
        // let (_tcp, mut sess, _dev_env) = develope_data::connect_to_ubuntu();
        // let (successed, failed) = local_dir.download_files(&mut sess);
        // info!("{:?}", local_dir);
        // assert_eq!(failed, 0_u64);
        // assert_eq!(successed, 5_u64);
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
