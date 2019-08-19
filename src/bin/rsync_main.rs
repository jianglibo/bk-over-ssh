extern crate ssh_client_demo;

use std::io::prelude::*;
use std::io::Cursor;
use librsync::{Delta, Patch, Signature};

fn main() {
    let base = "base file".as_bytes();
    let new = "modified base file".as_bytes();

    // create signature starting from base file
    let mut sig = Signature::new(base).unwrap();
    // create delta from new file and the base signature
    let delta = Delta::new(new, &mut sig).unwrap();
    // create and store the new file from the base one and the delta
    let mut patch = Patch::new(Cursor::new(base), delta).unwrap();
    let mut computed_new = Vec::new();
    patch.read_to_end(&mut computed_new).unwrap();

    // test whether the computed file is exactly the new file, as expected
    assert_eq!(computed_new, new);
}