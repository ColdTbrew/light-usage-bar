use crate::{Cancellation, CredentialRevision, ProbeFailure};
use usage_core::Snapshot;

#[cfg(all(unix, any(target_os = "macos", test)))]
mod native;
#[cfg(any(target_os = "macos", test))]
mod parser;

pub(crate) struct ClaudeObservation {
    pub(crate) snapshot: Snapshot,
    pub(crate) revision: CredentialRevision,
}

pub(crate) async fn collect(cancel: &Cancellation) -> Result<ClaudeObservation, ProbeFailure> {
    #[cfg(target_os = "macos")]
    {
        native::collect(cancel).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cancel;
        Err(usage_core::FetchError::Unsupported.into())
    }
}
