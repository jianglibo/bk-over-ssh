use log::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;
use std::{fs, io};

pub const VERBATIM_PREFIX: &str = r#"\\?\"#;

/// A slash ended string with trailing slash removed.
#[derive(Debug, Clone, PartialEq)]
pub struct SlashPath {
    pub slash: String,
}

impl std::default::Default for SlashPath {
    fn default() -> Self {
        Self {
            slash: "".to_string(),
        }
    }
}

impl fmt::Display for SlashPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.slash)
    }
}

impl Serialize for SlashPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.slash.as_str())
    }
}

pub fn deserialize_slash_path_from_str<'de, D>(deserializer: D) -> Result<SlashPath, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Ok(SlashPath { slash: s })
}

fn sanitiaze(any_path: impl AsRef<str>) -> String {
    let any_path = any_path.as_ref().trim();
    let mut slash = strip_verbatim_prefixed(any_path).replace('\\', "/");
    if slash.len() > 1 && slash.ends_with('/') {
        slash = slash.trim_end_matches('/').to_string();
    }
    slash
}

impl SlashPath {
    pub fn new(any_path: impl AsRef<str>) -> Self {
        Self {
            slash: sanitiaze(any_path),
        }
    }

    pub fn get_local_file_reader(&self) -> Result<impl std::io::Read, failure::Error> {
        Ok(fs::OpenOptions::new().read(true).open(self.as_path())?)
    }

    pub fn as_str(&self) -> &str {
        self.slash.as_str()
    }

    pub fn strip_prefix(&self, full_path: impl AsRef<Path>) -> String {
        let full = SlashPath::from_path(full_path.as_ref());
        full.as_str().split_at(self.as_str().len() + 1).1.to_owned()
    }

    pub fn from_path(path: &Path) -> Self {
        let s = match path.to_str() {
            Some(s) => s,
            None => panic!("path to string failed: {:?}", path),
        };
        SlashPath::new(s)
    }

    pub fn set_slash(&mut self, any_path: impl AsRef<str>) {
        self.slash = sanitiaze(any_path);
    }

    pub fn get_slash(&self) -> String {
        self.slash.clone()
    }

    /// /a/ will return a, /a will still return a.
    pub fn get_last_name(&self) -> String {
        let mut split = self.slash.rsplitn(3, '/');
        let mut s = split.next().expect("local_dir should has dir name.");
        if s.is_empty() {
            s = split.next().expect("local_dir should has dir name.");
        }
        s.to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.slash.is_empty() || self.slash.as_str() == "~" || self.slash.as_str() == "null"
    }

    fn get_not_slash_end_str(&self) -> &str {
        if self.slash.ends_with('/') {
            ""
        } else {
            self.slash.as_str()
        }
    }
    #[allow(dead_code)]
    pub fn ends_with(&self, end_str: impl AsRef<str>) -> bool {
        self.slash.ends_with(end_str.as_ref())
    }

    pub fn as_path(&self) -> &Path {
        &Path::new(&self.slash)
    }

    pub fn exists(&self) -> bool {
        Path::new(&self.slash).exists()
    }

    fn get_not_slash_start_str(&self) -> &str {
        if self.slash.starts_with('/') {
            self.slash.split_at(1).1
        } else {
            self.slash.as_str()
        }
    }

    pub fn join(&self, extra_path: impl AsRef<str>) -> SlashPath {
        let extra_path = SlashPath::new(extra_path.as_ref());
        SlashPath::new(format!(
            "{}/{}",
            self.get_not_slash_end_str(),
            extra_path.get_not_slash_start_str()
        ))
    }

    #[allow(dead_code)]
    pub fn join_path(&self, path: &Path) -> SlashPath {
        let extra_path = SlashPath::from_path(path);
        SlashPath::new(format!(
            "{}/{}",
            self.get_not_slash_end_str(),
            extra_path.get_not_slash_start_str()
        ))
    }

    pub fn join_another(&self, another: &SlashPath) -> SlashPath {
        SlashPath::new(format!(
            "{}/{}",
            self.get_not_slash_end_str(),
            another.get_not_slash_start_str()
        ))
    }

    pub fn create_dir_all(&self) -> io::Result<()> {
        fs::create_dir_all(&self.slash)
    }

    pub fn slash_equal_to(&self, str_line: impl AsRef<str>) -> bool {
        let another = SlashPath::new(str_line.as_ref().to_owned());
        self == &another
    }

    pub fn get_os_string(&self) -> std::ffi::OsString {
        std::ffi::OsString::from(&self.slash)
    }

    pub fn parent(&self) -> Result<SlashPath, failure::Error> {
        if self.slash.len() < 2 {
            bail!("no parent for slash_path: {}", self.slash);
        }
        let vs: Vec<&str> = self.slash.rsplitn(2, '/').collect();
        if vs.len() != 2 {
            bail!("no parent for slash_path: {}", self.slash);
        } else {
            let s = if vs[1].is_empty() { "/" } else { vs[1] };
            Ok(SlashPath {
                slash: s.to_string(),
            })
        }
    }
}

pub fn is_windows_path_start(s: &str) -> bool {
    let mut chars = s.chars();
    if let (Some(c0), Some(c1)) = (chars.next(), chars.next()) {
        c0.is_ascii_alphabetic() && c1 == ':'
    } else {
        false
    }
}

pub fn strip_verbatim_prefixed(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    if s.starts_with(VERBATIM_PREFIX) {
        trace!("dir start with VERBATIM_PREFIX, stripping it. {}", s);
        s.split_at(4).1.to_string()
    } else {
        s.to_string()
    }
}

#[allow(dead_code)]
pub fn path_equal(
    win_or_linux_path_str_a: impl AsRef<str>,
    win_or_linux_path_str_b: impl AsRef<str>,
) -> bool {
    let mut a = win_or_linux_path_str_a.as_ref();
    let mut b = win_or_linux_path_str_b.as_ref();

    if a.starts_with(VERBATIM_PREFIX) {
        // it's a windows path.
        a = a.split_at(4).1;
    }

    if b.starts_with(VERBATIM_PREFIX) {
        // it's a windows path.
        b = b.split_at(4).1;
    }

    let aa = a.replace('\\', "/");
    let bb = b.replace('\\', "/");
    aa == bb
}

pub fn join_path<P: AsRef<str> + fmt::Display>(path_parent: P, path_child: P) -> String {
    let pp = path_parent.as_ref();
    if pp.starts_with(VERBATIM_PREFIX) {
        // it's a windows path.
        let (_, pp1) = pp.split_at(4);
        format!("{}\\{}", pp1, path_child)
    } else if is_windows_path_start(pp) {
        format!("{}\\{}", pp, path_child)
    } else {
        format!("{}/{}", pp, path_child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t_idx() {
        let s = "a:\\b";
        let c0 = s.chars().nth(0).expect("at least have one char.");
        let c1 = s.chars().nth(1).expect("at least have one char.");
        assert!(c0.is_ascii_alphabetic());
        assert_eq!(c1, ':');
    }

    #[test]
    fn t_slash_path() -> Result<(), failure::Error> {
        let sp = SlashPath::new("/abc/dd");
        assert_eq!(sp.parent()?, SlashPath::new("/abc"));
        assert_eq!(sp.parent()?.parent()?.slash.as_str(), "/");

        let sp = SlashPath::new("./");
        assert!(sp.parent().is_err());

        let sp = SlashPath::new("/");
        assert!(sp.parent().is_err());

        let sp = SlashPath::new("/abc");
        assert_eq!(sp.parent()?.slash, "/");

        let pp = r#"\\?\D:\Documents\GitHub\bk-over-ssh\fixtures\adir"#;
        let sp = SlashPath::new(pp);
        assert_eq!(sp.slash, "D:/Documents/GitHub/bk-over-ssh/fixtures/adir");

        let sp = SlashPath::new("/abc");
        let sp = sp.join("cc");
        assert_eq!(sp.slash, "/abc/cc");

        let sp = SlashPath::new("/abc");
        let sp = sp.join("/cc");
        assert_eq!(sp.slash, "/abc/cc");

        let sp = SlashPath::new("");
        let sp = sp.join("/cc");
        assert_eq!(sp.slash, "/abc/cc");

        Ok(())
    }

    #[test]
    fn t_join() {
        let pp = r#"\\?\D:\Documents\GitHub\bk-over-ssh\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#"D:\Documents\GitHub\bk-over-ssh\fixtures\adir\a.txt"#);

        let pp = r#"D:\Documents\GitHub\bk-over-ssh\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#"D:\Documents\GitHub\bk-over-ssh\fixtures\adir\a.txt"#);

        let pp = r#":\Documents\GitHub\bk-over-ssh\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#":\Documents\GitHub\bk-over-ssh\fixtures\adir/a.txt"#);
    }
}
