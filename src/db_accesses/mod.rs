// placeholders
pub mod scheduler_util;
pub mod sqlite_access;

use crate::actions::hash_file_sha1;
use chrono::{DateTime, SecondsFormat, Utc};
use log::*;
use r2d2;
use std::path::{Path, PathBuf};

// lazy_static! {
//     static ref EMPTY_STRING: String = String::from("");
// }

#[derive(Debug)]
pub enum DbAction {
    Insert,
    Update,
    UpdateChangedField,
}

pub use sqlite_access::SqliteDbAccess;

#[derive(Default, Debug, Clone)]
pub struct CountItemParam {
    changed: Option<bool>,
    confirmed: Option<bool>,
    is_and: u8,
}

#[allow(dead_code)]
impl CountItemParam {
    pub fn get_changed(&self) -> Option<bool> {
        self.changed
    }

    pub fn get_confirmed(&self) -> Option<bool> {
        self.confirmed
    }

    pub fn is_and(&self) -> bool {
        self.is_and == 1
    }
    pub fn changed(&mut self, changed: bool) -> Self {
        self.changed = Some(changed);
        self.clone()
    }

    pub fn confirmed(&mut self, confirmed: bool) -> Self {
        self.confirmed = Some(confirmed);
        self.clone()
    }

    pub fn and(&mut self) -> Self {
        self.is_and = 1;
        self.clone()
    }
    pub fn or(&mut self) -> Self {
        self.is_and = 0;
        self.clone()
    }
}


#[derive(Debug, Default)]
pub struct RemoteFileItemInDb {
    pub id: i64,
    pub path: String,
    pub sha1: Option<String>,
    pub len: i64,
    pub modified: Option<DateTime<Utc>>,
    pub created: Option<DateTime<Utc>>,
    pub dir_id: i64,
    pub changed: bool,
    pub confirmed: bool,
}

impl RemoteFileItemInDb {
    #[allow(dead_code)]
    pub fn duplicate_self(&self) -> RemoteFileItemInDb {
        RemoteFileItemInDb {
            id: self.id,
            path: self.path.clone(),
            sha1: self.sha1.clone(),
            len: self.len,
            modified: self.modified,
            created: self.created,
            dir_id: self.dir_id,
            changed: self.changed,
            confirmed: self.confirmed,
        }
    }

    pub fn from_path(
        base_path: impl AsRef<Path>,
        path: PathBuf,
        skip_sha1: bool,
        dir_id: i64,
    ) -> Option<Self> {
        let metadata_r = path.metadata();
        match metadata_r {
            Ok(metadata) => {
                let sha1 = if !skip_sha1 {
                    hash_file_sha1(&path)
                } else {
                    Option::<String>::None
                };
                let relative_o = path.strip_prefix(&base_path).ok().and_then(|p| p.to_str());
                if let Some(relative) = relative_o {
                    return Some(Self {
                        id: 0,
                        path: relative.to_string(),
                        sha1,
                        len: metadata.len() as i64,
                        modified: metadata.modified().ok().map(Into::into),
                        created: metadata.created().ok().map(Into::into),
                        dir_id,
                        changed: false,
                        confirmed: false,
                    });
                } else {
                    error!("RemoteFileItem path name to_str() failed. {:?}", path);
                }
            }
            Err(err) => {
                error!("RemoteFileItem metadata failed: {:?}, {:?}", path, err);
            }
        }
        None
    }

    pub fn to_insert_sql_string(&self) -> String {
        // path, sha1, len, time_modified, time_created, dir_id, changed
        format!("INSERT INTO remote_file_item (path, {}len, {}{}dir_id, changed, confirmed) VALUES ('{}', {}{}, {}{}{}, {}, {});"
        , self.sha1.as_ref().map(|_|"sha1, ").unwrap_or("")
        , self.modified.map(|_|"time_modified, ").unwrap_or("")
        , self.created.map(|_|"time_created, ").unwrap_or("")
        , self.path
        , self.sha1.as_ref().map(|s|format!("'{}', ", s)).unwrap_or_else(|| "".to_string())
        , self.len
        , self.modified.map(|m|format!("'{}', ", m.to_rfc3339_opts(SecondsFormat::Nanos, true))).unwrap_or_else(|| "".to_string())
        , self.created.map(|m|format!("'{}', ", m.to_rfc3339_opts(SecondsFormat::Nanos, true))).unwrap_or_else(|| "".to_string())
        , self.dir_id
        , 1
        , 0
        )
    }

    pub fn to_update_sql_string(&self) -> String {
        format!(
            "UPDATE remote_file_item SET len = {}, {}{}changed = 1, confirmed = 0 where id = {};",
            self.len,
            self.sha1
                .as_ref()
                .map(|s| format!("sha1 = '{}', ", s))
                .unwrap_or_else(|| "".to_string()),
            self.modified
                .map(|m| format!(
                    "time_modified = '{}', ",
                    m.to_rfc3339_opts(SecondsFormat::Nanos, true)
                ))
                .unwrap_or_else(|| "".to_string()),
            self.id
        )
    }

    pub fn to_update_changed_sql_string(&self) -> String {
        format!(
            "UPDATE remote_file_item SET changed = {}, confirmed = 0 where id = {};",
            if self.changed { 1 } else { 0 },
            self.id
        )
    }

    pub fn to_sql_string(&self, da: &DbAction) -> String {
        match da {
            DbAction::Insert => self.to_insert_sql_string(),
            DbAction::UpdateChangedField => self.to_update_changed_sql_string(),
            DbAction::Update => self.to_update_sql_string(),
        }
    }
}

pub trait DbAccess<M>: Clone + 'static
where
    M: r2d2::ManageConnection,
{
    fn insert_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error>;
    fn insert_or_update_remote_file_item(
        &self,
        rfi: RemoteFileItemInDb,
        batch: bool,
    ) -> Option<(RemoteFileItemInDb, DbAction)>;
    fn get_file_item(&self, num: usize) -> Result<Vec<RemoteFileItemInDb>, failure::Error>;
    fn find_remote_file_item(
        &self,
        dir_id: i64,
        path: impl AsRef<str>,
    ) -> Result<RemoteFileItemInDb, failure::Error>;
    fn create_database(&self) -> Result<(), failure::Error>;
    fn find_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error>;
    fn count_directory(&self) -> Result<u64, failure::Error>;
    fn count_remote_file_item(&self, cc: CountItemParam) -> Result<u64, failure::Error>;
    fn iterate_all_file_items<P>(&self, processor: P) -> Result<(usize, usize), failure::Error>
    where
        P: Fn(RemoteFileItemInDb) -> ();
    fn iterate_files_by_directory<F>(&self, processor: F) -> Result<(), failure::Error>
    where
        F: FnMut((Option<RemoteFileItemInDb>, Option<String>)) -> ();

    fn iterate_files_by_directory_changed_or_unconfirmed<F>(&self, processor: F) -> Result<(), failure::Error>
    where
        F: FnMut((Option<RemoteFileItemInDb>, Option<String>)) -> ();

    fn find_next_execute(
        &self,
        server_yml_path: impl AsRef<str>,
        task_name: impl AsRef<str>,
    ) -> Option<(i64, DateTime<Utc>, bool)>;
    fn insert_next_execute(
        &self,
        server_yml_path: impl AsRef<str>,
        task_name: impl AsRef<str>,
        time_execution: DateTime<Utc>,
    );
    fn update_next_execute_done(&self, id: i64) -> Result<(), failure::Error>;
    fn delete_next_execute(&self, id: i64) -> Result<(), failure::Error>;
    fn count_next_execute(&self) -> Result<u64, failure::Error>;

    fn execute_batch(&self, sit: impl Iterator<Item = String>);

    fn confirm_all(&self) -> Result<u64, failure::Error>;

    fn exclude_by_sql(&self, select_id_sql: impl AsRef<str>) -> Result<u64, failure::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_count_item_param() {
        let p = CountItemParam::default().changed(true).and().confirmed(false);

        assert_eq!(p.get_changed(), Some(true));
        assert_eq!(p.get_confirmed(), Some(false));
    }

}