use std::{fmt, net::SocketAddr};

pub struct ZeroServer<'a> {
    pub address: SocketAddr,
    pub root: &'a str,
}

impl fmt::Display for ZeroServer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ZeroServer(address: {}, root: {})",
            self.address, self.root
        )
    }
}

impl ZeroServer<'_> {
    pub fn new<'a>(address: SocketAddr, root: &'a str) -> ZeroServer<'a> {
        ZeroServer { address, root }
    }

    pub async fn run(&self) {}
}
