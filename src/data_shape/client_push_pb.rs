use indicatif::{ProgressBar, ProgressStyle};

pub struct ClientPushProgressBar {
    pub total_files: u64,
    pub consumed_files: u64,
    pub pb: ProgressBar,
}

impl ClientPushProgressBar {
    pub fn new(total_files: u64) -> Self {
        let pb = ProgressBar::new(!0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{prefix}][{elapsed_precise}] {bar:40.cyan/blue} {bytes:>7}/{total_bytes:7} {bytes_per_sec} {msg}")
                .progress_chars("##-"),
        );

        Self {
            total_files,
            pb,
            consumed_files: 0,
        }
    }

    pub fn push_one(&mut self, file_len: u64, file_name: impl AsRef<str>) {
        self.consumed_files += 1;
        self.pb.reset();
        self.pb.set_length(file_len);
        self.pb.set_message(file_name.as_ref());
        let prefix = format!("{}/{}", self.consumed_files, self.total_files);
        self.pb.set_prefix(prefix.as_str());
    }

    pub fn skip_one(&mut self) {
        self.consumed_files += 1;
        self.pb.reset();
        self.pb.set_length(!0);
        self.pb.set_message("skipping");
        let prefix = format!("{}/{}", self.consumed_files, self.total_files);
        self.pb.set_prefix(prefix.as_str());
    }
}