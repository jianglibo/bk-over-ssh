use crate::data_shape::ServerYml;
use crate::protocol::{ProtocolReader, ServerYmlHeader, TransferType};
use dirs;
use std::io::{self};

/// how to determine the directories? it's in the user's home directory.

pub fn server_receive_loop() -> Result<(), failure::Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin_handler = stdin.lock();
    let mut stdout_handler = stdout.lock();

    // home_dir joins app_instance_id.
    let home_dir = dirs::home_dir().expect("get home_dir");

    let mut server_yml_op: Option<ServerYml> = None;

    let mut protocol_reader = ProtocolReader::new(&mut stdin_handler);

    loop {
        match protocol_reader.read_type_byte()? {
            TransferType::ServerYml => {
                let yml_header = ServerYmlHeader::parse(&mut protocol_reader)?;
                let server_yml = serde_json::from_str::<ServerYml>(&yml_header.yml_string)?;
                server_yml_op.replace(server_yml);
            }
            _ => panic!("unimplement transfer type."),
        }
    }

    Ok(())
}
