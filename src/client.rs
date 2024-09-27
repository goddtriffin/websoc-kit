use futures_util::{stream::SplitSink, stream::SplitStream, SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{error, info};
use uuid::Uuid;

use crate::error::{WebsocKitError, WebsocKitResult};

#[expect(clippy::module_name_repetitions)]
pub struct WebsocKitClient {
    writer: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
    reader: Arc<Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
}

impl WebsocKitClient {
    /// # Errors
    ///
    /// TODO
    pub async fn new(url: &str) -> WebsocKitResult<Self> {
        let (ws_stream, _) = connect_async(url).await?;
        info!("WebSocket handshake has been successfully completed");

        let (write, read) = ws_stream.split();

        Ok(WebsocKitClient {
            writer: Arc::new(Mutex::new(write)),
            reader: Arc::new(Mutex::new(read)),
        })
    }

    /// # Errors
    ///
    /// TODO
    pub async fn send_message(&self, message: Vec<u8>) -> WebsocKitResult<()> {
        let mut writer = self.writer.lock().await;
        writer.send(Message::Binary(message)).await?;
        Ok(())
    }

    /// # Errors
    ///
    /// TODO
    pub async fn read_message(&self) -> WebsocKitResult<Option<Vec<u8>>> {
        let mut reader = self.reader.lock().await;
        if let Some(Ok(message)) = reader.next().await {
            match message {
                // valid
                Message::Binary(binary) => Ok(Some(binary)),
                Message::Close(close) => {
                    close.map_or_else(
                        || {
                            info!("Received close frame.");
                        },
                        |close_frame| {
                            info!("Received close frame: {close_frame:?}");
                        },
                    );
                    Ok(None)
                }

                // invalid
                Message::Text(invalid_text_message) => {
                    // terminate the connection for not sending binary
                    Err(WebsocKitError::TextMessagesNotAllowed(
                        Uuid::nil().into(), // ConnectionId only makes sense in the context of WebsocKitManager
                        invalid_text_message,
                    ))
                }
                Message::Ping(_ping) => {
                    // NOP - handled by Axum
                    Ok(None)
                }
                Message::Pong(_pong) => {
                    // NOP - handled by Axum
                    Ok(None)
                }
                Message::Frame(frame) => {
                    error!("unexpected frame: {frame}");
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }
}
