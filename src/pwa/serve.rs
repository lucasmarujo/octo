use std::io::{self, Read, Write};
use std::sync::Arc;
use std::thread;

use crate::paths::Home;
use crate::pwa::bridge;
use crate::pwa::crypto::{self, Secret};
use crate::pwa::hub::Hub;
use crate::pwa::server::Server;

pub const READY: &str = "octo-ready";
pub const FAILED: &str = "octo-error";
pub const PORT_TAKEN: &str = "port";

pub fn ready_line(port: u16, secret: &Secret) -> String {
    format!("{READY} {port} {}", crypto::encode(secret))
}

pub fn parse_ready(line: &str) -> Option<(u16, Secret)> {
    let mut parts = line.split_whitespace();
    if parts.next()? != READY {
        return None;
    }
    let port = parts.next()?.parse().ok()?;
    let secret = crypto::decode_secret(parts.next()?)?;
    parts.next().is_none().then_some((port, secret))
}

pub fn run(home: &Home, port: u16) -> io::Result<()> {
    let hub = Hub::open(home)?;
    let secret = bridge::load_secret(home)?;
    let server = match Server::bind(port, Arc::clone(&hub), secret) {
        Ok(server) => server,
        Err(err) => {
            if err.kind() == io::ErrorKind::AddrInUse {
                println!("{FAILED} {PORT_TAKEN}");
            }
            return Err(err);
        }
    };
    println!("{}", ready_line(server.port(), &secret));
    io::stdout().flush()?;
    thread::Builder::new().name("orkai-serve-accept".into()).spawn(move || server.serve())?;
    let mut ignored = [0u8; 256];
    let mut stdin = io::stdin().lock();
    while stdin.read(&mut ignored).is_ok_and(|read| read > 0) {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_that_says_the_server_is_up_carries_its_port_and_key() {
        let secret = crypto::secret().unwrap();
        let line = ready_line(50123, &secret);
        assert_eq!(parse_ready(&line), Some((50123, secret)));
        assert_eq!(parse_ready(&format!("  {line}\r\n")), Some((50123, secret)));
        assert_eq!(parse_ready("octo-ready 50123"), None);
        assert_eq!(parse_ready("octo-ready x key"), None);
        assert_eq!(parse_ready(&format!("{line} more")), None);
        assert_eq!(parse_ready("octo-error port"), None);
    }
}
