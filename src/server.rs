use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures_util::{SinkExt, StreamExt, TryStreamExt};
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Frame, Incoming},
    header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE},
    service::service_fn,
    upgrade::Upgraded,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hyper_util::rt::TokioIo;
use notify::{Event, RecursiveMode, Watcher};
use sha1::{Digest, Sha1};
use tokio::{fs::File, net::TcpListener, sync::broadcast};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};
use tokio_util::io::ReaderStream;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type HttpResponse = Response<BoxBody<Bytes, std::io::Error>>;

pub struct ZeroServer {
    pub addr: SocketAddr,
    pub root: PathBuf,

    reload_tx: broadcast::Sender<()>,
}

const LIVE_RELOAD_SCRIPT: &str = r#"
<script>
(function() {
    const ws = new WebSocket('ws://' + window.location.host + '/__live_reload');
    ws.onmessage = () => {
        console.log('reloading');
        window.location.reload();
    };
    ws.onclose = () => {
        console.log('disconnected');
        setTimeout(() => window.location.reload(), 1000);
    };
})();
</script>
"#;

impl fmt::Display for ZeroServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ZeroServer(address: {}, root: {})",
            self.addr,
            self.root.display()
        )
    }
}

impl ZeroServer {
    pub fn new(addr: SocketAddr, root: impl Into<PathBuf>) -> Self {
        ZeroServer {
            addr,
            root: root.into(),
            reload_tx: broadcast::channel(16).0,
        }
    }

    pub async fn run(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.addr).await?;

        let reload_tx = Arc::new(self.reload_tx.clone());
        let root = Arc::new(self.root.clone());

        self.watch()?;

        loop {
            let (stream, _) = listener.accept().await?;

            let root = Arc::clone(&root);
            let reload_tx = Arc::clone(&reload_tx);

            tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let root = Arc::clone(&root);
                    let reload_tx = Arc::clone(&reload_tx);

                    async move { Self::serve(root, reload_tx, req).await }
                });

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await
                {
                    eprintln!("error serving connection: {:?}", err);
                }
            });
        }
    }

    fn not_found() -> HttpResponse {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(
                Full::new("404 Not Found".into())
                    .map_err(|_| unreachable!())
                    .boxed(),
            )
            .unwrap()
    }

    fn compute_websocket_accept(key: &str) -> String {
        const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        let mut sha1 = Sha1::new();
        sha1.update(key.as_bytes());
        sha1.update(WEBSOCKET_GUID.as_bytes());
        BASE64.encode(sha1.finalize())
    }

    async fn handle_websocket(
        reload_tx: Arc<broadcast::Sender<()>>,
        req: Request<Incoming>,
    ) -> hyper::Result<HttpResponse> {
        let key = req
            .headers()
            .get(SEC_WEBSOCKET_KEY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let accept = Self::compute_websocket_accept(key);

        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = Self::handle_ws_connection(upgraded, reload_tx).await {
                        eprintln!("WebSocket error: {}", e);
                    }
                }
                Err(e) => eprintln!("upgrade error: {}", e),
            }
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
        let ws = WebSocketStream::from_raw_socket(
            TokioIo::new(upgraded),
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;

        let (mut ws_tx, mut ws_rx) = ws.split();
        let mut reload_rx = reload_tx.subscribe();

        loop {
            tokio::select! {
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Ping(data))) => {
                            ws_tx.send(Message::Pong(data)).await?;
                        }
                        Some(Err(e)) => {
                            eprintln!("WebSocket receive error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
                _ = reload_rx.recv() => {
                    if ws_tx.send(Message::Text("reload".to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn serve(
        root: Arc<PathBuf>,
        reload_tx: Arc<broadcast::Sender<()>>,
        req: Request<Incoming>,
    ) -> hyper::Result<HttpResponse> {
        let path = req.uri().path();

        if path == "/__live_reload"
            && req.headers().get(UPGRADE).and_then(|v| v.to_str().ok()) == Some("websocket")
        {
            return Self::handle_websocket(reload_tx, req).await;
        }

        if path.starts_with("/static/") {
            let file_path = root.join(path.trim_start_matches('/'));

            return Ok(Self::serve_file(&file_path, false)
                .await
                .unwrap_or_else(|_| Self::not_found()));
        }

        let mut page_path = root.join("pages").join(path.trim_start_matches('/'));
        if page_path.is_dir() || path == "/" {
            page_path.push("index.html");
        } else if page_path.extension().is_none() {
            page_path.set_extension("html");
        }

        Ok(Self::serve_file(&page_path, true)
            .await
            .unwrap_or_else(|_| Self::not_found()))
    }

    fn watch(&self) -> Result<(), BoxError> {
        let reload_tx = self.reload_tx.clone();
        let watch_path = self.root.clone();

        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut watcher = notify::recommended_watcher(tx).unwrap();
            watcher
                .watch(&watch_path, RecursiveMode::Recursive)
                .unwrap();

            for res in rx {
                match res {
                    Ok(Event {
                        kind:
                            notify::event::EventKind::Modify(_)
                            | notify::event::EventKind::Create(_)
                            | notify::event::EventKind::Remove(_),
                        ..
                    }) => {
                        let _ = reload_tx.send(());
                        println!("file changed, triggering reload");
                    }
                    Err(e) => eprintln!("watch error: {:?}", e),
                    _ => {}
                }
            }
        });

        Ok(())
    }

    async fn serve_file(path: &Path, inject: bool) -> Result<HttpResponse, std::io::Error> {
        let file = File::open(path).await?;
        let content_type = match path.extension().and_then(|ext| ext.to_str()) {
            Some("html") => "text/html",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("svg") => "image/svg+xml",
            Some("json") => "application/json",
            Some("wasm") => "application/wasm",
            _ => "application/octet-stream",
        };

        if inject && content_type == "text/html" {
            let contents = tokio::fs::read_to_string(path).await?;

            return Ok(Response::builder()
                .header("Content-Type", content_type)
                .body(
                    Full::new(format!("{}{}", contents, LIVE_RELOAD_SCRIPT).into())
                        .map_err(|_| unreachable!())
                        .boxed(),
                )
                .unwrap());
        }

        Ok(Response::builder()
            .header("Content-Type", content_type)
            .body(BodyExt::boxed(StreamBody::new(
                ReaderStream::new(file).map_ok(Frame::data),
            )))
            .unwrap())
    }
}
