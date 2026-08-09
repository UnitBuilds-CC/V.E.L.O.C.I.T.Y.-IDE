//! Pre-built integration templates for common workflows.
//!
//! Templates provide ready-made connector configurations and sync rules
//! for popular services, reducing setup to a single step.

use serde::{Deserialize, Serialize};

use super::sync::{SyncDirection, SyncRule};
use super::types::{AuthScheme, ConnectorConfig, ConnectorKind};
use super::webhooks::{OutgoingWebhook, WebhookEvent};

/// A complete integration template for a service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationTemplate {
    /// Template ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Service category.
    pub category: IntegrationCategory,
    /// Connector configuration template.
    pub connector: ConnectorTemplate,
    /// Sync rules included with this template.
    pub sync_rules: Vec<SyncRuleTemplate>,
    /// Webhooks included with this template.
    pub webhooks: Vec<WebhookTemplate>,
    /// Required credentials (handles into the secret store).
    pub required_secrets: Vec<String>,
    /// Setup instructions.
    pub setup_instructions: String,
}

/// Service categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationCategory {
    SourceControl,
    ProjectManagement,
    Communication,
    Documentation,
    CI_CD,
    Monitoring,
    Custom,
}

impl IntegrationCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SourceControl => "source_control",
            Self::ProjectManagement => "project_management",
            Self::Communication => "communication",
            Self::Documentation => "documentation",
            Self::CI_CD => "ci_cd",
            Self::Monitoring => "monitoring",
            Self::Custom => "custom",
        }
    }
}

/// Template for creating a connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorTemplate {
    pub kind: ConnectorKind,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub default_headers: Vec<(String, String)>,
}

/// Template for creating a sync rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRuleTemplate {
    pub name: String,
    pub resource_type: String,
    pub direction: SyncDirection,
    pub poll_interval_secs: u64,
    pub field_mappings: Vec<(String, String)>,
}

/// Template for creating a webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTemplate {
    pub name: String,
    pub events: Vec<String>,
    pub direction: WebhookDirection,
}

/// Whether a webhook template is incoming or outgoing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WebhookDirection {
    Incoming,
    Outgoing,
}

impl IntegrationTemplate {
    /// Instantiate this template into a concrete ConnectorConfig.
    pub fn instantiate_connector(&self, id: &str, name: &str, secret_handle: Option<String>) -> ConnectorConfig {
        ConnectorConfig {
            id: id.to_string(),
            name: name.to_string(),
            kind: self.connector.kind,
            base_url: self.connector.base_url.clone(),
            auth_secret: secret_handle,
            auth: self.connector.auth_scheme.clone(),
            headers: self.connector.default_headers.clone(),
        }
    }

    /// Instantiate sync rules from this template.
    pub fn instantiate_sync_rules(&self, connector_id: &str) -> Vec<SyncRule> {
        self.sync_rules.iter().enumerate().map(|(i, tmpl)| {
            SyncRule {
                id: format!("{}_{}", connector_id, i),
                name: tmpl.name.clone(),
                connector_id: connector_id.to_string(),
                direction: tmpl.direction,
                resource_type: tmpl.resource_type.clone(),
                poll_interval_secs: tmpl.poll_interval_secs,
                last_sync: None,
                enabled: false,
                field_mappings: tmpl.field_mappings.clone(),
                filter: None,
            }
        }).collect()
    }

    /// Instantiate outgoing webhooks from this template.
    pub fn instantiate_outgoing_webhooks(&self, connector_id: &str, url: &str) -> Vec<OutgoingWebhook> {
        self.webhooks.iter()
            .filter(|w| matches!(w.direction, WebhookDirection::Outgoing))
            .enumerate()
            .map(|(i, tmpl)| {
                OutgoingWebhook {
                    id: format!("{}_wh_{}", connector_id, i),
                    name: tmpl.name.clone(),
                    url: url.to_string(),
                    events: tmpl.events.iter().map(|e| WebhookEvent::from_label(e)).collect(),
                    secret_handle: None,
                    headers: Vec::new(),
                    enabled: false,
                    fire_count: 0,
                    last_fired: None,
                    last_status: None,
                }
            })
            .collect()
    }
}

/// Get all available integration templates.
pub fn all_templates() -> Vec<IntegrationTemplate> {
    vec![
        github_template(),
        gitlab_template(),
        jira_template(),
        slack_template(),
        discord_template(),
        notion_template(),
    ]
}

/// Find a template by ID.
pub fn find_template(id: &str) -> Option<IntegrationTemplate> {
    all_templates().into_iter().find(|t| t.id == id)
}

// ── Template Definitions ──

fn github_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "github".to_string(),
        name: "GitHub".to_string(),
        description: "Full GitHub integration: issues, PRs, webhooks for CI/CD events".to_string(),
        category: IntegrationCategory::SourceControl,
        connector: ConnectorTemplate {
            kind: ConnectorKind::GitHub,
            base_url: "https://api.github.com".to_string(),
            auth_scheme: AuthScheme::Bearer,
            default_headers: vec![
                ("Accept".to_string(), "application/vnd.github+json".to_string()),
                ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
            ],
        },
        sync_rules: vec![
            SyncRuleTemplate {
                name: "Issues".to_string(),
                resource_type: "issues".to_string(),
                direction: SyncDirection::BiDirectional,
                poll_interval_secs: 300,
                field_mappings: vec![
                    ("title".to_string(), "title".to_string()),
                    ("body".to_string(), "body".to_string()),
                    ("status".to_string(), "state".to_string()),
                    ("labels".to_string(), "labels".to_string()),
                    ("assignees".to_string(), "assignees".to_string()),
                ],
            },
            SyncRuleTemplate {
                name: "Pull Requests".to_string(),
                resource_type: "pull_requests".to_string(),
                direction: SyncDirection::PullOnly,
                poll_interval_secs: 120,
                field_mappings: vec![
                    ("title".to_string(), "title".to_string()),
                    ("status".to_string(), "state".to_string()),
                    ("branch".to_string(), "head_ref".to_string()),
                    ("reviewers".to_string(), "requested_reviewers".to_string()),
                ],
            },
        ],
        webhooks: vec![
            WebhookTemplate {
                name: "GitHub Events".to_string(),
                events: vec!["workflow.completed".to_string(), "workflow.failed".to_string()],
                direction: WebhookDirection::Outgoing,
            },
        ],
        required_secrets: vec!["github_token".to_string()],
        setup_instructions: "1. Go to GitHub Settings > Developer Settings > Personal Access Tokens\n\
            2. Create a token with 'repo' and 'read:org' scopes\n\
            3. Store the token as 'github_token' in the secret store"
            .to_string(),
    }
}

fn gitlab_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "gitlab".to_string(),
        name: "GitLab".to_string(),
        description: "GitLab integration: issues, merge requests, CI/CD pipelines".to_string(),
        category: IntegrationCategory::SourceControl,
        connector: ConnectorTemplate {
            kind: ConnectorKind::GitLab,
            base_url: "https://gitlab.com/api/v4".to_string(),
            auth_scheme: AuthScheme::Header { name: "PRIVATE-TOKEN".to_string() },
            default_headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
        },
        sync_rules: vec![
            SyncRuleTemplate {
                name: "Issues".to_string(),
                resource_type: "issues".to_string(),
                direction: SyncDirection::BiDirectional,
                poll_interval_secs: 300,
                field_mappings: vec![
                    ("title".to_string(), "title".to_string()),
                    ("body".to_string(), "description".to_string()),
                    ("status".to_string(), "state".to_string()),
                    ("labels".to_string(), "labels".to_string()),
                ],
            },
            SyncRuleTemplate {
                name: "Merge Requests".to_string(),
                resource_type: "merge_requests".to_string(),
                direction: SyncDirection::PullOnly,
                poll_interval_secs: 120,
                field_mappings: vec![
                    ("title".to_string(), "title".to_string()),
                    ("status".to_string(), "state".to_string()),
                    ("branch".to_string(), "source_branch".to_string()),
                ],
            },
        ],
        webhooks: vec![
            WebhookTemplate {
                name: "GitLab Events".to_string(),
                events: vec!["build.completed".to_string(), "build.failed".to_string()],
                direction: WebhookDirection::Outgoing,
            },
        ],
        required_secrets: vec!["gitlab_token".to_string()],
        setup_instructions: "1. Go to GitLab > User Settings > Access Tokens\n\
            2. Create a token with 'api' scope\n\
            3. Store the token as 'gitlab_token' in the secret store"
            .to_string(),
    }
}

fn jira_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "jira".to_string(),
        name: "Jira".to_string(),
        description: "Jira Cloud integration: sync issues, epics, and sprints".to_string(),
        category: IntegrationCategory::ProjectManagement,
        connector: ConnectorTemplate {
            kind: ConnectorKind::Jira,
            base_url: "https://api.atlassian.com/ex/jira/{cloud_id}".to_string(),
            auth_scheme: AuthScheme::Bearer,
            default_headers: vec![
                ("Accept".to_string(), "application/json".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
        },
        sync_rules: vec![
            SyncRuleTemplate {
                name: "Issues".to_string(),
                resource_type: "issues".to_string(),
                direction: SyncDirection::BiDirectional,
                poll_interval_secs: 180,
                field_mappings: vec![
                    ("title".to_string(), "fields.summary".to_string()),
                    ("body".to_string(), "fields.description".to_string()),
                    ("status".to_string(), "fields.status.name".to_string()),
                    ("priority".to_string(), "fields.priority.name".to_string()),
                    ("assignee".to_string(), "fields.assignee.displayName".to_string()),
                ],
            },
            SyncRuleTemplate {
                name: "Sprints".to_string(),
                resource_type: "sprints".to_string(),
                direction: SyncDirection::PullOnly,
                poll_interval_secs: 600,
                field_mappings: vec![
                    ("name".to_string(), "name".to_string()),
                    ("status".to_string(), "state".to_string()),
                    ("start_date".to_string(), "startDate".to_string()),
                    ("end_date".to_string(), "endDate".to_string()),
                ],
            },
        ],
        webhooks: vec![],
        required_secrets: vec!["jira_token".to_string()],
        setup_instructions: "1. Go to Atlassian Account Settings > Security > API Tokens\n\
            2. Create an API token\n\
            3. Store the token as 'jira_token' in the secret store\n\
            4. Find your Cloud ID at https://api.atlassian.com/oauth2/accessible-resources"
            .to_string(),
    }
}

fn slack_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "slack".to_string(),
        name: "Slack".to_string(),
        description: "Slack integration: send notifications and receive events".to_string(),
        category: IntegrationCategory::Communication,
        connector: ConnectorTemplate {
            kind: ConnectorKind::Slack,
            base_url: "https://slack.com/api".to_string(),
            auth_scheme: AuthScheme::Bearer,
            default_headers: Vec::new(),
        },
        sync_rules: vec![],
        webhooks: vec![
            WebhookTemplate {
                name: "Critical Alerts".to_string(),
                events: vec!["agent.critical_alert".to_string()],
                direction: WebhookDirection::Outgoing,
            },
            WebhookTemplate {
                name: "Build Status".to_string(),
                events: vec![
                    "build.completed".to_string(),
                    "build.failed".to_string(),
                ],
                direction: WebhookDirection::Outgoing,
            },
            WebhookTemplate {
                name: "Task Updates".to_string(),
                events: vec![
                    "task.started".to_string(),
                    "task.completed".to_string(),
                ],
                direction: WebhookDirection::Outgoing,
            },
        ],
        required_secrets: vec!["slack_token".to_string()],
        setup_instructions: "1. Go to https://api.slack.com/apps\n\
            2. Create a new app with a Bot Token\n\
            3. Install to workspace\n\
            4. Store the Bot User OAuth Token as 'slack_token'"
            .to_string(),
    }
}

fn discord_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "discord".to_string(),
        name: "Discord".to_string(),
        description: "Discord integration: webhook notifications for build and task events".to_string(),
        category: IntegrationCategory::Communication,
        connector: ConnectorTemplate {
            kind: ConnectorKind::Discord,
            base_url: "https://discord.com/api/v10".to_string(),
            auth_scheme: AuthScheme::Bearer,
            default_headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
        },
        sync_rules: vec![],
        webhooks: vec![
            WebhookTemplate {
                name: "Build Notifications".to_string(),
                events: vec![
                    "build.completed".to_string(),
                    "build.failed".to_string(),
                ],
                direction: WebhookDirection::Outgoing,
            },
            WebhookTemplate {
                name: "Agent Alerts".to_string(),
                events: vec!["agent.critical_alert".to_string()],
                direction: WebhookDirection::Outgoing,
            },
        ],
        required_secrets: vec!["discord_bot_token".to_string()],
        setup_instructions: "1. Go to https://discord.com/developers/applications\n\
            2. Create a new application and bot\n\
            3. Copy the bot token\n\
            4. Store the token as 'discord_bot_token' in the secret store"
            .to_string(),
    }
}

fn notion_template() -> IntegrationTemplate {
    IntegrationTemplate {
        id: "notion".to_string(),
        name: "Notion".to_string(),
        description: "Notion integration: sync pages and databases for documentation".to_string(),
        category: IntegrationCategory::Documentation,
        connector: ConnectorTemplate {
            kind: ConnectorKind::Notion,
            base_url: "https://api.notion.com/v1".to_string(),
            auth_scheme: AuthScheme::Bearer,
            default_headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Notion-Version".to_string(), "2022-06-28".to_string()),
            ],
        },
        sync_rules: vec![
            SyncRuleTemplate {
                name: "Pages".to_string(),
                resource_type: "pages".to_string(),
                direction: SyncDirection::BiDirectional,
                poll_interval_secs: 600,
                field_mappings: vec![
                    ("title".to_string(), "properties.title.title[0].plain_text".to_string()),
                    ("content".to_string(), "blocks".to_string()),
                    ("last_edited".to_string(), "last_edited_time".to_string()),
                ],
            },
            SyncRuleTemplate {
                name: "Databases".to_string(),
                resource_type: "databases".to_string(),
                direction: SyncDirection::PullOnly,
                poll_interval_secs: 900,
                field_mappings: vec![
                    ("title".to_string(), "title[0].plain_text".to_string()),
                    ("properties".to_string(), "properties".to_string()),
                ],
            },
        ],
        webhooks: vec![],
        required_secrets: vec!["notion_token".to_string()],
        setup_instructions: "1. Go to https://www.notion.so/my-integrations\n\
            2. Create a new integration\n\
            3. Copy the Internal Integration Token\n\
            4. Store the token as 'notion_token' in the secret store\n\
            5. Share your pages/databases with the integration"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_available() {
        let templates = all_templates();
        assert_eq!(templates.len(), 6);
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"github"));
        assert!(ids.contains(&"gitlab"));
        assert!(ids.contains(&"jira"));
        assert!(ids.contains(&"slack"));
        assert!(ids.contains(&"discord"));
        assert!(ids.contains(&"notion"));
    }

    #[test]
    fn find_template_by_id() {
        let t = find_template("github").unwrap();
        assert_eq!(t.name, "GitHub");
        assert_eq!(t.category, IntegrationCategory::SourceControl);
    }

    #[test]
    fn find_template_not_found() {
        assert!(find_template("nonexistent").is_none());
    }

    #[test]
    fn instantiate_connector() {
        let t = find_template("github").unwrap();
        let cfg = t.instantiate_connector("gh1", "My GitHub", Some("gh_token".to_string()));
        assert_eq!(cfg.id, "gh1");
        assert_eq!(cfg.kind, ConnectorKind::GitHub);
        assert_eq!(cfg.base_url, "https://api.github.com");
        assert_eq!(cfg.auth_secret.as_deref(), Some("gh_token"));
    }

    #[test]
    fn instantiate_sync_rules() {
        let t = find_template("github").unwrap();
        let rules = t.instantiate_sync_rules("gh1");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].connector_id, "gh1");
        assert_eq!(rules[0].resource_type, "issues");
        assert_eq!(rules[1].resource_type, "pull_requests");
    }

    #[test]
    fn instantiate_outgoing_webhooks() {
        let t = find_template("slack").unwrap();
        let whs = t.instantiate_outgoing_webhooks("sl1", "https://hooks.slack.com/xxx");
        assert_eq!(whs.len(), 3);
        assert_eq!(whs[0].url, "https://hooks.slack.com/xxx");
    }

    #[test]
    fn category_labels() {
        assert_eq!(IntegrationCategory::SourceControl.label(), "source_control");
        assert_eq!(IntegrationCategory::Communication.label(), "communication");
        assert_eq!(IntegrationCategory::Documentation.label(), "documentation");
    }

    #[test]
    fn templates_have_required_secrets() {
        for t in all_templates() {
            assert!(!t.required_secrets.is_empty(), "Template '{}' should have required secrets", t.id);
        }
    }

    #[test]
    fn templates_have_setup_instructions() {
        for t in all_templates() {
            assert!(!t.setup_instructions.is_empty(), "Template '{}' should have setup instructions", t.id);
        }
    }

    #[test]
    fn jira_template_has_cloud_id_placeholder() {
        let t = find_template("jira").unwrap();
        assert!(t.connector.base_url.contains("{cloud_id}"));
    }
}
