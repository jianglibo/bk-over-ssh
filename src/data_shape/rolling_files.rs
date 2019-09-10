use log::*;
use std::path::{Component, Path};
use std::{fs, io};

#[derive(Debug)]
struct FileCopy {
    dir_entry: fs::DirEntry,
    copy_trait: String,
}

fn dir_entry_matches(
    dir: impl AsRef<Path>,
    dir_entry: io::Result<fs::DirEntry>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<Option<FileCopy>, failure::Error> {
    let dir = dir.as_ref();
    let dir_entry = dir_entry?;
    let name_stem = name_stem.as_ref();
    let name_ext_with_dot = name_ext_with_dot.as_ref();
    if let Some(com) = dir_entry.path().strip_prefix(dir)?.components().next() {
        if let Component::Normal(d_name) = com {
            if let Some(d_name) = d_name.to_str() {
                if d_name.starts_with(name_stem) && d_name.ends_with(name_ext_with_dot) {
                    let mut sn = d_name.splitn(2, name_stem);
                    sn.next();
                    let strip_prefix = sn.next().expect("strip name_stem should success.");
                    let mut sn = strip_prefix.rsplitn(2, name_ext_with_dot);
                    sn.next();
                    let copy_trait = sn
                        .next()
                        .expect("strip name_ext_with_dot should success")
                        .to_string();
                    let fc = FileCopy {
                        dir_entry,
                        copy_trait,
                    };
                    return Ok(Some(fc));
                } else {
                    warn!("skip item in directory: {:?}", d_name);
                }
            } else {
                bail!("OsStr to_str failed: {:?}", d_name);
            }
        } else {
            bail!("first component isn't a Normal component. {:?}", com);
        }
    } else {
        bail!("empty components. {:?}", dir_entry);
    }
    Ok(None)
}

/// monthly, weekly, daily, hourly, minutely,
#[allow(dead_code)]
pub fn get_next_file_name(
    dir: impl AsRef<Path>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<String, failure::Error> {
    let rd = dir.as_ref().read_dir()?;
    let dir_entry: Vec<FileCopy> = rd
        .filter_map(|et| {
            match dir_entry_matches(dir.as_ref(), et, name_stem.as_ref(), name_ext_with_dot.as_ref()) {
                Ok(dn) => dn,
                Err(err) => {
                    error!("dir_entry_matches got error: {:?}", err);
                    None
                }
            }
        })
        .collect();
    info!("dir_entry: {:?}", dir_entry);
    Ok("".to_string())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;

    #[test]
    fn t_rolling_file_1() -> Result<(), failure::Error> {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::rolling_files"],
            Some(vec!["ssh2"]),
        )?;
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20190823.tar", "abc")?;
        let t_name = t_dir.tmp_dir_path();
        let de = dir_entry_matches(t_name, t_name.read_dir()?.next().expect("at least has one."), "abc_", ".tar")?.expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "20190823");

        let de = dir_entry_matches(t_name, t_name.read_dir()?.next().expect("at least has one."), "abc_", "")?.expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "20190823.tar");

        let de = dir_entry_matches(t_name, t_name.read_dir()?.next().expect("at least has one."), "", "")?.expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "abc_20190823.tar");

        let de = dir_entry_matches(t_name, t_name.read_dir()?.next().expect("at least has one."), "", "xx")?.expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "abc_20190823.tar");
        Ok(())
    }

}