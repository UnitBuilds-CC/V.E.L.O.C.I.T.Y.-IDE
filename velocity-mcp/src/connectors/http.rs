//! HTTP request assembly and execution for connectors.
//!
//! [`build_request`] is a *pure* function: it takes a [`ConnectorConfig`], a
//! per-call [`ConnectorRequest`], and an already-resolved secret value, and
//! produces a fully-formed [`PreparedRequest`] (absolute URL, merged headers,
//! injected auth). It performs no I/O so it can be unit-tested without a
//! network. [`execute`] then performs the actual `ureq` call.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::types::{AuthScheme, ConnectorConfig};

/// A single call made through a connector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorRequest {
    /// HTTP method (`GET`, `POST`, ...). Case-insensitive; defaults to `GET`.
    #[serde(default)]
    pub method: String,
    /// Path appended to the connector's base URL (leading slash optional).
    #[serde(default)]
    pub path: String,
    /// Extra headers for this call, layered on top of the connector's statics.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Optional request body sent verbatim (JSON string, form data, ...).
    #[serde(default)]
    pub body: Option<String>,
}

/// A fully-resolved request ready to be sent. Produced by [`build_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// The outcome of an executed request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorResponse {
    pub status: u16,
    pub body: String,
}

/// Join a base URL and a path with exactly one separating slash.
fn join_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if path.is_empty() {
        return base.to_string();
    }
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

/// Assemble a [`PreparedRequest`] from static + per-call inputs.
///
/// `secret` is the plaintext credential already resolved from the secret store
/// (or `None`). If the connector's [`AuthScheme`] needs a secret but none was
/// provided, this returns an error rather than sending an unauthenticated call.
pub fn build_request(
    config: &ConnectorConfig,
    req: &ConnectorRequest,
    secret: Option<&str>,
) -> Result<PreparedRequest, String> {
    let method = if req.method.trim().is_empty() {
        "GET".to_string()
    } else {
        req.method.trim().to_uppercase()
    };

    let mut url = join_url(&config.base_url, &req.path);

    // Static connector headers first, then per-call headers (which win on dupes
    // is left to the server; both are sent in order).
    let mut headers: Vec<(String, String)> = config.headers.clone();
    headers.extend(req.headers.iter().cloned());

    if config.requires_secret() {
        let secret = secret.ok_or_else(|| {
            format!(
                "connector '{}' requires secret handle '{}' but it was not resolved",
                config.id,
                config.auth_secret.as_deref().unwrap_or("<none>")
            )
        })?;
        match &config.auth {
            AuthScheme::None => {}
            AuthScheme::Bearer => {
                headers.push(("Authorization".to_string(), format!("Bearer {secret}")));
            }
            AuthScheme::Header { name } => {
                headers.push((name.clone(), secret.to_string()));
            }
            AuthScheme::Query { param } => {
                let sep = if url.contains('?') { '&' } else { '?' };
                url.push(sep);
                url.push_str(&format!("{param}={secret}"));
            }
        }
    }

    Ok(PreparedRequest {
        method,
        url,
        headers,
        body: req.body.clone(),
    })
}

/// Execute a prepared request over the network via `ureq`.
pub fn execute(prepared: &PreparedRequest) -> Result<ConnectorResponse, String> {
    let mut request = ureq::request(&prepared.method, &prepared.url)
        .timeout(Duration::from_secs(30));
    for (name, value) in &prepared.headers {
        request = request.set(name, value);
    }

    let response = match &prepared.body {
        Some(body) => request.send_string(body),
        None => request.call(),
    };

    match response {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            Ok(ConnectorResponse { status, body })
        }
        // `ureq` models non-2xx as `Error::Status`; surface those as a normal
        // response so callers can inspect the status/body rather than failing.
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Ok(ConnectorResponse { status, body })
        }
        Err(ureq::Error::Transport(t)) => Err(format!("transport error: {t}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::types::ConnectorConfig;

    #[test]
    fn join_url_normalizes_slashes() {
        assert_eq!(join_url("https://x.com/", "/a/b"), "https://x.com/a/b");
        assert_eq!(join_url("https://x.com", "a/b"), "https://x.com/a/b");
        assert_eq!(join_url("https://x.com/", ""), "https://x.com");
    }

    #[test]
    fn build_request_defaults_method_to_get() {
        let cfg = ConnectorConfig::generic("g", "Generic", "https://api.example.com");
        let req = ConnectorRequest {
            path: "/status".to_string(),
            ..Default::default()
        };
        let prepared = build_request(&cfg, &req, None).unwrap();
        assert_eq!(prepared.method, "GET");
        assert_eq!(prepared.url, "https://api.example.com/status");
        assert!(prepared.headers.is_empty());
    }

    #[test]
    fn build_request_injects_bearer_and_merges_headers() {
        let cfg = ConnectorConfig::github("gh", "GitHub", Some("gh_token".to_string()));
        let req = ConnectorRequest {
            method: "post".to_string(),
            path: "repos/o/r/issues".to_string(),
            headers: vec![("X-Extra".to_string(), "1".to_string())],
            body: Some("{}".to_string()),
        };
        let prepared = build_request(&cfg, &req, Some("secret123")).unwrap();
        assert_eq!(prepared.method, "POST");
        assert_eq!(prepared.url, "https://api.github.com/repos/o/r/issues");
        // static github headers + per-call header + injected auth
        assert!(prepared
            .headers
            .contains(&("Accept".to_string(), "application/vnd.github+json".to_string())));
        assert!(prepared
            .headers
            .contains(&("X-Extra".to_string(), "1".to_string())));
        assert!(prepared.headers.contains(&(
            "Authorization".to_string(),
            "Bearer secret123".to_string()
        )));
        assert_eq!(prepared.body.as_deref(), Some("{}"));
    }

    #[test]
    fn build_request_header_scheme_uses_named_header() {
        let mut cfg = ConnectorConfig::generic("k", "Keyed", "https://api.example.com");
        cfg.auth = AuthScheme::Header {
            name: "x-api-key".to_string(),
        };
        cfg.auth_secret = Some("api".to_string());
        let req = ConnectorRequest {
            path: "/v1/thing".to_string(),
            ..Default::default()
        };
        let prepared = build_request(&cfg, &req, Some("KEYVAL")).unwrap();
        assert!(prepared
            .headers
            .contains(&("x-api-key".to_string(), "KEYVAL".to_string())));
    }

    #[test]
    fn build_request_query_scheme_appends_param() {
        let mut cfg = ConnectorConfig::generic("q", "Queried", "https://api.example.com");
        cfg.auth = AuthScheme::Query {
            param: "token".to_string(),
        };
        cfg.auth_secret = Some("t".to_string());
        let req = ConnectorRequest {
            path: "/search?q=rust".to_string(),
            ..Default::default()
        };
        let prepared = build_request(&cfg, &req, Some("TOK")).unwrap();
        assert_eq!(
            prepared.url,
            "https://api.example.com/search?q=rust&token=TOK"
        );
    }

    #[test]
    fn build_request_errors_when_secret_missing() {
        let cfg = ConnectorConfig::github("gh", "GitHub", Some("gh_token".to_string()));
        let req = ConnectorRequest::default();
        let err = build_request(&cfg, &req, None).unwrap_err();
        assert!(err.contains("requires secret"));
    }
}
