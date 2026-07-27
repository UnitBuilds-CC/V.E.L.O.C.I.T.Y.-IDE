use crate::ipc::shmem::{
    SharedMemoryBuffer, STATE_ERROR, STATE_IDLE, STATE_PROCESSING, STATE_REQ_READY, STATE_RES_READY,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;
use std::sync::atomic::AtomicU64;

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

pub struct TelemetryClient {
    shmem: SharedMemoryBuffer,
}

impl TelemetryClient {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let shmem = SharedMemoryBuffer::create_or_open(path)?;
        Ok(Self { shmem })
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

        // Write request
        let req_str = serde_json::to_string(req)?;
        self.shmem.write_input(&req_str)?;
        self.shmem.set_state(STATE_REQ_READY);
        self.shmem.flush()?;
        self.shmem.signal_request();

        // Wait for response
        self.shmem.wait_for_response();

        if self.shmem.get_state() == STATE_ERROR {
            return Err("Server returned error state".into());
        }

        let res_str = self.shmem.read_output()?;
        let res: TelemetryResponse = serde_json::from_str(&res_str)?;

        // Set state back to idle
        self.shmem.set_state(STATE_IDLE);
        self.shmem.flush()?;

        Ok(res)
    }
}

pub struct TelemetryServer {
    shmem: SharedMemoryBuffer,
}

impl TelemetryServer {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let shmem = SharedMemoryBuffer::create_or_open(path)?;
        Ok(Self { shmem })
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

            self.shmem.set_state(STATE_PROCESSING);
            self.shmem.flush()?;

            let req_str = match self.shmem.read_input() {
                Ok(s) => s,
                Err(_) => {
                    self.shmem.set_state(STATE_ERROR);
                    let _ = self.shmem.flush();
                    self.shmem.signal_response();
                    continue;
                }
            };

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

            if self.shmem.write_output(&res_str).is_err() {
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

    #[test]
    fn test_telemetry_shared_memory_communication() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let path_clone = path.clone();

        // Spawn server in a background thread
        let _handle = thread::spawn(move || {
            let mut server = TelemetryServer::open(&path_clone).unwrap();
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

        let mut client = TelemetryClient::open(&path).unwrap();

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
}
