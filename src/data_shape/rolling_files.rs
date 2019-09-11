use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike, Utc};
use log::*;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::{fs, io};

#[derive(Debug)]
struct FileCopy {
    pub dir_entry: fs::DirEntry,
    pub copy_trait: DateTime<Utc>,
}

enum GroupPeriod {
    Yearly,
    Monthly,
    Weekly,
    Daily,
    Hourly,
    Minutely,
}

fn format_dt(
    dt: DateTime<Utc>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> String {
    let s = format!(
        "{}%Y%m%d%H%M%S{}",
        name_stem.as_ref(),
        name_ext_with_dot.as_ref()
    );
    dt.format(&s).to_string()
}

fn parse_date_time(ss: impl AsRef<str>) -> Result<DateTime<FixedOffset>, failure::Error> {
    let ss = ss.as_ref();
    if ss.len() != 14 {
        bail!("not a 14 digitals string.");
    }
    let s = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &ss[0..4],
        &ss[4..6],
        &ss[6..8],
        &ss[8..10],
        &ss[10..12],
        &ss[12..14]
    );
    DateTime::parse_from_rfc3339(&s)
        .map_err(|e| failure::format_err!("parse_from_rfc3339 failed.{:?}", e))
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
                    if let Ok(copy_trait) = parse_date_time(&copy_trait) {
                        let fc = FileCopy {
                            dir_entry,
                            copy_trait: copy_trait.into(),
                        };
                        return Ok(Some(fc));
                    } else {
                        warn!("parse_date_time failed.");
                    }
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
    dir_entrys.sort_unstable_by_key(|k| k.copy_trait);
    Ok(dir_entrys)
}

fn group_file_copies(
    dir_entrys: Vec<FileCopy>,
    group_period: GroupPeriod,
) -> Result<HashMap<i32, Vec<FileCopy>>, failure::Error> {
    Ok(dir_entrys
        .into_iter()
        .fold(HashMap::<i32, Vec<FileCopy>>::new(), |mut mp, fc| {
            let s = match &group_period {
                GroupPeriod::Yearly => fc.copy_trait.year(),
                GroupPeriod::Monthly => fc.copy_trait.month() as i32,
                GroupPeriod::Weekly => {
                    let isow = fc.copy_trait.iso_week();
                    let y = fc.copy_trait.year();
                    if y != isow.year() {
                        (isow.week() - 53) as i32
                    } else {
                        isow.week() as i32
                    }
                }
                GroupPeriod::Daily => fc.copy_trait.day() as i32,
                GroupPeriod::Hourly => fc.copy_trait.hour() as i32,
                GroupPeriod::Minutely => fc.copy_trait.minute() as i32,
            };
            mp.entry(s).or_insert_with(Vec::new).push(fc);
            mp
        }))
}

// fn group_file_copies(
//     dir_entrys: Vec<FileCopy>,
//     compare_char_len: usize,
// ) -> Result<HashMap<String, Vec<FileCopy>>, failure::Error> {
//     Ok(dir_entrys
//         .into_iter()
//         .fold(HashMap::<String, Vec<FileCopy>>::new(), |mut mp, fc| {
//             let mut s = fc.copy_trait.clone();
//             s.replace_range(compare_char_len.., "");

//             mp.entry(s).or_insert_with(Vec::new).push(fc);
//             mp
//         }))
// }

/// monthly, weekly, daily, hourly, minutely,
/// The file name pattern is yyyyMMddHHmmss 14 chars.
#[allow(dead_code)]
pub fn get_next_file_name(
    dir: impl AsRef<Path>,
    name_stem: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> PathBuf {
    let dir = dir.as_ref();
    let name_stem = name_stem.as_ref();
    let name_ext_with_dot = name_ext_with_dot.as_ref();

    let s = format_dt(Utc::now(), name_stem, name_ext_with_dot);
    dir.join(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::rolling_files"],
            Some(vec!["ssh2"]),
        )
        .unwrap();
    }

    #[test]
    fn t_weekly() -> Result<(), failure::Error> {
        log();
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20130101010155.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20130110120009.tar", "abc")?;
        t_dir.make_a_file_with_content("abc_20130117120009.tar", "abc")?;

        let t_name = t_dir.tmp_dir_path();
        let mv = get_file_copy_vec(t_name, "abc_", ".tar")?;
        let mp = group_file_copies(mv, GroupPeriod::Weekly)?; // yearly result.

        let dt = Utc.ymd(2013, 1, 1).and_hms(1, 1, 55);
        assert_eq!(dt.iso_week().year(), 2013);
        assert_eq!(dt.iso_week().week(), 1);

        let dt = Utc.ymd(2014, 1, 1).and_hms(1, 1, 55);
        assert_eq!(dt.iso_week().year(), 2014);
        assert_eq!(dt.iso_week().week(), 1);

        let dt = Utc.ymd(2015, 12, 31).and_hms(23, 1, 55);
        assert_eq!(dt.iso_week().year(), 2015);
        assert_eq!(dt.iso_week().week(), 53);

        let mut keys = mp.keys().collect::<Vec<&i32>>();
        keys.sort_unstable();
        assert_eq!(keys, [&1, &2, &3]); // weekly.
        Ok(())
    }

    #[test]
    fn t_chrono() -> Result<(), failure::Error> {
        log();
        let dt = Utc.ymd(2014, 11, 28).and_hms(12, 0, 9);
        info!("{:?}", dt); // 2014-11-28T12:00:09Z

        let dt_parsed = DateTime::parse_from_rfc3339("2014-11-28T12:00:09Z")?;
        assert_eq!(dt, dt_parsed);

        assert_eq!(format_dt(dt, "", "").to_string(), "20141128120009");
        let ss = "20141128120009";
        assert_eq!(ss.len(), 14);
        let s = format!(
            "{}-{}-{}T{}:{}:{}Z",
            &ss[0..4],
            &ss[4..6],
            &ss[6..8],
            &ss[8..10],
            &ss[10..12],
            &ss[12..14]
        );
        assert_eq!(s, "2014-11-28T12:00:09Z");

        assert_eq!(dt.year(), 2014);
        assert_eq!(dt.month(), 11);
        assert_eq!(dt.day(), 28);
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 0);
        assert_eq!(dt.second(), 9);
        assert_eq!(dt.iso_week().week(), 48);

        // let dt1 = DateTime::parse_from_str("20141128120009UTC", "%Y%m%d%H%M%S")?;
        // info!("origin: {:?}, parsed: {:?}", dt, dt1);
        // assert_eq!(dt, dt1);

        Ok(())
    }

    #[test]
    fn t_rolling_file_1() -> Result<(), failure::Error> {
        log();
        let dt = Utc.ymd(2014, 11, 28).and_hms(12, 0, 9);
        let file_name = format_dt(dt, "abc_", ".tar");
        let t_dir = tutil::create_a_dir_and_a_file_with_content(&file_name, "abc")?;
        let t_name = t_dir.tmp_dir_path();
        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "abc_",
            ".tar",
        )?
        .expect("result has some.");
        assert_eq!(de.copy_trait, dt);

        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "abc_",
            "",
        )?;
        assert!(de.is_none());

        let de = dir_entry_matches(
            t_name,
            t_name.read_dir()?.next().expect("at least has one."),
            "",
            "",
        )?;
        assert!(de.is_none());

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
        log();
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
        let mp = group_file_copies(mv, GroupPeriod::Yearly)?; // yearly result.
        info!("{:?}", mp);
        assert_eq!(mp.keys().len(), 3);

        assert!(mp.keys().collect::<Vec<&i32>>().contains(&&2013));

        assert!(mp.keys().collect::<Vec<&i32>>().contains(&&2014));

        assert!(mp.keys().collect::<Vec<&i32>>().contains(&&2015));

        let v2013 = mp
            .get(&2013)
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.month())
            .collect::<Vec<u32>>();
        assert_eq!(v2013, [9, 10, 11]);

        let v2014 = mp
            .get(&2014)
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.month())
            .collect::<Vec<u32>>();
        assert_eq!(v2014, [1, 2, 3]);

        let v2015 = mp
            .get(&2015)
            .expect("has value.")
            .iter()
            .map(|fc| fc.copy_trait.day())
            .collect::<Vec<u32>>();
        assert_eq!(v2015, [1, 2, 3]);

        let mut keys = mp.keys().collect::<Vec<&i32>>();
        keys.sort_unstable();

        assert_eq!(keys, [&2013, &2014, &2015]); // keep last copy of the 2013s and 2014s.

        let mv = mp.get(&2013).map(|v| {
            if let Some((last, elements)) = v.split_last() {
                elements.iter().for_each(|vv| info!("{:?}", vv.dir_entry));
            }
            Option::<u8>::None
        });
        Ok(())
    }

}
