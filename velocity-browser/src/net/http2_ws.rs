use std::io::{Read, Write};
use std::net::TcpStream;

pub struct WsFrame {
    pub fin: bool,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

pub struct NativeWsClient {
    pub stream: TcpStream,
    pub is_connected: bool,
}

impl NativeWsClient {
    pub fn connect(host: &str, port: u16, path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)?;

        let handshake = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            path, host
        );
        stream.write_all(handshake.as_bytes())?;
        stream.flush()?;

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf)?;
        let response = String::from_utf8_lossy(&buf[..n]);

        if !response.contains("101") && !response.contains("Switching Protocols") {
            return Err("WebSocket handshake failed".into());
        }

        Ok(Self { stream, is_connected: true })
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = text.as_bytes();
        let mut frame = Vec::with_capacity(payload.len() + 10);
        frame.push(0x81); // FIN + Text frame
        frame.push(0x80 | (payload.len() as u8)); // Masked bit
        let mask = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask);
        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i % 4]);
        }
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }
}
