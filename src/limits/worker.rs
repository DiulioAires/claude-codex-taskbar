//! Background thread that fetches limits and hands results to the UI thread.

use super::{FetchError, ProviderUsage};
use crate::model::Kind;
use crate::timeutil::now_unix;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Fetches one provider at `now`: usage, whether it came from session logs, and its time.
pub type Fetcher = Box<dyn Fn(i64) -> Result<(ProviderUsage, bool, i64), FetchError> + Send>;

#[derive(Debug)]
pub struct LimitResult {
    pub kind: Kind,
    pub result: Result<(ProviderUsage, bool, i64), FetchError>,
}

/// Shared knobs the UI thread uses to steer the worker.
pub struct Control {
    interval_secs: AtomicU32,
    claude_on: AtomicBool,
    codex_on: AtomicBool,
    quit: AtomicBool,
    woken: Mutex<bool>,
    cv: Condvar,
}

impl Control {
    pub fn new(interval_secs: u32) -> Arc<Control> {
        Arc::new(Control {
            interval_secs: AtomicU32::new(interval_secs),
            claude_on: AtomicBool::new(false),
            codex_on: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            woken: Mutex::new(false),
            cv: Condvar::new(),
        })
    }

    fn flag(&self, kind: Kind) -> Option<&AtomicBool> {
        match kind {
            Kind::Claude => Some(&self.claude_on),
            Kind::Codex => Some(&self.codex_on),
            Kind::Hardware => None,
        }
    }

    pub fn set_enabled(&self, kind: Kind, on: bool) {
        if let Some(flag) = self.flag(kind) {
            flag.store(on, Ordering::Relaxed);
        }
    }

    pub fn is_enabled(&self, kind: Kind) -> bool {
        self.flag(kind).is_some_and(|f| f.load(Ordering::Relaxed))
    }

    pub fn set_interval(&self, secs: u32) {
        self.interval_secs.store(secs, Ordering::Relaxed);
    }

    /// Fetch now instead of waiting for the interval.
    pub fn wake(&self) {
        *self.woken.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    pub fn stop(&self) {
        self.quit.store(true, Ordering::Relaxed);
        self.wake();
    }

    fn wait(&self) {
        let timeout = Duration::from_secs(self.interval_secs.load(Ordering::Relaxed).into());
        let guard = self.woken.lock().unwrap_or_else(|e| e.into_inner());
        let (mut woken, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |w| !*w)
            .unwrap_or_else(|e| e.into_inner());
        *woken = false;
    }
}

/// Fetches every enabled provider right away, then every interval or on `wake`.
pub fn spawn_with(
    control: Arc<Control>,
    fetchers: Vec<(Kind, Fetcher)>,
    post: impl Fn(LimitResult) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !control.quit.load(Ordering::Relaxed) {
            for (kind, fetch) in &fetchers {
                if control.is_enabled(*kind) && !control.quit.load(Ordering::Relaxed) {
                    post(LimitResult {
                        kind: *kind,
                        result: fetch(now_unix()),
                    });
                }
            }
            control.wait();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{FetchError, ProviderUsage};
    use crate::model::Kind;
    use std::sync::mpsc;
    use std::time::Duration;

    fn fake(pct: f32) -> Fetcher {
        Box::new(move |now| {
            Ok((
                ProviderUsage {
                    five_hour: Some(crate::limits::WindowUsage {
                        used_pct: pct,
                        resets_at: None,
                    }),
                    weekly: None,
                },
                false,
                now,
            ))
        })
    }

    #[test]
    fn worker_posts_and_respects_wake() {
        let control = Control::new(3600);
        control.set_enabled(Kind::Claude, true);
        let (tx, rx) = mpsc::channel();
        let handle = spawn_with(
            control.clone(),
            vec![(Kind::Claude, fake(10.0))],
            move |r| {
                tx.send(r).unwrap();
            },
        );
        let first = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("initial fetch");
        assert_eq!(first.kind, Kind::Claude);
        assert!(first.result.is_ok());
        control.wake();
        rx.recv_timeout(Duration::from_secs(2))
            .expect("fetch after wake");
        control.stop();
        handle.join().unwrap();
    }

    #[test]
    fn disabled_provider_is_not_fetched() {
        let control = Control::new(3600);
        control.set_enabled(Kind::Codex, false);
        control.set_enabled(Kind::Claude, true);
        let failing: Fetcher = Box::new(|_| Err(FetchError::NoLogin));
        let (tx, rx) = mpsc::channel();
        let handle = spawn_with(
            control.clone(),
            vec![(Kind::Codex, failing), (Kind::Claude, fake(1.0))],
            move |r| tx.send(r).unwrap(),
        );
        let r = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(r.kind, Kind::Claude);
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        control.stop();
        handle.join().unwrap();
    }
}
