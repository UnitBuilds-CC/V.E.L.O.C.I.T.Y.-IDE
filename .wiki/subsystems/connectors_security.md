# Connectors, Security & External Service Integration

The `connectors/` and `security/` modules within `velocity-mcp` provide external service integration (HTTP connectors, OAuth2, webhooks, sync rules) and encrypted credential storage with policy governance.

---

## External Service Connectors

### Module Structure (`connectors/`)

```
connectors/
├── mod.rs          # Connector trait, call_connector() entry point
├── http.rs         # HTTP request building and execution
├── oauth2.rs       # OAuth2 token management
├── registry.rs     # ConnectorRegistry: persisted connector configurations
├── sync.rs         # SyncEngine: bidirectional data synchronization
├── templates.rs    # Pre-built integration templates (GitHub, GitLab, Jira, etc.)
├── types.rs        # ConnectorConfig, AuthScheme, ConnectorKind
└── webhooks.rs     # WebhookManager: incoming webhook processing
```

### Connector Trait

```rust
pub trait Connector {
    fn prepare(&self, req: &ConnectorRequest, secret: Option<&str>)
        -> Result<PreparedRequest, String>;

    fn send(&self, req: &ConnectorRequest, secret: Option<&str>)
        -> Result<ConnectorResponse, String>;
}
```

Both preset templates and generic connectors implement the same `Connector` trait, so they share one code path.

### ConnectorConfig

```rust
pub struct ConnectorConfig {
    pub id: String,
    pub name: String,
    pub kind: ConnectorKind,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub auth_secret: Option<String>,  // Handle into SecretStore
    pub headers: HashMap<String, String>,
}

pub enum AuthScheme {
    None,
    Bearer,
    Basic,
    ApiKey { header: String },
    OAuth2 { provider: String },
}
```

### Integration Templates (`templates.rs`)

Pre-built connector templates for popular services:

| Template | Service | Auth |
|----------|---------|------|
| GitHub | GitHub API | Bearer token |
| GitLab | GitLab API | Bearer token |
| Jira | Atlassian Jira | OAuth2 |
| Azure DevOps | Azure DevOps | Bearer token |
| Slack | Slack API | Bearer token |
| Linear | Linear API | Bearer token |

### OAuth2 Manager (`oauth2.rs`)

```rust
pub struct OAuth2Manager {
    provider: OAuth2Provider,
    token: Option<OAuth2Token>,
}

pub struct OAuth2Provider {
    pub name: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

pub struct OAuth2Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}
```

Handles token acquisition, refresh, and expiry detection.

### Sync Engine (`sync.rs`)

Bidirectional data synchronization between the workspace and external services:

```rust
pub struct SyncRule {
    pub id: String,
    pub connector_id: String,
    pub direction: SyncDirection,
    pub local_path: PathBuf,
    pub remote_path: String,
    pub filter: Option<String>,
}

pub enum SyncDirection {
    LocalToRemote,
    RemoteToLocal,
    Bidirectional,
}
```

### Webhook Manager (`webhooks.rs`)

Processes incoming webhooks from external services:

```rust
pub struct WebhookManager { ... }

pub struct WebhookEvent {
    pub source: String,
    pub event_type: String,
    pub payload: Value,
    pub timestamp: u64,
}
```

---

## Security Subsystem

### Module Structure (`security/`)

```
security/
├── mod.rs              # Module root
└── secrets.rs          # SecretStore: encrypted credential storage
```

### SecretStore (`security/secrets.rs`)

Encrypted credential storage — secrets never touch disk in the clear:

```rust
pub struct SecretStore {
    entries: HashMap<String, String>,  // handle → encrypted value
}
```

**Encryption**:
- Workspace master key via `agent::crypto`
- Windows DPAPI-backed on Windows
- AES-256-GCM `NDA1` envelope encryption
- Secrets referenced by *handle* (name), never embedded in configs

### Security Model

```
Connector/Provider config
    │
    ├── auth_secret: Some("github_token")  ← handle name
    │
    ▼
SecretStore::load(workspace_root)
    │
    ├── Decrypt with workspace master key
    │
    ▼
Secret value resolved at call time
    │
    ├── Passed to Connector::prepare() as Option<&str>
    │
    ▼
HTTP request built with credential
```

**Key properties**:
- Secrets are encrypted at rest (AES-256-GCM)
- Master key is DPAPI-protected on Windows
- Credentials resolved by handle, not embedded
- No plaintext secrets in configuration files

---

## Call Connector Flow

```
1. Agent calls connector tool (e.g., "call_github_issue")
       │
       ▼
2. registry::call_tool_in_workspace() matches connector handler
       │
       ▼
3. call_connector(workspace_root, connector_id, request)
       │
       ├── ConnectorRegistry::load() → find config by id
       │
       ├── SecretStore::load() → resolve auth_secret handle
       │
       ├── config.prepare(request, secret) → build HTTP request
       │
       └── http::execute(prepared) → send and return response
       │
       ▼
4. Response returned as JSON to agent context
```

---

## See Also

- [MCP Tool Registry](mcp_tool_registry.md) — Tool dispatch and categories
- [NDA Format & Security Model](nda_security.md) — NDA binary security
- [velocity-mcp: Agent Loop & Orchestrator](../architecture/velocity_mcp.md) — Provider management
