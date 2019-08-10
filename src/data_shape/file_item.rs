use serde::{Deserialize, Serialize};
use std::path::Path;
use std::ffi::{OsStr};
use std::iter::Iterator;
// use std::marker::PhantomData;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteFileItemDir<'a> {
    pub base_path: Option<&'a str>,
    pub items: Vec<RemoteFileItem<'a>>,
}

impl<'a> RemoteFileItemDir<'a> {
    pub fn new() -> Self {
        Self {
            base_path: None,
            items: Vec::new(),
        }
    }

    // pub fn load_dir(dir_path: impl AsRef<str>) -> Self {

    // }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct FileItemDir<'a> {
    pub base_path: &'a str,
    pub remote_base_path: Option<&'a str>,
    pub items: Vec<FileItem<'a>>,
}

impl<'a> FileItemDir<'a> {
    pub fn new(base_path: &'a str, remote_item_dir: RemoteFileItemDir<'a>) -> Self {
        Self {
            base_path,
            remote_base_path: remote_item_dir.base_path,
            items: remote_item_dir.items.into_iter().map(|ri| FileItem::new(ri)).collect()
        }
    }
}

impl<'a> Iterator for FileItemDir<'a> {
    type Item = &'a mut FileItem<'a>;
    fn next(&mut self) -> Option<&'a mut FileItem<'a>> {
        None
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteFileItem<'a> {
    #[serde(borrow)]
    pub path: &'a str,
    #[serde(borrow)]
    pub sha1: Option<&'a str>,
    pub len: u64,
}

impl<'a> RemoteFileItem<'a> {
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            sha1: None,
            len: 0_u64,
        }
    }
}

// The intention is that the underlying data is only valid for the lifetime 'a, so Slice should not outlive 'a
// Deserializer lifetimes are described here 20. When you have a struct that’s borrowing from the deserializer, which has a 'de lifetime parameter, the #[serde(borrow)] attribute instructs the proc macro to add a lifetime bound specifying that 'de outlives the lifetime parameter on the field. This is to enforce that the data the deserializer has, which is being borrowed from, won’t vanish from underneath the borrows in your struct.
// In your case the lifetime parameter is 'a - the macro will then generate a 'de: 'a bound on the Deserialize impl for the struct.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct FileItem<'a> {
    #[serde(borrow)]
    pub remote_item: RemoteFileItem<'a>,
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub len: u64,
    pub fail_reason: Option<String>,
}

impl<'a> FileItem<'a> {
    pub fn new(remote_item: RemoteFileItem<'a>) -> Self {
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

impl FileItem<'_> {
    pub fn get_local_path(&self) -> Option<&OsStr> {
        if let Some(lp) = self.path.as_ref() {
            Some(OsStr::new(lp))
        } else {
            Path::new(self.remote_item.path).file_name()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_file_item() {
        let s = 
r#"
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
    let fid: RemoteFileItemDir = serde_json::from_str(&s).expect("load develope_data should success.");
    assert_eq!(fid.items.get(0).and_then(|ri|Some(ri.path)), Some("a"));
    }

    #[test]
    fn new_file_item() {
        let fi = FileItem::new(RemoteFileItem::new("a"));
        let fi_str = serde_json::to_string(&fi).expect("should serialize.");
        println!("{:?}", fi_str);
        let fi1 = serde_json::from_str::<FileItem>(&fi_str).expect("should deserialize.");
        assert!(fi1.path.is_none());
    }
}