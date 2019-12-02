use super::{RelativeFileItem, SlashPath, AppRole};
use log::*;
use std::io;
use std::io::prelude::{BufRead, Read};
use std::iter::Iterator;
use std::sync::{Arc, Mutex};

/// Like a disk directory, but it contains PrimaryFileItem.
#[derive(Debug)]
pub struct PrimaryFileItem {
    pub from_dir: Arc<SlashPath>,
    pub to_dir: Arc<SlashPath>,
    pub relative_item: RelativeFileItem,
}

impl PrimaryFileItem {
    pub fn get_local_path(&self) -> SlashPath {
        self.from_dir.join(self.relative_item.get_path())
    }

    pub fn get_remote_path(&self) -> SlashPath {
        self.to_dir.join(self.relative_item.get_path())
    }

    pub fn get_relative_item(&self) -> &RelativeFileItem {
        &self.relative_item
    }
    #[allow(dead_code)]
    pub fn is_sha1_not_equal(&self, local_sha1: impl AsRef<str>) -> bool {
        Some(local_sha1.as_ref().to_ascii_uppercase())
            != self.relative_item.get_sha1().map(str::to_ascii_uppercase)
    }
}

/// represent a directory on the dist.
#[derive(Debug)]
pub struct FileItemDirectory<R: BufRead> {
    pub from_dir: Arc<SlashPath>,
    pub to_dir: Arc<SlashPath>,
    reader: Arc<Mutex<R>>,
    maybe_dir_line: Arc<Mutex<Option<String>>>,
}

impl<R: BufRead> Iterator for FileItemDirectory<R> {
    type Item = PrimaryFileItem;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();
        loop {
            line.clear();
            match self
                .reader
                .lock()
                .expect("lock FileItemDirectory reader failed")
                .read_line(&mut line)
            {
                Ok(0) => {
                    return None;
                }
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if line.starts_with('{') {
                        trace!("got item line {}", line);
                        match serde_json::from_str::<RelativeFileItem>(&line) {
                            Ok(relative_item) => {
                                let fi = PrimaryFileItem {
                                    from_dir: self.from_dir.clone(),
                                    to_dir: self.to_dir.clone(),
                                    relative_item,
                                };
                                return Some(fi);
                            }
                            Err(err) => {
                                error!("deserialize line failed: {}, {:?}", line, err);
                            }
                        }
                    } else {
                        trace!(
                            "got directory line, it's a remote represent of path, be careful: {:?}",
                            line
                        );
                        self.maybe_dir_line
                            .lock()
                            .expect("maybe_dir_line lock failed")
                            .replace(SlashPath::new(line).get_slash());
                        return None;
                    }
                }
                Err(err) => error!("read line failed failed: {:?}", err),
            }
        }
    }
}

impl<R: BufRead> FileItemDirectory<R> {
    pub fn new(
        from_dir: SlashPath,
        to_dir: SlashPath,
        reader: Arc<Mutex<R>>,
        maybe_dir_line: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            from_dir: Arc::new(from_dir),
            to_dir: Arc::new(to_dir),
            reader,
            maybe_dir_line,
        }
    }
}

pub struct FileItemDirectories<R: BufRead> {
    reader: Arc<Mutex<R>>,
    local_remote_pairs: Vec<(SlashPath, SlashPath)>,
    maybe_dir_line: Arc<Mutex<Option<String>>>,
    app_role: AppRole,
}

impl<R: BufRead> FileItemDirectories<R> {
    pub fn from_file_reader<RR: Read>(
        reader: RR,
        local_remote_pairs: Vec<(SlashPath, SlashPath)>,
        app_role: AppRole,
    ) -> FileItemDirectories<io::BufReader<RR>> {
        let reader = io::BufReader::new(reader);
        FileItemDirectories {
            reader: Arc::new(Mutex::new(reader)),
            maybe_dir_line: Arc::new(Mutex::new(None)),
            local_remote_pairs,
            app_role,
        }
    }

    fn process_dir_line(&self, line: impl AsRef<str>) -> Option<FileItemDirectory<R>> {
        let line = SlashPath::new(line);

        if let Some((from_dir, to_dir)) =
            self.local_remote_pairs.iter().find(|pair| {
                match &self.app_role {
                    AppRole::PassiveLeaf => pair.1 == line,
                    AppRole::ActiveLeaf => pair.0 == line,
                    _ => false,
                }
            })
        {
            Some(FileItemDirectory::new(
                from_dir.clone(),
                to_dir.clone(),
                self.reader.clone(),
                self.maybe_dir_line.clone(),
            ))
        } else {
            warn!("no matching local directory: {:?}", line);
            None
        }
    }
}

impl<R: BufRead> Iterator for FileItemDirectories<R> {
    type Item = FileItemDirectory<R>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(line) = self
            .maybe_dir_line
            .lock()
            .expect("maybe_dir_line lock failed")
            .take()
        {
            info!("got maybe_dir_line: {}", line);
            if let Some(d) = self.process_dir_line(line) {
                return Some(d);
            }
        }

        let mut line = String::new();
        loop {
            line.clear();
            match self
                .reader
                .lock()
                .expect("lock FileItemDirectories reader failed")
                .read_line(&mut line)
            {
                Ok(0) => {
                    return None;
                }
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if line.starts_with('{') {
                        warn!(
                            "got item line {}, it will not happen in the normal condition.",
                            line
                        );
                    } else if let Some(d) = self.process_dir_line(line) {
                        return Some(d);
                    }
                }
                Err(err) => error!("read line failed failed: {:?}", err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use std::fs;

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
    fn t_read_file_item_directories() -> Result<(), failure::Error> {
        log();
        let tu = tutil::TestDir::new();
        let content = r##"
xabc
\\?\F:\github\bk-over-ssh\fixtures\a-dir
\\?\F:\github\bk-over-ssh\fixtures\a-dir
{"path":"a.txt","sha1":null,"len":1,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b\\b.txt","sha1":null,"len":1,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b\\c c\\c c .txt","sha1":null,"len":3,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b b\\b b.txt","sha1":null,"len":5,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"qrcode.png","sha1":null,"len":6044,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"Tomcat6\\logs\\catalina.out","sha1":null,"len":5,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"鮮やか","sha1":null,"len":6,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
"##;
        let ge = || {
            let f = tu
                .make_a_file_with_content("abc.txt", content)
                .expect("make_a_file_with_content failed");

            let reader = fs::OpenOptions::new()
                .read(true)
                .open(f)
                .expect("read failed");

            let local_remote_pairs = vec![(
                SlashPath::new("F:\\github\\bk-over-ssh\\fixtures\\a-dir"),
                SlashPath::new("a-dir"),
            )];

            FileItemDirectories::<io::BufReader<fs::File>>::from_file_reader(
                reader,
                local_remote_pairs,
                AppRole::PassiveLeaf,
            )
        };

        assert_eq!(ge().count(), 2);

        let mut file_item_directories = ge();

        let one = file_item_directories.next().unwrap();
        assert_eq!(one.count(), 0);
        let two = file_item_directories.next().unwrap();
        assert_eq!(two.count(), 7);

        for file_item_directory in ge() {
            for primay_file_item in file_item_directory {
                eprintln!("{:?}", primay_file_item);
            }
        }
        Ok(())
    }
}
