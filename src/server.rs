use crate::{BoxError, HttpResponse, is_livereload, serve_file, watch, ws::handle_websocket};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{fmt, net::SocketAddr, path::Path, path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::broadcast};

pub struct ZeroServer {
    pub addr: SocketAddr,
    pub root: PathBuf,

    reload_tx: broadcast::Sender<()>,
}

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

        if is_livereload() {
            watch(self.reload_tx.clone(), self.root.clone())?;
        }

        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };

            let root = Arc::clone(&root);
            let reload_tx = Arc::clone(&reload_tx);

            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    ZeroServer::serve(Arc::clone(&root), Arc::clone(&reload_tx), req)
                });

                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await;
            });
        }
    }

    async fn serve(
        root: Arc<PathBuf>,
        reload_tx: Arc<broadcast::Sender<()>>,
        req: Request<Incoming>,
    ) -> hyper::Result<HttpResponse> {
        let path = req.uri().path();

        if is_livereload()
            && path == "/__live_reload"
            && req
                .headers()
                .get(hyper::header::UPGRADE)
                .and_then(|v| v.to_str().ok())
                == Some("websocket")
        {
            return handle_websocket(reload_tx, req).await;
        }

        if path.starts_with("/static/") {
            let file_path = root.join(path.trim_start_matches('/'));
            return Self::serve_or_404(&file_path).await;
        }

        let mut page = root.join("pages").join(path.trim_start_matches('/'));

        if path == "/" || page.is_dir() {
            page.push("index.html");
        }

        if page.extension().is_none() {
            page.set_extension("html");
        }

        Self::serve_or_404(&page).await
    }

    async fn serve_or_404(path: &Path) -> hyper::Result<HttpResponse> {
        Ok(serve_file(path, is_livereload()).await.unwrap_or_else(|_| {
            hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(
                    Full::new("404 Not Found".into())
                        .map_err(|_| unreachable!())
                        .boxed(),
                )
                .unwrap()
        }))
    }
}
