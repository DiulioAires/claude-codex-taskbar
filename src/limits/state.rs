//! How a provider's displayed status evolves after each fetch.

use super::{FetchError, ProviderStatus, ProviderUsage};

/// Previous data stays on screen this long after fetches start failing.
pub const STALE_SECS: i64 = 15 * 60;

/// Auth problems are reported immediately; transient errors keep recent data as `Stale`.
pub fn next_status(
    prev: &ProviderStatus,
    result: Result<ProviderUsage, FetchError>,
    now: i64,
) -> ProviderStatus {
    let err = match result {
        Ok(usage) => return ProviderStatus::Ok { usage, at: now },
        Err(err) => err,
    };
    let transient = matches!(err, FetchError::Network(_) | FetchError::BadResponse(_));
    if transient
        && let ProviderStatus::Ok { usage, at } | ProviderStatus::Stale { usage, at, .. } = prev
        && now - at <= STALE_SECS
    {
        return ProviderStatus::Stale {
            usage: usage.clone(),
            at: *at,
            err,
        };
    }
    ProviderStatus::Failed(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{FetchError, ProviderStatus, ProviderUsage, WindowUsage};

    fn usage(pct: f32) -> ProviderUsage {
        ProviderUsage {
            five_hour: Some(WindowUsage {
                used_pct: pct,
                resets_at: Some(5000),
            }),
            weekly: None,
        }
    }

    #[test]
    fn ok_result_sets_ok() {
        let s = next_status(&ProviderStatus::Loading, Ok(usage(10.0)), 100);
        assert_eq!(
            s,
            ProviderStatus::Ok {
                usage: usage(10.0),
                at: 100
            }
        );
    }

    #[test]
    fn network_error_within_15min_is_stale() {
        let prev = ProviderStatus::Ok {
            usage: usage(10.0),
            at: 1000,
        };
        let err = FetchError::Network("timeout".into());
        let s = next_status(&prev, Err(err.clone()), 1000 + 899);
        assert_eq!(
            s,
            ProviderStatus::Stale {
                usage: usage(10.0),
                at: 1000,
                err
            }
        );
    }

    #[test]
    fn stale_stays_stale_until_window_passes() {
        let err = FetchError::BadResponse("HTTP 500".into());
        let prev = ProviderStatus::Stale {
            usage: usage(10.0),
            at: 1000,
            err: err.clone(),
        };
        let s = next_status(&prev, Err(err.clone()), 1000 + 900);
        assert_eq!(
            s,
            ProviderStatus::Stale {
                usage: usage(10.0),
                at: 1000,
                err
            }
        );
    }

    #[test]
    fn network_error_after_15min_fails() {
        let prev = ProviderStatus::Ok {
            usage: usage(10.0),
            at: 1000,
        };
        let err = FetchError::Network("timeout".into());
        let s = next_status(&prev, Err(err.clone()), 1000 + 901);
        assert_eq!(s, ProviderStatus::Failed(err));
    }

    #[test]
    fn network_error_without_previous_data_fails() {
        let err = FetchError::Network("dns".into());
        let s = next_status(&ProviderStatus::Loading, Err(err.clone()), 10);
        assert_eq!(s, ProviderStatus::Failed(err));
    }

    #[test]
    fn expired_never_stale() {
        let prev = ProviderStatus::Ok {
            usage: usage(10.0),
            at: 1000,
        };
        let s = next_status(&prev, Err(FetchError::Expired), 1001);
        assert_eq!(s, ProviderStatus::Failed(FetchError::Expired));
    }
}
