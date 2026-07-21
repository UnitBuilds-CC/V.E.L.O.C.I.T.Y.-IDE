use std::io::{Read, Write};
use std::net::TcpStream;

/// Custom zero-dependency RFC 6455 WebSocket client for V.E.L.O.C.I.T.Y. platform
pub struct NativeWsClient {
    stream: TcpStream,
}

impl NativeWsClient {
    pub fn connect(host: &str, port: u16, path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)?;

        // Send HTTP WebSocket Upgrade handshake
        let handshake = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
            path, addr
        );
        stream.write_all(handshake.as_bytes())?;
        stream.flush()?;

        // Read handshake response
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf)?;
        let response = String::from_utf8_lossy(&buf[..n]);

        if !response.contains("101 Switching Protocols") {
            return Err("WebSocket handshake failed".into());
        }

        Ok(Self { stream })
    }

    /// Send a text frame with client-side masking
    pub fn send_text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = text.as_bytes();
        let len = payload.len();

        let mut frame = Vec::with_capacity(len + 14);
        frame.push(0x81); // Text frame (FIN = 1, Opcode = 1)

        if len <= 125 {
            frame.push(0x80 | (len as u8)); // Masked bit set
        } else if len <= 65535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }

        let mask_key: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask_key);

        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask_key[i % 4]);
        }

        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read a text frame
    pub fn read_text(&mut self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut header = [0u8; 2];
        self.stream.read_exact(&mut header)?;

        let len_byte = header[1] & 0x7F;
        let payload_len: usize = match len_byte {
            126 => {
                let mut b = [0u8; 2];
                self.stream.read_exact(&mut b)?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0u8; 8];
                self.stream.read_exact(&mut b)?;
                u64::from_be_bytes(b) as usize
            }
            _ => len_byte as usize,
        };

        let is_masked = (header[1] & 0x80) != 0;
        let mut mask = [0u8; 4];
        if is_masked {
            self.stream.read_exact(&mut mask)?;
        }

        let mut payload = vec![0u8; payload_len];
        self.stream.read_exact(&mut payload[..])?;

        if is_masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }

        Ok(String::from_utf8(payload)?)
    }
}
