use crate::nda::NdaTriple;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AuthTokenState {
    pub bearer_token: Option<String>,
    pub cookies: HashMap<String, String>,
    pub storage_keys: HashMap<String, String>,
}

pub struct AuthReseeder;

impl AuthReseeder {
    pub fn extract_auth_state(
        cookies: &[crate::session::Cookie],
        storage: &HashMap<String, String>,
    ) -> AuthTokenState {
        let mut cookie_map = HashMap::new();
        for c in cookies {
            cookie_map.insert(c.name.clone(), c.value.clone());
        }

        let bearer = storage
            .get("access_token")
            .or_else(|| storage.get("token"))
            .or_else(|| storage.get("auth_token"))
            .cloned();

        AuthTokenState {
            bearer_token: bearer,
            cookies: cookie_map,
            storage_keys: storage.clone(),
        }
    }

    pub fn reseed_into_session(
        session: &mut crate::session::BrowserSession,
        auth: &AuthTokenState,
    ) {
        for (k, v) in &auth.cookies {
            session.cookies.push(crate::session::Cookie {
                name: k.clone(),
                value: v.clone(),
                domain: String::new(),
                path: "/".to_string(),
                expires: 0.0,
                http_only: false,
                secure: true,
            });
        }
        for (k, v) in &auth.storage_keys {
            session.storage.insert(k.clone(), v.clone());
        }
    }

    pub fn export_auth_nda(auth: &AuthTokenState, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        if let Some(token) = &auth.bearer_token {
            triples.push(NdaTriple::new(session_id, 130, token));
        }
        for (k, v) in &auth.cookies {
            triples.push(NdaTriple::new(k, 131, v));
        }
        triples
    }
}
