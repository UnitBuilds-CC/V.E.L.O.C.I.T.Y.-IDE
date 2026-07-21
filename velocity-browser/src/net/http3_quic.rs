#[derive(Debug, Clone)]
pub struct QuicStream {
    pub stream_id: u64,
    pub data_buffer: Vec<u8>,
}

pub struct QuicConnection {
    pub connection_id: String,
    pub streams: Vec<QuicStream>,
}

impl QuicConnection {
    pub fn connect(peer_addr: &str) -> Self {
        Self {
            connection_id: format!("quic_conn_{}", peer_addr),
            streams: Vec::new(),
        }
    }

    pub fn open_stream(&mut self) -> u64 {
        let sid = self.streams.len() as u64 + 1;
        self.streams.push(QuicStream {
            stream_id: sid,
            data_buffer: Vec::new(),
        });
        sid
    }
}
