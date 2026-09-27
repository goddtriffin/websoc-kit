use std::fmt::{Debug, Formatter};
use std::{error, fmt};

use tokio_tungstenite::tungstenite;

use crate::connection_id::ConnectionId;

pub type WebsocKitResult<T> = Result<T, WebsocKitError>;

#[expect(clippy::module_name_repetitions)]
#[derive(Debug)]
pub enum WebsocKitError {
    TungsteniteError(tungstenite::Error),

    ConnectionDoesNotExist(ConnectionId),
    TextMessagesNotAllowed(ConnectionId, String),
}

impl error::Error for WebsocKitError {}

impl fmt::Display for WebsocKitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            WebsocKitError::TungsteniteError(error) => write!(f, "Tungstenite error: '{error}'"),

            WebsocKitError::ConnectionDoesNotExist(connection_id) => {
                write!(
                    f,
                    "No websocket connection exists with id: '{connection_id}'"
                )
            }
            WebsocKitError::TextMessagesNotAllowed(connection_id, invalid_text_message) => {
                write!(
                    f,
                    "Terminated websocket '{connection_id}' because it sent a non-binary text message: '{invalid_text_message}'"
                )
            }
        }
    }
}

impl From<tungstenite::Error> for WebsocKitError {
    fn from(error: tungstenite::Error) -> Self {
        WebsocKitError::TungsteniteError(error)
    }
}
