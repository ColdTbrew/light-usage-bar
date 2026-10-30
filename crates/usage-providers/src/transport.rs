use crate::{
    Cancellation, PreparedProbe, PreparedSource, ProbeFailure, antigravity, claude_cli,
    credentials, parsers,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use std::time::{Duration, SystemTime};
use usage_core::*;

const LIMIT: usize = 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(15);

pub struct ProviderClient {
    http: Client,
}
impl ProviderClient {
    pub fn new() -> Result<Self, FetchError> {
        let http = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(DEADLINE)
            .user_agent("codex-cli")
            .build()
            .map_err(|_| FetchError::TemporarilyUnavailable)?;
        Ok(Self { http })
    }
    pub async fn prepare(&self, provider: Provider) -> Result<PreparedProbe, ProbeFailure> {
        self.prepare_cancellable(provider, &Cancellation::default())
            .await
    }
    pub async fn prepare_cancellable(
        &self,
        provider: Provider,
        cancellation: &Cancellation,
    ) -> Result<PreparedProbe, ProbeFailure> {
        if provider == Provider::Claude {
            let observation = claude_cli::collect(cancellation).await?;
            return Ok(PreparedProbe {
                identity: observation.snapshot.identity.clone(),
                source: PreparedSource::ClaudeCli(observation),
            });
        }
        if provider == Provider::Antigravity {
            let (identity, session) = antigravity::prepare(cancellation).await?;
            return Ok(PreparedProbe {
                identity,
                source: PreparedSource::Antigravity(session),
            });
        }
        let (identity, credential) = credentials::load(provider, cancellation).await?;
        Ok(PreparedProbe {
            identity,
            source: PreparedSource::Remote(credential),
        })
    }
    pub async fn fetch(&self, probe: PreparedProbe) -> Result<Snapshot, ProbeFailure> {
        self.fetch_cancellable(probe, &Cancellation::default())
            .await
    }
    pub async fn fetch_cancellable(
        &self,
        probe: PreparedProbe,
        cancellation: &Cancellation,
    ) -> Result<Snapshot, ProbeFailure> {
        if let PreparedSource::ClaudeCli(observation) = probe.source {
            return tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(FetchError::TemporarilyUnavailable.into()),
                result = std::future::ready(Ok(observation.snapshot)) => result,
            };
        }
        let credential = match &probe.source {
            PreparedSource::Antigravity(session) => {
                return antigravity::fetch(session, &probe.identity, cancellation).await;
            }
            PreparedSource::Remote(credential) => credential,
            PreparedSource::ClaudeCli(_) => unreachable!(),
        };
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(FetchError::TemporarilyUnavailable.into()),
            result = tokio::time::timeout(DEADLINE, self.fetch_inner(&probe, credential)) =>
                result.unwrap_or_else(|_| Err(FetchError::TemporarilyUnavailable.into())),
        };
        let current = credentials::load(probe.identity.provider, cancellation).await;
        let (identity, credential) = current.map_err(invalidate)?;
        if identity != probe.identity || credential.revision != *probe.credential_revision() {
            return Err(invalidate(FetchError::NeedsVerification.into()));
        }
        result
    }
    async fn fetch_inner(
        &self,
        probe: &PreparedProbe,
        credential: &credentials::Credential,
    ) -> Result<Snapshot, ProbeFailure> {
        match probe.identity.provider {
            Provider::Codex => {
                let mut request = self
                    .http
                    .get("https://chatgpt.com/backend-api/wham/usage")
                    .bearer_auth(&credential.token)
                    .header("Accept", "application/json");
                if let Some(workspace) = &credential.workspace {
                    request = request.header("ChatGPT-Account-Id", workspace);
                }
                let bytes = body(
                    request
                        .send()
                        .await
                        .map_err(|_| FetchError::TemporarilyUnavailable)?,
                )
                .await?;
                parsers::parse_codex(&bytes, &probe.identity, SystemTime::now()).map_err(|error| {
                    let failure = error.into();
                    if error == FetchError::NeedsVerification {
                        invalidate(failure)
                    } else {
                        failure
                    }
                })
            }
            Provider::GrokBot => self.grok(probe, credential).await,
            Provider::Claude | Provider::Antigravity => Err(FetchError::NeedsVerification.into()),
        }
    }
    async fn grok(
        &self,
        probe: &PreparedProbe,
        credential: &credentials::Credential,
    ) -> Result<Snapshot, ProbeFailure> {
        use tokio_tungstenite::tungstenite::{
            client::IntoClientRequest,
            protocol::{Message, WebSocketConfig},
        };
        let mut request = "wss://computer-hub.grok.com/v1/tools?role=bot_client"
            .into_client_request()
            .map_err(|_| FetchError::InvalidResponse)?;
        let mut authorization = format!("Bearer {}", credential.token)
            .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
            .map_err(|_| FetchError::NeedsAuthentication)?;
        authorization.set_sensitive(true);
        request.headers_mut().insert("Authorization", authorization);
        let config = WebSocketConfig::default()
            .max_message_size(Some(LIMIT))
            .max_frame_size(Some(LIMIT));
        let (mut socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(config), true)
                .await
                .map_err(|e| match e {
                    tokio_tungstenite::tungstenite::Error::Http(response) => status(
                        response.status().as_u16(),
                        response
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok()),
                    ),
                    _ => FetchError::TemporarilyUnavailable.into(),
                })?;
        socket
            .send(Message::Text(
                r#"{"protocol_version":"1.0.0","kind":"bot_client"}"#.into(),
            ))
            .await
            .map_err(|_| FetchError::TemporarilyUnavailable)?;
        let mut acknowledged = false;
        for _ in 0..32 {
            let message = socket
                .next()
                .await
                .ok_or(FetchError::TemporarilyUnavailable)?
                .map_err(|_| FetchError::InvalidResponse)?;
            match message {
                Message::Ping(bytes) => {
                    socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|_| FetchError::TemporarilyUnavailable)?;
                }
                Message::Text(text) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).map_err(|_| FetchError::InvalidResponse)?;
                    if !acknowledged {
                        if !value["supported_protocol_versions"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == "1.0.0"))
                        {
                            return Err(FetchError::Unsupported.into());
                        }
                        if !value["capabilities"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == "bot.usage"))
                        {
                            return Err(FetchError::Unsupported.into());
                        }
                        let user = value["user_id"]
                            .as_str()
                            .and_then(|s| AccountId::new(s).ok())
                            .ok_or(FetchError::NeedsVerification)?;
                        if user != probe.identity.account {
                            return Err(invalidate(FetchError::NeedsVerification.into()));
                        }
                        acknowledged = true;
                        socket
                            .send(Message::Text(
                                r#"{"jsonrpc":"2.0","id":1,"method":"bot.usage","params":{}}"#
                                    .into(),
                            ))
                            .await
                            .map_err(|_| FetchError::TemporarilyUnavailable)?;
                    } else if value["id"] == 1 {
                        let parsed = parsers::parse_grok(
                            text.as_bytes(),
                            &probe.identity,
                            SystemTime::now(),
                        );
                        let _ = socket.close(None).await;
                        return parsed.map_err(Into::into);
                    }
                }
                Message::Close(_) => return Err(FetchError::TemporarilyUnavailable.into()),
                _ => {}
            }
        }
        Err(FetchError::InvalidResponse.into())
    }
}
pub(crate) async fn body(response: reqwest::Response) -> Result<Vec<u8>, ProbeFailure> {
    if response.status() != StatusCode::OK {
        return Err(status(
            response.status().as_u16(),
            response
                .headers()
                .get("Retry-After")
                .and_then(|h| h.to_str().ok()),
        ));
    }
    if response.content_length().is_some_and(|n| n > LIMIT as u64) {
        return Err(FetchError::InvalidResponse.into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| FetchError::TemporarilyUnavailable)?;
        if bytes.len() + chunk.len() > LIMIT {
            return Err(FetchError::InvalidResponse.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn status(code: u16, retry: Option<&str>) -> ProbeFailure {
    ProbeFailure {
        error: match code {
            401 => FetchError::NeedsAuthentication,
            403 => FetchError::PermissionDenied,
            429 => FetchError::RateLimited,
            500..=599 => FetchError::TemporarilyUnavailable,
            _ => FetchError::InvalidResponse,
        },
        retry_after: retry.and_then(|s| retry_after(s, SystemTime::now())),
        identity_invalidated: false,
    }
}
fn invalidate(mut failure: ProbeFailure) -> ProbeFailure {
    failure.identity_invalidated = true;
    failure
}
/// Parse both Retry-After wire forms against a supplied local retrieval time.
pub fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::http::Response;
    #[tokio::test]
    async fn claude_fetch_consumes_the_completed_observation_without_source_access() {
        let identity = Identity {
            provider: Provider::Claude,
            account: AccountId::observation(),
            workspace: None,
        };
        let snapshot = Snapshot {
            identity: identity.clone(),
            windows: vec![],
            timing: Timing {
                observed_at: None,
                fetched_at: SystemTime::UNIX_EPOCH,
                expires_at: SystemTime::UNIX_EPOCH,
            },
            source: Source {
                kind: SourceKind::CommandLine,
                provenance: Provenance::ProviderReported,
            },
        };
        let probe = PreparedProbe {
            identity,
            source: PreparedSource::ClaudeCli(claude_cli::ClaudeObservation {
                snapshot: snapshot.clone(),
                revision: crate::CredentialRevision([0; 32]),
            }),
        };
        assert_eq!(
            ProviderClient::new().unwrap().fetch(probe).await.unwrap(),
            snapshot
        );
    }
    #[tokio::test]
    async fn status_and_retry_hints_survive_without_response_text() {
        for (code, expected) in [
            (401, FetchError::NeedsAuthentication),
            (403, FetchError::PermissionDenied),
            (429, FetchError::RateLimited),
            (503, FetchError::TemporarilyUnavailable),
            (302, FetchError::InvalidResponse),
        ] {
            let response = Response::builder()
                .status(code)
                .header("Retry-After", "12")
                .body("synthetic private response")
                .unwrap();
            let failure = body(reqwest::Response::from(response)).await.unwrap_err();
            assert_eq!(failure.error, expected);
            assert_eq!(failure.retry_after, Some(Duration::from_secs(12)));
            assert!(!failure.identity_invalidated);
            assert!(!format!("{failure:?}").contains("synthetic private response"));
        }
    }
    #[tokio::test]
    async fn success_bodies_are_bounded_even_without_content_length() {
        let response = Response::builder()
            .status(200)
            .body(vec![0u8; LIMIT + 1])
            .unwrap();
        assert_eq!(
            body(reqwest::Response::from(response))
                .await
                .unwrap_err()
                .error,
            FetchError::InvalidResponse
        );
        let response = Response::builder()
            .status(200)
            .body(b"{}".to_vec())
            .unwrap();
        assert_eq!(
            body(reqwest::Response::from(response)).await.unwrap(),
            b"{}"
        );
    }
    #[tokio::test]
    async fn unavailable_current_local_identity_invalidates_a_prepared_probe() {
        let probe = PreparedProbe {
            identity: Identity {
                provider: Provider::Antigravity,
                account: AccountId::new("synthetic").unwrap(),
                workspace: None,
            },
            source: PreparedSource::Remote(credentials::Credential {
                token: "synthetic".into(),
                workspace: None,
                revision: crate::CredentialRevision([0; 32]),
            }),
        };
        let failure = ProviderClient::new()
            .unwrap()
            .fetch(probe)
            .await
            .unwrap_err();
        assert!(failure.identity_invalidated);
        assert_eq!(failure.error, FetchError::NeedsVerification);
    }
}
