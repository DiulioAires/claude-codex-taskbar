//! Local completion signals and bounded, independent provider-logo pulses.

use crate::model::Kind;
use std::ffi::OsString;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};
use windows::core::w;

/// Carries only a provider number, never a pointer or conversation data.
pub const WM_TASK_COMPLETE: u32 = WM_APP + 6;
pub const TIMER_INTERVAL_MS: u32 = 250;
const PULSE_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, PartialEq)]
pub enum Command {
    Launch,
    Notify(Kind),
    Invalid,
}

pub fn parse_command(args: &[OsString]) -> Command {
    match args {
        [] => Command::Launch,
        [flag, provider] if flag == "--task-complete" => match provider.to_str() {
            Some("claude") => Command::Notify(Kind::Claude),
            Some("codex") => Command::Notify(Kind::Codex),
            _ => Command::Invalid,
        },
        _ => Command::Invalid,
    }
}

#[derive(Default)]
pub struct Pulses {
    started: [Option<Instant>; 2],
}

fn provider_index(kind: Kind) -> Option<usize> {
    match kind {
        Kind::Claude => Some(0),
        Kind::Codex => Some(1),
        Kind::Hardware => None,
    }
}

pub fn message_provider(value: usize) -> Option<Kind> {
    match value {
        1 => Some(Kind::Claude),
        2 => Some(Kind::Codex),
        _ => None,
    }
}

/// Notify an existing widget. Missing/closed widgets are a harmless no-op for hooks.
pub fn notify_running(kind: Kind) {
    // SAFETY: searches the current desktop and sends an integer-only window message.
    if let Ok(hwnd) = unsafe { FindWindowW(w!("WidgetUsageStrip"), None) } {
        post_completion(hwnd, kind);
    }
}

fn post_completion(hwnd: HWND, kind: Kind) {
    if let Some(index) = provider_index(kind) {
        // SAFETY: PostMessage validates the target; no borrowed data crosses processes.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_TASK_COMPLETE, WPARAM(index + 1), LPARAM(0)) };
    }
}

impl Pulses {
    pub fn trigger(&mut self, kind: Kind, now: Instant) {
        if let Some(index) = provider_index(kind) {
            self.started[index] = Some(now);
        }
    }

    pub fn lit(&self, kind: Kind, now: Instant) -> bool {
        provider_index(kind)
            .and_then(|index| self.started[index])
            .and_then(|start| now.checked_duration_since(start))
            .is_some_and(|elapsed| {
                elapsed < PULSE_DURATION && (elapsed.as_millis() / 500).is_multiple_of(2)
            })
    }

    pub fn active(&self, now: Instant) -> bool {
        self.started.iter().flatten().any(|start| {
            now.checked_duration_since(*start)
                .is_some_and(|elapsed| elapsed < PULSE_DURATION)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_exact_completion_commands_are_accepted() {
        assert_eq!(parse_command(&[]), Command::Launch);
        assert_eq!(
            parse_command(&args(&["--task-complete", "claude"])),
            Command::Notify(Kind::Claude)
        );
        assert_eq!(
            parse_command(&args(&["--task-complete", "codex"])),
            Command::Notify(Kind::Codex)
        );
        for invalid in [
            vec!["--task-complete"],
            vec!["--task-complete", "hardware"],
            vec!["--task-complete", "claude", "extra"],
            vec!["--unknown"],
            vec!["claude"],
        ] {
            assert_eq!(parse_command(&args(&invalid)), Command::Invalid);
        }
    }

    #[test]
    fn pulses_have_three_on_phases_and_expire() {
        let now = Instant::now();
        let mut pulse = Pulses::default();
        assert!(!pulse.active(now));
        pulse.trigger(Kind::Claude, now);
        for (millis, expected) in [
            (0, true),
            (499, true),
            (500, false),
            (999, false),
            (1000, true),
            (1499, true),
            (1500, false),
            (1999, false),
            (2000, true),
            (2499, true),
            (2500, false),
            (2999, false),
            (3000, false),
        ] {
            let at = now + Duration::from_millis(millis);
            assert_eq!(pulse.lit(Kind::Claude, at), expected, "at {millis}ms");
            assert_eq!(pulse.active(at), millis < 3000);
            assert!(!pulse.lit(Kind::Codex, at));
            assert!(!pulse.lit(Kind::Hardware, at));
        }
        assert!(!pulse.active(now + Duration::from_secs(3600)));
    }

    #[test]
    fn providers_pulse_independently_and_new_events_restart_only_their_provider() {
        let now = Instant::now();
        let mut pulse = Pulses::default();
        pulse.trigger(Kind::Claude, now);
        let later = now + Duration::from_millis(700);
        pulse.trigger(Kind::Codex, later);
        assert!(!pulse.lit(Kind::Claude, later));
        assert!(pulse.lit(Kind::Codex, later));
        pulse.trigger(Kind::Claude, later);
        assert!(pulse.lit(Kind::Claude, later));
        assert!(pulse.lit(Kind::Codex, later));
        assert!(!pulse.active(later + Duration::from_secs(3)));
    }

    #[test]
    fn hardware_and_times_before_the_event_never_flash() {
        let now = Instant::now();
        let mut pulse = Pulses::default();
        pulse.trigger(Kind::Hardware, now);
        assert!(!pulse.active(now));
        pulse.trigger(Kind::Claude, now + Duration::from_secs(1));
        assert!(!pulse.lit(Kind::Claude, now));
    }

    #[test]
    fn native_message_carries_only_the_provider_to_an_isolated_window() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW,
        };
        // SAFETY: a private message-only STATIC window; never looks up or starts the app.
        unsafe {
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                None,
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap();
            for kind in [Kind::Claude, Kind::Codex] {
                post_completion(hwnd, kind);
                let mut msg = MSG::default();
                let found = PeekMessageW(
                    &mut msg,
                    Some(hwnd),
                    WM_TASK_COMPLETE,
                    WM_TASK_COMPLETE,
                    PM_REMOVE,
                );
                assert!(found.as_bool());
                assert_eq!(message_provider(msg.wParam.0), Some(kind));
                assert_eq!(msg.lParam.0, 0);
            }
            post_completion(hwnd, Kind::Hardware);
            let mut msg = MSG::default();
            assert!(
                !PeekMessageW(
                    &mut msg,
                    Some(hwnd),
                    WM_TASK_COMPLETE,
                    WM_TASK_COMPLETE,
                    PM_REMOVE
                )
                .as_bool()
            );
            DestroyWindow(hwnd).unwrap();
        }
        assert_eq!(message_provider(0), None);
        assert_eq!(message_provider(3), None);
        assert_eq!(message_provider(usize::MAX), None);
    }
}
