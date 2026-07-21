use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Debug, Clone)]
pub enum ProxyType {
    Direct,
    Http(String),
    Socks5(String),
}

pub struct ProxyResolver {
    pub proxy_type: ProxyType,
}

impl ProxyResolver {
    pub fn direct() -> Self {
        Self { proxy_type: ProxyType::Direct }
    }
}

pub enum TlsState {
    Uninitialized,
    HandshakeInProgress,
    Connected,
    Failed(String),
}

pub struct NativeTlsStream {
    pub socket: Option<TcpStream>,
    pub state: TlsState,
    pub hostname: String,
}

impl NativeTlsStream {
    pub fn connect(addr: &str, hostname: &str) -> Result<Self, std::io::Error> {
        let stream = TcpStream::connect(addr)?;
        Ok(Self {
            socket: Some(stream),
            state: TlsState::Connected,
            hostname: hostname.to_string(),
        })
    }

    pub fn write_all(&mut self, buf: &[u8]) -> Result<(), std::io::Error> {
        if let Some(sock) = &mut self.socket {
            sock.write_all(buf)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "TLS Socket not connected"))
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        if let Some(sock) = &mut self.socket {
            sock.read(buf)
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "TLS Socket not connected"))
        }
    }
}
