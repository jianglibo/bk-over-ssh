use crate::rustsync::DeltaWriter;

use log::*;
use std::path::Path;
use std::time::{Instant};
use std::{fs, io};

use crate::data_shape::{CountReader, Indicator};
use crate::rustsync;

pub fn restore_a_file(
    old_file: Option<&str>,
    maybe_delta_file: Option<&str>,
    maybe_out_file: Option<&str>,
) -> Result<String, failure::Error> {
    let old_file = old_file.unwrap();
    let delta_file = if let Some(f) = maybe_delta_file {
        f.to_string()
    } else {
        format!("{}.delta", old_file)
    };

    let out_file = if let Some(f) = maybe_out_file {
        f.to_string()
    } else {
        format!("{}.restore", old_file)
    };

    let mut dr = rustsync::DeltaFileReader::<fs::File>::read_delta_file(delta_file)?;
    dr.restore_from_file_to_file(&out_file, old_file)?;
    Ok(out_file)
}

pub fn delta_a_file(
    new_file: Option<&str>,
    maybe_sig_file: Option<&str>,
    maybe_out_file: Option<&str>,
    print_progress: bool,
) -> Result<String, failure::Error> {
    let new_file = new_file.unwrap();
    if print_progress {
        let new_file_length = Path::new(new_file).metadata()?.len();
        println!("size:{}", new_file_length);
    }
    let sig_file = if let Some(f) = maybe_sig_file {
        f.to_string()
    } else {
        format!("{}.sig", new_file)
    };

    let out_file = if let Some(f) = maybe_out_file {
        f.to_string()
    } else {
        format!("{}.delta", new_file)
    };

    let sig = rustsync::Signature::load_signature_file(sig_file)?;

    let new_file_input = io::BufReader::new(fs::OpenOptions::new().read(true).open(new_file)?);
    if print_progress {
        let mut sum = 0;
        let f = |num| {
            sum += num;
            if sum > 5_0000 || num == 0 {
                println!("{:?}", sum);
                sum = 0;
            }
        };
        let nr = CountReader::new(new_file_input, f);
        rustsync::DeltaFileWriter::<fs::File>::create_delta_file(&out_file, sig.window, None)?
            .compare(&sig, nr)?;
    } else {
        rustsync::DeltaFileWriter::<fs::File>::create_delta_file(&out_file, sig.window, None)?
            .compare(&sig, new_file_input)?;
    }
    Ok(out_file)
}

pub fn signature(
    file: Option<&str>,
    block_size: Option<&str>,
    out: Option<&str>,
) -> Result<String, failure::Error> {
    let file = file.unwrap();
    let block_size: Option<usize> = block_size.and_then(|s| s.parse().ok());
    let sig_file = format!("{}.sig", file);
    let out = out.unwrap_or_else(|| sig_file.as_str());
    let start = Instant::now();
    let indicator = Indicator::new(None);
    match rustsync::Signature::signature_a_file(file, block_size, &indicator) {
        Ok(mut sig) => {
            if let Err(err) = sig.write_to_file(out) {
                error!("rsync signature write_to_file failed: {:?}", err);
            }
        }
        Err(err) => {
            error!("rsync signature failed: {:?}", err);
        }
    }
    eprintln!("time costs: {:?}", start.elapsed().as_secs());
    Ok(out.to_owned())
}


pub fn rsync_cmd_line<'a>(sub_matches: &'a clap::ArgMatches<'a>,) -> Result<(), failure::Error> {
    match sub_matches.subcommand() {
            ("restore-a-file", Some(sub_sub_matches)) => {
                restore_a_file(
                    sub_sub_matches.value_of("old-file"),
                    sub_sub_matches.value_of("delta-file"),
                    sub_sub_matches.value_of("out-file"),
                )?;
            }
            ("delta-a-file", Some(sub_sub_matches)) => {
                delta_a_file(
                    sub_sub_matches.value_of("new-file"),
                    sub_sub_matches.value_of("sig-file"),
                    sub_sub_matches.value_of("out-file"),
                    sub_sub_matches.is_present("print-progress"),
                )?;
            }
            ("signature", Some(sub_sub_matches)) => {
                signature(
                    sub_sub_matches.value_of("file"),
                    sub_sub_matches.value_of("block-size"),
                    sub_sub_matches.value_of("out"),
                )?;
            }
            (_, _) => {
                println!("please add --help to view usage help.");
            }
        }
        Ok(())
}