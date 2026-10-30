//! Opt-in "keep Claude login active": when the token has expired, run the official CLI
//! (`claude auth status`, which sends no prompt) so the CLI itself renews its own login.
//! This app still never writes the credentials file.

use super::{FetchError, ProviderStatus};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

/// Minimum spacing between CLI runs.
pub const MIN_INTERVAL_SECS: i64 = 15 * 60;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    StillExpired { tried_at: i64 },
    Renewed { at: i64 },
}

#[derive(Debug)]
pub struct KeepAlive {
    last_attempt: Option<i64>,
    outcome: Outcome,
}

impl Default for KeepAlive {
    fn default() -> Self {
        KeepAlive {
            last_attempt: None,
            outcome: Outcome::None,
        }
    }
}

impl KeepAlive {
    pub fn should_run(&self, enabled: bool, claude: &ProviderStatus, now: i64) -> bool {
        enabled
            && *claude == ProviderStatus::Failed(FetchError::Expired)
            && self
                .last_attempt
                .is_none_or(|t| now - t >= MIN_INTERVAL_SECS)
    }

    pub fn started(&mut self, now: i64) {
        self.last_attempt = Some(now);
    }

    /// Tracks whether the login recovered after the last CLI run.
    pub fn observe(&mut self, claude: &ProviderStatus, now: i64) {
        let Some(tried_at) = self.last_attempt else {
            return;
        };
        match claude {
            ProviderStatus::Ok { .. } if !matches!(self.outcome, Outcome::Renewed { .. }) => {
                self.outcome = Outcome::Renewed { at: now };
            }
            ProviderStatus::Failed(FetchError::Expired) => {
                self.outcome = Outcome::StillExpired { tried_at };
            }
            _ => {}
        }
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
}

/// Runs `claude auth status` hidden and waits for it; `cmd /C` resolves both the native
/// `claude.exe` and the npm `claude.cmd`. Output is discarded (it contains account details).
pub fn run_claude_auth_status() -> bool {
    Command::new("cmd.exe")
        .args(["/C", "claude", "auth", "status"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{FetchError, ProviderStatus, ProviderUsage};

    fn ok() -> ProviderStatus {
        ProviderStatus::Ok {
            usage: ProviderUsage {
                five_hour: None,
                weekly: None,
            },
            at: 0,
        }
    }
    const EXPIRED: ProviderStatus = ProviderStatus::Failed(FetchError::Expired);

    #[test]
    fn runs_only_when_enabled_and_expired() {
        let k = KeepAlive::default();
        assert!(k.should_run(true, &EXPIRED, 1000));
        assert!(!k.should_run(false, &EXPIRED, 1000));
        assert!(!k.should_run(true, &ok(), 1000));
        assert!(!k.should_run(true, &ProviderStatus::Failed(FetchError::NoLogin), 1000));
    }

    #[test]
    fn at_most_once_per_15_minutes() {
        let mut k = KeepAlive::default();
        k.started(1000);
        assert!(!k.should_run(true, &EXPIRED, 1000 + 899));
        assert!(k.should_run(true, &EXPIRED, 1000 + 900));
    }

    #[test]
    fn records_renewal_when_ok_follows_attempt() {
        let mut k = KeepAlive::default();
        k.observe(&ok(), 500);
        assert_eq!(k.outcome(), Outcome::None);
        k.started(1000);
        k.observe(&EXPIRED, 1010);
        assert_eq!(k.outcome(), Outcome::StillExpired { tried_at: 1000 });
        k.observe(&ok(), 1020);
        assert_eq!(k.outcome(), Outcome::Renewed { at: 1020 });
    }
}

#[cfg(test)]
mod live {
    #[test]
    #[ignore = "runs the locally installed Claude Code CLI"]
    fn live_claude_auth_status_runs_hidden() {
        assert!(super::run_claude_auth_status());
    }
}
