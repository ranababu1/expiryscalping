//! WebSocket server. Plain `ws://` bound to localhost in Phase 1. TLS and
//! WireGuard come with the VPS (Phase 2).
//!
//! Protocol:
//! 1. The client sends `auth` with its device token within 5 seconds.
//! 2. The server replies `hello`, then the latest message of every stream.
//! 3. Commands get exactly one `ack` or `error` with the same `req_id`.

use crate::auth::DeviceStore;
use crate::msg::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Bigger messages from clients are refused (commands are small).
const MAX_CLIENT_MSG: usize = 64 * 1024;

/// A command from an authenticated device, for the engine.
#[derive(Debug)]
pub struct ApiCommand {
    pub device: String,
    pub msg: ClientMsg,
    /// The engine answers with `Ack` or `Error`.
    pub reply: oneshot::Sender<ServerMsg>,
}

/// Fan-out of engine updates to every connection, plus the latest message
/// per stream for clients that connect later.
#[derive(Clone)]
pub struct Publisher {
    tx: broadcast::Sender<ServerMsg>,
    latest: Arc<Mutex<HashMap<Stream, ServerMsg>>>,
}

impl Default for Publisher {
    fn default() -> Self {
        Publisher::new()
    }
}

impl Publisher {
    pub fn new() -> Publisher {
        let (tx, _) = broadcast::channel(1024);
        Publisher {
            tx,
            latest: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn publish(&self, m: ServerMsg) {
        if let Some(s) = m.stream() {
            // Fills and alerts are events, not state; keep only snapshots.
            if !matches!(m, ServerMsg::Fill(_) | ServerMsg::Alert(_)) {
                let mut g = self.latest.lock().unwrap_or_else(|e| e.into_inner());
                g.insert(s, m.clone());
            }
        }
        let _ = self.tx.send(m);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMsg> {
        self.tx.subscribe()
    }

    pub fn latest(&self) -> Vec<ServerMsg> {
        let g = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<(Stream, ServerMsg)> = g.iter().map(|(k, v)| (*k, v.clone())).collect();
        v.sort_by_key(|(k, _)| *k as u8);
        v.into_iter().map(|(_, m)| m).collect()
    }
}

/// Accepts connections until the listener fails.
pub async fn serve(
    listener: TcpListener,
    devices: Arc<RwLock<DeviceStore>>,
    commands: mpsc::Sender<ApiCommand>,
    publisher: Publisher,
    engine_version: String,
) {
    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                warn!(%e, "api: accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        debug!(%addr, "api: connection");
        let devices = devices.clone();
        let commands = commands.clone();
        let publisher = publisher.clone();
        let version = engine_version.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(stream, devices, commands, publisher, version).await {
                debug!(%addr, %e, "api: connection closed");
            }
        });
    }
}

#[derive(Debug, thiserror::Error)]
enum ConnError {
    #[error("websocket: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not authenticated")]
    Unauthorized,
}

fn text(m: &ServerMsg) -> Result<Message, ConnError> {
    Ok(Message::text(serde_json::to_string(m)?))
}

fn error(req_id: Option<u32>, code: ErrorCode, message: impl Into<String>) -> ServerMsg {
    ServerMsg::Error {
        req_id,
        code,
        message: message.into(),
    }
}

async fn connection(
    stream: TcpStream,
    devices: Arc<RwLock<DeviceStore>>,
    commands: mpsc::Sender<ApiCommand>,
    publisher: Publisher,
    version: String,
) -> Result<(), ConnError> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut source) = ws.split();

    // 1. Auth first.
    let first = tokio::time::timeout(AUTH_TIMEOUT, source.next()).await;
    let device = match first {
        Ok(Some(Ok(Message::Text(t)))) if t.len() <= MAX_CLIENT_MSG => {
            match serde_json::from_str::<ClientMsg>(&t) {
                Ok(ClientMsg::Auth { token }) => devices
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .verify(&token),
                _ => None,
            }
        }
        _ => None,
    };
    let Some(device) = device else {
        // Same reply for every failure, so it reveals nothing.
        let _ = sink
            .send(text(&error(
                None,
                ErrorCode::Unauthorized,
                "invalid device token",
            ))?)
            .await;
        let _ = sink.close().await;
        warn!("api: rejected a connection with a bad or missing token");
        return Err(ConnError::Unauthorized);
    };
    info!(%device, "api: device connected");

    // 2. Hello and current state.
    let mut updates = publisher.subscribe();
    sink.send(text(&ServerMsg::Hello {
        protocol: PROTOCOL_VERSION,
        engine_version: version,
        device: device.clone(),
    })?)
    .await?;
    for m in publisher.latest() {
        sink.send(text(&m)?).await?;
    }

    // 3. Commands and updates.
    let mut streams: HashSet<Stream> = all_streams();
    loop {
        tokio::select! {
            incoming = source.next() => {
                let Some(incoming) = incoming else { break };
                match incoming? {
                    Message::Text(t) => {
                        let reply = handle(&device, &t, &commands, &mut streams).await;
                        sink.send(text(&reply)?).await?;
                    }
                    Message::Ping(p) => sink.send(Message::Pong(p)).await?,
                    Message::Close(_) => break,
                    _ => {
                        sink.send(text(&error(None, ErrorCode::BadRequest, "text JSON messages only"))?).await?;
                    }
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(m) => {
                        if m.stream().is_some_and(|s| streams.contains(&s)) {
                            sink.send(text(&m)?).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(%device, n, "api: client lagged, resending snapshots");
                        for m in publisher.latest() {
                            sink.send(text(&m)?).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    info!(%device, "api: device disconnected");
    Ok(())
}

fn all_streams() -> HashSet<Stream> {
    use Stream::*;
    [Status, Chain, Positions, Orders, Risk, Strategies, Alerts]
        .into_iter()
        .collect()
}

async fn handle(
    device: &str,
    raw: &str,
    commands: &mpsc::Sender<ApiCommand>,
    streams: &mut HashSet<Stream>,
) -> ServerMsg {
    if raw.len() > MAX_CLIENT_MSG {
        return error(None, ErrorCode::BadRequest, "message too large");
    }
    let msg: ClientMsg = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(e) => {
            // Echo the req_id back if we can find one.
            let req_id = serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| v.get("req_id").and_then(|r| r.as_u64()))
                .and_then(|r| u32::try_from(r).ok());
            return error(req_id, ErrorCode::BadRequest, format!("bad message: {e}"));
        }
    };
    let req_id = msg.req_id();
    match msg {
        ClientMsg::Auth { .. } => error(None, ErrorCode::BadRequest, "already authenticated"),
        ClientMsg::Ping { req_id } => ServerMsg::Ack {
            req_id,
            message: Some("pong".into()),
        },
        ClientMsg::Subscribe { req_id, streams: s } => {
            *streams = s.into_iter().collect();
            ServerMsg::Ack {
                req_id,
                message: None,
            }
        }
        msg => {
            let (tx, rx) = oneshot::channel();
            let cmd = ApiCommand {
                device: device.to_string(),
                msg,
                reply: tx,
            };
            if commands.send(cmd).await.is_err() {
                return error(req_id, ErrorCode::Internal, "engine is not running");
            }
            match tokio::time::timeout(REPLY_TIMEOUT, rx).await {
                Ok(Ok(reply)) => reply,
                _ => error(req_id, ErrorCode::Internal, "engine did not answer in time"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
