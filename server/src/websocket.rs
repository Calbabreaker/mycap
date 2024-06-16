use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::sync::RwLock;
use warp::{filters::ws::WebSocket, Filter};

use crate::{
    serial::write_serial,
    tracker::{TrackerData, TrackerInfo},
};

pub const WEBSOCKET_PORT: u16 = 8298;

// Sent from server
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type")]
enum WebsocketServerMessage {
    Error { error: String },
    TrackerInfo { info: TrackerInfo },
    TrackerData { index: usize, data: TrackerData },
}

// Receieved from client
#[derive(Clone, serde::Deserialize)]
#[serde(tag = "type")]
enum WebsocketClientMessage {
    Wifi { ssid: String, password: String },
    FactoryReset,
}

type WebsocketTx = SplitSink<WebSocket, warp::ws::Message>;

pub struct WebsocketServer {
    websocket_channels: Vec<WebsocketTx>,
}

impl WebsocketServer {
    fn add_channel(&mut self, channel: WebsocketTx) {
        self.websocket_channels.push(channel)
    }

    /// Sends a websocket messsage to all clients connected to the websocket server
    async fn send_message_to_clients(&mut self, message: WebsocketServerMessage) {
        let mut to_remove = None;

        for (i, channel) in self.websocket_channels.iter_mut().enumerate() {
            // The channel got closed or something so remove it
            if let Ok(string) = serde_json::to_string(&message) {
                if channel.send(warp::ws::Message::text(string)).await.is_err() {
                    to_remove = Some(i)
                }
            }
        }

        if let Some(to_remove) = to_remove {
            self.websocket_channels.swap_remove(to_remove);
        }
    }

    async fn send_tracker_info(&mut self, info: TrackerInfo) {
        self.send_message_to_clients(WebsocketServerMessage::TrackerInfo { info })
            .await;
    }

    async fn send_tracker_data(&mut self, index: usize, data: TrackerData) {
        self.send_message_to_clients(WebsocketServerMessage::TrackerData { index, data })
            .await;
    }
}

pub async fn start_warp_server(
    websocket_server: Arc<RwLock<WebsocketServer>>,
) -> anyhow::Result<()> {
    let websocket = warp::ws()
        .and(warp::any().map(move || websocket_server.clone()))
        .map(|ws: warp::ws::Ws, websocket_server| {
            ws.on_upgrade(|ws| on_connect(ws, websocket_server))
        });

    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, WEBSOCKET_PORT));
    log::info!("Started websocket server on {address}");
    warp::serve(websocket).run(address).await;
    Ok(())
}

async fn on_connect(ws: WebSocket, websocket_server: Arc<RwLock<WebsocketServer>>) {
    log::info!("Websocket client connected");
    let (ws_tx, mut ws_rx) = ws.split();

    websocket_server.write().await.add_channel(ws_tx);

    for tracker in &main.read().await.trackers {
        send_websocket_message(
            &ws_tx,
            WebsocketServerMessage::TrackerInfo {
                info: tracker.info.clone(),
            },
        )
        .await;
    }

    while let Some(ws_result) = ws_rx.next().await {
        let msg = match ws_result {
            Ok(msg) => msg,
            Err(e) => {
                log::error!("Websocket error: {e}");
                break;
            }
        };

        if let Ok(string) = msg.to_str() {
            log::info!("Got from websocket: {string}");
            if let Err(error) = handle_client_message(string) {
                log::error!("{error}");
                send_websocket_message(
                    &ws_tx,
                    WebsocketServerMessage::Error {
                        error: error.to_string(),
                    },
                )
                .await;
            }
        }
    }

    log::info!("Websocket client disconnected");
    server_messages_task.abort();
    server_messages_task.await.ok();
}

async fn handle_server_message(message: ServerMessage, ws_tx: &WebsocketTx) {
    match message {
        ServerMessage::TrackerInfoUpdate(info) => {
            send_websocket_message(ws_tx, WebsocketServerMessage::TrackerInfo { info }).await;
        }
        ServerMessage::TrackerDataUpdate((index, data)) => {
            send_websocket_message(ws_tx, WebsocketServerMessage::TrackerData { index, data })
                .await;
        }
    }
}

fn handle_client_message(string: &str) -> anyhow::Result<()> {
    let message = serde_json::from_str(string)?;

    match message {
        WebsocketClientMessage::Wifi { ssid, password } => {
            if ssid.len() > 32 || password.len() > 64 {
                return Err(anyhow::Error::msg("SSID or password too long"));
            }

            write_serial(format!("Wifi\0{ssid}\0{password}").as_bytes())?;
        }
        WebsocketClientMessage::FactoryReset => {
            write_serial("FactoryReset".as_bytes())?;
        }
    }

    Ok(())
}
