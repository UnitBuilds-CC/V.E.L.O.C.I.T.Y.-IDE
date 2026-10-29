use crate::registry::types::Tool;
use serde_json::json;

/// MCP tool schemas for the AI-staffed company org (departments, roles,
/// capability grants, approval policies, permission checks and the
/// request/approve pipeline).
pub fn get_company_tools() -> Vec<Tool> {
    // `items` must be a *schema*, not the bare list of allowed values. A lax
    // frontend ignores the malformed `items: ["read", ...]`; llama.cpp's tool
    // grammar compiler validates it and 500s the whole request with
    // "JSON schema error at #/properties/capabilities/items/0: schema must be
    // an object", which fails every turn before the model is ever consulted.
    let capability_enum = json!({
        "type": "string",
        "enum": [
            "read",
            "write",
            "communicate",
            "spend",
            "deploy",
            "approve",
            "manage"
        ]
    });
    vec![
        Tool {
            name: "get_company_org".to_string(),
            description: "Read the company org chart: departments, roles with reporting lines, external resources (with secret references only, never secret values), capability grants and approval policies. Pass full=true for the raw JSON.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "full": { "type": "boolean", "description": "Return the entire org as pretty JSON instead of a summary." }
                }
            }),
        },
        Tool {
            name: "init_company_org".to_string(),
            description: "Seed the workspace with the UnitBuilds CC company org (Executive, Management, Engineering, Testing, Scouting, Marketing, Support, Finance, Legal, Operations) including capability grants and approval policies, persisted to .velocity/company_org.nda. No-op if an org already exists.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "add_org_role".to_string(),
            description: "Add a role/position to the company org chart.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string", "description": "Stable id, e.g. 'devops_lead'." },
                    "title": { "type": "string", "description": "Display title, e.g. 'DevOps Lead'." },
                    "department_id": { "type": "string", "description": "Owning department id (e.g. engineering)." },
                    "reports_to": { "type": "string", "description": "Parent role id, if any." },
                    "tier": { "type": "string", "enum": ["assisted", "standard", "trusted"], "description": "Default autonomy tier." },
                    "agent_team": { "type": "string", "description": "Optional ExpertTeam slug that AI-staffs this role. Empty binds it to the org's own synced team." }
                },
                "required": ["role_id", "title", "department_id"]
            }),
        },
        Tool {
            name: "add_org_resource".to_string(),
            description: "Register an external system the org can act on (GitHub, email, banking, social, cloud, ...). Only a secret *reference* (env-var name / keychain label) is stored — never the secret itself.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "resource_id": { "type": "string", "description": "Stable id, e.g. 'github'." },
                    "name": { "type": "string", "description": "Human-readable name." },
                    "kind": { "type": "string", "enum": ["source_control", "email", "chat", "banking", "social", "cloud", "analytics", "other"], "description": "System category. Drives default sensitivity." },
                    "secret_ref": { "type": "string", "description": "Env var name or keychain label the vault resolves at execution (NOT the secret value)." },
                    "sensitive": { "type": "boolean", "description": "Force sensitivity; defaults true for banking/source_control/cloud/email." }
                },
                "required": ["resource_id", "name"]
            }),
        },
        Tool {
            name: "grant_capability".to_string(),
            description: "Grant a role one or more capabilities over a resource (deny-by-default: without a grant the action is denied). Optionally set a per-action spend limit.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capabilities": { "type": "array", "items": capability_enum.clone() },
                    "spend_limit_per_action": { "type": "number", "description": "Max currency units the role may spend per action before approval. 0 = none." }
                },
                "required": ["role_id", "resource_id", "capabilities"]
            }),
        },
        Tool {
            name: "revoke_capability".to_string(),
            description: "Remove capabilities a role holds over a resource.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capabilities": { "type": "array", "items": capability_enum.clone() }
                },
                "required": ["role_id", "resource_id", "capabilities"]
            }),
        },
        Tool {
            name: "set_approval_policy".to_string(),
            description: "Add or update an approval policy: which capability (+resource, +amount threshold) requires how many approvals from which roles. Empty approvers means the human operator signs off.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Stable policy id." },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "resource_id": { "type": "string", "description": "Omit or empty to apply to every resource." },
                    "threshold": { "type": "number", "description": "Only fire when amount >= threshold. Omit to always fire." },
                    "required_approvers": { "type": "array", "items": { "type": "string" }, "description": "Role ids whose approval counts. Empty = human operator." },
                    "quorum": { "type": "integer", "description": "Approvals needed (min 1)." },
                    "description": { "type": "string" }
                },
                "required": ["id", "capability"]
            }),
        },
        Tool {
            name: "remove_approval_policy".to_string(),
            description: "Delete an approval policy by id.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        Tool {
            name: "check_org_permission".to_string(),
            description: "Deterministically evaluate whether a role may perform a capability on a resource at a given amount. Returns ALLOWED, REQUIRES APPROVAL (approvers+quorum) or DENIED. No LLM inference is used.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Spend total / recipient count / magnitude. Default 0." }
                },
                "required": ["role_id", "resource_id", "capability"]
            }),
        },
        Tool {
            name: "verify_org_credential".to_string(),
            description: "Audit whether a role's credential for a resource actually resolves through the vault, WITHOUT returning the secret. Reports GRANTED (masked preview), NEEDS APPROVAL, DENIED or MISSING (pointer not provisioned). The secret is zeroized immediately after resolution.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Spend total / recipient count / magnitude. Default 0." }
                },
                "required": ["role_id", "resource_id", "capability"]
            }),
        },
        Tool {
            name: "rotate_org_resource_credential".to_string(),
            description: "Rotate a resource's stored `secret_ref` POINTER through the credential-vault gate (Operations key-rotation primitive). Never returns the raw secret. The proposal is routed through check_permission(role, \"credentials\", deploy): ungranted roles get DENIED, the UnitBuilds CC preset wires dual-control (CEO + CTO), so a live rotation returns a PENDING request id to resume via execute_org_credential_rotation_after_approval. Refuses to clear a live pointer (empty new_secret_ref) and cannot target the credentials meta-resource itself.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string", "description": "Initiator; must hold Deploy on the credentials meta-resource." },
                    "resource_id": { "type": "string", "description": "Concrete resource whose pointer is swapped (github/email/banking/cloud/social_x/social_linkedin/chat/analytics). NOT 'credentials' itself." },
                    "new_secret_ref": { "type": "string", "description": "New env-var / keychain pointer label. Empty is refused — never clear a live credential." }
                },
                "required": ["role_id", "resource_id", "new_secret_ref"]
            }),
        },
        Tool {
            name: "execute_org_credential_rotation_after_approval".to_string(),
            description: "Apply a rotation whose PENDING request has already cleared to Approved. Mirrors the gated-playbook resume path: the request id is the proof, the pointer swap is applied, and the ledger entry transitions to Executed so the rotation cannot be replayed. Errors if the request is unknown, still Pending, or was recorded against a different capability/resource.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "description": "The PENDING request id returned by rotate_org_resource_credential." }
                },
                "required": ["request_id"]
            }),
        },
        Tool {
            name: "plan_org_browser_login".to_string(),
            description: "Build a gated, secret-free cross-platform browser plan (navigate/fill/submit login + optional composer post) for an org role. The password references the vault as 'vault:SECRET_REF', never a literal secret. Returns NO plan if the approval gate denies or awaits sign-off.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Magnitude for the gate (spend/count). Default 0." },
                    "login_url": { "type": "string" },
                    "username_selector": { "type": "string" },
                    "username": { "type": "string" },
                    "password_selector": { "type": "string" },
                    "submit_selector": { "type": "string" },
                    "composer_url": { "type": "string", "description": "Optional URL for the outward action (e.g. compose page)." },
                    "composer_selector": { "type": "string", "description": "Optional selector for the post/message field." },
                    "composer_text": { "type": "string", "description": "Optional non-secret marketing copy to post." },
                    "composer_submit_selector": { "type": "string", "description": "Selector of the composer's publish/send control (e.g. X's [data-testid='tweetButton']). When set, a runtime_session_click fires on it after filling; empty falls back to a generic form submit." },
                    "read_script": { "type": "string", "description": "Optional JS evaluated on the logged-in page to extract inbound content (support ticket, chat thread, analytics). Read-only; the returned text is surfaced under EXTRACTED." },
                    "capture_demo": { "type": "boolean", "description": "Snapshot the current page into the browser artifact model (a demo recording for Marketing). Read-only." },
                                        "extra_actions": { "type": "array", "description": "Ordered steps run after login and before read/capture (e.g. click into a specific ticket, open a feature tab, press a key). Each item is {tool, args}; tool is a runtime browser tool such as runtime_session_click / runtime_session_navigate / runtime_session_press_key. sessionId may be omitted and is auto-bound to the live session.", "items": { "type": "object", "properties": { "tool": { "type": "string" }, "args": { "type": "object" } }, "required": ["tool"] } },
                    "settle_ms": { "type": "integer", "minimum": 0, "description": "Post-action settle timeout (ms) applied to every interactive step (navigate/click/fill/submit/evaluate) so async pages render before the next action. 0 = no wait." },
                    "verify_expect": { "type": "string", "description": "Deterministic acceptance check: a required substring the read/extract content MUST contain for the run to VERIFY (PASS/FAIL). No inference: a plain containment assertion the Testing team pins a build against. Empty = no check; pair with read_script." },
                    "verify_absent": { "type": "string", "description": "Deterministic regression guard: a substring that MUST NOT appear in the read/extract content for the run to be clean (ABSENT PASS/FAIL). Catches a broken page (Internal Server Error, undefined, NaN) with no inference. Empty = no check; pair with read_script." },
                                        "next_selector": { "type": "string", "description": "Pagination crawl: selector of the 'next page' control. When set with max_pages > 1, read_script runs once per page with a click on this selector between pages (bounded, deterministic). Empty = single-page read." },
                                        "max_pages": { "type": "integer", "minimum": 0, "description": "Upper bound on pages for the next_selector crawl. 0 or 1 = current page only." },
                                                                                "read_urls": { "type": "array", "items": { "type": "string" }, "description": "Multi-URL read crawl: a bounded list of URLs to visit and evaluate read_script on in one session (each read labelled by its URL). Takes precedence over next_selector. Use for reading several distinct pages, e.g. competitor changelogs or posted announcements." },
                                                                                                    "form_fields": { "type": "array", "items": { "type": "object", "properties": { "selector": { "type": "string" }, "value": { "type": "string" } }, "required": ["selector"] }, "description": "Declarative multi-field form-fill: an ordered list of {selector, value} turned into fill steps after login and before composer/read. Use for campaign/ad-platform and ticket-creation forms with many fields. A value may be a vault:REF pointer (resolved at run time, never a literal secret)." },
                    "wait_for_selector": { "type": "string", "description": "Deterministic wait: poll until this CSS selector is visible before proceeding to form/composer/read. More robust than blind settle_ms on SPAs. Timeout inherits settle_ms (or 5000 ms default). Empty = no wait step." },
                    "scroll_to": { "type": "string", "description": "Scroll the matching element into view after wait_for_selector and before interactive fills/clicks. Needed on long pages (dashboards, feeds) where the target is below the fold. Empty = no scroll step." },
                    "switch_tab": { "type": "boolean", "description": "When true, emit a runtime_session_switch_tab step (position: latest) after wait/scroll — for when a prior click opened a new tab and subsequent steps must target it." },
                    "drag_from": { "type": "string", "description": "Drag-and-drop: source element selector. Must be paired with drag_to (both non-empty) to emit a runtime_session_drag step. Use for kanban card moves, file upload zones, sprint board reordering." },
                    "drag_to": { "type": "string", "description": "Drag-and-drop: target drop-zone selector. Paired with drag_from." },
                    "wait_for_url": { "type": "string", "description": "Wait until the page URL matches this pattern before proceeding (confirms a redirect landed after submit/click). Timeout inherits settle_ms or defaults 8000 ms. Empty = skip." },
                    "right_click_selector": { "type": "string", "description": "Right-click (context menu) on this element before form fill. Empty = skip." },
                    "select_option_selector": { "type": "string", "description": "Dropdown select by visible text: selector of the dropdown trigger. Pair with select_option_text. Empty = skip." },
                    "select_option_text": { "type": "string", "description": "Visible option text to select in the dropdown (e.g. 'Q3 2026'). Paired with select_option_selector." },
                    "form_submit_selector": { "type": "string", "description": "Form submit: selector of the submit button clicked after form_fields/select_option, closing the fill -> submit loop for standalone forms (ticket creation, campaign setup). Distinct from composer_submit_selector (which posts text). Empty = skip." },
                    "wait_for_text": { "type": "string", "description": "Deterministic text-readiness wait: a fragment that MUST appear in the page body before the next step fires. Confirms async content streamed in (node present is not enough). Timeout inherits settle_ms or 6000 ms. Empty = skip." },
                    "hover_selector": { "type": "string", "description": "Hover (mouse-enter) this element to reveal a dropdown/menu/tooltip before a subsequent fill/click/read. Empty = skip." },
                    "upload_selector": { "type": "string", "description": "File upload: the <input type=file> / drop-zone selector to attach files to. Pair with upload_files. Empty = skip." },
                    "upload_files": { "type": "array", "items": { "type": "string" }, "description": "Workspace-relative file paths to upload (e.g. a crash log, a demo image, a recorded artifact). Emitted after text fields and before select/submit." },
                    "extract_attribute_selector": { "type": "string", "description": "Element attribute/text extract: selector of the single element to read. Generates a safe evaluate (attribute, else text/value). Empty = skip." },
                    "extract_attribute_name": { "type": "string", "description": "Attribute to read (e.g. 'href', 'content'); empty reads the element's text/value. Pair with extract_attribute_selector." },
                    "screenshot_selector": { "type": "string", "description": "Per-element screenshot: capture this element/region as visual evidence (a chart, a UI regression, a campaign placement). Emitted near demo capture. Empty = skip." },
                                        "screenshot_full_page": { "type": "boolean", "description": "Full-viewport screenshot: also capture the whole rendered page (audit trail / campaign-placement review). Independent of screenshot_selector; both may fire on the same run. Default false." },
                                                            "frame_selector": { "type": "string", "description": "Iframe/frame entry: selector of the frame to switch the driver into before any other interactive step (banking 3-D-Secure, embedded payment sheets, admin dashboards). Empty = stay on the outer document." },
                                                                                "dialog_action": { "type": "string", "enum": ["", "none", "accept", "dismiss"], "description": "JS dialog auto-handling: clear a native `window.alert/confirm/prompt` modal proactively right after frame entry so subsequent steps do not silently time out. `accept` = OK/Confirm; `dismiss` = Cancel; empty/`none` = skip." },
                                                                                "dialog_prompt_text": { "type": "string", "description": "Optional input value for a `window.prompt()` dialog when dialog_action is `accept`. Ignored for alert/confirm. Empty = no promptText emitted." },
                                                                                "download_selector": { "type": "string", "description": "Download-triggering selector: clicking this element starts a file download (Export CSV, Download artifact, bank statement link). Emits runtime_session_download as the last interactive step. Empty = no download." },
                                                                                "download_dir": { "type": "string", "description": "Directory where the downloaded file is saved. Empty = driver-default temp download folder." },
                                                                                "download_timeout_ms": { "type": "number", "description": "Maximum time (ms) to wait for the download to begin after the triggering click. 0 = driver default." },
                                                                                "wait_for_network_idle": { "type": "boolean", "description": "Wait for network idle after login: block until no XHR/fetch requests are in-flight (SPA dashboards, analytics panels). Uses settle_ms as the upper bound (default 5000ms)." },
                                                                                "request_headers": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Custom HTTP request headers injected before navigation (feature flags, API tokens, tenant isolation). Each pair emits a runtime_session_set_headers step." },
                                                                                                    "cookies": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Session-bootstrap cookies injected before navigation (auth cookie, SSO ticket) so a token-auth system is reachable without a password form. Each pair emits a runtime_session_set_cookie step." },
                                                                                                    "local_storage": { "type": "array", "items": { "type": "object", "properties": { "key": { "type": "string" }, "value": { "type": "string" } }, "required": ["key", "value"] }, "description": "Session-bootstrap localStorage entries injected before navigation (SPA auth token read from local storage). Each pair emits a runtime_session_set_storage step." },
                                                                                                                        "basic_auth_user": { "type": "string", "description": "HTTP basic-auth realm username. With basic_auth_pass, emits a runtime_session_basic_auth step before navigation to authenticate a protected system." },
                                                                                                                        "basic_auth_pass": { "type": "string", "description": "HTTP basic-auth realm password (supply a vault pointer at run time so the plan stays secret-free)." },
                                                                                                                        "user_agent": { "type": "string", "description": "User-agent override for device/compatibility testing. Emits a runtime_session_set_user_agent step before navigation." },
                                                                                                                        "viewport_width": { "type": "number", "description": "Viewport width (px) for responsive/device emulation testing. Emitted with viewport_height as one runtime_session_set_viewport step. 0 = no override." },
                                                                                                                        "viewport_height": { "type": "number", "description": "Viewport height (px) for responsive/device emulation testing. 0 = no override." },
                    "captures": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "selector": { "type": "string" } }, "required": ["name", "selector"] }, "description": "Deterministic value capture: emit one evaluate per pair, storing the element's value/text under `name` in the plan's runtime variable map. Later steps reference it as `{{vars.NAME}}` in any string arg (selector, url, value). Empty list = no capture." }
                },
                "required": ["role_id", "resource_id", "capability", "login_url"]
            }),
        },
        Tool {
            name: "plan_org_browser_broadcast".to_string(),
            description: "Fan a single outward campaign out across many platforms at once (e.g. post to X and LinkedIn). Each platform is gated INDEPENDENTLY against the role's capability grant, so a partial grant yields runnable secret-free plans only for the cleared accounts and a block reason for the rest. Returns a per-platform summary; never leaks a secret.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Magnitude for the gate. Default 0." },
                    "platforms": {
                        "type": "array",
                        "description": "One object per platform: {resource_id, login_url, username_selector?, username?, password_selector?, submit_selector?, composer_url?, composer_selector?, composer_text?, composer_submit_selector?, settle_ms?}.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "resource_id": { "type": "string" },
                                "login_url": { "type": "string" },
                                "username_selector": { "type": "string" },
                                "username": { "type": "string" },
                                "password_selector": { "type": "string" },
                                "submit_selector": { "type": "string" },
                                "composer_url": { "type": "string" },
                                "composer_selector": { "type": "string" },
                                "composer_text": { "type": "string" },
                                "composer_submit_selector": { "type": "string" },
                                "settle_ms": { "type": "integer", "minimum": 0 }
                            },
                            "required": ["resource_id"]
                        }
                    }
                },
                "required": ["role_id", "capability", "platforms"]
            }),
        },
        Tool {
            name: "execute_org_browser_broadcast".to_string(),
            description: "Fan an outward campaign across many platforms and actually drive each cleared account against the live browser engine. Each platform is gated independently: give a platform a request_id to post only once that action request is Approved (counsel-gated comms, request advanced to Executed), or omit it to post only when the stateless permission matrix returns Allowed. Blocked accounts are reported, never leaked. Pass dry_run=true to preview per-platform steps without touching the browser or resolving any secret. Returns a per-platform summary; never leaks a secret.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Magnitude for the gate. Default 0." },
                    "dry_run": { "type": "boolean", "description": "Preview per-platform steps; no browser, no secret. Default false." },
                    "platforms": {
                        "type": "array",
                        "description": "One object per platform: {resource_id, login_url, username_selector?, username?, password_selector?, submit_selector?, composer_url?, composer_selector?, composer_text?, composer_submit_selector?, settle_ms?, request_id?}. A request_id makes that account's approved action request the execution gate.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "resource_id": { "type": "string" },
                                "login_url": { "type": "string" },
                                "username_selector": { "type": "string" },
                                "username": { "type": "string" },
                                "password_selector": { "type": "string" },
                                "submit_selector": { "type": "string" },
                                "composer_url": { "type": "string" },
                                "composer_selector": { "type": "string" },
                                "composer_text": { "type": "string" },
                                "composer_submit_selector": { "type": "string" },
                                "settle_ms": { "type": "integer", "minimum": 0 },
                                "request_id": { "type": "string", "description": "Optional approved action-request id gating this account's post." }
                            },
                            "required": ["resource_id"]
                        }
                    }
                },
                "required": ["role_id", "capability", "platforms"]
            }),
        },
        Tool {
            name: "execute_org_browser_task".to_string(),
            description: "Run a gated cross-platform browser task for an org role against the live browser engine: gate the action, resolve the credential from the vault at execution (never echoed), open a session and replay the login + optional composer steps. Pass dry_run=true to preview the substituted step list without touching the browser or resolving any secret. Returns a per-step log only if the approval gate clears.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Magnitude for the gate (spend/count). Default 0." },
                    "dry_run": { "type": "boolean", "description": "Preview the plan without executing or resolving secrets. Default false." },
                    "login_url": { "type": "string" },
                    "username_selector": { "type": "string" },
                    "username": { "type": "string" },
                    "password_selector": { "type": "string" },
                    "submit_selector": { "type": "string" },
                    "composer_url": { "type": "string", "description": "Optional URL for the outward action (e.g. compose page)." },
                    "composer_selector": { "type": "string", "description": "Optional selector for the post/message field." },
                    "composer_text": { "type": "string", "description": "Optional non-secret marketing copy to post." },
                    "composer_submit_selector": { "type": "string", "description": "Selector of the composer's publish/send control (e.g. X's [data-testid='tweetButton']). When set, a runtime_session_click fires on it after filling; empty falls back to a generic form submit." },
                    "read_script": { "type": "string", "description": "Optional JS evaluated on the logged-in page to extract inbound content (support ticket, chat thread, analytics). Read-only; the returned text is surfaced under EXTRACTED." },
                    "capture_demo": { "type": "boolean", "description": "Snapshot the current page into the browser artifact model (a demo recording for Marketing). Read-only." },
                                        "extra_actions": { "type": "array", "description": "Ordered steps run after login and before read/capture (e.g. click into a specific ticket, open a feature tab, press a key). Each item is {tool, args}; tool is a runtime browser tool such as runtime_session_click / runtime_session_navigate / runtime_session_press_key. sessionId may be omitted and is auto-bound to the live session.", "items": { "type": "object", "properties": { "tool": { "type": "string" }, "args": { "type": "object" } }, "required": ["tool"] } },
                    "settle_ms": { "type": "integer", "minimum": 0, "description": "Post-action settle timeout (ms) applied to every interactive step (navigate/click/fill/submit/evaluate) so async pages render before the next action. 0 = no wait." },
                    "verify_expect": { "type": "string", "description": "Deterministic acceptance check: a required substring the read/extract content MUST contain for the run to VERIFY (PASS/FAIL). No inference: a plain containment assertion the Testing team pins a build against. Empty = no check; pair with read_script." },
                    "verify_absent": { "type": "string", "description": "Deterministic regression guard: a substring that MUST NOT appear in the read/extract content for the run to be clean (ABSENT PASS/FAIL). Catches a broken page (Internal Server Error, undefined, NaN) with no inference. Empty = no check; pair with read_script." },
                                        "next_selector": { "type": "string", "description": "Pagination crawl: selector of the 'next page' control. When set with max_pages > 1, read_script runs once per page with a click on this selector between pages (bounded, deterministic). Empty = single-page read." },
                                        "max_pages": { "type": "integer", "minimum": 0, "description": "Upper bound on pages for the next_selector crawl. 0 or 1 = current page only." },
                                                                                "read_urls": { "type": "array", "items": { "type": "string" }, "description": "Multi-URL read crawl: a bounded list of URLs to visit and evaluate read_script on in one session (each read labelled by its URL). Takes precedence over next_selector. Use for reading several distinct pages, e.g. competitor changelogs or posted announcements." },
                                                                                                    "form_fields": { "type": "array", "items": { "type": "object", "properties": { "selector": { "type": "string" }, "value": { "type": "string" } }, "required": ["selector"] }, "description": "Declarative multi-field form-fill: an ordered list of {selector, value} turned into fill steps after login and before composer/read. Use for campaign/ad-platform and ticket-creation forms with many fields. A value may be a vault:REF pointer (resolved at run time, never a literal secret)." },
                                        "wait_for_selector": { "type": "string", "description": "Deterministic wait: poll until this CSS selector is visible before proceeding to form/composer/read. More robust than blind settle_ms on SPAs. Timeout inherits settle_ms (or 5000 ms default). Empty = no wait step." },
                                        "scroll_to": { "type": "string", "description": "Scroll the matching element into view after wait_for_selector and before interactive fills/clicks. Needed on long pages (dashboards, feeds) where the target is below the fold. Empty = no scroll step." },
                                        "switch_tab": { "type": "boolean", "description": "When true, emit a runtime_session_switch_tab step (position: latest) after wait/scroll." },
                                        "drag_from": { "type": "string", "description": "Drag-and-drop: source element selector. Pair with drag_to." },
                                        "drag_to": { "type": "string", "description": "Drag-and-drop: target drop-zone selector. Pair with drag_from." },
                                        "wait_for_url": { "type": "string", "description": "Wait until page URL matches this pattern before proceeding. Empty = skip." },
                                        "right_click_selector": { "type": "string", "description": "Right-click (context menu) on this element. Empty = skip." },
                                        "select_option_selector": { "type": "string", "description": "Dropdown selector for select-by-text. Pair with select_option_text." },
                                        "select_option_text": { "type": "string", "description": "Visible option text to select." },
                                        "form_submit_selector": { "type": "string", "description": "Form submit: selector of the submit button clicked after form_fields/select_option (fill -> submit loop for standalone forms). Distinct from composer_submit_selector. Empty = skip." },
                                        "wait_for_text": { "type": "string", "description": "Wait until this text fragment appears in the page body before proceeding (async content readiness). Empty = skip." },
                                        "hover_selector": { "type": "string", "description": "Hover (mouse-enter) this element to reveal a dropdown/menu/tooltip before a subsequent fill/click/read. Empty = skip." },
                                        "upload_selector": { "type": "string", "description": "File upload: the <input type=file> / drop-zone selector to attach files to. Pair with upload_files. Empty = skip." },
                                        "upload_files": { "type": "array", "items": { "type": "string" }, "description": "Workspace-relative file paths to upload (e.g. a crash log, a demo image, a recorded artifact). Emitted after text fields and before select/submit." },
                                        "extract_attribute_selector": { "type": "string", "description": "Element attribute/text extract: selector of the single element to read. Generates a safe evaluate (attribute, else text/value). Empty = skip." },
                                        "extract_attribute_name": { "type": "string", "description": "Attribute to read (e.g. 'href', 'content'); empty reads the element's text/value. Pair with extract_attribute_selector." },
                                        "screenshot_selector": { "type": "string", "description": "Per-element screenshot: capture this element/region as visual evidence (a chart, a UI regression, a campaign placement). Emitted near demo capture. Empty = skip." },
                                                            "screenshot_full_page": { "type": "boolean", "description": "Full-viewport screenshot: also capture the whole rendered page (audit trail / campaign-placement review). Independent of screenshot_selector; both may fire on the same run. Default false." },
                                                                                "frame_selector": { "type": "string", "description": "Iframe/frame entry: selector of the frame to switch the driver into before any other interactive step (banking 3-D-Secure, embedded payment sheets, admin dashboards). Empty = stay on the outer document." },
                                                                                                    "dialog_action": { "type": "string", "enum": ["", "none", "accept", "dismiss"], "description": "JS dialog auto-handling: clear a native `window.alert/confirm/prompt` modal proactively right after frame entry so subsequent steps do not silently time out. `accept` = OK/Confirm; `dismiss` = Cancel; empty/`none` = skip." },
                                                                                                    "dialog_prompt_text": { "type": "string", "description": "Optional input value for a `window.prompt()` dialog when dialog_action is `accept`. Ignored for alert/confirm. Empty = no promptText emitted." },
                                                                                                    "download_selector": { "type": "string", "description": "Download-triggering selector: clicking this element starts a file download (Export CSV, Download artifact, bank statement link). Emits runtime_session_download as the last interactive step. Empty = no download." },
                                                                                                    "download_dir": { "type": "string", "description": "Directory where the downloaded file is saved. Empty = driver-default temp download folder." },
                                                                                                    "download_timeout_ms": { "type": "number", "description": "Maximum time (ms) to wait for the download to begin after the triggering click. 0 = driver default." },
                                                                                                    "wait_for_network_idle": { "type": "boolean", "description": "Wait for network idle after login: block until no XHR/fetch requests are in-flight (SPA dashboards, analytics panels). Uses settle_ms as the upper bound (default 5000ms)." },
                                                                                                    "request_headers": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Custom HTTP request headers injected before navigation (feature flags, API tokens, tenant isolation). Each pair emits a runtime_session_set_headers step." },
                                                                                                                        "cookies": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Session-bootstrap cookies injected before navigation (auth cookie, SSO ticket) so a token-auth system is reachable without a password form. Each pair emits a runtime_session_set_cookie step." },
                                                                                                                        "local_storage": { "type": "array", "items": { "type": "object", "properties": { "key": { "type": "string" }, "value": { "type": "string" } }, "required": ["key", "value"] }, "description": "Session-bootstrap localStorage entries injected before navigation (SPA auth token read from local storage). Each pair emits a runtime_session_set_storage step." },
                                                                                                                                            "basic_auth_user": { "type": "string", "description": "HTTP basic-auth realm username. With basic_auth_pass, emits a runtime_session_basic_auth step before navigation to authenticate a protected system." },
                                                                                                                                            "basic_auth_pass": { "type": "string", "description": "HTTP basic-auth realm password (supply a vault pointer at run time so the plan stays secret-free)." },
                                                                                                                                            "user_agent": { "type": "string", "description": "User-agent override for device/compatibility testing. Emits a runtime_session_set_user_agent step before navigation." },
                                                                                                                                            "viewport_width": { "type": "number", "description": "Viewport width (px) for responsive/device emulation testing. Emitted with viewport_height as one runtime_session_set_viewport step. 0 = no override." },
                                                                                                                                            "viewport_height": { "type": "number", "description": "Viewport height (px) for responsive/device emulation testing. 0 = no override." },
                                        "captures": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "selector": { "type": "string" } }, "required": ["name", "selector"] }, "description": "Deterministic value capture: emit one evaluate per pair, storing the element's value/text under `name` in the plan's runtime variable map. Later steps reference it as `{{vars.NAME}}` in any string arg (selector, url, value). Empty list = no capture." }
                },
                "required": ["role_id", "resource_id", "capability", "login_url"]
            }),
        },
        Tool {
            name: "execute_org_browser_task_for_request".to_string(),
            description: "Run a browser task on behalf of an ALREADY-APPROVED action request (by request_id). Where execute_org_browser_task gates on the stateless permission matrix, this gates on the request: it must exist and be Approved, otherwise nothing runs and the block reason is returned. This is how a gated outward action (e.g. a marketing post awaiting counsel sign-off) actually executes once cleared. The secret is resolved only on a live run (never echoed); dry_run=true previews the substituted step list without touching the browser or resolving a secret. A live run marks the request Executed so it cannot be replayed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "description": "An Approved request id (from request_org_action / approve_org_action)." },
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number", "description": "Magnitude for the gate (spend/count). Default 0." },
                    "dry_run": { "type": "boolean", "description": "Preview the plan without executing or resolving secrets. Default false." },
                    "login_url": { "type": "string" },
                    "username_selector": { "type": "string" },
                    "username": { "type": "string" },
                    "password_selector": { "type": "string" },
                    "submit_selector": { "type": "string" },
                    "composer_url": { "type": "string", "description": "Optional URL for the outward action (e.g. compose page)." },
                    "composer_selector": { "type": "string", "description": "Optional selector for the post/message field." },
                    "composer_text": { "type": "string", "description": "Optional non-secret marketing copy to post." },
                    "composer_submit_selector": { "type": "string", "description": "Selector of the composer's publish/send control (e.g. X's [data-testid='tweetButton']). When set, a runtime_session_click fires on it after filling; empty falls back to a generic form submit." },
                    "read_script": { "type": "string", "description": "Optional JS evaluated on the logged-in page to extract inbound content (support ticket, chat thread, analytics). Read-only; the returned text is surfaced under EXTRACTED." },
                    "capture_demo": { "type": "boolean", "description": "Snapshot the current page into the browser artifact model (a demo recording for Marketing). Read-only." },
                                        "extra_actions": { "type": "array", "description": "Ordered steps run after login and before read/capture (e.g. click into a specific ticket, open a feature tab, press a key). Each item is {tool, args}; tool is a runtime browser tool such as runtime_session_click / runtime_session_navigate / runtime_session_press_key. sessionId may be omitted and is auto-bound to the live session.", "items": { "type": "object", "properties": { "tool": { "type": "string" }, "args": { "type": "object" } }, "required": ["tool"] } },
                    "settle_ms": { "type": "integer", "minimum": 0, "description": "Post-action settle timeout (ms) applied to every interactive step (navigate/click/fill/submit/evaluate) so async pages render before the next action. 0 = no wait." },
                    "verify_expect": { "type": "string", "description": "Deterministic acceptance check: a required substring the read/extract content MUST contain for the run to VERIFY (PASS/FAIL). No inference: a plain containment assertion the Testing team pins a build against. Empty = no check; pair with read_script." },
                    "verify_absent": { "type": "string", "description": "Deterministic regression guard: a substring that MUST NOT appear in the read/extract content for the run to be clean (ABSENT PASS/FAIL). Catches a broken page (Internal Server Error, undefined, NaN) with no inference. Empty = no check; pair with read_script." },
                                        "next_selector": { "type": "string", "description": "Pagination crawl: selector of the 'next page' control. When set with max_pages > 1, read_script runs once per page with a click on this selector between pages (bounded, deterministic). Empty = single-page read." },
                                        "max_pages": { "type": "integer", "minimum": 0, "description": "Upper bound on pages for the next_selector crawl. 0 or 1 = current page only." },
                                                                                "read_urls": { "type": "array", "items": { "type": "string" }, "description": "Multi-URL read crawl: a bounded list of URLs to visit and evaluate read_script on in one session (each read labelled by its URL). Takes precedence over next_selector. Use for reading several distinct pages, e.g. competitor changelogs or posted announcements." },
                                                                                                    "form_fields": { "type": "array", "items": { "type": "object", "properties": { "selector": { "type": "string" }, "value": { "type": "string" } }, "required": ["selector"] }, "description": "Declarative multi-field form-fill: an ordered list of {selector, value} turned into fill steps after login and before composer/read. Use for campaign/ad-platform and ticket-creation forms with many fields. A value may be a vault:REF pointer (resolved at run time, never a literal secret)." },
                                        "wait_for_selector": { "type": "string", "description": "Deterministic wait: poll until this CSS selector is visible before proceeding to form/composer/read. More robust than blind settle_ms on SPAs. Timeout inherits settle_ms (or 5000 ms default). Empty = no wait step." },
                                        "scroll_to": { "type": "string", "description": "Scroll the matching element into view after wait_for_selector and before interactive fills/clicks. Needed on long pages (dashboards, feeds) where the target is below the fold. Empty = no scroll step." },
                                        "switch_tab": { "type": "boolean", "description": "When true, emit a runtime_session_switch_tab step (position: latest) after wait/scroll." },
                                        "drag_from": { "type": "string", "description": "Drag-and-drop: source element selector. Pair with drag_to." },
                                        "drag_to": { "type": "string", "description": "Drag-and-drop: target drop-zone selector. Pair with drag_from." },
                                        "wait_for_url": { "type": "string", "description": "Wait until page URL matches this pattern before proceeding. Empty = skip." },
                                        "right_click_selector": { "type": "string", "description": "Right-click (context menu) on this element. Empty = skip." },
                                        "select_option_selector": { "type": "string", "description": "Dropdown selector for select-by-text. Pair with select_option_text." },
                                        "select_option_text": { "type": "string", "description": "Visible option text to select." },
"form_submit_selector": { "type": "string", "description": "Form submit: selector of the submit button clicked after form_fields/select_option (fill -> submit loop for standalone forms). Distinct from composer_submit_selector. Empty = skip." },
"wait_for_text": { "type": "string", "description": "Wait until this text fragment appears in the page body before proceeding (async content readiness). Empty = skip." },
"hover_selector": { "type": "string", "description": "Hover (mouse-enter) this element to reveal a dropdown/menu/tooltip before a subsequent fill/click/read. Empty = skip." },
"upload_selector": { "type": "string", "description": "File upload: the <input type=file> / drop-zone selector to attach files to. Pair with upload_files. Empty = skip." },
"upload_files": { "type": "array", "items": { "type": "string" }, "description": "Workspace-relative file paths to upload (e.g. a crash log, a demo image, a recorded artifact). Emitted after text fields and before select/submit." },
"extract_attribute_selector": { "type": "string", "description": "Element attribute/text extract: selector of the single element to read. Generates a safe evaluate (attribute, else text/value). Empty = skip." },
"extract_attribute_name": { "type": "string", "description": "Attribute to read (e.g. 'href', 'content'); empty reads the element's text/value. Pair with extract_attribute_selector." },
"screenshot_selector": { "type": "string", "description": "Per-element screenshot: capture this element/region as visual evidence (a chart, a UI regression, a campaign placement). Emitted near demo capture. Empty = skip." },
                    "screenshot_full_page": { "type": "boolean", "description": "Full-viewport screenshot: also capture the whole rendered page (audit trail / campaign-placement review). Independent of screenshot_selector; both may fire on the same run. Default false." },
                                        "frame_selector": { "type": "string", "description": "Iframe/frame entry: selector of the frame to switch the driver into before any other interactive step (banking 3-D-Secure, embedded payment sheets, admin dashboards). Empty = stay on the outer document." },
                                                            "dialog_action": { "type": "string", "enum": ["", "none", "accept", "dismiss"], "description": "JS dialog auto-handling: clear a native `window.alert/confirm/prompt` modal proactively right after frame entry so subsequent steps do not silently time out. `accept` = OK/Confirm; `dismiss` = Cancel; empty/`none` = skip." },
                                                            "dialog_prompt_text": { "type": "string", "description": "Optional input value for a `window.prompt()` dialog when dialog_action is `accept`. Ignored for alert/confirm. Empty = no promptText emitted." },
                                                            "download_selector": { "type": "string", "description": "Download-triggering selector: clicking this element starts a file download (Export CSV, Download artifact, bank statement link). Emits runtime_session_download as the last interactive step. Empty = no download." },
                                                            "download_dir": { "type": "string", "description": "Directory where the downloaded file is saved. Empty = driver-default temp download folder." },
                                                            "download_timeout_ms": { "type": "number", "description": "Maximum time (ms) to wait for the download to begin after the triggering click. 0 = driver default." },
                                                            "wait_for_network_idle": { "type": "boolean", "description": "Wait for network idle after login: block until no XHR/fetch requests are in-flight (SPA dashboards, analytics panels). Uses settle_ms as the upper bound (default 5000ms)." },
                                                            "request_headers": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Custom HTTP request headers injected before navigation (feature flags, API tokens, tenant isolation). Each pair emits a runtime_session_set_headers step." },
                                                                                "cookies": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"] }, "description": "Session-bootstrap cookies injected before navigation (auth cookie, SSO ticket) so a token-auth system is reachable without a password form. Each pair emits a runtime_session_set_cookie step." },
                                                                                "local_storage": { "type": "array", "items": { "type": "object", "properties": { "key": { "type": "string" }, "value": { "type": "string" } }, "required": ["key", "value"] }, "description": "Session-bootstrap localStorage entries injected before navigation (SPA auth token read from local storage). Each pair emits a runtime_session_set_storage step." },
                                                                                                    "basic_auth_user": { "type": "string", "description": "HTTP basic-auth realm username. With basic_auth_pass, emits a runtime_session_basic_auth step before navigation to authenticate a protected system." },
                                                                                                    "basic_auth_pass": { "type": "string", "description": "HTTP basic-auth realm password (supply a vault pointer at run time so the plan stays secret-free)." },
                                                                                                    "user_agent": { "type": "string", "description": "User-agent override for device/compatibility testing. Emits a runtime_session_set_user_agent step before navigation." },
                                                                                                    "viewport_width": { "type": "number", "description": "Viewport width (px) for responsive/device emulation testing. Emitted with viewport_height as one runtime_session_set_viewport step. 0 = no override." },
                                                                                                    "viewport_height": { "type": "number", "description": "Viewport height (px) for responsive/device emulation testing. 0 = no override." },
"captures": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "selector": { "type": "string" } }, "required": ["name", "selector"] }, "description": "Deterministic value capture: emit one evaluate per pair, storing the element's value/text under `name` in the plan's runtime variable map. Later steps reference it as `{{vars.NAME}}` in any string arg (selector, url, value). Empty list = no capture." }
                },
                "required": ["request_id", "role_id", "resource_id", "capability", "login_url"]
            }),
        },
        Tool {
            name: "request_org_action".to_string(),
            description: "Propose an action and record it in the approval ledger. Auto-approved if permission allows; otherwise queued Pending for the required approvers.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] },
                    "amount": { "type": "number" },
                    "description": { "type": "string", "description": "What will be done, for the audit trail." }
                },
                "required": ["role_id", "resource_id", "capability"]
            }),
        },
        Tool {
            name: "approve_org_action".to_string(),
            description: "Record an approval from a designated approver on a pending request; advances to Approved once quorum is reached. Approver role must hold Approve authority on the resource.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string" },
                    "approver_id": { "type": "string", "description": "Approving role id, or 'human-operator'." }
                },
                "required": ["request_id", "approver_id"]
            }),
        },
        Tool {
            name: "approve_and_resume_org_action".to_string(),
            description: "Sign a pending request and, if that signature reaches quorum, immediately run the playbook whose steps it was gating (the one-click form of approve_org_action + resume_org_playbook). Below quorum nothing runs; a request not raised by a playbook is approved without stored steps to execute.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string" },
                    "approver_id": { "type": "string", "description": "Approving role id, or 'human-operator'." }
                },
                "required": ["request_id", "approver_id"]
            }),
        },
        Tool {
            name: "reject_org_action".to_string(),
            description: "Reject a pending action request.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string" },
                    "by": { "type": "string", "description": "Who is rejecting (role id or human-operator)." },
                    "reason": { "type": "string" }
                },
                "required": ["request_id"]
            }),
        },
        Tool {
            name: "list_org_actions".to_string(),
            description: "List action requests, optionally filtered by status (pending|approved|rejected|executed).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["pending", "approved", "rejected", "executed"] }
                }
            }),
        },
        Tool {
            name: "list_org_escalations".to_string(),
            description: "Deterministic escalation routing for the approval pipeline. With 'approver_id', lists exactly the pending actions that approver is designated to clear (their queue), each annotated with its chain context: the playbook a signature runs, the upstream hop that queued it, and the duty waiting behind its own gate. Without it, renders the whole-org roster of who still owes a decision on what. No inference: a plain ledger query, secret-free, for surfacing pending approvals to the right human/manager.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "approver_id": { "type": "string", "description": "Role id (or 'human-operator') whose designated pending approvals to list. Omit for the whole-org roster." }
                }
            }),
        },
        Tool {
            name: "add_org_playbook".to_string(),
            description: "Add or update a deterministic playbook owned by a role. Running it first submits the (role, resource, capability, amount) intent to the approval engine; the steps execute ONLY if that action clears. Steps reuse the workflow engine so repeatable operations run without LLM inference (inference happens only at an 'agent' step).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Stable playbook id." },
                    "name": { "type": "string", "description": "Display name (defaults to id)." },
                    "role_id": { "type": "string", "description": "Owning role id." },
                    "resource_id": { "type": "string", "description": "Resource the gated action targets." },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"], "description": "Capability the action requires (drives the gate)." },
                    "amount": { "type": "number", "description": "Spend total / magnitude for threshold policies. Default 0." },
                    "description": { "type": "string", "description": "What the playbook does, for the audit trail." },
                    "handoff_on_success": { "type": "string", "description": "Optional: id of an existing playbook to queue automatically when this one finishes clean. The downstream still runs through its OWN gate — chaining grants no authority." },
                    "steps": {
                        "type": "array",
                        "description": "Ordered steps. Each is {tool, args?} | {agent, team?} | {connector, req?} | {condition}.",
                        "items": { "type": "object" }
                    }
                },
                "required": ["id", "role_id", "resource_id", "capability", "steps"]
            }),
        },
        Tool {
            name: "remove_org_playbook".to_string(),
            description: "Delete a playbook by id.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        Tool {
            name: "list_org_playbooks".to_string(),
            description: "List the org's playbooks with their gate (role, capability, resource).".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "validate_org_playbooks".to_string(),
            description: "Validate all playbooks: check every Tool step references a registered tool. Reports issues or confirms all valid.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "run_org_playbook".to_string(),
            description: "Run a playbook: evaluate its permission gate, and if cleared execute the deterministic steps. If gated, no steps run and the pending request is returned for approval.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        Tool {
            name: "resume_org_playbook".to_string(),
            description: "Resume a gated playbook whose approval request has cleared. Takes the request_id returned by a prior gated run_org_playbook. Runs the stored deterministic steps ONLY if that request is now Approved, threading {{requestId}} into outward-action browser steps (so a gated marketing post can finally publish via the request-gated executor) and marking the request Executed so it cannot be replayed. A still-pending or rejected request runs no step and returns the block reason.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "request_id": { "type": "string", "description": "The pending request id from a gated run_org_playbook, now advanced to Approved." } },
                "required": ["request_id"]
            }),
        },
        Tool {
            name: "sync_org_team".to_string(),
            description: "Project the company org chart into Velocity's expert-team roster so the AI company becomes runnable. Each role becomes a team member with a persona drawn from its title, department charter and capability grants. Upserts the org's team by slug (e.g. 'unitbuilds-cc') without disturbing other teams. Idempotent.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "dispatch_org_role".to_string(),
            description: "Route a natural-language task to the right org role and hand it to the synced expert team for execution. Routing and team sync are deterministic; only the member's work is inference. Outward/sensitive actions remain gated by the org approval engine at execution time. Pass role_id to force a role, or omit for keyword routing; pass files[] to help scope-based routing.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The work to perform, e.g. 'ask the CFO to review Q3 margins'." },
                    "role_id": { "type": "string", "description": "Optional: force this role to handle the task." },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Optional: files the task touches, used for scope-based routing." }
                },
                "required": ["task"]
            }),
        },
        Tool {
            name: "grant_org_credential_elevation".to_string(),
            description: "Mint a single-use, TTL-bounded just-in-time credential elevation from an ALREADY-APPROVED action request. The elevation's scope (role, resource, capability) is copied verbatim from the request, so an agent can never mint broader access than what the quorum cleared. Auto-expires on the TTL (clamped 1..=3600s) and authorizes exactly one execution. Records a Granted line in the secret-free audit ledger on success, or a Denied line if the request is missing or not Approved. This replaces standing high-value grants: an irreversible banking Spend / cloud Deploy / credential rotation is enabled for one run, then gone.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "description": "An Approved request id (from request_org_action / a gated playbook run / rotate_org_resource_credential)." },
                    "ttl_secs": { "type": "integer", "minimum": 1, "maximum": 3600, "description": "How long the elevation stays active, in seconds. Clamped to [1, 3600]." }
                },
                "required": ["request_id", "ttl_secs"]
            }),
        },
        Tool {
            name: "elevate_and_resume_org_request".to_string(),
            description: "Complete an approved irreversible action: mint the single-use TTL-bounded credential elevation its scope requires, then run the playbook waiting on that request (the elevation is consumed by the run, so one call authorizes exactly one execution). Refuses any request that is not Approved. For a capability that needs no elevation this simply resumes the playbook. ttl_secs defaults to 300.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "description": "An Approved request id awaiting elevation." },
                    "ttl_secs": { "type": "integer", "minimum": 1, "maximum": 3600, "description": "Elevation window in seconds (clamped 1..=3600)." }
                },
                "required": ["request_id"]
            }),
        },
        Tool {
            name: "check_org_credential_elevation".to_string(),
            description: "Query whether an active (unexpired, unconsumed) just-in-time elevation exists for a role's (resource, capability) right now. This is the deterministic gate an elevated action consults before releasing a secret — a plain ledger query, no inference. Returns the grant id and expiry when active, or NONE when the role must fall back to standing permissions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role_id": { "type": "string" },
                    "resource_id": { "type": "string" },
                    "capability": { "type": "string", "enum": ["read","write","communicate","spend","deploy","approve","manage"] }
                },
                "required": ["role_id", "resource_id", "capability"]
            }),
        },
        Tool {
            name: "consume_org_credential_elevation".to_string(),
            description: "Spend an active elevation to authorize exactly one execution. Errors (and records Expired/Denied in the ledger) if the grant is past its TTL or already consumed, so a minted elevation can never be replayed. Returns the consumed grant's scope. Records a Used line on success.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "grant_id": { "type": "string", "description": "The elevation id returned by grant_org_credential_elevation (format 'elev_<request_id>')." }
                },
                "required": ["grant_id"]
            }),
        },
        Tool {
            name: "list_org_credential_audit".to_string(),
            description: "Read the append-only, secret-free credential-use audit ledger: who elevated or used what capability on which resource, when, and the outcome (Granted/Used/Denied/Expired). Never stores a credential value — only the pointer-level facts. Returns the most recent N entries (newest last) for compliance review.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "description": "Max most-recent entries to return. Default 20." }
                }
            }),
        },
        Tool {
            name: "list_org_playbook_verdicts".to_string(),
            description: "Cross-SOP verdict memory: the recent playbook runs that HALTED on a verdict gate (e.g. a browser VERIFY: FAIL, an elevation-gated spend refused, a campaign that never landed) or FAILED mid-run, as a secret-free escalation briefing — playbook, cleared request, the step that stopped it and the engine's own explanation. A halted SOP is a stuck process approvers must see; clean successes record nothing.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "description": "Max most-recent stuck runs to return. Default 20." }
                }
            }),
        },
        Tool {
            name: "chain_org_playbooks".to_string(),
            description: "Wire a deterministic cross-SOP handoff: when `upstream_id` finishes CLEAN (every step ok), the downstream playbook's intent is queued automatically. Chaining routes WORK, never authority — the downstream SOP is submitted under its own role/capability/resource gate, so a gated hop halts at 'awaiting approval' for its own approvers and a denied hop runs nothing. A downstream that halted mid-run hands off nothing, so a half-done duty never springs the next department. Omit `downstream_id` (or pass \"\") to clear the edge. Refuses unknown ids and self-handoffs; chains are walked to a max depth of 8 so a cycle terminates with a recorded refusal.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "upstream_id": { "type": "string", "description": "Playbook that, on a clean run, hands off (e.g. 'ship_github_patch')." },
                    "downstream_id": { "type": "string", "description": "Playbook to queue next (e.g. 'verify_staging_build'). Empty or absent clears the upstream's existing edge." }
                },
                "required": ["upstream_id"]
            }),
        },
        Tool {
            name: "list_org_sop_handoffs".to_string(),
            description: "The cross-SOP chain ledger: every recorded hop (which SOP handed off to which, at what depth, under which requests) with the downstream gate's own verdict — ran, awaiting-approval, denied, not-found or depth-limit. Secret-free by construction: ids and outcome labels only, never step output. Use it to see whether an upstream SOP actually passed its work to the next department, or whether a hop is sitting in someone's approval queue.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "description": "Max most-recent hops to return, newest last. Default 20." }
                }
            }),
        },
        Tool {
            name: "schedule_org_playbook".to_string(),
            description: "Arm a recurring cadence on an existing playbook (the org's self-starting duties: uptime sweeps, staging verification, weekly reviews). Arming grants NOTHING — when the cadence falls due it runs through the same approval gate as a manual run_org_playbook, so a scheduled spend or outward post queues an approval request instead of acting on its own. Re-arming replaces the previous interval. Refuses playbooks that don't exist and intervals under 60s.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Playbook id to run repeatedly (must already exist in the org)." },
                    "every_minutes": { "type": "integer", "minimum": 1, "description": "Cadence. Default 60 (hourly); 1440 for daily." }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "unschedule_org_playbook".to_string(),
            description: "Retire a playbook's recurring cadence. The playbook itself stays defined and manually runnable.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Playbook id whose cadence should be removed." }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "pause_org_playbook_schedule".to_string(),
            description: "Pause (default) or resume an armed cadence without disturbing its interval or due clock. A paused duty is skipped by every tick; resuming leaves it on its original schedule rather than dumping a backlog.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Playbook id whose cadence should change state." },
                    "paused": { "type": "boolean", "description": "true to pause (default), false to resume." }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "list_org_schedules".to_string(),
            description: "The org's recurring duties: each armed cadence with its interval, whether it is due/overdue or when it next fires, last run and armed/PAUSED state. Secret-free — ids, intervals and timestamps only.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "run_due_org_playbooks".to_string(),
            description: "The scheduler tick: fire every due cadence through the gated playbook runner and advance each clock by exactly one interval, so a backfill after downtime runs a duty once (never N times) and a halting SOP waits a full interval instead of hammering the queue. Pass `at` (unix seconds) for a reproducible or catch-up tick; omit it to tick on wall-clock time.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "at": { "type": "integer", "minimum": 0, "description": "Optional explicit clock (unix seconds) to evaluate due cadences against." }
                }
            }),
        },
    ]
}
