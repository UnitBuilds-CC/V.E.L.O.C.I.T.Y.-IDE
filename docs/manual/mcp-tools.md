# MCP Tool Reference

Every Model-Context-Protocol tool the IDE exposes to agents, grouped by the registry module that defines it. Descriptions are taken verbatim from the tool definitions in `velocity-mcp/src/registry/tool_definitions/`, so this table matches the running binary. **Total: 343 tools.**

## Categories

| Category | Tools | Contents |
|----------|------:|----------|
| [browser](#browser) | 109 | Native browser-engine and web-automation tools: navigation, crawling, AOM/DOM inspection, forms, auth profiles, runtime capture, and visual artifacts. |
| [company](#company) | 44 |  |
| [drone](#drone) | 10 | Distributed build-node (drone) tools: register, ping, remote build/run, and peer file operations. |
| [generation](#generation) | 5 | Code generation tools driven by the agent. |
| [instances](#instances) | 7 | Workspace instance management tools. |
| [system](#system) | 55 | Core IDE/system tools: file & editor operations, build/test, git, search, knowledge, memory, and app control. |
| [team](#team) | 27 | Expert-team orchestration tools: create teams, assign agents, route work. |
| [wa](#wa) | 86 | Windows Automation (UIA) tools: desktop element discovery, action planning, snapshots, OCR, clipboard, and virtual desktops. |


## browser

Native browser-engine and web-automation tools: navigation, crawling, AOM/DOM inspection, forms, auth profiles, runtime capture, and visual artifacts.

109 tools.

| Tool | Description |
|------|-------------|
| `browser_access_diagnostics` | Read a compact, truthful access/challenge diagnosis for a persisted browser session using the current snapshot when available. |
| `browser_apply_auth_profile` | Apply a saved auth profile to a target browser session, then report the resulting auth diagnosis for that session. |
| `browser_auth_diagnostics` | Read a compact, truthful auth/session recovery diagnosis for a persisted browser session using cookies, storage, and the current snapshot if available. |
| `browser_create_session` | Create or reset a persisted browser session with its own cookie jar and navigation state. |
| `browser_diff_checkpoints` | Compute a truthful semantic diff between two saved browser checkpoints for the same session. |
| `browser_diff_snapshots` | Compute a truthful semantic diff between two persisted browser snapshots identified by their original page URLs. |
| `browser_get_cookies` | Read the persisted browser cookie jar for a session so authenticated flows can inspect recovery state explicitly. |
| `browser_get_session` | Read the current persisted browser session state, including current URL, cookies, and storage state. |
| `browser_get_session_network` | Read the persisted per-session network config that the truthful browser runtime can apply to outgoing requests. |
| `browser_get_storage` | Read persisted browser storage state for a session scope (local or session). |
| `browser_get_trace_logs` | Read rich runtime trace entries for browser operations. |
| `browser_get_trace_summary` | Read compact runtime trace summary for browser operations including console messages, network activity, DOM mutations, screenshots, and health warnings. |
| `browser_list_auth_profiles` | List saved browser auth profiles using compact summaries that include filtered auth cookie/storage counts and persisted artifact paths. |
| `browser_list_checkpoints` | List saved browser checkpoints for a session from the persisted checkpoint artifacts, including compact summaries with checkpoint artifact paths. |
| `browser_list_sessions` | List persisted browser sessions from the workspace session directory using compact summaries that include persisted session artifact paths. |
| `browser_list_snapshots` | List persisted browser snapshot artifacts from the workspace snapshot directory using compact summaries that include persisted snapshot artifact paths. |
| `browser_list_workflow_runs` | List persisted browser workflow run reports from the workspace run-artifact directory using compact summaries with run report paths. |
| `browser_list_workflow_suite_runs` | List persisted browser workflow suite run reports from the workspace suite-run artifact directory using compact summaries with suite run report paths. |
| `browser_list_workflow_suites` | List saved browser workflow suite artifacts from the workspace suite directory using compact summaries with suite artifact paths. |
| `browser_list_workflows` | List saved browser workflow artifacts from the workspace workflow directory using compact summaries with workflow JSON and NDA artifact paths. |
| `browser_native_assert` | Assert page-state conditions in one call instead of observe-then-read. Checks that the distilled page content contains `text` and/or that an element whose name contains `label` exists. With waitMs > 0 the checks poll until the conditions... |
| `browser_native_back` | Navigate the native browser session back to the previous page in its history stack and return the resulting NDA delta and refreshed AOM view. |
| `browser_native_brief` | One-call pre-action context bundle: page identity, structure digest (element counts + heading outline), guard health (assert pass/fail counts), suggested next action, learned per-domain patterns, semantically similar remembered pages, fa... |
| `browser_native_check_label` | Check or uncheck the checkbox/radio whose label or accessible name best matches the query, and return the resulting NDA delta and refreshed AOM view. Selecting a radio clears every other radio that shares its name, and the new state is r... |
| `browser_native_checkpoint` | Named page-state checkpoints in the native browser session. action=save snapshots the current state under a name; action=diff reports everything that changed since that snapshot as one NDA delta (spanning any number of actions); action=l... |
| `browser_native_click` | Click an element in the live native browser DOM, targeted by node id (from the current browser_native_read; a stale or invented id is an error) or by role + accessible name, and return the resulting NDA delta and refreshed AOM view. Clic... |
| `browser_native_click_text` | Click the clickable element (button, link, checkbox, radio) whose visible text or accessible name best matches the query. Exact matches beat substring matches; ties break on actionability. Returns the resulting NDA delta and refreshed AO... |
| `browser_native_cookies` | Get, set, or delete cookies for the current session/origin. |
| `browser_native_eval` | Evaluate a JavaScript expression in the live native browser session and return the result alongside the refreshed AOM view. |
| `browser_native_export_nda` | Persist the live session's state as an NDA artifact under .velocity/browser_artifacts/. format=binary (default) writes the 18-byte hashed triple stream ({sessionId}_native.nda); format=readable writes and returns the lossless fact text (... |
| `browser_native_extract` | Extract content from a DOM element: its text content, innerHTML, outerHTML, or a specific attribute value. The nodeId must exist in the currently loaded document (take it from browser_native_read); an id from an earlier page is reported ... |
| `browser_native_fill_label` | Fill the text input or textarea whose label, placeholder, or accessible name best matches the query. Returns the resulting NDA delta and refreshed AOM view. |
| `browser_native_find` | Query the live Agentic Object Model of the native browser session by role and/or a case-insensitive text match over accessible names and values. Returns only the matching elements with their node ids u{2014} far cheaper than reading the ... |
| `browser_native_focus_label` | Move session keyboard focus to the focusable element whose accessible name best matches the query. Focus is a readable fact (AOM focused) so the delta shows exactly where focus moved. Follow with browser_native_press to type or submit. |
| `browser_native_forward` | Navigate the native browser session forward in its history stack and return the resulting NDA delta and refreshed AOM view. |
| `browser_native_history` | List the native browser session's navigation history stack: every visited url with its page title in stack order, marking the entry the session currently points at. The token-cheap answer to "where have I been" u{2014} pairs with browser... |
| `browser_native_hover` | Hover an element (fire mouseenter/mouseover events) targeted by node id (from the current browser_native_read; a stale or invented id is an error) or role + accessible name. |
| `browser_native_learn` | Persist or restore the session's experience stores as NDA artifacts under .velocity/browser_artifacts/, so what one session learned improves later ones. what=confidence (default) is the learned per-domain action confidence; what=memory i... |
| `browser_native_links` | List the current page's navigation map in the native browser session: every link's text and href target in document order, with the node id to click. Optional case-insensitive filter over link text and href. The token-cheap answer to "wh... |
| `browser_native_navigate` | Navigate the native pure-Rust browser engine to a URL over HTTPS, load it into the live DOM, and return the readable AOM view plus the NDA delta the navigation produced. Session state (DOM, cookies, storage) persists across native tool c... |
| `browser_native_network` | List recent network requests (fetch/XHR) made during the session with URL, method, status, and resource type. |
| `browser_native_observe` | Dump the full readable fact base of the session (URL, title, AOM roles/names/values/checked state, focus, layout, cookies, storage) as 'subject predicate = object' lines. The complete observation an agent can diff or reason over. |
| `browser_native_page_text` | Read the current page in the native browser session as distilled text. format=text (default) is title + visible body text in reading order, whitespace collapsed, script/style content skipped u{2014} the token-cheapest way to read a whole... |
| `browser_native_predict` | Suggest the next best action on the current page using learned per-domain confidence: outcome scores from past native actions teach which (element role, action) combinations work on this domain, and the highest-confidence actionable elem... |
| `browser_native_press` | Press a key against the session's focused element: 'Enter' submits the enclosing form, 'Tab' advances focus to the next control (wrapping), and a single character types into the focused control. Requires focus set via browser_native_focu... |
| `browser_native_press_key` | Press a keyboard key (fire keydown/keypress/keyup events) in the session. |
| `browser_native_read` | Read the current live Agentic Object Model of a native browser session: URL, title, and every actionable element with its node id, role, accessible name, value, actionability score, and checked state (a checkbox or radio that is ticked i... |
| `browser_native_read_form` | Read every form control on the page as compact text: one line per control with its accessible name, role, and current value or checked state. The cheapest way to verify form state before submitting. |
| `browser_native_recall` | Recall pages previously stored with browser_native_remember. Modes: semantic (TF-IDF cosine similarity, scored), keyword (substring over text/url), tag (exact tag match), similar (query is a memory id; finds pages most similar to that me... |
| `browser_native_reflect` | Self-reflection over recent native browser actions: every action is scored from the NDA delta it actually produced, and this tool reports detected failure patterns (repeated failures, navigation loops, blocked clicks) with suggested stra... |
| `browser_native_remember` | Index the current page of the native browser session into vector memory (distilled markdown u{2014} title, headings and content with boilerplate stripped u{2014} plus optional note, TF-IDF embedded) so it can be recalled later by meaning... |
| `browser_native_screencast` | Structural screencast of the native browser session: frames record the page's shape (viewport size, AOM element count, content hash) instead of pixels. Actions: capture (record a frame of the current state), list (show the frame timeline... |
| `browser_native_screenshot` | Serialize the current DOM as a structured text snapshot showing URL, title, DOM node count, and all actionable AOM elements. A semantic representation, not pixels. |
| `browser_native_scroll` | Scroll the native browser viewport by a pixel delta and return the resulting NDA delta (the session scroll fact plus any nodes whose in-viewport visibility flipped) and refreshed AOM view. |
| `browser_native_scroll_into_view` | Scroll an element into the native browser viewport by its accessible name (button/link text, label, aria-label) and return the NDA delta showing which nodes entered or left the viewport, plus the refreshed AOM view. |
| `browser_native_select` | Set the selected value of a combobox/select element in the live native browser DOM, targeted by node id (from the current browser_native_read; a stale or invented id is an error) or role + accessible name, and return the resulting NDA de... |
| `browser_native_select_label` | Pick an option in the select/combobox whose label best matches the query. The option is matched by its visible text or value (exact beats substring). Returns the resulting NDA delta and refreshed AOM view. |
| `browser_native_settle` | Flush pending timers and microtasks in the session's JS runtime and return the NDA delta of everything that changed while settling. Call after actions that trigger async work (setTimeout, fetch callbacks). |
| `browser_native_storage` | Get, set, or clear localStorage/sessionStorage for the current session. |
| `browser_native_submit` | Submit a form (or form control) in the live native browser DOM, targeted by node id (from the current browser_native_read; a stale or invented id is an error) or role + accessible name, and return the resulting NDA delta and refreshed AO... |
| `browser_native_tab_close` | Close a background tab and drop its state. The active tab cannot be closed - switch to another tab first. Returns the refreshed tab list. |
| `browser_native_tab_list` | List every tab in the live native browser session with its id, title, URL, and which one is the active (foreground) tab that all other browser_native_* tools act on. |
| `browser_native_tab_open` | Open a new blank background tab in the live native browser session. Returns the refreshed tab list. Use browser_native_tab_switch to bring it to the foreground before acting on it. |
| `browser_native_tab_switch` | Bring the named tab to the foreground; the previous foreground tab is parked in the background with its full state (DOM, cookies, focus, traces). Returns the tab list plus the view of the newly active tab. |
| `browser_native_type` | Type text into an input-like element in the live native browser DOM, targeted by node id (from the current browser_native_read; a stale or invented id is an error) or role + accessible name, and return the resulting NDA delta and refresh... |
| `browser_native_validate` | Run HTML5 constraint validation over every form control on the current page of the native browser session: required, email/url/number/date/time type checks (including whether a temporal value is even well-formed), pattern, minlength/maxl... |
| `browser_native_wait` | Block until a condition holds on the page or the timeout elapses, instead of polling observe/brief. mode=content (default) waits for the distilled content size to change by at least minDelta chars (async updates, lazy loading); mode=elem... |
| `browser_native_wait_for` | Poll the live AOM until an element matching the given role and/or accessible name appears (with timeout). Returns the matched node id and refreshed view; if nothing matches before the timeout the call fails, so a miss cannot be read as a... |
| `browser_read_auth_profile` | Read a saved browser auth profile by name. |
| `browser_read_checkpoint` | Read a saved browser session checkpoint JSON artifact for inspection. |
| `browser_read_session_transcript` | Read persisted browser session transcript history for a session, with compact discovery or a full entry lookup by sequence. |
| `browser_read_snapshot` | Read a persisted browser snapshot JSON artifact by its original page URL. |
| `browser_read_visual_fallback` | Read the persisted HTML fallback artifact for a captured page URL when truthful raw HTML evidence is available. |
| `browser_read_workflow` | Read a saved browser workflow JSON artifact from the workspace. |
| `browser_read_workflow_run` | Read a persisted browser workflow run report for a workflow/session pair. |
| `browser_read_workflow_suite` | Read a saved browser workflow suite JSON artifact from the workspace. |
| `browser_read_workflow_suite_run` | Read a persisted browser workflow suite run report by suite name. |
| `browser_replay_workflow` | Replay a saved semantic browser workflow deterministically using the current static AOM-first engine with session/form support. Provide sessionId to run inside an existing persisted session. |
| `browser_reseed_auth` | Copy auth cookies plus CSRF-relevant storage from a source browser session or checkpoint into a target session, then report the resulting target auth diagnosis. |
| `browser_restore_checkpoint` | Restore a saved browser session checkpoint, optionally forking it into a different session id. |
| `browser_run_workflow_suite` | Execute a saved semantic browser workflow suite and persist an aggregated suite run report. |
| `browser_runtime_capture` | Capture a page through the Go chromedp runtime, then persist the resulting browser snapshot, session state, HTML fallback, and crawl facts into the Rust browser artifact model. |
| `browser_runtime_visual_capture` | Capture a truthful runtime PNG screenshot through the Go browser API and persist both the image and artifact metadata under .velocity/browser-runtime-visuals. |
| `browser_save_auth_profile` | Save a reusable auth profile from a source browser session or checkpoint using only auth cookies plus CSRF-relevant storage. |
| `browser_save_checkpoint` | Persist the current browser session and its latest snapshot as a named checkpoint for later restore or forking. |
| `browser_save_workflow` | Persist a semantic browser workflow as JSON plus NDA-backed DSL for later deterministic replay. |
| `browser_save_workflow_suite` | Persist a semantic browser workflow suite as JSON so multiple saved workflows can run as a lightweight regression pack. |
| `browser_session_click` | Click a navigable element in the current persisted browser session using the truthful static AOM-first interaction model. |
| `browser_session_fill` | Fill a form field in the current persisted browser session using the truthful static AOM/form interaction model. |
| `browser_session_health` | Read a truthful aggregated health and recovery report for a persisted browser session using saved session, snapshot, auth, access, checkpoint, network, and HTML fallback evidence. |
| `browser_session_navigate` | Navigate a persisted browser session so cookies and current URL survive across requests. |
| `browser_session_submit` | Submit a form in the current persisted browser session using the truthful static form-post interaction model. |
| `browser_session_wait` | Poll the current page in a persisted browser session until text or an element appears, then persist the updated snapshot and semantic diff. |
| `browser_set_cookies` | Seed or update the persisted browser cookie jar for a session to recover or resume authenticated flows. |
| `browser_set_session_network` | Update the persisted per-session network config for request headers, user-agent, timeout, redirect following, and allow/block URL policy. |
| `browser_set_storage` | Seed or update persisted browser storage state for a session scope (local or session). |
| `runtime_capture_session` | Capture the current page from an explicit Go runtime browser session and persist it into the Rust browser artifact model. |
| `runtime_close_session` | Close an explicit Go runtime browser session and remove the persisted Rust-side runtime session mapping. |
| `runtime_create_session` | Create an explicit Go runtime browser session and persist the Rust-side runtime session mapping under .velocity/runtime-browser-sessions. |
| `runtime_get_session` | Read a persisted explicit runtime browser session mapping. |
| `runtime_reseed_auth` | Copy auth cookies plus CSRF-relevant storage from a source browser session or checkpoint into a target explicit runtime session, then report the resulting runtime auth diagnosis. |
| `runtime_session_click` | Click a runtime target in an explicit Go browser session using either nodeId or selector, then persist the captured result. |
| `runtime_session_evaluate` | Evaluate JavaScript in an explicit Go runtime browser session, then persist the captured result and returned evaluation payload. |
| `runtime_session_fill` | Fill a runtime target in an explicit Go browser session using nodeId or selector, then persist the captured result. |
| `runtime_session_js_click` | Dispatch a JS click against an explicit Go runtime browser session target. Requires nodeId because the runtime API does not support selector fallback for js_click. |
| `runtime_session_navigate` | Navigate an explicit Go runtime browser session to an absolute URL, then persist the captured result. |
| `runtime_session_press_key` | Press a key in an explicit Go runtime browser session, then persist the captured result. |
| `runtime_session_submit` | Submit within an explicit Go runtime browser session by targeting a nodeId or selector when available, or by allowing the runtime to fall back to Enter-key submission. |
| `web_navigate` | Navigate and crawl a page with the current static AOM-first browser engine. It captures a truthful live snapshot, persists SiteMap facts, and writes browser artifacts for links, forms, and cookies discovered in fetched HTML. |

## company



44 tools.

| Tool | Description |
|------|-------------|
| `add_org_playbook` | Add or update a deterministic playbook owned by a role. Running it first submits the (role, resource, capability, amount) intent to the approval engine; the steps execute ONLY if that action clears. Steps reuse the workflow engine so rep... |
| `add_org_resource` | Register an external system the org can act on (GitHub, email, banking, social, cloud, ...). Only a secret *reference* (env-var name / keychain label) is stored — never the secret itself. |
| `add_org_role` | Add a role/position to the company org chart. |
| `approve_and_resume_org_action` | Sign a pending request and, if that signature reaches quorum, immediately run the playbook whose steps it was gating (the one-click form of approve_org_action + resume_org_playbook). Below quorum nothing runs; a request not raised by a p... |
| `approve_org_action` | Record an approval from a designated approver on a pending request; advances to Approved once quorum is reached. Approver role must hold Approve authority on the resource. |
| `chain_org_playbooks` | Wire a deterministic cross-SOP handoff: when `upstream_id` finishes CLEAN (every step ok), the downstream playbook's intent is queued automatically. Chaining routes WORK, never authority — the downstream SOP is submitted under its own ro... |
| `check_org_credential_elevation` | Query whether an active (unexpired, unconsumed) just-in-time elevation exists for a role's (resource, capability) right now. This is the deterministic gate an elevated action consults before releasing a secret — a plain ledger query, no ... |
| `check_org_permission` | Deterministically evaluate whether a role may perform a capability on a resource at a given amount. Returns ALLOWED, REQUIRES APPROVAL (approvers+quorum) or DENIED. No LLM inference is used. |
| `consume_org_credential_elevation` | Spend an active elevation to authorize exactly one execution. Errors (and records Expired/Denied in the ledger) if the grant is past its TTL or already consumed, so a minted elevation can never be replayed. Returns the consumed grant's s... |
| `dispatch_org_role` | Route a natural-language task to the right org role and hand it to the synced expert team for execution. Routing and team sync are deterministic; only the member's work is inference. Outward/sensitive actions remain gated by the org appr... |
| `elevate_and_resume_org_request` | Complete an approved irreversible action: mint the single-use TTL-bounded credential elevation its scope requires, then run the playbook waiting on that request (the elevation is consumed by the run, so one call authorizes exactly one ex... |
| `execute_org_browser_broadcast` | Fan an outward campaign across many platforms and actually drive each cleared account against the live browser engine. Each platform is gated independently: give a platform a request_id to post only once that action request is Approved (... |
| `execute_org_browser_task` | Run a gated cross-platform browser task for an org role against the live browser engine: gate the action, resolve the credential from the vault at execution (never echoed), open a session and replay the login + optional composer steps. P... |
| `execute_org_browser_task_for_request` | Run a browser task on behalf of an ALREADY-APPROVED action request (by request_id). Where execute_org_browser_task gates on the stateless permission matrix, this gates on the request: it must exist and be Approved, otherwise nothing runs... |
| `execute_org_credential_rotation_after_approval` | Apply a rotation whose PENDING request has already cleared to Approved. Mirrors the gated-playbook resume path: the request id is the proof, the pointer swap is applied, and the ledger entry transitions to Executed so the rotation cannot... |
| `get_company_org` | Read the company org chart: departments, roles with reporting lines, external resources (with secret references only, never secret values), capability grants and approval policies. Pass full=true for the raw JSON. |
| `grant_capability` | Grant a role one or more capabilities over a resource (deny-by-default: without a grant the action is denied). Optionally set a per-action spend limit. |
| `grant_org_credential_elevation` | Mint a single-use, TTL-bounded just-in-time credential elevation from an ALREADY-APPROVED action request. The elevation's scope (role, resource, capability) is copied verbatim from the request, so an agent can never mint broader access t... |
| `init_company_org` | Seed the workspace with the UnitBuilds CC company org (Executive, Management, Engineering, Testing, Scouting, Marketing, Support, Finance, Legal, Operations) including capability grants and approval policies, persisted to .velocity/compa... |
| `list_org_actions` | List action requests, optionally filtered by status (pending\|approved\|rejected\|executed). |
| `list_org_credential_audit` | Read the append-only, secret-free credential-use audit ledger: who elevated or used what capability on which resource, when, and the outcome (Granted/Used/Denied/Expired). Never stores a credential value — only the pointer-level facts. R... |
| `list_org_escalations` | Deterministic escalation routing for the approval pipeline. With 'approver_id', lists exactly the pending actions that approver is designated to clear (their queue), each annotated with its chain context: the playbook a signature runs, t... |
| `list_org_playbook_verdicts` | Cross-SOP verdict memory: the recent playbook runs that HALTED on a verdict gate (e.g. a browser VERIFY: FAIL, an elevation-gated spend refused, a campaign that never landed) or FAILED mid-run, as a secret-free escalation briefing — play... |
| `list_org_playbooks` | List the org's playbooks with their gate (role, capability, resource). |
| `list_org_schedules` | The org's recurring duties: each armed cadence with its interval, whether it is due/overdue or when it next fires, last run and armed/PAUSED state. Secret-free — ids, intervals and timestamps only. |
| `list_org_sop_handoffs` | The cross-SOP chain ledger: every recorded hop (which SOP handed off to which, at what depth, under which requests) with the downstream gate's own verdict — ran, awaiting-approval, denied, not-found or depth-limit. Secret-free by constru... |
| `pause_org_playbook_schedule` | Pause (default) or resume an armed cadence without disturbing its interval or due clock. A paused duty is skipped by every tick; resuming leaves it on its original schedule rather than dumping a backlog. |
| `plan_org_browser_broadcast` | Fan a single outward campaign out across many platforms at once (e.g. post to X and LinkedIn). Each platform is gated INDEPENDENTLY against the role's capability grant, so a partial grant yields runnable secret-free plans only for the cl... |
| `plan_org_browser_login` | Build a gated, secret-free cross-platform browser plan (navigate/fill/submit login + optional composer post) for an org role. The password references the vault as 'vault:SECRET_REF', never a literal secret. Returns NO plan if the approva... |
| `reject_org_action` | Reject a pending action request. |
| `remove_approval_policy` | Delete an approval policy by id. |
| `remove_org_playbook` | Delete a playbook by id. |
| `request_org_action` | Propose an action and record it in the approval ledger. Auto-approved if permission allows; otherwise queued Pending for the required approvers. |
| `resume_org_playbook` | Resume a gated playbook whose approval request has cleared. Takes the request_id returned by a prior gated run_org_playbook. Runs the stored deterministic steps ONLY if that request is now Approved, threading {{requestId}} into outward-a... |
| `revoke_capability` | Remove capabilities a role holds over a resource. |
| `rotate_org_resource_credential` | Rotate a resource's stored `secret_ref` POINTER through the credential-vault gate (Operations key-rotation primitive). Never returns the raw secret. The proposal is routed through check_permission(role, "credentials", deploy): ungranted ... |
| `run_due_org_playbooks` | The scheduler tick: fire every due cadence through the gated playbook runner and advance each clock by exactly one interval, so a backfill after downtime runs a duty once (never N times) and a halting SOP waits a full interval instead of... |
| `run_org_playbook` | Run a playbook: evaluate its permission gate, and if cleared execute the deterministic steps. If gated, no steps run and the pending request is returned for approval. |
| `schedule_org_playbook` | Arm a recurring cadence on an existing playbook (the org's self-starting duties: uptime sweeps, staging verification, weekly reviews). Arming grants NOTHING — when the cadence falls due it runs through the same approval gate as a manual ... |
| `set_approval_policy` | Add or update an approval policy: which capability (+resource, +amount threshold) requires how many approvals from which roles. Empty approvers means the human operator signs off. |
| `sync_org_team` | Project the company org chart into Velocity's expert-team roster so the AI company becomes runnable. Each role becomes a team member with a persona drawn from its title, department charter and capability grants. Upserts the org's team by... |
| `unschedule_org_playbook` | Retire a playbook's recurring cadence. The playbook itself stays defined and manually runnable. |
| `validate_org_playbooks` | Validate all playbooks: check every Tool step references a registered tool. Reports issues or confirms all valid. |
| `verify_org_credential` | Audit whether a role's credential for a resource actually resolves through the vault, WITHOUT returning the secret. Reports GRANTED (masked preview), NEEDS APPROVAL, DENIED or MISSING (pointer not provisioned). The secret is zeroized imm... |

## drone

Distributed build-node (drone) tools: register, ping, remote build/run, and peer file operations.

10 tools.

| Tool | Description |
|------|-------------|
| `drone_click` | Ask a drone to simulate a mouse click at coordinates on the remote machine. Queues a TaskRequest message and reports `success: false`, `status: "queued"`: the drone does not dispatch message kinds, so no click is delivered. |
| `drone_command` | Send a shell command to a deployed drone for execution. The drone runs the command asynchronously and returns a task ID. Use drone_task_status to poll for results. |
| `drone_deploy` | Deploy the velocity-drone binary to a remote machine via SSH. Copies the drone binary and starts it as a background process. Returns the drone's connection details (host, port, id). |
| `drone_network_stats` | Ask a drone for network statistics (bytes sent/received, active connections). Returns no counters: the request is queued on the message endpoint and answered with a receipt (`success: false`, `status: "queued"`). The drone implements a n... |
| `drone_pair` | Initiate pairing with a drone. After pairing, the drone appears in the IDE's peer panel and can receive messages and tasks through the peer protocol. |
| `drone_screenshot` | Ask a drone to capture its screen. Returns no image data: the request is filed on the drone's message queue and answered with a receipt only (`success: false`, `status: "queued"`). The drone implements screen capture in drone/src/system.... |
| `drone_status` | Query a drone's health and identity. Returns the drone's ID, name, version, capabilities, uptime, and environment. |
| `drone_task_status` | Check the status of a previously submitted drone task. Returns exit code, stdout, stderr, and completion state. |
| `drone_type_keys` | Ask a drone to simulate keyboard input on the remote machine. Queues a TaskRequest message and reports `success: false`, `status: "queued"`: the drone does not dispatch message kinds, so no keystroke is delivered. Use drone_command to ru... |
| `drone_upload` | Upload a file from the local workspace to the remote drone machine. Supports chunked transfer for large files with SHA-256 verification. |

## generation

Code generation tools driven by the agent.

5 tools.

| Tool | Description |
|------|-------------|
| `generate_image` | Generate an image from a text prompt. The provider is inferred from the selected model: a model registered in the generation registry (see list_generation_models, e.g. 'wan2.7-image') is routed through its native async generation endpoin... |
| `generate_media` | Generic multimodal generation: submit a generation request to any registered model with arbitrary input/parameters. Use list_generation_models first to discover available models and their expected input schemas. Supports future models wi... |
| `generate_video` | Generate a video from a text prompt using a text-to-video model (e.g. happyhorse-1.1-t2v). Submits an async job, polls until the video is ready, and downloads the MP4. Video generation may take 1-5 minutes. Returns the local file path an... |
| `list_generation_models` | List all available multimodal generation models with their input schemas, default parameters, and output types. Use this to discover what models are available before calling generate_media. |
| `register_generation_model` | Register a new multimodal generation model in the workspace registry so it becomes available via generate_media. Provide the model_id, API endpoint path, input schema, and output type. |

## instances

Workspace instance management tools.

7 tools.

| Tool | Description |
|------|-------------|
| `instance_add` | Register a managed instance (remote node) in the workspace registry at .velocity/instances.json. Requires a display name, a role (buildbox \| agent_host \| gui_server) and the drone endpoint address. Does not contact the node: use instance... |
| `instance_deploy` | Push the velocity-drone binary to a machine over SSH, start it, verify health, and register the node in the registry in one step - the buildbox equivalent of plugging it in. Generates a bearer token and stores it on the record (it is nev... |
| `instance_list` | List managed instances with their stored and effective status, eligibility and in-flight load. A node marked online but not seen healthy within 120s reports effective_status 'offline' - routing follows the effective status, not the store... |
| `instance_pick` | Choose which eligible node should take a piece of work: filters by role/platform/labels, then selects the fewest in-flight items (ties broken by id, so identical state always routes identically). Returns success false with an explanation... |
| `instance_ping` | Query a registered instance's drone /peer/health and record the outcome: reachable nodes become 'online', an unreachable one drops to 'degraded' or 'offline' and that downgrade is persisted. If the record had no platform, it is adopted f... |
| `instance_release` | Mark one in-flight item on a node as finished, undoing an instance_pick claim. The counter never goes below zero, so a double release is harmless. |
| `instance_remove` | Remove a managed instance from the registry by id. Does not stop the drone process on the remote machine - only forgets it locally. |

## system

Core IDE/system tools: file & editor operations, build/test, git, search, knowledge, memory, and app control.

55 tools.

| Tool | Description |
|------|-------------|
| `agent_checkpoint_create` | Create a workspace checkpoint (git-based snapshot) before making changes. Allows restoring to this point later. |
| `agent_checkpoint_list` | List all available workspace checkpoints with their IDs, labels, and creation times. |
| `agent_checkpoint_restore` | Restore the workspace to a previously created checkpoint. Reverts all file changes made after that checkpoint. |
| `agent_memory_forget` | Remove a specific memory by its key. Use to clean up outdated or incorrect memories. |
| `agent_memory_recall` | Recall memories relevant to a query using semantic similarity. Returns the most relevant stored memories. |
| `agent_memory_remember` | Store a persistent memory that the agent can recall in future sessions. Use for strategies, facts, and learned patterns. |
| `audit_status` | Show the tool execution audit log status: total entries recorded, active sessions, and the most recent tool calls with their outcomes and durations. Useful for verifying that all tool invocations are being tracked. |
| `clean_disk_artifacts` | Delete known-safe build-artifact trees to reclaim disk space. ONLY trees that scan_disk_artifacts classifies as 'safe' (artifact name plus its context manifest, e.g. a target/ beside a Cargo.toml) can be removed; review-classified and un... |
| `code_coverage_analyze` | Analyze test coverage for the workspace (or a specific file/directory via 'path'). Discovers testable functions, reports the coverage percentage and untested functions, and scaffolds test skeletons for the gaps. |
| `code_generate_tests` | Generate test scaffolding for source code. Analyzes function signatures and produces test stubs with edge cases. |
| `connector_call` | Invoke a configured external HTTP connector by id. Resolves the connector's credential from the encrypted secret store, assembles the request (base URL + path, headers, auth), and returns the response status and body. |
| `convert_to_nda` | Convert any file (text, code, CSV, image) into a portable, browser-viewable NDA1 (.nda) document with self-contained provenance. Text becomes wrapped draw-text commands plus content triples; images become a draw-image command. Optionally... |
| `delete_file` | Delete a file in the workspace. Include 'context' to record *why* this file is being removed — the reason is persisted in the codebase event log. |
| `describe_image` | Describe an image file. Vision-capable models can consume the image directly; returns an OCR text fallback for non-vision models plus mime and size. |
| `edit_file` | Apply targeted replacements to an existing file instead of rewriting it. Prefer this over write_file when only part of a file is wrong: it sends a few hundred bytes rather than the whole file, so a long fix cannot be cut off by the gener... |
| `event_attach_context` | Attach agent reasoning (the *why*) to an existing codebase event. Use this after a change to record why it was made — the requirement, the motivation, the design decision. This context is what makes the event store valuable for future ag... |
| `event_context` | Get full detail for a single codebase event by sequence number. Includes agent context, Merkle roots, affected files, and failure reason. |
| `event_history` | Query the codebase event history. Returns events newest-first with optional filters for file path and outcome (success/failure/revert/pending). Shows the full decision trail including failed attempts and reverts. |
| `event_mark_outcome` | Update the outcome of a previously recorded codebase event. Use this to mark a change as success, failure (with reason), or revert. This enables tracking which approaches worked and which didn't, so future agents don't repeat failed expe... |
| `event_record` | Record a codebase event linking an agent action to a site map state change. Captures *why* a change was made (context), what files were affected, and the Merkle root before/after. Events are persisted to .velocity/events/events.jsonl for... |
| `event_timeline` | Show a chronological timeline of all codebase events with state transitions (Merkle root changes). Shows the codebase's evolution over time — what changed, when, why, and with what outcome. Newest first. |
| `execute_nda` | Execute a runnable .nda container. If it holds a compiled C# binary, it is run in-memory. If it contains a script (e.g., Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process. |
| `fetch_panel_data` | Read structured IDE panel data without navigating the GUI. Supported panels: teams (expert-team roster), wiki (workspace documentation summary), graph (symbol/file summary), bookmarks (saved workspace bookmarks), and files (a workspace d... |
| `git_clone` | Clone a git repository into the workspace. git is run directly (not via a shell) so the URL cannot inject commands; it must start with http://, https://, git:// or git@. The destination is confined to the workspace and defaults to '.velo... |
| `grep_search` | Find lines containing a query string in the workspace. |
| `gui_app_map` | Get the running IDE's navigable surface as a graph: rails, their sub-tabs, menus, dock panels, workspace profiles and commands, with the action each hop needs. With 'to', returns the shortest route there instead. Requires the GUI to be r... |
| `gui_dismiss_overlays` | Close every transient overlay in the running IDE at once -- command palette, quick open, go-to-line/symbol, references, the tab and workspace switchers, the keyboard cheat-sheet, full diff, find/replace, and the in-app open/save-as/close... |
| `gui_get_state` | Get the current state of the running IDE: open files, active file, selected activity-bar rail and the sub-tab it is showing, focused central tab, whether the central area shows the dock or the welcome screen, active workspace profile/mod... |
| `gui_list_commands` | List the running IDE's command palette entries with their risk tier: 'navigate' only moves focus, 'modify' writes, 'execute' spawns processes or opens a dialog. Pass a category to filter. Requires the GUI to be running. |
| `gui_list_tabs` | List the tabs the running IDE holds, with each one's dock membership and which has focus. Reports 'docked' separately from membership in the tab list, because a dock rebuild can orphan a tab and those need different fixes. Requires the G... |
| `gui_navigate_panel` | _see source_ |
| `gui_navigate_to` | Move the running IDE to a node named by gui_app_map, walking there through the same entry points a click uses. Focus only: it refuses commands tiered modify or execute, so use gui_run_command for those. Requires the GUI to be running. |
| `gui_open_file` | Open a file in the running IDE's editor. Requires the GUI to be running. Sends a command via the GUI control named pipe. |
| `gui_quit` | Quit the running IDE. Requires the GUI to be running. |
| `gui_run_command` | Invoke a command palette entry by label in the running IDE, through the same action a click fires. Refused for anything tiered modify or execute unless allow_unsafe is set, and for entries the current workspace profile hides. Entries tha... |
| `gui_screenshot` | Capture the running IDE's own window to an image file and report its path and size, so a claim about the UI can be checked against pixels rather than only against what the app says it drew. The path must stay inside the workspace; omit i... |
| `gui_select_sub_tab` | _see source_ |
| `gui_select_tab` | Switch the running IDE's focused tab by numeric id, title or panel slug -- the Ctrl+Tab path, not the open-a-new-panel path. Requires the GUI to be running. |
| `gui_submit_dialog` | Answer the path prompt currently on screen in the running IDE -- the Open File or Save As dialog -- with the path a person would have typed into its box, and press the button. Completes rather than cancels: Save As writes the active buff... |
| `gui_toggle_panel` | _see source_ |
| `index_workspace` | Index the workspace by compiling all Rust source files into the site map (NDA triples). This populates the symbol graph, enables the wiki, and gives the IDE semantic understanding of the codebase. Must be run at least once before graph/w... |
| `knowledge_ingest` | Add content to the workspace knowledge base (a persistent, chunked RAG store). Provide either inline 'text' (with an optional 'source' name) or a workspace-relative 'path' to a file or directory. |
| `knowledge_search` | Search the workspace knowledge base and return the most relevant passages, ranked by TF-IDF cosine similarity. |
| `list_dir` | List the contents of a directory relative to the workspace root. |
| `read_file` | Read the contents of a file in the workspace. |
| `read_nda` | Read and parse a compiled .nda binary file to view its semantic triples, visual display commands, and string pool contents. |
| `run_command` | _see source_ |
| `scan_disk_artifacts` | Scan the workspace for build-artifact trees (cargo target/, node_modules/, __pycache__, venvs, dist/build/.next caches, .NET obj) and report each tree's reclaimable size, safety class, and the drive's free space. Updates the persisted hy... |
| `tool_list_custom` | List all dynamically registered custom tools with their names, descriptions, and input schemas. |
| `tool_register` | Register a new custom tool backed by a shell command. The tool becomes immediately available for invocation without restarting the MCP server. Use {{param}} placeholders in the command to substitute argument values at call time. Custom t... |
| `tool_unregister` | Remove a previously registered custom tool. The tool is removed from the active list and from disk persistence. |
| `use_skill` | Read the full markdown body of one workspace knowledge skill (.velocity/skills/*.md) by name. The PROJECT SKILLS block in your instructions and the on-demand index lines show which names exist; use this to pull the full guidance when a s... |
| `workflow_list` | List saved workflows from .velocity/workflows/ with per-step-type counts (T=tool/connector, C=condition, A=agent task, F=foreach) and tool-name validation issues flagged. Use workflow_run to execute. |
| `workflow_run` | Execute a saved workflow by id. Runs its steps in order (tool calls, agent tasks, conditions) and returns the run record with per-step status and output. |
| `write_file` | Write or overwrite a file with specific content in the workspace. Creates folders if they do not exist. Rust files are run through rustfmt after the write, so the bytes on disk may be laid out differently from what you sent; the result s... |

## team

Expert-team orchestration tools: create teams, assign agents, route work.

27 tools.

| Tool | Description |
|------|-------------|
| `add_team_member` | Add a new member to an existing expert team. The member object follows the same schema as create_expert_team members. Cannot add to preset teams. |
| `bulk_import_members` | Bulk import multiple members to an existing team at once. Each member object follows the same schema as create_expert_team members. More efficient than calling add_team_member multiple times. |
| `check_scope_overlaps` | Detect scope pattern overlaps between members of a team. Overlaps occur when one member's scope pattern contains another's (e.g. 'src/' and 'src/api/'). Returns detailed overlap pairs. |
| `classify_intent` | Run the intent decision brain on a request and return the routing verdict as JSON: whether it should be handled by a single agent or a specialized team, whether a repository must be cloned first, the detected target language, the special... |
| `clone_expert_team` | Clone an existing expert team (including preset teams) with a new name. All member configurations are preserved. The clone is a non-preset team that can be edited. Use this to customize a preset team. |
| `create_expert_team` | Create or update a mixture-of-experts team and persist it to .velocity/expert_teams.nda. Each member is assigned an AI provider/model, a role, optional skills (loadable .nda skill files), file scope patterns, and workflow instructions. T... |
| `create_skill_file` | Create or overwrite a reusable skill definition saved as .velocity/skills/<id>.nda. A skill body is injected into the system prompt of any team member that lists the skill id, specializing that member's behavior. |
| `create_team_quick` | Quick-create a team with minimal input. Provide a team name and member names (array or comma-separated string). Members are created with sensible defaults (Cloudflare provider, Team Lead role for first member, Specialist for others). Cus... |
| `debug_routing` | Debug the routing decision for a task without actually routing it. Shows which routing stage matched (file_scope_match, keyword_match, fallback_to_lead), the selected member, and keyword scores for all members. Useful for understanding w... |
| `export_expert_team` | Export an expert team to JSON format for sharing, backup, or version control. The JSON can be re-imported with import_expert_team. |
| `generate_wiki` | Generate a project wiki from the workspace site map with indexed analysis, PageRank reading order, and content caching. Supports markdown, HTML, and GitHub Pages export formats. Uses deterministic pre-indexing (zero LLM calls) for symbol... |
| `import_expert_team` | Import an expert team from JSON format (as produced by export_expert_team). If a team with the same slug exists, it is replaced; otherwise the team is appended. Member ids are regenerated if empty. |
| `list_expert_teams` | List the expert teams currently persisted for the workspace, including each team's slug and members. Use this before creating a team to avoid duplicating an existing one. |
| `list_providers` | List all available AI providers with their slugs, labels, and aliases. Use this to discover valid provider values for team members. |
| `list_skills` | List the reusable skill files persisted under .velocity/skills (id, name, description). Use this before creating a skill to avoid duplicating one, or to discover skill ids to attach to a team member. |
| `provision_team_from_intent` | Classify a request with the intent brain and, when it verdicts a team, author and persist the implied ExpertTeam (lead + specialists with skills and file-scope patterns) to .velocity/expert_teams.nda. Idempotent on the team slug (re-runn... |
| `remove_team_member` | Remove a member from an expert team by member id. Cannot remove from preset teams. |
| `repo_rewrite` | Orchestrate a 'clone X and rewrite it in Y' request end to end: safely git-clone the repository into .velocity/clones, analyze it deterministically (dominant languages, file count, layout), decompose it into dependency-ordered sectors sm... |
| `run_rewrite_mission` | Execute or resume the long-horizon rewrite mission that repo_rewrite planned. Reads mission.json and sector_plan.json from the mission directory, then runs the plan end to end without a human in the loop: the conventions/design pass, the... |
| `select_model` | Pick the best AI model for a task from the models that are ACTUALLY usable: live provider catalogs fetched with the API keys enabled in provider-settings (e.g. the Alibaba Qwen token-plan listing), never a fabricated model id. Optionally... |
| `team_analytics` | Show analytics and statistics for a team: member count, provider distribution, scope coverage, skills/tools per member. Useful for understanding team composition and identifying gaps. |
| `team_changelog` | Generate a versioned snapshot of a team for change tracking. Returns a snapshot hash, timestamp, and current team state. Compare hashes to detect changes over time. |
| `team_dispatch` | Dispatch a task to the best-matching team member for autonomous execution. Routes the task using file-scope matching, keyword scoring, or fallback to team lead. Builds a persona prompt from the member's role, skills, and workflow instruc... |
| `team_health_check` | Comprehensive health check combining validation, scope overlaps, and analytics. Returns a health score (0-100), status (excellent/good/fair/poor), error/warning details, and recommendations. |
| `update_expert_team` | Update an existing expert team's name and/or description. Cannot edit preset teams (clone first). The team id is regenerated from the new name. |
| `update_team_member` | Apply a partial update to a team member. Only the fields you provide are changed; all other fields remain unchanged. Cannot edit preset team members. |
| `validate_team` | Run composition validation on a team. Checks for: empty teams, team lead scope coverage, duplicate names/ids, empty fields, default model usage, and scope overlaps. Returns errors, warnings, info, and a health score (0-100). |

## wa

Windows Automation (UIA) tools: desktop element discovery, action planning, snapshots, OCR, clipboard, and virtual desktops.

86 tools.

| Tool | Description |
|------|-------------|
| `wa_bridge_run` | Execute an ordered cross-context workflow that mixes browser actions, desktop actions, cross-context waits, and data transfers. |
| `wa_browser_navigate` | _see source_ |
| `wa_browser_screenshot` | Capture a screenshot of the browser window via the bridge. |
| `wa_capture_windows_snapshot` | Capture a live Windows accessibility snapshot via UIAutomation and persist it as a WA snapshot. Read-only: it walks the target window's tree and synthesises no input. |
| `wa_clipboard_clear` | _see source_ |
| `wa_clipboard_read` | Read the current Windows clipboard content (text, HTML, files, or image detection). |
| `wa_clipboard_write` | _see source_ |
| `wa_create_session` | Create a Rust-native WA semantic session artifact with NDA-backed persistence. |
| `wa_event_poll` | Poll buffered UIA events that have been captured since last poll. |
| `wa_event_subscribe` | Block and capture Windows UI Automation events by polling: window_opened, window_closed, element_focus and structure_changed are supported; element_value_changed is not capturable by the polled listener and is rejected. |
| `wa_event_unsubscribe` | Unsubscribe from UIA events and stop the listener. |
| `wa_execute_windows_action` | Execute a deterministic Windows UIAutomation action against a saved WA snapshot. |
| `wa_file_dialog_detect` | Detect a live file dialog and report its handle, owner process, title and kind (open / save_as / folder_browse). |
| `wa_file_dialog_open` | Interact with an open file dialog: set the file path and confirm. |
| `wa_file_dialog_save` | Interact with a save file dialog: set the file path and confirm. |
| `wa_get_session` | Read a persisted WA semantic session artifact. |
| `wa_input_sequence` | _see source_ |
| `wa_list_runs` | List persisted WA script run artifacts. |
| `wa_list_scripts` | List saved WA semantic script artifacts. |
| `wa_list_sessions` | List persisted WA sessions using compact summaries. |
| `wa_list_snapshots` | List persisted WA snapshots using compact summaries. |
| `wa_monitor_list` | Enumerate all connected monitors with bounds, work area, DPI scaling, physical resolution, refresh rate and colour depth. |
| `wa_notifications_dismiss` | _see source_ |
| `wa_notifications_list` | List currently visible Windows toast notifications. |
| `wa_ocr_languages` | List the OCR language tags available on this system. |
| `wa_ocr_screen` | Perform OCR text recognition on a screen region, a full screen, or a specific window. |
| `wa_plan_action` | Plan a deterministic WA action against a saved semantic snapshot without executing it. |
| `wa_platform_info` | Report the host desktop platform and whether native desktop automation is supported. |
| `wa_platform_tree_snapshot` | Capture a platform-native accessibility tree snapshot for an application and persist it to the workspace. |
| `wa_process_info` | Get detailed information about a single process by PID. |
| `wa_process_kill` | Force-kill a process immediately by PID (no graceful shutdown). |
| `wa_process_kill_tree` | Kill a process and all of its descendant processes. |
| `wa_process_launch` | _see source_ |
| `wa_process_list` | List running Windows processes, optionally filtered by name. |
| `wa_process_running` | Check whether a process with the given PID is currently running. |
| `wa_process_terminate` | Gracefully terminate a Windows process by PID. |
| `wa_process_wait` | Wait for a process condition (exit by default, or a window title to appear) up to a timeout. |
| `wa_read_run` | Read a persisted WA script run artifact. |
| `wa_read_script` | Read a saved WA semantic script artifact. |
| `wa_read_snapshot` | Read a persisted WA semantic snapshot. |
| `wa_record_ingest` | Ingest recorded interaction events (as emitted by the recording hook) into the active recording session. |
| `wa_record_pause` | Pause or resume the active recording session. |
| `wa_record_replay_plan` | Compute the inter-step replay delays for the recorded events under a given replay configuration. |
| `wa_record_start` | Start a shared interaction recording session and return the PowerShell hook script that produces events. |
| `wa_record_status` | Report the state, event count, and elapsed time of the active recording session. |
| `wa_record_stop` | Stop the active recording session and optionally persist it as a WA semantic script artifact. |
| `wa_recovery_adaptive_wait` | Feed observed element ready-times to the adaptive wait estimator and get the recommended poll interval. |
| `wa_recovery_get_status` | Get the current recovery/circuit-breaker status for WA operations. |
| `wa_recovery_plan` | Return the ordered recovery action plan for a known automation failure scenario. |
| `wa_recovery_set_policy` | Configure the retry/recovery policy for WA operations (max retries, backoff, circuit breaker). |
| `wa_registry_delete` | Delete a Windows registry value. |
| `wa_registry_enumerate` | Enumerate the values and subkeys under a registry key. |
| `wa_registry_exists` | Check whether a registry key (and optionally a specific value under it) exists. |
| `wa_registry_read` | Read a Windows registry value. |
| `wa_registry_write` | Write a Windows registry value. |
| `wa_resolve_css_selector` | Resolve a CSS-style selector against a saved WA semantic snapshot and return matching nodes. |
| `wa_resolve_selector` | Resolve a deterministic WA selector against a saved semantic snapshot. |
| `wa_resolve_xpath` | Resolve an XPath expression against a saved WA semantic snapshot and return matching nodes. |
| `wa_run_script` | Run a saved WA semantic script deterministically against the Windows automation layer. |
| `wa_save_script` | Persist a deterministic WA semantic script artifact with NDA sidecar. |
| `wa_save_snapshot` | Persist a WA semantic snapshot with compact node metadata and NDA sidecar. |
| `wa_screenshot` | Capture the full screen, a single window by PID, or a screen region, and persist it in the format implied by the output file extension. |
| `wa_system_dark_mode` | Get or toggle Windows dark mode. |
| `wa_system_dpi` | Query the current system DPI and the equivalent scale percentage. |
| `wa_tray_click` | _see source_ |
| `wa_tray_list` | List system tray (notification area) icons with their tooltips and owning processes. |
| `wa_trigger_fire` | Manually fire a registered trigger by ID, executing its action script immediately. |
| `wa_trigger_list` | List all registered automation triggers. |
| `wa_trigger_register` | Register a new automation trigger (file watch, window appears, idle detect, etc.). |
| `wa_trigger_remove` | Remove a registered automation trigger by ID. |
| `wa_uac_detect` | Detect whether a UAC (User Account Control) consent prompt is currently visible. |
| `wa_uia_invoke` | Invoke a UIAutomation pattern (e.g. Invoke, Value, Toggle) on an element targeted by automationId, name, or x+y. |
| `wa_uia_lookup` | Look up a UIAutomation element in a process's cached tree by automationId, name, or screen point (x+y). |
| `wa_uia_root` | Read the root UIAutomation element for a process or the desktop, with explicit control over the cached tree. |
| `wa_uia_tree` | Build the cached UIAutomation tree for a process via direct COM (fast path) and report its size. |
| `wa_vdesktop_create` | _see source_ |
| `wa_vdesktop_move_window` | _see source_ |
| `wa_vdesktop_remove` | _see source_ |
| `wa_vdesktop_window_info` | Report which virtual desktop a window belongs to and whether it is on the current desktop. |
| `wa_virtual_desktop_list` | List all Windows virtual desktops. |
| `wa_virtual_desktop_switch` | _see source_ |
| `wa_wait_for_windows_condition` | Wait for a deterministic Windows UIAutomation condition against a saved WA snapshot. |
| `wa_window_action` | _see source_ |
| `wa_window_foreground` | Return the current foreground (focused) desktop window, if any. |
| `wa_window_list` | List visible desktop windows with titles, positions, and states. |
| `wa_window_tile` | Tile an explicitly named set of windows in a grid on one monitor. `hwnds` is required: this tool will not enumerate and rearrange every window on the desktop. Handles must come from wa_window_list. |
