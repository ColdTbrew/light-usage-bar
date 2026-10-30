pub(crate) mod boundary;
#[cfg(target_os = "macos")]
mod native;

use crate::{Cancellation, CredentialRevision, ProbeFailure, parsers};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    num::NonZeroU16,
    time::{Duration, SystemTime},
};
use tokio::time::Instant;
use usage_core::{AccountId, FetchError, Identity, Provider, Snapshot};

const DEADLINE: Duration = Duration::from_secs(15);
#[cfg(target_os = "macos")]
const MAX_CANDIDATES: usize = 16;

#[derive(Clone, PartialEq, Eq)]
struct ProcessGeneration {
    pid: i32,
    uid: u32,
    ruid: u32,
    start_sec: u64,
    start_usec: u64,
    executable: std::path::PathBuf,
}

#[derive(Clone, PartialEq, Eq)]
struct OwnedListener {
    fd: i32,
    port: NonZeroU16,
    address: [u8; 4],
    generation: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct Endpoint {
    process: ProcessGeneration,
    listener: OwnedListener,
    csrf: String,
}

pub(crate) struct LocalSession {
    endpoint: Endpoint,
    email: String,
    pub(crate) revision: CredentialRevision,
}

struct Budget<'a> {
    end: Instant,
    cancellation: &'a Cancellation,
}
impl<'a> Budget<'a> {
    fn new(cancellation: &'a Cancellation) -> Self {
        Self {
            end: Instant::now() + DEADLINE,
            cancellation,
        }
    }
    fn check(&self) -> Result<(), ProbeFailure> {
        if *self.cancellation.0.borrow() || Instant::now() >= self.end {
            Err(FetchError::TemporarilyUnavailable.into())
        } else {
            Ok(())
        }
    }
    async fn request<T>(
        &self,
        request: impl Future<Output = Result<T, ProbeFailure>>,
    ) -> Result<T, ProbeFailure> {
        self.check()?;
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(FetchError::TemporarilyUnavailable.into()),
            result = tokio::time::timeout_at(self.end, request) => result.unwrap_or_else(|_| Err(FetchError::TemporarilyUnavailable.into())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rpc {
    Status,
    Quota,
}
impl Rpc {
    fn method(&self) -> &'static str {
        match self {
            Self::Status => "GetUserStatus",
            Self::Quota => "RetrieveUserQuotaSummary",
        }
    }
    fn body(&self) -> &'static str {
        match self {
            Self::Status => {
                r#"{"metadata":{"ideName":"antigravity","extensionName":"antigravity","ideVersion":"unknown","locale":"en"}}"#
            }
            Self::Quota => r#"{"forceRefresh":true}"#,
        }
    }
}

fn client() -> Result<reqwest::Client, ProbeFailure> {
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_secs(3))
        .timeout(DEADLINE)
        .build()
        .map_err(|_| FetchError::TemporarilyUnavailable.into())
}

trait LocalIo {
    async fn validate(&self, endpoint: &Endpoint, budget: &Budget<'_>) -> Result<(), ProbeFailure>;
    async fn send(
        &self,
        endpoint: &Endpoint,
        rpc: Rpc,
        budget: &Budget<'_>,
    ) -> Result<Vec<u8>, ProbeFailure>;
}
impl LocalIo for reqwest::Client {
    async fn validate(&self, endpoint: &Endpoint, budget: &Budget<'_>) -> Result<(), ProbeFailure> {
        revalidate(endpoint, budget).await
    }
    async fn send(
        &self,
        endpoint: &Endpoint,
        rpc: Rpc,
        budget: &Budget<'_>,
    ) -> Result<Vec<u8>, ProbeFailure> {
        let mut csrf = reqwest::header::HeaderValue::from_str(&endpoint.csrf)
            .map_err(|_| invalidated(FetchError::NeedsVerification.into()))?;
        csrf.set_sensitive(true);
        let url = format!(
            "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/{}",
            endpoint.listener.port,
            rpc.method()
        );
        budget
            .request(async {
                let response = self
                    .post(url)
                    .header("Content-Type", "application/json")
                    .header("Connect-Protocol-Version", "1")
                    .header("X-Codeium-Csrf-Token", csrf)
                    .body(rpc.body())
                    .send()
                    .await
                    .map_err(|_| FetchError::TemporarilyUnavailable)?;
                crate::transport::body(response).await
            })
            .await
    }
}

async fn request(
    io: &impl LocalIo,
    endpoint: &Endpoint,
    rpc: Rpc,
    budget: &Budget<'_>,
) -> Result<Vec<u8>, ProbeFailure> {
    io.validate(endpoint, budget).await?;
    budget.check()?;
    io.send(endpoint, rpc, budget).await
}

pub(crate) async fn prepare(
    cancellation: &Cancellation,
) -> Result<(Identity, LocalSession), ProbeFailure> {
    let budget = Budget::new(cancellation);
    let endpoints = discover(&budget).await?;
    prepare_endpoints(endpoints, &client()?, &budget).await
}

async fn prepare_endpoints(
    endpoints: Vec<Endpoint>,
    io: &impl LocalIo,
    budget: &Budget<'_>,
) -> Result<(Identity, LocalSession), ProbeFailure> {
    if endpoints.is_empty() {
        return Err(FetchError::TemporarilyUnavailable.into());
    }
    let mut selected: Option<(Endpoint, String)> = None;
    for endpoint in endpoints {
        let bytes = request(io, &endpoint, Rpc::Status, budget).await?;
        let email = boundary::email(&bytes)?;
        io.validate(&endpoint, budget).await?;
        if let Some((_, prior)) = &selected {
            if prior != &email {
                return Err(FetchError::NeedsVerification.into());
            }
        } else {
            selected = Some((endpoint, email));
        }
    }
    let (endpoint, email) = selected.ok_or(FetchError::NeedsVerification)?;
    io.validate(&endpoint, budget).await?;
    let identity = Identity {
        provider: Provider::Antigravity,
        account: AccountId::new(&email).map_err(|_| FetchError::NeedsVerification)?,
        workspace: None,
    };
    let revision = revision(&endpoint, &email);
    budget.check()?;
    Ok((
        identity,
        LocalSession {
            endpoint,
            email,
            revision,
        },
    ))
}

pub(crate) async fn fetch(
    session: &LocalSession,
    identity: &Identity,
    cancellation: &Cancellation,
) -> Result<Snapshot, ProbeFailure> {
    let budget = Budget::new(cancellation);
    fetch_session(session, identity, &client()?, &budget).await
}

async fn fetch_session(
    session: &LocalSession,
    identity: &Identity,
    io: &impl LocalIo,
    budget: &Budget<'_>,
) -> Result<Snapshot, ProbeFailure> {
    let before = request(io, &session.endpoint, Rpc::Status, budget).await?;
    require_email(&session.email, &boundary::email(&before)?)?;
    let quota = request(io, &session.endpoint, Rpc::Quota, budget).await?;
    let after = request(io, &session.endpoint, Rpc::Status, budget).await?;
    require_email(&session.email, &boundary::email(&after)?)?;
    io.validate(&session.endpoint, budget).await?;
    let snapshot = parsers::parse_antigravity(&quota, identity, SystemTime::now())?;
    budget.check()?;
    Ok(snapshot)
}

fn require_email(expected: &str, observed: &str) -> Result<(), ProbeFailure> {
    if expected == observed {
        Ok(())
    } else {
        Err(invalidated(FetchError::NeedsVerification.into()))
    }
}
fn invalidated(mut failure: ProbeFailure) -> ProbeFailure {
    failure.identity_invalidated = true;
    failure
}
fn revision(endpoint: &Endpoint, email: &str) -> CredentialRevision {
    let mut digest = Sha256::new();
    for part in [
        endpoint.process.pid.to_ne_bytes().to_vec(),
        endpoint.process.uid.to_ne_bytes().to_vec(),
        endpoint.process.ruid.to_ne_bytes().to_vec(),
        endpoint.process.start_sec.to_ne_bytes().to_vec(),
        endpoint.process.start_usec.to_ne_bytes().to_vec(),
        endpoint
            .process
            .executable
            .as_os_str()
            .as_encoded_bytes()
            .to_vec(),
        endpoint.listener.fd.to_ne_bytes().to_vec(),
        endpoint.listener.port.get().to_ne_bytes().to_vec(),
        endpoint.listener.address.to_vec(),
        endpoint.listener.generation.to_ne_bytes().to_vec(),
        endpoint.csrf.as_bytes().to_vec(),
        email.as_bytes().to_vec(),
    ] {
        digest.update((part.len() as u64).to_ne_bytes());
        digest.update(part);
    }
    CredentialRevision(digest.finalize().into())
}

#[cfg(target_os = "macos")]
async fn discover(budget: &Budget<'_>) -> Result<Vec<Endpoint>, ProbeFailure> {
    native::discover(budget).await
}
#[cfg(not(target_os = "macos"))]
async fn discover(budget: &Budget<'_>) -> Result<Vec<Endpoint>, ProbeFailure> {
    budget.check()?;
    Err(FetchError::NeedsVerification.into())
}
#[cfg(target_os = "macos")]
async fn revalidate(endpoint: &Endpoint, budget: &Budget<'_>) -> Result<(), ProbeFailure> {
    native::revalidate(endpoint, budget).await
}
#[cfg(not(target_os = "macos"))]
async fn revalidate(_: &Endpoint, budget: &Budget<'_>) -> Result<(), ProbeFailure> {
    budget.check()?;
    Err(invalidated(FetchError::NeedsVerification.into()))
}

#[cfg(any(target_os = "macos", test))]
async fn verify_child(
    mut child: tokio::process::Child,
    budget: &Budget<'_>,
) -> Result<(), ProbeFailure> {
    let result = tokio::select! {
        biased;
        () = budget.cancellation.cancelled() => Err(FetchError::TemporarilyUnavailable.into()),
        result = tokio::time::timeout_at(budget.end, child.wait()) => match result {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(_)) => Err(FetchError::NeedsVerification.into()),
            _ => Err(FetchError::TemporarilyUnavailable.into()),
        },
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct SyntheticIo {
        responses: RefCell<VecDeque<Result<Vec<u8>, ProbeFailure>>>,
        sent: RefCell<Vec<Rpc>>,
        validations: Cell<usize>,
        fail_validation: Option<usize>,
    }
    impl SyntheticIo {
        fn new(responses: &[&[u8]]) -> Self {
            Self {
                responses: RefCell::new(responses.iter().map(|bytes| Ok(bytes.to_vec())).collect()),
                sent: RefCell::new(Vec::new()),
                validations: Cell::new(0),
                fail_validation: None,
            }
        }
    }
    impl LocalIo for SyntheticIo {
        async fn validate(&self, _: &Endpoint, budget: &Budget<'_>) -> Result<(), ProbeFailure> {
            budget.check()?;
            let count = self.validations.get() + 1;
            self.validations.set(count);
            if self.fail_validation == Some(count) {
                Err(invalidated(FetchError::NeedsVerification.into()))
            } else {
                Ok(())
            }
        }
        async fn send(
            &self,
            _: &Endpoint,
            rpc: Rpc,
            _: &Budget<'_>,
        ) -> Result<Vec<u8>, ProbeFailure> {
            self.sent.borrow_mut().push(rpc);
            self.responses
                .borrow_mut()
                .pop_front()
                .expect("unexpected RPC")
        }
    }
    const STATUS_A: &[u8] = br#"{"userStatus":{"email":"a@example.invalid"}}"#;
    const STATUS_B: &[u8] = br#"{"userStatus":{"email":"b@example.invalid"}}"#;
    const QUOTA: &[u8] = br#"{"summary":{"groups":[{"buckets":[{"bucketId":"synthetic","remainingFraction":0.5}]}]}}"#;
    fn endpoint() -> Endpoint {
        Endpoint {
            process: ProcessGeneration {
                pid: 123,
                uid: 501,
                ruid: 501,
                start_sec: 1,
                start_usec: 2,
                executable: "/synthetic/server".into(),
            },
            listener: OwnedListener {
                fd: 7,
                port: NonZeroU16::new(1234).unwrap(),
                address: [127, 0, 0, 1],
                generation: 3,
            },
            csrf: "synthetic".into(),
        }
    }
    async fn prepared() -> (Identity, LocalSession) {
        let cancellation = Cancellation::default();
        prepare_endpoints(
            vec![endpoint()],
            &SyntheticIo::new(&[STATUS_A]),
            &Budget::new(&cancellation),
        )
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn publishes_only_a_complete_same_account_transaction() {
        let (identity, session) = prepared().await;
        let cancellation = Cancellation::default();
        let io = SyntheticIo::new(&[STATUS_A, QUOTA, STATUS_A]);
        let snapshot = fetch_session(&session, &identity, &io, &Budget::new(&cancellation))
            .await
            .unwrap();
        assert_eq!(snapshot.identity, identity);
        assert_eq!(*io.sent.borrow(), [Rpc::Status, Rpc::Quota, Rpc::Status]);
        assert_eq!(io.validations.get(), 4);
        for responses in [vec![STATUS_B], vec![STATUS_A, QUOTA, STATUS_B]] {
            let io = SyntheticIo::new(&responses);
            assert!(
                fetch_session(&session, &identity, &io, &Budget::new(&cancellation))
                    .await
                    .unwrap_err()
                    .identity_invalidated
            );
        }
    }
    #[tokio::test]
    async fn unavailable_status_identity_rejects_new_data_without_confirming_a_switch() {
        let (identity, session) = prepared().await;
        let cancellation = Cancellation::default();
        for status in [
            b"{}".as_slice(),
            b"{",
            br#"{"code":7,"userStatus":{"email":"a@example.invalid"}}"#,
        ] {
            let io = SyntheticIo::new(&[STATUS_A, QUOTA, status]);
            let failure = fetch_session(&session, &identity, &io, &Budget::new(&cancellation))
                .await
                .unwrap_err();
            assert!(!failure.identity_invalidated);
        }
    }
    #[tokio::test]
    async fn identity_unknown_or_ambiguous_candidates_fail_closed() {
        let cancellation = Cancellation::default();
        for second in [STATUS_B, b"{}".as_slice()] {
            let io = SyntheticIo::new(&[STATUS_A, second]);
            assert!(
                prepare_endpoints(
                    vec![endpoint(), endpoint()],
                    &io,
                    &Budget::new(&cancellation)
                )
                .await
                .is_err()
            );
        }
        let io = SyntheticIo::new(&[STATUS_A, STATUS_A]);
        assert!(
            prepare_endpoints(
                vec![endpoint(), endpoint()],
                &io,
                &Budget::new(&cancellation)
            )
            .await
            .is_ok()
        );
    }
    #[tokio::test]
    async fn changed_source_stops_token_requests_and_blocks_publication() {
        let (identity, session) = prepared().await;
        let cancellation = Cancellation::default();
        for failure_at in 1..=4 {
            let mut io = SyntheticIo::new(&[STATUS_A, QUOTA, STATUS_A]);
            io.fail_validation = Some(failure_at);
            let error = fetch_session(&session, &identity, &io, &Budget::new(&cancellation))
                .await
                .unwrap_err();
            assert!(error.identity_invalidated);
            assert_eq!(io.sent.borrow().len(), failure_at - 1);
        }
        cancellation.cancel();
        let io = SyntheticIo::new(&[STATUS_A, QUOTA, STATUS_A]);
        assert!(
            fetch_session(&session, &identity, &io, &Budget::new(&cancellation))
                .await
                .is_err()
        );
        assert!(io.sent.borrow().is_empty());
    }
    #[tokio::test(start_paused = true)]
    async fn requests_share_one_deadline() {
        let cancellation = Cancellation::default();
        let budget = Budget {
            end: Instant::now() + Duration::from_millis(30),
            cancellation: &cancellation,
        };
        budget
            .request(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            })
            .await
            .unwrap();
        let error = budget
            .request(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            })
            .await
            .unwrap_err();
        assert_eq!(error.error, FetchError::TemporarilyUnavailable);
    }
    #[tokio::test]
    async fn cancellation_reaps_the_owned_signature_process() {
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            verify_child(child, &Budget::new(&cancel))
                .await
                .unwrap_err()
                .error,
            FetchError::TemporarilyUnavailable
        );
        assert!(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
    #[tokio::test]
    async fn total_deadline_reaps_the_owned_signature_process() {
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let cancel = Cancellation::default();
        let budget = Budget {
            end: Instant::now(),
            cancellation: &cancel,
        };
        assert_eq!(
            verify_child(child, &budget).await.unwrap_err().error,
            FetchError::TemporarilyUnavailable
        );
        assert!(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
}
