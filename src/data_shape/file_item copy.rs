use super::RemoteFileItemOwned;
use std::path::Path;
use super::string_path;

#[derive(Debug)]
pub struct FileItem<'a> {
    pub remote_item: &'a RemoteFileItem<'a>,
    base_dir: &'a Path,
    remote_base_dir: Option<&'a str>,
    sha1: Option<String>,
    len: u64,
    fail_reason: Option<String>,
}

impl<'a> FileItem<'a> {
    #[allow(dead_code)]
    pub fn standalone(file_path: &'a Path, remote_base_dir: Option<&'a str>, remote_item: &'a RemoteFileItem) -> Self {
        Self {
            remote_item,
            base_dir: file_path,
            remote_base_dir,
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }

    pub fn new(base_dir: &'a Path, remote_base_dir: &'a str, remote_item: &'a RemoteFileItem) -> Self {
        Self {
            base_dir,
            remote_item,
            remote_base_dir: Some(remote_base_dir),
            sha1: None,
            len: 0_u64,
            fail_reason: None,
        }
    }
}

impl<'a> FileItem<'a> {
    pub fn get_local_path(&self) -> Option<String> {
        let rp = self.remote_item.get_path();
        self.base_dir.join(&rp).to_str().map(|s| s.to_string())
    }

    pub fn get_remote_path(&self) -> String {
        if let Some(rbd) = self.remote_base_dir {
            string_path::join_path(rbd, self.remote_item.get_path())
        } else {
            self.remote_item.get_path().to_string()
        }
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
