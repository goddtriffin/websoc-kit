use crate::connection_id::ConnectionId;

#[expect(clippy::module_name_repetitions)]
pub struct WebsocKitMessage {
    pub connection_id: ConnectionId,
    pub payload: Vec<u8>,
}
