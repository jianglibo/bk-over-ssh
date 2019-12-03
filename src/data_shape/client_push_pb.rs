use indicatif::{ProgressBar, ProgressStyle};
use crate::data_shape::{FullPathFileItem};

pub struct TransferFileProgressBar {
    pub total_files: u64,
    pub consumed_files: u64,
    pub pb: ProgressBar,
    pub show_pb: bool,
}

impl TransferFileProgressBar {
    pub fn new(total_files: u64, show_pb: bool) -> Self {
        let pb = ProgressBar::new(!0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{prefix}][{elapsed_precise}] {wide_bar:cyan/blue} {bytes:>7}/{total_bytes:7} {bytes_per_sec} {msg}")
                .progress_chars("##-"),
        );

        TransferFileProgressBar {
            total_files,
            pb,
            show_pb,
            consumed_files: 0,
        }
    }

    pub fn push_one(&mut self, file_len: u64, file_item: &FullPathFileItem) {
        if self.show_pb {
            self.consumed_files += 1;
            self.pb.set_position(0);
            self.pb.set_length(file_len);
            self.pb.set_message(file_item.to_path.as_str());
            let prefix = format!("{}/{}", self.consumed_files, self.total_files);
            self.pb.set_prefix(prefix.as_str());
        }
    }

    pub fn skip_one(&mut self) {
        if self.show_pb {
            self.consumed_files += 1;
            self.pb.set_position(0);
            self.pb.set_length(!0);
            self.pb.set_message("skipping");
            let prefix = format!("{}/{}", self.consumed_files, self.total_files);
            self.pb.set_prefix(prefix.as_str());
        }
    }
}
