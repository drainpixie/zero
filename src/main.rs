use crate::server::ZeroServer;

pub mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ZeroServer::new("0.0.0.0", 8080, "../example");

    server.run().await;
    println!("{}", server);

    Ok(())
}
