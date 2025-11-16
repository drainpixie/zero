use std::path::Path;
use std::thread;
use std::{path::PathBuf, sync::mpsc};
use tokio_util::bytes;

use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Frame;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::broadcast;

pub mod server;
pub mod ws;

pub const LIVE_RELOAD_SCRIPT: &str = r#"
<script>
(function() {
    const ws = new WebSocket('ws://' + window.location.host + '/__live_reload');
    ws.onmessage = () => { console.log('reloading');
        window.location.reload();
    };
    ws.onclose = () => {
        console.log('disconnected');
        setTimeout(() => window.location.reload(), 1000);
    };
})();
</script>
"#;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type HttpResponse =
    hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::io::Error>>;

pub fn is_livereload() -> bool {
    std::env::var("PROD").map_or(true, |v| v == "dev")
}

pub async fn serve_file(path: &Path, inject: bool) -> Result<HttpResponse, std::io::Error> {
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

        return Ok(hyper::Response::builder()
            .header("Content-Type", content_type)
            .body(
                Full::new(format!("{}{}", contents, LIVE_RELOAD_SCRIPT).into())
                    .map_err(|_| unreachable!())
                    .boxed(),
            )
            .unwrap());
    }

    Ok(hyper::Response::builder()
        .header("Content-Type", content_type)
        .body(BodyExt::boxed(StreamBody::new(
            ReaderStream::new(file).map_ok(Frame::data),
        )))
        .unwrap())
}

// TODO: Debounce, file-system events are noisy
pub fn watch(reload_tx: broadcast::Sender<()>, watch_path: PathBuf) -> Result<(), BoxError> {
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(tx).expect("failed to create watcher");

        watcher
            .watch(&watch_path, RecursiveMode::Recursive)
            .expect("failed to watch path");

        rx.into_iter()
            .filter_map(|res| res.ok())
            .filter(|event| {
                matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                )
            })
            .for_each(|_| {
                let _ = reload_tx.send(());
            });
    });

    Ok(())
}
