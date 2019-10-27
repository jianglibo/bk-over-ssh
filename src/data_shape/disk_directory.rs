use super::string_path::{self, SlashPath};
use glob::Pattern;
use log::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
        let has_includes = self.includes_patterns.is_some();
        let keep_file = if has_includes {
            self.includes_patterns
                .as_ref()
                .unwrap()
                .iter()
                .any(|ptn| ptn.matches_path(&path))
        } else {
            true
        };

        if !keep_file {
            return None;
        }

        let has_excludes = self.excludes_patterns.is_some();

        let keep_file = if has_excludes {
            !self
                .excludes_patterns
                .as_ref()
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
    /// The remote directory is always relative to the 'directories' dir in the user's home directory.
    pub fn normalize_active_leaf_sync(
        &mut self,
        _directories_dir: impl AsRef<Path>,
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

        let remote_path = SlashPath::new("./directories").join(self.remote_dir.get_slash());
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

        self.local_dir = SlashPath::from_path(directories_dir).join_another(&self.local_dir);

        if !self.local_dir.exists() {
            self.local_dir.create_dir_all()?;
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
