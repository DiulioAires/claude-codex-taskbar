//! Minimal JSON GET used by the providers.

use super::FetchError;
use std::time::Duration;
use ureq::config::Config;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, RequestBuilder, typestate::WithoutBody};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Redirects are never followed: ureq would drop `Authorization` on a cross-host hop but keep
/// custom headers such as `ChatGPT-Account-Id`. A 3xx is returned as is and becomes an error.
fn agent_config() -> Config {
    Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
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
}

fn agent() -> Agent {
    agent_config().into()
}

/// 2xx is success, 401/403 mean expired credentials, anything else (3xx included) is an error.
fn map_status(status: u16) -> Result<(), FetchError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(FetchError::Expired),
        _ => Err(FetchError::BadResponse(format!("HTTP {status}"))),
    }
}

/// GETs `url` and parses JSON. 401/403 map to `Expired`. Error text never includes headers.
pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, FetchError> {
    send(agent().get(url), headers)
}

fn send(
    mut req: RequestBuilder<WithoutBody>,
    headers: &[(&str, &str)],
) -> Result<serde_json::Value, FetchError> {
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.call().map_err(|e| FetchError::Network(e.to_string()))?;
    map_status(resp.status().as_u16())?;
    resp.body_mut()
        .read_json::<serde_json::Value>()
        .map_err(|_| FetchError::BadResponse("invalid JSON".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const SECRET: &str = "acct-secret-123";

    #[test]
    fn agent_follows_no_redirects() {
        assert_eq!(agent_config().max_redirects(), 0);
    }

    #[test]
    fn agent_is_https_only() {
        assert!(agent_config().https_only());
    }

    #[test]
    fn redirect_statuses_are_bad_response() {
        for status in [300, 301, 302, 303, 307, 308] {
            assert_eq!(
                map_status(status),
                Err(FetchError::BadResponse(format!("HTTP {status}"))),
                "status {status}"
            );
        }
    }

    #[test]
    fn auth_statuses_are_expired() {
        assert_eq!(map_status(401), Err(FetchError::Expired));
        assert_eq!(map_status(403), Err(FetchError::Expired));
    }

    #[test]
    fn success_statuses_are_ok() {
        assert_eq!(map_status(200), Ok(()));
        assert_eq!(map_status(299), Ok(()));
    }

    #[test]
    fn other_statuses_are_bad_response() {
        for status in [100, 400, 404, 429, 500, 503] {
            assert_eq!(
                map_status(status),
                Err(FetchError::BadResponse(format!("HTTP {status}")))
            );
        }
    }

    /// Answers the first connection with `response` and returns the raw request it read.
    fn serve_once(listener: TcpListener, response: String) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = sock.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            sock.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&req).into_owned()
        })
    }

    fn assert_no_connection(listener: &TcpListener) {
        listener.set_nonblocking(true).unwrap();
        match listener.accept() {
            Err(e) => assert_eq!(e.kind(), ErrorKind::WouldBlock),
            Ok(_) => panic!("a connection reached the listener"),
        }
    }

    #[test]
    fn plain_http_is_refused_before_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usage", listener.local_addr().unwrap());

        let err = get_json(&url, &[("ChatGPT-Account-Id", SECRET)]).unwrap_err();

        assert!(matches!(err, FetchError::Network(_)), "{err:?}");
        assert!(!format!("{err:?}").contains(SECRET));
        assert_no_connection(&listener);
    }

    #[test]
    fn redirect_is_not_followed_and_header_does_not_leak() {
        // Cross-host target: a second listener that must never be contacted.
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_url = format!("http://{}/stolen", target.local_addr().unwrap());
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin_url = format!("http://{}/usage", origin.local_addr().unwrap());
        let server = serve_once(
            origin,
            format!(
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );

        // Production config, with only HTTPS relaxed so a plain local server can answer.
        let req = agent().get(&origin_url).config().https_only(false).build();
        let err = send(req, &[("ChatGPT-Account-Id", SECRET)]).unwrap_err();

        assert_eq!(err, FetchError::BadResponse("HTTP 302".into()));
        assert!(!format!("{err:?}").contains(SECRET));
        let seen = server.join().unwrap();
        assert!(
            seen.contains(SECRET),
            "origin should get the header: {seen}"
        );
        assert_no_connection(&target);
    }
}
