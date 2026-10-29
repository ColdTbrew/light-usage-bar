use std::time::{Duration, Instant};
use usage_core::FetchError;
use usage_providers::ProbeFailure;

pub(crate) const INTERVAL: Duration = Duration::from_secs(600);
const MINIMUM: Duration = Duration::from_secs(30);

pub(crate) struct Policy {
    pub enabled: bool,
    pub busy: bool,
    pub due: Option<Instant>,
    minimum: Instant,
    paused: bool,
    failures: u32,
}

impl Policy {
    pub fn new(enabled: bool, now: Instant) -> Self {
        Self {
            enabled,
            busy: false,
            due: enabled.then_some(now),
            minimum: now,
            paused: false,
            failures: 0,
        }
    }

    pub fn request(&mut self, now: Instant, manual: bool) {
        if self.enabled && !self.busy && (manual || !self.paused) {
            self.due = Some(now.max(self.minimum));
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, now: Instant) {
        self.enabled = enabled;
        self.due = None;
        if enabled {
            self.request(now, true);
        }
    }

    pub fn cancelled(&mut self, now: Instant) {
        self.busy = false;
        self.request(now, true);
    }

    pub fn start(&mut self, now: Instant) -> bool {
        if !self.enabled || self.busy || !self.due.is_some_and(|due| now >= due) {
            return false;
        }
        self.busy = true;
        self.due = None;
        self.minimum = now + MINIMUM;
        true
    }

    pub fn complete(&mut self, now: Instant, failure: Option<ProbeFailure>, jitter: u64) {
        self.busy = false;
        self.paused = false;
        match failure {
            None => {
                self.failures = 0;
                self.due = Some(now + INTERVAL);
            }
            Some(failure) => {
                self.failures = self.failures.saturating_add(1);
                if matches!(
                    failure.error,
                    FetchError::NeedsAuthentication
                        | FetchError::PermissionDenied
                        | FetchError::Unsupported
                        | FetchError::NeedsVerification
                ) {
                    self.paused = true;
                    self.due = None;
                } else {
                    let seconds = (30u64 << self.failures.saturating_sub(1).min(6)).min(1800);
                    let delay = Duration::from_secs(seconds + jitter % (seconds / 10 + 1));
                    let delay = failure.retry_after.unwrap_or_default().max(delay);
                    if let Some(due) = now.checked_add(delay) {
                        self.minimum = self.minimum.max(due);
                        self.due = Some(self.minimum);
                    } else {
                        self.paused = true;
                        self.due = None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_manual_requests_join_and_respect_retry_after_across_disable() {
        let now = Instant::now();
        let mut policy = Policy::new(true, now);
        assert!(policy.start(now));
        for _ in 0..20 {
            policy.request(now, true);
            assert!(!policy.start(now));
        }
        policy.complete(
            now,
            Some(ProbeFailure {
                error: FetchError::RateLimited,
                retry_after: Some(Duration::from_secs(3600)),
                identity_invalidated: false,
            }),
            0,
        );
        policy.set_enabled(false, now);
        policy.set_enabled(true, now);
        policy.request(now + Duration::from_secs(40), true);
        assert!(!policy.start(now + Duration::from_secs(3599)));
        assert!(policy.start(now + Duration::from_secs(3600)));
    }
    #[test]
    fn authentication_stops_automatic_work_but_manual_retry_is_possible() {
        let now = Instant::now();
        let mut policy = Policy::new(true, now);
        assert!(policy.start(now));
        policy.complete(now, Some(FetchError::NeedsAuthentication.into()), 0);
        policy.request(now + INTERVAL, false);
        assert!(!policy.start(now + INTERVAL));
        policy.request(now + INTERVAL, true);
        assert!(policy.start(now + INTERVAL));
    }
    #[test]
    fn waking_coalesces_missed_intervals_into_one_request() {
        let now = Instant::now();
        let mut policy = Policy::new(true, now);
        assert!(policy.start(now));
        policy.complete(now, None, 0);
        let later = now + Duration::from_secs(14400);
        policy.request(later, false);
        assert!(policy.start(later));
        assert!(!policy.start(later));
        policy.complete(later, None, 0);
        assert!(!policy.start(later));
        assert!(policy.start(later + INTERVAL));
    }
    #[test]
    fn disabled_provider_does_not_start_even_when_refreshed() {
        let now = Instant::now();
        let mut policy = Policy::new(false, now);
        policy.request(now, true);
        assert!(!policy.start(now));
        policy.set_enabled(true, now);
        assert!(policy.start(now));
    }
}
