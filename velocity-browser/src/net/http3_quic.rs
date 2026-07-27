/// QUIC stream states per the QUIC transport protocol.
#[derive(Debug, Clone, PartialEq)]
pub enum QuicStreamState {
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
    Reset,
}

/// A QUIC bidirectional or unidirectional stream.
#[derive(Debug, Clone)]
pub struct QuicStream {
    pub stream_id: u64,
    pub data_buffer: Vec<u8>,
    pub state: QuicStreamState,
    pub is_unidirectional: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub priority: u32,
}

/// QUIC connection states.
#[derive(Debug, Clone, PartialEq)]
pub enum QuicConnState {
    Idle,
    Handshaking,
    Connected,
    Closing,
    Draining,
    Closed,
}

/// QUIC connection with full protocol state tracking.
pub struct QuicConnection {
    pub connection_id: String,
    pub streams: Vec<QuicStream>,
    pub state: QuicConnState,
    pub peer_addr: String,
    pub tls_version: u16,
    pub alpn: String,
    pub max_streams_bidi: u64,
    pub max_streams_uni: u64,
    pub idle_timeout_ms: u64,
    pub rtt_us: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_lost: u64,
    next_stream_id: u64,
}

impl QuicConnection {
    /// Create a new QUIC connection in Idle state.
    pub fn connect(peer_addr: &str) -> Self {
        Self {
            connection_id: format!("quic_conn_{}", peer_addr),
            streams: Vec::new(),
            state: QuicConnState::Handshaking,
            peer_addr: peer_addr.to_string(),
            tls_version: 0x0304,
            alpn: "h3".to_string(),
            max_streams_bidi: 100,
            max_streams_uni: 100,
            idle_timeout_ms: 30_000,
            rtt_us: 50_000,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_lost: 0,
            next_stream_id: 1,
        }
    }

    /// Complete the handshake and transition to Connected.
    pub fn complete_handshake(&mut self) -> Result<(), &'static str> {
        if self.state != QuicConnState::Handshaking {
            return Err("Invalid state transition: not handshaking");
        }
        self.state = QuicConnState::Connected;
        Ok(())
    }

    /// Open a new bidirectional stream.
    pub fn open_stream(&mut self) -> Result<u64, &'static str> {
        if self.state != QuicConnState::Connected {
            return Err("Connection not in Connected state");
        }
        let bidi_count = self.streams.iter().filter(|s| !s.is_unidirectional).count() as u64;
        if bidi_count >= self.max_streams_bidi {
            return Err("Max bidirectional streams exceeded");
        }
        let sid = self.next_stream_id;
        self.next_stream_id += 4; // QUIC: client-initiated bidi = 0, 4, 8, ...
        self.streams.push(QuicStream {
            stream_id: sid,
            data_buffer: Vec::new(),
            state: QuicStreamState::Open,
            is_unidirectional: false,
            bytes_sent: 0,
            bytes_received: 0,
            priority: 0,
        });
        Ok(sid)
    }

    /// Open a new unidirectional stream.
    pub fn open_uni_stream(&mut self) -> Result<u64, &'static str> {
        if self.state != QuicConnState::Connected {
            return Err("Connection not in Connected state");
        }
        let uni_count = self.streams.iter().filter(|s| s.is_unidirectional).count() as u64;
        if uni_count >= self.max_streams_uni {
            return Err("Max unidirectional streams exceeded");
        }
        let sid = self.next_stream_id;
        self.next_stream_id += 4;
        self.streams.push(QuicStream {
            stream_id: sid,
            data_buffer: Vec::new(),
            state: QuicStreamState::Open,
            is_unidirectional: true,
            bytes_sent: 0,
            bytes_received: 0,
            priority: 0,
        });
        Ok(sid)
    }

    /// Send data on a stream.
    pub fn send_data(&mut self, stream_id: u64, data: &[u8]) -> Result<usize, &'static str> {
        let stream = self.streams.iter_mut().find(|s| s.stream_id == stream_id)
            .ok_or("Stream not found")?;
        if stream.state != QuicStreamState::Open && stream.state != QuicStreamState::HalfClosedRemote {
            return Err("Stream not writable");
        }
        stream.data_buffer.extend_from_slice(data);
        stream.bytes_sent += data.len() as u64;
        self.bytes_sent += data.len() as u64;
        self.packets_sent += 1;
        Ok(data.len())
    }

    /// Close a stream gracefully.
    pub fn close_stream(&mut self, stream_id: u64) -> Result<(), &'static str> {
        let stream = self.streams.iter_mut().find(|s| s.stream_id == stream_id)
            .ok_or("Stream not found")?;
        match stream.state {
            QuicStreamState::Open => { stream.state = QuicStreamState::HalfClosedLocal; }
            QuicStreamState::HalfClosedRemote => { stream.state = QuicStreamState::Closed; }
            _ => { return Err("Cannot close stream in current state"); }
        }
        Ok(())
    }

    /// Reset a stream with an error code.
    pub fn reset_stream(&mut self, stream_id: u64, _error_code: u32) -> Result<(), &'static str> {
        let stream = self.streams.iter_mut().find(|s| s.stream_id == stream_id)
            .ok_or("Stream not found")?;
        stream.state = QuicStreamState::Reset;
        Ok(())
    }

    /// Initiate connection close.
    pub fn close(&mut self) -> Result<(), &'static str> {
        if self.state != QuicConnState::Connected {
            return Err("Not connected");
        }
        self.state = QuicConnState::Closing;
        // Close all open streams
        for stream in &mut self.streams {
            if stream.state == QuicStreamState::Open {
                stream.state = QuicStreamState::Closed;
            }
        }
        self.state = QuicConnState::Closed;
        Ok(())
    }

    /// Get connection statistics.
    pub fn stats(&self) -> QuicStats {
        QuicStats {
            state: self.state.clone(),
            stream_count: self.streams.len(),
            bytes_sent: self.bytes_sent,
            bytes_received: self.bytes_received,
            packets_sent: self.packets_sent,
            packets_lost: self.packets_lost,
            rtt_us: self.rtt_us,
            loss_rate: if self.packets_sent > 0 {
                self.packets_lost as f64 / self.packets_sent as f64
            } else { 0.0 },
        }
    }
}

/// QUIC connection statistics.
#[derive(Debug, Clone)]
pub struct QuicStats {
    pub state: QuicConnState,
    pub stream_count: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_lost: u64,
    pub rtt_us: u64,
    pub loss_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_and_handshake() {
        let mut conn = QuicConnection::connect("example.com:443");
        assert_eq!(conn.state, QuicConnState::Handshaking);
        conn.complete_handshake().unwrap();
        assert_eq!(conn.state, QuicConnState::Connected);
    }

    #[test]
    fn test_open_stream() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_stream().unwrap();
        assert!(sid > 0);
        assert_eq!(conn.streams.len(), 1);
    }

    #[test]
    fn test_open_uni_stream() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_uni_stream().unwrap();
        assert!(conn.streams.iter().find(|s| s.stream_id == sid).unwrap().is_unidirectional);
    }

    #[test]
    fn test_send_data() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_stream().unwrap();
        let sent = conn.send_data(sid, b"GET / HTTP/3\r\n").unwrap();
        assert_eq!(sent, 14);
        assert_eq!(conn.bytes_sent, 14);
    }

    #[test]
    fn test_close_stream() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_stream().unwrap();
        conn.close_stream(sid).unwrap();
        assert_eq!(conn.streams[0].state, QuicStreamState::HalfClosedLocal);
    }

    #[test]
    fn test_reset_stream() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_stream().unwrap();
        conn.reset_stream(sid, 0x01).unwrap();
        assert_eq!(conn.streams[0].state, QuicStreamState::Reset);
    }

    #[test]
    fn test_connection_close() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        conn.open_stream().unwrap();
        conn.close().unwrap();
        assert_eq!(conn.state, QuicConnState::Closed);
    }

    #[test]
    fn test_cannot_open_stream_when_not_connected() {
        let mut conn = QuicConnection::connect("example.com:443");
        assert!(conn.open_stream().is_err());
    }

    #[test]
    fn test_stats() {
        let mut conn = QuicConnection::connect("example.com:443");
        conn.complete_handshake().unwrap();
        let sid = conn.open_stream().unwrap();
        conn.send_data(sid, b"hello").unwrap();
        let stats = conn.stats();
        assert_eq!(stats.bytes_sent, 5);
        assert_eq!(stats.stream_count, 1);
    }
}
