use crate::data_shape::{ServerYml, PushPrimaryFileItem, SlashPath};
use crate::protocol::{ProtocolReader, StringMessage, TransferType, U64Message};
use dirs;
use std::io::{self, Write};

/// how to determine the directories? it's in the user's home directory.

pub fn server_receive_loop() -> Result<(), failure::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin_handler = stdin.lock();
    let mut stdout_handler = stdout.lock();

    // home_dir joins app_instance_id.
    let home_dir = SlashPath::from_path(dirs::home_dir().expect("get home_dir").as_path()).expect("get slash path from home_dir");

    let mut server_yml_op: Option<ServerYml> = None;

    let mut protocol_reader = ProtocolReader::new(&mut stdin_handler);

    match protocol_reader.read_type_byte()? {
        TransferType::ServerYml => {
            let string_message = StringMessage::parse(&mut protocol_reader)?;
            let server_yml = serde_json::from_str::<ServerYml>(&string_message.content)?;
            server_yml_op.replace(server_yml);
        }
        _ => panic!("unimplement transfer type."),
    }

    let mut last_df: Option<SlashPath> = None;
    let mut buf = vec![0;8192];
    loop {
        match protocol_reader.read_type_byte()? {
            TransferType::FileItem => {
                let string_message = StringMessage::parse(&mut protocol_reader)?;
                let file_item = serde_json::from_str::<PushPrimaryFileItem>(&string_message.content)?;
                let df = home_dir.join_another(&file_item.remote_path);
                if file_item.changed(df.as_path()) {
                    stdout_handler.write_all(&[TransferType::FileItemChanged.to_u8()])?;
                    last_df = Some(df);
                } else {
                    stdout_handler.write_all(&[TransferType::FileItemUnchanged.to_u8()])?;
                }
            }
            TransferType::StartSend => {
                let content_len = U64Message::parse(&mut protocol_reader)?;
                if let Some(df) = last_df.take() {
                    protocol_reader.copy_to_file(&mut buf, content_len.value, df.as_path())?;
                }

            }
            TransferType::RepeatDone | TransferType::Eof => break,
            _ => panic!("unimplement transfer type."),
        }
    }

    Ok(())
}
