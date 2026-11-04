//! Claude Code subscription limits, read with the CLI's existing OAuth login.
//!
//! The credentials file is only ever read: tokens are never refreshed or written back,
//! so the CLI keeps sole ownership of its login.

use super::http::get_json;
use super::{FetchError, ModelKind, Provider, ProviderUsage, WindowUsage};
use crate::timeutil::parse_rfc3339;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

pub struct ClaudeCreds {
    pub access_token: String,
    pub expires_at_ms: Option<i64>,
}

impl std::fmt::Debug for ClaudeCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeCreds")
            .field("access_token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

pub fn parse_claude_creds(text: &str) -> Result<ClaudeCreds, FetchError> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| FetchError::NoLogin)?;
    let oauth = &v["claudeAiOauth"];
    let token = oauth["accessToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or(FetchError::NoLogin)?;
    Ok(ClaudeCreds {
        access_token: token.to_owned(),
        expires_at_ms: oauth["expiresAt"].as_i64(),
    })
}

fn parse_window(v: &Value) -> Option<WindowUsage> {
    let pct = v.get("utilization")?.as_f64()? as f32;
    Some(WindowUsage {
        used_pct: pct.clamp(0.0, 100.0),
        resets_at: v
            .get("resets_at")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339),
    })
}

fn parse_model_windows(v: &Value) -> Vec<(ModelKind, WindowUsage)> {
    [
        ("seven_day_opus", ModelKind::Opus),
        ("seven_day_sonnet", ModelKind::Sonnet),
    ]
    .into_iter()
    .filter_map(|(field, model)| parse_window(&v[field]).map(|window| (model, window)))
    .collect()
}

/// Reads reported windows without inventing absent values; e.g. `parse_claude_usage(&response)`.
pub fn parse_claude_usage(v: &Value) -> Result<ProviderUsage, FetchError> {
    let usage = ProviderUsage {
        five_hour: parse_window(&v["five_hour"]),
        weekly: parse_window(&v["seven_day"]),
        weekly_by_model: parse_model_windows(v),
        extra_usage_enabled: v["extra_usage"]["is_enabled"].as_bool() == Some(true),
    };
    if usage.five_hour.is_none()
        && usage.weekly.is_none()
        && usage.weekly_by_model.is_empty()
        && !usage.extra_usage_enabled
        && v.get("five_hour").is_none()
    {
        return Err(FetchError::BadResponse("unexpected usage format".into()));
    }
    Ok(usage)
}

pub struct ClaudeProvider {
    pub creds_path: PathBuf,
}

impl ClaudeProvider {
    /// `%USERPROFILE%\.claude\.credentials.json`.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join(".claude").join(".credentials.json")
    }

    fn read_creds(&self) -> Result<ClaudeCreds, FetchError> {
        let text = std::fs::read_to_string(&self.creds_path).map_err(|_| FetchError::NoLogin)?;
        parse_claude_creds(&text)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Provider for ClaudeProvider {
    fn fetch(&self) -> Result<ProviderUsage, FetchError> {
        let creds = self.read_creds()?;
        if creds.expires_at_ms.is_some_and(|exp| exp <= now_ms()) {
            return Err(FetchError::Expired);
        }
        let auth = format!("Bearer {}", creds.access_token);
        let v = get_json(
            USAGE_URL,
            &[
                ("Authorization", &auth),
                ("anthropic-beta", "oauth-2025-04-20"),
            ],
        )?;
        parse_claude_usage(&v)
    }

    fn has_login(&self) -> bool {
        self.read_creds().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{FetchError, Provider, WindowUsage};
    use serde_json::json;

    const CREDS: &str = include_str!("../../tests/fixtures/claude_credentials.json");
    const USAGE: &str = include_str!("../../tests/fixtures/claude_usage.json");

    #[test]
    fn creds_ok() {
        let c = parse_claude_creds(CREDS).unwrap();
        assert_eq!(c.access_token, "sk-ant-oat01-FAKE");
        assert_eq!(c.expires_at_ms, Some(1893456000000));
    }

    #[test]
    fn creds_with_bom() {
        let text = format!("\u{feff}{CREDS}");
        assert_eq!(
            parse_claude_creds(&text).unwrap().access_token,
            "sk-ant-oat01-FAKE"
        );
    }

    #[test]
    fn creds_missing_key_is_no_login() {
        assert_eq!(parse_claude_creds("{}").unwrap_err(), FetchError::NoLogin);
        assert_eq!(
            parse_claude_creds(r#"{"claudeAiOauth":{"accessToken":""}}"#).unwrap_err(),
            FetchError::NoLogin
        );
    }

    #[test]
    fn creds_debug_redacts_token() {
        let c = parse_claude_creds(CREDS).unwrap();
        assert!(!format!("{c:?}").contains("FAKE"));
    }

    #[test]
    fn creds_empty_file_is_no_login() {
        assert_eq!(parse_claude_creds("").unwrap_err(), FetchError::NoLogin);
    }

    #[test]
    fn usage_fixture_parses() {
        let v: serde_json::Value = serde_json::from_str(USAGE).unwrap();
        let u = parse_claude_usage(&v).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 18.0,
                resets_at: Some(1793381399)
            })
        );
        assert_eq!(
            u.weekly,
            Some(WindowUsage {
                used_pct: 3.0,
                resets_at: Some(1793948399)
            })
        );
    }

    #[test]
    fn usage_clamps_over_100() {
        let v = json!({"five_hour": {"utilization": 130, "resets_at": null}, "seven_day": null});
        let u = parse_claude_usage(&v).unwrap();
        assert_eq!(
            u.five_hour,
            Some(WindowUsage {
                used_pct: 100.0,
                resets_at: None
            })
        );
    }

    #[test]
    fn usage_null_weekly() {
        let v = json!({"five_hour": {"utilization": 5.5, "resets_at": "2026-10-30T15:00:00Z"}, "seven_day": null});
        let u = parse_claude_usage(&v).unwrap();
        assert_eq!(u.weekly, None);
        assert!(u.five_hour.is_some());
    }

    #[test]
    fn usage_models_preserve_identity_zero_and_reset() {
        let fixture = include_str!("../../tests/fixtures/claude_usage_models.json");
        let u = parse_claude_usage(&serde_json::from_str(fixture).unwrap()).unwrap();
        assert_eq!(
            u.weekly_by_model,
            vec![
                (
                    ModelKind::Opus,
                    WindowUsage {
                        used_pct: 72.0,
                        resets_at: Some(1793948399)
                    }
                ),
                (
                    ModelKind::Sonnet,
                    WindowUsage {
                        used_pct: 0.0,
                        resets_at: None
                    }
                ),
            ]
        );
        assert!(u.extra_usage_enabled);
    }

    #[test]
    fn usage_models_omit_null_missing_and_invalid_windows() {
        for fields in [
            json!({}),
            json!({"seven_day_opus": null, "seven_day_sonnet": null}),
            json!({"seven_day_opus": {}, "seven_day_sonnet": {"utilization": "72"}}),
        ] {
            let mut v = fields;
            v["five_hour"] = json!(null);
            let u = parse_claude_usage(&v).unwrap();
            assert_eq!(u.weekly_by_model, vec![]);
            assert!(!u.extra_usage_enabled);
        }
        let u = parse_claude_usage(&serde_json::from_str(USAGE).unwrap()).unwrap();
        assert!(u.weekly_by_model.is_empty());
        assert!(!u.extra_usage_enabled);
    }

    #[test]
    fn usage_models_clamp_percentages() {
        let v = json!({"five_hour": null, "seven_day_opus": {"utilization": 150},
            "seven_day_sonnet": {"utilization": -5}});
        let u = parse_claude_usage(&v).unwrap();
        assert_eq!(
            u.weekly_by_model,
            vec![
                (
                    ModelKind::Opus,
                    WindowUsage {
                        used_pct: 100.0,
                        resets_at: None
                    }
                ),
                (
                    ModelKind::Sonnet,
                    WindowUsage {
                        used_pct: 0.0,
                        resets_at: None
                    }
                ),
            ]
        );
    }

    #[test]
    fn extra_usage_requires_explicit_enabled_boolean() {
        for extra in [
            json!(null),
            json!({}),
            json!({"is_enabled": false}),
            json!({"is_enabled": "true"}),
        ] {
            let v = json!({"five_hour": null, "extra_usage": extra});
            assert!(!parse_claude_usage(&v).unwrap().extra_usage_enabled);
        }
    }

    #[test]
    fn usage_accepts_model_only_response() {
        let v = json!({"seven_day_opus": {"utilization": 72}});
        let u = parse_claude_usage(&v).unwrap();
        assert_eq!(
            u.weekly_by_model,
            vec![(
                ModelKind::Opus,
                WindowUsage {
                    used_pct: 72.0,
                    resets_at: None
                }
            )]
        );
        assert_eq!(u.five_hour, None);
        assert_eq!(u.weekly, None);
    }

    #[test]
    fn usage_accepts_extra_usage_only_response() {
        let u = parse_claude_usage(&json!({"extra_usage": {"is_enabled": true}})).unwrap();
        assert!(u.extra_usage_enabled);
        assert!(u.weekly_by_model.is_empty());
        assert_eq!(u.five_hour, None);
        assert_eq!(u.weekly, None);
    }

    #[test]
    fn usage_garbage_is_bad_response() {
        assert!(matches!(
            parse_claude_usage(&json!({"x": 1})),
            Err(FetchError::BadResponse(_))
        ));
    }

    #[test]
    fn missing_file_is_no_login() {
        let p = ClaudeProvider {
            creds_path: std::env::temp_dir().join("wuw-no-such-creds.json"),
        };
        assert!(!p.has_login());
        assert_eq!(p.fetch().unwrap_err(), FetchError::NoLogin);
    }

    #[test]
    fn expired_token_skips_network() {
        let path =
            std::env::temp_dir().join(format!("wuw-claude-expired-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"x","expiresAt":1000}}"#,
        )
        .unwrap();
        let p = ClaudeProvider { creds_path: path };
        assert!(p.has_login());
        assert_eq!(p.fetch().unwrap_err(), FetchError::Expired);
    }

    #[test]
    #[ignore = "calls the real endpoint with the local login"]
    fn live_claude() {
        let p = ClaudeProvider {
            creds_path: ClaudeProvider::default_path(),
        };
        let u = p.fetch().expect("live fetch");
        assert!(u.five_hour.is_some() || u.weekly.is_some());
    }
}
