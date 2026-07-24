use crate::nda::NdaTriple;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub resource_type: String,
}

pub struct NetworkTracker {
    pub requests: Vec<NetworkRequest>,
    pub headers: HashMap<String, String>,
    pub redirects: Vec<String>,
    pub downloads: Vec<String>,
}

impl Default for NetworkTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkTracker {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            headers: HashMap::new(),
            redirects: Vec::new(),
            downloads: Vec::new(),
        }
    }

    pub fn record_request(&mut self, url: &str, method: &str, status: u16, resource_type: &str) {
        self.requests.push(NetworkRequest {
            url: url.to_string(),
            method: method.to_string(),
            status,
            resource_type: resource_type.to_string(),
        });
    }

    pub fn export_triples_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(self.requests.len() * 2);
        for req in &self.requests {
            triples.push(NdaTriple::new(&req.url, 200, &req.method));
            triples.push(NdaTriple::new(&req.url, 201, &req.status.to_string()));
        }
        triples
    }
}
