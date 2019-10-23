use std::fmt;
use log::*;

pub const VERBATIM_PREFIX: &str = r#"\\?\"#;

/// A slash ended string with trailing slash removed.
#[derive(Debug, PartialEq)]
pub struct SlashPath {
    pub slash: String,
}

impl SlashPath {
    pub fn new(any_path: impl AsRef<str>) -> Self {
        let mut slash = strip_verbatim_prefixed(any_path).replace('\\', "/");
        if slash.len() > 1 && slash.ends_with('/') {
            slash = slash.trim_end_matches('/').to_string();
        }
        Self {
            slash,
        }
    }

    fn get_not_slash_end_str(&self) -> &str {
        if self.slash.ends_with('/') {
            ""
        } else {
            self.slash.as_str()
        }
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
        SlashPath::new(format!("{}/{}", self.get_not_slash_end_str(), extra_path.get_not_slash_start_str()))
    }

    pub fn parent(&self) -> Result<SlashPath, failure::Error> {
        if self.slash.len() < 2 {
            bail!("no parent for slash_path: {}", self.slash);
        }
        let vs: Vec<&str> = self.slash.rsplitn(2, '/').collect();
        if vs.len() != 2 {
            bail!("no parent for slash_path: {}", self.slash);
        } else {
            let s = if vs[1].is_empty() {
                "/"
            } else {
                vs[1]
            };
            Ok(
            SlashPath {
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

pub fn path_equal(win_or_linux_path_str_a: impl AsRef<str>, win_or_linux_path_str_b: impl AsRef<str>) -> bool {
    let mut a = win_or_linux_path_str_a.as_ref();
    let mut b = win_or_linux_path_str_b.as_ref();

    if a.starts_with(VERBATIM_PREFIX) { // it's a windows path.
        a = a.split_at(4).1;
    }

    if b.starts_with(VERBATIM_PREFIX) { // it's a windows path.
        b = b.split_at(4).1;
    }

    let aa = a.replace('\\', "/");
    let bb = b.replace('\\', "/");
    aa == bb
}

pub fn join_path<P: AsRef<str> + fmt::Display>(path_parent: P, path_child: P) -> String {
    let pp = path_parent.as_ref();
    if pp.starts_with(VERBATIM_PREFIX) { // it's a windows path.
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