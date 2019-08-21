extern crate ssh_client_demo;

use std::io::prelude::*;
use std::io::{self, Cursor};
use std::fs;
use librsync::{Delta, Patch, Signature};
use ssh_client_demo::develope::develope_data;
use std::time::Instant;

fn main() {
    let old = b"base file";
    let new = b"modified base file";
    let start = Instant::now();
    let dev_env = develope_data::load_env();

    let miedum_binary_file = fs::OpenOptions::new().read(true).open(&dev_env.servers.ubuntu18.test_files.midum_binary_file).expect("success.");

    // create signature starting from base file
    let mut sig = Signature::new(&old[..]).unwrap();
    // let mut sig = Signature::new(miedum_binary_file).unwrap();
    // create delta from new file and the base signature
    // let delta = Delta::new(&new[..], &mut sig).unwrap();
    let delta = Delta::new(miedum_binary_file, &mut sig).unwrap();
    // create and store the new file from the base one and the delta
    let mut patch = Patch::new(Cursor::new(old), delta).unwrap();
    let mut computed_new = Vec::new();
    patch.read_to_end(&mut computed_new).unwrap();
    println!("computed_new: {:?}", computed_new.len());

    // test whether the computed file is exactly the new file, as expected
    println!("costs seconds: {:?}", start.elapsed().as_secs());
    assert_eq!(computed_new, new);
}