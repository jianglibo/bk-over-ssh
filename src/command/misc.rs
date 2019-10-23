
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::thread;
use std::time::{Duration};
use crate::data_shape::{Server, ServerYml};
use crate::db_accesses::{SqliteDbAccess};
use r2d2_sqlite::SqliteConnectionManager;

pub fn verify_server_yml(mut server: Server<SqliteConnectionManager, SqliteDbAccess>) -> Result<(), failure::Error> {
                eprintln!(
                "found server configuration yml at: {:?}",
                server.yml_location.as_ref().expect("server.yml_location.as_ref should succeeded.")
            );
            eprintln!(
                "server content: {}",
                serde_yaml::to_string(&server.server_yml)?
            );
            server.connect()?;
            if let Err(err) = server.stats_remote_exec() {
                eprintln!(
                    "CAN'T FIND SERVER SIDE EXEC. {:?}\n{:?}",
                    server.server_yml.remote_exec, err
                );
            } else {
                let rp = server.get_remote_server_yml();
                match server.get_remote_file_content(&rp) {
                    Ok(content) => {
                        let ss: ServerYml = serde_yaml::from_str(content.as_str())?;
                        if !server.dir_equals(&ss.directories) {
                            eprintln!(
                                "SERVER DIRS DIDN'T EQUAL TO.\nlocal: {:?} vs remote: {:?}",
                                server.server_yml.directories, ss.directories
                            );
                        } else {
                            println!("SERVER SIDE CONFIGURATION IS OK!");
                        }
                    }
                    Err(err) => println!("got error: {:?}", err),
                }
            }
            Ok(())
}


pub fn demonstrate_pbr() -> Result<(), failure::Error> {
    let multi_bar = Arc::new(MultiProgress::new());

    let multi_bar1 = Arc::clone(&multi_bar);

    let count = 100;

    let mut i = 0;

    let _ = thread::spawn(move || loop {
        i += 1;
        println!("{}", i);
        multi_bar1.join_and_clear().expect("join_and_clear");
        thread::sleep(Duration::from_millis(5));
    });

    let pb1 = multi_bar.add(ProgressBar::new(1_000_000));
    pb1.set_style(
        ProgressStyle::default_bar()
            // .template("[{eta_precise}] {prefix:.bold.dim} {bar:40.cyan/blue} {pos:>7}/{len:7} {wide_msg}")
            .template("[{eta_precise}] {prefix:.bold.dim} {spinner} {bar:40.cyan/blue}  {decimal_bytes}/{decimal_total_bytes}  {bytes:>7}/{bytes_per_sec}/{total_bytes:7} {wide_msg}")
            .progress_chars("##-"),
    );
    // pb1.format_state(); format_style list all possible value.
    pb1.set_message("hello message.");
    pb1.set_prefix(&format!("[{}/?]", 33));

    let pb2 = multi_bar.add(ProgressBar::new(count));

    for _ in 0..count {
        pb1.inc(1000);
        pb2.inc(1);
        thread::sleep(Duration::from_millis(200));
    }
    pb1.finish();
    pb2.finish();

    if let Err(err) = multi_bar.join_and_clear() {
        println!("join_and_clear failed: {:?}", err);
    }
    Ok(())
}