use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
// use std::marker::PhantomData;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteFileItemDir {
    pub base_path: Option<String>,
    pub items: Vec<RemoteFileItem>,
}

impl RemoteFileItemDir {
    pub fn new() -> Self {
        Self {
            base_path: None,
            items: Vec::new(),
        }
    }

    pub fn load_dir(dir_path: impl AsRef<str>) -> Self {
        let bp = Path::new(dir_path.as_ref()).canonicalize();
        match bp {
            Ok(base_path) => {
                let rfs: Vec<RemoteFileItem> = WalkDir::new(&base_path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|d| d.file_type().is_file())
                    .filter_map(|d| d.path().canonicalize().ok())
                    .filter_map(|d| RemoteFileItem::from_path(&base_path, d))
                    .collect();
                Self {
                    base_path: Some(base_path.to_string_lossy().to_owned().to_string()),
                    items: rfs,
                }
            }
            Err(err) => {
                error!("load_dir resolve path failed: {:?}", err);
                Self::new()
            }
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct FileItemDir {
    pub base_path: String,
    pub remote_base_path: Option<String>,
    pub items: Vec<FileItem>,
}

impl FileItemDir {
    pub fn new(base_path: impl AsRef<str>, remote_item_dir: RemoteFileItemDir) -> Self {
        Self {
            base_path: base_path.as_ref().to_string(),
            remote_base_path: remote_item_dir.base_path,
            items: remote_item_dir
                .items
                .into_iter()
                .map(|ri| FileItem::new(ri))
                .collect(),
        }
    }
}

impl Iterator for FileItemDir {
    type Item = FileItem;
    fn next(&mut self) -> Option<FileItem> {
        None
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteFileItem {
    pub path: String,
    pub sha1: Option<String>,
    pub len: u64,
}

impl RemoteFileItem {
    pub fn new(path: impl AsRef<str>) -> Self {
        Self {
            path: path.as_ref().to_string(),
            sha1: None,
            len: 0_u64,
        }
    }

    pub fn from_path(base_path: impl AsRef<Path>, path: PathBuf) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                if let Some(sha1) = hash_file_sha1(&path) {
                    let relative_o = path
                        .strip_prefix(&base_path)
                        .ok()
                        .and_then(|p| p.to_str().map(|s| s.to_string()));
                    if let Some(relative) = relative_o {
                        return Some(Self {
                            path: relative,
                            sha1: Some(sha1),
                            len: metadata.len(),
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

// The intention is that the underlying data is only valid for the lifetime 'a, so Slice should not outlive 'a
// Deserializer lifetimes are described here 20. When you have a struct that’s borrowing from the deserializer, which has a 'de lifetime parameter, the #[serde(borrow)] attribute instructs the proc macro to add a lifetime bound specifying that 'de outlives the lifetime parameter on the field. This is to enforce that the data the deserializer has, which is being borrowed from, won’t vanish from underneath the borrows in your struct.
// In your case the lifetime parameter is 'a - the macro will then generate a 'de: 'a bound on the Deserialize impl for the struct.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct FileItem {
    pub remote_item: RemoteFileItem,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub len: u64,
    pub fail_reason: Option<String>,
}

impl FileItem {
    pub fn new(remote_item: RemoteFileItem) -> Self {
        Self {
            remote_item,
            path: None,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
            // phantom: PhantomData,
        }
    }
}

impl FileItem {
    pub fn get_local_path(&self) -> Option<&OsStr> {
        if let Some(lp) = self.path.as_ref() {
            Some(OsStr::new(lp))
        } else {
            Path::new(&self.remote_item.path).file_name()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;

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
        let fid: RemoteFileItemDir =
            serde_json::from_str(&s).expect("load develope_data should success.");
        assert_eq!(
            fid.items.get(0).and_then(|ri| Some(&ri.path)),
            Some(&"a".to_string())
        );
    }

    #[test]
    fn new_file_item() {
        let fi = FileItem::new(RemoteFileItem::new("a"));
        let fi_str = serde_json::to_string(&fi).expect("should serialize.");
        println!("{:?}", fi_str);
        let fi1 = serde_json::from_str::<FileItem>(&fi_str).expect("should deserialize.");
        assert!(fi1.path.is_none());
    }

    #[test]
    fn t_from_path() {
        log_util::setup_logger(vec![""], vec![]).expect("log should init.");
        let rd = RemoteFileItemDir::load_dir("fixtures");
        let json_str = serde_json::to_string(&rd).expect("deserialize should success.");
        println!("{:?}", json_str);
        assert_eq!(rd.items.len(), 4_usize);
    }
}
