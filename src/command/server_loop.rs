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

    let count: u64 = server_yml
        .directories
        .iter()
        .map(|dir| dir.file_item_iter("", false).count() as u64)
        .sum();
    let u64_message = U64Message::new(count);
    message_hub.write_and_flush(&u64_message.as_bytes())?;
    trace!("file count sent.");

    // let server_distinct_id = format!("{}/directories", server_yml.host);
    let mut buf = vec![0; 8192];
    for dir in server_yml.directories.iter() {
        trace!("start proceess directory: {:?}", dir);
        let push_file_items = dir.file_item_iter("", skip_sha1);
        for fi in push_file_items {
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
    }
    message_hub.write_and_flush(&[TransferType::RepeatDone.to_u8()])?;
    Ok(())
}
