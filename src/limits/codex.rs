//! Codex subscription limits, read with the CLI's existing ChatGPT login.
//!
//! `auth.json` is only ever read. When the endpoint fails, the newest `rate_limits`
//! event the CLI wrote to its session logs is used instead, as long as it has not reset.

use super::http::get_json;
use super::{FetchError, Provider, ProviderUsage, WindowUsage};
use crate::timeutil::{now_unix, parse_rfc3339};
use serde_json::Value;
use std::path::{Path, PathBuf};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub struct CodexCreds {
    pub access_token: String,
    pub account_id: Option<String>,
}

impl std::fmt::Debug for CodexCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexCreds")
            .field("access_token", &"<redacted>")
            .field(
                "account_id",
                &self.account_id.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

pub fn parse_codex_auth(text: &str) -> Result<CodexCreds, FetchError> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| FetchError::NoLogin)?;
    let token = v["tokens"]["access_token"]
        .as_str()
        .filter(|t| !t.is_empty());
    match token {
        Some(token) => Ok(CodexCreds {
            access_token: token.to_owned(),
            account_id: v["tokens"]["account_id"].as_str().map(str::to_owned),
        }),
        None if v["OPENAI_API_KEY"].as_str().is_some_and(|k| !k.is_empty()) => {
            Err(FetchError::NoSubscription)
        }
        None => Err(FetchError::NoLogin),
    }
}

fn window(pct: Option<f64>, resets_at: Option<i64>) -> Option<WindowUsage> {
    Some(WindowUsage {
        used_pct: (pct? as f32).clamp(0.0, 100.0),
        resets_at,
    })
}

/// Reported duration takes precedence over the legacy primary/secondary ordering.
fn assign_window(
    usage: &mut ProviderUsage,
    reading: Option<WindowUsage>,
    seconds: Option<i64>,
    secondary: bool,
) {
    let slot = match seconds {
        Some(18000) => &mut usage.five_hour,
        Some(604800) => &mut usage.weekly,
        None if secondary => &mut usage.weekly,
        None => &mut usage.five_hour,
        Some(_) => return,
    };
    if reading.is_some() {
        *slot = reading;
    }
}

/// `now` resolves the relative `reset_after_seconds` field.
pub fn parse_codex_usage(v: &Value, now: i64) -> Result<ProviderUsage, FetchError> {
    let rl = v
        .get("rate_limit")
        .filter(|r| r.is_object())
        .ok_or_else(|| FetchError::BadResponse("unexpected usage format".into()))?;
    let parse = |w: &Value| {
        let resets_at = w["reset_at"]
            .as_i64()
            .or_else(|| w["reset_after_seconds"].as_i64().map(|s| now + s));
        window(w["used_percent"].as_f64(), resets_at)
    };
    let mut usage = ProviderUsage {
        five_hour: None,
        weekly: None,
    };
    for (name, secondary) in [("primary_window", false), ("secondary_window", true)] {
        let w = &rl[name];
        assign_window(
            &mut usage,
            parse(w),
            w["limit_window_seconds"].as_i64(),
            secondary,
        );
    }
    Ok(usage)
}

/// Parses one session-log line; `None` unless it carries `payload.rate_limits`.
fn session_line_limits(line: &str) -> Option<(ProviderUsage, i64)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let rl = v["payload"].get("rate_limits").filter(|r| r.is_object())?;
    let at = v["timestamp"].as_str().and_then(parse_rfc3339)?;
    let parse = |w: &Value| {
        let resets_at = w["resets_at"]
            .as_i64()
            .or_else(|| w["resets_in_seconds"].as_i64().map(|s| at + s));
        window(w["used_percent"].as_f64(), resets_at)
    };
    let mut usage = ProviderUsage {
        five_hour: None,
        weekly: None,
    };
    for (name, secondary) in [("primary", false), ("secondary", true)] {
        let w = &rl[name];
        let seconds = w["window_minutes"].as_i64().map(|m| m.saturating_mul(60));
        assign_window(&mut usage, parse(w), seconds, secondary);
    }
    Some((usage, at))
}

/// Newest-first walk of `sessions/YYYY/MM/DD/*.jsonl`; stops at the first file with limits.
fn newest_session_limits(dir: &Path) -> Option<(ProviderUsage, i64)> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| {
        let modified = e.metadata().and_then(|m| m.modified()).ok();
        std::cmp::Reverse((modified, e.file_name()))
    });
    for entry in entries {
        let path = entry.path();
        let found = if path.is_dir() {
            newest_session_limits(&path)
        } else if path.extension().is_some_and(|x| x == "jsonl") {
            std::fs::read_to_string(&path).ok().and_then(|text| {
                text.lines()
                    .rev()
                    .filter(|l| l.contains("\"rate_limits\""))
                    .find_map(session_line_limits)
            })
        } else {
            None
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Last limits the CLI logged, with windows whose reset time has passed removed.
pub fn latest_session_limits(sessions_dir: &Path, now: i64) -> Option<(ProviderUsage, i64)> {
    let (mut usage, at) = newest_session_limits(sessions_dir)?;
    let still_valid = |w: &Option<WindowUsage>| {
        w.as_ref()
            .and_then(|w| w.resets_at)
            .is_some_and(|r| r > now)
    };
    if !still_valid(&usage.five_hour) {
        usage.five_hour = None;
    }
    if !still_valid(&usage.weekly) {
        usage.weekly = None;
    }
    if usage.five_hour.is_none() && usage.weekly.is_none() {
        return None;
    }
    Some((usage, at))
}

pub struct CodexProvider {
    pub home: PathBuf,
}

/// Result of a Codex fetch: usage, whether it came from session logs, and its timestamp.
pub type CodexFetch = (ProviderUsage, bool, i64);

impl CodexProvider {
    /// `%CODEX_HOME%`, or `%USERPROFILE%\.codex`.
    pub fn default_home() -> PathBuf {
        if let Some(home) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
            return PathBuf::from(home);
        }
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default();
        profile.join(".codex")
    }

    fn read_creds(&self) -> Result<CodexCreds, FetchError> {
        let text = std::fs::read_to_string(self.home.join("auth.json"))
            .map_err(|_| FetchError::NoLogin)?;
        parse_codex_auth(&text)
    }

    /// Falls back to session logs only for transient failures, never for auth problems.
    pub fn fetch_with_fallback(&self, now: i64) -> Result<CodexFetch, FetchError> {
        match self.fetch_at(now) {
            Ok(usage) => Ok((usage, false, now)),
            Err(err @ (FetchError::Network(_) | FetchError::BadResponse(_))) => {
                latest_session_limits(&self.home.join("sessions"), now)
                    .map(|(usage, at)| (usage, true, at))
                    .ok_or(err)
            }
            Err(err) => Err(err),
        }
    }

    fn fetch_at(&self, now: i64) -> Result<ProviderUsage, FetchError> {
        let creds = self.read_creds()?;
        let auth = format!("Bearer {}", creds.access_token);
        let mut headers = vec![("Authorization", auth.as_str())];
        if let Some(id) = &creds.account_id {
            headers.push(("ChatGPT-Account-Id", id.as_str()));
        }
        parse_codex_usage(&get_json(USAGE_URL, &headers)?, now)
    }
}

impl Provider for CodexProvider {
    fn fetch(&self) -> Result<ProviderUsage, FetchError> {
        self.fetch_at(now_unix())
    }

    fn has_login(&self) -> bool {
        self.read_creds().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{FetchError, WindowUsage};
    use crate::timeutil::parse_rfc3339;
    use serde_json::json;
    use std::path::PathBuf;

    const AUTH: &str = include_str!("../../tests/fixtures/codex_auth.json");
    const USAGE: &str = include_str!("../../tests/fixtures/codex_usage.json");
    const SESSION: &str = include_str!("../../tests/fixtures/codex_session.jsonl");

    fn sessions_dir(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("wuw-codex-{}-{name}", std::process::id()))
            .join("sessions");
        let day = dir.join("2026").join("10").join("26");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-FAKE.jsonl"), content).unwrap();
        dir
    }

    #[test]
    fn auth_ok() {
        let c = parse_codex_auth(AUTH).unwrap();
        assert_eq!(c.access_token, "FAKE-ACCESS");
        assert_eq!(
            c.account_id.as_deref(),
            Some("00000000-0000-0000-0000-000000000000")
        );
        assert!(!format!("{c:?}").contains("FAKE-ACCESS"));
    }

    #[test]
    fn auth_with_bom() {
        assert!(parse_codex_auth(&format!("\u{feff}{AUTH}")).is_ok());
    }

    #[test]
    fn auth_api_key_only_is_no_subscription() {
        let text = r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-FAKE","tokens":null}"#;
        assert_eq!(
            parse_codex_auth(text).unwrap_err(),
            FetchError::NoSubscription
        );
    }

    #[test]
    fn auth_empty_is_no_login() {
        assert_eq!(parse_codex_auth("").unwrap_err(), FetchError::NoLogin);
        assert_eq!(parse_codex_auth("{}").unwrap_err(), FetchError::NoLogin);
    }

    #[test]
    fn usage_fixture_parses() {
        let v: serde_json::Value = serde_json::from_str(USAGE).unwrap();
        let u = parse_codex_usage(&v, 0).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 12.0,
                resets_at: Some(1793385254)
            })
        );
        assert_eq!(
            u.weekly,
            Some(WindowUsage {
                used_pct: 40.0,
                resets_at: Some(1793972054)
            })
        );
    }

    #[test]
    fn usage_primary_weekly_is_not_five_hour() {
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 55, "limit_window_seconds": 604800, "reset_at": 1794267982i64},
            "secondary_window": null
        }});
        let u = parse_codex_usage(&v, 0).unwrap();
        assert_eq!(u.five_hour, None);
        assert_eq!(
            u.weekly,
            Some(WindowUsage {
                used_pct: 55.0,
                resets_at: Some(1794267982)
            })
        );
    }

    #[test]
    fn usage_window_duration_overrides_position() {
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 55, "limit_window_seconds": 604800},
            "secondary_window": {"used_percent": 12, "limit_window_seconds": 18000}
        }});
        let u = parse_codex_usage(&v, 0).unwrap();
        assert_eq!(u.five_hour.unwrap().used_pct, 12.0);
        assert_eq!(u.weekly.unwrap().used_pct, 55.0);
    }

    #[test]
    fn usage_unknown_duration_is_not_mislabeled() {
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 55, "limit_window_seconds": 3600}
        }});
        let u = parse_codex_usage(&v, 0).unwrap();
        assert_eq!(u.five_hour, None);
        assert_eq!(u.weekly, None);
    }

    #[test]
    fn session_primary_weekly_is_not_five_hour() {
        let line = r#"{"timestamp":"2026-10-26T15:56:19Z","payload":{"rate_limits":{"primary":{"used_percent":55,"window_minutes":10080,"resets_at":1794267982},"secondary":null}}}"#;
        let (u, _) = session_line_limits(line).unwrap();
        assert_eq!(u.five_hour, None);
        assert_eq!(
            u.weekly,
            Some(WindowUsage {
                used_pct: 55.0,
                resets_at: Some(1794267982)
            })
        );
    }

    #[test]
    fn usage_reset_after_seconds() {
        let v = json!({"rate_limit": {"primary_window": {"used_percent": 5, "reset_after_seconds": 600}}});
        let u = parse_codex_usage(&v, 1000).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 5.0,
                resets_at: Some(1600)
            })
        );
        assert_eq!(u.weekly, None);
    }

    #[test]
    fn usage_without_rate_limit_is_bad_response() {
        assert!(matches!(
            parse_codex_usage(&json!({"plan_type": "free"}), 0),
            Err(FetchError::BadResponse(_))
        ));
    }

    #[test]
    fn session_fallback_picks_last_line() {
        let dir = sessions_dir("last", SESSION);
        let (u, at) = latest_session_limits(&dir, 1793000000).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 44.0,
                resets_at: Some(1793035631)
            })
        );
        assert_eq!(
            u.weekly,
            Some(WindowUsage {
                used_pct: 7.0,
                resets_at: Some(1793622431)
            })
        );
        assert_eq!(at, parse_rfc3339("2026-10-26T15:56:19Z").unwrap());
    }

    #[test]
    fn session_fallback_drops_expired_window() {
        let dir = sessions_dir("expired", SESSION);
        let (u, _) = latest_session_limits(&dir, 1793100000).unwrap();
        assert_eq!(u.five_hour, None);
        assert!(u.weekly.is_some());
    }

    #[test]
    fn session_fallback_all_expired_is_none() {
        let dir = sessions_dir("allexpired", SESSION);
        assert!(latest_session_limits(&dir, 1799999999).is_none());
    }

    #[test]
    fn session_fallback_missing_dir_is_none() {
        assert!(
            latest_session_limits(&std::env::temp_dir().join("wuw-nope-sessions"), 0).is_none()
        );
    }

    #[test]
    fn session_fallback_resets_in_seconds() {
        let line = r#"{"timestamp":"2026-10-26T15:56:19Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":300,"resets_in_seconds":100}}}}"#;
        let dir = sessions_dir("relative", line);
        let ts = parse_rfc3339("2026-10-26T15:56:19Z").unwrap();
        let (u, _) = latest_session_limits(&dir, ts).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 10.0,
                resets_at: Some(ts + 100)
            })
        );
    }

    #[test]
    #[ignore = "calls the real endpoint with the local login"]
    fn live_codex() {
        let p = CodexProvider {
            home: CodexProvider::default_home(),
        };
        let u = crate::limits::Provider::fetch(&p).expect("live fetch");
        assert!(u.five_hour.is_some() || u.weekly.is_some());
    }
}
