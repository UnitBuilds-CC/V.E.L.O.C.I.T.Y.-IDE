use super::types::*;

pub fn escape_value(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn unescape_value(value: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }

        let escaped = chars
            .next()
            .ok_or_else(|| "Dangling escape in NDA registry value".to_string())?;
        match escaped {
            '\\' => result.push('\\'),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            other => {
                result.push('\\');
                result.push(other);
            }
        }
    }
    Ok(result)
}

pub fn ensure_template<'a>(
    templates: &'a mut Vec<InstructionTemplate>,
    id: &str,
) -> &'a mut InstructionTemplate {
    if let Some(index) = templates.iter().position(|template| template.id == id) {
        return &mut templates[index];
    }
    templates.push(InstructionTemplate {
        id: id.to_string(),
        label: String::new(),
        task_kind: AgentTaskKind::Planning,
        system_prompt: String::new(),
        checklist: Vec::new(),
    });
    templates.last_mut().expect("template inserted")
}

pub fn ensure_policy<'a>(
    policies: &'a mut Vec<DecompositionPolicy>,
    id: &str,
) -> &'a mut DecompositionPolicy {
    if let Some(index) = policies.iter().position(|policy| policy.id == id) {
        return &mut policies[index];
    }
    policies.push(DecompositionPolicy {
        id: id.to_string(),
        label: String::new(),
        task_kind: AgentTaskKind::Planning,
        instruction_template_id: String::new(),
        decomposition_style: DecompositionStyle::SequentialPipeline,
        shared_expectations: Vec::new(),
    });
    policies.last_mut().expect("policy inserted")
}

pub fn to_nda_string(
    templates_input: &[InstructionTemplate],
    policies_input: &[DecompositionPolicy],
    preferred_policies_input: &[PreferredPolicy],
) -> String {
    let mut templates = templates_input.to_vec();
    templates.sort_by(|a, b| {
        a.task_kind
            .as_str()
            .cmp(b.task_kind.as_str())
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut policies = policies_input.to_vec();
    policies.sort_by(|a, b| {
        a.task_kind
            .as_str()
            .cmp(b.task_kind.as_str())
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut preferred_policies = preferred_policies_input.to_vec();
    preferred_policies.sort_by(|a, b| a.task_kind.as_str().cmp(b.task_kind.as_str()));

    let mut lines = vec![
        "registry version 2".to_string(),
        format!("template_count {}", templates.len()),
        format!("policy_count {}", policies.len()),
        format!("preferred_policy_count {}", preferred_policies.len()),
    ];
    for template in templates {
        lines.push(format!("template\t{}", escape_value(&template.id)));
        lines.push(format!(
            "template_field\t{}\tlabel\t{}",
            escape_value(&template.id),
            escape_value(&template.label)
        ));
        lines.push(format!(
            "template_field\t{}\ttask_kind\t{}",
            escape_value(&template.id),
            template.task_kind.as_str()
        ));
        lines.push(format!(
            "template_field\t{}\tsystem_prompt\t{}",
            escape_value(&template.id),
            escape_value(&template.system_prompt)
        ));
        lines.push(format!(
            "template_checklist_count\t{}\t{}",
            escape_value(&template.id),
            template.checklist.len()
        ));
        for (index, checklist_item) in template.checklist.iter().enumerate() {
            lines.push(format!(
                "template_checklist\t{}\t{}\t{}",
                escape_value(&template.id),
                index,
                escape_value(checklist_item)
            ));
        }
    }

    for policy in policies {
        lines.push(format!("policy\t{}", escape_value(&policy.id)));
        lines.push(format!(
            "policy_field\t{}\tlabel\t{}",
            escape_value(&policy.id),
            escape_value(&policy.label)
        ));
        lines.push(format!(
            "policy_field\t{}\ttask_kind\t{}",
            escape_value(&policy.id),
            policy.task_kind.as_str()
        ));
        lines.push(format!(
            "policy_field\t{}\ttemplate\t{}",
            escape_value(&policy.id),
            escape_value(&policy.instruction_template_id)
        ));
        lines.push(format!(
            "policy_field\t{}\tdecomposition_style\t{}",
            escape_value(&policy.id),
            policy.decomposition_style.as_str()
        ));
        lines.push(format!(
            "policy_expectation_count\t{}\t{}",
            escape_value(&policy.id),
            policy.shared_expectations.len()
        ));
        for (index, expectation) in policy.shared_expectations.iter().enumerate() {
            lines.push(format!(
                "policy_expectation\t{}\t{}\t{}",
                escape_value(&policy.id),
                index,
                escape_value(expectation)
            ));
        }
    }

    for preferred in preferred_policies {
        lines.push(format!(
            "preferred_policy\t{}\t{}",
            preferred.task_kind.as_str(),
            escape_value(&preferred.policy_id)
        ));
    }

    lines.join("\n") + "\n"
}

pub fn parse_nda_registry(
    raw: &str,
) -> Result<
    (
        Vec<InstructionTemplate>,
        Vec<DecompositionPolicy>,
        Vec<PreferredPolicy>,
    ),
    String,
> {
    let mut templates = Vec::<InstructionTemplate>::new();
    let mut policies = Vec::<DecompositionPolicy>::new();
    let mut preferred_policies = Vec::<PreferredPolicy>::new();

    let mut lines = raw.lines();
    let header = lines
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Empty NDA instruction registry".to_string())?
        .trim()
        .to_string();

    if header == "registry version 2" {
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with("template_count ")
                || line.starts_with("policy_count ")
                || line.starts_with("preferred_policy_count ")
                || line.starts_with("template_checklist_count\t")
                || line.starts_with("policy_expectation_count\t")
            {
                continue;
            }

            let parts = line.split('\t').collect::<Vec<_>>();
            match parts.first().copied().unwrap_or_default() {
                "template" => {
                    let id = parts
                        .get(1)
                        .ok_or_else(|| format!("Missing template id on line: {line}"))?;
                    ensure_template(&mut templates, &unescape_value(id)?);
                }
                "template_field" => {
                    let id = unescape_value(
                        parts
                            .get(1)
                            .ok_or_else(|| format!("Missing template id on line: {line}"))?,
                    )?;
                    let field = *parts
                        .get(2)
                        .ok_or_else(|| format!("Missing template field on line: {line}"))?;
                    let value = *parts
                        .get(3)
                        .ok_or_else(|| format!("Missing template value on line: {line}"))?;
                    let template = ensure_template(&mut templates, &id);
                    match field {
                        "label" => template.label = unescape_value(value)?,
                        "task_kind" => {
                            template.task_kind = AgentTaskKind::parse(value)
                                .ok_or_else(|| format!("Unknown template task kind '{value}'"))?;
                        }
                        "system_prompt" => template.system_prompt = unescape_value(value)?,
                        _ => {
                            return Err(format!("Unknown template field '{field}' on line: {line}"))
                        }
                    }
                }
                "template_checklist" => {
                    let id = unescape_value(
                        parts
                            .get(1)
                            .ok_or_else(|| format!("Missing template id on line: {line}"))?,
                    )?;
                    let value = *parts
                        .get(3)
                        .ok_or_else(|| format!("Missing checklist value on line: {line}"))?;
                    let template = ensure_template(&mut templates, &id);
                    template.checklist.push(unescape_value(value)?);
                }
                "policy" => {
                    let id = parts
                        .get(1)
                        .ok_or_else(|| format!("Missing policy id on line: {line}"))?;
                    ensure_policy(&mut policies, &unescape_value(id)?);
                }
                "policy_field" => {
                    let id = unescape_value(
                        parts
                            .get(1)
                            .ok_or_else(|| format!("Missing policy id on line: {line}"))?,
                    )?;
                    let field = *parts
                        .get(2)
                        .ok_or_else(|| format!("Missing policy field on line: {line}"))?;
                    let value = *parts
                        .get(3)
                        .ok_or_else(|| format!("Missing policy value on line: {line}"))?;
                    let policy = ensure_policy(&mut policies, &id);
                    match field {
                        "label" => policy.label = unescape_value(value)?,
                        "task_kind" => {
                            policy.task_kind = AgentTaskKind::parse(value)
                                .ok_or_else(|| format!("Unknown policy task kind '{value}'"))?;
                        }
                        "template" => policy.instruction_template_id = unescape_value(value)?,
                        "decomposition_style" => {
                            policy.decomposition_style = DecompositionStyle::parse(value)
                                .ok_or_else(|| format!("Unknown decomposition style '{value}'"))?;
                        }
                        _ => return Err(format!("Unknown policy field '{field}' on line: {line}")),
                    }
                }
                "policy_expectation" => {
                    let id = unescape_value(
                        parts
                            .get(1)
                            .ok_or_else(|| format!("Missing policy id on line: {line}"))?,
                    )?;
                    let value = *parts
                        .get(3)
                        .ok_or_else(|| format!("Missing expectation value on line: {line}"))?;
                    let policy = ensure_policy(&mut policies, &id);
                    policy.shared_expectations.push(unescape_value(value)?);
                }
                "preferred_policy" => {
                    let task_kind = *parts.get(1).ok_or_else(|| {
                        format!("Missing preferred policy task kind on line: {line}")
                    })?;
                    let policy_id = *parts
                        .get(2)
                        .ok_or_else(|| format!("Missing preferred policy id on line: {line}"))?;
                    preferred_policies.push(PreferredPolicy {
                        task_kind: AgentTaskKind::parse(task_kind).ok_or_else(|| {
                            format!("Unknown preferred policy task kind '{task_kind}'")
                        })?,
                        policy_id: unescape_value(policy_id)?,
                    });
                }
                _ => return Err(format!("Unknown NDA instruction registry line: {line}")),
            }
        }

        return Ok((templates, policies, preferred_policies));
    }

    if header != "registry version 1" {
        return Err(format!(
            "Unsupported NDA instruction registry header: {header}"
        ));
    }

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("template ") {
            let mut parts = rest.splitn(3, ' ');
            let id = parts
                .next()
                .ok_or_else(|| format!("Missing template id on line: {line}"))?;
            let field = parts
                .next()
                .ok_or_else(|| format!("Missing template field on line: {line}"))?;
            let value = parts
                .next()
                .ok_or_else(|| format!("Missing template value on line: {line}"))?;
            let template = ensure_template(&mut templates, id);
            match field {
                "label" => template.label = unescape_value(value)?,
                "task_kind" => {
                    template.task_kind = AgentTaskKind::parse(value)
                        .ok_or_else(|| format!("Unknown template task kind '{value}'"))?;
                }
                "system_prompt" => template.system_prompt = unescape_value(value)?,
                "checklist" => template.checklist.push(unescape_value(value)?),
                _ => return Err(format!("Unknown template field '{field}' on line: {line}")),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("policy ") {
            let mut parts = rest.splitn(3, ' ');
            let id = parts
                .next()
                .ok_or_else(|| format!("Missing policy id on line: {line}"))?;
            let field = parts
                .next()
                .ok_or_else(|| format!("Missing policy field on line: {line}"))?;
            let value = parts
                .next()
                .ok_or_else(|| format!("Missing policy value on line: {line}"))?;
            let policy = ensure_policy(&mut policies, id);
            match field {
                "label" => policy.label = unescape_value(value)?,
                "task_kind" => {
                    policy.task_kind = AgentTaskKind::parse(value)
                        .ok_or_else(|| format!("Unknown policy task kind '{value}'"))?;
                }
                "template" => policy.instruction_template_id = unescape_value(value)?,
                "decomposition_style" => {
                    policy.decomposition_style = DecompositionStyle::parse(value)
                        .ok_or_else(|| format!("Unknown decomposition style '{value}'"))?;
                }
                "expectation" => policy.shared_expectations.push(unescape_value(value)?),
                _ => return Err(format!("Unknown policy field '{field}' on line: {line}")),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("preferred_policy ") {
            let mut parts = rest.splitn(2, ' ');
            let task_kind = parts
                .next()
                .ok_or_else(|| format!("Missing preferred policy task kind on line: {line}"))?;
            let policy_id = parts
                .next()
                .ok_or_else(|| format!("Missing preferred policy id on line: {line}"))?;
            preferred_policies.push(PreferredPolicy {
                task_kind: AgentTaskKind::parse(task_kind)
                    .ok_or_else(|| format!("Unknown preferred policy task kind '{task_kind}'"))?,
                policy_id: unescape_value(policy_id)?,
            });
            continue;
        }

        return Err(format!("Unknown NDA instruction registry line: {line}"));
    }

    Ok((templates, policies, preferred_policies))
}
