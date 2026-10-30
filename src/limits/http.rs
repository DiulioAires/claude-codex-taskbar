//! Minimal JSON GET used by the providers.

use super::FetchError;
use std::time::Duration;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

const TIMEOUT: Duration = Duration::from_secs(10);

fn agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .user_agent(concat!("claude-codex-taskbar/", env!("CARGO_PKG_VERSION")))
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

/// GETs `url` and parses JSON. 401/403 map to `Expired`. Error text never includes headers.
pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, FetchError> {
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.call().map_err(|e| FetchError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    match status {
        200..=299 => resp
            .body_mut()
            .read_json::<serde_json::Value>()
            .map_err(|_| FetchError::BadResponse("invalid JSON".into())),
        401 | 403 => Err(FetchError::Expired),
        _ => Err(FetchError::BadResponse(format!("HTTP {status}"))),
    }
}
