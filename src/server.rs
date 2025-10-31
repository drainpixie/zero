use std::{fmt, net::SocketAddr, path::PathBuf, sync::Arc};

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

pub struct ZeroServer<'a> {
    pub addr: SocketAddr,
    pub root: &'a str,
}

impl fmt::Display for ZeroServer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ZeroServer(address: {}, root: {})", self.addr, self.root)
    }
}

impl ZeroServer<'_> {
    pub fn new<'a>(addr: SocketAddr, root: &'a str) -> ZeroServer<'a> {
        ZeroServer { addr, root }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.addr).await?;
        let root = Arc::new(self.root.to_string());

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let root = Arc::clone(&root);

            tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let root = Arc::clone(&root);
                    async move { Self::serve_path(&root, req).await }
                });

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                {
                    eprintln!("error serving connection: {:?}", err);
                }
            });
        }
    }

    fn not_found() -> Response<BoxBody<Bytes, std::io::Error>> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(
                Full::new("404 Not Found".into())
                    .map_err(|e| match e {})
                    .boxed(),
            )
            .unwrap()
    }

    async fn serve_path(
        root: &str,
        req: Request<impl hyper::body::Body>,
    ) -> hyper::Result<Response<BoxBody<Bytes, std::io::Error>>> {
        let request_path = req.uri().path().trim_start_matches("/");
        let mut file_path = PathBuf::from(root).join(request_path);

        if file_path.is_dir() {
            file_path.push("index.html");
        }

        if !file_path.exists() {
            return Ok(Self::not_found());
        }

        match File::open(&file_path).await {
            Ok(file) => {
                // TODO: Use a proper mime detection library
                let content_type = match file_path.extension().and_then(|s| s.to_str()) {
                    Some("html") => "text/html",
                    Some("css") => "text/css",
                    Some("js") => "application/javascript",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("gif") => "image/gif",
                    _ => "application/octet-stream",
                };

                Ok(Response::builder()
                    .header("Content-Type", content_type)
                    .body(StreamBody::new(ReaderStream::new(file).map_ok(Frame::data)).boxed())
                    .unwrap())
            }
            Err(_) => Ok(Self::not_found()),
        }
    }
}
