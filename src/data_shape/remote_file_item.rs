use crate::actions::hash_file_sha1;
use log::*;
use serde::{Deserialize, Serialize};
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItemDir<'a> {
    pub base_path: Option<&'a str>,
    #[serde(borrow)]
    items: Vec<RemoteFileItem<'a>>,
}

impl<'a> RemoteFileItemDir<'a> {
    pub fn new() -> Self {
        Self {
            base_path: None,
            items: Vec::<RemoteFileItem<'a>>::new(),
        }
    }

    pub fn fill_base_dir(&mut self) {
        if self.base_path.is_some() {
            let bp = self.base_path.unwrap();
            for it in self.items.iter_mut() {
                it.base_dir.replace(bp);
            }
        }
    }

    pub fn get_items(&self) -> &Vec<RemoteFileItem<'a>> {
        &self.items
    }
}

impl<'a> std::convert::From<&'a RemoteFileItemDirOwned> for RemoteFileItemDir<'a> {
    fn from(rfio: &'a RemoteFileItemDirOwned) -> Self {
        Self {
            base_path: rfio.base_path.as_ref().map(|ss| ss.as_str()),
            items: rfio.items.iter().map(Into::into).collect(),
        }
    }
}

pub struct RemoteFileItemDirOwned {
    pub base_path: Option<String>,
    items: Vec<RemoteFileItemOwned>,
}

impl RemoteFileItemDirOwned {
    fn load_remote_item_owned(dir_path: impl AsRef<Path>) -> Vec<RemoteFileItemOwned> {
        let bp = Path::new(dir_path.as_ref()).canonicalize();
        match bp {
            Ok(base_path) => WalkDir::new(&base_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_file())
                .filter_map(|d| d.path().canonicalize().ok())
                .filter_map(|d| RemoteFileItemOwned::from_path(&base_path, d))
                .collect(),
            Err(err) => {
                error!("load_dir resolve path failed: {:?}", err);
                Vec::new()
            }
        }
    }

    pub fn load_path(base_path: impl AsRef<Path>) -> Self {
        let items = RemoteFileItemDirOwned::load_remote_item_owned(base_path.as_ref());
        Self {
            base_path: base_path.as_ref().to_str().map(|s| s.to_string()),
            items,
        }
    }
}

pub struct RemoteFileItemOwned {
    path: String,
    sha1: Option<String>,
    len: u64,
}

impl RemoteFileItemOwned {
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

impl<'a> std::convert::From<&'a RemoteFileItemOwned> for RemoteFileItem<'a> {
    fn from(rfio: &'a RemoteFileItemOwned) -> Self {
        Self {
            base_dir: None,
            path: rfio.path.as_str(),
            sha1: rfio.sha1.as_ref().map(|ss| ss.as_str()),
            len: rfio.len,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteFileItem<'a> {
    #[serde(skip)]
    base_dir: Option<&'a str>,
    path: &'a str,
    sha1: Option<&'a str>,
    len: u64,
}

impl<'a> RemoteFileItem<'a> {
    pub fn new(path: &'a str) -> Self {
        Self {
            base_dir: None,
            path,
            sha1: None,
            len: 0_u64,
        }
    }

    pub fn get_path(&self) -> String {
        if let Some(bp) = self.base_dir {
            format!("{}/{}", bp, self.path)
        } else {
            self.path.to_string()
        }
        
    }

    pub fn get_len(&self) -> u64 {
        self.len
    }

    pub fn get_sha1(&self) -> Option<&str> {
        self.sha1
    }

    // pub fn calculate_local_path(&self, local_dir: impl AsRef<str>) -> Option<String> {
    //     let path = Path::new(local_dir.as_ref());
    //     path.join(&self.path).to_str().map(|s| s.to_string())
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::write_str_to_file;
    use crate::log_util;
    use log::*;
    use std::io::prelude::Read;

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
        let rd: RemoteFileItemDir =
            serde_json::from_str(&s).expect("load develope_data should success.");
        assert_eq!(
            rd.get_items()
                .iter()
                .next()
                .and_then(|ri| Some(ri.get_path().to_string())),
            Some("a".to_string())
        );
    }

    #[test]
    fn t_from_path() {
        log_util::setup_logger(vec![""], vec![]);
        let rdo = RemoteFileItemDirOwned::load_path("fixtures/adir");
        let rd: RemoteFileItemDir = (&rdo).into();
        let json_str = serde_json::to_string_pretty(&rd).expect("deserialize should success");
        info!("{:?}", json_str);
        write_str_to_file(json_str, "fixtures/linux_remote_item_dir.json")
            .expect("should success.");
        assert_eq!(rd.get_items().len(), 5_usize);
    }
}
