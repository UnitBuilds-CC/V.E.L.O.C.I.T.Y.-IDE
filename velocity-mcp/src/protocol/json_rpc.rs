use serde_json::{json, Value};
use std::io::{self, BufRead};
use std::error::Error;
use crate::registry;

pub fn run_stdio_loop() -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    loop {
        let mut line = String::new();
        let bytes_read = handle.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                let err_res = json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32700, "message": "Parse error" },
                    "id": null
                });
                println!("{}", err_res);
                continue;
            }
        };

        let method = request["method"].as_str().unwrap_or("");
        let id = &request["id"];

        match method {
            "initialize" => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "velocity-mcp-rust-server",
                            "version": "1.0.0"
                        }
                    }
                });
                println!("{}", response);
            }
            "tools/list" => {
                let tools = registry::get_tools();
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tools
                    }
                });
                println!("{}", response);
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

                let response = json!({
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
                });
                println!("{}", response);
            }
            _ => {
                if !id.is_null() {
                    let response = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("Method '{}' not found", method)
                        }
                    });
                    println!("{}", response);
                }
            }
        }
    }

    Ok(())
}
