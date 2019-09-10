use log::*;
use std::collections::HashMap;
use std::path::{Component, Path};
use std::{fs, io};

#[derive(Debug)]
struct FileCopy {
    pub dir_entry: fs::DirEntry,
    pub copy_trait: String,
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
                    warn!(
                        "skip item in directory: {:?}, name_stem: {:?}, name_ext_with_dot: {:?}",
                        d_name, name_stem, name_ext_with_dot
                    );
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

fn get_file_copy_vec(
    dir: impl AsRef<Path>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<Vec<FileCopy>, failure::Error> {
    let rd = dir.as_ref().read_dir()?;
    let mut dir_entrys: Vec<FileCopy> = rd
        .filter_map(|et| {
            match dir_entry_matches(
                dir.as_ref(),
                et,
                name_stem.as_ref(),
                name_ext_with_dot.as_ref(),
            ) {
                Ok(dn) => dn,
                Err(err) => {
                    error!("dir_entry_matches got error: {:?}", err);
                    None
                }
            }
        })
        .collect::<Vec<FileCopy>>();

    dir_entrys.sort_unstable_by_key(|k| k.copy_trait.clone());
    Ok(dir_entrys)
}

fn group_file_copies(
    dir_entrys: Vec<FileCopy>,
    compare_char_len: usize,
) -> Result<HashMap<String, Vec<FileCopy>>, failure::Error> {
    Ok(dir_entrys
        .into_iter()
        .fold(HashMap::<String, Vec<FileCopy>>::new(), |mut mp, fc| {
            let mut s = fc.copy_trait.clone();
            s.replace_range(compare_char_len.., "");

            mp.entry(s).or_insert_with(Vec::new).push(fc);
            mp
        }))
}

/// monthly, weekly, daily, hourly, minutely,
/// The file name pattern is yyyyMMddHHmmss 14 chars.
#[allow(dead_code)]
pub fn get_next_file_name(
    dir: impl AsRef<Path>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<String, failure::Error> {
    let dir = dir.as_ref();
    let name_stem = name_stem.as_ref();
    let name_ext_with_dot = name_ext_with_dot.as_ref();

    let mv = get_file_copy_vec(dir, name_stem, name_ext_with_dot)?;
    let mp = group_file_copies(mv, 1);
    info!("dir_entry: {:?}", mp);
    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use chrono::{DateTime, TimeZone, Utc};

    #[test]
    fn t_chrono() {
        // let dt = Utc.now();
        let dt = Utc.ymd(2014, 11, 28).and_hms(12, 0, 9);
        assert_eq!(dt.format("%Y%m%d%H%M%S").to_string(), "20141128120009");
    }

    #[test]
    fn t_rolling_file_1() -> Result<(), failure::Error> {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::rolling_files"],
            Some(vec!["ssh2"]),
        )?;
        let dt = Utc.ymd(2014, 11, 28).and_hms(12, 0, 9);
        let file_name = dt.format("abc_%Y%m%d%H%M%S.tar").to_string();
        let t_dir = tutil::create_a_dir_and_a_file_with_content(&file_name, "abc")?;
        let t_name = t_dir.tmp_dir_path();
        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "abc_",
            ".tar",
        )?
        .expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "20141128120009");

        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "abc_",
            "",
        )?
        .expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), "20141128120009.tar");

        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "",
            "",
        )?
        .expect("result has some.");
        assert_eq!(de.copy_trait.as_str(), file_name.as_str());

        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "",
            "xx",
        )?;
        assert!(de.is_none());
        Ok(())
    }

    #[test]
    fn t_rolling_file_2() -> Result<(), failure::Error> {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::rolling_files"],
            Some(vec!["ssh2"]),
        )?;

        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20131128120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20131028120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20130928120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20140128120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20140228120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20140328120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20150101120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20150102120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20150103120009.tar", "abc")?;

        assert_eq!(t_dir.tmp_dir_path().read_dir()?.count(), 9);

        let t_name = t_dir.tmp_dir_path();
        let mv = get_file_copy_vec(t_name, "abc_", ".tar")?;
        let mp = group_file_copies(mv, 3)?;
        info!("{:?}", mp);
        assert_eq!(mp.keys().len(), 1);
        assert_eq!(mp.keys().next().expect("at least one key."), "201");

        let mv = get_file_copy_vec(t_name, "abc_", ".tar")?;
        let mp = group_file_copies(mv, 4)?; // yearly result.
        info!("{:?}", mp);
        assert_eq!(mp.keys().len(), 3);
        assert!(mp
            .keys()
            .collect::<Vec<&String>>()
            .contains(&&"2013".to_string()));
        assert!(mp
            .keys()
            .collect::<Vec<&String>>()
            .contains(&&"2014".to_string()));
        assert!(mp
            .keys()
            .collect::<Vec<&String>>()
            .contains(&&"2015".to_string()));

        let v2013 = mp
            .get("2013")
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.clone())
            .collect::<Vec<String>>();
        assert_eq!(
            v2013,
            ["20130928120009", "20131028120009", "20131128120009"]
        );

        let v2014 = mp
            .get("2014")
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.clone())
            .collect::<Vec<String>>();
        assert_eq!(
            v2014,
            ["20140128120009", "20140228120009", "20140328120009"]
        );

        let v2015 = mp
            .get("2015")
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.clone())
            .collect::<Vec<String>>();
        assert_eq!(
            v2015,
            ["20150101120009", "20150102120009", "20150103120009"]
        );

        let mut keys = mp.keys().map(String::as_str).collect::<Vec<&str>>();
        keys.sort_unstable();

        assert_eq!(keys, ["2013", "2014", "2015"]); // keep last copy of the 2013s and 2014s.

        let mv = mp.get("2013").map(|v| {
            if let Some((last, elements)) = v.split_last() {
                elements.iter().for_each(|vv| info!("{:?}", vv.dir_entry));
            }
            Option::<u8>::None
        });

        Ok(())
    }

}
