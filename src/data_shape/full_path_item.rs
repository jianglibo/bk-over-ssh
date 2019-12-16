use super::string_path;
use super::SlashPath;
use crate::data_shape::data_shape_util;
use crate::protocol::TransferType;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::io;
use std::path::{Path, PathBuf};
use encoding_rs::*;

#[derive(Debug, Fail)]
pub enum FullPathFileItemError {
    #[fail(display = "invalid encode: {:?}", _0)]
    Encode(PathBuf),
    #[fail(display = "get meta failed: {:?}", _0)]
    Meta(#[fail(cause)] io::Error),
    #[fail(display = "file not exist: {:?}", _0)]
    NotExist(PathBuf),
}

#[derive(Debug)]
pub enum FileChanged {
    Len(u64, u64),
    Modified(Option<u64>, Option<u64>),
    Sha1(Option<String>, Option<String>),
    NoMetadata,
    NoChange,
}

/// Like a disk directory, but it contains FullPathFileItem.
/// No from_dir and to_dir, but absolute local_path and remote_path.
#[derive(Deserialize, Serialize, Debug)]
pub struct FullPathFileItem {
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub from_path: SlashPath,
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub to_path: SlashPath,
    pub sha1: Option<String>,
    pub len: u64,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

impl FullPathFileItem {
    /// if dir_to_read is "/a/b" and the absolute_file_to_read is "/a/b/c.txt"
    /// after relativelize the result is: b/c.txt.
    /// if server_distinct_id is "abc", then the final to_dir is "abc/b/c.txt", it a relative path,
    /// and hand over to another side, get the final absolute path by another side.
    /// for pushing the server_distinct_id is necessory because there is no way to determine where do the file come from.
    /// but for pull, there is an ip address as distinctness, so it's unnecessary.
    pub fn create_item_from_path(
        from_dir: &SlashPath,
        absolute_file_to_read: PathBuf,
        to_dir_base: &SlashPath,
        skip_sha1: bool,
        possible_encoding: &Vec<&'static Encoding>,
    ) -> Result<Self, failure::Error> {
        let fmeta = data_shape_util::get_file_meta(absolute_file_to_read.as_path(), skip_sha1)?;
        let relative_path = from_dir.strip_prefix(absolute_file_to_read.as_path(), possible_encoding)?;
        let from_path = SlashPath::from_path(absolute_file_to_read.as_path(), possible_encoding)?;

        Ok(Self {
            from_path,
            to_path: to_dir_base.join(relative_path),
            sha1: fmeta.sha1,
            len: fmeta.len,
            modified: fmeta.modified,
            created: fmeta.created,
        })
    }

    pub fn changed(&self, file_path: impl AsRef<Path>) -> FileChanged {
        if let Ok(fmeta) = data_shape_util::get_file_meta(file_path, self.sha1.is_none()) {
            if fmeta.len != self.len {
                FileChanged::Len(fmeta.len, self.len)
            } else if fmeta.modified != self.modified {
                FileChanged::Modified(fmeta.modified, self.modified)
            } else if fmeta.sha1 != self.sha1 {
                FileChanged::Sha1(fmeta.sha1, self.sha1.as_ref().cloned())
            } else {
                FileChanged::NoChange
            }
        } else {
            FileChanged::NoMetadata
        }
    }

    pub fn as_sent_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.insert(0, TransferType::FileItem.to_u8());
        let json_str =
            serde_json::to_string(&self).expect("FullPathFileItem to serialize to string.");
        let bytes = json_str.as_bytes();
        let bytes_len: u64 = bytes.len().try_into().expect("usize convert to u64");
        v.append(&mut bytes_len.to_be_bytes().to_vec());
        v.append(&mut bytes.to_vec());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::Directory;
    use crate::develope::tutil;
    use crate::log_util;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::file_item_directory"],
            None,
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_push_file_directories() -> Result<(), failure::Error> {
        log();

        let tdir = tutil::TestDir::new();
        let dir_pb = tdir.create_sub_dir("a-dir/bbb");

        tutil::make_a_file_with_content(dir_pb.as_path(), "a.txt", "abc")?;
        tutil::make_a_file_with_content(dir_pb.as_path(), "a.png", "abc")?;
        tutil::make_a_file_with_content(dir_pb.as_path(), "a.log", "abc")?;
        tutil::make_a_file_with_content(dir_pb.as_path(), "a.bak", "abc")?;

        let yml = format!(
            r##"
to_dir: ~
from_dir: {}
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##,
            dir_pb.to_string_lossy()
        );

        let mut dir = serde_yaml::from_str::<Directory>(&yml)?;
        println!("{}", dir.from_dir);

        assert!(dir.includes_patterns.is_none());
        assert!(dir.excludes_patterns.is_none());

        dir.compile_patterns()?;

        println!("{}", dir.from_dir);
        assert!(dir.includes_patterns.is_some());
        assert!(dir.excludes_patterns.is_some());

        let files = dir
            .file_item_iter("abc", false, &vec![])
            .filter_map(|k| k.ok())
            .map(|it| {
                println!("to_path: {:?}", it.to_path.slash);
                it
            })
            .collect::<Vec<FullPathFileItem>>();

        assert_eq!(files.len(), 2);
        let s = &files.get(0).unwrap().to_path.slash;
        eprintln!("the final to_path is: {}", s);
        assert!(
            // s.starts_with("abc/a-dir/bbb"),
            s.starts_with("abc/bbb"), // need the result like above.
            "should starts_with abc/bbb"
        );

        let yml = format!(
            r##"
to_dir: a-dir/bbb
# if no to_dir the base should be 'bbb'
from_dir: {}
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##,
            dir_pb.to_string_lossy()
        );

        let mut dir = serde_yaml::from_str::<Directory>(&yml)?;
        println!("{}", dir.from_dir);
        dir.compile_patterns()?;

        let files = dir
            .file_item_iter("abc", false, &vec![])
            .filter_map(|k| k.ok())
            .map(|it| {
                println!("to_path: {:?}", it.to_path.slash);
                it
            })
            .collect::<Vec<FullPathFileItem>>();

        assert_eq!(files.len(), 2);
        let s = &files.get(0).unwrap().to_path.slash;
        eprintln!("the final to_path is: {}", s);
        assert!(
            // s.starts_with("abc/a-dir/bbb"),
            s.starts_with("abc/a-dir/bbb"), // need the result like above.
            "should starts_with abc/a-dir/bbb"
        );
        Ok(())
    }
}
