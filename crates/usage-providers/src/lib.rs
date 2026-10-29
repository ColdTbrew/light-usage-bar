//! Read-only subscription adapters. Credentials never cross the snapshot boundary.
mod antigravity;
mod credentials;
mod parsers;
mod transport;

pub use credentials::credential_helper_main;
pub use parsers::{parse_antigravity, parse_claude, parse_codex, parse_grok};
pub use transport::{ProviderClient, retry_after};

use std::{fmt, time::Duration};
use usage_core::{FetchError, Identity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeFailure {
    pub error: FetchError,
    pub retry_after: Option<Duration>,
    pub identity_invalidated: bool,
}
impl From<FetchError> for ProbeFailure {
    fn from(error: FetchError) -> Self {
        Self {
            error,
            retry_after: None,
            identity_invalidated: false,
        }
    }
}

/// An equality key for invalidating work after a credential change.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialRevision([u8; 32]);
impl fmt::Debug for CredentialRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialRevision([redacted])")
    }
}

pub struct PreparedProbe {
    identity: Identity,
    source: PreparedSource,
}
enum PreparedSource {
    Remote(credentials::Credential),
    Antigravity(antigravity::LocalSession),
}

impl PreparedProbe {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn credential_revision(&self) -> &CredentialRevision {
        match &self.source {
            PreparedSource::Remote(credential) => &credential.revision,
            PreparedSource::Antigravity(session) => &session.revision,
        }
    }
}
impl fmt::Debug for PreparedProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedProbe")
            .field("identity", &self.identity)
            .field("credential", &"[redacted]")
            .finish()
    }
}

/// Cooperative stop signal. Await the operation after cancelling to finish child cleanup.
#[derive(Clone)]
pub struct Cancellation(tokio::sync::watch::Sender<bool>);
impl Default for Cancellation {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}
