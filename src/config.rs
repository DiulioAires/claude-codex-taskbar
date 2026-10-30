//! User configuration persisted as JSON under %APPDATA%.

use crate::model::ItemId;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "widget-usage-windows";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub hw_interval_secs: u32,
    pub limits_interval_secs: u32,
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    /// Keys from [`item_key`] of items the user turned off.
    pub hidden_items: Vec<String>,
    pub first_run_done: bool,
    /// Opt-in: run `claude auth status` when the Claude login has expired.
    pub keep_claude_login: bool,
    /// The user was already asked about `keep_claude_login`.
    pub keepalive_asked: bool,
    /// Opt-in: run `codex login status` when the Codex login has expired.
    pub keep_codex_login: bool,
    pub codex_keepalive_asked: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hw_interval_secs: 1,
            limits_interval_secs: 120,
            claude_enabled: false,
            codex_enabled: false,
            hidden_items: Vec::new(),
            first_run_done: false,
            keep_claude_login: false,
            keepalive_asked: false,
            keep_codex_login: false,
            codex_keepalive_asked: false,
        }
    }
}

impl Config {
    /// Missing or corrupt files yield the default config; this never fails.
    pub fn load_from(path: &Path) -> Config {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| {
                serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).ok()
            })
            .unwrap_or_default()
            .clamped()
    }

    /// Writes atomically: temp file first, then rename over the target.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    pub fn clamped(mut self) -> Config {
        self.hw_interval_secs = self.hw_interval_secs.clamp(1, 5);
        self.limits_interval_secs = self.limits_interval_secs.clamp(60, 3600);
        self
    }

    pub fn is_visible(&self, id: &ItemId) -> bool {
        let key = item_key(id);
        !self.hidden_items.contains(&key)
    }

    pub fn set_visible(&mut self, id: &ItemId, visible: bool) {
        let key = item_key(id);
        self.hidden_items.retain(|k| *k != key);
        if !visible {
            self.hidden_items.push(key);
        }
    }
}

pub fn item_key(id: &ItemId) -> String {
    match id {
        ItemId::Cpu => "cpu".into(),
        ItemId::Ram => "ram".into(),
        ItemId::Gpu => "gpu".into(),
        ItemId::Disk(instance) => format!("disk:{instance}"),
        ItemId::ClaudeFive => "claude5h".into(),
        ItemId::ClaudeWeek => "claudeweek".into(),
        ItemId::CodexFive => "codex5h".into(),
        ItemId::CodexWeek => "codexweek".into(),
    }
}

/// `%APPDATA%\widget-usage-windows\config.json`.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(APP_DIR).join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wuw-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn config_roundtrip() {
        let path = temp_file("roundtrip");
        let cfg = Config {
            hw_interval_secs: 2,
            limits_interval_secs: 300,
            claude_enabled: true,
            codex_enabled: false,
            hidden_items: vec!["gpu".into()],
            first_run_done: true,
            keep_claude_login: true,
            keepalive_asked: true,
            keep_codex_login: true,
            codex_keepalive_asked: true,
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), cfg);
    }

    #[test]
    fn config_corrupt_file_gives_default() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{nope").unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn config_missing_file_gives_default() {
        let path = temp_file("missing").with_file_name("absent.json");
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn config_missing_fields_use_defaults() {
        let path = temp_file("partial");
        std::fs::write(&path, r#"{"claude_enabled":true}"#).unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.claude_enabled);
        assert_eq!(cfg.hw_interval_secs, 1);
        assert_eq!(cfg.limits_interval_secs, 120);
        assert!(!cfg.first_run_done);
        assert!(!cfg.keep_claude_login, "keep-alive is opt-in");
        assert!(!cfg.keepalive_asked);
        assert!(!cfg.keep_codex_login, "keep-alive is opt-in");
        assert!(!cfg.codex_keepalive_asked);
    }

    #[test]
    fn config_clamps_intervals() {
        let c = |hw, lim| {
            Config {
                hw_interval_secs: hw,
                limits_interval_secs: lim,
                ..Config::default()
            }
            .clamped()
        };
        assert_eq!(c(0, 120).hw_interval_secs, 1);
        assert_eq!(c(9, 120).hw_interval_secs, 5);
        assert_eq!(c(1, 10).limits_interval_secs, 60);
        assert_eq!(c(3, 600).limits_interval_secs, 600);
        assert_eq!(c(3, 99_999).limits_interval_secs, 3600);
    }

    #[test]
    fn visibility_respects_hidden_items() {
        let cfg = Config {
            hidden_items: vec!["disk:1 D:".into()],
            ..Config::default()
        };
        assert!(!cfg.is_visible(&ItemId::Disk("1 D:".into())));
        assert!(cfg.is_visible(&ItemId::Disk("0 C:".into())));
        assert!(cfg.is_visible(&ItemId::Cpu));
    }

    #[test]
    fn item_keys_are_stable() {
        assert_eq!(item_key(&ItemId::Cpu), "cpu");
        assert_eq!(item_key(&ItemId::Ram), "ram");
        assert_eq!(item_key(&ItemId::Gpu), "gpu");
        assert_eq!(item_key(&ItemId::Disk("0 C:".into())), "disk:0 C:");
        assert_eq!(item_key(&ItemId::ClaudeFive), "claude5h");
        assert_eq!(item_key(&ItemId::ClaudeWeek), "claudeweek");
        assert_eq!(item_key(&ItemId::CodexFive), "codex5h");
        assert_eq!(item_key(&ItemId::CodexWeek), "codexweek");
    }
}
