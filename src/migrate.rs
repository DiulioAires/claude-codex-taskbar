//! One-time migration from the previous app name (`widget-usage-windows`, v0.1.x).

use std::io;
use std::path::Path;

/// Copies `config.json` from the legacy folder unless the new folder already has one.
/// Returns whether a copy happened. The legacy file is left in place.
pub fn migrate_config_dir(legacy_dir: &Path, new_dir: &Path) -> io::Result<bool> {
    let (legacy, new) = (legacy_dir.join("config.json"), new_dir.join("config.json"));
    if new.exists() || !legacy.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(new_dir)?;
    std::fs::copy(&legacy, &new)?;
    Ok(true)
}

/// Runs every migration at startup; failures are ignored (the app falls back to defaults).
pub fn run() {
    if let Some(appdata) = std::env::var_os("APPDATA").map(std::path::PathBuf::from) {
        let _ = migrate_config_dir(
            &appdata.join(crate::config::LEGACY_APP_DIR),
            &appdata.join(crate::config::APP_DIR),
        );
    }
    crate::autostart::migrate_legacy();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dirs(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("cct-migrate-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        (base.join("old"), base.join("new"))
    }

    #[test]
    fn copies_legacy_config_when_new_is_missing() {
        let (old, new) = dirs("copy");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("config.json"), r#"{"claude_enabled":true}"#).unwrap();
        assert!(migrate_config_dir(&old, &new).unwrap());
        assert_eq!(
            std::fs::read_to_string(new.join("config.json")).unwrap(),
            r#"{"claude_enabled":true}"#
        );
        // The legacy file stays, so downgrading keeps working.
        assert!(old.join("config.json").exists());
    }

    #[test]
    fn never_overwrites_existing_new_config() {
        let (old, new) = dirs("keep");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("config.json"), "old").unwrap();
        std::fs::write(new.join("config.json"), "new").unwrap();
        assert!(!migrate_config_dir(&old, &new).unwrap());
        assert_eq!(
            std::fs::read_to_string(new.join("config.json")).unwrap(),
            "new"
        );
    }

    #[test]
    fn nothing_to_migrate() {
        let (old, new) = dirs("none");
        assert!(!migrate_config_dir(&old, &new).unwrap());
        assert!(!new.exists());
    }
}
