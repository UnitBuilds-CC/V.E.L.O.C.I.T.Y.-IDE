use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum ServiceWorkerState {
    Installing,
    Installed,
    Activating,
    Activated,
    Redundant,
}

#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub url: String,
    pub status: u16,
    pub body: String,
}

pub struct CacheStorageEngine {
    pub caches: HashMap<String, HashMap<String, CachedResponse>>,
}

impl Default for CacheStorageEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheStorageEngine {
    pub fn new() -> Self {
        Self { caches: HashMap::new() }
    }

    pub fn put(&mut self, cache_name: &str, url: &str, status: u16, body: &str) {
        let cache = self.caches.entry(cache_name.to_string()).or_default();
        cache.insert(url.to_string(), CachedResponse {
            url: url.to_string(),
            status,
            body: body.to_string(),
        });
    }

    pub fn match_url(&self, cache_name: &str, url: &str) -> Option<&CachedResponse> {
        self.caches.get(cache_name).and_then(|c| c.get(url))
    }
}

pub struct ServiceWorkerManager {
    pub script_url: String,
    pub state: ServiceWorkerState,
    pub cache_storage: CacheStorageEngine,
}

impl ServiceWorkerManager {
    pub fn register(script_url: &str) -> Self {
        Self {
            script_url: script_url.to_string(),
            state: ServiceWorkerState::Activated,
            cache_storage: CacheStorageEngine::new(),
        }
    }
}
