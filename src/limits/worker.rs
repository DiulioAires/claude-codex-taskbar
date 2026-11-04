//! Background thread that fetches limits and hands results to the UI thread.

use super::{FetchError, ProviderUsage};
use crate::model::Kind;
use crate::timeutil::now_unix;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
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

/// Like [`spawn_with`], but results wait in a queue the UI drains when it can; `notify`
/// only signals that something arrived, so no data travels inside window messages.
pub fn spawn_queued(
    control: Arc<Control>,
    fetchers: Vec<(Kind, Fetcher)>,
    notify: impl Fn() + Send + 'static,
) -> (JoinHandle<()>, Receiver<LimitResult>) {
    let (tx, rx) = channel();
    let handle = spawn_with(control, fetchers, move |result| {
        if tx.send(result).is_ok() {
            notify();
        }
    });
    (handle, rx)
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
                    ..Default::default()
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

#[cfg(test)]
mod queue_tests {
    use super::*;
    use crate::limits::{ProviderUsage, WindowUsage};
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    #[test]
    fn results_queue_until_drained() {
        let control = Control::new(3600);
        control.set_enabled(Kind::Claude, true);
        control.set_enabled(Kind::Codex, true);
        let ok = |pct: f32| -> Fetcher {
            Box::new(move |now| {
                Ok((
                    ProviderUsage {
                        five_hour: Some(WindowUsage {
                            used_pct: pct,
                            resets_at: None,
                        }),
                        weekly: None,
                        ..Default::default()
                    },
                    false,
                    now,
                ))
            })
        };
        let notified = Arc::new(AtomicUsize::new(0));
        let n = notified.clone();
        let (handle, inbox) = spawn_queued(
            control.clone(),
            vec![(Kind::Claude, ok(1.0)), (Kind::Codex, ok(2.0))],
            move || {
                n.fetch_add(1, Ordering::SeqCst);
            },
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while notified.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        // Nothing was consumed while "busy"; both results are still waiting.
        let kinds: Vec<Kind> = inbox.try_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::Claude, Kind::Codex]);
        control.stop();
        handle.join().unwrap();
    }
}
