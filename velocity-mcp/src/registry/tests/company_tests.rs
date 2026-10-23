//! Integration tests for the company-org MCP tools: verify dispatch, seeding,
//! deterministic permission checks and the request/approve pipeline through the
//! real tool entrypoint `call_tool_in_workspace`.

use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

#[test]
fn init_then_get_reports_unitbuilds_org() {
    let (_t, root) = setup_root();
    let out = call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    assert!(out.contains("UnitBuilds CC"), "{out}");
    assert!(out.contains("departments"), "{out}");

    let got = call_tool_in_workspace(&root, "get_company_org", &json!({})).unwrap();
    assert!(got.contains("CEO") && got.contains("CFO"), "{got}");
    assert!(got.contains("banking"), "{got}");
}

#[test]
fn check_permission_denies_unganted_tool() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let out = call_tool_in_workspace(
        &root,
        "check_org_permission",
        &json!({ "role_id": "support_agent", "resource_id": "banking", "capability": "spend", "amount": 50 }),
    )
    .unwrap();
    assert!(out.starts_with("DENIED"), "{out}");
}

#[test]
fn request_and_approve_pipeline_for_social_post() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "description": "launch tweet" }),
    )
    .unwrap();
    assert!(req.contains("Pending"), "{req}");
    // Pull the request id from the ledger line (which begins with the id).
    let req_id = req
        .lines()
        .find(|l| l.starts_with("req_"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    let approved = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(approved.contains("Approved"), "{approved}");

    let pending =
        call_tool_in_workspace(&root, "list_org_actions", &json!({ "status": "pending" })).unwrap();
    assert!(
        pending.contains("No matching") || !pending.contains(&req_id),
        "{pending}"
    );
}

#[test]
fn escalation_routes_pending_to_designated_approver_only() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "description": "launch tweet" }),
    )
    .unwrap();
    assert!(req.contains("Pending"), "{req}");
    let req_id = req
        .lines()
        .find(|l| l.starts_with("req_"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    // The designated approver (counsel) sees the item in their queue.
    let mine = call_tool_in_workspace(
        &root,
        "list_org_escalations",
        &json!({ "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(mine.contains("await your decision"), "{mine}");
    assert!(mine.contains(&req_id), "{mine}");

    // A non-designated approver's queue does not leak the item.
    let not_mine = call_tool_in_workspace(
        &root,
        "list_org_escalations",
        &json!({ "approver_id": "ceo" }),
    )
    .unwrap();
    assert!(not_mine.contains("No pending escalations"), "{not_mine}");
    assert!(!not_mine.contains(&req_id), "{not_mine}");

    // The whole-org roster names the approver who owes the decision.
    let roster = call_tool_in_workspace(&root, "list_org_escalations", &json!({})).unwrap();
    assert!(roster.contains("pending escalation"), "{roster}");
    assert!(roster.contains("counsel"), "{roster}");
}

#[test]
fn jit_elevation_pipeline_mints_checks_consumes_and_audits() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // A high-value banking spend clears the dual-control quorum (cfo + ceo).
    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "ceo", "resource_id": "banking", "capability": "spend", "amount": 5000, "description": "vendor payment" }),
    )
    .unwrap();
    assert!(req.contains("Pending"), "{req}");
    let req_id = req
        .lines()
        .find(|l| l.starts_with("req_"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    // An elevation cannot be minted while the request is still Pending.
    let too_early = call_tool_in_workspace(
        &root,
        "grant_org_credential_elevation",
        &json!({ "request_id": req_id, "ttl_secs": 300 }),
    );
    assert!(too_early.is_err(), "pending request must not elevate");

    call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cfo" }),
    )
    .unwrap();
    let approved = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "ceo" }),
    )
    .unwrap();
    assert!(approved.contains("Approved"), "{approved}");

    // Mint the TTL-bounded elevation scoped to the approved request.
    let grant = call_tool_in_workspace(
        &root,
        "grant_org_credential_elevation",
        &json!({ "request_id": req_id, "ttl_secs": 300 }),
    )
    .unwrap();
    assert!(grant.contains("GRANTED"), "{grant}");
    assert!(grant.contains(&format!("elev_{req_id}")), "{grant}");

    // The gate now reports an active elevation for the exact scope.
    let active = call_tool_in_workspace(
        &root,
        "check_org_credential_elevation",
        &json!({ "role_id": "ceo", "resource_id": "banking", "capability": "spend" }),
    )
    .unwrap();
    assert!(active.contains("ACTIVE"), "{active}");

    // Spend it once.
    let used = call_tool_in_workspace(
        &root,
        "consume_org_credential_elevation",
        &json!({ "grant_id": format!("elev_{req_id}") }),
    )
    .unwrap();
    assert!(used.contains("USED"), "{used}");

    // After consumption the gate reports NONE and a replay is refused.
    let gone = call_tool_in_workspace(
        &root,
        "check_org_credential_elevation",
        &json!({ "role_id": "ceo", "resource_id": "banking", "capability": "spend" }),
    )
    .unwrap();
    assert!(gone.contains("NONE"), "{gone}");
    let replay = call_tool_in_workspace(
        &root,
        "consume_org_credential_elevation",
        &json!({ "grant_id": format!("elev_{req_id}") }),
    );
    assert!(replay.is_err(), "a spent elevation cannot be replayed");

    // The audit ledger records the Granted and Used transitions and is secret-free.
    let audit = call_tool_in_workspace(&root, "list_org_credential_audit", &json!({ "limit": 50 }))
        .unwrap();
    assert!(
        audit.contains("GRANTED") || audit.contains("Granted"),
        "{audit}"
    );
    assert!(audit.contains(&req_id), "{audit}");
    assert!(
        !audit.contains("vault:"),
        "audit must never carry a secret pointer value"
    );
}

#[test]
fn grant_and_policy_edit_round_trip() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let g = call_tool_in_workspace(
        &root,
        "grant_capability",
        &json!({ "role_id": "support_agent", "resource_id": "github", "capabilities": ["read"] }),
    )
    .unwrap();
    assert!(g.contains("Granted"), "{g}");

    let p = call_tool_in_workspace(
        &root,
        "set_approval_policy",
        &json!({ "id": "test_gate", "capability": "deploy", "resource_id": "github", "required_approvers": ["cto"], "quorum": 1 }),
    )
    .unwrap();
    assert!(p.contains("Added") || p.contains("Updated"), "{p}");

    let rm = call_tool_in_workspace(
        &root,
        "remove_approval_policy",
        &json!({ "id": "test_gate" }),
    )
    .unwrap();
    assert!(rm.contains("Removed"), "{rm}");
}

#[test]
fn playbook_gate_blocks_until_approved() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let add = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "launch_tweet",
            "role_id": "social_operator",
            "resource_id": "social_x",
            "capability": "communicate",
            "description": "post launch",
            "steps": [ { "tool": "get_company_org" } ]
        }),
    )
    .unwrap();
    assert!(add.contains("Added playbook"), "{add}");

    let run = call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "launch_tweet" }))
        .unwrap();
    assert!(
        run.contains("gated") && run.contains("did NOT run"),
        "{run}"
    );

    let listed = call_tool_in_workspace(&root, "list_org_playbooks", &json!({})).unwrap();
    assert!(listed.contains("launch_tweet"), "{listed}");
}

#[test]
fn allowed_playbook_executes_via_tool() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "read_balances",
            "role_id": "accountant",
            "resource_id": "analytics",
            "capability": "read",
            "steps": [ { "tool": "get_company_org" } ]
        }),
    )
    .unwrap();
    let run = call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "read_balances" }))
        .unwrap();
    assert!(run.contains("Executed") && run.contains("1/1"), "{run}");
}

#[test]
fn verify_credential_never_leaks_secret() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // Denied path: support_agent has no banking spend grant.
    let denied = call_tool_in_workspace(
        &root,
        "verify_org_credential",
        &json!({ "role_id": "support_agent", "resource_id": "banking", "capability": "spend", "amount": 10 }),
    )
    .unwrap();
    assert!(denied.starts_with("DENIED"), "{denied}");

    // Gate path: ceo large spend needs approval, secret stays sealed.
    let gated = call_tool_in_workspace(
        &root,
        "verify_org_credential",
        &json!({ "role_id": "ceo", "resource_id": "banking", "capability": "spend", "amount": 5000 }),
    )
    .unwrap();
    assert!(gated.contains("NEEDS APPROVAL"), "{gated}");

    // Allowed but unprovisioned: accountant read banking -> MISSING (env unset).
    std::env::remove_var("UNITBUILDS_BANKING_TOKEN");
    let missing = call_tool_in_workspace(
        &root,
        "verify_org_credential",
        &json!({ "role_id": "accountant", "resource_id": "banking", "capability": "read" }),
    )
    .unwrap();
    assert!(missing.contains("MISSING"), "{missing}");
}

#[test]
fn browser_plan_is_gated_and_secret_free() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // Blocked: social post awaits counsel -> no plan.
    let gated = call_tool_in_workspace(
        &root,
        "plan_org_browser_login",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/login", "password_selector": "#pw" }),
    )
    .unwrap();
    assert!(gated.contains("NEEDS APPROVAL"), "{gated}");
    assert!(
        !gated.contains("runtime_session_fill"),
        "plan leaked despite gate: {gated}"
    );

    // Cleared: accountant read banking -> plan with vault pointer, no literal secret.
    let ok = call_tool_in_workspace(
        &root,
        "plan_org_browser_login",
        &json!({ "role_id": "accountant", "resource_id": "banking", "capability": "read", "login_url": "https://bank.test/login", "username_selector": "#u", "username": "ops@unitbuilds.com", "password_selector": "#p", "submit_selector": "button" }),
    )
    .unwrap();
    assert!(
        ok.contains("runtime_session_navigate") && ok.contains("vault:UNITBUILDS_BANKING_TOKEN"),
        "{ok}"
    );
}

#[test]
fn sync_org_team_projects_runnable_roster_idempotently() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let first = call_tool_in_workspace(&root, "sync_org_team", &json!({})).unwrap();
    assert!(first.contains("unitbuilds-cc"), "{first}");
    assert!(first.contains("member(s)"), "{first}");
    // The synced team must be readable back from the expert-team roster.
    let teams = crate::editor::expert_team::load_expert_teams(&root);
    let org_team = teams
        .iter()
        .find(|t| t.slug() == "unitbuilds-cc")
        .expect("unitbuilds-cc team present in roster");
    assert!(
        org_team.members.iter().any(|m| m.id == "ceo"),
        "CEO role must be a member"
    );

    // Idempotent: a second sync adds no new members.
    let second = call_tool_in_workspace(&root, "sync_org_team", &json!({})).unwrap();
    assert!(second.contains("0 new"), "{second}");
}

#[test]
fn dispatch_org_role_rejects_unknown_role_before_dispatch() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Forcing an unknown role must fail deterministically before any dispatch.
    let err = call_tool_in_workspace(
        &root,
        "dispatch_org_role",
        &json!({ "task": "review margins", "role_id": "nonexistent" }),
    );
    assert!(err.is_err(), "unknown role must be rejected");
}

#[test]
fn execute_org_browser_task_dry_run_is_gated_and_secret_free() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // Blocked gate -> no run, no session.
    let gated = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/login", "dry_run": true }),
    )
    .unwrap();
    assert!(gated.contains("NEEDS APPROVAL"), "{gated}");

    // Cleared gate, dry run -> preview with substituted session, no literal secret.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "accountant", "resource_id": "banking", "capability": "read", "login_url": "https://bank.test/login", "username_selector": "#u", "username": "ops@unitbuilds.com", "password_selector": "#p", "submit_selector": "button", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("runtime_session_navigate"), "{prev}");
    assert!(!prev.contains("{{sessionId}}"), "{prev}");
}

#[test]
fn plan_org_browser_broadcast_reports_per_platform_gating() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let out = call_tool_in_workspace(
        &root,
        "plan_org_browser_broadcast",
        &json!({
            "role_id": "social_operator",
            "capability": "communicate",
            "platforms": [
                { "resource_id": "social_x", "login_url": "https://x.com/login" },
                { "resource_id": "github", "login_url": "https://github.com/login" }
            ]
        }),
    )
    .unwrap();
    // social_x awaits counsel; github is not granted -> denied. Nothing cleared.
    assert!(out.contains("0 cleared, 2 blocked"), "{out}");
    assert!(
        out.contains("DENIED") && out.contains("NEEDS APPROVAL"),
        "{out}"
    );
}

#[test]
fn approved_request_unlocks_gated_browser_execution() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // social_operator -> social_x communicate is gated (needs counsel): the
    // stateless executor refuses before an approval exists.
    let gated = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/login", "password_selector": "#p", "submit_selector": "button", "dry_run": true }),
    )
    .unwrap();
    assert!(gated.contains("NEEDS APPROVAL"), "{gated}");

    // Submit the intent, leaving it Pending.
    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "description": "launch tweet" }),
    )
    .unwrap();
    let req_id = req
        .lines()
        .find(|l| l.starts_with("req_"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    // A PENDING request cannot be rehearsed as if cleared.
    let blocked = call_tool_in_workspace(
        &root,
        "execute_org_browser_task_for_request",
        &json!({ "request_id": req_id, "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/login", "dry_run": true }),
    )
    .unwrap();
    assert!(
        blocked.contains("NEEDS APPROVAL") && blocked.contains("not Approved"),
        "{blocked}"
    );

    // Counsel approves -> the request gate clears and the plan becomes runnable.
    let approved = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(approved.contains("Approved"), "{approved}");

    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task_for_request",
        &json!({ "request_id": req_id, "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/login", "password_selector": "#p", "submit_selector": "button", "composer_url": "https://x.com/compose", "composer_selector": "#post", "composer_text": "UnitBuilds ships!", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("runtime_session_navigate"), "{prev}");
    assert!(!prev.contains("{{sessionId}}"), "{prev}");
    // Secret-free: the plan references the vault pointer, never the token value.
    assert!(!prev.contains("UNITBUILDS_X_TOKEN"), "{prev}");
}

#[test]
fn gated_playbook_resumes_only_after_approval_and_cannot_replay() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // rotate_infra_secrets (ops_lead -> cloud deploy) is gated on the CTO, so a
    // run halts at AwaitingApproval and its steps do NOT execute.
    let gated = call_tool_in_workspace(
        &root,
        "run_org_playbook",
        &json!({ "id": "rotate_infra_secrets" }),
    )
    .unwrap();
    assert!(
        gated.contains("gated") && gated.contains("did NOT run"),
        "{gated}"
    );
    let req_id = gated
        .split("request=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("gated outcome carries a request id");

    // Resuming before approval runs nothing.
    let too_early = call_tool_in_workspace(
        &root,
        "resume_org_playbook",
        &json!({ "request_id": req_id }),
    )
    .unwrap();
    assert!(too_early.contains("did NOT run"), "{too_early}");

    // CTO signs off -> the gate clears.
    let approved = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cto" }),
    )
    .unwrap();
    assert!(approved.contains("Approved"), "{approved}");

    // Resume now runs the stored deterministic steps end to end.
    let resumed = call_tool_in_workspace(
        &root,
        "resume_org_playbook",
        &json!({ "request_id": req_id }),
    )
    .unwrap();
    assert!(
        resumed.contains("Executed playbook") && resumed.contains("steps ok"),
        "{resumed}"
    );

    // The approved request is marked Executed, so it can never be replayed.
    let executed =
        call_tool_in_workspace(&root, "list_org_actions", &json!({ "status": "executed" }))
            .unwrap();
    assert!(executed.contains(&req_id), "{executed}");
    let replay = call_tool_in_workspace(
        &root,
        "resume_org_playbook",
        &json!({ "request_id": req_id }),
    )
    .unwrap_or_else(|e| e.to_string());
    // A second resume sees an Executed (not Approved) request -> denied, no steps.
    assert!(
        replay.contains("denied") || replay.contains("did NOT run"),
        "replay must not re-execute: {replay}"
    );
}

#[test]
fn execute_org_browser_task_read_preview_includes_extract_steps() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // analyst Read on analytics is auto-allowed and needs no credential, so a
    // read + demo-capture task previews its extract steps without a live browser.
    // The extra_actions click step must be parsed and spliced in ahead of the read.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "analyst", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.test/report", "extra_actions": [{ "tool": "runtime_session_click", "args": { "selector": "#expand" } }], "read_script": "document.body.innerText", "capture_demo": true, "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(
        prev.contains("runtime_session_click"),
        "extra_actions click not parsed into the plan: {prev}"
    );
    assert!(prev.contains("runtime_session_evaluate -> ready"), "{prev}");
    assert!(prev.contains("runtime_capture_session -> ready"), "{prev}");
    // A dry run resolves nothing, so no secret is ever surfaced.
    assert!(!prev.contains("EXTRACTED"), "{prev}");
}

#[test]
fn execute_org_browser_broadcast_drives_cleared_and_blocks_others() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // Submit + have counsel approve an outward X post by the Marketing lead.
    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "mkt_lead", "resource_id": "social_x", "capability": "communicate", "description": "launch" }),
    )
    .unwrap();
    let req_id = req
        .split_whitespace()
        .find(|t| t.starts_with("req_"))
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("request id");
    let approved = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(approved.contains("Approved"), "{approved}");

    // Broadcast: X carries the approved request (clears); LinkedIn has no request
    // and its counsel gate leaves it blocked. dry_run so no live browser.
    let out = call_tool_in_workspace(
        &root,
        "execute_org_browser_broadcast",
        &json!({
            "role_id": "mkt_lead",
            "capability": "communicate",
            "dry_run": true,
            "platforms": [
                { "resource_id": "social_x", "login_url": "https://x.com/login", "request_id": req_id, "composer_url": "https://x.com/compose", "composer_selector": "#post", "composer_text": "Ship it" },
                { "resource_id": "social_linkedin", "login_url": "https://linkedin.com/login", "composer_text": "Ship it" }
            ]
        }),
    )
    .unwrap();
    assert!(out.contains("1 previewed, 1 blocked"), "{out}");
    assert!(out.contains("social_x"), "{out}");
    assert!(out.contains("NEEDS APPROVAL"), "{out}");
    // Secret-free: the pointer may appear, the resolved token never does.
    assert!(!out.contains("UNITBUILDS_X_TOKEN"), "{out}");
}

#[test]
fn pay_vendor_invoice_requires_cfo_and_ceo_dual_control() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // The spend SOP (amount >= 1,000) trips BOTH approval gates and runs no
    // steps until two distinct approvers clear it — deterministic human-in-loop.
    let gated = call_tool_in_workspace(
        &root,
        "run_org_playbook",
        &json!({ "id": "pay_vendor_invoice" }),
    )
    .unwrap();
    assert!(
        gated.contains("gated") && gated.contains("did NOT run"),
        "{gated}"
    );
    assert!(gated.contains("awaiting 2 approval(s)"), "{gated}");
    assert!(
        gated.contains("cfo") && gated.contains("ceo"),
        "both approvers named: {gated}"
    );
    let req_id = gated
        .split("request=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("gated outcome carries a request id");

    // A non-designated approver (counsel) is refused outright.
    let wrong = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap_or_else(|e| e.to_string());
    assert!(wrong.contains("not a designated approver"), "{wrong}");

    // One signature is not enough for quorum 2.
    let one = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cfo" }),
    )
    .unwrap();
    assert!(one.contains("Pending"), "first sig must not clear: {one}");

    // The second signature reaches quorum and clears the gate.
    let two = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "ceo" }),
    )
    .unwrap();
    assert!(two.contains("Approved"), "second sig should clear: {two}");
}

#[test]
fn check_service_uptime_previews_secret_free() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // sysadmin Read on cloud is auto-allowed (Read is not an outward capability,
    // even on a sensitive resource), so the uptime read previews without approval
    // and, on a dry run, resolves no cloud secret.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "sysadmin", "resource_id": "cloud", "capability": "read", "login_url": "https://status.unitbuilds.com", "read_script": "return document.title", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("runtime_session_navigate"), "{prev}");
    assert!(prev.contains("runtime_session_evaluate -> ready"), "{prev}");
    assert!(!prev.contains("UNITBUILDS_CLOUD_TOKEN"), "{prev}");
}

#[test]
fn revenue_report_previews_click_then_read_secret_free() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // The Operations analyst reads the revenue surface with a narrowing click
    // before the extract (generic extra_actions), settles, and no secret is
    // involved (analytics has none) — the whole deterministic flow previews.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "analyst", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/revenue", "extra_actions": [{"tool": "runtime_session_click", "args": {"selector": "[data-tab='mrr']"}}], "read_script": "return document.body.innerText", "settle_ms": 2000, "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("runtime_session_click"), "{prev}");
    assert!(prev.contains("runtime_session_evaluate"), "{prev}");
    assert!(
        !prev.contains("{{sessionId}}"),
        "extra_actions sessionId must be bound: {prev}"
    );
}

#[test]
fn support_reply_is_allowed_and_publishes_via_composer_click() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // An individual chat reply is outward Communicate on a NON-sensitive resource
    // with no policy, so it is Allowed (no approval kick) unlike a social post.
    // The composer publish control fires as a click, and the chat secret never
    // appears in the secret-free preview.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "support_agent", "resource_id": "chat", "capability": "communicate", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "composer_selector": "[data-testid=\"reply-box\"]", "composer_submit_selector": "button[aria-label=\"Send\"]", "composer_text": "Thanks for reaching out!", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    // The composer publishes via a publish-button click (runtime_session_click),
    // distinct from the login form submit (runtime_session_submit).
    assert!(
        prev.contains("runtime_session_click"),
        "composer publish click must fire: {prev}"
    );
    assert!(prev.contains("runtime_session_fill"), "{prev}");
    assert!(
        prev.contains("6/6 steps ready"),
        "login(4) + reply fill + publish click: {prev}"
    );
    assert!(
        !prev.contains("RequiresApproval"),
        "reply should be Allowed: {prev}"
    );
    assert!(!prev.contains("UNITBUILDS_CHAT_TOKEN"), "{prev}");
}

#[test]
fn read_task_previews_deterministic_verify_target() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // A QA read of the analytics surface with a verify_expect assertion: the dry
    // run previews the acceptance target (no browser touched, no inference). The
    // Testing team pins "the build rendered X" as a plain containment check.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "qa_engineer", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/staging", "read_script": "return document.body.innerText", "verify_expect": "Staging build green", "verify_absent": "Internal Server Error", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(
        prev.contains("VERIFY (pending): expects 'Staging build green'"),
        "{prev}"
    );
    assert!(
        prev.contains("ABSENT (pending): expects 'Internal Server Error' NOT"),
        "{prev}"
    );
    // Empty verify_expect must NOT surface a VERIFY line (no accidental assertion).
    let plain = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({ "role_id": "qa_engineer", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/staging", "read_script": "return document.body.innerText", "dry_run": true }),
    )
    .unwrap();
    assert!(!plain.contains("VERIFY"), "{plain}");
}

#[test]
fn approved_request_publishes_via_composer_click() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // social_operator -> social_x communicate is counsel-gated; approve it.
    let req = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({ "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "description": "post" }),
    )
    .unwrap();
    let req_id = req
        .split_whitespace()
        .find(|t| t.starts_with("req_"))
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("request id");
    call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();

    // With a composer_submit_selector, the cleared request's plan ends in a real
    // publish CLICK on the send button (not just a form submit).
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task_for_request",
        &json!({ "request_id": req_id, "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/i/flow/login", "composer_selector": "[data-testid='tweetTextarea_0']", "composer_text": "Ship it", "composer_submit_selector": "div[data-testid='tweetButton']", "dry_run": true }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(
        prev.contains("runtime_session_click"),
        "publish click missing: {prev}"
    );
    assert!(!prev.contains("{{sessionId}}"), "{prev}");
    assert!(!prev.contains("UNITBUILDS_X_TOKEN"), "{prev}");
}

#[test]
fn pagination_crawl_previews_bounded_reads_and_next_clicks() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // support_agent Read on chat is auto-allowed and needs a login. With a
    // next_selector + max_pages=3 the dry-run preview must show 3 reads and 2
    // next-clicks interleaved after the 4 login steps = 9 steps, no secret.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "support_agent", "resource_id": "chat", "capability": "read",
            "login_url": "https://app.unitbuilds.com/support/login",
            "username_selector": "input[name=\"email\"]", "username": "ops@unitbuilds.com",
            "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]",
            "read_script": "return document.querySelectorAll('[data-ticket]').length",
            "next_selector": "button[aria-label=\"Next page\"]", "max_pages": 3, "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("9/9 steps ready"), "expected 9 steps: {prev}");
    // 3 page reads + 2 next-clicks appear in the step log.
    let reads = prev.matches("runtime_session_evaluate").count();
    let nexts = prev.matches("runtime_session_click").count();
    assert_eq!(reads, 3, "3 page reads expected: {prev}");
    assert_eq!(nexts, 2, "2 next-clicks expected: {prev}");
    // The plan is secret-free even though a password field is present.
    assert!(!prev.contains("{{sessionId}}"), "{prev}");
    assert!(!prev.contains("UNITBUILDS_CHAT_TOKEN"), "{prev}");
}

#[test]
fn multi_url_read_crawl_previews_one_read_per_url() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // scout Read on analytics is a public, secret-free read. With 3 read_urls the
    // preview is navigate(login) + 3*(navigate + evaluate) = 7 steps.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "scout", "resource_id": "analytics", "capability": "read",
            "login_url": "https://example.com/competitor/changelog",
            "read_script": "return document.title",
            "read_urls": ["https://a.test/x", "https://b.test/y", "https://c.test/z"],
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("PREVIEW (dry-run)"), "{prev}");
    assert!(prev.contains("7/7 steps ready"), "expected 7 steps: {prev}");
    assert_eq!(
        prev.matches("runtime_session_evaluate").count(),
        3,
        "{prev}"
    );
    // login + one navigation per crawl URL = 4 navigate steps.
    assert_eq!(
        prev.matches("runtime_session_navigate").count(),
        4,
        "{prev}"
    );
}

#[test]
fn structured_form_fill_previews_ordered_fill_steps() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "support_agent",
            "resource_id": "chat",
            "capability": "communicate",
            "login_url": "https://app.unitbuilds.com/support/login",
            "username_selector": "input[name=\"email\"]",
            "username": "ops@unitbuilds.com",
            "password_selector": "input[name=\"password\"]",
            "submit_selector": "button[type=\"submit\"]",
            "form_fields": [
                {"selector": "input[name=\"subject\"]", "value": "Escalation"},
                {"selector": "select[name=\"priority\"]", "value": "high"},
                {"selector": "textarea[name=\"description\"]", "value": "login fails on staging"}
            ],
            "dry_run": true
        }),
    )
    .unwrap();
    // 1 navigate + 2 login fills + 1 submit + 3 form fills = 7 steps
    assert!(prev.contains("7/7 steps ready"), "expected 7 steps: {prev}");
    // 2 login fills + 3 form-field fills = 5 total runtime_session_fill
    assert_eq!(prev.matches("runtime_session_fill").count(), 5, "{prev}");
    // secret-free: vault pointer, never literal token
    assert!(!prev.contains("UNITBUILDS_CHAT_TOKEN"), "{prev}");
}

#[test]
fn wait_and_scroll_emit_deterministic_steps_in_preview() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/revenue",
            "wait_for_selector": "[data-panel='loaded']",
            "scroll_to": "[data-metric='mrr']",
            "read_script": "return document.body.innerText",
            "dry_run": true
        }),
    )
    .unwrap();
    // 4 steps: navigate -> wait -> scroll -> evaluate
    assert!(prev.contains("4/4 steps ready"), "expected 4 steps: {prev}");
    assert!(
        prev.contains("runtime_session_wait -> ready"),
        "wait step missing: {prev}"
    );
    assert!(
        prev.contains("runtime_session_scroll -> ready"),
        "scroll step missing: {prev}"
    );
}

#[test]
fn switch_tab_and_drag_preview_in_plan_steps() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/board",
            "username_selector": "input[name=\"email\"]",
            "username": "bot@unitbuilds.com",
            "password_selector": "input[name=\"password\"]",
            "submit_selector": "button[type=\"submit\"]",
            "extra_actions": [{"tool": "runtime_session_click", "args": {"selector": "a[target=_blank]"}}],
            "switch_tab": true,
            "drag_from": "[data-card='bug']",
            "drag_to": "[data-column='done']",
            "read_script": "return document.title",
            "dry_run": true
        }),
    )
    .unwrap();
    // navigate + 2 fill + submit + click + switch + drag + evaluate = 8 steps
    assert!(prev.contains("8/8 steps ready"), "expected 8 steps: {prev}");
    assert!(
        prev.contains("runtime_session_switch_tab -> ready"),
        "switch_tab step missing: {prev}"
    );
    assert!(
        prev.contains("runtime_session_drag -> ready"),
        "drag step missing: {prev}"
    );
    assert!(
        !prev.contains("UNITBUILDS_ANALYTICS_TOKEN"),
        "secret leak: {prev}"
    );
}

#[test]
fn form_submit_closes_fill_loop_in_preview() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "support_agent",
            "resource_id": "chat",
            "capability": "communicate",
            "login_url": "https://app.unitbuilds.com/support/login",
            "username_selector": "input[name=\"email\"]",
            "username": "ops@unitbuilds.com",
            "password_selector": "input[name=\"password\"]",
            "submit_selector": "button[type=\"submit\"]",
            "form_fields": [
                {"selector": "input[name=\"subject\"]", "value": "Escalation"},
                {"selector": "textarea[name=\"description\"]", "value": "login fails on staging"}
            ],
            "select_option_selector": "[role=listbox]",
            "select_option_text": "Critical",
            "form_submit_selector": "button[type=\"submit\"]",
            "dry_run": true
        }),
    )
    .unwrap();
    // navigate + 2 login fills + login submit + 2 form fills + select + form submit = 8 steps
    assert!(prev.contains("8/8 steps ready"), "expected 8 steps: {prev}");
    assert!(
        prev.contains("runtime_session_select -> ready"),
        "select step missing: {prev}"
    );
    // one click is the terminal form-submit (select -> click), distinct from the
    // login runtime_session_submit step.
    assert!(
        prev.contains("runtime_session_click -> ready"),
        "form-submit click step missing: {prev}"
    );
    assert!(
        !prev.contains("UNITBUILDS_CHAT_TOKEN"),
        "secret leak: {prev}"
    );
}

#[test]
fn wait_for_text_and_hover_preview_in_plan_steps() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/report",
            "wait_for_selector": "[data-panel]",
            "wait_for_text": "Loaded",
            "hover_selector": "[data-menu-trigger]",
            "read_script": "return document.body.innerText",
            "dry_run": true
        }),
    )
    .unwrap();
    // navigate + wait + wait_text + hover + evaluate = 5 steps (no login selectors)
    assert!(prev.contains("5/5 steps ready"), "expected 5 steps: {prev}");
    assert!(
        prev.contains("runtime_session_wait_text -> ready"),
        "wait_text step missing: {prev}"
    );
    assert!(
        prev.contains("runtime_session_hover -> ready"),
        "hover step missing: {prev}"
    );
}

#[test]
fn file_upload_preview_attaches_files() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "support_agent",
            "resource_id": "chat",
            "capability": "communicate",
            "login_url": "https://app.unitbuilds.com/support/login",
            "username_selector": "input[name=\"email\"]",
            "username": "ops@unitbuilds.com",
            "password_selector": "input[name=\"password\"]",
            "submit_selector": "button[type=\"submit\"]",
            "form_fields": [
                {"selector": "input[name=\"subject\"]", "value": "Crash on staging"}
            ],
            "upload_selector": "input[type=file]",
            "upload_files": ["logs/crash.txt"],
            "form_submit_selector": "button[type=\"submit\"]",
            "dry_run": true
        }),
    )
    .unwrap();
    // navigate + 2 login fills + login submit + 1 form fill + upload + form submit = 7 steps
    assert!(prev.contains("7/7 steps ready"), "expected 7 steps: {prev}");
    assert!(
        prev.contains("runtime_session_upload -> ready"),
        "upload step missing: {prev}"
    );
    assert!(
        !prev.contains("UNITBUILDS_CHAT_TOKEN"),
        "secret leak: {prev}"
    );
}

#[test]
fn extract_attribute_and_screenshot_preview_in_plan_steps() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/report",
            "extract_attribute_selector": "a.permalink",
            "extract_attribute_name": "href",
            "screenshot_selector": "[data-chart='mrr']",
            "dry_run": true
        }),
    )
    .unwrap();
    // navigate + evaluate (generated read) + screenshot = 3 steps
    assert!(prev.contains("3/3 steps ready"), "expected 3 steps: {prev}");
    assert!(
        prev.contains("runtime_session_evaluate -> ready"),
        "generated-extract step missing: {prev}"
    );
    assert!(
        prev.contains("runtime_session_screenshot -> ready"),
        "screenshot step missing: {prev}"
    );
}

#[test]
fn rich_browser_verbs_are_routed_not_unknown() {
    let (_t, root) = setup_root();
    // Every interaction verb the org planner emits must be dispatchable at the
    // browser layer — otherwise a *live* run silently no-ops with "unknown tool"
    // (the dry-run preview would still claim the step is ready). With a bogus
    // session id the generic forwarder errors on session lookup, which proves the
    // verb was routed; the failure mode we are guarding against is `Ok(None)`.
    for verb in [
        "runtime_session_wait",
        "runtime_session_wait_text",
        "runtime_session_wait_url",
        "runtime_session_scroll",
        "runtime_session_hover",
        "runtime_session_right_click",
        "runtime_session_select",
        "runtime_session_drag",
        "runtime_session_switch_tab",
        "runtime_session_upload",
        "runtime_session_screenshot",
        "runtime_session_enter_frame",
        "runtime_session_dialog",
        "runtime_session_download",
        "runtime_session_network_idle",
        "runtime_session_set_headers",
        "runtime_session_set_cookie",
        "runtime_session_set_storage",
        "runtime_session_basic_auth",
        "runtime_session_set_user_agent",
        "runtime_session_set_viewport",
    ] {
        let res = crate::registry::browser_tools::handle_browser_tool(
            &root,
            verb,
            &json!({ "sessionId": "no-such-session" }),
        );
        match res {
            // Routed: the handler attempted the action (fails on the missing session).
            Err(_) => {}
            Ok(Some(out)) => panic!("{verb} unexpectedly succeeded without a session: {out}"),
            Ok(None) => panic!("{verb} is UNKNOWN at the browser dispatch layer"),
        }
    }
}

#[test]
fn capture_variables_preview_emits_one_evaluate_per_pair() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // A read-only sweep: capture a specific row's id + amount due from the
    // analytics view, then click *that row's* details button — one gated
    // action, no AI re-inference between browser steps. The preview proves
    // captures emit before extra_actions and that `{{vars.*}}` tokens remain
    // verbatim in the emitted args (they resolve at dispatch inside drive_plan,
    // never at preview time).
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/invoices",
            "username_selector": "input[name=user]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "captures": [
                { "name": "invoiceId", "selector": "tr.row[data-id]:first-child" },
                { "name": "amountDue", "selector": "input[name=\"amount\"]" }
            ],
            "extra_actions": [
                { "tool": "runtime_session_click",
                  "args": { "selector": "tr[data-id='{{vars.invoiceId}}'] button.details" } }
            ],
            "dry_run": true
        }),
    )
    .unwrap();
    // Analyst Read on analytics is Allowed -> a runnable preview is produced.
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    // Two capture evaluates emit and the {{vars.*}}-bearing click survives
    // the planner untouched (7 steps total: nav + fill + fill + submit +
    // 2 evaluates + click).
    let evals = prev.matches("runtime_session_evaluate -> ready").count();
    assert!(
        evals >= 2,
        "expected 2 capture evaluates, saw {evals} in:\n{prev}"
    );
    assert!(
        prev.contains("runtime_session_click -> ready"),
        "the {{vars.*}}-bearing click step is missing:\n{prev}"
    );
}

#[test]
fn frame_entry_preview_emits_enter_frame_before_capture() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Banking-style iframe flow: after login, switch the driver into the
    // 3-D-Secure iframe, capture a value that only exists inside it, then
    // click the confirm button — also inside the frame. Preview ordering is
    // what makes this safe: `enter_frame -> evaluate -> click`, so a
    // frame-scoped selector cannot accidentally run against the outer doc.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/3ds",
            "username_selector": "input[name=user]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "frame_selector": "iframe[name=\"secure\"]",
            "captures": [{"name": "txnId", "selector": "input[name=txn]"}],
            "extra_actions": [
                { "tool": "runtime_session_click",
                  "args": { "selector": "button.confirm" } }
            ],
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    // Frame entry is dispatched as its own routable step.
    assert!(
        prev.contains("runtime_session_enter_frame -> ready"),
        "enter_frame step missing from preview:\n{prev}"
    );
    // Ordering: enter_frame appears before the capture evaluate, which
    // appears before the click — a swap would silently capture null.
    let idx_frame = prev.find("runtime_session_enter_frame -> ready").unwrap();
    let idx_eval = prev.find("runtime_session_evaluate -> ready").unwrap();
    let idx_click = prev.find("runtime_session_click -> ready").unwrap();
    assert!(
        idx_frame < idx_eval && idx_eval < idx_click,
        "expected enter_frame < evaluate < click, got frame@{idx_frame} eval@{idx_eval} click@{idx_click}\n{prev}"
    );
}

#[test]
fn dialog_handle_preview_emits_after_frame_before_capture() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Legacy 3-D-Secure: after entering the bank's secure iframe a native
    // `confirm()` modal blocks the driver. Preview order proves the plan
    // clears the dialog *before* trying to read anything inside the frame —
    // otherwise the evaluate would hang on a modal no selector can reach.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://bank.unitbuilds.com/3ds",
            "username_selector": "input[name=user]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "frame_selector": "iframe[name=\"secure\"]",
            "dialog_action": "accept",
            "dialog_prompt_text": "hunter2",
            "captures": [{"name": "challengeId", "selector": "input[name=cid]"}],
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    assert!(
        prev.contains("runtime_session_dialog -> ready"),
        "dialog step missing from preview:\n{prev}"
    );
    let idx_frame = prev.find("runtime_session_enter_frame -> ready").unwrap();
    let idx_dialog = prev.find("runtime_session_dialog -> ready").unwrap();
    let idx_eval = prev.find("runtime_session_evaluate -> ready").unwrap();
    assert!(
        idx_frame < idx_dialog && idx_dialog < idx_eval,
        "expected enter_frame < dialog < evaluate, got frame@{idx_frame} dialog@{idx_dialog} eval@{idx_eval}\n{prev}"
    );
}

#[test]
fn download_preview_emits_download_after_read() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Analyst downloads a CSV report after verifying the page:
    // evaluate (read) -> screenshot -> download.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://ci.unitbuilds.com/builds/staging",
            "username_selector": "input[name=user]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "read_script": "return document.querySelector('.artifact-name').textContent",
            "verify_expect": "velocity-1.2.3",
            "screenshot_full_page": true,
            "download_selector": "a[data-download=zip]",
            "download_dir": "/tmp/artifacts",
            "download_timeout_ms": 30000,
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    assert!(
        prev.contains("runtime_session_download -> ready"),
        "download step missing from preview:\n{prev}"
    );
    let idx_eval = prev.find("runtime_session_evaluate -> ready").unwrap();
    let idx_dl = prev.find("runtime_session_download -> ready").unwrap();
    assert!(
        idx_eval < idx_dl,
        "expected evaluate < download, got eval@{idx_eval} dl@{idx_dl}\n{prev}"
    );
}

#[test]
fn network_idle_and_headers_preview_in_order() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Analyst loads a SPA dashboard with tenant header and waits for idle:
    // set_headers -> navigate -> fill -> fill -> submit -> network_idle -> evaluate
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "analyst",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://dash.unitbuilds.com",
            "username_selector": "input[name=email]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "request_headers": [{"name": "X-Tenant", "value": "unitbuilds"}],
            "wait_for_network_idle": true,
            "settle_ms": 6000,
            "read_script": "return document.querySelector('.mrr').textContent",
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    assert!(
        prev.contains("runtime_session_set_headers -> ready"),
        "set_headers missing from preview:\n{prev}"
    );
    assert!(
        prev.contains("runtime_session_network_idle -> ready"),
        "network_idle missing from preview:\n{prev}"
    );
    let idx_hdr = prev.find("runtime_session_set_headers -> ready").unwrap();
    let idx_nav = prev.find("runtime_session_navigate -> ready").unwrap();
    let idx_idle = prev.find("runtime_session_network_idle -> ready").unwrap();
    let idx_eval = prev.find("runtime_session_evaluate -> ready").unwrap();
    assert!(
        idx_hdr < idx_nav && idx_nav < idx_idle && idx_idle < idx_eval,
        "headers < nav < idle < eval: hdr@{idx_hdr} nav@{idx_nav} idle@{idx_idle} eval@{idx_eval}\n{prev}"
    );
}

#[test]
fn export_preview_combines_headers_idle_read_and_download() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Operations (ops_lead) exports a tenant-scoped analytics CSV: the newest
    // verbs must all appear in one deterministic plan, headers first, download
    // last, and nothing may leak a secret into the preview.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "ops_lead",
            "resource_id": "analytics",
            "capability": "read",
            "login_url": "https://analytics.unitbuilds.com/export",
            "username_selector": "input[name=email]",
            "password_selector": "input[name=pass]",
            "submit_selector": "button.login",
            "request_headers": [{"name": "X-Tenant-ID", "value": "unitbuilds-cc"}],
            "wait_for_network_idle": true,
            "read_script": "return document.querySelector('[data-export-state]').textContent",
            "verify_expect": "ready",
            "screenshot_full_page": true,
            "download_selector": "a[data-download=csv]",
            "download_dir": "reports/",
            "download_timeout_ms": 45000,
            "settle_ms": 6000,
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    for verb in [
        "runtime_session_set_headers -> ready",
        "runtime_session_network_idle -> ready",
        "runtime_session_evaluate -> ready",
        "runtime_session_download -> ready",
    ] {
        assert!(prev.contains(verb), "{verb} missing from preview:\n{prev}");
    }
    // The download is emitted after the read (the export must finish before fetching).
    let idx_eval = prev.find("runtime_session_evaluate -> ready").unwrap();
    let idx_dl = prev.find("runtime_session_download -> ready").unwrap();
    assert!(
        idx_eval < idx_dl,
        "eval < download: eval@{idx_eval} dl@{idx_dl}\n{prev}"
    );
    // Secret-free: no credential value is echoed into the plan.
    assert!(
        !prev.contains("input[name=pass]\" : \"") && !prev.contains("password\":\""),
        "preview must not leak a password:\n{prev}"
    );
}

#[test]
fn cookie_bootstrap_preview_emits_session_steps_before_navigate() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // Operations reaches a token-auth status dashboard via cookie + localStorage
    // bootstrap (no password form). Both session steps must be routed and land
    // before navigation; the plan is secret-free.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "ops_lead",
            "resource_id": "cloud",
            "capability": "read",
            "login_url": "https://status.unitbuilds.internal/services",
            "cookies": [{"name": "sso_session", "value": "sess-abc123"}],
            "local_storage": [{"key": "auth.token", "value": "tok-xyz789"}],
            "wait_for_network_idle": true,
            "read_script": "return document.title",
            "verify_expect": "healthy",
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    assert!(
        prev.contains("runtime_session_set_cookie -> ready"),
        "set_cookie missing from preview:\n{prev}"
    );
    assert!(
        prev.contains("runtime_session_set_storage -> ready"),
        "set_storage missing from preview:\n{prev}"
    );
    // Session bootstrap precedes navigation.
    let idx_cookie = prev.find("runtime_session_set_cookie -> ready").unwrap();
    let idx_nav = prev.find("runtime_session_navigate -> ready").unwrap();
    assert!(idx_cookie < idx_nav, "cookie < navigate\n{prev}");
    // No password-form fill is emitted (token auth, not credentials).
    assert!(
        !prev.contains("runtime_session_submit -> ready"),
        "token-auth plan should not drive a login submit:\n{prev}"
    );
}

#[test]
fn device_override_preview_emits_basic_auth_ua_viewport_before_navigate() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // QA tests a basic-auth staging site under a mobile profile: all three
    // overrides must route and land before navigation. Following the plan's
    // secret-free contract, the basic-auth password is carried as a vault
    // POINTER (resolved only on a live run), never a literal.
    let prev = call_tool_in_workspace(
        &root,
        "execute_org_browser_task",
        &json!({
            "role_id": "qa_engineer",
            "resource_id": "cloud",
            "capability": "read",
            "login_url": "https://staging.unitbuilds.com/app",
            "basic_auth_user": "staging-bot",
            "basic_auth_pass": "{{vault:STAGING_BASIC_PASS}}",
            "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)",
            "viewport_width": 390,
            "viewport_height": 844,
            "read_script": "return document.title",
            "dry_run": true
        }),
    )
    .unwrap();
    assert!(prev.contains("steps ready"), "expected preview: {prev}");
    for verb in [
        "runtime_session_basic_auth -> ready",
        "runtime_session_set_user_agent -> ready",
        "runtime_session_set_viewport -> ready",
    ] {
        assert!(prev.contains(verb), "{verb} missing from preview:\n{prev}");
    }
    let idx_auth = prev.find("runtime_session_basic_auth -> ready").unwrap();
    let idx_nav = prev.find("runtime_session_navigate -> ready").unwrap();
    assert!(idx_auth < idx_nav, "basic_auth < navigate\n{prev}");
    // Secret-free contract: the dry-run preview lists the routed verbs but never
    // echoes step argument values, so no vault pointer or secret is disclosed.
    assert!(
        !prev.contains("STAGING_BASIC_PASS") && !prev.contains("{{vault:"),
        "preview must not echo step arg values:\n{prev}"
    );
}

// ── Credential rotation lifecycle ────────────────────────────────────────

#[test]
fn rotation_propose_approve_dual_execute_swaps_pointer() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // 1. Propose rotation for cloud by ops_lead -> Pending (dual-control).
    let proposed = call_tool_in_workspace(
        &root,
        "rotate_org_resource_credential",
        &json!({
            "role_id": "ops_lead",
            "resource_id": "cloud",
            "new_secret_ref": "UNITBUILDS_CLOUD_TOKEN_V2"
        }),
    )
    .unwrap();
    assert!(proposed.starts_with("PENDING"), "{proposed}");
    assert!(proposed.contains("request"), "{proposed}");
    let req_id = proposed
        .split("request ")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .unwrap()
        .trim()
        .to_string();

    // 2. Execute before approvals -> rejected (not yet Approved).
    let too_early = call_tool_in_workspace(
        &root,
        "execute_org_credential_rotation_after_approval",
        &json!({ "request_id": req_id }),
    );
    assert!(too_early.is_err(), "must fail while still Pending");

    // 3. Approve with CEO.
    let a1 = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "ceo" }),
    )
    .unwrap();
    assert!(
        a1.contains("Pending") || a1.contains("1/2"),
        "after 1/2: {a1}"
    );

    // 4. Approve with CTO -> Approved.
    let a2 = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cto" }),
    )
    .unwrap();
    assert!(a2.contains("Approved"), "after 2/2: {a2}");

    // 5. Execute the rotation.
    let executed = call_tool_in_workspace(
        &root,
        "execute_org_credential_rotation_after_approval",
        &json!({ "request_id": req_id }),
    )
    .unwrap();
    assert!(executed.contains("EXECUTED"), "{executed}");
    assert!(executed.contains("UNITBUILDS_CLOUD_TOKEN_V2"), "{executed}");

    // 6. Verify the org now carries the new pointer.
    let got = call_tool_in_workspace(&root, "get_company_org", &json!({ "full": true })).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
    let cloud = parsed["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some("cloud"))
        .unwrap();
    assert_eq!(
        cloud["secret_ref"].as_str(),
        Some("UNITBUILDS_CLOUD_TOKEN_V2"),
        "pointer must be swapped"
    );

    // 7. No-replay: second execute fails.
    let replay = call_tool_in_workspace(
        &root,
        "execute_org_credential_rotation_after_approval",
        &json!({ "request_id": req_id }),
    );
    assert!(
        replay.is_err(),
        "replay must fail: {:?}",
        replay.unwrap_err()
    );
}

#[test]
fn rotation_denied_for_ungranted_role_via_mcp() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let out = call_tool_in_workspace(
        &root,
        "rotate_org_resource_credential",
        &json!({
            "role_id": "support_agent",
            "resource_id": "cloud",
            "new_secret_ref": "V2"
        }),
    )
    .unwrap();
    assert!(out.starts_with("DENIED"), "{out}");
}

#[test]
fn approved_spend_without_elevation_returns_the_verdict_the_spend_gate_forbids() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // Trip the dual-control gate and clear it with both required approvers.
    let gated = call_tool_in_workspace(
        &root,
        "run_org_playbook",
        &json!({ "id": "pay_vendor_invoice" }),
    )
    .unwrap();
    let req_id = gated
        .split("request=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("gated outcome carries a request id");
    call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cfo" }),
    )
    .unwrap();
    call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "ceo" }),
    )
    .unwrap();

    // Approved, but no JIT elevation was minted: the request-gated executor
    // refuses BEFORE opening any browser session and surfaces its denial as
    // ordinary output ("DENIED: … elevation-gated …"). pay_vendor_invoice's
    // Phase 64 verdict gate lists "DENIED" among its forbidden verdicts, so
    // the SOP halts before the audit-receipt dispatch — an approved-but-
    // unelevated payment can never be reported as made. This is the exact
    // seam where Phase 62b's enforcement text meets Phase 63's branching.
    let denied = call_tool_in_workspace(
        &root,
        "execute_org_browser_task_for_request",
        &json!({
            "request_id": req_id,
            "role_id": "cfo",
            "resource_id": "banking",
            "capability": "spend",
            "amount": 1000.0,
            "login_url": "https://bank.unitbuilds.com/login",
            "username_selector": "input[name=\"user\"]",
            "password_selector": "input[name=\"pass\"]",
            "submit_selector": "button[type=\"submit\"]",
            "dry_run": false
        }),
    )
    .unwrap_or_else(|e| e.to_string());
    assert!(denied.contains("DENIED"), "{denied}");
    assert!(denied.contains("elevation"), "{denied}");
    // No run verdict: nothing executed, nothing to record.
    assert!(!denied.contains("Browser run"), "{denied}");
}

#[test]
fn mcp_authored_verdict_playbook_halts_and_is_recorded_in_memory() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // A page whose read-back carries a FAIL verdict — the MCP-authored SOP
    // gates its outward write on that verdict via the Phase 63 text predicate.
    std::fs::write(
        root.join("halt_page.txt"),
        "read back: campaign page\nVERIFY: FAIL (expected 'marker', not found). pageSecretToken=abc123\n",
    )
    .unwrap();
    let added = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "verdict_probe",
            "role_id": "qa_engineer",
            "resource_id": "analytics",
            "capability": "read",
            "name": "Verdict probe",
            "description": "read a page, gate on its verdict, then write a report",
            "steps": [
                { "tool": "read_file", "args": { "relativeFilePath": "halt_page.txt" } },
                { "condition": "ok", "text": { "all": ["VERIFY: PASS"], "none": ["VERIFY: FAIL"] } },
                { "tool": "write_file", "args": { "relativeFilePath": "should_not_exist.txt", "content": "x" } }
            ]
        }),
    )
    .unwrap();
    assert!(added.contains("verdict_probe"), "{added}");

    // Read on analytics is auto-allowed, so the steps run: the gate refuses
    // the FAIL verdict, the outward write never happens, and the run reports
    // itself as stuck with a pointer to the verdict memory.
    let ran = call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "verdict_probe" }))
        .unwrap();
    assert!(ran.contains("failed"), "{ran}");
    assert!(ran.contains("list_org_playbook_verdicts"), "{ran}");
    assert!(
        !std::path::Path::new(&root)
            .join("should_not_exist.txt")
            .exists(),
        "the halted gate must short-circuit the outward write"
    );

    // Cross-SOP verdict memory: the halt is a queryable fact after the run.
    let mem = call_tool_in_workspace(&root, "list_org_playbook_verdicts", &json!({})).unwrap();
    assert!(mem.contains("PLAYBOOK VERDICTS"), "{mem}");
    assert!(mem.contains("verdict_probe"), "{mem}");
    assert!(mem.contains("halted"), "{mem}");
    assert!(mem.contains("condition==ok"), "{mem}");
    // Secret-free: the ledger stores the engine's own message, never the
    // page-extracted content it was deciding about.
    assert!(!mem.contains("pageSecretToken"), "{mem}");
    assert!(!mem.contains("EXTRACTED"), "{mem}");
}

/// Phase 67: the org can now start its own work on a cadence — but a cadence is
/// a *clock*, not a permission. Ticking a gated SOP must land the request in the
/// approval queue, and advance-then-run must fire each duty exactly once.
#[test]
fn scheduled_sop_ticks_fire_once_and_still_pass_through_the_approval_gate() {
    let (_dir, root) = setup_root();

    let armed = call_tool_in_workspace(
        &root,
        "schedule_org_playbook",
        &json!({ "id": "pay_vendor_invoice", "every_minutes": 60 }),
    )
    .unwrap();
    assert!(
        armed.contains("Scheduled 'pay_vendor_invoice' every 60min"),
        "{armed}"
    );
    assert!(
        !armed.contains("Executed"),
        "arming must not run the SOP: {armed}"
    );
    let due_at: u64 = armed
        .split("t=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.trim_end_matches('.').parse().ok())
        .expect("due timestamp in the schedule confirmation");

    let listed = call_tool_in_workspace(&root, "list_org_schedules", &json!({})).unwrap();
    assert!(listed.contains("RECURRING SOPs (1 armed)"), "{listed}");
    assert!(listed.contains("every 60min"), "{listed}");

    // Before the clock: nothing fires, so no request is raised early.
    let early =
        call_tool_in_workspace(&root, "run_due_org_playbooks", &json!({ "at": due_at - 1 }))
            .unwrap();
    assert!(early.contains("No SOPs due"), "{early}");

    // Due: the gated spend queues for CFO+CEO instead of paying itself.
    let tick =
        call_tool_in_workspace(&root, "run_due_org_playbooks", &json!({ "at": due_at })).unwrap();
    assert!(tick.contains("Ran 1 due SOP(s)"), "{tick}");
    assert!(tick.contains("pay_vendor_invoice"), "{tick}");
    assert!(
        !tick.contains("Executed"),
        "the gate must still hold: {tick}"
    );

    // Same tick twice: the cadence already advanced, so the duty cannot double-fire
    // (and no second approval request is spawned for the same window).
    let again =
        call_tool_in_workspace(&root, "run_due_org_playbooks", &json!({ "at": due_at })).unwrap();
    assert!(again.contains("No SOPs due"), "{again}");

    // Paused duties are skipped even when overdue; unscheduling retires the clock.
    let paused = call_tool_in_workspace(
        &root,
        "pause_org_playbook_schedule",
        &json!({ "id": "pay_vendor_invoice", "paused": true }),
    )
    .unwrap();
    assert!(paused.contains("PAUSED"), "{paused}");
    let skipped = call_tool_in_workspace(
        &root,
        "run_due_org_playbooks",
        &json!({ "at": due_at + 3600 }),
    )
    .unwrap();
    assert!(skipped.contains("No SOPs due"), "{skipped}");

    let removed = call_tool_in_workspace(
        &root,
        "unschedule_org_playbook",
        &json!({ "id": "pay_vendor_invoice" }),
    )
    .unwrap();
    assert!(removed.contains("Removed the cadence"), "{removed}");
    let empty = call_tool_in_workspace(&root, "list_org_schedules", &json!({})).unwrap();
    assert!(empty.contains("No recurring SOPs armed"), "{empty}");
}

/// Phase 69: cross-SOP handoff chains over the MCP surface. Chaining moves WORK
/// to the next department and AUTHORITY nowhere — a gated hop lands in the
/// approval queue having written nothing, and the human who clears it resumes
/// the very SOP that queued the hop, not a sibling sharing the same gate.
#[test]
fn mcp_sop_chain_queues_a_gated_hop_and_resumes_the_exact_sop() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // The downstream is authored first: a chain target must exist when declared.
    // It shares the social_operator/social_x/communicate gate with the preset's
    // 4-step post_product_announcement — which is what a triple-only resume
    // would have run by mistake.
    let gated = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "announce_followup",
            "role_id": "social_operator",
            "resource_id": "social_x",
            "capability": "communicate",
            "name": "Announce follow-up",
            "description": "post the follow-up once Legal clears it",
            "steps": [
                { "tool": "write_file", "args": { "relativeFilePath": "followup.txt", "content": "posted" } }
            ]
        }),
    )
    .unwrap();
    assert!(gated.contains("announce_followup"), "{gated}");

    // A typo'd target is refused at authoring time, not dropped at run time.
    let dangling = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "ghost_note", "role_id": "qa_engineer", "resource_id": "analytics",
            "capability": "read", "handoff_on_success": "does_not_exist",
            "steps": [ { "tool": "get_company_org" } ]
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(dangling.contains("does not exist"), "{dangling}");

    // A self-handoff would spend the whole depth budget on a single node.
    let selfref = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "loop_note", "role_id": "qa_engineer", "resource_id": "analytics",
            "capability": "read", "handoff_on_success": "loop_note",
            "steps": [ { "tool": "get_company_org" } ]
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(selfref.contains("different playbook"), "{selfref}");

    let added = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "ship_note",
            "role_id": "qa_engineer",
            "resource_id": "analytics",
            "capability": "read",
            "name": "Ship note",
            "description": "record the ship, then queue the follow-up post",
            "handoff_on_success": "announce_followup",
            "steps": [
                { "tool": "write_file", "args": { "relativeFilePath": "note.txt", "content": "shipped" } }
            ]
        }),
    )
    .unwrap();
    assert!(added.contains("ship_note"), "{added}");

    let listed = call_tool_in_workspace(&root, "list_org_playbooks", &json!({})).unwrap();
    assert!(
        listed.contains("on clean run → announce_followup"),
        "{listed}"
    );

    // Read on analytics is auto-allowed, so the duty runs and then hands off.
    let ran =
        call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "ship_note" })).unwrap();
    assert!(ran.contains("success"), "{ran}");
    assert!(ran.contains("Chain:"), "{ran}");
    assert!(ran.contains("queued, steps did NOT run"), "{ran}");
    assert!(
        root.join("note.txt").exists(),
        "the upstream ran its own step"
    );
    assert!(
        !root.join("followup.txt").exists(),
        "a gated hop must not execute on the chain's say-so"
    );

    let req_id = ran
        .split("(req=")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("the chain hop carries its request id");

    // The hop is an ordinary queue entry, labelled with the SOP it unblocks.
    let queue = call_tool_in_workspace(&root, "list_org_actions", &json!({})).unwrap();
    assert!(queue.contains(&req_id), "{queue}");
    assert!(queue.contains("announce_followup"), "{queue}");

    let hops = call_tool_in_workspace(&root, "list_org_sop_handoffs", &json!({})).unwrap();
    assert!(hops.contains("SOP HANDOFFS (1 hop)"), "{hops}");
    assert!(hops.contains("ship_note"), "{hops}");
    assert!(hops.contains("awaiting-approval"), "{hops}");
    // Secret-free: neither SOP's step content reaches the ledger.
    assert!(!hops.contains("posted"), "{hops}");
    assert!(!hops.contains("shipped"), "{hops}");

    // Legal signs off on the hop's own request; resuming runs the 1-step SOP.
    let cleared = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(cleared.contains("Approved"), "{cleared}");
    let resumed = call_tool_in_workspace(
        &root,
        "resume_org_playbook",
        &json!({ "request_id": req_id }),
    )
    .unwrap();
    assert!(resumed.contains("announce_followup"), "{resumed}");
    assert!(resumed.contains("1/1 steps ok"), "{resumed}");
    assert!(root.join("followup.txt").exists(), "the approved hop ran");

    // Re-pointing the chain through the tool persists into the sealed org.
    let chained = call_tool_in_workspace(
        &root,
        "chain_org_playbooks",
        &json!({ "upstream_id": "ship_note", "downstream_id": "weekly_metrics_report" }),
    )
    .unwrap();
    assert!(
        chained.contains("Chained 'ship_note' → 'weekly_metrics_report'"),
        "{chained}"
    );
    let listed2 = call_tool_in_workspace(&root, "list_org_playbooks", &json!({})).unwrap();
    assert!(
        listed2.contains("on clean run → weekly_metrics_report"),
        "{listed2}"
    );

    // Clearing the edge is explicit, and the ledger keeps the history.
    let uncoupled = call_tool_in_workspace(
        &root,
        "chain_org_playbooks",
        &json!({ "upstream_id": "ship_note" }),
    )
    .unwrap();
    assert!(uncoupled.contains("Uncoupled"), "{uncoupled}");
    let still = call_tool_in_workspace(&root, "list_org_sop_handoffs", &json!({})).unwrap();
    assert!(still.contains("SOP HANDOFFS (1 hop)"), "{still}");
}

/// One-click human-in-the-loop: the signature that reaches quorum also runs the
/// SOP that asked for it, so an approved duty cannot be left sitting in the
/// ledger with its side effect unwritten (this is what the dashboard's APPROVE
/// button calls).
#[test]
fn mcp_approve_and_resume_runs_the_gated_sop_in_one_call() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "launch_once",
            "role_id": "social_operator",
            "resource_id": "social_x",
            "capability": "communicate",
            "name": "Launch post",
            "description": "write the launch marker once Legal signs",
            "steps": [
                { "tool": "write_file", "args": { "relativeFilePath": "launch.txt", "content": "live" } }
            ]
        }),
    )
    .unwrap();

    let gated =
        call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "launch_once" })).unwrap();
    assert!(gated.contains("did NOT run"), "{gated}");
    let req_id = gated
        .split("request=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("gated outcome carries a request id");
    assert!(
        !root.join("launch.txt").exists(),
        "a gated SOP writes nothing before sign-off"
    );

    let done = call_tool_in_workspace(
        &root,
        "approve_and_resume_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(done.contains("quorum reached"), "{done}");
    assert!(done.contains("Ran 'launch_once'"), "{done}");
    assert!(done.contains("1/1 steps ok"), "{done}");
    assert!(root.join("launch.txt").exists(), "{done}");

    // The ledger closes the loop: the request is Executed, and a second click
    // is refused rather than posting twice.
    let executed =
        call_tool_in_workspace(&root, "list_org_actions", &json!({ "status": "executed" }))
            .unwrap();
    assert!(executed.contains(&req_id), "{executed}");
    let replay = call_tool_in_workspace(
        &root,
        "approve_and_resume_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap_or_else(|e| e.to_string());
    assert!(replay.contains("not pending"), "{replay}");

    // A request raised directly carries no stored script: the signature clears
    // it, but the tool invents no work for it.
    let manual = call_tool_in_workspace(
        &root,
        "request_org_action",
        &json!({
            "role_id": "social_operator",
            "resource_id": "social_x",
            "capability": "communicate",
            "description": "one-off post"
        }),
    )
    .unwrap();
    let manual_id = manual
        .split_whitespace()
        .find(|t| t.starts_with("req_"))
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("request id");
    let manual_done = call_tool_in_workspace(
        &root,
        "approve_and_resume_org_action",
        &json!({ "request_id": manual_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(
        manual_done.contains("not raised by a playbook"),
        "{manual_done}"
    );
    assert!(
        manual_done.contains("no stored steps to run"),
        "{manual_done}"
    );
}

/// An approver deciding an irreversible publish should see the shape of the work,
/// not just a (role, capability, resource) triple: which SOP their signature
/// runs, which duty already finished to queue it, and what waits behind it.
#[test]
fn mcp_escalation_shows_the_chain_context_behind_a_pending_hop() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    for (id, role, resource, cap, handoff) in [
        (
            "confirm_followup",
            "qa_engineer",
            "analytics",
            "read",
            serde_json::Value::Null,
        ),
        (
            "announce_followup",
            "social_operator",
            "social_x",
            "communicate",
            json!("confirm_followup"),
        ),
        (
            "ship_note",
            "accountant",
            "analytics",
            "read",
            json!("announce_followup"),
        ),
    ] {
        call_tool_in_workspace(
            &root,
            "add_org_playbook",
            &json!({
                "id": id,
                "role_id": role,
                "resource_id": resource,
                "capability": cap,
                "handoff_on_success": handoff,
                "steps": [ { "tool": "get_company_org" } ]
            }),
        )
        .unwrap();
    }

    // The auto-allowed duty runs and hands off; the gated hop sits in the queue.
    let ran =
        call_tool_in_workspace(&root, "run_org_playbook", &json!({ "id": "ship_note" })).unwrap();
    assert!(ran.contains("queued, steps did NOT run"), "{ran}");

    let queue = call_tool_in_workspace(
        &root,
        "list_org_escalations",
        &json!({ "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(
        queue.contains("signing runs 'announce_followup'"),
        "{queue}"
    );
    assert!(queue.contains("(hop from 'ship_note')"), "{queue}");
    assert!(
        queue.contains("then 'confirm_followup' awaits its own gate"),
        "{queue}"
    );

    // The whole-org roster says the same thing without needing an approver id.
    let roster = call_tool_in_workspace(&root, "list_org_escalations", &json!({})).unwrap();
    assert!(roster.contains("counsel"), "{roster}");
    assert!(
        roster.contains("signing runs 'announce_followup'"),
        "{roster}"
    );

    // Signing the hop runs the SOP the context named — the line is a promise the
    // engine keeps, not decoration.
    let req_id = ran
        .split("(req=")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("the hop carries its request id");
    let done = call_tool_in_workspace(
        &root,
        "approve_and_resume_org_action",
        &json!({ "request_id": req_id, "approver_id": "counsel" }),
    )
    .unwrap();
    assert!(done.contains("Ran 'announce_followup'"), "{done}");
}

/// The second half of the financial pipeline over MCP: a dual-control spend that
/// has been signed is still not executable until its scope carries a live JIT
/// elevation, and one call completes both steps.
#[test]
fn mcp_elevate_and_resume_completes_an_approved_spend() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    let gated = call_tool_in_workspace(
        &root,
        "run_org_playbook",
        &json!({ "id": "pay_vendor_invoice" }),
    )
    .unwrap();
    let req_id = gated
        .split("request=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_end_matches('.').to_string())
        .expect("gated spend carries a request id");
    call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "cfo" }),
    )
    .unwrap();
    let signed = call_tool_in_workspace(
        &root,
        "approve_org_action",
        &json!({ "request_id": req_id, "approver_id": "ceo" }),
    )
    .unwrap();
    assert!(signed.contains("Approved"), "{signed}");

    // Approved, but the elevation factor still holds the money back.
    let held = call_tool_in_workspace(
        &root,
        "execute_org_browser_task_for_request",
        &json!({
            "request_id": req_id,
            "role_id": "cfo",
            "resource_id": "banking",
            "capability": "spend",
            "amount": 1000.0,
            "login_url": "https://bank.unitbuilds.com/login"
        }),
    )
    .unwrap_or_else(|e| e.to_string());
    assert!(
        held.to_lowercase().contains("elevat")
            || held.contains("Denied")
            || held.contains("denied"),
        "an approved-but-unelevated spend must be refused: {held}"
    );

    let done = call_tool_in_workspace(
        &root,
        "elevate_and_resume_org_request",
        &json!({ "request_id": req_id, "ttl_secs": 300 }),
    )
    .unwrap();
    assert!(done.contains("Minted elevation"), "{done}");
    assert!(done.contains("Ran 'pay_vendor_invoice'"), "{done}");

    let audit = call_tool_in_workspace(&root, "list_org_credential_audit", &json!({})).unwrap();
    assert!(audit.contains("Granted"), "{audit}");

    // One click, one execution: the request is no longer Approved, so a repeat
    // cannot mint a second elevation for the same invoice.
    let again = call_tool_in_workspace(
        &root,
        "elevate_and_resume_org_request",
        &json!({ "request_id": req_id, "ttl_secs": 300 }),
    )
    .unwrap_or_else(|e| e.to_string());
    assert!(again.contains("not Approved"), "{again}");
}

/// Phase 73: queue_invoice_payments ForEach SOP integration test.
/// The SOP's gate is Read (auto-approved), so the workflow executes its steps.
/// In test mode the browser is absent → credential MISSING → verdict_gate fails
/// (output lacks "[") → ForEach is Skipped → no Spend requests are created.
/// This proves the deterministic fan-out is properly gated behind a successful
/// browser extraction and can never fire on a blocked/missing credential.
#[test]
fn mcp_queue_invoice_payments_foreach_halts_behind_verdict_gate() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();

    // The SOP must appear in the listing with its 5 steps.
    let listed = call_tool_in_workspace(&root, "list_org_playbooks", &json!({})).unwrap();
    assert!(
        listed.contains("queue_invoice_payments"),
        "SOP not seeded: {listed}"
    );
    assert!(
        listed.contains("(5 step(s):"),
        "expected step breakdown for queue_invoice_payments: {listed}"
    );

    // Run the playbook. CFO/banking/Read auto-approves, so steps execute.
    let ran = call_tool_in_workspace(
        &root,
        "run_org_playbook",
        &json!({ "id": "queue_invoice_payments" }),
    )
    .unwrap();
    // Status: failed — verdict_gate halts before ForEach.
    assert!(
        ran.contains("failed"),
        "expected failed status (verdict gate): {ran}"
    );
    // 3 ok steps: verify_org_credential + proceed + execute_org_browser_task.
    assert!(
        ran.contains("3/5 steps ok"),
        "expected 3/5 ok (verdict + foreach failed/skipped): {ran}"
    );

    // ForEach was skipped → no Spend request was submitted by the loop body.
    // The playbook's own request (Read) exists but no second Spend request.
    let actions = call_tool_in_workspace(&root, "list_org_actions", &json!({})).unwrap();
    assert!(
        !actions.contains("spend"),
        "ForEach must not fire on a failed verdict: {actions}"
    );

    // The verdict ledger records the halt so operators can see the stuck SOP.
    let verdicts = call_tool_in_workspace(&root, "list_org_playbook_verdicts", &json!({})).unwrap();
    assert!(
        verdicts.contains("queue_invoice_payments"),
        "halted SOP must appear in verdict ledger: {verdicts}"
    );
    assert!(
        verdicts.contains("halted") || verdicts.contains("failed"),
        "verdict outcome should indicate failure: {verdicts}"
    );
    // Secret-free: the step output (which could carry page content) never reaches.
    assert!(
        !verdicts.contains("data-invoice"),
        "verdict detail must not leak browser output: {verdicts}"
    );
}

#[test]
fn validate_org_playbooks_reports_all_valid() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let result = call_tool_in_workspace(&root, "validate_org_playbooks", &json!({})).unwrap();
    assert!(
        result.contains("valid"),
        "preset org playbooks should all validate: {result}"
    );
}

#[test]
fn validate_org_playbooks_flags_unknown_tool() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    // add_org_playbook should reject an unknown tool reference at write time.
    let err = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "broken_pb",
            "name": "Bad reference",
            "role_id": "cfo",
            "resource_id": "banking",
            "capability": "read",
            "description": "refs a ghost tool",
            "steps": [{"tool": "this_tool_does_not_exist", "args": {}}]
        }),
    );
    assert!(err.is_err(), "should reject unknown tool");
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("this_tool_does_not_exist"),
        "error should name the ghost tool: {msg}"
    );
}

/// Connector steps resolve against `.velocity/connectors.json`, a separate
/// namespace — an unconfigured connector id must be rejected at write time.
#[test]
fn add_org_playbook_rejects_unknown_connector_at_write_time() {
    let (_t, root) = setup_root();
    call_tool_in_workspace(&root, "init_company_org", &json!({})).unwrap();
    let err = call_tool_in_workspace(
        &root,
        "add_org_playbook",
        &json!({
            "id": "broken_pb_conn",
            "name": "Bad connector reference",
            "role_id": "cfo",
            "resource_id": "banking",
            "capability": "read",
            "description": "refs a ghost connector",
            "steps": [{"connector": "ghost_svc_no_config", "req": {}}]
        }),
    );
    assert!(err.is_err(), "should reject unknown connector");
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("unknown connector 'ghost_svc_no_config'"),
        "{msg}"
    );
    // The rejected playbook must not have been persisted.
    let listed = call_tool_in_workspace(&root, "list_org_playbooks", &json!({})).unwrap();
    assert!(!listed.contains("broken_pb_conn"), "{listed}");
}
