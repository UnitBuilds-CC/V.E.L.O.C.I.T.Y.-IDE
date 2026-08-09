use crate::wa::model::{WaScript, WaScriptRunReport, WaSession, WaSnapshot};
use std::error::Error;
use std::io::{Error as IoError, ErrorKind};

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn encode_optional_nda_text(value: Option<&str>) -> String {
    value
        .map(encode_nda_text)
        .unwrap_or_else(|| "-".to_string())
}

pub fn serialize_session_nda(session: &WaSession) -> String {
    let mut lines = vec![
        "wa-session version 2".to_string(),
        format!("field_count {}", 6),
        format!("id {}", encode_nda_text(&session.id)),
        format!("created_at_ms {}", session.created_at_ms),
        format!("updated_at_ms {}", session.updated_at_ms),
        format!(
            "latest_snapshot_name {}",
            encode_optional_nda_text(session.latest_snapshot_name.as_deref())
        ),
        format!(
            "latest_snapshot_nda_path {}",
            encode_optional_nda_text(session.latest_snapshot_nda_path.as_deref())
        ),
        format!("snapshot_count {}", session.snapshot_count),
    ];
    lines.push(String::new());
    lines.join("\n")
}

pub fn serialize_snapshot_nda(snapshot: &WaSnapshot) -> String {
    let mut lines = vec![
        "wa-snapshot version 2".to_string(),
        format!("field_count {}", 7),
        format!("session_id {}", encode_nda_text(&snapshot.session_id)),
        format!("snapshot_name {}", encode_nda_text(&snapshot.snapshot_name)),
        format!("created_at_ms {}", snapshot.created_at_ms),
        format!("url {}", encode_nda_text(&snapshot.url)),
        format!("title {}", encode_nda_text(&snapshot.title)),
        format!(
            "focus_node_id {}",
            encode_optional_nda_text(snapshot.focus_node_id.as_deref())
        ),
        format!("node_count {}", snapshot.nodes.len()),
    ];
    for (index, node) in snapshot.nodes.iter().enumerate() {
        lines.push(format!(
            "node\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            index,
            encode_nda_text(&node.id),
            encode_nda_text(&node.role),
            encode_nda_text(&node.name),
            encode_nda_text(&node.value),
            encode_nda_text(&node.actions.join(",")),
            node.visible,
            node.enabled,
            encode_nda_text(&node.provenance),
            node.confidence,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn serialize_script_nda(script: &WaScript) -> String {
    let mut lines = vec![
        "wa-script version 2".to_string(),
        format!("field_count {}", 4),
        format!("name {}", encode_nda_text(&script.name)),
        format!("created_at_ms {}", script.created_at_ms),
        format!(
            "start_url {}",
            encode_optional_nda_text(script.start_url.as_deref())
        ),
        format!("step_count {}", script.steps.len()),
    ];
    for (index, step) in script.steps.iter().enumerate() {
        lines.push(format!(
            "step\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            index,
            encode_nda_text(&step.action),
            encode_optional_nda_text(step.node_id.as_deref()),
            encode_optional_nda_text(step.role.as_deref()),
            encode_optional_nda_text(step.name.as_deref()),
            encode_optional_nda_text(step.value.as_deref()),
            step.required,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn serialize_run_nda(report: &WaScriptRunReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string())
}

fn decode_nda_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn decode_optional_nda_text(value: &str) -> Option<String> {
    if value == "-" {
        None
    } else {
        Some(decode_nda_text(value))
    }
}

fn invalid_nda(message: &str) -> Box<dyn Error> {
    IoError::new(ErrorKind::InvalidData, message).into()
}

fn parse_key_value_line<'a>(line: &'a str, expected_key: &str) -> Result<&'a str, Box<dyn Error>> {
    let prefix = format!("{expected_key} ");
    line.strip_prefix(&prefix)
        .ok_or_else(|| invalid_nda(&format!("invalid WA NDA line for key '{expected_key}'")))
}

pub fn deserialize_session_nda(content: &str) -> Result<WaSession, Box<dyn Error>> {
    let mut lines = content.lines();
    if lines.next() != Some("wa-session version 2") {
        return Err(invalid_nda("invalid WA session NDA header"));
    }
    let _ = lines.next();
    let id = decode_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing session id"))?,
        "id",
    )?);
    let created_at_ms = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing created_at_ms"))?,
        "created_at_ms",
    )?
    .parse()?;
    let updated_at_ms = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing updated_at_ms"))?,
        "updated_at_ms",
    )?
    .parse()?;
    let latest_snapshot_name = decode_optional_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing latest_snapshot_name"))?,
        "latest_snapshot_name",
    )?);
    let latest_snapshot_nda_path = decode_optional_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing latest_snapshot_nda_path"))?,
        "latest_snapshot_nda_path",
    )?);
    let snapshot_count = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing snapshot_count"))?,
        "snapshot_count",
    )?
    .parse()?;
    Ok(WaSession {
        id,
        created_at_ms,
        updated_at_ms,
        latest_snapshot_name,
        latest_snapshot_nda_path,
        snapshot_count,
    })
}

pub fn deserialize_snapshot_nda(content: &str) -> Result<WaSnapshot, Box<dyn Error>> {
    let mut lines = content.lines();
    if lines.next() != Some("wa-snapshot version 2") {
        return Err(invalid_nda("invalid WA snapshot NDA header"));
    }
    let _ = lines.next();
    let session_id = decode_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing session_id"))?,
        "session_id",
    )?);
    let snapshot_name = decode_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing snapshot_name"))?,
        "snapshot_name",
    )?);
    let created_at_ms = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing created_at_ms"))?,
        "created_at_ms",
    )?
    .parse()?;
    let url = decode_nda_text(parse_key_value_line(
        lines.next().ok_or_else(|| invalid_nda("missing url"))?,
        "url",
    )?);
    let title = decode_nda_text(parse_key_value_line(
        lines.next().ok_or_else(|| invalid_nda("missing title"))?,
        "title",
    )?);
    let focus_node_id = decode_optional_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing focus_node_id"))?,
        "focus_node_id",
    )?);
    let node_count: usize = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing node_count"))?,
        "node_count",
    )?
    .parse()?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let line = lines
            .next()
            .ok_or_else(|| invalid_nda("missing snapshot node row"))?;
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() != 11 || parts[0] != "node" {
            return Err(invalid_nda("invalid snapshot node row"));
        }
        let actions = if parts[6].is_empty() {
            Vec::new()
        } else {
            decode_nda_text(parts[6])
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .collect()
        };
        nodes.push(crate::wa::model::WaNode {
            id: decode_nda_text(parts[2]),
            role: decode_nda_text(parts[3]),
            name: decode_nda_text(parts[4]),
            value: decode_nda_text(parts[5]),
            actions,
            visible: parts[7].parse()?,
            enabled: parts[8].parse()?,
            provenance: decode_nda_text(parts[9]),
            confidence: parts[10].parse()?,
        });
    }
    Ok(WaSnapshot {
        session_id,
        snapshot_name,
        created_at_ms,
        url,
        title,
        focus_node_id,
        nodes,
    })
}

pub fn deserialize_script_nda(content: &str) -> Result<WaScript, Box<dyn Error>> {
    let mut lines = content.lines();
    if lines.next() != Some("wa-script version 2") {
        return Err(invalid_nda("invalid WA script NDA header"));
    }
    let _ = lines.next();
    let name = decode_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing script name"))?,
        "name",
    )?);
    let created_at_ms = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing created_at_ms"))?,
        "created_at_ms",
    )?
    .parse()?;
    let start_url = decode_optional_nda_text(parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing start_url"))?,
        "start_url",
    )?);
    let step_count: usize = parse_key_value_line(
        lines
            .next()
            .ok_or_else(|| invalid_nda("missing step_count"))?,
        "step_count",
    )?
    .parse()?;
    let mut steps = Vec::with_capacity(step_count);
    for _ in 0..step_count {
        let line = lines
            .next()
            .ok_or_else(|| invalid_nda("missing script step row"))?;
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() != 8 || parts[0] != "step" {
            return Err(invalid_nda("invalid script step row"));
        }
        steps.push(crate::wa::model::WaScriptStep {
            action: decode_nda_text(parts[2]),
            node_id: decode_optional_nda_text(parts[3]),
            role: decode_optional_nda_text(parts[4]),
            name: decode_optional_nda_text(parts[5]),
            value: decode_optional_nda_text(parts[6]),
            required: parts[7].parse()?,
        });
    }
    Ok(WaScript {
        name,
        created_at_ms,
        start_url,
        steps,
    })
}

pub fn deserialize_run_nda(content: &str) -> Result<WaScriptRunReport, Box<dyn Error>> {
    Ok(serde_json::from_str(content)?)
}
