pub mod app_conf;
pub mod count_reader;
pub mod disk_directory;
pub mod file_item_map;
pub mod file_item_directory;
pub mod indicator;
pub mod relative_file_item;
pub mod rolling_files;
pub mod server;
pub mod sha1_reader;
pub mod string_path;
pub mod writer_with_progress;
pub mod full_path_item;
pub mod data_shape_util;
pub mod client_push_pb;

pub use data_shape_util::{get_file_meta};

pub use client_push_pb::{ClientPushProgressBar};

pub use app_conf::{
    demo_app_conf, AppConf, AppRole, MailConf, MiniAppConf, ReadAppConfException, CONF_FILE_NAME,
};
pub use count_reader::CountReader;
pub use disk_directory::Directory;
pub use file_item_map::{FileItemMap, FileItemProcessResult, FileItemProcessResultStats, SyncType};
pub use file_item_directory::{FileItemDirectory, FileItemDirectories, PrimaryFileItem};
pub use indicator::{Indicator, PbProperties};
pub use relative_file_item::{RelativeFileItem};
pub use full_path_item::{FullPathFileItem, FileChanged};
pub use server::{Server, ServerYml};
pub use sha1_reader::Sha1Reader;
pub use string_path::SlashPath;
pub use writer_with_progress::ProgressWriter;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ScheduleItem {
    pub name: String,
    pub cron: String,
}

#[derive(Builder, Deserialize, Serialize, Debug)]
#[builder(setter(into))]
pub struct PruneStrategy {
    #[builder(default = "1")]
    pub yearly: u8,
    #[builder(default = "1")]
    pub monthly: u8,
    #[builder(default = "0")]
    pub weekly: u8,
    #[builder(default = "1")]
    pub daily: u8,
    #[builder(default = "1")]
    pub hourly: u8,
    #[builder(default = "1")]
    pub minutely: u8,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum AuthMethod {
    Password,
    Agent,
    IdentityFile,
}
