//! MCP tools for the AI-staffed company org: authoring departments/roles,
//! scoping capability grants over external resources, editing approval
//! policies, deterministically checking permission, and running the
//! request/approve/reject pipeline.
//!
//! These tools are thin adapters over [`crate::editor::company_org`]; all the
//! permission logic lives there so it stays inference-free and unit-tested.

use crate::editor::company_org::{
    self, ApprovalPolicy, AutonomyTier, Capability, CredentialGrant, Playbook, Resource,
    ResourceKind, Role,
};
use serde_json::Value;
use std::error::Error;
use std::path::Path;

/// Dispatch company-org tools. Returns `Ok(None)` when `name` is not one of
/// these tools so the caller can try other handlers.
pub fn handle_company_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "get_company_org" => get_org(root, arguments)?,
        "init_company_org" => init_org(root)?,
        "add_org_role" => add_role(root, arguments)?,
        "add_org_resource" => add_resource(root, arguments)?,
        "grant_capability" => grant_capability(root, arguments)?,
        "revoke_capability" => revoke_capability(root, arguments)?,
        "set_approval_policy" => set_policy(root, arguments)?,
        "remove_approval_policy" => remove_policy(root, arguments)?,
        "check_org_permission" => check_permission(root, arguments)?,
        "verify_org_credential" => verify_credential(root, arguments)?,
        "rotate_org_resource_credential" => rotate_credential(root, arguments)?,
        "execute_org_credential_rotation_after_approval" => {
            execute_rotation_after_approval(root, arguments)?
        }
        "plan_org_browser_login" => plan_browser_login(root, arguments)?,
        "plan_org_browser_broadcast" => plan_browser_broadcast(root, arguments)?,
        "execute_org_browser_broadcast" => execute_browser_broadcast(root, arguments)?,
        "execute_org_browser_task" => execute_browser_task(root, arguments)?,
        "execute_org_browser_task_for_request" => {
            execute_browser_task_for_request(root, arguments)?
        }
        "request_org_action" => request_action(root, arguments)?,
        "approve_org_action" => approve_action(root, arguments)?,
        "approve_and_resume_org_action" => approve_and_resume(root, arguments)?,
        "reject_org_action" => reject_action(root, arguments)?,
        "list_org_actions" => list_actions(root, arguments)?,
        "list_org_escalations" => list_escalations(root, arguments)?,
        "add_org_playbook" => add_playbook(root, arguments)?,
        "remove_org_playbook" => remove_playbook(root, arguments)?,
        "list_org_playbooks" => list_playbooks(root)?,
        "validate_org_playbooks" => validate_playbooks(root)?,
        "run_org_playbook" => run_playbook(root, arguments)?,
        "resume_org_playbook" => resume_playbook(root, arguments)?,
        "sync_org_team" => sync_team(root)?,
        "dispatch_org_role" => dispatch_role(root, arguments)?,
        "grant_org_credential_elevation" => grant_elevation(root, arguments)?,
        "check_org_credential_elevation" => check_elevation(root, arguments)?,
        "consume_org_credential_elevation" => consume_elevation(root, arguments)?,
        "elevate_and_resume_org_request" => elevate_and_resume_request(root, arguments)?,
        "list_org_credential_audit" => list_credential_audit(root, arguments)?,
        "list_org_playbook_verdicts" => list_playbook_verdicts(root, arguments)?,
        "chain_org_playbooks" => chain_playbooks(root, arguments)?,
        "list_org_sop_handoffs" => list_sop_handoffs(root, arguments)?,
        "schedule_org_playbook" => schedule_playbook(root, arguments)?,
        "unschedule_org_playbook" => unschedule_playbook(root, arguments)?,
        "pause_org_playbook_schedule" => pause_schedule(root, arguments)?,
        "list_org_schedules" => list_schedules(root)?,
        "run_due_org_playbooks" => run_due_playbooks(root, arguments)?,
        _ => return Ok(None),
    };
    Ok(Some(result))
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Load the stored org, or seed the UnitBuilds CC preset when none exists yet.
fn load_or_seed(root: &Path) -> company_org::CompanyOrg {
    company_org::load_company_org(root).unwrap_or_else(company_org::default_company_org)
}

fn parse_caps(arr: Option<&Vec<Value>>) -> Result<Vec<Capability>, Box<dyn Error>> {
    let arr = arr.ok_or("'capabilities' must be an array of strings")?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let s = v.as_str().ok_or("capability entries must be strings")?;
        out.push(Capability::parse(s).ok_or_else(|| -> Box<dyn Error> {
            format!(
                "unknown capability '{s}' (use read|write|communicate|spend|deploy|approve|manage)"
            )
            .into()
        })?);
    }
    Ok(out)
}

fn require_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    arguments[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| -> Box<dyn Error> { format!("'{key}' is required").into() })
}

fn action_line(a: &company_org::ActionRequest) -> String {
    format!(
        "{} | {} -[{}]-> {}{} | {:?}{} | {}{}",
        a.id,
        a.role_id,
        a.capability.as_str(),
        a.resource_id,
        if a.amount > 0.0 {
            format!(" (${:.2})", a.amount)
        } else {
            String::new()
        },
        a.status,
        if matches!(a.status, company_org::ActionStatus::Pending) {
            format!(
                " need {}/{} from [{}]",
                a.approvals.len(),
                a.quorum.max(1),
                a.required_approvers.join(", ")
            )
        } else {
            String::new()
        },
        a.description,
        // Which SOP this intent came from — several can share one gate, so an
        // approver needs to see the duty a signature actually unblocks.
        match &a.playbook_id {
            Some(p) => format!(" | SOP '{p}'"),
            None => String::new(),
        }
    )
}

// ── read / seed ────────────────────────────────────────────────────────────

fn get_org(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let org = load_or_seed(root);
    if arguments["full"].as_bool().unwrap_or(false) {
        return Ok(serde_json::to_string_pretty(&org)?);
    }
    let mut s = String::new();
    s.push_str(&format!("Company org: {} ({})\n", org.name, org.id));
    s.push_str(&format!("Departments ({}):\n", org.departments.len()));
    for d in &org.departments {
        s.push_str(&format!("  - {}: {}\n", d.id, d.charter));
    }
    s.push_str(&format!("Roles ({}):\n", org.roles.len()));
    for r in &org.roles {
        let mgr = r.reports_to.as_deref().unwrap_or("-");
        s.push_str(&format!(
            "  - {} ({}, in {}, reports to {}, {:?})\n",
            r.id, r.title, r.department_id, mgr, r.tier
        ));
    }
    s.push_str(&format!("Resources ({}):\n", org.resources.len()));
    for res in &org.resources {
        s.push_str(&format!(
            "  - {} [{}] sensitive={} secret_ref={}\n",
            res.id,
            res.name,
            res.sensitive,
            if res.secret_ref.is_empty() {
                "(none)"
            } else {
                &res.secret_ref
            }
        ));
    }
    s.push_str(&format!(
        "Grants ({}), Policies ({})\n",
        org.grants.len(),
        org.policies.len()
    ));
    Ok(s)
}

fn init_org(root: &Path) -> Result<String, Box<dyn Error>> {
    let existed = company_org::load_company_org(root).is_some();
    let org = company_org::ensure_default_org(root);
    Ok(format!(
        "{} org '{}' ({} departments, {} roles, {} resources, {} grants, {} policies).",
        if existed {
            "Org already present:"
        } else {
            "Seeded"
        },
        org.name,
        org.departments.len(),
        org.roles.len(),
        org.resources.len(),
        org.grants.len(),
        org.policies.len()
    ))
}

// ── authoring ──────────────────────────────────────────────────────────────

fn add_role(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "role_id")?;
    let title = require_str(arguments, "title")?;
    let dept = require_str(arguments, "department_id")?;
    let mut org = load_or_seed(root);
    if org.find_role(id).is_some() {
        return Err(format!("role '{id}' already exists").into());
    }
    let tier = match arguments["tier"].as_str() {
        Some("assisted") => AutonomyTier::Assisted,
        Some("trusted") => AutonomyTier::Trusted,
        _ => AutonomyTier::Standard,
    };
    org.roles.push(Role {
        id: id.to_string(),
        title: title.to_string(),
        department_id: dept.to_string(),
        reports_to: arguments["reports_to"].as_str().map(|s| s.to_string()),
        tier,
        agent_team: arguments["agent_team"].as_str().unwrap_or("").to_string(),
        humans: arguments["humans"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
    });
    company_org::save_company_org(root, &org);
    Ok(format!(
        "Added role '{id}' ({title}) to department '{dept}'."
    ))
}

fn add_resource(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "resource_id")?;
    let name = require_str(arguments, "name")?;
    let kind = ResourceKind::parse(arguments["kind"].as_str().unwrap_or("other"));
    let mut org = load_or_seed(root);
    if org.find_resource(id).is_some() {
        return Err(format!("resource '{id}' already exists").into());
    }
    // Store only a pointer to the secret, never the secret value.
    let sensitive = match arguments["sensitive"].as_bool() {
        Some(b) => b,
        None => matches!(
            kind,
            ResourceKind::Banking
                | ResourceKind::SourceControl
                | ResourceKind::Cloud
                | ResourceKind::Email
        ),
    };
    org.resources.push(Resource {
        id: id.to_string(),
        name: name.to_string(),
        kind: Some(kind),
        secret_ref: arguments["secret_ref"].as_str().unwrap_or("").to_string(),
        sensitive,
    });
    company_org::save_company_org(root, &org);
    Ok(format!(
        "Added resource '{id}' ({name}, {}). sensitive={}.",
        kind.as_str(),
        sensitive
    ))
}

fn grant_capability(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let caps = parse_caps(arguments["capabilities"].as_array())?;
    let spend_limit = arguments["spend_limit_per_action"].as_f64().unwrap_or(0.0);

    let mut org = load_or_seed(root);
    if org.find_role(role_id).is_none() {
        return Err(format!("unknown role '{role_id}'").into());
    }
    if org.find_resource(resource_id).is_none() {
        return Err(format!("unknown resource '{resource_id}'").into());
    }
    let mut added = Vec::new();
    if let Some(g) = org
        .grants
        .iter_mut()
        .find(|g| g.role_id == role_id && g.resource_id == resource_id)
    {
        for c in &caps {
            if !g.capabilities.contains(c) {
                g.capabilities.push(*c);
                added.push(c.as_str().to_string());
            }
        }
        if spend_limit > 0.0 {
            g.spend_limit_per_action = spend_limit;
        }
    } else {
        org.grants.push(CredentialGrant {
            role_id: role_id.to_string(),
            resource_id: resource_id.to_string(),
            capabilities: caps.clone(),
            spend_limit_per_action: spend_limit,
        });
        added = caps.iter().map(|c| c.as_str().to_string()).collect();
    }
    company_org::save_company_org(root, &org);
    Ok(format!(
        "Granted {role_id} -> {resource_id}: [{}].",
        if added.is_empty() {
            "(no new capabilities)".to_string()
        } else {
            added.join(", ")
        }
    ))
}

fn revoke_capability(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let caps = parse_caps(arguments["capabilities"].as_array())?;
    let mut org = load_or_seed(root);
    let mut removed = Vec::new();
    if let Some(g) = org
        .grants
        .iter_mut()
        .find(|g| g.role_id == role_id && g.resource_id == resource_id)
    {
        for c in &caps {
            if g.capabilities.contains(c) {
                g.capabilities.retain(|x| x != c);
                removed.push(c.as_str().to_string());
            }
        }
        if g.capabilities.is_empty() {
            org.grants
                .retain(|g| !(g.role_id == role_id && g.resource_id == resource_id));
        }
    }
    company_org::save_company_org(root, &org);
    Ok(format!(
        "Revoked from {role_id} on {resource_id}: [{}].",
        if removed.is_empty() {
            "(nothing)".to_string()
        } else {
            removed.join(", ")
        }
    ))
}

fn set_policy(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let resource_id = arguments["resource_id"].as_str().unwrap_or("").to_string();
    let threshold = arguments["threshold"].as_f64();
    let required_approvers: Vec<String> = arguments["required_approvers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let quorum = arguments["quorum"].as_u64().unwrap_or(1) as usize;
    let description = arguments["description"].as_str().unwrap_or("").to_string();

    let mut org = load_or_seed(root);
    let policy = ApprovalPolicy {
        id: id.to_string(),
        resource_id: resource_id.clone(),
        capability,
        threshold,
        required_approvers: required_approvers.clone(),
        quorum,
        description,
    };
    let replaced = if let Some(existing) = org.policies.iter_mut().find(|p| p.id == id) {
        *existing = policy;
        true
    } else {
        org.policies.push(policy);
        false
    };
    company_org::save_company_org(root, &org);
    let approvers_label = if required_approvers.is_empty() {
        "human-operator".to_string()
    } else {
        required_approvers.join(", ")
    };
    Ok(format!(
        "{} approval policy '{id}' ({} on {}, approvers [{}], quorum {}).",
        if replaced { "Updated" } else { "Added" },
        capability.as_str(),
        if resource_id.is_empty() {
            "ANY resource"
        } else {
            &resource_id
        },
        approvers_label,
        quorum.max(1)
    ))
}

fn remove_policy(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let mut org = load_or_seed(root);
    let before = org.policies.len();
    org.policies.retain(|p| p.id != id);
    let removed = before - org.policies.len();
    company_org::save_company_org(root, &org);
    Ok(if removed > 0 {
        format!("Removed approval policy '{id}'.")
    } else {
        format!("No policy '{id}' to remove.")
    })
}

// ── permission + pipeline ──────────────────────────────────────────────────

fn check_permission(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let org = load_or_seed(root);
    let decision = company_org::check_permission(&org, role_id, resource_id, capability, amount);
    Ok(match decision {
        company_org::Decision::Allowed => format!(
            "ALLOWED: {role_id} may {} on {resource_id} ({}).",
            capability.as_str(),
            amount
        ),
        company_org::Decision::RequiresApproval {
            approvers,
            quorum,
            reason,
            ..
        } => format!(
            "REQUIRES APPROVAL: {role_id} {} on {resource_id} needs {quorum} approval(s) from [{}] - {reason}.",
            capability.as_str(),
            if approvers.is_empty() { "human-operator".to_string() } else { approvers.join(", ") },
        ),
        company_org::Decision::Denied { reason } => {
            format!("DENIED: {reason}.")
        }
    })
}

/// Audit whether a role's credential access actually resolves — WITHOUT ever
/// returning the secret value. `resolve_credential` only releases the secret
/// when the permission gate returns Allowed; the handle is dropped (zeroized)
/// at the end of this call, so at most a masked preview is reported.
fn verify_credential(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let org = load_or_seed(root);
    let cap = capability.as_str();
    let outcome =
        match company_org::resolve_credential(&org, role_id, resource_id, capability, amount) {
            Ok(handle) => {
                if handle.label().is_empty() {
                    format!("GRANTED: resource '{resource_id}' needs no credential.")
                } else {
                    format!(
                        "GRANTED: provisioned via {} (masked {}).",
                        handle.label(),
                        handle.masked()
                    )
                }
                // `handle` drops here -> CredentialScope zeroizes the secret.
            }
            Err(blocked) => blocked.reason_line(),
        };
    Ok(format!("{outcome} [{role_id} -{cap}-> {resource_id}]"))
}

/// Rotate a stored `secret_ref` pointer through the credential-vault gate.
/// Never touches the raw secret — the org document carries pointers only, so
/// the response can be persisted/logged safely. Rotation on the credentials
/// meta-resource is dual-control by default in the UnitBuilds CC preset
/// (CEO + CTO), so the common path returns a Pending request id that the
/// caller resumes via `execute_org_credential_rotation_after_approval` once
/// both approvers have signed off.
fn rotate_credential(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let new_secret_ref = require_str(arguments, "new_secret_ref")?;
    let mut org = load_or_seed(root);
    let result = company_org::propose_credential_rotation(
        root,
        &mut org,
        role_id,
        resource_id,
        new_secret_ref,
    );
    let out = match result {
        company_org::RotationResult::Sealed(applied) => {
            company_org::save_company_org(root, &org);
            format!(
                "SEALED: rotated {resource} pointer ({prev} -> {next}).",
                resource = applied.resource_id,
                prev = if applied.previous_ref.is_empty() {
                    "(none)"
                } else {
                    &applied.previous_ref
                },
                next = applied.new_ref,
            )
        }
        company_org::RotationResult::Pending {
            request_id,
            approvers,
            quorum,
        } => {
            company_org::save_company_org(root, &org);
            format!(
                "PENDING: rotation queued as request {request_id}. Needs {quorum} approval(s) from [{}]. Resume with execute_org_credential_rotation_after_approval once cleared.",
                approvers.join(", "),
            )
        }
        company_org::RotationResult::Denied { reason } => format!("DENIED: {reason}."),
    };
    Ok(out)
}

/// Complete a rotation whose approval request has cleared. Mirrors the
/// gated-playbook resume path: request must be `Approved`, the swap is
/// applied, and the ledger entry transitions to `Executed` so a rotation
/// cannot be replayed.
fn execute_rotation_after_approval(
    root: &Path,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let mut org = load_or_seed(root);
    let applied = company_org::execute_rotation_after_approval(root, &mut org, request_id)
        .map_err(|e| -> Box<dyn Error> { e.into() })?;
    company_org::save_company_org(root, &org);
    Ok(format!(
        "EXECUTED: rotated '{}' pointer ({prev} -> {next}). request={req}.",
        applied.resource_id,
        prev = if applied.previous_ref.is_empty() {
            "(none)"
        } else {
            &applied.previous_ref
        },
        next = applied.new_ref,
        req = request_id,
    ))
}

/// Build a gated, secret-free cross-platform browser plan for a role. Returns
/// the ordered runtime actions (with the password referencing `vault:REF`, never
/// a literal secret), or the approval/denial block if the gate has not cleared.
fn plan_browser_login(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let opt = |k: &str| arguments[k].as_str().unwrap_or("").to_string();
    let composer_url = arguments["composer_url"].as_str();
    let composer_selector = arguments["composer_selector"].as_str();
    let read_script = arguments["read_script"].as_str();
    let capture_demo = arguments["capture_demo"].as_bool().unwrap_or(false);
    // Hoist every owned string so the borrowed spec fields outlive construction.
    let login_url = require_str(arguments, "login_url")?.to_string();
    let username_selector = opt("username_selector");
    let username = opt("username");
    let password_selector = opt("password_selector");
    let submit_selector = opt("submit_selector");
    let composer_text = opt("composer_text");
    let composer_submit_selector = opt("composer_submit_selector");
    let verify_expect = opt("verify_expect");
    let verify_absent = opt("verify_absent");
    let next_selector = opt("next_selector");
    let max_pages = arguments["max_pages"].as_u64().unwrap_or(0) as usize;
    let read_urls = parse_read_urls(arguments);
    let form_fields = parse_form_fields(arguments);
    let extra_actions = parse_extra_actions(arguments);
    let wait_for_selector = opt("wait_for_selector");
    let scroll_to = opt("scroll_to");
    let drag_from = opt("drag_from");
    let drag_to = opt("drag_to");
    let wait_for_url = opt("wait_for_url");
    let right_click_selector = opt("right_click_selector");
    let select_option_sel = opt("select_option_selector");
    let select_option_text = opt("select_option_text");
    let form_submit_selector = opt("form_submit_selector");
    let wait_for_text = opt("wait_for_text");
    let hover_selector = opt("hover_selector");
    let upload_selector = opt("upload_selector");
    let upload_files = parse_upload_files(arguments);
    let extract_attr_sel = opt("extract_attribute_selector");
    let extract_attr_name = opt("extract_attribute_name");
    let screenshot_selector = opt("screenshot_selector");
    let frame_selector = opt("frame_selector");
    let dialog_action = opt("dialog_action");
    let dialog_prompt_text = opt("dialog_prompt_text");
    let download_selector = opt("download_selector");
    let download_dir = opt("download_dir");
    let spec = company_org::BrowserTaskSpec {
        role_id,
        resource_id,
        capability,
        amount,
        login_url: &login_url,
        username_selector: &username_selector,
        username: &username,
        password_selector: &password_selector,
        submit_selector: &submit_selector,
        composer_url,
        composer_selector,
        composer_text: &composer_text,
        composer_submit_selector: &composer_submit_selector,
        read_script,
        capture_demo,
        extra_actions,
        settle_ms: arguments["settle_ms"].as_u64().unwrap_or(0),
        verify_expect: &verify_expect,
        verify_absent: &verify_absent,
        next_selector: &next_selector,
        max_pages,
        read_urls,
        form_fields,
        wait_for_selector: &wait_for_selector,
        scroll_to: &scroll_to,
        switch_tab: arguments["switch_tab"].as_bool().unwrap_or(false),
        drag_from: &drag_from,
        drag_to: &drag_to,
        wait_for_url: &wait_for_url,
        right_click_selector: &right_click_selector,
        select_option: (&select_option_sel, &select_option_text),
        form_submit_selector: &form_submit_selector,
        wait_for_text: &wait_for_text,
        hover_selector: &hover_selector,
        upload: (&upload_selector, upload_files),
        extract_attribute: (&extract_attr_sel, &extract_attr_name),
        screenshot_selector: &screenshot_selector,
        screenshot_full_page: arguments["screenshot_full_page"].as_bool().unwrap_or(false),
        frame_selector: &frame_selector,
        dialog_action: &dialog_action,
        dialog_prompt_text: &dialog_prompt_text,
        download_selector: &download_selector,
        download_dir: &download_dir,
        download_timeout_ms: arguments["download_timeout_ms"].as_u64().unwrap_or(0),
        wait_for_network_idle: arguments["wait_for_network_idle"]
            .as_bool()
            .unwrap_or(false),
        request_headers: parse_request_headers(arguments),
        cookies: parse_pairs(arguments, "cookies", "name"),
        local_storage: parse_pairs(arguments, "local_storage", "key"),
        basic_auth_user: arguments["basic_auth_user"].as_str().unwrap_or(""),
        basic_auth_pass: arguments["basic_auth_pass"].as_str().unwrap_or(""),
        user_agent: arguments["user_agent"].as_str().unwrap_or(""),
        viewport_width: arguments["viewport_width"].as_u64().unwrap_or(0) as u32,
        viewport_height: arguments["viewport_height"].as_u64().unwrap_or(0) as u32,
        captures: parse_captures(arguments),
    };
    let org = load_or_seed(root);
    let outcome = company_org::plan_org_browser_login(&org, &spec);
    match outcome {
        Ok(plan) => Ok(plan.describe()),
        Err(blocked) => Ok(blocked.reason_line()),
    }
}

/// Run a gated cross-platform browser task for a role against the live engine.
/// Same inputs as `plan_org_browser_login`, plus `dry_run` (default false). The
/// secret is resolved only on a real run and is never echoed; a blocked gate
/// returns the reason and opens no session.
fn execute_browser_task(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);
    let opt = |k: &str| arguments[k].as_str().unwrap_or("").to_string();
    let composer_url = arguments["composer_url"].as_str();
    let composer_selector = arguments["composer_selector"].as_str();
    let read_script = arguments["read_script"].as_str();
    let capture_demo = arguments["capture_demo"].as_bool().unwrap_or(false);
    let login_url = require_str(arguments, "login_url")?.to_string();
    let username_selector = opt("username_selector");
    let username = opt("username");
    let password_selector = opt("password_selector");
    let submit_selector = opt("submit_selector");
    let composer_text = opt("composer_text");
    let composer_submit_selector = opt("composer_submit_selector");
    let verify_expect = opt("verify_expect");
    let verify_absent = opt("verify_absent");
    let next_selector = opt("next_selector");
    let max_pages = arguments["max_pages"].as_u64().unwrap_or(0) as usize;
    let read_urls = parse_read_urls(arguments);
    let form_fields = parse_form_fields(arguments);
    let extra_actions = parse_extra_actions(arguments);
    let wait_for_selector = opt("wait_for_selector");
    let scroll_to = opt("scroll_to");
    let drag_from = opt("drag_from");
    let drag_to = opt("drag_to");
    let wait_for_url = opt("wait_for_url");
    let right_click_selector = opt("right_click_selector");
    let select_option_sel = opt("select_option_selector");
    let select_option_text = opt("select_option_text");
    let form_submit_selector = opt("form_submit_selector");
    let wait_for_text = opt("wait_for_text");
    let hover_selector = opt("hover_selector");
    let upload_selector = opt("upload_selector");
    let upload_files = parse_upload_files(arguments);
    let extract_attr_sel = opt("extract_attribute_selector");
    let extract_attr_name = opt("extract_attribute_name");
    let screenshot_selector = opt("screenshot_selector");
    let frame_selector = opt("frame_selector");
    let dialog_action = opt("dialog_action");
    let dialog_prompt_text = opt("dialog_prompt_text");
    let download_selector = opt("download_selector");
    let download_dir = opt("download_dir");
    let spec = company_org::BrowserTaskSpec {
        role_id,
        resource_id,
        capability,
        amount,
        login_url: &login_url,
        username_selector: &username_selector,
        username: &username,
        password_selector: &password_selector,
        submit_selector: &submit_selector,
        composer_url,
        composer_selector,
        composer_text: &composer_text,
        composer_submit_selector: &composer_submit_selector,
        read_script,
        capture_demo,
        extra_actions,
        settle_ms: arguments["settle_ms"].as_u64().unwrap_or(0),
        verify_expect: &verify_expect,
        verify_absent: &verify_absent,
        next_selector: &next_selector,
        max_pages,
        read_urls,
        form_fields,
        wait_for_selector: &wait_for_selector,
        scroll_to: &scroll_to,
        switch_tab: arguments["switch_tab"].as_bool().unwrap_or(false),
        drag_from: &drag_from,
        drag_to: &drag_to,
        wait_for_url: &wait_for_url,
        right_click_selector: &right_click_selector,
        select_option: (&select_option_sel, &select_option_text),
        form_submit_selector: &form_submit_selector,
        wait_for_text: &wait_for_text,
        hover_selector: &hover_selector,
        upload: (&upload_selector, upload_files),
        extract_attribute: (&extract_attr_sel, &extract_attr_name),
        screenshot_selector: &screenshot_selector,
        screenshot_full_page: arguments["screenshot_full_page"].as_bool().unwrap_or(false),
        frame_selector: &frame_selector,
        dialog_action: &dialog_action,
        dialog_prompt_text: &dialog_prompt_text,
        download_selector: &download_selector,
        download_dir: &download_dir,
        download_timeout_ms: arguments["download_timeout_ms"].as_u64().unwrap_or(0),
        wait_for_network_idle: arguments["wait_for_network_idle"]
            .as_bool()
            .unwrap_or(false),
        request_headers: parse_request_headers(arguments),
        cookies: parse_pairs(arguments, "cookies", "name"),
        local_storage: parse_pairs(arguments, "local_storage", "key"),
        basic_auth_user: arguments["basic_auth_user"].as_str().unwrap_or(""),
        basic_auth_pass: arguments["basic_auth_pass"].as_str().unwrap_or(""),
        user_agent: arguments["user_agent"].as_str().unwrap_or(""),
        viewport_width: arguments["viewport_width"].as_u64().unwrap_or(0) as u32,
        viewport_height: arguments["viewport_height"].as_u64().unwrap_or(0) as u32,
        captures: parse_captures(arguments),
    };
    let org = load_or_seed(root);
    let outcome = company_org::execute_org_browser_plan(root, &org, &spec, dry_run);
    match outcome {
        Ok(run) => Ok(with_extraction(
            &run,
            dry_run,
            &verify_expect,
            &verify_absent,
        )),
        Err(blocked) => Ok(blocked.reason_line()),
    }
}

/// Run a browser task for an *already-approved* action request. Where
/// `execute_org_browser_task` gates on the stateless permission matrix, this
/// gates on the request: it must exist and be `Approved`. That is how a gated
/// outward action (a marketing post awaiting counsel sign-off) actually executes
/// once a human/manager clears it. Requires `request_id`; same browser fields as
/// `execute_org_browser_task`, plus `dry_run`. A live run marks the request
/// `Executed` so it can't be replayed.
fn execute_browser_task_for_request(
    root: &Path,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);
    let opt = |k: &str| arguments[k].as_str().unwrap_or("").to_string();
    let composer_url = arguments["composer_url"].as_str();
    let composer_selector = arguments["composer_selector"].as_str();
    let read_script = arguments["read_script"].as_str();
    let capture_demo = arguments["capture_demo"].as_bool().unwrap_or(false);
    let login_url = require_str(arguments, "login_url")?.to_string();
    let username_selector = opt("username_selector");
    let username = opt("username");
    let password_selector = opt("password_selector");
    let submit_selector = opt("submit_selector");
    let composer_text = opt("composer_text");
    let composer_submit_selector = opt("composer_submit_selector");
    let verify_expect = opt("verify_expect");
    let verify_absent = opt("verify_absent");
    let next_selector = opt("next_selector");
    let max_pages = arguments["max_pages"].as_u64().unwrap_or(0) as usize;
    let read_urls = parse_read_urls(arguments);
    let form_fields = parse_form_fields(arguments);
    let extra_actions = parse_extra_actions(arguments);
    let wait_for_selector = opt("wait_for_selector");
    let scroll_to = opt("scroll_to");
    let drag_from = opt("drag_from");
    let drag_to = opt("drag_to");
    let wait_for_url = opt("wait_for_url");
    let right_click_selector = opt("right_click_selector");
    let select_option_sel = opt("select_option_selector");
    let select_option_text = opt("select_option_text");
    let form_submit_selector = opt("form_submit_selector");
    let wait_for_text = opt("wait_for_text");
    let hover_selector = opt("hover_selector");
    let upload_selector = opt("upload_selector");
    let upload_files = parse_upload_files(arguments);
    let extract_attr_sel = opt("extract_attribute_selector");
    let extract_attr_name = opt("extract_attribute_name");
    let screenshot_selector = opt("screenshot_selector");
    let frame_selector = opt("frame_selector");
    let dialog_action = opt("dialog_action");
    let dialog_prompt_text = opt("dialog_prompt_text");
    let download_selector = opt("download_selector");
    let download_dir = opt("download_dir");
    let spec = company_org::BrowserTaskSpec {
        role_id,
        resource_id,
        capability,
        amount,
        login_url: &login_url,
        username_selector: &username_selector,
        username: &username,
        password_selector: &password_selector,
        submit_selector: &submit_selector,
        composer_url,
        composer_selector,
        composer_text: &composer_text,
        composer_submit_selector: &composer_submit_selector,
        read_script,
        capture_demo,
        extra_actions,
        settle_ms: arguments["settle_ms"].as_u64().unwrap_or(0),
        verify_expect: &verify_expect,
        verify_absent: &verify_absent,
        next_selector: &next_selector,
        max_pages,
        read_urls,
        form_fields,
        wait_for_selector: &wait_for_selector,
        scroll_to: &scroll_to,
        switch_tab: arguments["switch_tab"].as_bool().unwrap_or(false),
        drag_from: &drag_from,
        drag_to: &drag_to,
        wait_for_url: &wait_for_url,
        right_click_selector: &right_click_selector,
        select_option: (&select_option_sel, &select_option_text),
        form_submit_selector: &form_submit_selector,
        wait_for_text: &wait_for_text,
        hover_selector: &hover_selector,
        upload: (&upload_selector, upload_files),
        extract_attribute: (&extract_attr_sel, &extract_attr_name),
        screenshot_selector: &screenshot_selector,
        screenshot_full_page: arguments["screenshot_full_page"].as_bool().unwrap_or(false),
        frame_selector: &frame_selector,
        dialog_action: &dialog_action,
        dialog_prompt_text: &dialog_prompt_text,
        download_selector: &download_selector,
        download_dir: &download_dir,
        download_timeout_ms: arguments["download_timeout_ms"].as_u64().unwrap_or(0),
        wait_for_network_idle: arguments["wait_for_network_idle"]
            .as_bool()
            .unwrap_or(false),
        request_headers: parse_request_headers(arguments),
        cookies: parse_pairs(arguments, "cookies", "name"),
        local_storage: parse_pairs(arguments, "local_storage", "key"),
        basic_auth_user: arguments["basic_auth_user"].as_str().unwrap_or(""),
        basic_auth_pass: arguments["basic_auth_pass"].as_str().unwrap_or(""),
        user_agent: arguments["user_agent"].as_str().unwrap_or(""),
        viewport_width: arguments["viewport_width"].as_u64().unwrap_or(0) as u32,
        viewport_height: arguments["viewport_height"].as_u64().unwrap_or(0) as u32,
        captures: parse_captures(arguments),
    };
    let org = load_or_seed(root);
    let outcome =
        company_org::execute_org_browser_plan_for_request(root, &org, request_id, &spec, dry_run);
    match outcome {
        Ok(run) => Ok(with_extraction(
            &run,
            dry_run,
            &verify_expect,
            &verify_absent,
        )),
        Err(blocked) => Ok(blocked.reason_line()),
    }
}

/// Parse the optional `read_urls` array into a bounded multi-page crawl list.
/// Each entry is a URL the read task visits and evaluates `read_script` on, in
/// one session — how a scout sweeps several competitor pages or marketing checks
/// several posted announcements without a separate call per page.
fn parse_read_urls(arguments: &Value) -> Vec<String> {
    arguments["read_urls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the optional `upload_files` array of workspace-relative file paths for a
/// file-upload step. Non-string entries are dropped; absent = empty (no upload).
fn parse_upload_files(arguments: &Value) -> Vec<String> {
    arguments["upload_files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the optional `form_fields` array into an ordered `(selector, value)`
/// list for declarative multi-field forms. Each entry is
/// `{"selector": "...", "value": "..."}`; the planner turns each into a
/// `runtime_session_fill` step, so a campaign/ticket form is authored as data
/// rather than a per-field `extra_actions` sequence.
fn parse_form_fields(arguments: &Value) -> Vec<(String, String)> {
    arguments["form_fields"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let selector = e["selector"].as_str()?;
                    let value = e["value"].as_str().unwrap_or("").to_string();
                    Some((selector.to_string(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the optional `captures` array into ordered `(name, selector)` pairs.
/// Each entry is `{"name": "invoiceId", "selector": "tr.row[data-id]"}` — the
/// planner emits a generated `runtime_session_evaluate` per pair, storing the
/// read value in drive_plan's variable map so later steps can reference
/// `{{vars.invoiceId}}` in any selector / URL / value arg. This is how a
/// banking SOP reads a specific row's id then clicks *that* row's pay button
/// without any AI re-inference between browser steps.
fn parse_captures(arguments: &Value) -> Vec<(String, String)> {
    arguments["captures"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let name = e["name"].as_str()?;
                    let selector = e["selector"].as_str()?;
                    Some((name.to_string(), selector.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_request_headers(arguments: &Value) -> Vec<(String, String)> {
    parse_pairs(arguments, "request_headers", "name")
}

/// Generic `["<field>": [{ <key_name>: k, value: v }, ...]]` reader used for the
/// session-bootstrap arrays: cookies (`{name, value}`) and localStorage
/// (`{key, value}`). The `key_name` argument names the first property so a
/// single helper serves both shapes.
fn parse_pairs(arguments: &Value, field: &str, key_name: &str) -> Vec<(String, String)> {
    arguments[field]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let name = e[key_name].as_str()?;
                    let value = e["value"].as_str()?;
                    Some((name.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the optional `extra_actions` array into ordered browser steps. Each
/// entry is `{"tool": "runtime_session_click", "args": {...}}`; `sessionId` may
/// be omitted — the planner injects the live-session placeholder so a Support
/// agent can click into a ticket or a tester open a feature tab before reading.
fn parse_extra_actions(arguments: &Value) -> Vec<company_org::BrowserAction> {
    arguments["extra_actions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let tool = e["tool"].as_str()?;
                    Some(company_org::BrowserAction {
                        tool: tool.to_string(),
                        args: e["args"].clone(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Render a browser run: the log-safe step summary plus, when a read/extract or
/// demo-capture step produced content, that content appended under an EXTRACTED
/// block. The extracted payload is intentionally kept out of `describe()` (which
/// never carries secrets) and surfaced only here, since reading a ticket/thread or
/// capturing a demo is the caller's explicit purpose.
fn with_extraction(run: &company_org::BrowserRun, dry: bool, verify: &str, absent: &str) -> String {
    let mut out = run.describe(dry);
    let text = run.extraction();
    if !text.is_empty() {
        out.push_str("\nEXTRACTED:\n");
        out.push_str(&text);
        out.push('\n');
    }
    // Deterministic acceptance check on the extracted content. No inference: a
    // plain containment assertion the Testing team pins a build against.
    if !verify.is_empty() {
        if dry {
            out.push_str(&format!(
                "\nVERIFY (pending): expects '{verify}' in extracted content.\n"
            ));
        } else if run.verify(verify) {
            out.push_str(&format!("\nVERIFY: PASS ('{verify}' present).\n"));
        } else {
            out.push_str(&format!(
                "\nVERIFY: FAIL (expected '{verify}', not found in extracted content).\n"
            ));
        }
    }
    // Deterministic regression guard: a substring that must NOT appear.
    if !absent.is_empty() {
        if dry {
            out.push_str(&format!(
                "\nABSENT (pending): expects '{absent}' NOT in extracted content.\n"
            ));
        } else if run.verify_absent(absent) {
            out.push_str(&format!("\nABSENT: PASS ('{absent}' not present).\n"));
        } else {
            out.push_str(&format!(
                "\nABSENT: FAIL (regression marker '{absent}' found in extracted content).\n"
            ));
        }
    }
    out
}

/// Fan an outward campaign across many platforms at once, gating each account
/// independently and returning a secret-free per-platform summary. Marketing
/// uses this to see which accounts are cleared to post and which await sign-off.
fn plan_browser_broadcast(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let arr = arguments["platforms"]
        .as_array()
        .ok_or("'platforms' must be an array of platform objects")?;
    let mut platforms = Vec::with_capacity(arr.len());
    for p in arr {
        let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
        platforms.push(company_org::PlatformLogin {
            resource_id: require_str(p, "resource_id")?.to_string(),
            login_url: s("login_url"),
            username_selector: s("username_selector"),
            username: s("username"),
            password_selector: s("password_selector"),
            submit_selector: s("submit_selector"),
            composer_url: p["composer_url"].as_str().map(|x| x.to_string()),
            composer_selector: p["composer_selector"].as_str().map(|x| x.to_string()),
            composer_text: s("composer_text"),
            composer_submit_selector: s("composer_submit_selector"),
            settle_ms: p["settle_ms"].as_u64().unwrap_or(0),
        });
    }
    let org = load_or_seed(root);
    let results =
        company_org::plan_org_browser_broadcast(&org, role_id, capability, amount, &platforms);
    Ok(company_org::describe_broadcast(&results))
}

/// Parse the `platforms` array into `(PlatformLogin, Option<request_id>)` pairs.
/// A `request_id` on a platform makes that account's *approved* action request
/// the execution gate; omitting it falls back to the stateless permission matrix.
fn parse_platforms_with_requests(
    arguments: &Value,
) -> Result<Vec<(company_org::PlatformLogin, Option<String>)>, Box<dyn Error>> {
    let arr = arguments["platforms"]
        .as_array()
        .ok_or("'platforms' must be an array of platform objects")?;
    let mut out = Vec::with_capacity(arr.len());
    for p in arr {
        let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
        out.push((
            company_org::PlatformLogin {
                resource_id: require_str(p, "resource_id")?.to_string(),
                login_url: s("login_url"),
                username_selector: s("username_selector"),
                username: s("username"),
                password_selector: s("password_selector"),
                submit_selector: s("submit_selector"),
                composer_url: p["composer_url"].as_str().map(|x| x.to_string()),
                composer_selector: p["composer_selector"].as_str().map(|x| x.to_string()),
                composer_text: s("composer_text"),
                composer_submit_selector: s("composer_submit_selector"),
                settle_ms: p["settle_ms"].as_u64().unwrap_or(0),
            },
            p["request_id"].as_str().map(|x| x.to_string()),
        ));
    }
    Ok(out)
}

/// Fan an outward campaign across many platforms and actually drive each cleared
/// account against the live browser. A platform carrying a `request_id` posts
/// only if that request is `Approved` (counsel-gated comms); a platform without
/// one posts only if the stateless gate returns `Allowed`. Blocked accounts are
/// reported, never leaked.
fn execute_browser_broadcast(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);
    let platforms = parse_platforms_with_requests(arguments)?;
    let org = load_or_seed(root);
    let results = company_org::execute_org_browser_broadcast(
        root, &org, role_id, capability, amount, &platforms, dry_run,
    );
    Ok(company_org::describe_broadcast_runs(&results, dry_run))
}

fn request_action(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let description = arguments["description"].as_str().unwrap_or("");
    let org = load_or_seed(root);
    let req = company_org::submit_action(
        root,
        &org,
        role_id,
        resource_id,
        capability,
        amount,
        description,
    );
    Ok(format!(
        "Action {} recorded as {:?}. {}",
        req.id, req.status, req.decision_reason
    ) + "\n"
        + &action_line(&req))
}

fn approve_action(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let approver_id = require_str(arguments, "approver_id")?;
    let org = load_or_seed(root);
    let req = company_org::approve_action(root, &org, request_id, approver_id)?;
    Ok(format!(
        "Approver '{approver_id}' signed {}. Now {:?}.",
        req.id, req.status
    ) + "\n"
        + &action_line(&req))
}

/// Sign a request and, if that signature reaches quorum, immediately run the
/// SOP the request came from — the one-click form of the `approve_org_action` +
/// `resume_org_playbook` pair, and exactly what the dashboard's APPROVE button
/// does. Below quorum nothing runs and the reply names the missing signatures; a
/// request that no playbook raised is approved without any script being guessed.
fn approve_and_resume(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let approver_id = require_str(arguments, "approver_id")?;
    let org = load_or_seed(root);
    Ok(company_org::approve_and_resume(
        root,
        &org,
        request_id,
        approver_id,
    )?)
}

fn reject_action(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let by = arguments["by"]
        .as_str()
        .unwrap_or(company_org::HUMAN_OPERATOR);
    let reason = arguments["reason"].as_str().unwrap_or("rejected");
    let req = company_org::reject_action(root, request_id, by, reason)?;
    Ok(format!("Rejected {}. {}", req.id, req.decision_reason))
}

fn list_actions(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let filter = arguments["status"].as_str().map(|s| s.to_lowercase());
    let all = company_org::load_requests(root);
    let wanted: Vec<&company_org::ActionRequest> = all
        .iter()
        .filter(|a| match filter.as_deref() {
            None => true,
            Some("pending") => matches!(a.status, company_org::ActionStatus::Pending),
            Some("approved") => matches!(a.status, company_org::ActionStatus::Approved),
            Some("rejected") => matches!(a.status, company_org::ActionStatus::Rejected),
            Some("executed") => matches!(a.status, company_org::ActionStatus::Executed),
            _ => true,
        })
        .collect();
    if wanted.is_empty() {
        return Ok("No matching action requests.".to_string());
    }
    let mut s = format!("{} action request(s):\n", wanted.len());
    for a in &wanted {
        s.push_str(&action_line(a));
        s.push('\n');
    }
    Ok(s)
}

/// Deterministic escalation routing for the approval pipeline. With an
/// `approver_id`, lists exactly the pending actions that approver is designated
/// to clear; without one, renders the whole-org roster of who owes a decision on
/// what. No inference: a plain ledger query the org can poll or surface in a UI.
fn list_escalations(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    if let Some(approver) = arguments.get("approver_id").and_then(|v| v.as_str()) {
        let mine = company_org::pending_for_approver(root, approver);
        if mine.is_empty() {
            return Ok(format!(
                "No pending escalations for '{approver}': nothing awaits their decision."
            ));
        }
        let org = load_or_seed(root);
        let mut s = format!(
            "{approver}: {} pending action(s) await your decision:\n",
            mine.len()
        );
        for a in &mine {
            s.push_str(&action_line(a));
            // Say what the signature is for: the SOP it runs, the chain hop that
            // queued it, and the duty waiting on the far side of this gate.
            s.push_str(&format!(
                "\n        \u{2514} {}",
                company_org::escalation_context(&org, a)
            ));
            s.push('\n');
        }
        return Ok(s);
    }
    Ok(company_org::escalation_roster(root))
}

// ── just-in-time credential elevation + audit ledger ───────────────────────

/// Mint a single-use, TTL-bounded elevation from an approved request. The clock
/// is read once here and passed to the primitive so behaviour is deterministic
/// and unit-testable independently of wall time.
fn grant_elevation(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let ttl_secs = arguments["ttl_secs"]
        .as_u64()
        .ok_or("'ttl_secs' must be a positive integer")?;
    let grant =
        company_org::grant_jit_elevation(root, request_id, ttl_secs, company_org::now_secs())
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
    Ok(format!(
        "GRANTED: elevation '{id}' for {role} -[{cap}]-> {res} from request {req}, active until t={exp}.",
        id = grant.id,
        role = grant.role_id,
        cap = grant.capability.as_str(),
        res = grant.resource_id,
        req = grant.request_id,
        exp = grant.expires_at,
    ))
}

/// Mint the elevation an approved Spend/Deploy request still needs and run the
/// playbook waiting on it, in one deterministic step — the completion half of
/// the financial/infrastructure pipeline (`approve_org_action` decides, this
/// executes). Refuses anything that is not Approved, so the click cannot mint
/// access ahead of the quorum.
fn elevate_and_resume_request(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let ttl_secs = arguments["ttl_secs"].as_u64().unwrap_or(300);
    let org = load_or_seed(root);
    Ok(company_org::elevate_and_resume(
        root,
        &org,
        request_id,
        ttl_secs,
        company_org::now_secs(),
    )?)
}

/// Deterministic gate check: is an elevation active for this scope right now?
fn check_elevation(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    match company_org::find_active_elevation(root, role_id, resource_id, capability, company_org::now_secs()) {
        Some(g) => Ok(format!(
            "ACTIVE: elevation '{}' (request {req}) usable once until t={exp}.",
            g.id,
            req = g.request_id,
            exp = g.expires_at,
        )),
        None => Ok(format!(
            "NONE: no active elevation for {role_id} -[{}]-> {resource_id}; fall back to standing permissions.",
            capability.as_str(),
        )),
    }
}

/// Spend an active elevation to authorize exactly one execution.
fn consume_elevation(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let grant_id = require_str(arguments, "grant_id")?;
    let g = company_org::consume_jit_elevation(root, grant_id, company_org::now_secs())
        .map_err(|e| -> Box<dyn Error> { e.into() })?;
    Ok(format!(
        "USED: elevation '{grant_id}' spent \u{2014} {role} -[{cap}]-> {res} (request {req}) is now single-use-consumed.",
        role = g.role_id,
        cap = g.capability.as_str(),
        res = g.resource_id,
        req = g.request_id,
    ))
}

/// Read the secret-free credential-use audit ledger.
fn list_credential_audit(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
    let entries = company_org::credential_audit_trail(root, limit);
    Ok(company_org::describe_credential_audit(&entries))
}

/// Cross-SOP verdict memory: the recent playbook runs that HALTED (a verdict
/// gate refused) or FAILED, as a secret-free escalation briefing. A halted SOP
/// is a stuck process — approvers must see it, not rediscover it by inference.
fn list_playbook_verdicts(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
    let entries = company_org::playbook_verdict_trail(root, limit);
    Ok(company_org::describe_playbook_verdicts(&entries))
}

// ── cross-SOP handoff chains ──────────────────────────────────────────────

/// Declare — or clear — the success handoff between two playbooks. Chaining
/// grants nothing: the downstream SOP is submitted under its own
/// role/capability/resource gate when it fires, exactly as if an operator had
/// started it, so a gated hop queues an approval instead of acting.
fn chain_playbooks(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let upstream = require_str(arguments, "upstream_id")?;
    let downstream = arguments["downstream_id"]
        .as_str()
        .filter(|s| !s.is_empty());
    let mut org = load_or_seed(root);
    Ok(company_org::chain_playbooks(
        root, &mut org, upstream, downstream,
    )?)
}

/// The recorded chain hops: which SOP handed off to which, how deep in the
/// chain, and what the downstream gate decided. Secret-free ledger rendering.
fn list_sop_handoffs(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
    let entries = company_org::sop_handoff_trail(root, limit);
    Ok(company_org::describe_sop_handoffs(&entries))
}

// ── recurring SOP scheduler ────────────────────────────────────────────────

/// Arm a cadence on an existing playbook. The scheduled run goes through the
/// same approval gate as a manual `run_org_playbook` — arming a duty never
/// grants it capability, it only decides *when* the request is made.
fn schedule_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let minutes = arguments["every_minutes"].as_u64().unwrap_or(60);
    let org = load_or_seed(root);
    let s = company_org::schedule_sop(root, &org, id, minutes.saturating_mul(60))?;
    Ok(format!(
        "Scheduled '{}' every {minutes}min: next run due at t={}. A gated SOP will queue an approval request when it fires, not act on its own.",
        s.playbook_id, s.next_due_at
    ))
}

/// Retire a cadence.
fn unschedule_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    Ok(if company_org::unschedule_sop(root, id) {
        format!("Removed the cadence on '{id}'.")
    } else {
        format!("No cadence was armed on '{id}'.")
    })
}

/// Pause (or resume) a cadence without disturbing its interval or due clock.
fn pause_schedule(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let paused = arguments["paused"].as_bool().unwrap_or(true);
    if company_org::set_sop_schedule_enabled(root, id, !paused) {
        Ok(format!(
            "Cadence on '{id}' is now {}.",
            if paused { "PAUSED" } else { "armed" }
        ))
    } else {
        Ok(format!("No cadence was armed on '{id}'."))
    }
}

/// Show what repeats and when each duty next fires.
fn list_schedules(root: &Path) -> Result<String, Box<dyn Error>> {
    let entries = company_org::sop_schedules(root);
    Ok(company_org::describe_sop_schedules(
        &entries,
        company_org::now_secs(),
    ))
}

/// The scheduler tick. `at` (unix seconds) is optional: passing an explicit
/// clock makes the tick reproducible and lets one call backfill duties that fell
/// due while the machine slept — each cadence fires at most once per call, so a
/// catch-up never replays a side effect N times.
fn run_due_playbooks(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let now = arguments["at"]
        .as_u64()
        .unwrap_or_else(company_org::now_secs);
    let org = load_or_seed(root);
    let fired = company_org::run_due_sops(root, &org, now);
    if fired.is_empty() {
        let armed = company_org::sop_schedules(root).len();
        return Ok(format!(
            "No SOPs due at t={now} — {armed} cadence(s) armed."
        ));
    }
    let mut out = format!("Ran {} due SOP(s) at t={now}:\n", fired.len());
    for (id, outcome) in &fired {
        out.push_str(&fmt_playbook_outcome(id, outcome.clone()));
        out.push('\n');
    }
    Ok(out)
}

// ── playbooks ──────────────────────────────────────────────────────────────

fn add_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let role_id = require_str(arguments, "role_id")?;
    let resource_id = require_str(arguments, "resource_id")?;
    let capability = Capability::parse(arguments["capability"].as_str().unwrap_or(""))
        .ok_or("a valid 'capability' is required")?;
    let amount = arguments["amount"].as_f64().unwrap_or(0.0);
    let name = arguments["name"].as_str().unwrap_or(id).to_string();
    let description = arguments["description"].as_str().unwrap_or("").to_string();
    // An MCP-authored SOP may declare a downstream handoff. The target id is
    // validated against the live playbook list further down, so a typo can
    // never become a silent dead end.
    let handoff_target = arguments["handoff_on_success"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let steps_json = arguments["steps"]
        .as_array()
        .ok_or("'steps' must be an array of step objects")?;
    let mut steps = Vec::with_capacity(steps_json.len());
    for (i, s) in steps_json.iter().enumerate() {
        steps.push(
            company_org::parse_step(s)
                .map_err(|e| -> Box<dyn Error> { format!("step {i}: {e}").into() })?,
        );
    }

    let mut org = load_or_seed(root);
    if org.find_role(role_id).is_none() {
        return Err(format!("unknown role '{role_id}'").into());
    }
    if org.find_resource(resource_id).is_none() {
        return Err(format!("unknown resource '{resource_id}'").into());
    }
    let on_success_handoff = match handoff_target {
        None => None,
        Some(target) => {
            if target == id {
                return Err("'handoff_on_success' must name a different playbook".into());
            }
            if !org.playbooks.iter().any(|p| p.id == target) {
                return Err(format!("handoff target playbook '{target}' does not exist").into());
            }
            Some(target)
        }
    };
    let pb = Playbook {
        id: id.to_string(),
        name,
        role_id: role_id.to_string(),
        resource_id: resource_id.to_string(),
        capability,
        amount,
        description,
        steps,
        on_success_handoff,
    };
    // Reject playbooks that reference unknown tools or connectors at write time.
    company_org::validate_step_references(Some(root), "playbook", id, &pb.steps)
        .map_err(|e| -> Box<dyn Error> { e.into() })?;
    let step_count = pb.steps.len();
    let replaced = if let Some(existing) = org.playbooks.iter_mut().find(|p| p.id == id) {
        *existing = pb;
        true
    } else {
        org.playbooks.push(pb);
        false
    };
    company_org::save_company_org(root, &org);
    Ok(format!(
        "{} playbook '{id}' ({} steps, gated on {} {} -> {}).",
        if replaced { "Updated" } else { "Added" },
        step_count,
        role_id,
        capability.as_str(),
        resource_id
    ))
}

fn remove_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let mut org = load_or_seed(root);
    let before = org.playbooks.len();
    org.playbooks.retain(|p| p.id != id);
    let removed = before - org.playbooks.len();
    company_org::save_company_org(root, &org);
    Ok(if removed > 0 {
        format!("Removed playbook '{id}'.")
    } else {
        format!("No playbook '{id}' to remove.")
    })
}

fn list_playbooks(root: &Path) -> Result<String, Box<dyn Error>> {
    let org = load_or_seed(root);
    if org.playbooks.is_empty() {
        return Ok("No playbooks defined.".to_string());
    }
    let mut s = format!("{} playbook(s):\n", org.playbooks.len());
    for p in &org.playbooks {
        // The declared chain edge rides along, so an operator reading the list
        // can see which duty springs which — and can spot a dead end.
        let chain = match &p.on_success_handoff {
            Some(t) if org.playbooks.iter().any(|x| x.id == *t) => format!(" | on clean run → {t}"),
            Some(t) => format!(" | on clean run → {t} (DANGLING)"),
            None => String::new(),
        };
        let mut tools = 0u32;
        let mut conds = 0u32;
        let mut agents = 0u32;
        let mut foreaches = 0u32;
        for st in &p.steps {
            match st {
                crate::editor::workflow::WorkflowStep::Tool { .. } => tools += 1,
                crate::editor::workflow::WorkflowStep::Condition { .. } => conds += 1,
                crate::editor::workflow::WorkflowStep::AgentTask { .. } => agents += 1,
                crate::editor::workflow::WorkflowStep::ForEach { .. } => foreaches += 1,
                crate::editor::workflow::WorkflowStep::Connector { .. } => tools += 1,
            }
        }
        s.push_str(&format!(
            "  - {} ({} step(s): T{} C{} A{} F{}): {} {} -> {}{}{}\n",
            p.id,
            p.steps.len(),
            tools,
            conds,
            agents,
            foreaches,
            p.role_id,
            p.capability.as_str(),
            p.resource_id,
            if p.amount > 0.0 {
                format!(" (${:.2})", p.amount)
            } else {
                String::new()
            },
            chain
        ));
    }
    Ok(s)
}

/// Validate all playbooks: check that every Tool step references a registered
/// tool and every Connector step an id present in the connector registry.
/// Returns a line per issue found, or "All playbooks valid." if none.
fn validate_playbooks(root: &Path) -> Result<String, Box<dyn Error>> {
    let org = load_or_seed(root);
    let mut issues: Vec<String> = Vec::new();
    for pb in &org.playbooks {
        if let Err(e) =
            company_org::validate_step_references(Some(root), "playbook", &pb.id, &pb.steps)
        {
            issues.push(e);
        }
    }
    if issues.is_empty() {
        Ok(format!(
            "All {} playbook(s) valid (tool and connector references resolved).",
            org.playbooks.len()
        ))
    } else {
        Ok(issues.join("\n"))
    }
}

fn run_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let id = require_str(arguments, "id")?;
    let org = load_or_seed(root);
    let outcome = company_org::run_playbook(root, &org, id)?;
    Ok(fmt_playbook_outcome(id, outcome))
}

/// Resume a gated playbook whose approval request has cleared. Takes the
/// `request_id` returned by a prior gated `run_org_playbook`; runs the stored
/// deterministic steps only if that request is now Approved (threading
/// `{{requestId}}` into outward-action browser steps so a gated post can finally
/// publish), and marks the request Executed. A still-pending request runs no step.
fn resume_playbook(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let request_id = require_str(arguments, "request_id")?;
    let org = load_or_seed(root);
    let outcome = company_org::resume_playbook_after_approval(root, &org, request_id)?;
    // Label the run with the SOP the engine actually resumed: the request names
    // its own playbook when it can, so a message can't claim a sibling on the
    // same gate ran instead.
    let label = company_org::load_requests(root)
        .into_iter()
        .find(|r| r.id == request_id)
        .and_then(|r| company_org::playbook_for_request(&org, &r).map(|p| p.id.clone()))
        .unwrap_or_else(|| request_id.to_string());
    Ok(fmt_playbook_outcome(&label, outcome))
}

fn fmt_playbook_outcome(id: &str, outcome: company_org::PlaybookOutcome) -> String {
    match outcome {
        company_org::PlaybookOutcome::Executed {
            request_id,
            status,
            ok_steps,
            total_steps,
            handoff,
        } => {
            let mut out = format!(
                "Executed playbook '{id}' ({status}): {ok_steps}/{total_steps} steps ok. request={request_id}."
            );
            if status == "failed" {
                out.push_str(" Stuck SOP: see list_org_playbook_verdicts for the halting step.");
            }
            // A chained SOP hands its successor on only after a clean run: the
            // caller sees the hop and the downstream gate's own verdict on it.
            if let Some(trail) = handoff {
                out.push_str("\nChain:");
                out.push('\n');
                out.push_str(&trail);
            }
            out
        }
        company_org::PlaybookOutcome::AwaitingApproval {
            request_id,
            approvers,
            quorum,
        } => format!(
            "Playbook '{id}' is gated: awaiting {quorum} approval(s) from [{}]. Steps did NOT run. request={request_id}.",
            approvers.join(", ")
        ),
        company_org::PlaybookOutcome::Denied { reason } => {
            format!("Playbook '{id}' denied: {reason}.")
        }
    }
}

// ── staffing bridge: sync org -> expert team, dispatch work to a role ───────

/// Project the org chart into Velocity's expert-team roster so the AI company
/// becomes runnable via `team_dispatch`. Idempotent: re-running re-derives the
/// members from the current org without disturbing other teams.
fn sync_team(root: &Path) -> Result<String, Box<dyn Error>> {
    let org = load_or_seed(root);
    let sync = company_org::sync_org_to_expert_team(root, &org);
    Ok(format!(
        "Synced org '{}' to expert team '{}' ({} member(s), {} new, {} carried over). Dispatch with team_id='{}'.",
        org.name,
        sync.team_slug,
        sync.member_count,
        sync.new_members,
        sync.carried_over,
        sync.team_slug
    ))
}

/// Route a natural-language task to the right org role and hand it to the
/// synced expert team for execution. The routing (role resolution) and the
/// team sync are deterministic; only the member's work itself is inference.
/// An outward/sensitive task is still gated at action time by the org approval
/// engine, so this never bypasses capability isolation.
fn dispatch_role(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let task = require_str(arguments, "task")?;
    let org = load_or_seed(root);

    // Resolve the role: explicit `role_id` wins, otherwise deterministic keyword routing.
    let role_id = match arguments["role_id"].as_str().map(str::trim) {
        Some(s) if !s.is_empty() => {
            if org.find_role(s).is_none() {
                return Err(format!("unknown role '{s}'").into());
            }
            Some(s.to_string())
        }
        _ => company_org::route_org_role_for_task(&org, task),
    };

    // Always ensure the roster is current before dispatching.
    let sync = company_org::sync_org_to_expert_team(root, &org);

    // Build the dispatch task, tagging the persona when we resolved a role.
    let dispatch_task = match &role_id {
        Some(id) => {
            let title = org
                .find_role(id)
                .map(|r| r.title.clone())
                .unwrap_or_else(|| id.clone());
            format!("[role:{id} - {title}] {task}")
        }
        None => task.to_string(),
    };

    let files: Vec<Value> = arguments["files"].as_array().cloned().unwrap_or_default();
    let args = serde_json::json!({
        "team_id": sync.team_slug,
        "task": dispatch_task,
        "files": files,
    });
    let routed = match &role_id {
        Some(id) => format!("routed to role '{id}'"),
        None => "no role matched; team default routing".to_string(),
    };
    let outcome = crate::registry::dispatch::call_tool_in_workspace(root, "team_dispatch", &args)?;
    Ok(format!(
        "Dispatched to team '{}' ({routed}).\n{outcome}",
        sync.team_slug
    ))
}
