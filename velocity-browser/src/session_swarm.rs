use crate::net::tls::{ProxyResolver, ProxyType};
use crate::session::BrowserSession;

/// Health status of a swarm session.
#[derive(Debug, Clone, PartialEq)]
pub enum SwarmHealth {
    Healthy,
    Degraded,
    Unresponsive,
    Terminated,
}

/// A swarm session with metadata for orchestration.
pub struct SwarmEntry {
    pub health: SwarmHealth,
    pub task_label: String,
    pub started_at_ms: u64,
    pub last_active_ms: u64,
}

/// Swarm orchestrator managing multiple browser sessions with health checks and load balancing.
pub struct SwarmSessionOrchestrator {
    pub swarm_sessions: Vec<BrowserSession>,
    entries: Vec<SwarmEntry>,
    max_concurrent: usize,
    now_ms: u64,
}

impl Default for SwarmSessionOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl SwarmSessionOrchestrator {
    pub fn new() -> Self {
        Self {
            swarm_sessions: Vec::new(),
            entries: Vec::new(),
            max_concurrent: 16,
            now_ms: 0,
        }
    }

    /// Set the max concurrent sessions.
    pub fn set_max_concurrent(&mut self, max: usize) {
        self.max_concurrent = max;
    }

    /// Spawn a new swarm tab with a task label.
    pub fn spawn_swarm_tab(&mut self, session_id: &str) -> &mut BrowserSession {
        let session = BrowserSession::new(session_id.to_string());
        self.swarm_sessions.push(session);
        let idx = self.swarm_sessions.len() - 1;
        &mut self.swarm_sessions[idx]
    }

    /// Spawn a swarm tab with task metadata.
    pub fn spawn_with_task(
        &mut self,
        session_id: &str,
        task_label: &str,
    ) -> Result<usize, &'static str> {
        if self.active_swarm_count() >= self.max_concurrent {
            return Err("Max concurrent swarm sessions reached");
        }
        let session = BrowserSession::new(session_id.to_string());
        self.swarm_sessions.push(session);
        let idx = self.swarm_sessions.len() - 1;
        self.entries.push(SwarmEntry {
            health: SwarmHealth::Healthy,
            task_label: task_label.to_string(),
            started_at_ms: self.now_ms,
            last_active_ms: self.now_ms,
        });
        Ok(idx)
    }

    /// Get active swarm count.
    pub fn active_swarm_count(&self) -> usize {
        self.swarm_sessions.len()
    }

    /// Terminate a swarm session by index.
    pub fn terminate(&mut self, idx: usize) -> bool {
        if idx < self.swarm_sessions.len() {
            self.swarm_sessions.remove(idx);
            if idx < self.entries.len() {
                self.entries.remove(idx);
            }
            true
        } else {
            false
        }
    }

    /// Perform health check on all swarm sessions.
    pub fn health_check(&mut self) -> Vec<(usize, SwarmHealth)> {
        let mut results = Vec::new();
        for (i, session) in self.swarm_sessions.iter().enumerate() {
            let health = if session.current_url.is_empty() {
                SwarmHealth::Degraded
            } else {
                SwarmHealth::Healthy
            };
            results.push((i, health));
        }
        results
    }

    /// Find the least-loaded session for task distribution.
    pub fn least_loaded(&self) -> Option<usize> {
        if self.swarm_sessions.is_empty() {
            return None;
        }
        // Simple heuristic: session with shortest URL (least activity)
        self.swarm_sessions
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.current_url.len())
            .map(|(i, _)| i)
    }

    /// Get all session IDs.
    pub fn session_ids(&self) -> Vec<&str> {
        self.swarm_sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect()
    }

    /// Look up a swarm session by its session id.
    pub fn get_session(&self, session_id: &str) -> Option<&BrowserSession> {
        self.swarm_sessions
            .iter()
            .find(|s| s.session_id == session_id)
    }

    /// Mutable lookup of a swarm session by its session id.
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut BrowserSession> {
        self.swarm_sessions
            .iter_mut()
            .find(|s| s.session_id == session_id)
    }

    /// Remove a swarm session by id, dropping its metadata entry with it.
    pub fn remove_session(&mut self, session_id: &str) -> bool {
        match self
            .swarm_sessions
            .iter()
            .position(|s| s.session_id == session_id)
        {
            Some(idx) => self.terminate(idx),
            None => false,
        }
    }

    /// Broadcast a URL to all swarm sessions.
    pub fn broadcast_navigate(&mut self, url: &str) {
        for session in &mut self.swarm_sessions {
            session.current_url = url.to_string();
        }
    }

    /// Set a shared proxy for all current and future swarm sessions.
    pub fn set_proxy_for_all(&mut self, proxy_type: ProxyType) {
        for session in &mut self.swarm_sessions {
            let resolver = ProxyResolver {
                proxy_type: proxy_type.clone(),
            };
            session.set_proxy(resolver);
        }
    }

    /// Spawn a swarm tab with a preconfigured proxy.
    pub fn spawn_with_proxy(
        &mut self,
        session_id: &str,
        proxy_type: ProxyType,
    ) -> &mut BrowserSession {
        let mut session = BrowserSession::new(session_id.to_string());
        let resolver = ProxyResolver { proxy_type };
        session.set_proxy(resolver);
        self.swarm_sessions.push(session);
        let idx = self.swarm_sessions.len() - 1;
        &mut self.swarm_sessions[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1");
        assert_eq!(swarm.active_swarm_count(), 1);
    }

    #[test]
    fn test_terminate() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1");
        swarm.spawn_swarm_tab("tab2");
        assert!(swarm.terminate(0));
        assert_eq!(swarm.active_swarm_count(), 1);
    }

    #[test]
    fn test_health_check() {
        let mut swarm = SwarmSessionOrchestrator::new();
        let tab = swarm.spawn_swarm_tab("tab1");
        tab.current_url = "https://example.com".to_string();
        let results = swarm.health_check();
        assert_eq!(results[0].1, SwarmHealth::Healthy);
    }

    #[test]
    fn test_health_degraded() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1"); // no URL = degraded
        let results = swarm.health_check();
        assert_eq!(results[0].1, SwarmHealth::Degraded);
    }

    #[test]
    fn test_least_loaded() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1");
        swarm.spawn_swarm_tab("tab2");
        let idx = swarm.least_loaded();
        assert!(idx.is_some());
    }

    #[test]
    fn test_broadcast() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1");
        swarm.spawn_swarm_tab("tab2");
        swarm.broadcast_navigate("https://example.com");
        for s in &swarm.swarm_sessions {
            assert_eq!(s.current_url, "https://example.com");
        }
    }

    #[test]
    fn test_session_ids() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("a");
        swarm.spawn_swarm_tab("b");
        let ids = swarm.session_ids();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_max_concurrent() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.set_max_concurrent(2);
        assert!(swarm.spawn_with_task("a", "task1").is_ok());
    }

    #[test]
    fn test_set_proxy_for_all() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab1");
        swarm.spawn_swarm_tab("tab2");
        swarm.set_proxy_for_all(ProxyType::Socks5("127.0.0.1:1080".to_string()));
        for s in &swarm.swarm_sessions {
            assert!(matches!(s.proxy_resolver.proxy_type, ProxyType::Socks5(_)));
        }
    }

    #[test]
    fn test_spawn_with_proxy() {
        let mut swarm = SwarmSessionOrchestrator::new();
        let session =
            swarm.spawn_with_proxy("proxy_tab", ProxyType::Http("proxy.local:8080".to_string()));
        assert!(matches!(
            session.proxy_resolver.proxy_type,
            ProxyType::Http(_)
        ));
    }

    #[test]
    fn test_get_session_by_id() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab-a");
        swarm.spawn_swarm_tab("tab-b");
        assert!(swarm.get_session("tab-b").is_some());
        assert!(swarm.get_session("missing").is_none());
        let tab = swarm.get_session_mut("tab-a").unwrap();
        tab.current_url = "https://a.test".to_string();
        assert_eq!(
            swarm.get_session("tab-a").unwrap().current_url,
            "https://a.test"
        );
    }

    #[test]
    fn test_remove_session_by_id() {
        let mut swarm = SwarmSessionOrchestrator::new();
        swarm.spawn_swarm_tab("tab-a");
        swarm.spawn_swarm_tab("tab-b");
        assert!(swarm.remove_session("tab-a"));
        assert!(!swarm.remove_session("tab-a"), "second removal is a no-op");
        assert_eq!(swarm.session_ids(), vec!["tab-b"]);
    }
}
