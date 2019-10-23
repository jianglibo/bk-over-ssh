use ssh2;
use std::io::{Read};

pub fn get_stdout_eprintln_stderr(channel: &mut ssh2::Channel, eprint_stdout: bool) -> (String, String) {

    let mut s = String::new();
    let std_out = if let Err(err) =  channel.read_to_string(&mut s) {
        eprintln!("read channel stdout failure: {:?}", err);
        "".to_string()
    } else {
        if eprint_stdout {
            eprintln!("std_out: {}", s);
        }
        s
    };

    let mut s = String::new();
    let std_err = if let Err(err) =  channel.stderr().read_to_string(&mut s) {
        eprintln!("read channel stderr failure: {:?}", err);
        "".to_string()
    } else {
        eprintln!("std_err: {}", s);
        s
    };

    (std_out, std_err)
}