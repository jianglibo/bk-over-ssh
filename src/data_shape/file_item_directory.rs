use super::RemoteFileItem;
use log::*;
use std::io;
use std::io::prelude::{BufRead, Read};
use std::iter::Iterator;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct PrimaryFileItem {
    pub local_dir: Arc<PathBuf>,
    pub remote_dir: Arc<String>,
    pub remote_file_item: RemoteFileItem,
}

#[derive(Debug)]
pub struct FileItemDirectory<R: BufRead> {
    pub dir_name: Arc<PathBuf>,
    pub remote_dir: Arc<String>,
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
                        match serde_json::from_str::<RemoteFileItem>(&line) {
                            Ok(remote_file_item) => {
                                let fi = PrimaryFileItem {
                                    local_dir: self.dir_name.clone(),
                                    remote_dir: self.remote_dir.clone(),
                                    remote_file_item,
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
                            .replace(line.to_owned());
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
        dir_name: String,
        reader: Arc<Mutex<R>>,
        maybe_dir_line: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            dir_name: Arc::new(PathBuf::from(dir_name)),
            reader,
            maybe_dir_line,
            remote_dir: Arc::new("".to_owned()),
        }
    }
}

pub struct FileItemDirectories<R: BufRead> {
    reader: Arc<Mutex<R>>,
    maybe_dir_line: Arc<Mutex<Option<String>>>,
}

impl<R: BufRead> FileItemDirectories<R> {
    pub fn from_file_reader<RR: Read>(reader: RR) -> FileItemDirectories<io::BufReader<RR>> {
        let reader = io::BufReader::new(reader);
        FileItemDirectories {
            reader: Arc::new(Mutex::new(reader)),
            maybe_dir_line: Arc::new(Mutex::new(None)),
            // phantom: PhantomData,
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
            Some(FileItemDirectory::new(
                line,
                self.reader.clone(),
                self.maybe_dir_line.clone(),
            ))
        } else {
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
                            trace!("got item line {}", line);
                        } else {
                            trace!(
                            "got directory line, it's a remote represent of path, be careful: {:?}",
                            line
                        );
                            return Some(FileItemDirectory::new(
                                line.to_owned(),
                                self.reader.clone(),
                                self.maybe_dir_line.clone(),
                            ));
                        }
                    }
                    Err(err) => error!("read line failed failed: {:?}", err),
                }
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
\\?\F:\github\bk-over-ssh\fixtures\a-dir
{"path":"a.txt","sha1":null,"len":1,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b\\b.txt","sha1":null,"len":1,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b\\c c\\c c .txt","sha1":null,"len":3,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"b b\\b b.txt","sha1":null,"len":5,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"qrcode.png","sha1":null,"len":6044,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"Tomcat6\\logs\\catalina.out","sha1":null,"len":5,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
{"path":"鮮やか","sha1":null,"len":6,"modified":1571310663,"created":1571310663,"changed":false,"confirmed":false}
"##;
        let f = tu.make_a_file_with_content("abc.txt", content)?;

        let reader = fs::OpenOptions::new().read(true).open(f)?;

        let ddd = FileItemDirectories::<io::BufReader<fs::File>>::from_file_reader(reader);
        // let ddd: FileItemDirectories<io::BufReader<fs::File>> = FileItemDirectories::<_>::from_file_reader(reader);

        for d in ddd {
            // println!("{:?}", d);
            for fi in d {
                println!("{:?}", fi);
            }
        }
        Ok(())
    }
}
