use crate::{Cancellation, CredentialRevision, ProbeFailure};
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use usage_core::*;

pub(crate) struct Credential {
    pub token: String,
    pub workspace: Option<String>,
    pub revision: CredentialRevision,
}
fn revision(bytes: &[u8]) -> CredentialRevision {
    CredentialRevision(Sha256::digest(bytes).into())
}
fn nonempty(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.trim().is_empty())
}
fn home() -> Result<PathBuf, FetchError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(FetchError::NeedsAuthentication)
}
fn config_dir(variable: &str, default: &str) -> Result<PathBuf, FetchError> {
    match std::env::var_os(variable) {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(home()?.join(default)),
    }
}
async fn read(path: PathBuf) -> Result<Vec<u8>, FetchError> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            FetchError::PermissionDenied
        } else {
            FetchError::NeedsAuthentication
        }
    })?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(262145)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| FetchError::TemporarilyUnavailable)?;
    if bytes.len() > 262144 {
        return Err(FetchError::NeedsVerification);
    }
    Ok(bytes)
}
async fn read_cancellable(
    path: PathBuf,
    cancellation: &Cancellation,
) -> Result<Vec<u8>, FetchError> {
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(FetchError::TemporarilyUnavailable),
        result = tokio::time::timeout(std::time::Duration::from_secs(3), read(path)) =>
            result.unwrap_or(Err(FetchError::TemporarilyUnavailable)),
    }
}
fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}
fn identity(
    provider: Provider,
    account: String,
    workspace: Option<String>,
) -> Result<Identity, FetchError> {
    Ok(Identity {
        provider,
        account: AccountId::new(account).map_err(|_| FetchError::NeedsVerification)?,
        workspace: workspace
            .map(WorkspaceId::new)
            .transpose()
            .map_err(|_| FetchError::NeedsVerification)?,
    })
}
pub(crate) async fn load(
    provider: Provider,
    cancellation: &Cancellation,
) -> Result<(Identity, Credential), ProbeFailure> {
    let bytes = match provider {
        Provider::Codex => {
            read_cancellable(
                config_dir("CODEX_HOME", ".codex")?.join("auth.json"),
                cancellation,
            )
            .await?
        }
        Provider::GrokBot => {
            read_cancellable(
                config_dir("GROK_HOME", ".grok")?.join("auth.json"),
                cancellation,
            )
            .await?
        }
        Provider::Claude | Provider::Antigravity => {
            return Err(FetchError::NeedsVerification.into());
        }
    };
    decode(
        provider,
        &bytes,
        std::env::var("LIGHT_USAGE_BAR_GROK_SCOPE").ok().as_deref(),
    )
}

fn decode(
    provider: Provider,
    bytes: &[u8],
    scope: Option<&str>,
) -> Result<(Identity, Credential), ProbeFailure> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| FetchError::NeedsVerification)?;
    let rev = revision(bytes);
    let (token, account, workspace) = match provider {
        Provider::Codex => {
            if v["auth_mode"].as_str().is_some_and(|m| m != "chatgpt") {
                return Err(FetchError::NeedsAuthentication.into());
            }
            let token =
                nonempty(&v["tokens"]["access_token"]).ok_or(FetchError::NeedsAuthentication)?;
            let workspace =
                nonempty(&v["tokens"]["account_id"]).ok_or(FetchError::NeedsVerification)?;
            let claims = nonempty(&v["tokens"]["id_token"])
                .and_then(claims)
                .or_else(|| claims(token))
                .ok_or(FetchError::NeedsVerification)?;
            let auth = &claims["https://api.openai.com/auth"];
            if nonempty(&auth["chatgpt_account_id"]).is_some_and(|id| id != workspace) {
                return Err(FetchError::NeedsVerification.into());
            }
            if auth["chatgpt_account_is_fedramp"] == true {
                return Err(FetchError::Unsupported.into());
            }
            let account = nonempty(&auth["chatgpt_user_id"])
                .or_else(|| nonempty(&auth["user_id"]))
                .ok_or(FetchError::NeedsVerification)?;
            (
                token.to_owned(),
                account.to_owned(),
                Some(workspace.to_owned()),
            )
        }
        Provider::GrokBot => {
            let entries = v.as_object().ok_or(FetchError::NeedsVerification)?;
            let candidates: Vec<_> = entries
                .iter()
                .filter(|(_, e)| {
                    nonempty(&e["oidc_issuer"]).is_some()
                        && nonempty(&e["key"]).is_some()
                        && nonempty(&e["user_id"]).is_some()
                })
                .collect();
            let entry = if let Some(scope) = scope {
                entries.get(scope).ok_or(FetchError::NeedsVerification)?
            } else if candidates.len() == 1 {
                candidates[0].1
            } else {
                return Err(FetchError::NeedsVerification.into());
            };
            if nonempty(&entry["oidc_issuer"]).is_none() {
                return Err(FetchError::NeedsVerification.into());
            }
            if entry["principal_type"]
                .as_str()
                .is_some_and(|t| t != "User")
            {
                return Err(FetchError::NeedsVerification.into());
            }
            (
                nonempty(&entry["key"])
                    .ok_or(FetchError::NeedsVerification)?
                    .to_owned(),
                nonempty(&entry["user_id"])
                    .ok_or(FetchError::NeedsVerification)?
                    .to_owned(),
                None,
            )
        }
        Provider::Claude | Provider::Antigravity => {
            return Err(FetchError::NeedsVerification.into());
        }
    };
    Ok((
        identity(provider, account, workspace.clone())?,
        Credential {
            token,
            workspace,
            revision: rev,
        },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::PreparedProbe;
    use serde_json::json;
    fn codex(workspace: &str, fedramp: bool) -> Vec<u8> {
        let payload = json!({"https://api.openai.com/auth": {"chatgpt_user_id":"synthetic-user", "chatgpt_account_id":"synthetic-workspace", "chatgpt_account_is_fedramp":fedramp}});
        let jwt = format!(
            "synthetic.{}.unsigned",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        json!({"auth_mode":"chatgpt", "tokens":{"access_token":"synthetic-access", "id_token":jwt, "account_id":workspace}}).to_string().into_bytes()
    }
    #[test]
    fn codex_rejects_api_keys_wrong_workspace_and_fedramp_routing() {
        for bytes in [
            br#"{"auth_mode":"apikey","OPENAI_API_KEY":"synthetic"}"#.to_vec(),
            codex("different", false),
            codex("synthetic-workspace", true),
        ] {
            assert!(decode(Provider::Codex, &bytes, None).is_err());
        }
        assert!(decode(Provider::Codex, &codex("synthetic-workspace", false), None,).is_ok());
    }
    #[test]
    fn grok_requires_unambiguous_official_oauth_scope() {
        let bytes = br#"{"first":{"key":"synthetic-a","user_id":"synthetic-user-a","oidc_issuer":"https://auth.x.ai"},"second":{"key":"synthetic-b","user_id":"synthetic-user-b","oidc_issuer":"https://auth.x.ai"}}"#;
        assert_eq!(
            decode(Provider::GrokBot, bytes, None).err().unwrap().error,
            FetchError::NeedsVerification
        );
        assert!(decode(Provider::GrokBot, bytes, Some("first")).is_ok());
        assert!(decode(Provider::GrokBot, br#"{"cursor-accounts":{}}"#, None,).is_err());
    }
    #[test]
    fn grok_accepts_official_user_principal_and_rejects_team_scope() {
        for (principal, accepted) in [("User", true), ("Team", false), ("user", false)] {
            let bytes = json!({"selected": {"key":"synthetic", "user_id":"synthetic-user", "oidc_issuer":"https://auth.x.ai", "principal_type":principal}}).to_string();
            assert_eq!(
                decode(Provider::GrokBot, bytes.as_bytes(), None).is_ok(),
                accepted
            );
        }
    }
    #[test]
    fn changed_credentials_change_revision_and_redact_diagnostics() {
        let bytes = codex("synthetic-workspace", false);
        let (identity, credential) = decode(Provider::Codex, &bytes, None).unwrap();
        let probe = PreparedProbe {
            identity,
            source: crate::PreparedSource::Remote(credential),
        };
        let debug = format!("{probe:?} {:?}", probe.credential_revision());
        assert!(!debug.contains("synthetic-access"));
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace("synthetic-access", "synthetic-next");
        let (identity, credential) = decode(Provider::Codex, changed.as_bytes(), None).unwrap();
        assert_eq!(&identity, probe.identity());
        assert_ne!(&credential.revision, probe.credential_revision());
    }
}
