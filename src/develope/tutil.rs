use rand::{Rng};
use std::path::{PathBuf, Path};
use std::{fs, io, io::BufRead, io::BufWriter, io::Seek, io::Write};
use tempfile::{TempDir};

#[allow(dead_code)]
pub fn get_a_cursor_writer() -> io::Cursor<Vec<u8>> {
    let v = Vec::<u8>::new();
    io::Cursor::new(v)
}

#[allow(dead_code)]
pub fn count_cursor_lines(mut cursor: io::Cursor<Vec<u8>>) -> usize {
    cursor.seek(io::SeekFrom::Start(0)).unwrap();
    io::BufReader::new(cursor).lines().count()
}

#[allow(dead_code)]
pub struct TestDir {
    pub tmp_dir: TempDir,
    tmp_file: PathBuf,
}

impl TestDir {
    pub fn new(tmp_dir: TempDir, tmp_file: PathBuf) -> Self {
        Self { tmp_dir, tmp_file }
    }

    pub fn tmp_dir_path(&self) -> &Path {
        self.tmp_dir.path()
    }

    pub fn tmp_dir_str(&self) -> &str {
        self.tmp_dir_path().to_str().unwrap()
    }

    #[allow(dead_code)]
    pub fn tmp_file_str(&self) -> &str {
        self.tmp_file.to_str().unwrap()
    }

    #[allow(dead_code)]
    pub fn tmp_file_name_only(&self) -> &str {
        self.tmp_file.file_name().unwrap().to_str().unwrap()
    }

    #[allow(dead_code)]
    pub fn tmp_file_len(&self) -> u64 {
        self.tmp_file.metadata().unwrap().len()
    }
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_filename(file_name: impl AsRef<str>) -> Result<TestDir, failure::Error> {
    let tmp_dir = TempDir::new()?;
    let tmp_file = tmp_dir.path().join(file_name.as_ref());
    Ok(TestDir::new(tmp_dir, tmp_file))
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_file_with_content(
    file_name: impl AsRef<str>,
    content: impl AsRef<str>,
) -> Result<TestDir, failure::Error> {
    let tmp_dir = TempDir::new()?;
    let tmp_file = tmp_dir.path().join(file_name.as_ref());
    let mut tmp_file_writer = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&tmp_file)?,
    );
    write!(tmp_file_writer, "{}", content.as_ref())?;
    Ok(TestDir::new(tmp_dir, tmp_file))
}

#[allow(dead_code)]
pub fn create_a_dir_and_a_file_with_len(
    file_name: impl AsRef<str>,
    file_len: usize,
) -> Result<TestDir, failure::Error> {
    let tmp_dir = TempDir::new()?;
    let tmp_file = tmp_dir.path().join(file_name.as_ref());

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
    Ok(TestDir::new(tmp_dir, tmp_file))
}

pub fn change_file_content(file_path: impl AsRef<Path>) -> Result<(), failure::Error> {
    let mut f = fs::OpenOptions::new().append(true).open(file_path.as_ref())?;
    write!(f, "hello")?;
    Ok(())
}