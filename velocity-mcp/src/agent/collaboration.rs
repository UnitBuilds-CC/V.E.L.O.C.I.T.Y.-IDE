//! Multi-user collaboration: identity, sessions, and presence.
//!
//! Manages user identities, shared agent sessions, and real-time presence
//! tracking so multiple users can collaborate on agent tasks simultaneously.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A user in the collaboration system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique user ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Role in the team.
    pub role: TeamRole,
    /// When this user was created.
    pub created_at: u64,
    /// Last seen timestamp.
    pub last_seen: u64,
    /// Whether this user is currently online.
    pub online: bool,
    /// User's current activity description.
    pub activity: Option<String>,
}

/// Team roles with different permission levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamRole {
    /// Full access: manage team, workflows, and agents.
    Owner,
    /// Can create/edit workflows and run agents.
    Admin,
    /// Can run agents and view workflows.
    Editor,
    /// Can view sessions and agent output.
    Viewer,
}

impl TeamRole {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }

    /// Check if this role has a specific permission.
    pub fn has_permission(&self, perm: Permission) -> bool {
        match (self, perm) {
            // Owner can do everything.
            (Self::Owner, _) => true,
            // Admin can manage workflows, run agents, view.
            (Self::Admin, Permission::ManageWorkflows) => true,
            (Self::Admin, Permission::RunAgents) => true,
            (Self::Admin, Permission::ViewSessions) => true,
            (Self::Admin, Permission::ManageTeam) => false,
            // Editor can run agents and view.
            (Self::Editor, Permission::RunAgents) => true,
            (Self::Editor, Permission::ViewSessions) => true,
            (Self::Editor, Permission::ManageWorkflows) => false,
            (Self::Editor, Permission::ManageTeam) => false,
            // Viewer can only view.
            (Self::Viewer, Permission::ViewSessions) => true,
            (Self::Viewer, _) => false,
        }
    }
}

/// Permissions that can be checked for team actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Create/edit/delete workflows.
    ManageWorkflows,
    /// Start and control agent sessions.
    RunAgents,
    /// View shared sessions and agent output.
    ViewSessions,
    /// Manage team members and roles.
    ManageTeam,
}

/// A shared agent session that multiple users can observe/participate in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSession {
    /// Unique session ID.
    pub id: String,
    /// Session name/description.
    pub name: String,
    /// User who created this session.
    pub owner_id: String,
    /// Users currently in this session.
    pub participants: Vec<String>,
    /// When the session was created.
    pub created_at: u64,
    /// Session status.
    pub status: SessionStatus,
    /// Chat messages in this session.
    pub messages: Vec<SessionMessage>,
    /// Maximum messages to retain.
    pub max_messages: usize,
}

/// Status of a shared session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is being prepared.
    Draft,
    /// Session is active with agent running.
    Active,
    /// Session is paused.
    Paused,
    /// Session has completed.
    Completed,
    /// Session was abandoned.
    Abandoned,
}

impl SessionStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// A message in a shared session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Unique message ID.
    pub id: String,
    /// Who sent this message (user ID or "agent").
    pub sender_id: String,
    /// Message content.
    pub content: String,
    /// When this message was sent.
    pub timestamp: u64,
    /// Message type.
    pub kind: MessageKind,
}

/// Type of session message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageKind {
    /// A user chat message.
    UserChat,
    /// An agent response.
    AgentResponse,
    /// A system notification.
    System,
    /// An action was taken (e.g., file edited).
    Action,
}

/// Manages all collaboration state for a workspace.
#[derive(Debug, Clone, Default)]
pub struct CollaborationManager {
    /// Registered users keyed by ID.
    pub users: HashMap<String, User>,
    /// Shared sessions keyed by ID.
    pub sessions: HashMap<String, SharedSession>,
    /// Presence: user_id -> last heartbeat timestamp.
    pub presence: HashMap<String, u64>,
    /// Presence timeout (seconds). Users not seen within this are offline.
    pub presence_timeout_secs: u64,
}

impl CollaborationManager {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            sessions: HashMap::new(),
            presence: HashMap::new(),
            presence_timeout_secs: 300, // 5 minutes
        }
    }

    // ── User Management ──

    /// Register a new user.
    pub fn add_user(&mut self, user: User) {
        self.users.insert(user.id.clone(), user);
    }

    /// Remove a user.
    pub fn remove_user(&mut self, id: &str) -> bool {
        self.presence.remove(id);
        self.users.remove(id).is_some()
    }

    /// Get a user by ID.
    pub fn get_user(&self, id: &str) -> Option<&User> {
        self.users.get(id)
    }

    /// Check if a user has a specific permission.
    pub fn check_permission(&self, user_id: &str, perm: Permission) -> bool {
        self.users
            .get(user_id)
            .map(|u| u.role.has_permission(perm))
            .unwrap_or(false)
    }

    /// List all users.
    pub fn list_users(&self) -> Vec<&User> {
        self.users.values().collect()
    }

    // ── Presence ──

    /// Record a heartbeat for a user.
    pub fn heartbeat(&mut self, user_id: &str) {
        let now = now_secs();
        self.presence.insert(user_id.to_string(), now);
        if let Some(user) = self.users.get_mut(user_id) {
            user.online = true;
            user.last_seen = now;
        }
    }

    /// Update presence based on heartbeat timeouts.
    pub fn update_presence(&mut self) {
        let now = now_secs();
        let timeout = self.presence_timeout_secs;
        for (user_id, last_heartbeat) in &self.presence {
            if let Some(user) = self.users.get_mut(user_id) {
                user.online = now - last_heartbeat < timeout;
            }
        }
    }

    /// Get currently online users.
    pub fn online_users(&self) -> Vec<&User> {
        self.users.values().filter(|u| u.online).collect()
    }

    // ── Session Management ──

    /// Create a new shared session.
    pub fn create_session(&mut self, owner_id: &str, name: &str) -> Result<String, String> {
        if !self.check_permission(owner_id, Permission::RunAgents) {
            return Err("User does not have permission to create sessions".to_string());
        }

        let id = format!("sess_{}_{}", now_secs(), self.sessions.len());
        let session = SharedSession {
            id: id.clone(),
            name: name.to_string(),
            owner_id: owner_id.to_string(),
            participants: vec![owner_id.to_string()],
            created_at: now_secs(),
            status: SessionStatus::Draft,
            messages: Vec::new(),
            max_messages: 500,
        };
        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    /// Join a shared session.
    pub fn join_session(&mut self, user_id: &str, session_id: &str) -> Result<(), String> {
        if !self.check_permission(user_id, Permission::ViewSessions) {
            return Err("User does not have permission to view sessions".to_string());
        }

        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session '{}' not found", session_id))?;

        if !session.participants.contains(&user_id.to_string()) {
            session.participants.push(user_id.to_string());
        }
        Ok(())
    }

    /// Leave a shared session.
    pub fn leave_session(&mut self, user_id: &str, session_id: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.participants.retain(|p| p != user_id);
        }
    }

    /// Add a message to a session.
    pub fn send_message(
        &mut self,
        session_id: &str,
        sender_id: &str,
        content: &str,
        kind: MessageKind,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session '{}' not found", session_id))?;

        let msg = SessionMessage {
            id: format!("msg_{}_{}", now_secs(), session.messages.len()),
            sender_id: sender_id.to_string(),
            content: content.to_string(),
            timestamp: now_secs(),
            kind,
        };

        session.messages.push(msg);
        while session.messages.len() > session.max_messages {
            session.messages.remove(0);
        }
        Ok(())
    }

    /// Update session status.
    pub fn set_session_status(&mut self, session_id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.status = status;
        }
    }

    /// Get sessions a user is participating in.
    pub fn user_sessions(&self, user_id: &str) -> Vec<&SharedSession> {
        self.sessions
            .values()
            .filter(|s| s.participants.contains(&user_id.to_string()))
            .collect()
    }

    /// Get all active sessions.
    pub fn active_sessions(&self) -> Vec<&SharedSession> {
        self.sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .collect()
    }

    /// List all sessions.
    pub fn list_sessions(&self) -> Vec<&SharedSession> {
        self.sessions.values().collect()
    }

    // ── Persistence ──

    /// Save collaboration state to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedCollabState {
            users: self.users.values().cloned().collect(),
            sessions: self.sessions.values().cloned().collect(),
        };
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("collaboration.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load collaboration state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut mgr = Self::new();
        let path = workspace_root.join(".velocity").join("collaboration.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedCollabState>(&bytes) {
                for user in state.users {
                    mgr.users.insert(user.id.clone(), user);
                }
                for session in state.sessions {
                    mgr.sessions.insert(session.id.clone(), session);
                }
            }
        }
        mgr
    }
}

/// Serializable persistence for collaboration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedCollabState {
    users: Vec<User>,
    sessions: Vec<SharedSession>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user(id: &str, role: TeamRole) -> User {
        User {
            id: id.to_string(),
            name: format!("User {}", id),
            email: format!("{}@test.com", id),
            role,
            created_at: now_secs(),
            last_seen: now_secs(),
            online: false,
            activity: None,
        }
    }

    #[test]
    fn add_and_get_user() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Editor));
        assert!(mgr.get_user("u1").is_some());
        assert_eq!(mgr.get_user("u1").unwrap().name, "User u1");
    }

    #[test]
    fn remove_user() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Editor));
        assert!(mgr.remove_user("u1"));
        assert!(mgr.get_user("u1").is_none());
    }

    #[test]
    fn permission_checks() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("owner", TeamRole::Owner));
        mgr.add_user(test_user("viewer", TeamRole::Viewer));
        mgr.add_user(test_user("editor", TeamRole::Editor));

        assert!(mgr.check_permission("owner", Permission::ManageTeam));
        assert!(!mgr.check_permission("viewer", Permission::RunAgents));
        assert!(mgr.check_permission("editor", Permission::RunAgents));
        assert!(!mgr.check_permission("editor", Permission::ManageTeam));
    }

    #[test]
    fn heartbeat_and_presence() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Editor));
        mgr.heartbeat("u1");
        assert!(mgr.get_user("u1").unwrap().online);

        let online = mgr.online_users();
        assert_eq!(online.len(), 1);
    }

    #[test]
    fn create_session_requires_permission() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("viewer", TeamRole::Viewer));
        assert!(mgr.create_session("viewer", "Test").is_err());
    }

    #[test]
    fn create_and_join_session() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("owner", TeamRole::Admin));
        mgr.add_user(test_user("viewer", TeamRole::Viewer));

        let sess_id = mgr.create_session("owner", "Test Session").unwrap();
        assert!(mgr.join_session("viewer", &sess_id).is_ok());

        let sessions = mgr.user_sessions("viewer");
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn leave_session() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Editor));
        mgr.add_user(test_user("u2", TeamRole::Editor));

        let sess_id = mgr.create_session("u1", "Test").unwrap();
        mgr.join_session("u2", &sess_id).unwrap();
        mgr.leave_session("u2", &sess_id);

        let sessions = mgr.user_sessions("u2");
        assert_eq!(sessions.len(), 0);
    }

    #[test]
    fn send_message() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Editor));

        let sess_id = mgr.create_session("u1", "Chat").unwrap();
        mgr.send_message(&sess_id, "u1", "Hello!", MessageKind::UserChat)
            .unwrap();

        let session = mgr.sessions.get(&sess_id).unwrap();
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].content, "Hello!");
    }

    #[test]
    fn active_sessions_filter() {
        let mut mgr = CollaborationManager::new();
        mgr.add_user(test_user("u1", TeamRole::Admin));

        let s1 = mgr.create_session("u1", "S1").unwrap();
        let s2 = mgr.create_session("u1", "S2").unwrap();
        mgr.set_session_status(&s1, SessionStatus::Active);
        mgr.set_session_status(&s2, SessionStatus::Completed);

        assert_eq!(mgr.active_sessions().len(), 1);
    }

    #[test]
    fn team_role_permissions() {
        assert!(TeamRole::Owner.has_permission(Permission::ManageTeam));
        assert!(TeamRole::Admin.has_permission(Permission::ManageWorkflows));
        assert!(!TeamRole::Admin.has_permission(Permission::ManageTeam));
        assert!(TeamRole::Editor.has_permission(Permission::RunAgents));
        assert!(!TeamRole::Editor.has_permission(Permission::ManageWorkflows));
        assert!(TeamRole::Viewer.has_permission(Permission::ViewSessions));
        assert!(!TeamRole::Viewer.has_permission(Permission::RunAgents));
    }

    #[test]
    fn session_status_labels() {
        assert_eq!(SessionStatus::Draft.label(), "draft");
        assert_eq!(SessionStatus::Active.label(), "active");
        assert_eq!(SessionStatus::Paused.label(), "paused");
        assert_eq!(SessionStatus::Completed.label(), "completed");
    }
}
