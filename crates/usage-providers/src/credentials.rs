use crate::{Cancellation, CredentialRevision, ProbeFailure};
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
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
fn claude_source(
    config: Option<std::ffi::OsString>,
    secure: Option<std::ffi::OsString>,
    owner_home: PathBuf,
) -> (PathBuf, bool) {
    let selected = match secure {
        Some(value) => (!value.is_empty()).then_some(value),
        None => config.filter(|value| !value.is_empty()),
    };
    match selected {
        Some(path) => (PathBuf::from(path).join(".credentials.json"), false),
        None => (owner_home.join(".claude/.credentials.json"), true),
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
fn opaque(rev: &CredentialRevision) -> String {
    rev.0.iter().map(|b| format!("{b:02x}")).collect()
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
        Provider::Claude => {
            let (path, default_profile) = claude_source(
                std::env::var_os("CLAUDE_CONFIG_DIR"),
                std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR"),
                home()?,
            );
            match read_cancellable(path, cancellation).await {
                Ok(bytes) => bytes,
                Err(FetchError::NeedsAuthentication) if default_profile => {
                    keychain(cancellation).await?
                }
                Err(e) => return Err(e.into()),
            }
        }
        Provider::GrokBot => {
            read_cancellable(
                config_dir("GROK_HOME", ".grok")?.join("auth.json"),
                cancellation,
            )
            .await?
        }
        Provider::Antigravity => return Err(FetchError::NeedsVerification.into()),
    };
    decode(
        provider,
        &bytes,
        std::env::var("LIGHT_USAGE_BAR_GROK_SCOPE").ok().as_deref(),
        SystemTime::now(),
    )
}

fn decode(
    provider: Provider,
    bytes: &[u8],
    scope: Option<&str>,
    now: SystemTime,
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
        Provider::Claude => {
            let oauth = &v["claudeAiOauth"];
            let token = nonempty(&oauth["accessToken"]).ok_or(FetchError::NeedsAuthentication)?;
            if let Some(expires) = oauth["expiresAt"].as_u64()
                && now
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    >= expires as u128
            {
                return Err(FetchError::NeedsAuthentication.into());
            }
            if let Some(scopes) = oauth["scopes"].as_array()
                && !scopes.iter().any(|s| s.as_str() == Some("user:profile"))
            {
                return Err(FetchError::PermissionDenied.into());
            }
            (
                token.to_owned(),
                format!("credential:{}", opaque(&rev)),
                None,
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
        Provider::Antigravity => unreachable!(),
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
#[cfg(not(target_os = "macos"))]
async fn keychain(_cancellation: &Cancellation) -> Result<Vec<u8>, FetchError> {
    Err(FetchError::NeedsAuthentication)
}

#[cfg(target_os = "macos")]
async fn keychain(cancellation: &Cancellation) -> Result<Vec<u8>, FetchError> {
    let mut command = tokio::process::Command::new(
        std::env::current_exe().map_err(|_| FetchError::TemporarilyUnavailable)?,
    );
    command
        .arg("--credential-helper")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|_| FetchError::TemporarilyUnavailable)?;
    collect_credential(child, std::time::Duration::from_secs(2), cancellation).await
}

#[cfg(any(target_os = "macos", test))]
async fn collect_credential(
    mut child: tokio::process::Child,
    deadline: std::time::Duration,
    cancellation: &Cancellation,
) -> Result<Vec<u8>, FetchError> {
    use tokio::io::AsyncReadExt;
    let result = tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(FetchError::TemporarilyUnavailable),
        result = tokio::time::timeout(deadline, async {
        let stdout = child.stdout.take().ok_or(FetchError::TemporarilyUnavailable)?;
        let mut bytes = Vec::new();
        stdout
            .take(262145)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| FetchError::TemporarilyUnavailable)?;
        if bytes.len() > 262144 {
            return Err(FetchError::NeedsVerification);
        }
        let status = child
            .wait()
            .await
            .map_err(|_| FetchError::TemporarilyUnavailable)?;
        match status.code() {
            Some(0) => Ok(bytes),
            Some(2) => Err(FetchError::NeedsAuthentication),
            Some(3) => Err(FetchError::PermissionDenied),
            _ => Err(FetchError::NeedsVerification),
        }
    }) => result.unwrap_or(Err(FetchError::TemporarilyUnavailable)),
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

pub fn credential_helper_main() -> ! {
    #[cfg(target_os = "macos")]
    {
        use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
        use std::io::Write;
        let result = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service("Claude Code-credentials")
            .load_data(true)
            .skip_authenticated_items(true)
            .limit(2i64)
            .search();
        let mut items = match result {
            Ok(items) => items,
            Err(error) => std::process::exit(if error.code() == -25300 { 2 } else { 3 }),
        };
        if items.len() != 1 {
            std::process::exit(4);
        }
        if let SearchResult::Data(bytes) = items.remove(0)
            && bytes.len() <= 262144
            && std::io::stdout().lock().write_all(&bytes).is_ok()
        {
            std::process::exit(0);
        }
    }
    std::process::exit(4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PreparedProbe;
    use serde_json::json;
    #[tokio::test]
    async fn stalled_credential_process_is_killed_and_reaped() {
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        assert_eq!(
            collect_credential(
                child,
                std::time::Duration::from_millis(40),
                &Cancellation::default()
            )
            .await,
            Err(FetchError::TemporarilyUnavailable)
        );
        let alive = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive);
    }
    #[test]
    fn cancellation_reaps_helper_before_current_thread_runtime_teardown() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let pid = runtime.block_on(async {
            let child = tokio::process::Command::new("/bin/sleep")
                .arg("30")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let pid = child.id().unwrap();
            let cancellation = Cancellation::default();
            let mut collection = Box::pin(collect_credential(
                child,
                std::time::Duration::from_secs(30),
                &cancellation,
            ));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(collection.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            cancellation.cancel();
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(1), collection)
                    .await
                    .unwrap(),
                Err(FetchError::TemporarilyUnavailable)
            );
            pid
        });
        drop(runtime);
        let alive_or_zombie = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive_or_zombie, "helper {pid} survived runtime teardown");
        #[cfg(target_os = "linux")]
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
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
            assert!(decode(Provider::Codex, &bytes, None, UNIX_EPOCH).is_err());
        }
        assert!(
            decode(
                Provider::Codex,
                &codex("synthetic-workspace", false),
                None,
                UNIX_EPOCH
            )
            .is_ok()
        );
    }
    #[test]
    fn grok_requires_unambiguous_official_oauth_scope() {
        let bytes = br#"{"first":{"key":"synthetic-a","user_id":"synthetic-user-a","oidc_issuer":"https://auth.x.ai"},"second":{"key":"synthetic-b","user_id":"synthetic-user-b","oidc_issuer":"https://auth.x.ai"}}"#;
        assert_eq!(
            decode(Provider::GrokBot, bytes, None, UNIX_EPOCH)
                .err()
                .unwrap()
                .error,
            FetchError::NeedsVerification
        );
        assert!(decode(Provider::GrokBot, bytes, Some("first"), UNIX_EPOCH).is_ok());
        assert!(
            decode(
                Provider::GrokBot,
                br#"{"cursor-accounts":{}}"#,
                None,
                UNIX_EPOCH
            )
            .is_err()
        );
    }
    #[test]
    fn grok_accepts_official_user_principal_and_rejects_team_scope() {
        for (principal, accepted) in [("User", true), ("Team", false), ("user", false)] {
            let bytes = json!({"selected": {"key":"synthetic", "user_id":"synthetic-user", "oidc_issuer":"https://auth.x.ai", "principal_type":principal}}).to_string();
            assert_eq!(
                decode(Provider::GrokBot, bytes.as_bytes(), None, UNIX_EPOCH).is_ok(),
                accepted
            );
        }
    }
    #[test]
    fn selected_claude_profile_never_falls_back_to_default_keychain() {
        let home = PathBuf::from("/synthetic-home");
        assert_eq!(
            claude_source(Some("/selected".into()), None, home.clone()),
            (PathBuf::from("/selected/.credentials.json"), false)
        );
        assert_eq!(
            claude_source(
                Some("/selected".into()),
                Some("/secure".into()),
                home.clone()
            ),
            (PathBuf::from("/secure/.credentials.json"), false)
        );
        assert_eq!(
            claude_source(Some("/selected".into()), Some("".into()), home.clone()),
            (
                PathBuf::from("/synthetic-home/.claude/.credentials.json"),
                true
            )
        );
        assert_eq!(
            claude_source(Some("".into()), None, home),
            (
                PathBuf::from("/synthetic-home/.claude/.credentials.json"),
                true
            )
        );
    }
    #[test]
    fn expired_or_insufficient_claude_oauth_is_explicit() {
        assert_eq!(
            decode(
                Provider::Claude,
                br#"{"claudeAiOauth":{"accessToken":"synthetic","expiresAt":1}}"#,
                None,
                UNIX_EPOCH + std::time::Duration::from_secs(1)
            )
            .err()
            .unwrap()
            .error,
            FetchError::NeedsAuthentication
        );
        assert_eq!(
            decode(
                Provider::Claude,
                br#"{"claudeAiOauth":{"accessToken":"synthetic","scopes":[]}}"#,
                None,
                UNIX_EPOCH
            )
            .err()
            .unwrap()
            .error,
            FetchError::PermissionDenied
        );
    }
    #[tokio::test]
    async fn changed_credential_file_changes_revision_and_redacts_diagnostics() {
        let path =
            std::env::temp_dir().join(format!("usage-provider-synthetic-{}", std::process::id()));
        tokio::fs::write(
            &path,
            br#"{"claudeAiOauth":{"accessToken":"synthetic-first"}}"#,
        )
        .await
        .unwrap();
        let (identity, credential) = decode(
            Provider::Claude,
            &read(path.clone()).await.unwrap(),
            None,
            UNIX_EPOCH,
        )
        .unwrap();
        let probe = PreparedProbe {
            identity,
            source: crate::PreparedSource::Remote(credential),
        };
        let debug = format!("{probe:?} {:?}", probe.credential_revision());
        assert!(!debug.contains("synthetic-first"));
        assert!(!debug.contains(&opaque(probe.credential_revision())));
        tokio::fs::write(
            &path,
            br#"{"claudeAiOauth":{"accessToken":"synthetic-second"}}"#,
        )
        .await
        .unwrap();
        let (changed, credential) = decode(
            Provider::Claude,
            &read(path.clone()).await.unwrap(),
            None,
            UNIX_EPOCH,
        )
        .unwrap();
        assert_ne!(&changed, probe.identity());
        assert_ne!(&credential.revision, probe.credential_revision());
        tokio::fs::remove_file(path).await.unwrap();
    }
}
