use rand::Rng;
// use rustsync::*;

use rand::distributions::Alphanumeric;

// use crate::develope::develope_data;
use librsync::{Delta, Patch, Signature};
use log::*;
use std::fs;
use std::io::prelude::*;
use std::io::{self, Cursor};
use std::path::Path;
use std::time::Instant;

use librsync::whole;

// fn tt() {
//     // Create 4 different random strings first.
//     let chunk_size = 1000;
//     let a = rand::thread_rng()
//         .sample_iter(&Alphanumeric)
//         .take(chunk_size)
//         .collect::<String>();
//     let b = rand::thread_rng()
//         .sample_iter(&Alphanumeric)
//         .take(50)
//         .collect::<String>();
//     let b_ = rand::thread_rng()
//         .sample_iter(&Alphanumeric)
//         .take(100)
//         .collect::<String>();
//     let c = rand::thread_rng()
//         .sample_iter(&Alphanumeric)
//         .take(chunk_size)
//         .collect::<String>();

//     // Now concatenate them in two different ways.

//     let mut source = a.clone() + &b + &c;
//     let mut modified = a + &b_ + &c;

//     // Suppose we want to download `modified`, and we already have
//     // `source`, which only differs by a few characters in the
//     // middle.

//     // We first have to choose a block size, which will be recorded
//     // in the signature below. Blocks should normally be much bigger
//     // than this in order to be efficient on large files.

//     let block = [0; 32];

//     // We then create a signature of `source`, to be uploaded to the
//     // remote machine. Signatures are typically much smaller than
//     // files, with just a few bytes per block.

//     let source_sig = signature(source.as_bytes(), block).unwrap();

//     // Then, we let the server compare our signature with their
//     // version.

//     let comp = compare(&source_sig, modified.as_bytes(), block).unwrap();

//     // We finally download the result of that comparison, and
//     // restore their file from that.

//     let mut restored = Vec::new();
//     restore_seek(
//         &mut restored,
//         std::io::Cursor::new(source.as_bytes()),
//         vec![0; 1000],
//         &comp,
//     )
//     .unwrap();
//     assert_eq!(&restored[..], modified.as_bytes())
// }

fn create_sig_file(old_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let old_file = fs::OpenOptions::new()
        .read(true)
        .open(old_name.as_ref())
        .expect("success.");
    let mut sig = Signature::new(&old_file)?;

    let sig_name = format!(
        "{}.sig",
        old_name.as_ref()
    );
    let mut sig_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&sig_name)?;
    io::copy(&mut sig, &mut sig_file)?;
    info!("sig");    
    Ok(sig_name)
}

fn create_sig_file_whole(old_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let mut old_file = fs::OpenOptions::new()
        .read(true)
        .open(old_name.as_ref())
        .expect("success.");

    let sig_name = format!(
        "{}.sig",
        old_name.as_ref()
    );
    let mut sig_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&sig_name)?;
    let num = whole::signature(&mut old_file, &mut sig_file)?;
    info!("sig : {}", num);
    Ok(sig_name)
}

fn create_delta_file_whole(old_name: impl AsRef<str>,changed_name: impl AsRef<str>, sig_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let delta_name = format!(
        "{}.delta",
        old_name.as_ref()
    );
    let mut delta_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&delta_name)?;

    let mut changed_file = fs::OpenOptions::new().read(true).open(changed_name.as_ref())?;
    let mut sig_file = fs::OpenOptions::new().read(true).open(sig_name.as_ref())?;
    let num = whole::delta(&mut changed_file, &mut sig_file, &mut delta_file)?;
    info!("delta {:?}", num);
    Ok(delta_name)
}

fn create_delta_file(old_name: impl AsRef<str>,changed_name: impl AsRef<str>, sig_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let delta_name = format!(
        "{}.delta",
        old_name.as_ref()
    );
    let mut delta_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&delta_name)?;
    let mut changed_file = fs::OpenOptions::new().read(true).open(changed_name.as_ref())?;
    let mut sig_file = fs::OpenOptions::new().read(true).open(sig_name.as_ref())?;
    let mut delta = Delta::new(&changed_file, &mut sig_file)?;
    io::copy(&mut delta, &mut delta_file)?;
    info!("delta");
    Ok(delta_name)
}





fn create_patch_file_whole(
    old_name: impl AsRef<str>,
    delta_name: impl AsRef<str>,
) -> Result<(), failure::Error> {

    let mut delta_file = fs::OpenOptions::new()
        .read(true)
        .open(delta_name.as_ref())?;
    
    let mut old_file = fs::OpenOptions::new().read(true).open(old_name.as_ref())?;

    let restored_name = format!("{}.restored", old_name.as_ref());

    let mut restored_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&restored_name)?;

    whole::patch(&mut old_file, &mut delta_file, &mut restored_file)?;

    eprintln!(
        "computed_new: {:?}",
        Path::new(&restored_name).metadata()?.len()
    );
    Ok(())
}

fn create_patch_file(
    old_name: impl AsRef<str>,
    delta_name: impl AsRef<str>,
) -> Result<(), failure::Error> {
    let delta_file = fs::OpenOptions::new()
        .read(true)
        .open(delta_name.as_ref())?;
    
    let old_file = fs::OpenOptions::new().read(true).open(old_name.as_ref())?;

    let mut patch = Patch::new(&old_file, delta_file)?;

    let restored_name = format!("{}.restored", old_name.as_ref());

    let mut restored_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&restored_name)?;

    io::copy(&mut patch, &mut restored_file)?;

    eprintln!(
        "computed_new: {:?}",
        Path::new(&restored_name).metadata()?.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_util;
    use crate::develope::tutil;
    use log::*;
    use std::{fs, io};

    // #[test]
    // fn t_signature() -> Result<(), failure::Error> {
    //     let file_name = "/mnt/f/1804164.7z";
    //     let fr = fs::File::open(file_name)?;
    //     let reader = io::BufReader::new(fr);
    //     let block = [0; 32];
    //     let sig = signature(reader, block)?;
    //     println!("{:?}", sig);
    //     Ok(())
    // }

    fn get_changed_file(old_name: impl AsRef<str>) -> Result<String, failure::Error> {
    let changed_name = format!(
        "{}.changed",
        old_name.as_ref()
    );

    if !Path::new(&changed_name).exists() {
        fs::copy(
            old_name.as_ref(),
            &changed_name,
        )?;
    }
    Ok(changed_name)
}

    fn rsynclib() -> Result<(), failure::Error> {
    let start = Instant::now();
    let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024*1024*4)?;
    let old_name = test_dir.tmp_file_str();
    let changed_name = get_changed_file(&old_name)?;

    let sig_name = create_sig_file(&old_name)?;
    let delta_name = create_delta_file(&old_name, changed_name, sig_name)?;

    create_patch_file(
        &old_name,
        delta_name,
    )?;
    eprintln!("costs seconds: {:?}", start.elapsed().as_secs());
    Ok(())
}

fn rsynclib_whole() -> Result<(), failure::Error> {
    let start = Instant::now();
    let test_dir = tutil::create_a_dir_and_a_file_with_len("xx.bin", 1024*1024*4)?;
    let old_name = test_dir.tmp_file_str();
    let changed_name = get_changed_file(&old_name)?;
    let sig_name = create_sig_file_whole(&old_name)?;

    let delta_name = create_delta_file_whole(&old_name, changed_name, sig_name)?;

    create_patch_file_whole(
        &old_name,
        delta_name,
    )?;
    eprintln!("costs seconds: {:?}", start.elapsed().as_secs());
    Ok(())
}

    #[test]
    fn t_librsync() -> Result<(), failure::Error>{
        log_util::setup_logger(vec![""], vec![]);
        rsynclib()?;
        Ok(())
    }


    #[test]
    fn t_librsync_whole() -> Result<(), failure::Error> {
        rsynclib_whole()?;
        Ok(())
    }

}
