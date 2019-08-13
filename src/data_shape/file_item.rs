use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
// use std::marker::PhantomData;

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItemDir<'a> {
    pub base_path: Option<String>,
    #[serde(borrow)]
    _items: Vec<RemoteFileItem<'a>>,
}

impl<'a> RemoteFileItemDir<'a> {
    pub fn new() -> Self {
        Self {
            base_path: None,
            _items: Vec::new(),
        }
    }

    pub fn get_items(&self) -> &Vec<RemoteFileItem<'a>> {
        &self._items
    }

    pub fn load_dir_to_tuple(dir_path: impl AsRef<Path>) -> Vec<(String, String, u64)> {
        let bp = Path::new(dir_path.as_ref()).canonicalize();
        match bp {
            Ok(base_path) => {
                WalkDir::new(&base_path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|d| d.file_type().is_file())
                    .filter_map(|d| d.path().canonicalize().ok())
                    .filter_map(|d| RemoteFileItem::tuple_from_path(&base_path, d))
                    .collect()
            }
            Err(err) => {
                error!("load_dir resolve path failed: {:?}", err);
                Vec::new()
            }
        }
    }

    pub fn load_from_tuple_vec(dir_path: impl AsRef<Path>, tuple_vec: &'a Vec<(String, String, u64)>) -> Self {
        let bp = Path::new(dir_path.as_ref()).canonicalize().expect("canonicalize should success.");

        let its = tuple_vec.iter().map(|v| RemoteFileItem::from_tuple(v)).collect::<Vec<RemoteFileItem<'a>>>();
        Self {
            base_path: Some(bp.to_string_lossy().to_string()),
            _items: its,
        }
    }

    // pub fn load_dir(dir_path: impl AsRef<Path>) -> Self {
    //     let bp = Path::new(dir_path.as_ref()).canonicalize();
    //     match bp {
    //         Ok(base_path) => {
    //             let rfs: Vec<RemoteFileItem> = WalkDir::new(&base_path)
    //                 .follow_links(false)
    //                 .into_iter()
    //                 .filter_map(|e| e.ok())
    //                 .filter(|d| d.file_type().is_file())
    //                 .filter_map(|d| d.path().canonicalize().ok())
    //                 .filter_map(|d| RemoteFileItem::from_path(&base_path, d))
    //                 .collect();
    //             Self {
    //                 base_path: Some(base_path.to_string_lossy().to_string()),
    //                 _items: rfs,
    //             }
    //         }
    //         Err(err) => {
    //             error!("load_dir resolve path failed: {:?}", err);
    //             Self::new()
    //         }
    //     }
    // }
}

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

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItem<'a> {
    path: &'a str,
    sha1: Option<&'a str>,
    len: u64,
}

impl<'a> RemoteFileItem<'a> {
    // pub fn new(path: impl AsRef<str>) -> Self {
    //     Self {
    //         path: path.as_ref().to_string(),
    //         sha1: None,
    //         len: 0_u64,
    //     }
    // }

    pub fn get_path(&self) -> &str {
        self.path
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1
    }

    pub fn calculate_local_path(&self, local_dir: impl AsRef<str>) -> Option<String> {
        let path = Path::new(local_dir.as_ref());
        path.join(&self.path).to_str().map(|s| s.to_string())
    }

    pub fn tuple_from_path(base_path: impl AsRef<Path>, path: PathBuf) -> Option<(String, String, u64)> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                if let Some(sha1) = hash_file_sha1(&path) {
                    let relative_o = path
                        .strip_prefix(&base_path)
                        .ok()
                        .and_then(|p| p.to_str());
                    if let Some(relative) = relative_o {
                        return Some((
                            relative.to_string(),
                            sha1,
                            metadata.len()
                        ));
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

    pub fn from_tuple(tuple_value: &'a (String, String, u64)) -> Self {
        Self {
            path: tuple_value.0.as_str(),
            sha1: Some(tuple_value.1.as_ref()),
            len: tuple_value.2
        }
    }

    // pub fn from_path(base_path: impl AsRef<Path>, path: PathBuf) -> Option<Self> {
    //     let metadata_r = path.metadata();
    //     match metadata_r {
    //         Ok(metadata) => {
    //             if let Some(sha1) = hash_file_sha1(&path) {
    //                 let relative_o = path
    //                     .strip_prefix(&base_path)
    //                     .ok()
    //                     .and_then(|p| p.to_str());
    //                 if let Some(relative) = relative_o {
    //                     return Some(Self {
    //                         path: relative,
    //                         sha1: Some(sha1.as_ref()),
    //                         len: metadata.len(),
    //                     });
    //                 } else {
    //                     error!("RemoteFileItem path name to_str() failed. {:?}", path);
    //                 }
    //             }
    //         }
    //         Err(err) => {
    //             error!("RemoteFileItem from_path failed: {:?}, {:?}", path, err);
    //         }
    //     }
    //     None
    // }
}

// The intention is that the underlying data is only valid for the lifetime 'a, so Slice should not outlive 'a
// Deserializer lifetimes are described here 20. When you have a struct that’s borrowing from the deserializer, which has a 'de lifetime parameter, the #[serde(borrow)] attribute instructs the proc macro to add a lifetime bound specifying that 'de outlives the lifetime parameter on the field. This is to enforce that the data the deserializer has, which is being borrowed from, won’t vanish from underneath the borrows in your struct.
// In your case the lifetime parameter is 'a - the macro will then generate a 'de: 'a bound on the Deserialize impl for the struct.
#[derive(Debug)]
pub struct FileItem<'a> {
    // #[serde(borrow)]
    pub remote_item: &'a RemoteFileItem<'a>,
    path: Option<String>,
    sha1: Option<String>,
    len: u64,
    fail_reason: Option<String>,
}

impl<'a> FileItem<'a> {
    pub fn new(local_dir: impl AsRef<str>, remote_item: &'a RemoteFileItem) -> Self {
        let path = remote_item.calculate_local_path(local_dir);
        Self {
            remote_item,
            path,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
}

impl<'a> FileItem<'a> {
    pub fn get_path(&self) -> Option<&str> {
        self.path.as_ref().map(|s|s.as_str())
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use crate::actions::write_str_to_file;
    use std::io::prelude::{Read};
    use log::*;

    #[test]
    fn deserialize_file_item() {
        let s = r#"
{
    "base_path": "/abc",
    "items":[
    {
            "path": "a",
            "sha1": "cc",
            "len": 55
    }]
}
"#;
        let mut rd: RemoteFileItemDir =
            serde_json::from_str(&s).expect("load develope_data should success.");
        assert_eq!(
            rd.get_items().iter().next().and_then(|ri| Some(ri.get_path().to_string())),
            Some("a".to_string())
        );
    }

    #[test]
    fn new_file_item() {
        let values = RemoteFileItemDir::load_dir_to_tuple("fixtures/adir");
        let rd = RemoteFileItemDir::load_from_tuple_vec("fixtures/adir", &values);
        let remote_item = rd.get_items().iter()
            .find(|ri|ri.get_path().ends_with("鮮やか")).expect("must have at least one.");
        let fi = FileItem::new("not_in_git", remote_item);
        assert_eq!(fi.get_path(), Some("not_in_git/鮮やか"));
    }

    #[test]
    fn t_from_path() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let values = RemoteFileItemDir::load_dir_to_tuple("fixtures/adir");
        let rd = RemoteFileItemDir::load_from_tuple_vec("fixtures/adir", &values);
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
