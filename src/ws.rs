use crate::{BoxError, HttpResponse};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response, StatusCode,
    body::Incoming,
    header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE},
    upgrade::Upgraded,
};
use hyper_util::rt::TokioIo;
use sha1::{Digest, Sha1};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::protocol::{Message, Role},
};

const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn compute_websocket_accept(key: &str) -> String {
    let mut sha1 = Sha1::new();

    sha1.update(key.as_bytes());
    sha1.update(WEBSOCKET_GUID.as_bytes());

    BASE64.encode(sha1.finalize())
}

pub async fn handle_websocket(
    reload_tx: Arc<broadcast::Sender<()>>,
    req: Request<Incoming>,
) -> hyper::Result<HttpResponse> {
    let key = req
        .headers()
        .get(SEC_WEBSOCKET_KEY)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let accept = compute_websocket_accept(key);

    tokio::spawn(async move {
        hyper::upgrade::on(req)
            .await
            .map_err(|e| eprintln!("upgrade error: {}", e))
            .ok()
            .map(|upgraded| async move {
                handle_ws_connection(upgraded, reload_tx)
                    .await
                    .map_err(|e| eprintln!("ws error: {}", e))
                    .ok()
            })
            .map(|fut| tokio::spawn(fut));
    });

    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "Upgrade")
        .header(SEC_WEBSOCKET_ACCEPT, accept)
        .body(Full::new("".into()).map_err(|_| unreachable!()).boxed())
        .unwrap())
}

async fn handle_ws_connection(
    upgraded: Upgraded,
    reload_tx: Arc<broadcast::Sender<()>>,
) -> Result<(), BoxError> {
    let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;

    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut reload_rx = reload_tx.subscribe();

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                if msg
                    .as_ref()
                    .is_none_or(|r| r.as_ref().map_or_else(
                        |e| { eprintln!("ws receive error: {}", e); true },
                        |m| matches!(m, Message::Close(_))
                    )) {

                        break;
                }

                if let Some(Ok(Message::Ping(data))) = msg {
                    let _ = ws_tx.send(Message::Pong(data)).await;
                }
            }

            _ = reload_rx.recv() => {
                if ws_tx.send(Message::Text("reload".into())).await.is_err() {
                    break;
                }
            }
        }
    }

    Ok(())
}
