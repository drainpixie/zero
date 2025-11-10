use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::{fs::File, net::TcpListener};
use tokio_util::io::ReaderStream;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type HttpResponse = Response<BoxBody<Bytes, std::io::Error>>;

pub struct ZeroServer {
    pub addr: SocketAddr,
    pub root: PathBuf,
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
        }
    }

    pub async fn run(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.addr).await?;
        let root = Arc::new(self.root.clone());

        loop {
            let (stream, _) = listener.accept().await?;
            let root = Arc::clone(&root);

            tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let root = Arc::clone(&root);
                    async move { Self::serve(root, req).await }
                });

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
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

    async fn serve(
        root: Arc<PathBuf>,
        req: Request<impl hyper::body::Body>,
    ) -> hyper::Result<HttpResponse> {
        let path = req.uri().path();

        if path.starts_with("/static/") {
            let file_path = root.join(path.trim_start_matches('/'));

            return Ok(Self::serve_file(&file_path)
                .await
                .unwrap_or_else(|_| Self::not_found()));
        }

        let mut page_path = root.join("pages").join(path.trim_start_matches('/'));
        if page_path.is_dir() || path == "/" {
            page_path.push("index.html");
        } else if page_path.extension().is_none() {
            page_path.set_extension("html");
        }

        Ok(Self::serve_file(&page_path)
            .await
            .unwrap_or_else(|_| Self::not_found()))
    }

    async fn serve_file(path: &Path) -> Result<HttpResponse, std::io::Error> {
        let file = File::open(path).await?;
        let content_type = match path.extension().and_then(|ext| ext.to_str()).unwrap() {
            "html" => "text/html",
            "css" => "text/css",
            "js" => "application/javascript",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "json" => "application/json",
            "wasm" => "application/wasm",
            _ => "application/octet-stream",
        };

        Ok(Response::builder()
            .header("Content-Type", content_type)
            .body(StreamBody::new(ReaderStream::new(file).map_ok(Frame::data)).boxed())
            .unwrap())
    }
}
