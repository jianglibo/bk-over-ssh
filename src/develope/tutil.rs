use rand::Rng;
use std::path::{Path, PathBuf};
use std::{fs, io, io::BufRead, io::BufWriter, io::Seek, io::Write};
use tempfile::TempDir;
use crate::db_accesses::{SqliteDbAccess};

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

pub fn print_cursor_lines(cursor: &mut io::Cursor<Vec<u8>>) {
    cursor.seek(io::SeekFrom::Start(0)).unwrap();
    io::BufReader::new(cursor).lines().for_each(|line| {
        println!("{}", line.unwrap());
    });
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

    pub fn get_file_path(&self, file_name: impl AsRef<str>) -> PathBuf {
        self.tmp_dir_path().join(file_name.as_ref())
    }

    pub fn assert_file_exists(&self, file_name: impl AsRef<str>) {
        let f = self.tmp_dir_path().join(file_name.as_ref());
        assert!(f.exists() && f.is_file());
    }

    pub fn open_a_file_for_read(
        &self,
        file_name: impl AsRef<str>,
    ) -> Result<impl io::Read, failure::Error> {
        let f = self.tmp_dir_path().join(file_name.as_ref());
        Ok(fs::OpenOptions::new().read(true).open(f)?)
    }

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
    ) -> Result<(), failure::Error> {
        let tmp_file = self.tmp_dir.path().join(file_name.as_ref());
        let mut tmp_file_writer = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&tmp_file)?,
        );
        write!(tmp_file_writer, "{}", content.as_ref())?;
        Ok(())
    }

    pub fn make_a_file_with_len(
        &self,
        file_name: impl AsRef<str>,
        file_len: usize,
    ) -> Result<(), failure::Error> {
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
        Ok(())
    }
}

impl std::default::Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_filename(file_name: impl AsRef<str>) -> Result<TestDir, failure::Error> {
    let td = TestDir::new();
    td.make_a_file_with_content(file_name, "")?;
    Ok(td)
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
