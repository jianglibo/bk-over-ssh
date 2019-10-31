use crate::data_shape::PruneStrategy;
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use log::*;
use std::collections::HashMap;
use std::convert::TryInto;
use std::path::{Component, Path, PathBuf};
use std::{fs, io};

#[derive(Debug)]
struct FileCopy {
    pub dir_entry: fs::DirEntry,
    pub copy_trait: DateTime<Utc>,
}

#[derive(Debug)]
struct FileCopies {
    inner: HashMap<i32, Vec<FileCopy>>,
}

impl FileCopies {
    pub fn take_latest(&mut self) -> Option<Vec<FileCopy>> {
        let mut keys = self.inner.keys().cloned().collect::<Vec<i32>>();
        keys.sort_unstable(); // 2012, 2013, 2014
        if let Some(i) = keys.last() {
            self.inner.remove(i)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn set_mp(&mut self, mp: HashMap<i32, Vec<FileCopy>>) {
        self.inner = mp;
    }

    #[allow(dead_code)]
    pub fn count_fcs(&self) -> usize {
        self.inner.values().map(|v| v.len()).sum()
    }

    pub fn take_remains(&mut self) -> Vec<FileCopy> {
        self.inner.values_mut().flat_map(|v| v.drain(..)).collect()
    }
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
    dt: &DateTime<Utc>,
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
    name_prefix: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<Option<FileCopy>, failure::Error> {
    let dir = dir.as_ref();
    let dir_entry = dir_entry?;
    let name_prefix = name_prefix.as_ref();
    let name_ext_with_dot = name_ext_with_dot.as_ref();
    if let Some(com) = dir_entry.path().strip_prefix(dir)?.components().next() {
        if let Component::Normal(d_name) = com {
            if let Some(d_name) = d_name.to_str() {
                if d_name.starts_with(name_prefix) && d_name.ends_with(name_ext_with_dot) {
                    let mut sn = d_name.splitn(2, name_prefix);
                    sn.next();
                    let strip_prefix = sn.next().expect("strip name_prefix should success.");
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
                        "skip item in directory: {:?}, name_prefix: {:?}, name_ext_with_dot: {:?}",
                        d_name, name_prefix, name_ext_with_dot
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
    name_prefix: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<Vec<FileCopy>, failure::Error> {
    let rd = dir.as_ref().read_dir()?;
    let mut dir_entrys: Vec<FileCopy> = rd
        .filter_map(|et| {
            match dir_entry_matches(
                dir.as_ref(),
                et,
                name_prefix.as_ref(),
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

/// dir_entrys were orderby time asc.
fn group_file_copies(
    dir_entrys: Vec<FileCopy>,
    group_period: GroupPeriod,
) -> HashMap<i32, Vec<FileCopy>> {
    dir_entrys
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
        })
}

/// We group files by different unit. For every unit other than latest unit keep only the number of keep_num.
/// for the latest unit if the sub item number is greater than sub_keep_num, also delete it.
fn prune_period(
    mut file_group: HashMap<i32, Vec<FileCopy>>,
    keep_num: usize,
    sub_keep_num: usize,
) -> (FileCopies, Vec<FileCopy>) {
    trace!("sub_keep_num: {:?}", sub_keep_num);
    let mut keys = file_group.keys().cloned().collect::<Vec<i32>>();
    keys.sort_unstable(); // 2012, 2013, 2014
    keys.reverse(); // [2014, 2013, 2012]

    let mut file_copies_to_delete: Vec<FileCopy> = Vec::new();

    let remain_keys = if keys.len() > keep_num {
        // delete copy more than specified number.
        let (remain_keys, about_delete) = keys.split_at(keep_num);
        for it in about_delete {
            if let Some(mut cfs) = file_group.remove(it) {
                file_copies_to_delete.append(&mut cfs);
            }
        }
        remain_keys
    } else {
        keys.split_at(0).1
    };
    if let Some((latest_key, other_keys)) = remain_keys.split_first() {
        for period_key in other_keys {
            // except latest year all others years keep the last file copy only.
            if let Some(file_copies) = file_group.get_mut(period_key) {
                if file_copies.len() > 1 {
                    file_copies_to_delete
                        .append(&mut file_copies.drain(0..file_copies.len() - 1).collect());
                } else {
                    warn!("file copy items can't be empty.");
                }
            }
        }
        if sub_keep_num > 0 {
            if let Some(file_copies) = file_group.get_mut(latest_key) {
                if file_copies.len() > sub_keep_num {
                    file_copies_to_delete.append(
                        &mut file_copies
                            .drain(0..file_copies.len() - sub_keep_num)
                            .collect(),
                    );
                } else {
                    warn!("file copy items can't be empty.");
                }
            }
        }
    }

    (FileCopies { inner: file_group }, file_copies_to_delete)
}

fn prune_dir_result(
    prune_strategy: &PruneStrategy,
    dir: impl AsRef<Path>,
    name_prefix: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<(Vec<FileCopy>, Vec<FileCopy>), failure::Error> {
    // order by created time asc.
    let mv = get_file_copy_vec(dir, name_prefix, name_ext_with_dot)?;

    let mut all_to_delete: Vec<FileCopy> = Vec::new();
    let mut all_remains: Vec<FileCopy> = Vec::new();

    let mp = group_file_copies(mv, GroupPeriod::Yearly); // yearly

    let yearly_keep_num: usize = prune_strategy.yearly.try_into().unwrap();
    let weekly_keep_num: usize = prune_strategy.weekly.try_into().unwrap();
    let monthly_keep_num: usize = prune_strategy.monthly.try_into().unwrap();
    let daily_keep_num: usize = prune_strategy.daily.try_into().unwrap();
    let hourly_keep_num: usize = prune_strategy.hourly.try_into().unwrap();
    let minutely_keep_num: usize = prune_strategy.minutely.try_into().unwrap();

    // to_remain may contains another years' data. how to identify this problem.

    let (mut file_copies, mut to_delete) = prune_period(mp, yearly_keep_num, 0);
    let lastest_remain = file_copies
        .take_latest()
        .expect("yearly latest remains should't empty.");
    all_remains.append(&mut file_copies.take_remains());
    all_to_delete.append(&mut to_delete);
    trace!(
        "yearly keep_num: {:?}, to_remain: {:?}, to_delete: {:?}",
        yearly_keep_num,
        all_remains.len() + lastest_remain.len(),
        all_to_delete.len()
    );

    let lastest_remain = if prune_strategy.weekly > 0 {
        let mp = group_file_copies(lastest_remain, GroupPeriod::Weekly); // weekly
        let (mut file_copies, mut to_delete) = prune_period(mp, weekly_keep_num, daily_keep_num);

        let lastest_remain = file_copies
            .take_latest()
            .expect("weekly latest remains should't empty.");
        all_remains.append(&mut file_copies.take_remains());
        all_to_delete.append(&mut to_delete);
        trace!(
            "weekly keep_num: {:?}, to_remain: {:?}, to_delete: {:?}",
            weekly_keep_num,
            all_remains.len() + lastest_remain.len(),
            all_to_delete.len()
        );
        lastest_remain
    } else {
        let mp = group_file_copies(lastest_remain, GroupPeriod::Monthly); //monthly
        let (mut file_copies, mut to_delete) = prune_period(mp, monthly_keep_num, daily_keep_num);
        let lastest_remain = file_copies
            .take_latest()
            .expect("monthly latest remains should't empty.");
        all_remains.append(&mut file_copies.take_remains());
        all_to_delete.append(&mut to_delete);
        trace!(
            "monthly keep_num: {:?}, to_remain {:?}, to_delete: {:?}",
            monthly_keep_num,
            all_remains.len() + lastest_remain.len(),
            all_to_delete.len()
        );
        lastest_remain
    };

    let mp = group_file_copies(lastest_remain, GroupPeriod::Daily); // daily
    let (mut file_copies, mut to_delete) = prune_period(mp, daily_keep_num, hourly_keep_num);
    let lastest_remain = file_copies
        .take_latest()
        .expect("daily latest remains should't empty.");
    all_remains.append(&mut file_copies.take_remains());
    all_to_delete.append(&mut to_delete);
    trace!(
        "daily keep_num: {:?}, to_remain: {:?}, to_delete: {:?}",
        daily_keep_num,
        all_remains.len() + lastest_remain.len(),
        all_to_delete.len()
    );

    let mp = group_file_copies(lastest_remain, GroupPeriod::Hourly); // hourly
    let (mut file_copies, mut to_delete) = prune_period(mp, hourly_keep_num, minutely_keep_num);
    let lastest_remain = file_copies
        .take_latest()
        .expect("hourly latest remains should't empty.");
    all_remains.append(&mut file_copies.take_remains());
    all_to_delete.append(&mut to_delete);
    trace!(
        "hourly keep_num: {:?}, to_remain: {:?}, to_delete: {:?}",
        hourly_keep_num,
        all_remains.len() + lastest_remain.len(),
        all_to_delete.len()
    );

    let mp = group_file_copies(lastest_remain, GroupPeriod::Minutely); // minutely
    let (mut file_copies, mut to_delete) = prune_period(mp, minutely_keep_num, 0);
    let mut lastest_remain = file_copies
        .take_latest()
        .expect("hourly latest remains should't empty.");
    all_remains.append(&mut file_copies.take_remains());
    all_to_delete.append(&mut to_delete);
    trace!(
        "hourly keep_num: {:?}, to_remain: {:?}, to_delete: {:?}",
        minutely_keep_num,
        all_remains.len() + lastest_remain.len(),
        all_to_delete.len()
    );

    all_remains.append(&mut lastest_remain);
    all_remains.sort_unstable_by_key(|it| it.copy_trait);
    Ok((all_remains, all_to_delete))
}

pub fn do_prune_dir(
    prune_strategy: &PruneStrategy,
    dir: impl AsRef<Path>,
    name_prefix: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> Result<(), failure::Error> {
    let (_to_remain, to_delete) =
        prune_dir_result(prune_strategy, dir, name_prefix, name_ext_with_dot)?;
    for d in to_delete {
        let p = d.dir_entry.path();
        if d.dir_entry.metadata()?.file_type().is_file() {
            fs::remove_file(p)?;
        } else {
            fs::remove_dir_all(p)?;
        }
    }
    Ok(())
}

pub fn get_next_file_name(
    dir: impl AsRef<Path>,
    name_prefix: impl AsRef<str>,
    name_ext_with_dot: impl AsRef<str>,
) -> PathBuf {
    let dir = dir.as_ref();
    let name_stem = name_prefix.as_ref();
    let name_ext_with_dot = name_ext_with_dot.as_ref();

    let s = format_dt(&Utc::now(), name_stem, name_ext_with_dot);
    dir.join(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::PruneStrategyBuilder;
    use crate::develope::tutil;
    use crate::log_util;
    use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc, Weekday};

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::rolling_files"],
            Some(vec!["ssh2"]),
            "",
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
        let mp = group_file_copies(mv, GroupPeriod::Weekly); // yearly result.

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

        assert_eq!(format_dt(&dt, "", "").to_string(), "20141128120009");
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
        Ok(())
    }

    #[test]
    fn t_rolling_file_1() -> Result<(), failure::Error> {
        log();
        let dt = Utc.ymd(2014, 11, 28).and_hms(12, 0, 9);
        let file_name = format_dt(&dt, "abc_", ".tar");
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
        let mp = group_file_copies(mv, GroupPeriod::Yearly); // yearly result.
        info!("{:?}", mp);
        assert_eq!(mp.keys().len(), 3);

        assert!(mp.keys().any(|&x| x == 2013));

        assert!(mp.keys().any(|&x| x == 2014));

        assert!(mp.keys().any(|&x| x == 2015));

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

        let _mv = mp.get(&2013).map(|v| {
            if let Some((_last, elements)) = v.split_last() {
                elements.iter().for_each(|vv| info!("{:?}", vv.dir_entry));
            }
            Option::<u8>::None
        });
        Ok(())
    }

    #[derive(Builder, Debug)]
    #[builder(setter(into))]
    struct FileCopyGenerator {
        #[builder(setter(skip))]
        pub t_dir: tutil::TestDir,
        #[builder(default = "2015")]
        start_year: u32,
        #[builder(default = "1")]
        start_month: u32,
        #[builder(default = "1")]
        start_day: u32,
        #[builder(default = "1")]
        start_week: u32,
        #[builder(default = "1")]
        start_hour: u32,
        #[builder(default = "1")]
        years: u32,
        #[builder(default = "1")]
        months: u32,
        #[builder(default = "1")]
        days: u32,
        #[builder(default = "0")]
        weeks: u32,
        #[builder(default = "1")]
        hours: u32,
        #[builder(default = "\"a_\".to_string()")]
        pub prefix: String,
        #[builder(default = "\".txt\".to_string()")]
        pub postfix: String,
    }

    fn week_day_from_u32(d: u32) -> Option<Weekday> {
        match d {
            1 => Some(Weekday::Mon),
            2 => Some(Weekday::Tue),
            3 => Some(Weekday::Wed),
            4 => Some(Weekday::Thu),
            5 => Some(Weekday::Fri),
            6 => Some(Weekday::Sat),
            7 => Some(Weekday::Sun),
            _ => None,
        }
    }

    fn get_weekly_days(fc: &FileCopyGenerator) -> Vec<DateTime<Utc>> {
        (fc.start_year..fc.start_year + fc.years)
            .flat_map(|y| {
                (fc.start_week..fc.start_week + fc.weeks).flat_map(move |w| {
                    (fc.start_day..fc.start_day + fc.days).flat_map(move |d| {
                        (fc.start_hour..fc.start_hour + fc.hours).map(move |h| {
                            Utc.isoywd(
                                y as i32,
                                w,
                                week_day_from_u32(d).expect("week day should be between 1-7."),
                            )
                            .and_hms(h, 10, 11)
                        })
                    })
                })
            })
            .collect()
    }

    fn get_monthly_days(fc: &FileCopyGenerator) -> Vec<DateTime<Utc>> {
        (fc.start_year..fc.start_year + fc.years)
            .flat_map(|y| {
                (fc.start_month..fc.start_month + fc.months).flat_map(move |m| {
                    (fc.start_day..fc.start_day + fc.days).flat_map(move |d| {
                        (fc.start_hour..fc.start_hour + fc.hours)
                            .map(move |h| Utc.ymd(y as i32, m, d).and_hms(h, 10, 11))
                    })
                })
            })
            .collect()
    }

    impl FileCopyGenerator {
        pub fn init(&self) -> Result<(), failure::Error> {
            let days = if self.weeks > 0 {
                get_weekly_days(self)
            } else {
                get_monthly_days(self)
            };

            days.iter().for_each(|dt| {
                let s = format_dt(dt, &self.prefix, &self.postfix);
                self.t_dir
                    .make_a_file_with_content(&s, "a")
                    .expect("make_a_file_with_content should success.");
            });
            Ok(())
        }
    }

    #[test]
    fn t_monday() -> Result<(), failure::Error> {
        log();
        // test weekly.
        let fcg = FileCopyGeneratorBuilder::default()
            .prefix("a")
            .postfix("b")
            .start_day(5_u32)
            .weeks(3_u32)
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        info!("file_copy_generator: {:?}", fcg);
        assert_eq!(fcg.t_dir.count_files(), 9);

        let prune_strategy = PruneStrategyBuilder::default()
            .daily(3) // keep 3 days.
            .build()
            .map_err(failure::err_msg)?;

        let (to_remain, to_delete) = prune_dir_result(
            &prune_strategy,
            fcg.t_dir.tmp_dir_path(),
            fcg.prefix,
            fcg.postfix,
        )?;
        info!("to_remain: {:?}, to_delete: {:?}", to_remain, to_delete);
        assert_eq!(to_remain.len(), 3);
        assert_eq!(to_delete.len(), 6);
        let fc = to_remain.get(0).expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.iso_week().week(), 3); // the week 1 of year may span to previous year.
        Ok(())
    }

    #[test]
    fn t_rolling_file_3() -> Result<(), failure::Error> {
        log();
        let fcg = FileCopyGeneratorBuilder::default()
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        assert_eq!(fcg.t_dir.count_files(), 1);
        info!("file_copy_generator: {:?}", fcg);

        // test monthly.
        let fcg = FileCopyGeneratorBuilder::default()
            .prefix("a")
            .postfix("b")
            .months(3_u32)
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        assert_eq!(fcg.t_dir.count_files(), 3);

        let p = PruneStrategyBuilder::default()
            .build()
            .map_err(failure::err_msg)?;

        let (to_remain, to_delete) =
            prune_dir_result(&p, fcg.t_dir.tmp_dir_path(), fcg.prefix, fcg.postfix)?;
        info!("to_remain: {:?}, to_delete: {:?}", to_remain, to_delete);
        assert_eq!(to_remain.len(), 1);
        assert_eq!(to_delete.len(), 2);
        let fc = to_remain.get(0).expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.month(), 3);

        Ok(())
    }

    #[test]
    fn t_rolling_file_4() -> Result<(), failure::Error> {
        log();
        let fcg = FileCopyGeneratorBuilder::default()
            .prefix("a")
            .postfix("b")
            .months(3_u32)
            .days(7_u32)
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        assert_eq!(fcg.t_dir.count_files(), 21);

        let mut a = vec!["a"];
        a.drain(0..0);
        assert_eq!(a.len(), 1);
        // should remain 3 daily and 1 monthly = 4.
        let p = PruneStrategyBuilder::default()
            .monthly(2)
            .daily(3)
            .build()
            .map_err(failure::err_msg)?;
        info!("prune_strategy: {:?}", p);

        let (to_remain, to_delete) =
            prune_dir_result(&p, fcg.t_dir.tmp_dir_path(), fcg.prefix, fcg.postfix)?;
        info!("to_remain:");
        for tr in &to_remain {
            info!("{:?}", tr);
        }

        info!("to_delete:");
        for tr in &to_delete {
            info!("{:?}", tr);
        }
        assert_eq!(to_remain.len(), 4);
        assert_eq!(to_delete.len(), 17);
        let fc = to_remain.last().expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.month(), 3);
        assert_eq!(fc.copy_trait.day(), 7);

        let fc = to_remain.first().expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.month(), 2);
        assert_eq!(fc.copy_trait.day(), 7);
        Ok(())
    }

    #[test]
    fn t_rolling_file_5() -> Result<(), failure::Error> {
        log();
        let fcg = FileCopyGeneratorBuilder::default()
            .prefix("a")
            .postfix("b")
            .months(3_u32)
            .days(7_u32)
            .hours(3_u32)
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        assert_eq!(fcg.t_dir.count_files(), 63);

        let mut a = vec!["a"];
        a.drain(0..0);
        assert_eq!(a.len(), 1);
        // should remain 3 daily and 1 monthly = 4.
        let p = PruneStrategyBuilder::default()
            .monthly(2)
            .daily(3)
            .hourly(2)
            .build()
            .map_err(failure::err_msg)?;
        info!("prune_strategy: {:?}", p);

        let (to_remain, to_delete) =
            prune_dir_result(&p, fcg.t_dir.tmp_dir_path(), fcg.prefix, fcg.postfix)?;
        info!("to_remain:");
        for tr in &to_remain {
            info!("{:?}", tr);
        }

        info!("to_delete:");
        for tr in &to_delete {
            info!("{:?}", tr);
        }
        assert_eq!(to_remain.len(), 5); // because we don't touch the content of the latest day.
        assert_eq!(to_delete.len(), 58);
        let fc = to_remain.last().expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.month(), 3);
        assert_eq!(fc.copy_trait.day(), 7);
        assert_eq!(fc.copy_trait.hour(), 3);

        let fc = to_remain.first().expect("remain one.");
        assert_eq!(fc.copy_trait.year(), 2015);
        assert_eq!(fc.copy_trait.month(), 2);
        assert_eq!(fc.copy_trait.day(), 7);
        assert_eq!(fc.copy_trait.hour(), 3);
        Ok(())
    }

    #[test]
    fn t_rolling_file_6() -> Result<(), failure::Error> {
        log();
        let fcg = FileCopyGeneratorBuilder::default()
            .prefix("a")
            .postfix("b")
            .months(3_u32)
            .days(7_u32)
            .hours(3_u32)
            .build()
            .map_err(failure::err_msg)?;
        fcg.init()?;
        assert_eq!(fcg.t_dir.count_files(), 63);

        let mut a = vec!["a"];
        a.drain(0..0);
        assert_eq!(a.len(), 1);
        // should remain 3 daily and 1 monthly = 4.
        let p = PruneStrategyBuilder::default()
            .monthly(2)
            .daily(3)
            .hourly(2)
            .build()
            .map_err(failure::err_msg)?;
        info!("prune_strategy: {:?}", p);
        do_prune_dir(&p, fcg.t_dir.tmp_dir_path(), &fcg.prefix, &fcg.postfix)?;
        assert_eq!(fcg.t_dir.count_files(), 5);
        Ok(())
    }
}
