use std::io::{Read, Write};
use std::net::TcpStream;
use crate::net::tls::ProxyResolver;

/// WebSocket opcodes per RFC 6455.
#[allow(dead_code)]
const OPCODE_CONTINUATION: u8 = 0x0;
const OPCODE_TEXT: u8 = 0x1;
const OPCODE_BINARY: u8 = 0x2;
const OPCODE_CLOSE: u8 = 0x8;
const OPCODE_PING: u8 = 0x9;
const OPCODE_PONG: u8 = 0xA;

pub struct WsFrame {
    pub fin: bool,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

impl WsFrame {
    /// Decode a text frame payload as UTF-8.
    pub fn text(&self) -> Option<String> {
        if self.opcode == OPCODE_TEXT {
            String::from_utf8(self.payload.clone()).ok()
        } else {
            None
        }
    }

    /// Whether this is a close frame.
    pub fn is_close(&self) -> bool {
        self.opcode == OPCODE_CLOSE
    }
}

pub struct NativeWsClient {
    pub stream: TcpStream,
    pub is_connected: bool,
}

impl NativeWsClient {
    /// Connect with HTTP/1.1 upgrade handshake (ws:// only; wss:// should use
    /// TLS wrapper externally and pass the underlying stream).
    pub fn connect(host: &str, port: u16, path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)?;

        let key = base64_ws_key();
        let handshake = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            path, host, key
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

    /// Connect through a proxy resolver (HTTP CONNECT / SOCKS5 tunnel first,
    /// then WebSocket upgrade over the tunneled stream).
    pub fn connect_via(
        resolver: &ProxyResolver,
        host: &str,
        port: u16,
        path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = resolver.connect_tcp(host, port)?;

        let key = base64_ws_key();
        let handshake = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            path, host, key
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

    /// Send a masked text frame.
    pub fn send_text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_frame(OPCODE_TEXT, text.as_bytes())
    }

    /// Send a masked binary frame.
    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_frame(OPCODE_BINARY, data)
    }

    /// Send a close frame with optional status code.
    pub fn send_close(&mut self, code: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = code.to_be_bytes();
        self.send_frame(OPCODE_CLOSE, &payload)?;
        self.is_connected = false;
        Ok(())
    }

    /// Read the next frame from the server (blocks until data available).
    pub fn recv_frame(&mut self) -> Result<WsFrame, Box<dyn std::error::Error + Send + Sync>> {
        let mut header = [0u8; 2];
        self.stream.read_exact(&mut header)?;

        let fin = (header[0] & 0x80) != 0;
        let opcode = header[0] & 0x0F;
        let masked = (header[1] & 0x80) != 0;
        let mut payload_len = (header[1] & 0x7F) as u64;

        if payload_len == 126 {
            let mut ext = [0u8; 2];
            self.stream.read_exact(&mut ext)?;
            payload_len = u16::from_be_bytes(ext) as u64;
        } else if payload_len == 127 {
            let mut ext = [0u8; 8];
            self.stream.read_exact(&mut ext)?;
            payload_len = u64::from_be_bytes(ext);
        }

        let mask_key = if masked {
            let mut mk = [0u8; 4];
            self.stream.read_exact(&mut mk)?;
            Some(mk)
        } else {
            None
        };

        let mut payload = vec![0u8; payload_len as usize];
        self.stream.read_exact(&mut payload)?;

        if let Some(mk) = mask_key {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mk[i % 4];
            }
        }

        // Auto-respond to Ping with Pong
        if opcode == OPCODE_PING {
            self.send_frame(OPCODE_PONG, &payload)?;
        }
        if opcode == OPCODE_CLOSE {
            self.is_connected = false;
        }

        Ok(WsFrame { fin, opcode, payload })
    }

    /// Low-level: send a masked frame with given opcode.
    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let len = payload.len();
        let mut frame = Vec::with_capacity(len + 14);

        // FIN + opcode
        frame.push(0x80 | opcode);

        // Mask bit set + payload length encoding
        if len < 126 {
            frame.push(0x80 | (len as u8));
        } else if len <= 65535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }

        // Masking key (deterministic for agent reproducibility)
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

/// Generate a base64-encoded 16-byte key for the WebSocket handshake.
fn base64_ws_key() -> String {
    // Deterministic for agent reproducibility
    "dGhlIHNhbXBsZSBub25jZQ==".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_frame_text_decode() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_TEXT,
            payload: b"Hello".to_vec(),
        };
        assert_eq!(frame.text(), Some("Hello".to_string()));
        assert!(!frame.is_close());
    }

    #[test]
    fn ws_frame_close_detection() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_CLOSE,
            payload: vec![0x03, 0xE8], // code 1000
        };
        assert!(frame.is_close());
        assert_eq!(frame.text(), None);
    }

    #[test]
    fn ws_frame_binary_no_text() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_BINARY,
            payload: vec![0xFF, 0xFE],
        };
        assert_eq!(frame.text(), None);
        assert!(!frame.is_close());
    }

    #[test]
    fn ws_frame_continuation_not_close() {
        let frame = WsFrame {
            fin: false,
            opcode: OPCODE_CONTINUATION,
            payload: vec![1, 2, 3],
        };
        assert!(!frame.is_close());
        assert_eq!(frame.text(), None);
    }

    #[test]
    fn ws_frame_ping_not_close() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_PING,
            payload: vec![],
        };
        assert!(!frame.is_close());
    }

    #[test]
    fn ws_frame_pong_not_close() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_PONG,
            payload: vec![],
        };
        assert!(!frame.is_close());
    }

    #[test]
    fn base64_ws_key_is_deterministic() {
        let k1 = base64_ws_key();
        let k2 = base64_ws_key();
        assert_eq!(k1, k2);
        assert!(!k1.is_empty());
    }

    #[test]
    fn ws_frame_empty_text() {
        let frame = WsFrame {
            fin: true,
            opcode: OPCODE_TEXT,
            payload: vec![],
        };
        assert_eq!(frame.text(), Some(String::new()));
    }
}
