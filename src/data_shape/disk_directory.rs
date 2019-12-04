use super::{
    app_conf,
    string_path::{self, SlashPath},
    AppRole, FullPathFileItem, RelativeFileItem,
};
use crate::db_accesses::{DbAccess, RelativeFileItemInDb};
use glob::Pattern;
use itertools::Itertools;
use log::*;
use r2d2;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

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

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub enum FileSelector {
    Latest(usize),
    LatestWithPattern(usize, String),
    All,
}

#[derive(Deserialize, Serialize, Default, Debug)]
pub struct Directory {
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub to_dir: string_path::SlashPath,
    #[serde(deserialize_with = "string_path::deserialize_slash_path_from_str")]
    pub from_dir: string_path::SlashPath,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    pub file_selector: Option<FileSelector>,
    #[serde(skip)]
    pub includes_patterns: Option<Vec<Pattern>>,
    #[serde(skip)]
    pub excludes_patterns: Option<Vec<Pattern>>,
}

impl Directory {
    /// for test purpose.
    /// from_dir will change to absolute when load from yml file.
    #[allow(dead_code)]
    pub fn new(
        to_dir: impl AsRef<str>,
        from_dir: impl AsRef<str>,
        includes: Vec<impl AsRef<str>>,
        excludes: Vec<impl AsRef<str>>,
    ) -> Self {
        let mut o = Self {
            to_dir: SlashPath::new(to_dir),
            from_dir: SlashPath::new(from_dir),
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
    }
    /// When includes is empty, includes_patterns will be None, excludes is the same.
    pub fn compile_patterns(&mut self) -> Result<(), failure::Error> {
        self.from_dir.sanitize();
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
        WalkDir::new(self.from_dir.as_path())
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
        WalkDir::new(self.from_dir.as_path())
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
        if self.from_dir.is_empty() {
            bail!("when in push mode, from_dir cannot be empty.");
        }

        if !self.from_dir.exists() {
            bail!("from_dir does not exist: {}", &self.from_dir);
        }

        trace!("origin directory: {:?}", self);

        if self.to_dir.is_empty() {
            self.to_dir.set_slash(self.from_dir.get_last_name());
        }

        let remote_path = SlashPath::new(remote_exec)
            .parent()
            .expect("remote_exec parent should exist")
            .join("data")
            .join(app_conf::RECEIVE_SERVERS_DATA)
            .join(app_instance_id)
            .join("directories")
            .join(self.to_dir.get_slash());
        self.to_dir = remote_path;
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

        if self.from_dir.is_empty() || !self.from_dir.as_path().is_absolute() {
            bail!("from_dir is always absolute and existing.");
        }

        if self.to_dir.is_empty() {
            self.to_dir.set_slash(self.from_dir.get_last_name());
        }

        let to_path = SlashPath::from_path(directories_dir)
            .expect("directories_dir to_str should succeed.")
            .join(self.to_dir.get_slash());
        self.to_dir = to_path;
        Ok(())
    }

    /// When pulling remote the remote directory is absolute path, local path is relative.
    /// This method is for normalize local directory ready for coping.
    // pub fn normalize_pull_hub_sync(
    //     &mut self,
    //     directories_dir: impl AsRef<Path>,
    // ) -> Result<(), failure::Error> {
    //     let directories_dir = directories_dir.as_ref();
    //     trace!("origin directory: {:?}", self);

    //     if self.from_dir.is_empty() || !self.from_dir.as_path().is_absolute() {
    //         bail!("from_dir is always absolute and existing.");
    //     }
    //     Ok(())
    // }

    /// The method read information of files from disk file.
    /// but from which directory to read on? from_dir or to_dir?
    /// It depends on the role of the running application.
    /// when as AppRole::PassiveLeaf use to_dir.
    /// when as AppRole::ActiveLeaf use from_dir.
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
                AppRole::PassiveLeaf => &self.to_dir,
                AppRole::ActiveLeaf => &self.from_dir,
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

    fn file_item_iter_file_selector(
        &self,
        server_distinct_id: String,
        dir_to_read: SlashPath,
        skip_sha1: bool,
        file_selector: &FileSelector,
    ) -> impl Iterator<Item = FullPathFileItem> + '_ {
        match file_selector {
            FileSelector::Latest(num) => {
                return WalkDir::new(dir_to_read.as_path())
                    .min_depth(1)
                    .max_depth(1)
                    .sort_by(|a, b| {
                        if let (Ok(a_meta), Ok(b_meta)) = (a.metadata(), b.metadata()) {
                            if let (Ok(a_time), Ok(b_time)) = (a_meta.modified(), b_meta.modified())
                            {
                                // return a_time.cmp(&b_time);
                                return b_time.cmp(&a_time);
                            }
                        }
                        Ordering::Equal
                    })
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| e.file_type().is_file())
                    .filter_map(|e| e.ok())
                    .filter_map(|dir_entry| dir_entry.path().canonicalize().ok())
                    .filter_map(move |absolute_file_path| {
                        FullPathFileItem::create_item_from_path(
                            &dir_to_read,
                            absolute_file_path,
                            &server_distinct_id,
                            skip_sha1,
                        )
                    })
                    .take(*num);
            }
            FileSelector::LatestWithPattern(num, ptn) => {
                panic!(
                    "unimplement FileSelector::LatestWithPattern({}, {})",
                    num, ptn
                );
            }
            FileSelector::All => {
                panic!("unimplement FileSelector::All");
            }
        }
    }

    fn file_item_iter_no_file_selector(
        &self,
        server_distinct_id: String,
        dir_to_read: SlashPath,
        skip_sha1: bool,
    ) -> impl Iterator<Item = FullPathFileItem> + '_ {
        let includes_patterns = self.includes_patterns.clone();
        let excludes_patterns = self.excludes_patterns.clone();
        let excludes = self
            .excludes
            .iter()
            .map(SlashPath::new)
            .collect::<Vec<SlashPath>>();

        let not_exact_exclude = move |de: &DirEntry| -> bool {
            !if de.file_type().is_dir() {
                if let Some(path) = de.path().to_str() {
                    let sl = SlashPath::new(path);
                    excludes.iter().find(|&p| p == &sl).is_some()
                } else {
                    false // keep it
                }
            } else {
                false // keep it
            }
        };

        WalkDir::new(dir_to_read.as_path())
            .follow_links(false)
            .into_iter()
            .filter_entry(not_exact_exclude)
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
            .filter_map(move |absolute_file_path| {
                FullPathFileItem::create_item_from_path(
                    &dir_to_read,
                    absolute_file_path,
                    &server_distinct_id,
                    skip_sha1,
                )
            })
    }

    /// When push to remote server the server_distinct_id is app_instance_id,
    /// When be pulled the server_distinct_id is unnecessary.
    pub fn file_item_iter(
        &self,
        server_distinct_id: impl AsRef<str>,
        // dir_to_read: &SlashPath,
        skip_sha1: bool,
    ) -> Box<dyn Iterator<Item = FullPathFileItem> + '_> {
        let server_distinct_id = server_distinct_id.as_ref().to_string();
        let dir_to_read = self.from_dir.clone();

        if let Some(file_selector) = self.file_selector.as_ref() {
            trace!("find file_selector");
            Box::new(self.file_item_iter_file_selector(
                server_distinct_id,
                dir_to_read,
                skip_sha1,
                file_selector,
            ))
        } else {
            Box::new(self.file_item_iter_no_file_selector(
                server_distinct_id,
                dir_to_read,
                skip_sha1,
            ))
        }
    }

    /// get all leaf directories under this directory.
    #[allow(dead_code)]
    pub fn get_sub_directory_names(&self) -> Vec<String> {
        vec![]
    }

    pub fn count_local_files(&self, app_role: Option<&AppRole>) -> Result<u64, failure::Error> {
        let dir_to_read = if let Some(app_role) = app_role {
            match app_role {
                AppRole::ReceiveHub => &self.to_dir,
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

    pub fn count_from_dir_files(&self) -> u64 {
        let includes_patterns = self.includes_patterns.clone();
        let excludes_patterns = self.excludes_patterns.clone();

        WalkDir::new(self.from_dir.as_path())
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
                AppRole::PassiveLeaf | AppRole::ReceiveHub => &self.to_dir,
                AppRole::ActiveLeaf => &self.from_dir,
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
    use crate::develope::tutil;
    use std::io::BufRead;
    use std::time::Duration;

    #[test]
    fn t_directory_i() -> Result<(), failure::Error> {
        let yml = r##"
to_dir: F:/github/bk-over-ssh/fixtures/adir
from_dir: ~
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

    #[test]
    fn t_directory_file_selector() -> Result<(), failure::Error> {
        let tdir = tutil::TestDir::new();
        let _a1 = tdir.make_a_file_with_content("a1", "abc")?;
        let _a2 = tdir.make_a_file_with_content("a_2", "abc")?;
        let _a3 = tdir.make_a_file_with_content("xx", "abc")?;

        let d1 = tdir.tmp_dir_path().join("kkc");
        std::fs::create_dir(d1.as_path())?;
        tutil::make_a_file_with_content(d1.as_path(), "ccc", "cccc")?;

        let d1_path = SlashPath::from_path(d1.as_path()).expect("from path.");
        println!("ccc dir: {}", d1_path);

        let yml = format!(
            r##"
to_dir: ~
from_dir: {}
includes: []
excludes:
  - {}
  - "*.log"
  - "*.bak"
"##,
            SlashPath::new(tdir.tmp_dir_str()),
            d1_path
        );

        let mut d = serde_yaml::from_str::<Directory>(&yml)?;
        d.compile_patterns()?;
        let files = d
            .file_item_iter("abc", false)
            .collect::<Vec<FullPathFileItem>>();
        assert_eq!(files.len(), 3);

        let tdir = tutil::TestDir::new();
        let _a1 = tdir.make_a_file_with_content("a1", "abc")?;
        std::thread::sleep(Duration::from_secs(2));
        let _a2 = tdir.make_a_file_with_content("a_2", "abc")?;
        std::thread::sleep(Duration::from_secs(2));
        let _a3 = tdir.make_a_file_with_content("xx", "abc")?;

        let yml = format!(
            r##"
to_dir: ~
from_dir: {}
file_selector:
  Latest: 2
includes:
  - "*.txt"
  - "*.png"
excludes:
  - "*.log"
  - "*.bak"
"##,
            SlashPath::new(tdir.tmp_dir_str())
        );

        let mut d = serde_yaml::from_str::<Directory>(&yml)?;
        d.compile_patterns()?;
        let files = d
            .file_item_iter("abc", false)
            .collect::<Vec<FullPathFileItem>>();
        assert_eq!(files.len(), 2);
        let a1 = &files.get(0).as_ref().unwrap().to_path;
        let a2 = &files.get(1).as_ref().unwrap().to_path;

        eprintln!("{}", a1);
        eprintln!("{}", a2);

        assert!(a1.slash.ends_with("xx"));
        assert!(a2.slash.ends_with("a_2"));

        Ok(())
    }

    #[derive(Deserialize, Serialize, Debug)]
    struct FileSelectorContainer {
        file_selector: Option<FileSelector>,
    }

    #[test]
    fn t_file_selector_enum() -> Result<(), failure::Error> {
        let file_selector = FileSelector::Latest(3);
        let s = serde_yaml::to_string(&file_selector)?;
        eprintln!("{}", s);
        let file_selector_1 = serde_yaml::from_str::<FileSelector>(&s)?;
        assert_eq!(file_selector, file_selector_1);

        let file_selector = FileSelector::LatestWithPattern(3, "abc".to_owned());
        let s = serde_yaml::to_string(&file_selector)?;
        eprintln!("{}", s);
        let file_selector_1 = serde_yaml::from_str::<FileSelector>(&s)?;
        assert_eq!(file_selector, file_selector_1);

        let file_selector = FileSelector::All;
        let s = serde_yaml::to_string(&file_selector)?;
        eprintln!("{}", s);
        let file_selector_1 = serde_yaml::from_str::<FileSelector>(&s)?;
        assert_eq!(file_selector, file_selector_1);

        let fc = FileSelectorContainer {
            file_selector: Some(FileSelector::All),
        };
        let s = serde_yaml::to_string(&fc)?;
        eprintln!("{}", s);
        let fc1 = serde_yaml::from_str::<FileSelectorContainer>(&s)?;
        assert_eq!(fc.file_selector, fc1.file_selector);

        let fc = FileSelectorContainer {
            file_selector: Some(FileSelector::Latest(3)),
        };
        let s = serde_yaml::to_string(&fc)?;
        eprintln!("{}", s);
        let fc1 = serde_yaml::from_str::<FileSelectorContainer>(&s)?;
        assert_eq!(fc.file_selector, fc1.file_selector);

        let fc = FileSelectorContainer {
            file_selector: Some(FileSelector::LatestWithPattern(3, "abc".to_owned())),
        };
        let s = serde_yaml::to_string(&fc)?;
        eprintln!("{}", s);
        let fc1 = serde_yaml::from_str::<FileSelectorContainer>(&s)?;
        assert_eq!(fc.file_selector, fc1.file_selector);

        let fc = FileSelectorContainer {
            file_selector: None,
        };
        let s = serde_yaml::to_string(&fc)?;
        eprintln!("{}", s);
        let fc1 = serde_yaml::from_str::<FileSelectorContainer>(&s)?;
        assert_eq!(fc.file_selector, fc1.file_selector);

        // ---
        // All
        // ---
        // file_selector: All
        // ---
        // file_selector:
        //   Latest: 3
        // ---
        // file_selector:
        //   LatestWithPattern:
        //     - 3
        //     - abc
        // ---
        // file_selector: ~
        Ok(())
    }
}
