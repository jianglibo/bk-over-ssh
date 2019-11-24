use ssh2;
use crate::protocol::{ProtocolReader, ServerYmlHeader, CopyOutHeader, TransferType};

pub struct ChannelWrapper {
    pub channel: ssh2::Channel,

}