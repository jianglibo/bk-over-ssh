use super::string_path;
use super::SlashPath;
use crate::actions::hash_file_sha1;
use crate::data_shape::data_shape_util;
use crate::protocol::TransferType;
use log::*;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug)]
pub enum FileChanged {
    Len(u64, u64),
    Modified(Option<u64>, Option<u64>),
    Sha1(Option<String>, Option<String>),
    NoChange,
}

/// Like a disk directory, but it contains PushPrimaryFileItem.
/// No local_dir and remote_dir, but absolute local_path and remote_path.
#[derive(Deserialize, Serialize, Debug)]
pub struct PushPrimaryFileItem {
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub local_path: SlashPath,
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub remote_path: SlashPath,
    pub sha1: Option<String>,
    pub len: u64,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

impl PushPrimaryFileItem {
    pub fn from_path(
        base_path: &SlashPath,
        absolute_path_buf: PathBuf,
        app_instance_id: impl AsRef<str>,
        skip_sha1: bool,
    ) -> Option<Self> {
        let app_instance_id = SlashPath::new(app_instance_id);

        let metadata_r = absolute_path_buf.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 {
                    hash_file_sha1(&absolute_path_buf)
                } else {
                    Option::<String>::None
                };

                if let Some(relative_path) = base_path
                    .parent()
                    .expect("dir to backup shouldn't be the root.")
                    .strip_prefix(absolute_path_buf.as_path())
                {
                    Some(Self {
                        local_path: SlashPath::from_path(absolute_path_buf.as_path())
                            .expect("slashpath from absolute file path"),
                        // remote path is determined by the app_instance_id, base_path's name and the relative path.
                        // the relative path already include base_path's name.
                        remote_path: app_instance_id.join(relative_path),
                        sha1,
                        len: metadata.len(),
                        modified: metadata
                            .modified()
                            .ok()
                            .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs()),
                        created: metadata
                            .created()
                            .ok()
                            .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs()),
                    })
                } else {
                    None
                }
            }
            Err(err) => {
                error!(
                    "PushPrimaryFileItem get_meta failed: {:?}, {:?}",
                    absolute_path_buf, err
                );
                None
            }
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
            FileChanged::NoChange
        }
    }

    pub fn as_sent_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.insert(0, TransferType::FileItem.to_u8());
        let json_str =
            serde_json::to_string(&self).expect("PushPrimaryFileItem to serialize to string.");
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
remote_dir: ~
local_dir: E:/ws/bk-over-ssh/fixtures/a-dir
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##;

        let mut d = serde_yaml::from_str::<Directory>(&yml)?;
        println!("{:?}", d);

        assert!(d.includes_patterns.is_none());
        assert!(d.excludes_patterns.is_none());

        d.compile_patterns()?;

        assert!(d.includes_patterns.is_some());
        assert!(d.excludes_patterns.is_some());

        let files = d
            .push_file_item_iter("abc", &d.local_dir, false)
            .map(|it| {
                println!("remote_path: {:?}", it.remote_path.slash);
                it
            })
            .collect::<Vec<PushPrimaryFileItem>>();

        assert_eq!(files.len(), 5);
        assert!(
            files
                .get(0)
                .unwrap()
                .remote_path
                .slash
                .starts_with("abc/a-dir"),
            "should starts_with fixtures/"
        );

        Ok(())
    }
}
