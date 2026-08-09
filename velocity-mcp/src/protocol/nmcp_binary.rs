use crate::ipc::shmem::{self, SharedMemoryBuffer};
use crate::registry;
use serde_json::{json, Value};
use std::error::Error;

pub fn run_shmem_loop(buffer_path: &str) -> Result<(), Box<dyn Error>> {
    println!("Initializing Shared Memory Buffer at: {}", buffer_path);
    let mut buffer = SharedMemoryBuffer::create_or_open(buffer_path)?;
    println!("Shared Memory Server initialized. Waiting for host requests...");

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
                    let request: Value = match serde_json::from_str(&input_str) {
                        Ok(v) => v,
                        Err(e) => {
                            let err_res = json!({
                                "jsonrpc": "2.0",
                                "error": { "code": -32700, "message": format!("Parse error: {}", e) },
                                "id": null
                            });
                            let res_str = serde_json::to_string(&err_res)?;
                            let _ = buffer.write_output(&res_str);
                            buffer.set_state(shmem::STATE_ERROR);
                            let _ = buffer.flush();
                            buffer.signal_response();
                            continue;
                        }
                    };

                    let method = request["method"].as_str().unwrap_or("");
                    let id = &request["id"];

                    let response = match method {
                        "tools/list" => {
                            let tools = registry::get_tools();
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "tools": tools }
                            })
                        }
                        "tools/call" => {
                            let name = request["params"]["name"].as_str().unwrap_or("");
                            let arguments = &request["params"]["arguments"];

                            let mut is_error = false;
                            let output_text = match registry::call_tool(name, arguments) {
                                Ok(res) => res,
                                Err(e) => {
                                    is_error = true;
                                    format!("Error running tool '{}': {}", name, e)
                                }
                            };

                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": output_text
                                        }
                                    ],
                                    "isError": is_error
                                }
                            })
                        }
                        _ => {
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32601, "message": format!("Method '{}' not supported", method) }
                            })
                        }
                    };

                    let res_str = serde_json::to_string(&response)?;
                    buffer.write_output(&res_str)?;
                    buffer.set_state(shmem::STATE_RES_READY);
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

        let merkle_root = unsafe { &*(bytes[4..36].as_ptr() as *const [u8; 32]) };
        let payload = &bytes[36..];

        Ok(NmcpBinaryFrame {
            magic,
            merkle_root,
            payload,
        })
    }
}
