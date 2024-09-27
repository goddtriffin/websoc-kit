use std::{collections::HashMap, sync::Arc};

use axum::extract::ws::{Message, WebSocket};
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::sync::{mpsc::Sender, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tracing::{error, info, instrument, warn};

use crate::{
    connection_id::ConnectionId,
    error::{WebsocKitError, WebsocKitResult},
    message::WebsocKitMessage,
    subscription::Subscription,
};

#[expect(clippy::module_name_repetitions)]
pub struct WebsocKitManager {
    connections: RwLock<HashMap<ConnectionId, RwLock<SplitSink<WebSocket, Message>>>>,
    subscriptions: RwLock<HashMap<ConnectionId, HashMap<Subscription, usize>>>,
    payload_listener_tx: Sender<WebsocKitMessage>,
}

impl WebsocKitManager {
    #[must_use]
    pub fn new(payload_listener_tx: Sender<WebsocKitMessage>) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            payload_listener_tx,
        }
    }

    /// Splits the given Websocket connection into a sender/receiver, tracks the connection via an ID,
    /// listens for incoming socket packets, cleans up connection data once connection is lost.
    #[instrument(skip_all)]
    pub async fn handle_new_websocket(
        self: &Arc<Self>,
        socket: WebSocket,
    ) -> WebsocKitResult<ConnectionId> {
        // split the websocket connection into sender/receiver
        let (websocket_sender, websocket_listener): (
            SplitSink<WebSocket, Message>,
            SplitStream<WebSocket>,
        ) = socket.split();

        // store the new websocket connection
        let connection_id: ConnectionId = ConnectionId::new();
        self.connections
            .write()
            .await
            .insert(connection_id, RwLock::new(websocket_sender));
        info!("websocket connection established: '{connection_id}'");

        // receive packets from socket
        self.clone()
            .listen_to_websocket(websocket_listener, connection_id)
            .await?;

        // websocket cleanup
        if self
            .connections
            .write()
            .await
            .remove(&connection_id)
            .is_none()
        {
            error!("attempted to discard dead Connection, but none existed with the given ID: '{connection_id}'");
            // TODO - should I return an error?
        }
        info!("websocket connection closed: '{connection_id}'");

        // return connection_id so that the caller can handle their cleanup
        Ok(connection_id)
    }

    /// Receive packet from websocket, pass to the listener.
    #[instrument(skip_all)]
    async fn listen_to_websocket(
        self: Arc<Self>,
        mut socket_receiver: SplitStream<WebSocket>,
        connection_id: ConnectionId,
    ) -> WebsocKitResult<()> {
        // indefinitely listen to the websocket
        let cloned_self: Arc<Self> = Arc::clone(&self);
        while let Some(Ok(message)) = socket_receiver.next().await {
            match message {
                // valid
                Message::Binary(binary) => {
                    // forward the binary payload to the payload listener
                    if let Err(_send_error) = cloned_self
                        .payload_listener_tx
                        .send(WebsocKitMessage {
                            connection_id,
                            payload: binary,
                        })
                        .await
                    {
                        // If sending the payload fails, this means that the receiver has been closed/dropped.
                        // This means they don't want to receive any more payloads from this connection, so we can break the loop.
                        break;
                    }
                }
                Message::Close(close) => {
                    close.map_or_else(
                        || {
                            info!("Websocket '{connection_id}' received close frame.");
                        },
                        |close_frame| {
                            info!("Websocket '{connection_id}' received close frame: '{close_frame:?}'.");
                        },
                    );
                    break;
                }

                // invalid
                Message::Text(invalid_text_message) => {
                    // terminate the connection for not sending binary
                    return Err(WebsocKitError::TextMessagesNotAllowed(
                        connection_id,
                        invalid_text_message,
                    ));
                }
                Message::Ping(_ping) => {
                    // NOP - handled by Axum
                    info!("Websocket '{connection_id}' received ping.");
                }
                Message::Pong(_pong) => {
                    // NOP - handled by Axum
                    info!("Websocket '{connection_id}' received pong.");
                }
            }
        }
        Ok(())
    }

    /// Send a payload to multiple websocket.
    #[instrument(skip_all)]
    pub async fn send_payload(
        &self,
        connection_ids: Vec<ConnectionId>,
        payload: Vec<u8>,
    ) -> WebsocKitResult<()> {
        // make sure that at least one websocket session ID was given
        if connection_ids.is_empty() {
            warn!("attempted to send payload to zero websockets: {payload:?}");
            return Ok(());
        }

        // loop over all the given Connection IDs
        // TODO - create JoinHandles via tokio::spawn() to parallelize?
        for connection_id in connection_ids {
            // retrieve the Connection by ID, if it exists
            let connections: RwLockReadGuard<
                HashMap<ConnectionId, RwLock<SplitSink<WebSocket, Message>>>,
            > = self.connections.read().await;
            let Some(outgoing_payload_sender) = connections.get(&connection_id) else {
                return Err(WebsocKitError::ConnectionDoesNotExist(connection_id));
            };

            // send the outgoing payload
            match outgoing_payload_sender
                .write()
                .await
                .send(Message::Binary(payload.clone()))
                .await
            {
                Ok(()) => {
                    info!("sent payload to websocket '{connection_id}': {payload:?}");
                }
                Err(error) => {
                    error!(
                        "failed to send payload to websocket '{connection_id}': {payload:?} -> Error: {error}"
                    );
                    break;
                }
            };
        }

        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn send_payload_to_all_connections(&self, payload: Vec<u8>) -> WebsocKitResult<()> {
        let all_connection_ids: Vec<ConnectionId> =
            self.connections.read().await.keys().copied().collect();
        self.send_payload(all_connection_ids, payload).await
    }

    #[instrument(skip_all)]
    pub async fn send_payload_to_subscribers(
        &self,
        subscription: Subscription,
        payload: Vec<u8>,
    ) -> WebsocKitResult<()> {
        // retrieve all the Connection IDs that are subscribed to the given subscription
        let mut connection_ids: Vec<ConnectionId> = Vec::new();
        let subscriptions: RwLockReadGuard<HashMap<ConnectionId, HashMap<Subscription, usize>>> =
            self.subscriptions.read().await;
        for (connection_id, subscriptions) in subscriptions.iter() {
            if subscriptions.contains_key(&subscription) {
                connection_ids.push(*connection_id);
            }
        }
        info!("found websockets subscribed to '{subscription}': {connection_ids:?} - sending payload: {payload:?}");

        // send the payload to all the subscribers
        self.send_payload(connection_ids, payload).await
    }

    /// Add a subscription for a websocket connection.
    #[instrument(skip_all)]
    pub async fn add_subscription(&self, connection_id: ConnectionId, subscription: Subscription) {
        // retrieve the subscriptions by Connection ID
        let mut subscriptions_lock: RwLockWriteGuard<
            HashMap<ConnectionId, HashMap<Subscription, usize>>,
        > = self.subscriptions.write().await;
        let subscriptions: &mut HashMap<Subscription, usize> =
            subscriptions_lock.entry(connection_id).or_default();
        let subscription_count: &mut usize = subscriptions.entry(subscription.clone()).or_insert(0);

        // add the subscription
        *subscription_count += 1;
        info!("subscribed websocket '{connection_id}' to '{subscription}'");
    }

    /// Remove a subscription for a websocket connection.
    #[instrument(skip_all)]
    pub async fn remove_subscription(
        &self,
        connection_id: ConnectionId,
        subscription: Subscription,
    ) {
        // retrieve the subscriptions by Connection ID
        let mut subscriptions_lock: RwLockWriteGuard<
            HashMap<ConnectionId, HashMap<Subscription, usize>>,
        > = self.subscriptions.write().await;
        let Some(subscriptions) = subscriptions_lock.get_mut(&connection_id) else {
            error!("attempted to unsubscribe from '{subscription}', but websocket '{connection_id}' had zero subscriptions at all");
            return;
        };

        // remove the subscription
        if let Some(subscription_count) = subscriptions.get_mut(&subscription) {
            *subscription_count -= 1;
            info!("unsubscribed '{connection_id}' from '{subscription}'");

            // remove the subscription if the count is zero
            if *subscription_count == 0 {
                subscriptions.remove(&subscription);
                info!("deleted subscription '{subscription}' from '{connection_id}'");

                // remove the Connection ID if it has no subscriptions
                if subscriptions.is_empty() {
                    subscriptions_lock.remove(&connection_id);
                    info!("deleted all subscriptions for '{connection_id}'");
                }
            }
        } else {
            error!("attempted to unsubscribe from '{subscription}', but websocket '{connection_id}' was not subscribed to it");
        }
    }

    #[instrument(skip_all)]
    pub async fn remove_all_subscriptions(&self, connection_id: ConnectionId) {
        // retrieve the subscriptions by Connection ID
        let mut subscriptions_lock: RwLockWriteGuard<
            HashMap<ConnectionId, HashMap<Subscription, usize>>,
        > = self.subscriptions.write().await;

        // remove all subscriptions
        if let Some(subscriptions) = subscriptions_lock.remove(&connection_id) {
            info!("unsubscribed '{connection_id}' from all subscriptions: {subscriptions:?}");
        } else {
            error!("attempted to unsubscribe from all subscriptions, but websocket '{connection_id}' had zero subscriptions at all");
        }
    }

    #[instrument(skip_all)]
    pub async fn get_subscriptions(
        &self,
        connection_id: ConnectionId,
    ) -> Option<HashMap<Subscription, usize>> {
        self.subscriptions.read().await.get(&connection_id).cloned()
    }

    #[instrument(skip_all)]
    pub async fn get_subscribers(&self, subscription: Subscription) -> Vec<ConnectionId> {
        let mut connection_ids: Vec<ConnectionId> = Vec::new();
        let subscriptions: RwLockReadGuard<HashMap<ConnectionId, HashMap<Subscription, usize>>> =
            self.subscriptions.read().await;
        for (connection_id, subscriptions) in subscriptions.iter() {
            if subscriptions.contains_key(&subscription) {
                connection_ids.push(*connection_id);
            }
        }
        connection_ids
    }
}
