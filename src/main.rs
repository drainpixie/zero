use std::net::SocketAddr;

use crate::server::ZeroServer;

pub mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    let root = std::env::current_dir()?.join("example");
    let server = ZeroServer::new(addr, root.to_str().unwrap());

    println!("{}", server);
    server.run().await?;

    Ok(())
}
