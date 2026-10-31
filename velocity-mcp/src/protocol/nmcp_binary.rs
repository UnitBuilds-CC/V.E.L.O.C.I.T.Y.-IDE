use crate::ipc::shmem::{self, SharedMemoryBuffer};
use serde_json::json;
use std::error::Error;

pub fn run_shmem_loop(buffer_path: &str) -> Result<(), Box<dyn Error>> {
    // stderr, not stdout: a harness that captures the child's output into a file
    // stores whatever the server prints there as if it were protocol traffic.
    log::info!("Initializing Shared Memory Buffer at: {}", buffer_path);
    eprintln!("Initializing Shared Memory Buffer at: {}", buffer_path);
    let mut buffer = SharedMemoryBuffer::create_or_open(buffer_path)?;
    log::info!("Shared Memory Server ready. Waiting for host requests...");
    eprintln!("Shared Memory Server initialized. Waiting for host requests...");

    loop {
        // Block until request event is signaled (native wait on Windows, sleep on non-Windows)
        buffer.wait_for_request();

        let state = buffer.get_state();
        if state == shmem::STATE_REQ_READY {
            // Set state to processing instantly to lock the buffer
            buffer.set_state(shmem::STATE_PROCESSING);
            buffer.flush()?;

            // Read the binary JSON-RPC input from the shared memory request region
            match buffer.read_input() {
                Ok(input_str) => {
                    // One protocol face, one set of rules. This loop and the stdio
                    // loop used to hold near-identical copies of the request
                    // switch, and they had drifted: the shared-memory side answers
                    // no `initialize`, no `ping`, and reported every failed tool
                    // call as one prose string, so a workspace refusal and a
                    // handler fault were the same fact to the host.
                    let response = super::json_rpc::response_for_transport(&input_str);
                    // STATE_ERROR means "there is no answer to read", which is the
                    // one thing the host cannot act on. Anything else — including
                    // a refusal or an unknown method — is an answer in the buffer,
                    // and the host learns what kind from the payload.
                    let state = if super::json_rpc::is_unanswerable(&response) {
                        shmem::STATE_ERROR
                    } else {
                        shmem::STATE_RES_READY
                    };
                    let res_str = serde_json::to_string(&response)?;
                    buffer.write_output(&res_str)?;
                    buffer.set_state(state);
                    buffer.flush()?;
                    buffer.signal_response();
                }
                Err(e) => {
                    let err_res = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32603, "message": format!("Internal memory error: {}", e) },
                        "id": null
                    });
                    let res_str = serde_json::to_string(&err_res)?;
                    let _ = buffer.write_output(&res_str);
                    buffer.set_state(shmem::STATE_ERROR);
                    let _ = buffer.flush();
                    buffer.signal_response();
                }
            }
        }
    }
}

// Zero-allocation binary parser specifications for custom high-speed binary drivers
#[allow(dead_code)]
pub struct NmcpBinaryFrame<'a> {
    pub magic: &'a [u8; 4],
    pub merkle_root: &'a [u8; 32],
    pub payload: &'a [u8],
}

#[allow(dead_code)]
impl<'a> NmcpBinaryFrame<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() < 36 {
            return Err("Buffer too small for NMCP binary frame header");
        }

        // SAFETY: We verified bytes.len() >= 36 above, so:
        // - bytes[0..4] is a valid 4-byte slice that can be reinterpret as *const [u8; 4]
        // - bytes[4..36] is a valid 32-byte slice that can be reinterpret as *const [u8; 32]
        // The pointer casts are sound because the slices have the exact size of the target arrays.
        let magic = unsafe { &*(bytes[0..4].as_ptr() as *const [u8; 4]) };
        if magic != b"NMCP" {
            return Err("Invalid NMCP magic signature");
        }

        // SAFETY: bytes[4..36] is exactly 32 bytes; pointer cast to *const [u8; 32] is sound.
        let merkle_root = unsafe { &*(bytes[4..36].as_ptr() as *const [u8; 32]) };
        let payload = &bytes[36..];

        Ok(NmcpBinaryFrame {
            magic,
            merkle_root,
            payload,
        })
    }
}
