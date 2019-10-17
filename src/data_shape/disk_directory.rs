use super::string_path;
use glob::Pattern;
use log::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Deserialize, Serialize, Default, Debug)]
pub struct Directory {
    pub remote_dir: String,
    pub local_dir: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    #[serde(skip)]
    pub includes_patterns: Option<Vec<Pattern>>,
    #[serde(skip)]
    pub excludes_patterns: Option<Vec<Pattern>>,
}

impl Directory {
    pub fn get_remote_dir(&self) -> &str {
        self.remote_dir.as_str()
    }

    pub fn get_remote_canonicalized_dir_str(&self) -> Option<String> {
        let bp = Path::new(self.get_remote_dir()).canonicalize();
        match bp {
            Ok(base_path) => {
                if let Some(path_str) = base_path.to_str() {
                    return Some(path_str.to_owned());
                } else {
                    error!("base_path to_str failed: {:?}", base_path);
                }
            }
            Err(err) => {
                error!("load_dir resolve path failed: {:?}", err);
            }
        }
        None
    }
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
            remote_dir: remote_dir.as_ref().to_string(),
            local_dir: local_dir.as_ref().to_string(),
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
        WalkDir::new(Path::new(self.local_dir.as_str()))
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
        WalkDir::new(Path::new(self.local_dir.as_str()))
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

    /// When pushing remote, the local directory is absolute, the remote directory is relative.
    /// The remote directory is always relative to the 'directories' dir in the user's home directory.
    pub fn normalize_push_sync(
        &mut self,
        directories_dir: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let directories_dir = directories_dir.as_ref();
        trace!("origin directory: {:?}", self);
        let ld = self.local_dir.trim();
        if ld.is_empty() || ld == "~" || ld == "null" {
            let mut split = self.remote_dir.trim().rsplitn(3, &['/', '\\'][..]);
            let mut s = split.next().expect("remote_dir should has dir name.");
            if s.is_empty() {
                s = split.next().expect("remote_dir should has dir name.");
            }
            self.local_dir = s.to_string();
            trace!("local_dir is empty. change to {}", s);
        } else {
            self.local_dir = ld.to_string();
        }

        let a_local_dir = Path::new(&self.local_dir);
        if a_local_dir.is_absolute() {
            bail!(
                "the local_dir of a server can't be absolute. {:?}",
                a_local_dir
            );
        } else {
            let ld_path = directories_dir.join(&self.local_dir);
            self.local_dir = ld_path
                .to_str()
                .expect("local_dir to_str should success.")
                .to_string();

            self.local_dir = string_path::strip_verbatim_prefixed(&self.local_dir);

            if ld_path.exists() {
                fs::create_dir_all(ld_path)?;
            }
        }
        Ok(())
    }

    /// When pulling remote the remote directory is absolute path, local path is relative.
    pub fn normalize_pull_sync(
        &mut self,
        directories_dir: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let directories_dir = directories_dir.as_ref();
        trace!("origin directory: {:?}", self);
        let ld = self.local_dir.trim();
        if ld.is_empty() || ld == "~" || ld == "null" {
            let mut split = self.remote_dir.trim().rsplitn(3, &['/', '\\'][..]);
            let mut s = split.next().expect("remote_dir should has dir name.");
            if s.is_empty() {
                s = split.next().expect("remote_dir should has dir name.");
            }
            self.local_dir = s.to_string();
            trace!("local_dir is empty. change to {}", s);
        } else {
            self.local_dir = ld.to_string();
        }

        let a_local_dir = Path::new(&self.local_dir);
        if a_local_dir.is_absolute() {
            bail!(
                "the local_dir of a server can't be absolute. {:?}",
                a_local_dir
            );
        } else {
            let ld_path = directories_dir.join(&self.local_dir);
            self.local_dir = ld_path
                .to_str()
                .expect("local_dir to_str should success.")
                .to_string();

            self.local_dir = string_path::strip_verbatim_prefixed(&self.local_dir);

            if ld_path.exists() {
                fs::create_dir_all(ld_path)?;
            }
        }
        Ok(())
    }
}
