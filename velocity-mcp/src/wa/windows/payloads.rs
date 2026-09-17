use crate::wa::WaNode;
use serde::Deserialize;
use std::error::Error;
use std::io::{Error as IoError, ErrorKind};

#[derive(Debug, Deserialize)]
pub struct WindowsCapturePayload {
    #[serde(default)]
    pub window_title: String,
    #[serde(default)]
    pub process_id: Option<u32>,
    #[serde(default)]
    pub focus_node_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WaNode>,
}

#[derive(Debug, Deserialize)]
pub struct WindowsActionPayload {
    #[serde(default)]
    pub window_title: String,
    #[serde(default)]
    pub process_id: Option<u32>,
    #[serde(default)]
    pub executed_node_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Deserialize)]
pub struct WindowsWaitPayload {
    #[serde(default)]
    pub window_title: String,
    #[serde(default)]
    pub process_id: Option<u32>,
    #[serde(default)]
    pub observed_value: Option<String>,
    #[serde(default)]
    pub satisfied: bool,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub detail: String,
}

/// Decode a script payload without leaking a raw `serde_json` complaint.
///
/// Bug #33: when the generated script printed nothing the caller saw
/// "EOF while parsing a value at line 1 column 0" - true but useless, because
/// it named neither the operation that failed nor what the script actually
/// emitted. Both are now in the message.
fn decode_payload<T: serde::de::DeserializeOwned>(
    json_payload: &str,
    label: &str,
) -> Result<T, Box<dyn Error>> {
    let trimmed = json_payload.trim();
    if trimmed.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!("{label} produced no output: the UIAutomation script exited clean but printed nothing"),
        )
        .into());
    }
    serde_json::from_str(trimmed).map_err(|err| {
        let head: String = trimmed.chars().take(160).collect();
        IoError::new(
            ErrorKind::InvalidData,
            format!("{label} returned unreadable JSON ({err}); script output began: {head:?}"),
        )
        .into()
    })
}

pub fn parse_capture_payload(json_payload: &str) -> Result<WindowsCapturePayload, Box<dyn Error>> {
    let payload: WindowsCapturePayload = decode_payload(json_payload, "Windows capture")?;
    if payload.nodes.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "Windows capture returned no accessible nodes",
        )
        .into());
    }
    Ok(payload)
}

pub fn parse_action_payload(json_payload: &str) -> Result<WindowsActionPayload, Box<dyn Error>> {
    let payload: WindowsActionPayload = decode_payload(json_payload, "Windows action")?;
    if payload.executed_node_id.trim().is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "Windows action reported no executed node id (status: {:?}, detail: {:?})",
                payload.status, payload.detail
            ),
        )
        .into());
    }
    Ok(payload)
}

pub fn parse_wait_payload(json_payload: &str) -> Result<WindowsWaitPayload, Box<dyn Error>> {
    decode_payload(json_payload, "Windows wait")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_script_output_names_the_operation_instead_of_an_eof_parse_error() {
        for (label, result) in [
            (
                "capture",
                parse_capture_payload("")
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            (
                "action",
                parse_action_payload("   ")
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            (
                "wait",
                parse_wait_payload("\n")
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
        ] {
            let err = result.expect_err("blank output must not parse as a payload");
            assert!(
                !err.contains("EOF while parsing"),
                "{label} still leaks the raw serde message: {err}"
            );
            assert!(
                err.contains("produced no output"),
                "{label} should say the script printed nothing: {err}"
            );
            assert!(
                err.starts_with("Windows "),
                "{label} should name the operation: {err}"
            );
        }
    }

    #[test]
    fn unreadable_output_reports_the_operation_and_what_was_emitted() {
        let err = parse_wait_payload(
            "Add-Type : Cannot bind argument to parameter 'Path' because it is null.",
        )
        .map(|_| ())
        .expect_err("PowerShell complaint is not a payload");
        let msg = err.to_string();
        assert!(msg.contains("Windows wait"), "got: {msg}");
        assert!(msg.contains("unreadable JSON"), "got: {msg}");
        assert!(
            msg.contains("Add-Type"),
            "should quote the script output: {msg}"
        );
    }

    #[test]
    fn a_wellformed_payload_still_decodes() {
        let payload = parse_wait_payload(
            r#"{"satisfied":true,"elapsed_ms":12,"detail":"node appeared","window_title":"Settings"}"#,
        )
        .unwrap();
        assert!(payload.satisfied);
        assert_eq!(payload.elapsed_ms, 12);
        assert_eq!(payload.window_title, "Settings");
    }

    // A payload of all-defaults used to satisfy `parse_action_payload` when the
    // script genuinely found nothing, so the missing-node-id error has to carry
    // whatever status the script did report.
    #[test]
    fn an_action_without_a_node_id_surfaces_the_scripts_own_status() {
        let err = parse_action_payload(r#"{"status":"not_found","detail":"no element named X"}"#)
            .map(|_| ())
            .expect_err("no executed node id is a failure");
        let msg = err.to_string();
        assert!(msg.contains("not_found"), "got: {msg}");
        assert!(msg.contains("no element named X"), "got: {msg}");
    }
}
