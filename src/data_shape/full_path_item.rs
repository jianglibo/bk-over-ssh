use super::string_path;
use super::SlashPath;
use crate::data_shape::data_shape_util;
use crate::protocol::TransferType;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::path::Path;
use std::path::PathBuf;

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
    pub fn create_item_from_path(
        dir_to_read: &SlashPath,
        absolute_file_to_read: PathBuf,
        server_distinct_id: impl AsRef<str>,
        skip_sha1: bool,
    ) -> Option<Self> {
        let server_distinct_id = SlashPath::new(server_distinct_id);
        if let Some(fmeta) =
            data_shape_util::get_file_meta(absolute_file_to_read.as_path(), skip_sha1)
        {
            // if dir_to_read is "/a/b" and the absolute_file_to_read is "/a/b/c.txt"
            // after relativelize the result is: b/c.txt.
            if let Some(relative_path) = dir_to_read
                .parent()
                .expect("dir to backup shouldn't be the root.")
                .strip_prefix(absolute_file_to_read.as_path())
            {
                if let Some(from_path) = SlashPath::from_path(absolute_file_to_read.as_path()) {
                    Some(Self {
                        from_path,
                        to_path: server_distinct_id.join(relative_path),
                        sha1: fmeta.sha1,
                        len: fmeta.len,
                        modified: fmeta.modified,
                        created: fmeta.created,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn changed(&self, file_path: impl AsRef<Path>) -> FileChanged {
        if let Some(fmeta) = data_shape_util::get_file_meta(file_path, self.sha1.is_none()) {
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
        let yml = r##"
to_dir: ~
from_dir: E:/ws/bk-over-ssh/fixtures/a-dir
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##;

        let mut dir = serde_yaml::from_str::<Directory>(&yml)?;
        println!("{:?}", dir);

        assert!(dir.includes_patterns.is_none());
        assert!(dir.excludes_patterns.is_none());

        dir.compile_patterns()?;

        assert!(dir.includes_patterns.is_some());
        assert!(dir.excludes_patterns.is_some());

        let files = dir
            .file_item_iter("abc", false)
            .map(|it| {
                println!("to_path: {:?}", it.to_path.slash);
                it
            })
            .collect::<Vec<FullPathFileItem>>();

        assert_eq!(files.len(), 5);
        assert!(
            files.get(0).unwrap().to_path.slash.starts_with("abc/a-dir"),
            "should starts_with fixtures/"
        );
        Ok(())
    }
}
