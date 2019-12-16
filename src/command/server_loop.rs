use crate::data_shape::{FileChanged, FullPathFileItem, ServerYml, SlashPath};
use crate::protocol::{MessageHub, StdInOutMessageHub, StringMessage, TransferType, U64Message};
use dirs;
use filetime;
use log::*;
use std::io::{self};

/// how to determine the directories? it's in the user's home directory.
pub fn server_receive_loop() -> Result<(), failure::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin_handler = stdin.lock();
    let stdout_handler = stdout.lock();

    // home_dir joins app_instance_id.
    let home_dir = SlashPath::from_path(
        dirs::home_dir()
            .expect("get home_dir")
            .as_path()
            .join("directories")
            .as_path(),
        &vec![],
    )
    .expect("get slash path from home_dir");

    let mut server_yml_op: Option<ServerYml> = None;

    let mut message_hub = StdInOutMessageHub::new(stdin_handler, stdout_handler);
    trace!("protocol reader ready.");

    match message_hub.read_type_byte()? {
        TransferType::ServerYml => {
            let string_message = StringMessage::parse(&mut message_hub)?;
            trace!("got server_yml content: {}", string_message.content);
            match serde_yaml::from_str::<ServerYml>(&string_message.content) {
                Ok(server_yml) => {
                    server_yml_op.replace(server_yml);
                }
                Err(err) => {
                    error!("parse error: {:?}", err);
                    panic!("parse error: {:?}", err);
                }
            }
        }
        t => {
            error!("unimplement transfer type: {}", t.to_u8());
            panic!("unimplement transfer type: {}", t.to_u8());
        }
    }
    trace!("server yml read.");

    let mut last_df: Option<SlashPath> = None;
    let mut last_file_item: Option<FullPathFileItem> = None;
    let mut buf = vec![0; 8192];
    // after read server_yml, we wait the other side to send file items.
    loop {
        let type_byte = match message_hub.read_type_byte() {
            Err(err) => {
                error!("got error type byte: {}", err);
                break;
            }
            Ok(type_byte) => type_byte,
        };

        match type_byte {
            TransferType::FileItem => {
                let string_message = StringMessage::parse(&mut message_hub)?;
                trace!("got file item: {}", string_message.content);
                match serde_json::from_str::<FullPathFileItem>(&string_message.content) {
                    Ok(file_item) => {
                        let df = home_dir.join_another(&file_item.to_path); // use to path.
                        match file_item.changed(df.as_path()) {
                            FileChanged::NoChange => {
                                message_hub
                                    .write_transfer_type_only(TransferType::FileItemUnchanged)?;
                            }
                            fc => {
                                let string_message = StringMessage::new(format!("{:?}", fc));
                                message_hub.write_and_flush(
                                    &string_message.as_string_sent_bytes_with_header(
                                        TransferType::FileItemChanged,
                                    ),
                                )?;
                                last_df.replace(df);
                                last_file_item.replace(file_item);
                            }
                        }
                    }
                    Err(err) => {
                        message_hub.write_error_message(format!("{:?}", err))?;
                    }
                };
            }
            TransferType::StartSend => {
                let content_len = U64Message::parse(&mut message_hub)?;
                if let (Some(df), Some(file_item)) = (last_df.take(), last_file_item.take()) {
                    trace!("copy to file: {:?}", df.as_path());
                    match message_hub.copy_to_file(&mut buf, content_len.value, df.as_path(), None)
                    {
                        Err(err) => {
                            message_hub.write_error_message(format!("{:?}", err))?;
                        }
                        Ok(()) => {
                            if let Some(md) = file_item.modified {
                                let ft = filetime::FileTime::from_unix_time(md as i64, 0);
                                filetime::set_file_mtime(df.as_path(), ft)?;
                            } else {
                                message_hub.write_error_message(
                                    "push_primary_file_item has no modified value.",
                                )?;
                            }
                        }
                    }
                } else {
                    error!("the other side start send content, but the last_df is empty.");
                }
            }
            TransferType::RepeatDone | TransferType::Eof => {
                info!("got eof, exiting.");
                break;
            }
            t => {
                error!("unhandled transfer type: {:?}", t);
                panic!("unimplement transfer type.");
            }
        }
    }
    Ok(())
}

/// When sending the iteration happened at server side which may meet malformed path names.
/// send these malfromed path names to the client side, log it, analysys it, solve it.
pub fn server_send_loop(skip_sha1: bool) -> Result<(), failure::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin_handler = stdin.lock();
    let stdout_handler = stdout.lock();

    let mut server_yml_op: Option<ServerYml> = None;

    let mut message_hub = StdInOutMessageHub::new(stdin_handler, stdout_handler);
    trace!("protocol reader ready.");

    match message_hub.read_type_byte()? {
        TransferType::ServerYml => {
            let string_message = StringMessage::parse(&mut message_hub)?;
            trace!("got server_yml content: {}", string_message.content);
            match serde_yaml::from_str::<ServerYml>(&string_message.content) {
                Ok(server_yml) => {
                    server_yml_op.replace(server_yml);
                }
                Err(err) => {
                    error!("parse error: {:?}", err);
                    panic!("parse error: {:?}", err);
                }
            }
        }
        t => {
            error!("unimplement transfer type: {}", t.to_u8());
            panic!("unimplement transfer type: {}", t.to_u8());
        }
    }
    trace!("server yml read.");
    // after read server_yml, we start send file items to other side.
    let mut server_yml = server_yml_op.expect("server_yml should already received.");

    for dir in server_yml.directories.iter_mut() {
        dir.compile_patterns()?;
    }

    // let count: u64 = server_yml
    //     .directories
    //     .iter()
    //     .map(|dir| dir.file_item_iter("", false).count() as u64)
    //     .sum();
    // let u64_message = U64Message::new(count);
    // message_hub.write_and_flush(&u64_message.as_bytes())?;
    // trace!("file count sent.");

    // let server_distinct_id = format!("{}/directories", server_yml.host);
    let mut buf = vec![0; 8192];

    let possible_encoding = server_yml.get_possible_encoding();

    for dir in server_yml.directories.iter() {
        trace!("start proceess directory: {:?}", dir);
        let push_file_items = dir.file_item_iter("", skip_sha1, &possible_encoding);
        for fi in push_file_items {
            match fi {
                Ok(fi) => {
                    message_hub.write_and_flush(&fi.as_sent_bytes())?;
                    match message_hub.read_type_byte().expect("read type byte.") {
                        TransferType::FileItemChanged => {
                            let change_message = StringMessage::parse(&mut message_hub)?;
                            trace!("changed file: {}.", change_message.content);
                            message_hub.copy_from_file(&mut buf, &fi, None)?;
                            trace!("send file content done.");
                        }
                        TransferType::FileItemUnchanged => {
                            trace!("unchanged file.");
                        }
                        TransferType::StringError => {
                            let ss = StringMessage::parse(&mut message_hub)?;
                            error!("string error: {:?}", ss.content);
                        }
                        i => error!("got unexpected transfer type {:?}", i),
                    }
                }
                Err(e) => {
                    message_hub.write_error_message(format!("{}", e))?;
                    error!("{:?}", e);
                }
            }
        }
    }
    message_hub.write_and_flush(&[TransferType::RepeatDone.to_u8()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use encoding_rs::*;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn t_sample_string() -> Result<(), failure::Error> {
        let bytes: Vec<u8> = vec![
            47, 117, 115, 114, 47, 11, 08, 111, 99, 97, 108, 47, 102, 104, 113, 108, 47, 87, 101,
            98, 82, 111, 111, 116, 47, 87, 69, 66, 45, 73, 78, 70, 47, 99, 108, 97, 115, 115, 101,
            115, 47, 99, 111, 109, 47, 108, 105, 110, 101, 119, 101, 108, 108, 47, 119, 97, 115,
            47, 103, 114, 97, 100, 101, 47, 101, 110, 103, 105, 110, 101, 47, 112, 97, 114, 97,
            109, 115, 47, 229, 174, 161, 230, 137, 185, 228, 186, 139, 233, 161, 185, 231, 138,
            182, 230, 128, 95, 99, 108, 97, 115, 115,
        ];
        let (cow, _encoding_used, had_errors) = GBK.decode(&bytes[..]);
        eprintln!("{}", cow);
        eprintln!("{}", had_errors);
        let (cow, _encoding_used, had_errors) = UTF_8.decode(&bytes[..]);
        eprintln!("{}", cow);
        eprintln!("{}", had_errors);

        Ok(())
    }

    #[test]
    fn t_osstr_path() -> Result<(), failure::Error> {
        #[cfg(any(unix, target_os = "redox"))]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;

            // Here, the values 0x66 and 0x6f correspond to 'f' and 'o'
            // respectively. The value 0x80 is a lone continuation byte, invalid
            // in a UTF-8 sequence.
            let source = [0x66, 0x6f, 0x80, 0x6f];
            let os_str = OsStr::from_bytes(&source[..]);

            assert_eq!(os_str.to_string_lossy(), "fo�o");
        }
        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::prelude::*;

            // Here the values 0x0066 and 0x006f correspond to 'f' and 'o'
            // respectively. The value 0xD800 is a lone surrogate half, invalid
            // in a UTF-16 sequence.
            let source = [0x0066, 0x006f, 0xD800, 0x006f];
            let os_string = OsString::from_wide(&source[..]);
            let os_str = os_string.as_os_str();

            assert_eq!(os_str.to_string_lossy(), "fo�o");
        }

        use encoding_rs::*;

        let expectation = "\u{30CF}\u{30ED}\u{30FC}\u{30FB}\u{30EF}\u{30FC}\u{30EB}\u{30C9}";
        let bytes = b"\x83n\x83\x8D\x81[\x81E\x83\x8F\x81[\x83\x8B\x83h";

        let (cow, encoding_used, had_errors) = SHIFT_JIS.decode(bytes);
        eprintln!("{}", cow);
        eprintln!("{}", expectation);
        assert_eq!(&cow[..], expectation);
        assert_eq!(encoding_used, SHIFT_JIS);
        assert!(!had_errors);
        // let os_str = OsStr::from_bytes(bytes);
        // let path = Path::new(os_str);

        // walkdir::DirEntry path()

        let path = Path::new("abc");
        let os_str = path.as_os_str();

        #[cfg(any(unix, target_os = "redox"))]
        {
            use std::os::unix::ffi::OsStrExt;
            let bytes = os_str.as_bytes();
            let (cow, encoding_used, had_errors) = SHIFT_JIS.decode(bytes);
        }

        #[cfg(windows)]
        {
            use std::os::windows::ffi::EncodeWide;
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::ffi::OsStringExt;

            let source = [0x0055, 0x006E, 0x0069, 0x0063, 0x006F, 0x0064, 0x0065];
            // Re-encodes an OsStr as a wide character sequence, i.e., potentially ill-formed UTF-16
            let string = OsString::from_wide(&source[..]);

            eprintln!("from_wide: {:?}", string);

            let result: Vec<u16> = string.encode_wide().collect();
            assert_eq!(&source[..], &result[..]);
        }

        Ok(())
    }
}
