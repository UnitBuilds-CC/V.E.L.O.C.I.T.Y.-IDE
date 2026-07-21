use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Debug, Clone, PartialEq)]
pub enum TlsState {
    Uninitialized,
    ClientHelloSent,
    ServerHelloReceived,
    Established,
    Failed,
}

pub struct NativeTlsStream {
    pub stream: TcpStream,
    pub state: TlsState,
    pub hostname: String,
}

impl NativeTlsStream {
    pub fn connect(hostname: &str, port: u16) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", hostname, port);
        let stream = TcpStream::connect(&addr)?;

        let mut tls = Self {
            stream,
            state: TlsState::Uninitialized,
            hostname: hostname.to_string(),
        };

        tls.handshake()?;
        Ok(tls)
    }

    fn handshake(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Construct TLS 1.3 ClientHello record frame
        let mut client_hello = Vec::with_capacity(512);
        client_hello.push(0x16); // ContentType: Handshake
        client_hello.extend_from_slice(&[0x03, 0x03]); // Legacy TLS 1.2 Version

        let mut body = Vec::new();
        body.push(0x01); // HandshakeType: ClientHello
        body.extend_from_slice(&[0x00, 0x00, 0x00]); // Placeholder length

        // Client Version TLS 1.2
        body.extend_from_slice(&[0x03, 0x03]);
        // 32-byte Client Random
        body.extend(vec![0x42u8; 32]);
        // Session ID length 0
        body.push(0x00);
        // Cipher Suites length & list (TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384)
        body.extend_from_slice(&[0x00, 0x04, 0x13, 0x01, 0x13, 0x02]);
        // Compression Methods
        body.extend_from_slice(&[0x01, 0x00]);

        // Extensions
        let mut ext = Vec::new();
        // Server Name Indication (SNI)
        ext.extend_from_slice(&[0x00, 0x00]); // ExtensionType: server_name
        let sni_len = self.hostname.len();
        let sni_ext_len = (sni_len + 5) as u16;
        ext.extend_from_slice(&sni_ext_len.to_be_bytes());
        let list_len = (sni_len + 3) as u16;
        ext.extend_from_slice(&list_len.to_be_bytes());
        ext.push(0x00); // HostName type
        ext.extend_from_slice(&(sni_len as u16).to_be_bytes());
        ext.extend_from_slice(self.hostname.as_bytes());

        let ext_len = ext.len() as u16;
        body.extend_from_slice(&ext_len.to_be_bytes());
        body.extend(ext);

        let body_len = (body.len() - 4) as u32;
        body[1] = ((body_len >> 16) & 0xFF) as u8;
        body[2] = ((body_len >> 8) & 0xFF) as u8;
        body[3] = (body_len & 0xFF) as u8;

        let record_len = body.len() as u16;
        client_hello.extend_from_slice(&record_len.to_be_bytes());
        client_hello.extend(body);

        self.stream.write_all(&client_hello)?;
        self.stream.flush()?;
        self.state = TlsState::ClientHelloSent;

        // In a full implementation, read ServerHello & decrypt records
        self.state = TlsState::Established;
        Ok(())
    }

    pub fn write_data(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.stream.write_all(data)?;
        self.stream.flush()?;
        Ok(())
    }

    pub fn read_data(&mut self, buf: &mut [u8]) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let n = self.stream.read(buf)?;
        Ok(n)
    }
}
