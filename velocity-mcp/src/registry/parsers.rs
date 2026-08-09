use crate::wa::{WaNode, WaScriptStep};
use serde_json::Value;
use std::error::Error;

pub fn parse_wa_nodes(nodes: &[Value]) -> Result<Vec<WaNode>, Box<dyn Error>> {
    let mut parsed_nodes = Vec::with_capacity(nodes.len());
    for node in nodes {
        let actions = node["actions"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|text| text.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        parsed_nodes.push(WaNode {
            id: node["id"]
                .as_str()
                .ok_or("WA node id is required")?
                .to_string(),
            role: node["role"]
                .as_str()
                .ok_or("WA node role is required")?
                .to_string(),
            name: node["name"]
                .as_str()
                .ok_or("WA node name is required")?
                .to_string(),
            value: node["value"].as_str().unwrap_or("").to_string(),
            actions,
            visible: node["visible"].as_bool().unwrap_or(true),
            enabled: node["enabled"].as_bool().unwrap_or(true),
            provenance: node["provenance"].as_str().unwrap_or("").to_string(),
            confidence: node["confidence"].as_f64().unwrap_or(1.0) as f32,
        });
    }
    Ok(parsed_nodes)
}

pub fn parse_wa_steps(steps: &[Value]) -> Result<Vec<WaScriptStep>, Box<dyn Error>> {
    let mut parsed_steps = Vec::with_capacity(steps.len());
    for step in steps {
        parsed_steps.push(WaScriptStep {
            action: step["action"]
                .as_str()
                .ok_or("WA script step action is required")?
                .to_string(),
            node_id: step["nodeId"].as_str().map(|value| value.to_string()),
            role: step["role"].as_str().map(|value| value.to_string()),
            name: step["name"].as_str().map(|value| value.to_string()),
            value: step["value"].as_str().map(|value| value.to_string()),
            required: step["required"].as_bool().unwrap_or(true),
        });
    }
    Ok(parsed_steps)
}

pub fn parse_browser_steps(
    steps: &[Value],
) -> Result<Vec<crate::editor::browser::BrowserWorkflowStep>, Box<dyn Error>> {
    let mut parsed_steps = Vec::with_capacity(steps.len());
    for step in steps {
        let kind = step["kind"]
            .as_str()
            .ok_or("workflow step kind is required")?;
        let parsed = match kind {
            "navigate" => crate::editor::browser::BrowserWorkflowStep::Navigate {
                url: step["url"]
                    .as_str()
                    .ok_or("navigate step url is required")?
                    .to_string(),
            },
            "click" => crate::editor::browser::BrowserWorkflowStep::Click {
                role: step["role"]
                    .as_str()
                    .ok_or("click step role is required")?
                    .to_string(),
                name: step["name"]
                    .as_str()
                    .ok_or("click step name is required")?
                    .to_string(),
            },
            "fill_field" => crate::editor::browser::BrowserWorkflowStep::FillField {
                field: step["field"]
                    .as_str()
                    .ok_or("fill_field step field is required")?
                    .to_string(),
                value: step["value"]
                    .as_str()
                    .ok_or("fill_field step value is required")?
                    .to_string(),
            },
            "submit_form" => crate::editor::browser::BrowserWorkflowStep::SubmitForm {
                form: step["form"].as_str().map(|value| value.to_string()),
            },
            "wait_for_text" => crate::editor::browser::BrowserWorkflowStep::WaitForText {
                text: step["text"]
                    .as_str()
                    .ok_or("wait_for_text step text is required")?
                    .to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_element" => crate::editor::browser::BrowserWorkflowStep::WaitForElement {
                role: step["role"]
                    .as_str()
                    .ok_or("wait_for_element step role is required")?
                    .to_string(),
                name: step["name"]
                    .as_str()
                    .ok_or("wait_for_element step name is required")?
                    .to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_title" => crate::editor::browser::BrowserWorkflowStep::WaitForTitle {
                title: step["title"]
                    .as_str()
                    .ok_or("wait_for_title step title is required")?
                    .to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_url_contains" => {
                crate::editor::browser::BrowserWorkflowStep::WaitForUrlContains {
                    fragment: step["fragment"]
                        .as_str()
                        .ok_or("wait_for_url_contains step fragment is required")?
                        .to_string(),
                    timeout_ms: step["timeoutMs"].as_u64(),
                    interval_ms: step["intervalMs"].as_u64(),
                }
            }
            "wait_for_mutation" => crate::editor::browser::BrowserWorkflowStep::WaitForMutation {
                label: step["label"]
                    .as_str()
                    .ok_or("wait_for_mutation step label is required")?
                    .to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_request" => crate::editor::browser::BrowserWorkflowStep::WaitForRequest {
                method: step["method"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                url_contains: step["urlContains"]
                    .as_str()
                    .or_else(|| step["url_contains"].as_str())
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                status: step["status"].as_u64().map(|value| value as u16),
                resource: step["resource"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_storage" => crate::editor::browser::BrowserWorkflowStep::WaitForStorage {
                scope: step["scope"]
                    .as_str()
                    .ok_or("wait_for_storage step scope is required")?
                    .to_string(),
                key: step["key"]
                    .as_str()
                    .ok_or("wait_for_storage step key is required")?
                    .to_string(),
                value: step["value"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_settle" => crate::editor::browser::BrowserWorkflowStep::WaitForSettle {
                label: step["label"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                scope: step["scope"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                state: step["state"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_runtime_state" => {
                crate::editor::browser::BrowserWorkflowStep::WaitForRuntimeState {
                    scope: step["scope"]
                        .as_str()
                        .ok_or("wait_for_runtime_state step scope is required")?
                        .to_string(),
                    key: step["key"]
                        .as_str()
                        .ok_or("wait_for_runtime_state step key is required")?
                        .to_string(),
                    value: step["value"]
                        .as_str()
                        .map(|value| value.to_string())
                        .filter(|value| !value.is_empty()),
                    timeout_ms: step["timeoutMs"].as_u64(),
                    interval_ms: step["intervalMs"].as_u64(),
                }
            }
            "wait_for_protocol_event" => {
                crate::editor::browser::BrowserWorkflowStep::WaitForProtocolEvent {
                    event_kind: step["eventKind"]
                        .as_str()
                        .or_else(|| step["event_kind"].as_str())
                        .or_else(|| step["kind"].as_str())
                        .map(|value| value.to_string())
                        .filter(|value| !value.is_empty()),
                    phase: step["phase"]
                        .as_str()
                        .map(|value| value.to_string())
                        .filter(|value| !value.is_empty()),
                    target: step["target"]
                        .as_str()
                        .or_else(|| step["targetContains"].as_str())
                        .or_else(|| step["protocolTarget"].as_str())
                        .map(|value| value.to_string())
                        .filter(|value| !value.is_empty()),
                    detail: step["detail"]
                        .as_str()
                        .or_else(|| step["detailContains"].as_str())
                        .or_else(|| step["protocolDetail"].as_str())
                        .map(|value| value.to_string())
                        .filter(|value| !value.is_empty()),
                    timeout_ms: step["timeoutMs"].as_u64(),
                    interval_ms: step["intervalMs"].as_u64(),
                }
            }
            "wait_for_stable" => crate::editor::browser::BrowserWorkflowStep::WaitForStable {
                stable_polls: step["stablePolls"].as_u64().map(|value| value as u32),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "extract_text" => crate::editor::browser::BrowserWorkflowStep::ExtractText {
                output: step["output"]
                    .as_str()
                    .ok_or("extract_text step output is required")?
                    .to_string(),
                source: step["source"]
                    .as_str()
                    .ok_or("extract_text step source is required")?
                    .to_string(),
                role: step["role"].as_str().map(|value| value.to_string()),
                name: step["name"].as_str().map(|value| value.to_string()),
                field: step["field"].as_str().map(|value| value.to_string()),
            },
            "save_checkpoint" => crate::editor::browser::BrowserWorkflowStep::SaveCheckpoint {
                name: step["name"]
                    .as_str()
                    .ok_or("save_checkpoint step name is required")?
                    .to_string(),
            },
            "restore_checkpoint" => {
                crate::editor::browser::BrowserWorkflowStep::RestoreCheckpoint {
                    name: step["name"]
                        .as_str()
                        .ok_or("restore_checkpoint step name is required")?
                        .to_string(),
                }
            }
            "if_text_contains" => crate::editor::browser::BrowserWorkflowStep::IfTextContains {
                text: step["text"]
                    .as_str()
                    .ok_or("if_text_contains step text is required")?
                    .to_string(),
                then_steps: parse_browser_steps(
                    step["thenSteps"]
                        .as_array()
                        .ok_or("if_text_contains thenSteps must be an array")?,
                )?,
                else_steps: parse_browser_steps(
                    step["elseSteps"]
                        .as_array()
                        .map(|steps| steps.as_slice())
                        .unwrap_or(&[]),
                )?,
            },
            "if_output_equals" => crate::editor::browser::BrowserWorkflowStep::IfOutputEquals {
                output: step["output"]
                    .as_str()
                    .ok_or("if_output_equals step output is required")?
                    .to_string(),
                equals: step["equals"]
                    .as_str()
                    .ok_or("if_output_equals step equals is required")?
                    .to_string(),
                then_steps: parse_browser_steps(
                    step["thenSteps"]
                        .as_array()
                        .ok_or("if_output_equals thenSteps must be an array")?,
                )?,
                else_steps: parse_browser_steps(
                    step["elseSteps"]
                        .as_array()
                        .map(|steps| steps.as_slice())
                        .unwrap_or(&[]),
                )?,
            },
            "assert_element" => crate::editor::browser::BrowserWorkflowStep::AssertElement {
                role: step["role"]
                    .as_str()
                    .ok_or("assert_element step role is required")?
                    .to_string(),
                name: step["name"]
                    .as_str()
                    .ok_or("assert_element step name is required")?
                    .to_string(),
            },
            "assert_text_contains" => {
                crate::editor::browser::BrowserWorkflowStep::AssertTextContains {
                    text: step["text"]
                        .as_str()
                        .ok_or("assert_text_contains step text is required")?
                        .to_string(),
                }
            }
            "assert_output" => crate::editor::browser::BrowserWorkflowStep::AssertOutput {
                output: step["output"]
                    .as_str()
                    .ok_or("assert_output step output is required")?
                    .to_string(),
                equals: step["equals"].as_str().map(|value| value.to_string()),
                contains: step["contains"].as_str().map(|value| value.to_string()),
            },
            other => {
                return Err(format!("unsupported browser workflow step kind: {}", other).into())
            }
        };
        parsed_steps.push(parsed);
    }
    Ok(parsed_steps)
}
