use super::{
    app_conf,
    string_path::{self, SlashPath},
    AppRole, PushPrimaryFileItem, RelativeFileItem,
};
use crate::db_accesses::{DbAccess, RelativeFileItemInDb};
use glob::Pattern;
use itertools::Itertools;
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// if has includes get includes first.
/// if has excludes exclude files.
fn match_path(
    path: PathBuf,
    includes_patterns: Option<&Vec<Pattern>>,
    excludes_patterns: Option<&Vec<Pattern>>,
) -> Option<PathBuf> {
    let has_includes = includes_patterns.is_some();
    let keep_file = if has_includes {
        includes_patterns
            .unwrap()
            .iter()
            .any(|ptn| ptn.matches_path(&path))
    } else {
        true
    };

    if !keep_file {
        return None;
    }

    let has_excludes = excludes_patterns.is_some();

    let keep_file = if has_excludes {
        !excludes_patterns
            .unwrap()
            .iter()
            .any(|p| p.matches_path(&path))
    } else {
        true
    };

    if keep_file {
        Some(path)
    } else {
        None
    }
}

#[derive(Deserialize, Serialize, Default, Debug)]
pub struct Directory {
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub remote_dir: string_path::SlashPath,
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub local_dir: string_path::SlashPath,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    #[serde(skip)]
    pub includes_patterns: Option<Vec<Pattern>>,
    #[serde(skip)]
    pub excludes_patterns: Option<Vec<Pattern>>,
}

impl Directory {
    /// for test purpose.
    /// local_dir will change to absolute when load from yml file.
    #[allow(dead_code)]
    pub fn new(
        remote_dir: impl AsRef<str>,
        local_dir: impl AsRef<str>,
        includes: Vec<impl AsRef<str>>,
        excludes: Vec<impl AsRef<str>>,
    ) -> Self {
        let mut o = Self {
            remote_dir: SlashPath::new(remote_dir),
            local_dir: SlashPath::new(local_dir),
            includes: includes.iter().map(|s| s.as_ref().to_string()).collect(),
            excludes: excludes.iter().map(|s| s.as_ref().to_string()).collect(),
            ..Directory::default()
        };
        o.compile_patterns()
            .expect("directory pattern should compile.");
        o
    }
    /// if has includes get includes first.
    /// if has excludes exclude files.
    pub fn match_path(&self, path: PathBuf) -> Option<PathBuf> {
        match_path(
            path,
            self.includes_patterns.as_ref(),
            self.excludes_patterns.as_ref(),
        )
        // let has_includes = self.includes_patterns.is_some();
        // let keep_file = if has_includes {
        //     self.includes_patterns
        //         .as_ref()
        //         .unwrap()
        //         .iter()
        //         .any(|ptn| ptn.matches_path(&path))
        // } else {
        //     true
        // };

        // if !keep_file {
        //     return None;
        // }

        // let has_excludes = self.excludes_patterns.is_some();

        // let keep_file = if has_excludes {
        //     !self
        //         .excludes_patterns
        //         .as_ref()
        //         .unwrap()
        //         .iter()
        //         .any(|p| p.matches_path(&path))
        // } else {
        //     true
        // };

        // if keep_file {
        //     Some(path)
        // } else {
        //     None
        // }
    }
    /// When includes is empty, includes_patterns will be None, excludes is the same.
    pub fn compile_patterns(&mut self) -> Result<(), failure::Error> {
        if self.includes_patterns.is_none() && !self.includes.is_empty() {
            self.includes_patterns.replace(
                self.includes
                    .iter()
                    .map(|s| Pattern::new(s).unwrap())
                    .collect(),
            );
        }

        if self.excludes_patterns.is_none() && !self.excludes.is_empty() {
            self.excludes_patterns.replace(
                self.excludes
                    .iter()
                    .map(|s| Pattern::new(s).unwrap())
                    .collect(),
            );
        }
        Ok(())
    }

    pub fn count_total_size(&self) -> u64 {
        WalkDir::new(self.local_dir.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| {
                let meta = d.metadata();
                if let Ok(meta) = meta {
                    Some(meta.len())
                } else {
                    None
                }
            })
            .sum()
    }

    #[allow(dead_code)]
    pub fn list_files_recursive(&self) -> impl Iterator<Item = (u64, String)> {
        WalkDir::new(self.local_dir.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|d| d.file_type().is_file())
            .filter_map(|d| {
                let meta = d.metadata();
                let file_name = d.file_name().to_str().unwrap_or_else(|| "").to_string();
                if let Ok(meta) = meta {
                    Some((meta.len(), file_name))
                } else {
                    None
                }
            })
    }

    /// When in active leaf mode, the local directory is absolute, the remote directory is relative.
    /// How to find the remote data directory?
    ///
    pub fn normalize_active_leaf_sync(
        &mut self,
        _directories_dir: impl AsRef<Path>,
        app_instance_id: &str,
        remote_exec: &str,
    ) -> Result<(), failure::Error> {
        trace!("origin directory: {:?}", self);
        if self.local_dir.is_empty() {
            bail!("when in push mode, local_dir cannot be empty.");
        }

        if !self.local_dir.exists() {
            bail!("local_dir does not exist: {}", &self.local_dir);
        }

        trace!("origin directory: {:?}", self);

        if self.remote_dir.is_empty() {
            self.remote_dir.set_slash(self.local_dir.get_last_name());
        }

        let remote_path = SlashPath::new(remote_exec)
            .parent()
            .expect("remote_exec parent should exist")
            .join("data")
            .join(app_conf::RECEIVE_SERVERS_DATA)
            .join(app_instance_id)
            .join("directories")
            .join(self.remote_dir.get_slash());
        self.remote_dir = remote_path;
        Ok(())
    }

    /// When in receive hub mode, the local directory is absolute, the remote directory is relative.
    /// The remote directory is always relative to the 'directories' dir in the user's home directory.
    pub fn normalize_receive_hub_sync(
        &mut self,
        directories_dir: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let directories_dir = directories_dir.as_ref();
        trace!("origin directory: {:?}", self);

        if self.local_dir.is_empty() {
            bail!("when in push mode, local_dir cannot be empty.");
        }

        if self.remote_dir.is_empty() {
            self.remote_dir.set_slash(self.local_dir.get_last_name());
        }

        let remote_path = SlashPath::from_path(directories_dir)
            .expect("normalize_receive_hub_sync directories_dir to_str should succeed.")
            .join(self.remote_dir.get_slash());
        self.remote_dir = remote_path;
        Ok(())
    }

    /// When pulling remote the remote directory is absolute path, local path is relative.
    /// This method is for normalize local directory ready for coping.
    pub fn normalize_pull_hub_sync(
        &mut self,
        directories_dir: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let directories_dir = directories_dir.as_ref();
        trace!("origin directory: {:?}", self);

        if self.local_dir.is_empty() {
            self.local_dir.set_slash(self.remote_dir.get_last_name());
        }

        if self.local_dir.as_path().is_absolute() {
            bail!(
                "the local_dir of a server can't be absolute. {:?}",
                self.local_dir
            );
        }

        self.local_dir = SlashPath::from_path(directories_dir)
            .expect("normalize_pull_hub_sync directories_dir to_str should succeed.")
            .join_another(&self.local_dir);

        if !self.local_dir.exists() {
            self.local_dir.create_dir_all()?;
        }
        Ok(())
    }

    /// The method read information of files from disk file.
    /// but from which directory to read on? local_dir or remote_dir?
    /// It depends on the role of the running application.
    /// when as AppRole::PassiveLeaf use remote_dir.
    /// when as AppRole::ActiveLeaf use local_dir.
    pub fn load_relative_item<O>(
        &self,
        app_role: Option<&AppRole>,
        out: &mut O,
        skip_sha1: bool,
    ) -> Result<(), failure::Error>
    where
        O: io::Write,
    {
        trace!("load_relative_item, skip_sha1: {}", skip_sha1);
        let dir_to_read = if let Some(app_role) = app_role {
            match app_role {
                AppRole::PassiveLeaf => &self.remote_dir,
                AppRole::ActiveLeaf => &self.local_dir,
                _ => bail!(
                    "when invoking load_relative_item, got unsupported app role. {:?}",
                    app_role
                ),
            }
        } else {
            bail!("no app_role when load_relative_item");
        };
        self.relative_item_iter(dir_to_read.clone(), skip_sha1)
            .for_each(|rfi| match serde_json::to_string(&rfi) {
                Ok(line) => {
                    if let Err(err) = writeln!(out, "{}", line) {
                        error!("write item line failed: {:?}, {:?}", err, line);
                    }
                }
                Err(err) => {
                    error!("serialize item line failed: {:?}", err);
                }
            });
        Ok(())
    }

    pub fn relative_item_iter(
        &self,
        dir_to_read: SlashPath,
        skip_sha1: bool,
    ) -> impl Iterator<Item = RelativeFileItem> + '_ {
        let includes_patterns = self.includes_patterns.clone();
        let excludes_patterns = self.excludes_patterns.clone();

        WalkDir::new(dir_to_read.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|dir_entry| dir_entry.file_type().is_file())
            .filter_map(|dir_entry| dir_entry.path().canonicalize().ok())
            .filter_map(move |disk_file_path_buf| {
                match_path(
                    disk_file_path_buf,
                    includes_patterns.as_ref(),
                    excludes_patterns.as_ref(),
                )
            })
            .filter_map(move |absolute_path_buf| {
                RelativeFileItem::from_path(&dir_to_read, absolute_path_buf, skip_sha1)
            })
    }

    pub fn push_file_item_iter(
        &self,
        app_instance_id: impl AsRef<str>,
        dir_to_read: &SlashPath,
        skip_sha1: bool,
    ) -> impl Iterator<Item = PushPrimaryFileItem> + '_ {
        let includes_patterns = self.includes_patterns.clone();
        let excludes_patterns = self.excludes_patterns.clone();
        let app_instance_id = app_instance_id.as_ref().to_string();
        let dir_to_read = dir_to_read.clone();

        WalkDir::new(dir_to_read.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|dir_entry| dir_entry.file_type().is_file())
            .filter_map(|dir_entry| dir_entry.path().canonicalize().ok())
            .filter_map(move |disk_file_path_buf| {
                match_path(
                    disk_file_path_buf,
                    includes_patterns.as_ref(),
                    excludes_patterns.as_ref(),
                )
            })
            .filter_map(move |absolute_path_buf| {
                PushPrimaryFileItem::from_path(
                    &dir_to_read,
                    absolute_path_buf,
                    &app_instance_id,
                    skip_sha1,
                )
            })
    }

    /// get all leaf directories under this directory.
    #[allow(dead_code)]
    pub fn get_sub_directory_names(&self) -> Vec<String> {
        vec![]
    }

    pub fn count_local_files(&self, app_role: Option<&AppRole>) -> Result<u64, failure::Error> {
        let dir_to_read = if let Some(app_role) = app_role {
            match app_role {
                AppRole::ReceiveHub => &self.remote_dir,
                _ => bail!(
                    "when invoking load_relative_item_to_sqlite, got unsupported app role. {:?}",
                    app_role
                ),
            }
        } else {
            return Ok(0);
        };
        let file_num = WalkDir::new(dir_to_read.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|dir_entry| dir_entry.ok())
            .filter(|dir_entry| dir_entry.file_type().is_file())
            .count();
        Ok(file_num as u64)
    }

    pub fn count_local_dir_files(&self) -> u64 {
        let includes_patterns = self.includes_patterns.clone();
        let excludes_patterns = self.excludes_patterns.clone();

        WalkDir::new(self.local_dir.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(|dir_entry| dir_entry.ok())
            .filter(|dir_entry| dir_entry.file_type().is_file())
            .map(|dir_entry| dir_entry.path().to_path_buf())
            .filter_map(move |disk_file_path_buf| {
                match_path(
                    disk_file_path_buf,
                    includes_patterns.as_ref(),
                    excludes_patterns.as_ref(),
                )
            })
            .count() as u64
    }

    /// this function will walk over the directory, for every file checking it's metadata and compare to corepsonding item in the db.
    /// for new and changed items mark changed field to true.
    /// for unchanged items, if the status in db is changed chang to unchanged.
    /// So after invoking this method all changed item will be marked, at the same time, metadata of items were updated too, this means you cannot regenerate the same result if the task is interupted.
    /// To avoid this kind of situation, add a confirm field to the table. when the taks is done, we chang the confirm field to true.
    /// Now we get the previous result by select the unconfirmed items.
    /// This iteration doesn't pick out deleted items!!!
    pub fn load_relative_item_to_sqlite<M, D>(
        &self,
        app_role: Option<&AppRole>,
        db_access: &D,
        skip_sha1: bool,
        sql_batch_size: usize,
        sig_ext: &str,
        delta_ext: &str,
    ) -> Result<(), failure::Error>
    where
        M: r2d2::ManageConnection,
        D: DbAccess<M>,
    {
        trace!(
            "load_relative_item_to_sqlite, skip_sha1: {}, sql_batch_size: {}",
            skip_sha1,
            sql_batch_size
        );

        let dir_to_read = if let Some(app_role) = app_role {
            match app_role {
                AppRole::PassiveLeaf | AppRole::ReceiveHub => &self.remote_dir,
                AppRole::ActiveLeaf => &self.local_dir,
                _ => bail!(
                    "when invoking load_relative_item_to_sqlite, got unsupported app role. {:?}",
                    app_role
                ),
            }
        } else {
            bail!("no app_role when load_relative_item_to_sqlite");
        };

        let base_path = dir_to_read.as_str();
        let dir_id = db_access.insert_directory(base_path)?;

        if sql_batch_size > 1 {
            WalkDir::new(&base_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_file())
                .filter_map(|d| d.path().canonicalize().ok())
                .filter_map(|d| self.match_path(d))
                .filter_map(|d| RelativeFileItemInDb::from_path(dir_to_read, d, skip_sha1, dir_id))
                .filter(|rfi| !(rfi.path.ends_with(sig_ext) || rfi.path.ends_with(delta_ext)))
                .filter_map(|rfi| db_access.insert_or_update_relative_file_item(rfi, true))
                .map(|(rfi, da)| rfi.to_sql_string(&da))
                .chunks(sql_batch_size)
                .into_iter()
                .for_each(|ck| {
                    trace!("start batch insert.");
                    db_access.execute_batch(ck);
                    trace!("end batch insert.");
                });
        } else {
            let _c = WalkDir::new(&base_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|d| d.file_type().is_file())
                .filter_map(|d| d.path().canonicalize().ok())
                .filter_map(|d| self.match_path(d))
                .filter_map(|d| RelativeFileItemInDb::from_path(dir_to_read, d, skip_sha1, dir_id))
                .filter_map(|rfi| db_access.insert_or_update_relative_file_item(rfi, false))
                .count();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    #[test]
    fn t_directory_i() -> Result<(), failure::Error> {
        let yml = r##"
remote_dir: F:/github/bk-over-ssh/fixtures/adir
local_dir: ~
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##;

        let d = serde_yaml::from_str::<Directory>(&yml)?;
        println!("{:?}", d);
        let content = serde_yaml::to_string(&d)?;
        let cur = std::io::Cursor::new(content);
        for line in cur.lines() {
            println!("{:?}", line?);
        }
        Ok(())
    }
}
