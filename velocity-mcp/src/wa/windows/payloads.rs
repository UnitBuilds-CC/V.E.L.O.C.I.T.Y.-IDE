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

pub fn parse_capture_payload(json_payload: &str) -> Result<WindowsCapturePayload, Box<dyn Error>> {
    let payload: WindowsCapturePayload = serde_json::from_str(json_payload)?;
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
    let payload: WindowsActionPayload = serde_json::from_str(json_payload)?;
    if payload.executed_node_id.trim().is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "Windows action returned no executed node id",
        )
        .into());
    }
    Ok(payload)
}

pub fn parse_wait_payload(json_payload: &str) -> Result<WindowsWaitPayload, Box<dyn Error>> {
    let payload: WindowsWaitPayload = serde_json::from_str(json_payload)?;
    Ok(payload)
}
