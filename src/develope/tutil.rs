use crate::data_shape::{demo_app_conf, AppConf, Server, AppRole};
use crate::db_accesses::{DbAccess, SqliteDbAccess};
use rand::Rng;
use std::path::{Path, PathBuf};
use std::{fs, io, io::BufRead, io::BufWriter, io::Seek, io::Write};
use tempfile::TempDir;

/// data dir in the current directory.
#[allow(dead_code)]
pub fn load_demo_app_conf_sqlite(data_dir: Option<&str>, app_role: AppRole) -> AppConf {
    let data_dir = data_dir.unwrap_or_else(||"data");
    demo_app_conf(data_dir, app_role)
}

#[allow(dead_code)]
pub fn load_demo_server_sqlite (
    app_conf: &AppConf,
    server_yml: Option<&str>,
) -> Server {
    let server_yml = server_yml.unwrap_or_else(||"localhost.yml");
    app_conf.load_server_from_yml(
        server_yml,
    )
    .unwrap()
}

#[allow(dead_code)]
pub fn get_a_cursor_writer() -> io::Cursor<Vec<u8>> {
    let v = Vec::<u8>::new();
    io::Cursor::new(v)
}

#[allow(dead_code)]
pub fn count_cursor_lines(cursor: &mut io::Cursor<Vec<u8>>) -> usize {
    cursor.seek(io::SeekFrom::Start(0)).unwrap();
    io::BufReader::new(cursor).lines().count()
}

#[allow(dead_code)]
pub fn create_a_sqlite_file_db(db_dir: &TestDir) -> Result<SqliteDbAccess, failure::Error> {
    let db_file = db_dir.tmp_dir_path().join("db.db");
    let db_access = SqliteDbAccess::new(db_file);
    db_access.create_database()?;
    Ok(db_access)
}
#[allow(dead_code)]
pub fn print_cursor_lines(cursor: &mut io::Cursor<Vec<u8>>) {
    cursor.seek(io::SeekFrom::Start(0)).unwrap();
    io::BufReader::new(cursor).lines().for_each(|line| {
        eprintln!("{}", line.unwrap());
    });
}
    pub fn make_a_file_with_content(
        dir: impl AsRef<Path>,
        file_name: impl AsRef<str>,
        content: impl AsRef<str>,
    ) -> Result<PathBuf, failure::Error> {
        let dir = dir.as_ref();
        let tmp_file = dir.join(file_name.as_ref());
        let mut tmp_file_writer = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&tmp_file)?,
        );
        write!(tmp_file_writer, "{}", content.as_ref())?;
        Ok(tmp_file)
    }


#[allow(dead_code)]
#[derive(Debug)]
pub struct TestDir {
    pub tmp_dir: TempDir,
}

impl TestDir {
    pub fn new() -> Self {
        Self {
            tmp_dir: TempDir::new().expect("TempDir::new should success."),
        }
    }
    #[allow(dead_code)]
    pub fn count_files(&self) -> usize {
        self.tmp_dir
            .path()
            .read_dir()
            .expect("tmp_dir read_dir should success.")
            .count()
    }

    #[allow(dead_code)]
    pub fn tmp_dir_path(&self) -> &Path {
        self.tmp_dir.path()
    }

    /// Returns the str representation of TestDir.
    #[allow(dead_code)]
    pub fn tmp_dir_str(&self) -> &str {
        self.tmp_dir_path().to_str().unwrap()
    }

    pub fn tmp_file(&self) -> Result<PathBuf, failure::Error> {
        Ok(self.tmp_dir.path().read_dir()?.next().unwrap()?.path())
    }

    #[allow(dead_code)]
    pub fn tmp_file_str(&self) -> Result<String, failure::Error> {
        let de = self.tmp_file()?.to_str().unwrap().to_string();
        Ok(de)
    }

    #[allow(dead_code)]
    pub fn tmp_file_name_only(&self) -> Result<String, failure::Error> {
        Ok(self
            .tmp_file()?
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string())
    }

    #[allow(dead_code)]
    pub fn tmp_file_len(&self) -> Result<u64, failure::Error> {
        Ok(self.tmp_file()?.metadata().unwrap().len())
    }

    #[allow(dead_code)]
    pub fn get_file_path(&self, file_name: impl AsRef<str>) -> PathBuf {
        self.tmp_dir_path().join(file_name.as_ref())
    }

    #[allow(dead_code)]
    pub fn assert_file_exists(&self, file_name: impl AsRef<str>) {
        let f = self.tmp_dir_path().join(file_name.as_ref());
        assert!(f.exists() && f.is_file());
    }
    #[allow(dead_code)]
    pub fn open_a_file_for_read(
        &self,
        file_name: impl AsRef<str>,
    ) -> Result<impl io::Read, failure::Error> {
        let f = self.tmp_dir_path().join(file_name.as_ref());
        Ok(fs::OpenOptions::new().read(true).open(f)?)
    }
    #[allow(dead_code)]
    pub fn open_an_empty_file_for_write(
        &self,
        file_name: impl AsRef<str>,
    ) -> Result<impl io::Write, failure::Error> {
        let f = self.tmp_dir_path().join(file_name.as_ref());
        Ok(fs::OpenOptions::new().create(true).write(true).open(f)?)
    }

    pub fn make_a_file_with_content(
        &self,
        file_name: impl AsRef<str>,
        content: impl AsRef<str>,
    ) -> Result<PathBuf, failure::Error> {
        make_a_file_with_content(self.tmp_dir.path(), file_name, content)
    }

    pub fn make_a_file_with_len(
        &self,
        file_name: impl AsRef<str>,
        file_len: usize,
    ) -> Result<PathBuf, failure::Error> {
        let tmp_file = self.tmp_dir.path().join(file_name.as_ref());

        let mut tmp_file_writer = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&tmp_file)?,
        );

        let mut rng = rand::thread_rng();
        (0..)
            .take(file_len)
            .map(|_| rng.gen::<u8>())
            .try_for_each(|c| {
                let x: Result<(), failure::Error> = tmp_file_writer
                    .write(&[c])
                    .map(|_| ())
                    .map_err(|e| e.into());
                x
            })?;
        Ok(tmp_file)
    }
}

impl std::default::Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_file_with_content(
    file_name: impl AsRef<str>,
    content: impl AsRef<str>,
) -> Result<TestDir, failure::Error> {
    let td = TestDir::new();
    td.make_a_file_with_content(file_name, content)?;
    Ok(td)
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_file_with_len(
    file_name: impl AsRef<str>,
    file_len: usize,
) -> Result<TestDir, failure::Error> {
    let td = TestDir::new();
    td.make_a_file_with_len(file_name, file_len)?;
    Ok(td)
}

#[allow(dead_code)]
pub fn change_file_content(file_path: impl AsRef<Path>) -> Result<(), failure::Error> {
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(file_path.as_ref())?;
    write!(f, "hello")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use failure;
    use std::io::{self};
    use bytes::{BytesMut, BufMut, Buf, Bytes};


    #[test]
    fn t_channel_io() -> Result<(), failure::Error> {
        let mut stdout = io::stdout();

        let array: [u8; 3] = [0; 3];

        stdout.write_all(&array[..])?;

        let mut buf = BytesMut::with_capacity(8);
        assert_eq!(buf.len(), 0);


        let a_u64: u64 = 2;
        buf.put_u64(a_u64);
        assert_eq!(buf.len(), 8);
        {
            let b1 = buf.clone().to_vec();
            let mut buf1 = Bytes::from(b1);
            assert_eq!(buf1.get_u64(), 2);
        }

        assert_eq!(buf[..].len(), 8);
        println!("{:?}", buf);
        assert_eq!(buf.freeze().get_u64(), 2);
        // println!("{:?}", buf);
        Ok(())
    }
}