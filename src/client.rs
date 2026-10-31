use std::io::{self, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

use crate::model::RuntimeInfo;
use crate::protocol::{ClientMsg, Hello, HolderMsg, PROTOCOL_VERSION, Snapshot, Welcome};

pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(1_500);
const PEEK_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn send(&self, msg: &ClientMsg) -> io::Result<()> {
        let mut stream = &self.stream;
        stream.write_all(&msg.to_frame())?;
        stream.flush()
    }

    pub fn close(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn connect(runtime: &RuntimeInfo) -> io::Result<(Connection, TcpStream, Welcome)> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, runtime.port));
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    ClientMsg::Hello(Hello { token: runtime.token.clone(), protocol: PROTOCOL_VERSION }).write(&mut stream)?;
    let welcome = match HolderMsg::read(&mut stream)? {
        HolderMsg::Welcome(welcome) => welcome,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected handshake reply")),
    };
    if welcome.protocol != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("holder speaks protocol {} (expected {PROTOCOL_VERSION})", welcome.protocol),
        ));
    }
    stream.set_read_timeout(None)?;
    let reader = stream.try_clone()?;
    Ok((Connection { stream }, reader, welcome))
}

pub fn peek(runtime: &RuntimeInfo) -> io::Result<Snapshot> {
    let (conn, mut reader, _) = connect(runtime)?;
    reader.set_read_timeout(Some(PEEK_TIMEOUT))?;
    conn.send(&ClientMsg::Peek)?;
    loop {
        if let HolderMsg::Snapshot(snapshot) = HolderMsg::read(&mut reader)? {
            return Ok(snapshot);
        }
    }
}

pub fn spawn_reader<F>(mut reader: TcpStream, mut on_message: F) -> io::Result<()>
where
    F: FnMut(Option<HolderMsg>) -> bool + Send + 'static,
{
    thread::Builder::new()
        .name("orkai-conn-reader".into())
        .spawn(move || {
            loop {
                match HolderMsg::read(&mut reader) {
                    Ok(msg) => {
                        if !on_message(Some(msg)) {
                            return;
                        }
                    }
                    Err(_) => {
                        on_message(None);
                        return;
                    }
                }
            }
        })
        .map(|_| ())
}
