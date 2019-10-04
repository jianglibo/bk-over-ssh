use ssh2;
use std::io::{Read};

pub fn get_stdout_eprintln_stderr(channel: &mut ssh2::Channel, eprint_stdout: bool) -> Option<String> {

    let mut s = String::new();
    let r = if let Err(err) =  channel.read_to_string(&mut s) {
        eprintln!("read channel stdout failure: {:?}", err);
        None
    } else {
        if eprint_stdout {
            eprintln!("{}", s);
        }
        Some(s)
    };

    let mut s = String::new();
    if let Err(err) =  channel.stderr().read_to_string(&mut s) {
        eprintln!("read channel stderr failure: {:?}", err);
    } else {
        eprintln!("{}", s);
    }

    r
}