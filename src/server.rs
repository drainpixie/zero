use std::fmt;

pub struct ZeroServer<'a> {
    pub address: &'a str,
    pub root: &'a str,
    pub port: u16,
}

impl fmt::Display for ZeroServer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ZeroServer(address: {}, root: {}, port: {})",
            self.address, self.root, self.port
        )
    }
}

impl ZeroServer<'_> {
    pub fn new<'a>(address: &'a str, port: u16, root: &'a str) -> ZeroServer<'a> {
        ZeroServer {
            address,
            root,
            port,
        }
    }

    pub async fn run(&self) {}
}
