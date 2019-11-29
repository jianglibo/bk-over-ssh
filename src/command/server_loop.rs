use crate::data_shape::{FileChanged, PushPrimaryFileItem, ServerYml, SlashPath};
use crate::protocol::{MessageHub, StdInOutMessageHub, StringMessage, TransferType, U64Message};
use dirs;
use filetime;
use log::*;
use std::io::{self, StdoutLock, Write};

/// how to determine the directories? it's in the user's home directory.
pub fn server_receive_loop() -> Result<(), failure::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin_handler = stdin.lock();
    let mut stdout_handler = stdout.lock();

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
    let mut last_file_item: Option<PushPrimaryFileItem> = None;
    let mut buf = vec![0; 8192];
    loop {
        match message_hub.read_type_byte()? {
            TransferType::FileItem => {
                let string_message = StringMessage::parse(&mut message_hub)?;
                trace!("got file item: {}", string_message.content);
                match serde_json::from_str::<PushPrimaryFileItem>(&string_message.content) {
                    Ok(file_item) => {
                        let df = home_dir.join_another(&file_item.remote_path);
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
                if let Some(df) = last_df.take() {
                    trace!("copy to file: {:?}", df.as_path());
                    if let Err(err) =
                        message_hub.copy_to_file(&mut buf, content_len.value, df.as_path())
                    {
                        message_hub.write_error_message(format!("{:?}", err))?;
                    } else {
                        if let Some(lfi) = last_file_item.as_ref() {
                            if let Some(md) = lfi.modified {
                                let ft = filetime::FileTime::from_unix_time(md as i64, 0);
                                filetime::set_file_mtime(df.as_path(), ft)?;
                            } else {
                                message_hub.write_error_message(
                                    "push_primary_file_item has no modified value.",
                                )?;
                            }
                        } else {
                            message_hub.write_error_message("last_file_item is empty.")?;
                        }
                    }
                } else {
                    error!("empty last_df.");
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
