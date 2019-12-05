    // #[test]
    // fn t_sync_push_dirs() -> Result<(), failure::Error> {
    //     log();
    //     let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::ActiveLeaf);
    //     app_conf.mini_app_conf.verbose = true;

    //     assert!(app_conf.mini_app_conf.app_role == Some(AppRole::ActiveLeaf));
    //     let mut server = tutil::load_demo_server_sqlite(&app_conf, None);

    //     let db_file = server.get_db_file();

    //     if db_file.exists() {
    //         fs::remove_file(db_file.as_path())?;
    //     }

    //     let sqlite_db_access = SqliteDbAccess::new(db_file);
    //     sqlite_db_access.create_database()?;
    //     server.set_db_access(sqlite_db_access);

    //     server.connect()?;
    //     let cc = server.count_remote_files()?;
    //     assert_eq!(cc, 1, "should have 1 files at server side.");
    //     let a_dir = &server
    //         .server_yml
    //         .directories
    //         .iter()
    //         .find(|d| d.to_dir.ends_with("a-dir"))
    //         .expect("should have a directory who's to_dir end with 'a-dir'")
    //         .to_dir;

    //     if a_dir.exists() {
    //         info!("directories path: {:?}", a_dir.as_path());
    //         fs::remove_dir_all(a_dir.as_path())?;
    //     }

    //     let mut indicator = Indicator::new(None);
    //     let stats = server.sync_push_dirs(&mut indicator)?;
    //     indicator.pb_finish();
    //     info!("result {:?}", stats);
    //     info!("a_dir is {:?}", a_dir);
    //     let cc_txt = a_dir.join("b").join("c c").join("c c .txt");
    //     info!("cc_txt is {:?}", cc_txt);
    //     assert!(cc_txt.exists());
    //     assert!(a_dir.join("b").join("b.txt").exists());
    //     assert!(a_dir.join("b b").join("b b.txt").exists());
    //     assert!(a_dir.join("a.txt").exists());
    //     assert!(a_dir.join("qrcode.png").exists());
    //     Ok(())
    // }

    /// pull downed files were saved in the ./data/pull-servers-data directory.
    /// remote generated file_list_file was saved in the 'file_list_file' property of server.yml point to.
    /// This test also involved compiled executable so remember to compile the app if result is not expected.
    // #[test]
    // fn t_sync_pull_dirs() -> Result<(), failure::Error> {
    //     log();
    //     let mut app_conf = tutil::load_demo_app_conf_sqlite(None, AppRole::PullHub);
    //     app_conf.mini_app_conf.verbose = true;

    //     assert!(app_conf.mini_app_conf.app_role == Some(AppRole::PullHub));
    //     let mut server = tutil::load_demo_server_sqlite(&app_conf, None);

    //     // it's useless. because the db file is from remote server's perspective.
    //     let db_file = server.get_db_file();
    //     if db_file.exists() {
    //         fs::remove_file(db_file.as_path())?;
    //     }
    //     let remote_db_path =
    //         Path::new("./target/debug/data/passive-leaf-data/demo-app-instance-id/db.db");

    //     if remote_db_path.exists() {
    //         fs::remove_file(remote_db_path)?;
    //     }

    //     let sqlite_db_access = SqliteDbAccess::new(db_file);
    //     sqlite_db_access.create_database()?;
    //     server.set_db_access(sqlite_db_access);

    //     let a_dir = server
    //         .server_yml
    //         .directories
    //         .iter()
    //         .find(|dir| dir.to_dir.ends_with("a-dir"))
    //         .expect("should have a directory who's to_dir end with 'a-dir'")
    //         .from_dir
    //         .clone();

    //     let a_dir = a_dir.as_path();
    //     if a_dir.exists() {
    //         info!("remove directory: {:?}", a_dir);
    //         fs::remove_dir_all(a_dir)?;
    //     }

    //     let mut indicator = Indicator::new(None);
    //     server.connect()?;
    //     let stats = server.sync_pull_dirs(&mut indicator, false)?;
    //     indicator.pb_finish();
    //     info!("result {:?}", stats);
    //     info!("a_dir is {:?}", a_dir);
    //     let cc_txt = a_dir.join("b").join("c c").join("c c .txt");
    //     info!("cc_txt is {:?}", cc_txt);
    //     assert!(cc_txt.exists());
    //     assert!(a_dir.join("b").join("b.txt").exists());
    //     assert!(a_dir.join("b b").join("b b.txt").exists());
    //     assert!(a_dir.join("a.txt").exists());
    //     assert!(a_dir.join("qrcode.png").exists());
    //     // t.join().unwrap();
    //     Ok(())
    // }

    #[test]
    fn t_bzip2() -> Result<(), failure::Error> {
        log();
        let vc = ['c'; 10000];
        let s: String = vc.iter().collect();
        let test_dir = tutil::create_a_dir_and_a_file_with_content("a", &s)?;

        let out = test_dir.open_an_empty_file_for_write("xx.bzip2")?;

        let mut encoder = BzEncoder::new(out, Compression::Best);

        let mut read_in = test_dir.open_a_file_for_read("a")?;

        io::copy(&mut read_in, &mut encoder)?;
        encoder.try_finish()?;
        test_dir.assert_file_exists("xx.bzip2");
        let of = test_dir.get_file_path("xx.bzip2");
        info!(
            "len: {}, ratio: {:?}",
            of.metadata().expect("sg").len(),
            (encoder.total_out() as f64 / encoder.total_in() as f64) * 100_f64
        );

        let out = test_dir.open_an_empty_file_for_write("b.txt")?;

        let mut decoder = BzDecoder::new(out);

        let mut read_in = test_dir.open_a_file_for_read("xx.bzip2")?;

        io::copy(&mut read_in, &mut decoder)?;
        decoder.try_finish()?;

        assert_eq!(
            test_dir.get_file_path("b.txt").metadata()?.len(),
            10000,
            "len should equal to file before compress."
        );

        Ok(())
    }

    #[test]
    fn t_glob() -> Result<(), failure::Error> {
        log();
        let ptn1 = Pattern::new("a/")?;
        assert!(!ptn1.matches("xa/bc"));
        let ptn1 = Pattern::new("?a/*")?;
        assert!(ptn1.matches("xa/bc"));

        let ptn1 = Pattern::new("**/a/b/**")?;
        assert!(ptn1.matches("x/a/b/c"));

        let ptn1 = Pattern::new("**/c/a/**")?;
        let p1 = Path::new("xy/c/a/3.txt");

        assert!(ptn1.matches_path(p1));

        Ok(())
    }

    // #[test]
    // fn t_from_path() -> Result<(), failure::Error> {
    //     log_util::setup_logger_empty();
    //     let mut cur = tutil::get_a_cursor_writer();
    //     let mut one_dir = Directory {
    //         to_dir: SlashPath::new("fixtures/a-dir"),
    //         ..Directory::default()
    //     };

    //     one_dir.compile_patterns()?;
    //     one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
    //     let num = tutil::count_cursor_lines(&mut cur);
    //     assert_eq!(num, 8);
    //     tutil::print_cursor_lines(&mut cur);

    //     let mut cur = tutil::get_a_cursor_writer();
    //     let mut one_dir = Directory {
    //         to_dir: SlashPath::new("fixtures/a-dir"),
    //         includes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
    //         ..Directory::default()
    //     };

    //     one_dir.compile_patterns()?;
    //     assert!(one_dir.excludes_patterns.is_none());
    //     one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
    //     let num = tutil::count_cursor_lines(&mut cur);
    //     assert_eq!(num, 2); // one dir line, one file line.

    //     let mut cur = tutil::get_a_cursor_writer();
    //     let mut one_dir = Directory {
    //         to_dir: SlashPath::new("fixtures/a-dir"),
    //         excludes: vec!["**/fixtures/a-dir/b/b.txt".to_string()],
    //         ..Directory::default()
    //     };

    //     one_dir.compile_patterns()?;
    //     assert!(one_dir.includes_patterns.is_none());
    //     one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
    //     let num = tutil::count_cursor_lines(&mut cur);
    //     assert_eq!(num, 7, "if exclude 1 file there should 7 left.");

    //     let mut cur = tutil::get_a_cursor_writer();
    //     let mut one_dir = Directory {
    //         to_dir: SlashPath::new("fixtures/a-dir"),
    //         excludes: vec!["**/Tomcat6/logs/**".to_string()],
    //         ..Directory::default()
    //     };

    //     one_dir.compile_patterns()?;
    //     assert!(one_dir.includes_patterns.is_none());
    //     one_dir.load_relative_item(Some(&AppRole::PassiveLeaf), &mut cur, true)?;
    //     let num = tutil::count_cursor_lines(&mut cur);
    //     assert_eq!(num, 7, "if exclude logs file there should 7 left.");

    //     Ok(())
    // }
    #[test]
    fn t_main_password() {
        // Connect to the local SSH server
        let tcp = TcpStream::connect("127.0.0.1:22").unwrap();
        let mut sess = ssh2::Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake().unwrap();

        sess.userauth_password("Administrator", "pass.")
            .expect("should authenticate succeeded.");
        assert!(sess.authenticated(), "should authenticate succeeded.");
    }

    #[test]
    fn t_pb_in_action() -> Result<(), failure::Error> {
        let progress_bar = ProgressBar::new(1000);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{prefix}[{elapsed_precise}] {bar:40.cyan/blue} {bytes:>7}/{total_bytes:7} {bytes_per_sec} {msg}")
                .progress_chars("##-"),
        );
        let three = std::time::Duration::from_millis(10);
        progress_bar.set_message("a");
        for _ in 0..1000 {
            progress_bar.inc(1);
            std::thread::sleep(three);
        }
        progress_bar.reset();
        progress_bar.set_length(2000);
        progress_bar.set_message("hello b");
        for _ in 0..2000 {
            progress_bar.inc(1);
            std::thread::sleep(three);
        }
        progress_bar.finish();
        Ok(())
    }
}

// fn accumulate_file_process(
//     mut accu: FileItemProcessResultStats,
//     item: FileItemProcessResult,
// ) -> FileItemProcessResultStats {
//     match item {
//         FileItemProcessResult::DeserializeFailed(_) => accu.deserialize_failed += 1,
//         FileItemProcessResult::Skipped(_) => accu.skipped += 1,
//         FileItemProcessResult::NoCorrespondedLocalDir(_) => accu.no_corresponded_from_dir += 1,
//         FileItemProcessResult::Directory(_) => accu.directory += 1,
//         FileItemProcessResult::LengthNotMatch(_) => accu.length_not_match += 1,
//         FileItemProcessResult::Sha1NotMatch(_) => accu.sha1_not_match += 1,
//         FileItemProcessResult::CopyFailed(_) => accu.copy_failed += 1,
//         FileItemProcessResult::SkipBecauseNoBaseDir => accu.skip_because_no_base_dir += 1,
//         FileItemProcessResult::Succeeded(fl, _, _) => {
//             accu.bytes_transferred += fl;
//             accu.succeeded += 1;
//         }
//         FileItemProcessResult::GetLocalPathFailed => accu.get_local_path_failed += 1,
//         FileItemProcessResult::SftpOpenFailed => accu.sftp_open_failed += 1,
//         FileItemProcessResult::ScpOpenFailed => accu.scp_open_failed += 1,
//         FileItemProcessResult::MayBeNoParentDir(_) => (),
//     };
//     accu
// }

// pub fn push_a_file_item_sftp(
//     sftp: &ssh2::Sftp,
//     file_item_primary: PrimaryFileItem,
//     buf: &mut [u8],
//     progress_bar: &mut Indicator,
// ) -> FileItemProcessResult {
//     progress_bar.init_item_pb_style_1(
//         file_item_primary.get_local_path().as_str(),
//         file_item_primary.relative_item.get_len(),
//     );
//     trace!(
//         "staring create remote file: {}.",
//         file_item_primary.get_remote_path()
//     );
//     match sftp.create(file_item_primary.get_remote_path().as_path()) {
//         Ok(mut ssh_file) => {
//             let local_file_path = file_item_primary.get_local_path();
//             trace!(
//                 "coping {} to {}.",
//                 local_file_path,
//                 file_item_primary.get_remote_path()
//             );
//             match local_file_path.get_local_file_reader() {
//                 Ok(mut local_reader) => {
//                     match copy_file::copy_stream_with_pb(
//                         &mut local_reader,
//                         &mut ssh_file,
//                         buf,
//                         progress_bar,
//                     ) {
//                         Ok(length) => {
//                             if length != file_item_primary.get_relative_item().get_len() {
//                                 FileItemProcessResult::LengthNotMatch(local_file_path.get_slash())
//                             } else {
//                                 FileItemProcessResult::Succeeded(
//                                     length,
//                                     local_file_path.get_slash(),
//                                     SyncType::Sftp,
//                                 )
//                             }
//                         }
//                         Err(err) => {
//                             error!("write_stream_to_file failed: {:?}", err);
//                             FileItemProcessResult::CopyFailed(local_file_path.get_slash())
//                         }
//                     }
//                 }
//                 Err(_err) => FileItemProcessResult::GetLocalPathFailed,
//             }
//         }
//         Err(err) => {
//             error!("sftp create failed: {:?}", err);
//             if err.code() == 2 {
//                 error!("sftp create failed return code 2.");
//                 FileItemProcessResult::MayBeNoParentDir(file_item_primary)
//             } else {
//                 FileItemProcessResult::SftpOpenFailed
//             }
//         }
//     }
// }

// pub fn count_local_files(&self) -> u64 {
//     self.server_yml
//         .directories
//         .iter()
//         .filter_map(|dir| dir.count_local_files(self.app_conf.app_role.as_ref()).ok())
//         .count() as u64
// }

// pub fn count_remote_files(&self) -> Result<u64, failure::Error> {
//     let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
//         match app_role {
//             _ => bail!(
//                 "create_remote_files: unsupported app role. {:?}",
//                 self.app_conf.app_role
//             ),
//         }
//     } else {
//         bail!("no app_role whne count_remote_files");
//     };
//     let mut channel: ssh2::Channel = self.create_channel()?;
//     let cmd = format!(
//         "{} {} {} --app-instance-id {} --app-role {}  count-local-files {}",
//         self.get_remote_exec(),
//         if self.app_conf.console_log {
//             "--console-log"
//         } else {
//             ""
//         },
//         if self.app_conf.verbose { "--vv" } else { "" },
//         self.app_conf.app_instance_id,
//         app_role.to_str(),
//         self.get_remote_server_yml(),
//     );
//     info!("invoking remote command: {:?}", cmd);
//     channel.exec(cmd.as_str())?;
//     let ss = ssh_util::get_stdout_eprintln_stderr(&mut channel, true);
//     Ok(ssh_util::parse_scalar_value(ss)
//         .unwrap_or_else(|| "0".to_string())
//         .parse()?)
// }

// located in the data/passive-leaf-data/{self.app_conf.app_instance_id}/file_list_file.txt
// pub fn get_passive_leaf_file_list_file(&self) -> String {
//     let yml = format!(
//         "/data/{}/{}/{}",
//         app_conf::PASSIVE_LEAF_DATA,
//         self.app_conf.app_instance_id,
//         FILE_LIST_FILE_NAME,
//     );
//     self.get_remote_exec()
//         .parent()
//         .expect("the remote executable's parent directory should exist")
//         .join(yml)
//         .slash
// }

// pub fn get_active_leaf_file_list_file(&self) -> PathBuf {
//     self.my_dir.join(FILE_LIST_FILE_NAME)
// }

// pub fn remove_working_file_list_file(&self) {
//     let wf = self.get_working_file_list_file();
//     if let Err(err) = fs::remove_file(&wf) {
//         error!(
//             "delete working file list file failed: {:?}, {:?}",
//             self.get_working_file_list_file(),
//             err
//         );
//     }
// }

// /// We temporarily save file list file at 'file_list_file' property of server.yml.
// pub fn list_remote_file_sftp(&self) -> Result<PathBuf, failure::Error> {
//     let mut channel: ssh2::Channel = self.create_channel()?;
//     let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
//         match app_role {
//             AppRole::PullHub => AppRole::PassiveLeaf,
//             _ => bail!(
//                 "list_remote_file_sftp: unsupported app role. {:?}",
//                 self.app_conf.app_role
//             ),
//         }
//     } else {
//         bail!("no app_role when list_remote_file_sftp");
//     };
//     let cmd = format!(
//         "{} {} --app-instance-id {} --app-role {} list-local-files {} --out {}",
//         self.get_remote_exec(),
//         if self.is_skip_sha1() {
//             ""
//         } else {
//             "--enable-sha1"
//         },
//         self.app_conf.app_instance_id,
//         app_role.to_str(),
//         self.get_remote_server_yml(),
//         self.get_passive_leaf_file_list_file(),
//     );
//     trace!("invoking list remote files by sftp command: {:?}", cmd);
//     channel.exec(cmd.as_str())?;
//     let (std_out, std_err) =
//         ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);

//     let sftp = self.session.as_ref().unwrap().sftp()?;

//     if std_err.find("server yml file doesn").is_some()
//         || std_out.find("server yml file doesn").is_some()
//     {
//         // now copy server yml to remote.
//         let yml_location = self
//             .yml_location
//             .as_ref()
//             .expect("yml_location should exist")
//             .to_str()
//             .expect("yml_location to_str should succeeded.");
//         if let Err(err) = copy_a_file_sftp(&sftp, yml_location, self.get_remote_server_yml()) {
//             bail!("sftp copy failed: {:?}", err);
//         }

//         // execute cmd again.
//         let mut channel: ssh2::Channel = self.create_channel()?;
//         channel.exec(cmd.as_str())?;
//         ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
//     }

//     let mut f = sftp.open(Path::new(&self.get_passive_leaf_file_list_file().as_str()))?;

//     let working_file = self.get_working_file_list_file();
//     let mut wf = fs::OpenOptions::new()
//         .create(true)
//         .truncate(true)
//         .write(true)
//         .open(&working_file)?;
//     io::copy(&mut f, &mut wf)?;
//     Ok(working_file)
// }

// pub fn create_remote_db(
//     &self,
//     db_type: impl AsRef<str>,
//     force: bool,
// ) -> Result<(), failure::Error> {
//     let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
//         match app_role {
//             AppRole::PullHub => AppRole::PassiveLeaf,
//             _ => bail!(
//                 "create_remote_db: unsupported app role. {:?}",
//                 self.app_conf.app_role
//             ),
//         }
//     } else {
//         bail!("no app_role when create_remote_db");
//     };
//     let mut channel: ssh2::Channel = self.create_channel()?;
//     let db_type = db_type.as_ref();
//     let cmd = format!(
//         "{} {} {} --app-instance-id {} --app-role {}  create-db {} --db-type {}{}",
//         self.get_remote_exec(),
//         if self.app_conf.console_log {
//             "--console-log"
//         } else {
//             ""
//         },
//         if self.app_conf.verbose { "--vv" } else { "" },
//         self.app_conf.app_instance_id,
//         app_role.to_str(),
//         self.get_remote_server_yml(),
//         db_type,
//         if force { " --force" } else { "" },
//     );
//     info!("invoking remote command: {:?}", cmd);
//     channel.exec(cmd.as_str())?;
//     ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
//     Ok(())
// }

// pub fn confirm_remote_sync(&self) -> Result<(), failure::Error> {
//     let mut channel: ssh2::Channel = self.create_channel()?;
//     let app_role = if let Some(app_role) = self.app_conf.app_role.as_ref() {
//         match app_role {
//             AppRole::PullHub => AppRole::PassiveLeaf,
//             _ => bail!("there is no need to confirm remote sync."),
//         }
//     } else {
//         bail!("no app_role when confirm_remote_sync");
//     };
//     let cmd = format!(
//         "{} --app-instance-id {} --app-role {} confirm-local-sync {}",
//         self.get_remote_exec(),
//         self.app_conf.app_instance_id,
//         app_role.to_str(),
//         self.get_remote_server_yml(),
//     );
//     info!("invoking remote command: {:?}", cmd);
//     channel.exec(cmd.as_str())?;
//     ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
//     Ok(())
// }

// pub fn confirm_local_sync(&self) -> Result<(), failure::Error> {
//     trace!("confirm sync, db file is: {:?}", self.get_db_file());
//     let confirm_num = if let Some(db_access) = self.db_access.as_ref() {
//         db_access.confirm_all()?
//     } else {
//         0
//     };
//     println!("{}", confirm_num);
//     Ok(())
// }
// / This method only used for command line list remote files. We use list_remote_file_sftp in actual sync task.
// pub fn list_remote_file_exec(&self, no_db: bool) -> Result<PathBuf, failure::Error> {
//     let mut channel: ssh2::Channel = self.create_channel()?;
//     let cmd = format!(
//         "{} {} --app-instance-id {} --app-role {} {} list-local-files {}",
//         self.get_remote_exec(),
//         if self.is_skip_sha1() {
//             ""
//         } else {
//             "--enable-sha1"
//         },
//         self.app_conf.app_instance_id,
//         self.app_conf
//             .app_role
//             .as_ref()
//             .unwrap_or(&AppRole::PullHub)
//             .to_str(),
//         if no_db { " --no-db" } else { "" },
//         self.get_remote_server_yml(),
//     );
//     info!("invoking list remote files command: {:?}", cmd);
//     channel.exec(cmd.as_str())?;
//     let working_file = self.get_working_file_list_file();
//     let mut wf = fs::OpenOptions::new()
//         .create(true)
//         .truncate(true)
//         .write(true)
//         .open(&working_file)?;
//     io::copy(&mut channel, &mut wf)?;
//     Ok(working_file)
// }

// fn count_and_len(&self, input: &mut (impl io::BufRead + Seek)) -> (u64, u64) {
//     let mut count_and_len = (0u64, 0u64);
//     loop {
//         let mut buf = String::new();
//         match input.read_line(&mut buf) {
//             Ok(0) => break,
//             Ok(_length) => {
//                 if buf.starts_with('{') {
//                     match serde_json::from_str::<RelativeFileItem>(&buf) {
//                         Ok(remote_item) => {
//                             count_and_len.0 += 1;
//                             count_and_len.1 += remote_item.get_len();
//                         }
//                         Err(err) => {
//                             error!("deserialize cursor line failed: {}, {:?}", buf, err);
//                         }
//                     };
//                 }
//             }
//             Err(err) => {
//                 error!("read line from cursor failed: {:?}", err);
//                 break;
//             }
//         };
//     }
//     count_and_len
// }

// / Preparing file list includes invoking remote command to collect file list and downloading to local.
// pub fn prepare_file_list(&self) -> Result<(), failure::Error> {
//     if self.get_working_file_list_file().exists() {
//         eprintln!(
//             "uncompleted list file exists: {:?} continue processing",
//             self.get_working_file_list_file()
//         );
//     } else {
//         self.list_remote_file_sftp()?;
//     }
//     Ok(())
// }

// fn init_total_progress_bar(
//     &self,
//     progress_bar: &mut Indicator,
//     file_list_file: impl AsRef<Path>,
// ) -> Result<(), failure::Error> {
//     let count_and_len_op = if progress_bar.is_some() {
//         let mut wfb = io::BufReader::new(fs::File::open(file_list_file.as_ref())?);
//         Some(self.count_and_len(&mut wfb))
//     } else {
//         None
//     };
//     let total_count = count_and_len_op.map(|cl| cl.0).unwrap_or_default();
//     progress_bar.count_total = total_count;

//     progress_bar.active_pb_total().alter_pb(PbProperties {
//         set_style: Some(ProgressStyle::default_bar().template("{prefix} {bytes_per_sec: 11} {decimal_bytes:>11}/{decimal_total_bytes} {bar:30.cyan/blue} {percent}% {eta}").progress_chars("#-")),
//         set_length: count_and_len_op.map(|cl| cl.1),
//         ..PbProperties::default()
//     });
//     progress_bar.active_pb_item().alter_pb(PbProperties {
//         set_style: Some(ProgressStyle::default_bar().template("{bytes_per_sec:10} {decimal_bytes:>8}/{decimal_total_bytes:8} {spinner} {percent:>4}% {eta:5} {wide_msg}").progress_chars("#-")),
//         ..PbProperties::default()
//     });
//     Ok(())
// }

// fn start_pull_sync_working_file_list(
//     &self,
//     pb: &mut Indicator,
// ) -> Result<FileItemProcessResultStats, failure::Error> {
//     self.prepare_file_list()?;
//     let working_file = &self.get_working_file_list_file();
//     let rb = io::BufReader::new(fs::File::open(working_file)?);
//     self.start_pull_sync(rb, pb)
// }

// / First list changed file to file_list_file.
// / Then for each line in file_list_file
// fn start_push_sync_working_file_list(
//     &self,
//     progress_bar: &mut Indicator,
// ) -> Result<FileItemProcessResultStats, failure::Error> {
//     let file_list_file = self.get_active_leaf_file_list_file();

//     {
//         info!("start creating file_list_file: {:?}", file_list_file);
//         let mut o = fs::OpenOptions::new()
//             .write(true)
//             .create(true)
//             .truncate(true)
//             .open(file_list_file.as_path())
//             .expect("file list file should be created.");
//         self.create_file_list_files(&mut o)?;
//     }

//     self.init_total_progress_bar(progress_bar, file_list_file.as_path())?;

//     info!("start reading file_list_file: {:?}", file_list_file);
//     let reader = fs::OpenOptions::new()
//         .read(true)
//         .open(file_list_file.as_path())?;

//     let file_item_directories =
//         FileItemDirectories::<io::BufReader<fs::File>>::from_file_reader(
//             reader,
//             self.get_local_remote_pairs(),
//             AppRole::ActiveLeaf,
//         );

//     let sftp = self.session.as_ref().unwrap().sftp()?;
//     let mut buff = vec![0_u8; self.server_yml.buf_len];

//     let mut result = FileItemProcessResultStats::default();
//     for file_item_dir in file_item_directories {
//         result += file_item_dir
//             .map(|item| {
//                 let file_len = item.get_relative_item().get_len();
//                 let push_result = push_a_file_item_sftp(&sftp, item, &mut buff, progress_bar);
//                 progress_bar.tick_total_pb_style_1(self.get_host(), file_len);
//                 if let FileItemProcessResult::MayBeNoParentDir(item) = push_result {
//                     match self.create_to_dir(
//                         item.get_remote_path()
//                             .parent()
//                             .expect("slash path's parent directory should exist.")
//                             .as_str(),
//                     ) {
//                         Ok(_) => {
//                             info!("push_a_file_item_sftp again.");
//                             push_a_file_item_sftp(&sftp, item, &mut buff, progress_bar)
//                         }
//                         Err(err) => {
//                             error!("create_to_dir failed: {:?}", err);
//                             FileItemProcessResult::SftpOpenFailed
//                         }
//                     }
//                 } else {
//                     push_result
//                 }
//             })
//             .fold(
//                 FileItemProcessResultStats::default(),
//                 accumulate_file_process,
//             );
//     }
//     Ok(result)
// }

// / Do not try to return item stream from this function.
// / consume it locally, pass in function to alter the behavior.
// /
// / Take a reader as parameter, each line may be a directory name or a file name.
// / the file names are relative to last read directory line.
// /
// / # Examples
// /
// / ```no_run
// / use std::fs;
// /
// /
// / ```
// fn start_pull_sync<R: BufRead>(
//     &self,
//     file_item_lines: R,
//     progress_bar: &mut Indicator,
// ) -> Result<FileItemProcessResultStats, failure::Error> {
//     let mut current_to_dir = Option::<String>::None;
//     let mut current_from_dir = Option::<&Path>::None;

//     self.init_total_progress_bar(progress_bar, self.get_working_file_list_file())?;

//     let sftp = self.session.as_ref().unwrap().sftp()?;
//     let mut buff = vec![0_u8; self.server_yml.buf_len];

//     let result = file_item_lines
//         .lines()
//         .filter_map(|li| match li {
//             Err(err) => {
//                 error!("read line failed: {:?}", err);
//                 None
//             }
//             Ok(line) => Some(line),
//         })
//         .map(|line| {
//             if line.starts_with('{') {
//                 trace!("got item line {}", line);
//                 if let (Some(to_dir), Some(from_dir)) =
//                     (current_to_dir.as_ref(), current_from_dir)
//                 {
//                     match serde_json::from_str::<RelativeFileItem>(&line) {
//                         Ok(remote_item) => {
//                             let remote_len = remote_item.get_len();
//                             let sync_type = if self.server_yml.rsync.valve > 0
//                                 && remote_item.get_len() > self.server_yml.rsync.valve
//                             {
//                                 SyncType::Rsync
//                             } else {
//                                 SyncType::Sftp
//                             };
//                             let file_item_map = FileItemMap::new(
//                                 from_dir,
//                                 to_dir.as_str(),
//                                 remote_item,
//                                 sync_type,
//                                 true,
//                             );

//                             progress_bar.init_item_pb_style_1(file_item_map.get_relative_item().get_path(), remote_len);

//                             let mut skipped = false;
//                             // if use_db all received item are changed.
//                             // let r = if self.server_yml.use_db || local_item.had_changed() { // even use_db still check change or not.
//                             let r = if file_item_map.had_changed() {
//                                 trace!("file had changed. start copy_a_file_item.");
//                                 copy_a_file_item(&self, &sftp, file_item_map, &mut buff, progress_bar)
//                             } else {
//                                 skipped = true;
//                                 FileItemProcessResult::Skipped(
//                                     file_item_map.get_local_path_str().expect(
//                                         "get_local_path_str should has some at this point.",
//                                     ),
//                                 )
//                             };

//                             progress_bar.tick_total_pb_style_1(self.get_host(), remote_len);
//                                 if skipped {
//                                     progress_bar.active_pb_item().inc_pb_item(remote_len);
//                                 }
//                             r
//                         }
//                         Err(err) => {
//                             error!("deserialize line failed: {}, {:?}", line, err);
//                             FileItemProcessResult::DeserializeFailed(line)
//                         }
//                     }
//                 } else {
//                     FileItemProcessResult::SkipBecauseNoBaseDir
//                 }
//             } else {
//                 // it's a directory line.
//                 trace!(
//                     "got directory line, it's a remote represent of path, be careful: {:?}",
//                     line
//                 );
//                 let found_directory = self
//                     .server_yml
//                     .directories
//                     .iter()
//                     .find(|dir| dir.to_dir.slash_equal_to(&line));

//                 if let Some(found_directory) = found_directory {
//                     current_to_dir = Some(line.clone());
//                     current_from_dir = Some(found_directory.from_dir.as_path());
//                     FileItemProcessResult::Directory(line)
//                 } else {
//                     // we compare the remote dir line with this server_yml.directories's remote dir
//                     error!(
//                         "this is line from remote: {:?}, this is all to_dir in configuration file: {:?}, no one matches.",
//                         line, self.server_yml
//                             .directories
//                             .iter()
//                             .map(|dir| dir.from_dir.as_str())
//                             .collect::<Vec<&str>>()
//                     );
//                     current_to_dir = None;
//                     current_from_dir = None;
//                     FileItemProcessResult::NoCorrespondedLocalDir(line)
//                 }
//             }
//         })
//         .fold(FileItemProcessResultStats::default(), accumulate_file_process);
//     Ok(result)
// }

// / We can push files to multiple destinations simultaneously.
// pub fn sync_push_dirs(
//     &self,
//     progress_bar: &mut Indicator,
// ) -> Result<Option<SyncDirReport>, failure::Error> {
//     info!(
//         "start sync_push_dirs on server: {} at: {}",
//         self.get_host(),
//         Local::now()
//     );
//     let start = Instant::now();
//     let started_at = Local::now();

//     let rs = self.start_push_sync_working_file_list(progress_bar)?;
//     self.confirm_local_sync()?;
//     Ok(Some(SyncDirReport::new(start.elapsed(), started_at, rs)))
// }
// / If as_service is true, one must connect to server first, and then after executing task close the connection.
// pub fn sync_pull_dirs(
//     &mut self,
//     pb: &mut Indicator,
//     as_service: bool,
// ) -> Result<Option<SyncDirReport>, failure::Error> {
//     info!(
//         "start sync_pull_dirs on server: {} at: {}",
//         self.get_host(),
//         Local::now()
//     );
//     if as_service {
//         self.session.take();
//         self.connect()?;
//     }
//     let start = Instant::now();
//     let started_at = Local::now();
//     let rs = self.start_pull_sync_working_file_list(pb)?;
//     self.remove_working_file_list_file();
//     self.confirm_remote_sync()?;
//     if as_service {
//         if let Some(sess) = self.session.as_mut() {
//             sess.disconnect(None, "", None).ok();
//         }
//         self.session.take();
//     }
//     Ok(Some(SyncDirReport::new(start.elapsed(), started_at, rs)))
// }

// / When in the role of AppRole::ActiveLeaf, it list changed files in the local disk.
// / But it has no way to know the changes happen in the remote side, what if the remote file has been deleted? at that situation it should upload again.
//     pub fn create_file_list_files<O: io::Write>(&self, out: &mut O) -> Result<(), failure::Error> {
//         if self.db_access.is_some() && self.server_yml.use_db {
//             let db_access = self.db_access.as_ref().unwrap();
//             for one_dir in self.server_yml.directories.iter() {
//                 trace!("start load directory: {:?}", one_dir);
//                 one_dir.load_relative_item_to_sqlite(
//                     self.app_conf.app_role.as_ref(),
//                     db_access,
//                     self.is_skip_sha1(),
//                     self.server_yml.sql_batch_size,
//                     self.server_yml.rsync.sig_ext.as_str(),
//                     self.server_yml.rsync.delta_ext.as_str(),
//                 )?;
//                 trace!("load_relative_item_to_sqlite done.");
//                 for sql in self.server_yml.exclude_by_sql.iter() {
//                     if let Err(err) = db_access.exclude_by_sql(sql) {
//                         eprintln!("exclude_by_sql execution failed: {:?}", err);
//                         error!("exclude_by_sql execution failed: {:?}", err);
//                     }
//                 }
//                 trace!("exclude_by_sql done.");
//                 db_access.iterate_files_by_directory_changed_or_unconfirmed(|fi_db_or_path| {
//                     match fi_db_or_path {
//                         (Some(fi_db), None) => {
//                             if fi_db.changed || !fi_db.confirmed {
//                                 match serde_json::to_string(&RelativeFileItem::from(fi_db)) {
//                                     Ok(line) => {
//                                         writeln!(out, "{}", line).ok();
//                                     }
//                                     Err(err) => error!("serialize item line failed: {:?}", err),
//                                 }
//                             }
//                         }
//                         (None, Some(path)) => {
//                             if let Err(err) = writeln!(out, "{}", path) {
//                                 error!("write path failed: {:?}", err);
//                             }
//                         }
//                         _ => {}
//                     }
//                 })?;
//             }
//         } else {
//             for one_dir in self.server_yml.directories.iter() {
//                 one_dir.load_relative_item(
//                     self.app_conf.app_role.as_ref(),
//                     out,
//                     self.is_skip_sha1(),
//                 )?;
//             }
//         }
//         Ok(())
//     }

    // pub fn get_dir_sync_report_file(&self) -> PathBuf {
    //     self.reports_dir.join("sync_dir_report.json")
    // }

    // pub fn get_working_file_list_file(&self) -> PathBuf {
    //     self.working_dir.join("file_list_working.txt")
    // }

    // pub fn create_channel(&self) -> Result<ssh2::Channel, failure::Error> {
    //     Ok(self
    //         .session
    //         .as_ref()
    //         .expect("should already connected.")
    //         .channel_session()
    //         .expect("a channel session."))
    // }

    // pub fn is_skip_sha1(&self) -> bool {
    //     if !self.app_conf.skip_sha1 {
    //         // if force not to skip.
    //         false
    //     } else {
    //         self.server_yml.skip_sha1
    //     }
    // }

    // pub fn create_to_dir(&self, dir: &str) -> Result<(), failure::Error> {
    //     let mut channel: ssh2::Channel = self.create_channel()?;
    //     let dir = base64::encode(dir);
    //     let cmd = format!(
    //         "{} {} {} mkdir {}",
    //         self.get_remote_exec(),
    //         if self.app_conf.console_log {
    //             "--console-log"
    //         } else {
    //             ""
    //         },
    //         if self.app_conf.verbose { "--vv" } else { "" },
    //         dir,
    //     );
    //     info!("invoking remote command: {}", cmd);
    //     channel.exec(cmd.as_str())?;
    //     ssh_util::get_stdout_eprintln_stderr(&mut channel, self.app_conf.verbose);
    //     Ok(())
    // }

    // fn get_local_remote_pairs(&self) -> Vec<(SlashPath, SlashPath)> {
    //     self.server_yml
    //         .directories
    //         .iter()
    //         .map(|dir| (dir.from_dir.clone(), dir.to_dir.clone()))
    //         .collect()
    // }
