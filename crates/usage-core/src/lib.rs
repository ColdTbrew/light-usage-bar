//! Subscription quota values and a single selected identity's in-memory cache.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Codex,
    GrokBot,
    Antigravity,
}

/// An equality key whose original value is excluded from diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyIdentifier;

impl AccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, EmptyIdentifier> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(EmptyIdentifier)
        } else {
            Ok(Self(value))
        }
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountId([redacted])")
    }
}

/// A provider's workspace equality key, excluded from diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn new(value: impl Into<String>) -> Result<Self, EmptyIdentifier> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(EmptyIdentifier)
        } else {
            Ok(Self(value))
        }
    }
}

impl fmt::Debug for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkspaceId([redacted])")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub provider: Provider,
    pub account: AccountId,
    pub workspace: Option<WorkspaceId>,
}

/// A finite, nonnegative provider value. Percentages above 100 are preserved.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct NonNegative(f64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidNumber;

impl NonNegative {
    pub fn new(value: f64) -> Result<Self, InvalidNumber> {
        if value.is_finite() && value >= 0.0 {
            Ok(Self(value))
        } else {
            Err(InvalidNumber)
        }
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Used,
    Remaining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    Tokens,
    Requests,
    Credits,
    ProviderDefined(ProviderUnit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderUnit(String);

impl ProviderUnit {
    pub fn new(value: impl Into<String>) -> Result<Self, EmptyIdentifier> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(EmptyIdentifier)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Measurement {
    Unknown,
    Percent {
        value: NonNegative,
        direction: Direction,
    },
    Amount {
        value: NonNegative,
        limit: Option<NonNegative>,
        direction: Direction,
        unit: Unit,
    },
}

/// A stable provider window key, independent of model and duration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowId(String);

impl WindowId {
    pub fn new(value: impl Into<String>) -> Result<Self, EmptyIdentifier> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(EmptyIdentifier)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuotaWindow {
    pub id: Option<WindowId>,
    pub label: Option<String>,
    pub measurement: Measurement,
    pub model: Option<String>,
    pub period: Option<Duration>,
    pub period_started_at: Option<SystemTime>,
    pub resets_at: Option<SystemTime>,
    pub trial_expires_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    DocumentedApi,
    ProviderClient,
    LocalLog,
    LocalService,
    CommandLine,
}

/// Supplied explanation of input values and assumptions. No calculation is performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derivation(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyDerivation;

impl Derivation {
    pub fn new(explanation: impl Into<String>) -> Result<Self, EmptyDerivation> {
        let explanation = explanation.into();
        if explanation.trim().is_empty() {
            Err(EmptyDerivation)
        } else {
            Ok(Self(explanation))
        }
    }

    pub fn explanation(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    ProviderReported,
    LocalObserved,
    Derived(Derivation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub kind: SourceKind,
    pub provenance: Provenance,
}

/// Provider observation time is retained independently of local retrieval time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub observed_at: Option<SystemTime>,
    pub fetched_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub identity: Identity,
    pub windows: Vec<QuotaWindow>,
    pub timing: Timing,
    /// One common source for every window in this snapshot.
    pub source: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    NeedsAuthentication,
    PermissionDenied,
    RateLimited,
    TemporarilyUnavailable,
    Unsupported,
    NeedsVerification,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchState {
    Idle,
    Refreshing,
    Failed(FetchError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Missing,
    Fresh,
    Stale,
}

/// A read never changes a snapshot or its timestamps.
#[derive(Debug)]
pub struct View<'a> {
    pub snapshot: Option<&'a Snapshot>,
    pub freshness: Freshness,
    pub fetch_state: FetchState,
}

/// Only the cache that issued this token can accept its completion.
#[derive(Clone, Debug)]
pub struct RequestToken(Arc<()>);

#[derive(Debug)]
pub enum Begin {
    Started(RequestToken),
    Joined(RequestToken),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionError {
    ObsoleteRequest,
    IdentityMismatch,
}

#[derive(Debug)]
enum Refresh {
    Idle,
    InFlight(RequestToken),
    Failed(FetchError),
}

#[derive(Debug)]
pub struct Cache {
    selected: Identity,
    snapshot: Option<Snapshot>,
    refresh: Refresh,
}

impl Cache {
    pub fn new(selected: Identity) -> Self {
        Self {
            selected,
            snapshot: None,
            refresh: Refresh::Idle,
        }
    }

    pub fn selected(&self) -> &Identity {
        &self.selected
    }

    /// Selecting the same identity preserves pending work and the last success.
    /// Any identity change invalidates pending work and clears visible data.
    pub fn select(&mut self, selected: Identity) {
        if self.selected != selected {
            self.selected = selected;
            self.snapshot = None;
            self.refresh = Refresh::Idle;
        }
    }

    pub fn begin(&mut self) -> Begin {
        if let Refresh::InFlight(token) = &self.refresh {
            return Begin::Joined(token.clone());
        }
        let token = RequestToken(Arc::new(()));
        self.refresh = Refresh::InFlight(token.clone());
        Begin::Started(token)
    }

    /// Rejected completions leave both pending work and the last success intact.
    pub fn succeed(
        &mut self,
        token: &RequestToken,
        snapshot: Snapshot,
    ) -> Result<(), CompletionError> {
        self.check(token)?;
        if snapshot.identity != self.selected {
            return Err(CompletionError::IdentityMismatch);
        }
        self.snapshot = Some(snapshot);
        self.refresh = Refresh::Idle;
        Ok(())
    }

    pub fn fail(&mut self, token: &RequestToken, error: FetchError) -> Result<(), CompletionError> {
        self.check(token)?;
        self.refresh = Refresh::Failed(error);
        Ok(())
    }

    pub fn cancel(&mut self, token: &RequestToken) -> Result<(), CompletionError> {
        self.check(token)?;
        self.refresh = Refresh::Idle;
        Ok(())
    }

    /// Expiry is exclusive. Quota resets never change freshness or measured values.
    pub fn view(&self, now: SystemTime) -> View<'_> {
        View {
            snapshot: self.snapshot.as_ref(),
            freshness: match &self.snapshot {
                None => Freshness::Missing,
                Some(snapshot) if now < snapshot.timing.expires_at => Freshness::Fresh,
                Some(_) => Freshness::Stale,
            },
            fetch_state: match &self.refresh {
                Refresh::Idle => FetchState::Idle,
                Refresh::InFlight(_) => FetchState::Refreshing,
                Refresh::Failed(error) => FetchState::Failed(*error),
            },
        }
    }

    fn check(&self, token: &RequestToken) -> Result<(), CompletionError> {
        if matches!(&self.refresh, Refresh::InFlight(pending) if Arc::ptr_eq(&pending.0, &token.0))
        {
            Ok(())
        } else {
            Err(CompletionError::ObsoleteRequest)
        }
    }
}
