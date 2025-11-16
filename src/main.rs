use std::net::SocketAddr;
use zero::server::ZeroServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = ZeroServer::new(
        SocketAddr::from(([127, 0, 0, 1], 3000)),
        std::env::current_dir()?.join("example"),
    );

    println!("{server}");
    server.run().await
}
