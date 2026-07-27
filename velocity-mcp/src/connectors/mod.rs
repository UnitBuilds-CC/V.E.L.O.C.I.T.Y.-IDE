//! External service connectors.
//!
//! A connector is a persisted [`ConnectorConfig`] describing how to reach an
//! HTTP service. Requests are assembled by [`http::build_request`] (pure) and
//! sent by [`http::execute`]. Credentials are resolved *by handle* from the
//! encrypted [`SecretStore`](crate::security::secrets::SecretStore) at call
//! time and never persisted in plaintext.

pub mod http;
pub mod registry;
pub mod types;

use std::path::Path;

pub use http::{ConnectorRequest, ConnectorResponse, PreparedRequest};
pub use registry::ConnectorRegistry;
pub use types::{AuthScheme, ConnectorConfig};

use crate::security::secrets::SecretStore;

/// Behaviour common to all connectors: prepare and run a request.
///
/// Implemented for [`ConnectorConfig`] so presets and generic connectors share
/// one code path. `secret` is the already-resolved credential (or `None`).
pub trait Connector {
    fn prepare(
        &self,
        req: &ConnectorRequest,
        secret: Option<&str>,
    ) -> Result<PreparedRequest, String>;

    fn send(
        &self,
        req: &ConnectorRequest,
        secret: Option<&str>,
    ) -> Result<ConnectorResponse, String> {
        let prepared = self.prepare(req, secret)?;
        http::execute(&prepared)
    }
}

impl Connector for ConnectorConfig {
    fn prepare(
        &self,
        req: &ConnectorRequest,
        secret: Option<&str>,
    ) -> Result<PreparedRequest, String> {
        http::build_request(self, req, secret)
    }
}

/// High-level entry point: look up a configured connector by id, resolve its
/// secret from the encrypted store, and execute `req`.
pub fn call_connector(
    workspace_root: &Path,
    id: &str,
    req: &ConnectorRequest,
) -> Result<ConnectorResponse, String> {
    let registry = ConnectorRegistry::load(workspace_root);
    let config = registry
        .get(id)
        .ok_or_else(|| format!("unknown connector: {id}"))?;

    let secret = match &config.auth_secret {
        Some(handle) => {
            let store = SecretStore::load(workspace_root);
            let value = store
                .get(handle)
                .ok_or_else(|| format!("secret handle '{handle}' not found in store"))?;
            Some(value.to_string())
        }
        None => None,
    };

    config.send(req, secret.as_deref())
}
