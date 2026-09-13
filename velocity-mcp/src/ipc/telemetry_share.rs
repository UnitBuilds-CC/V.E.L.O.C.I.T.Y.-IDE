use crate::ipc::shmem::{
    SharedMemoryBuffer, STATE_ERROR, STATE_IDLE, STATE_PROCESSING, STATE_REQ_READY, STATE_RES_READY,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

pub static TELEMETRY_LATENCY_US: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TelemetryRequest {
    AstUpdate {
        file_path: String,
        triples: Vec<(u64, u16, u64)>, // Subject, Predicate, Object
    },
    AstDelete {
        file_path: String,
    },
    PresenceUpdate {
        cursor_line: usize,
        cursor_col: usize,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TelemetryResponse {
    pub success: bool,
    pub warning: Option<String>,
}

// ─── Rate Limiter ──────────────────────────────────────────────────────────────

/// Token bucket rate limiter for IPC message processing
pub struct RateLimiter {
    max_tokens: u32,
    tokens: u32,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(max_tokens: u32, refill_rate: f64) -> Self {
        Self {
            max_tokens,
            tokens: max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Try to consume a token. Returns true if allowed, false if rate limited.
    pub fn try_acquire(&mut self) -> bool {
        self.refill();
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let new_tokens = elapsed * self.refill_rate;
        self.tokens = (self.tokens + new_tokens as u32).min(self.max_tokens);
        self.last_refill = now;
    }
}

// ─── Authenticated Telemetry Client ────────────────────────────────────────────

pub struct TelemetryClient {
    shmem: SharedMemoryBuffer,
    key: Vec<u8>,
    seq: u32,
}

impl TelemetryClient {
    pub fn open<P: AsRef<Path>>(path: P, key: &[u8]) -> Result<Self, Box<dyn Error>> {
        let shmem = SharedMemoryBuffer::create_or_open(path)?;
        Ok(Self {
            shmem,
            key: key.to_vec(),
            seq: 0,
        })
    }

    pub fn send(&mut self, req: &TelemetryRequest) -> Result<TelemetryResponse, Box<dyn Error>> {
        // Spin lock until STATE_IDLE or timeout
        let mut attempts = 0;
        while self.shmem.get_state() != STATE_IDLE {
            std::thread::sleep(std::time::Duration::from_millis(5));
            attempts += 1;
            if attempts > 200 {
                return Err("Timeout waiting for Shared Memory channel to become IDLE".into());
            }
        }

        // Increment sequence number for replay protection
        self.seq = self.seq.wrapping_add(1);

        // Write authenticated request with HMAC
        let req_str = serde_json::to_string(req)?;
        self.shmem
            .write_authenticated_input(&req_str, &self.key, self.seq)?;
        self.shmem.set_state(STATE_REQ_READY);
        self.shmem.flush()?;
        self.shmem.signal_request();

        // Wait for response
        self.shmem.wait_for_response();

        if self.shmem.get_state() == STATE_ERROR {
            return Err("Server returned error state".into());
        }

        // Read and verify authenticated response
        let (res_str, _res_seq) = self
            .shmem
            .read_authenticated_output(&self.key, Some(self.seq))?;
        let res: TelemetryResponse = serde_json::from_str(&res_str)?;

        // Set state back to idle
        self.shmem.set_state(STATE_IDLE);
        self.shmem.flush()?;

        Ok(res)
    }
}

// ─── Authenticated Telemetry Server ────────────────────────────────────────────

pub struct TelemetryServer {
    shmem: SharedMemoryBuffer,
    key: Vec<u8>,
    rate_limiter: RateLimiter,
    last_seq: u32,
}

impl TelemetryServer {
    pub fn open<P: AsRef<Path>>(path: P, key: &[u8]) -> Result<Self, Box<dyn Error>> {
        let shmem = SharedMemoryBuffer::create_or_open(path)?;
        Ok(Self {
            shmem,
            key: key.to_vec(),
            rate_limiter: RateLimiter::new(100, 10.0), // 100 max burst, 10 tokens/sec refill
            last_seq: 0,
        })
    }

    pub fn listen<F>(&mut self, mut handler: F) -> Result<(), Box<dyn Error>>
    where
        F: FnMut(TelemetryRequest) -> TelemetryResponse,
    {
        loop {
            // Wait for a request
            self.shmem.wait_for_request();

            if self.shmem.get_state() != STATE_REQ_READY {
                continue;
            }

            // Rate limiting check
            if !self.rate_limiter.try_acquire() {
                eprintln!("IPC rate limit exceeded - dropping message");
                self.shmem.set_state(STATE_ERROR);
                let _ = self.shmem.flush();
                self.shmem.signal_response();
                continue;
            }

            self.shmem.set_state(STATE_PROCESSING);
            self.shmem.flush()?;

            // Read and verify authenticated request
            let expected_seq = self.last_seq.wrapping_add(1);
            let (req_str, req_seq) = match self
                .shmem
                .read_authenticated_input(&self.key, Some(expected_seq))
            {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("IPC authentication failed: {}", e);
                    self.shmem.set_state(STATE_ERROR);
                    let _ = self.shmem.flush();
                    self.shmem.signal_response();
                    continue;
                }
            };

            // Update last seen sequence number
            self.last_seq = req_seq;

            let req: TelemetryRequest = match serde_json::from_str(&req_str) {
                Ok(r) => r,
                Err(_) => {
                    self.shmem.set_state(STATE_ERROR);
                    let _ = self.shmem.flush();
                    self.shmem.signal_response();
                    continue;
                }
            };

            let res = handler(req);

            let res_str = match serde_json::to_string(&res) {
                Ok(s) => s,
                Err(_) => {
                    self.shmem.set_state(STATE_ERROR);
                    let _ = self.shmem.flush();
                    self.shmem.signal_response();
                    continue;
                }
            };

            // Write authenticated response with HMAC
            if self
                .shmem
                .write_authenticated_output(&res_str, &self.key, req_seq)
                .is_err()
            {
                self.shmem.set_state(STATE_ERROR);
                let _ = self.shmem.flush();
                self.shmem.signal_response();
                continue;
            }

            self.shmem.set_state(STATE_RES_READY);
            self.shmem.flush()?;
            self.shmem.signal_response();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    const TEST_KEY: &[u8] = b"test_hmac_key_for_ipc_authentication";

    #[test]
    fn test_telemetry_shared_memory_communication() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let path_clone = path.clone();

        // Spawn server in a background thread
        let _handle = thread::spawn(move || {
            let mut server = TelemetryServer::open(&path_clone, TEST_KEY).unwrap();
            server
                .listen(|req| match req {
                    TelemetryRequest::AstUpdate { file_path, triples } => {
                        assert_eq!(file_path, "src/main.rs");
                        assert_eq!(triples.len(), 2);
                        TelemetryResponse {
                            success: true,
                            warning: None,
                        }
                    }
                    TelemetryRequest::AstDelete { file_path } => {
                        assert_eq!(file_path, "src/deleted.rs");
                        TelemetryResponse {
                            success: true,
                            warning: None,
                        }
                    }
                    TelemetryRequest::PresenceUpdate {
                        cursor_line,
                        cursor_col,
                    } => {
                        assert_eq!(cursor_line, 42);
                        assert_eq!(cursor_col, 10);
                        TelemetryResponse {
                            success: true,
                            warning: Some("Overlap warning!".to_string()),
                        }
                    }
                })
                .ok();
        });

        // Sleep to give server time to set up
        std::thread::sleep(Duration::from_millis(50));

        let mut client = TelemetryClient::open(&path, TEST_KEY).unwrap();

        // Test AstUpdate
        let req1 = TelemetryRequest::AstUpdate {
            file_path: "src/main.rs".to_string(),
            triples: vec![(100, 1, 200), (300, 2, 400)],
        };
        let res1 = client.send(&req1).unwrap();
        assert!(res1.success);
        assert!(res1.warning.is_none());

        // Test PresenceUpdate
        let req2 = TelemetryRequest::PresenceUpdate {
            cursor_line: 42,
            cursor_col: 10,
        };
        let res2 = client.send(&req2).unwrap();
        assert!(res2.success);
        assert_eq!(res2.warning, Some("Overlap warning!".to_string()));
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(5, 1.0);

        // Should allow first 5 requests
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }

        // 6th request should be denied
        assert!(!limiter.try_acquire());

        // Wait for refill
        std::thread::sleep(Duration::from_millis(1100));

        // Should allow 1 more request after refill
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_hmac_authentication() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let path_clone = path.clone();
        let wrong_key = b"wrong_key";

        // Spawn server with correct key
        let _handle = thread::spawn(move || {
            let mut server = TelemetryServer::open(&path_clone, TEST_KEY).unwrap();
            server
                .listen(|_req| TelemetryResponse {
                    success: true,
                    warning: None,
                })
                .ok();
        });

        std::thread::sleep(Duration::from_millis(50));

        // Client with wrong key should fail
        let mut client = TelemetryClient::open(&path, wrong_key).unwrap();
        let req = TelemetryRequest::AstDelete {
            file_path: "test.rs".to_string(),
        };

        let result = client.send(&req);
        assert!(result.is_err(), "Should fail with wrong HMAC key");
    }
}
