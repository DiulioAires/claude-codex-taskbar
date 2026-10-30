//! Opt-in "keep login active": when a provider's token has expired, run the official CLI's
//! status command (`claude auth status`, `codex login status`; neither sends a prompt) so
//! the CLI itself renews its own login. This app still never writes credentials files.

use super::{FetchError, ProviderStatus};
use crate::model::Kind;
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
    pub fn should_run(&self, enabled: bool, status: &ProviderStatus, now: i64) -> bool {
        enabled
            && *status == ProviderStatus::Failed(FetchError::Expired)
            && self
                .last_attempt
                .is_none_or(|t| now - t >= MIN_INTERVAL_SECS)
    }

    pub fn started(&mut self, now: i64) {
        self.last_attempt = Some(now);
    }

    /// Tracks whether the login recovered after the last CLI run.
    pub fn observe(&mut self, status: &ProviderStatus, now: i64) {
        let Some(tried_at) = self.last_attempt else {
            return;
        };
        match status {
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

/// The CLI command that checks (and, if needed, renews) a provider's login.
pub fn status_command(kind: Kind) -> Option<&'static [&'static str]> {
    match kind {
        Kind::Claude => Some(&["claude", "auth", "status"]),
        Kind::Codex => Some(&["codex", "login", "status"]),
        Kind::Hardware => None,
    }
}

/// Providers to offer keep-alive for: logged in and not asked before.
pub fn invitation_kinds(
    claude_asked: bool,
    codex_asked: bool,
    claude_login: bool,
    codex_login: bool,
) -> Vec<Kind> {
    [
        (Kind::Claude, !claude_asked && claude_login),
        (Kind::Codex, !codex_asked && codex_login),
    ]
    .into_iter()
    .filter_map(|(kind, offer)| offer.then_some(kind))
    .collect()
}

/// Runs the provider's status command hidden and waits for it; `cmd /C` resolves both native
/// `.exe` installs and npm `.cmd` shims. Output is discarded (it contains account details).
pub fn run_login_status(kind: Kind) -> bool {
    let Some(command) = status_command(kind) else {
        return false;
    };
    Command::new("cmd.exe")
        .arg("/C")
        .args(command)
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
        assert!(super::run_login_status(crate::model::Kind::Claude));
    }

    #[test]
    #[ignore = "runs the locally installed Codex CLI"]
    fn live_codex_login_status_runs_hidden() {
        assert!(super::run_login_status(crate::model::Kind::Codex));
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::model::Kind;

    #[test]
    fn each_provider_uses_its_own_status_command() {
        assert_eq!(
            status_command(Kind::Claude),
            Some(&["claude", "auth", "status"][..])
        );
        assert_eq!(
            status_command(Kind::Codex),
            Some(&["codex", "login", "status"][..])
        );
        assert_eq!(status_command(Kind::Hardware), None);
    }

    #[test]
    fn invitation_covers_only_logged_in_providers_not_yet_asked() {
        assert_eq!(
            invitation_kinds(false, false, true, true),
            [Kind::Claude, Kind::Codex]
        );
        assert_eq!(invitation_kinds(true, false, true, true), [Kind::Codex]);
        assert_eq!(invitation_kinds(false, false, false, true), [Kind::Codex]);
        assert!(invitation_kinds(true, true, true, true).is_empty());
    }
}
