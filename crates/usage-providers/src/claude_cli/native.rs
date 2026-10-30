#[cfg(target_os = "macos")]
use super::ClaudeObservation;
use super::parser::{self, ScreenState};
#[cfg(target_os = "macos")]
use crate::CredentialRevision;
use crate::{Cancellation, ProbeFailure};
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    num::NonZeroU16,
    os::fd::{AsRawFd, FromRawFd},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
#[cfg(target_os = "macos")]
use std::{path::Path, time::SystemTime};
use tokio::{
    io::unix::AsyncFd,
    process::{Child, Command},
    time::{Instant, timeout_at},
};
use usage_core::FetchError;
#[cfg(target_os = "macos")]
use usage_core::{AccountId, Identity, Provenance, Provider, Snapshot, Source, SourceKind, Timing};

const LIMIT: usize = 1024 * 1024;
#[cfg(target_os = "macos")]
const BUDGET: Duration = Duration::from_secs(45);
#[cfg(target_os = "macos")]
const SETTINGS: &str = r#"{"remoteControlAtStartup":false,"disableAllHooks":true}"#;

#[cfg(target_os = "macos")]
pub(super) async fn collect(cancel: &Cancellation) -> Result<ClaudeObservation, ProbeFailure> {
    let deadline = Instant::now() + BUDGET;
    let home = std::env::var_os("HOME")
        .filter(|h| Path::new(h).is_absolute())
        .ok_or(FetchError::Unsupported)?;
    let executable = [
        PathBuf::from(&home).join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .ok_or(FetchError::Unsupported)?;
    let cwd = tempfile::Builder::new()
        .prefix("light-usage-claude-")
        .tempdir()
        .map_err(|_| FetchError::TemporarilyUnavailable)?;
    let environment: Vec<_> = std::env::vars_os()
        .filter(|(k, _)| {
            let key = k.to_string_lossy();
            !key.starts_with("ANTHROPIC_")
                && !matches!(
                    key.as_ref(),
                    "CLAUDE_CODE_OAUTH_TOKEN" | "CLAUDE_CODE_OAUTH_SCOPES"
                )
        })
        .collect();
    let profile = Profile {
        output_bytes: std::sync::atomic::AtomicUsize::new(0),
        executable,
        cwd: cwd.path().to_owned(),
        environment,
    };
    let version = run(&profile, &["--version"], Mode::Text, cancel, deadline).await?;
    let version: Vec<_> = version
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .split('.')
        .filter_map(|n| n.parse::<u32>().ok())
        .collect();
    if version.len() != 3 || version[0] != 2 || version[1..] < [1, 312][..] {
        return Err(FetchError::Unsupported.into());
    }
    let help = run(&profile, &["--help"], Mode::Text, cancel, deadline).await?;
    for flag in [
        "--allowed-tools",
        "--tools",
        "--mcp-config",
        "--strict-mcp-config",
        "--settings",
        "--session-id",
    ] {
        if !help.contains(flag) {
            return Err(FetchError::Unsupported.into());
        }
    }
    let session = uuid::Uuid::new_v4().to_string();
    let screen = run(
        &profile,
        &[
            "--allowed-tools",
            "",
            "--tools",
            "",
            "--mcp-config",
            r#"{"mcpServers":{}}"#,
            "--strict-mcp-config",
            "--settings",
            SETTINGS,
            "--session-id",
            &session,
        ],
        Mode::Usage,
        cancel,
        deadline,
    )
    .await?;
    let now = SystemTime::now();
    let windows = parser::parse_screen(&screen)?;
    let mut hash = Sha256::new();
    hash.update(profile.executable.as_os_str().as_encoded_bytes());
    hash.update(home.as_encoded_bytes());
    for (key, value) in &profile.environment {
        if matches!(
            key.to_str(),
            Some("CLAUDE_CONFIG_DIR" | "CLAUDE_SECURESTORAGE_CONFIG_DIR")
        ) {
            hash.update(key.as_encoded_bytes());
            hash.update(value.as_encoded_bytes());
        }
    }
    Ok(ClaudeObservation {
        snapshot: Snapshot {
            identity: Identity {
                provider: Provider::Claude,
                account: AccountId::observation(),
                workspace: None,
            },
            windows,
            timing: Timing {
                observed_at: None,
                fetched_at: now,
                expires_at: now + Duration::from_secs(600),
            },
            source: Source {
                kind: SourceKind::CommandLine,
                provenance: Provenance::ProviderReported,
            },
        },
        revision: CredentialRevision(hash.finalize().into()),
    })
}

struct Profile {
    output_bytes: std::sync::atomic::AtomicUsize,
    executable: PathBuf,
    cwd: PathBuf,
    environment: Vec<(OsString, OsString)>,
}
#[derive(Clone, Copy)]
enum Mode {
    Text,
    Usage,
}
struct Session {
    child: Child,
    terminal: Option<AsyncFd<File>>,
    group: i32,
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.group > 0 {
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
        drop(self.terminal.take());
    }
}

impl Session {
    fn spawn(profile: &Profile, args: &[&str]) -> io::Result<Self> {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 50,
            ws_col: 140,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::addr_of_mut!(size),
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error());
        }
        let terminal = AsyncFd::new(master)?;
        let mut command = Command::new(&profile.executable);
        command
            .args(args)
            .current_dir(&profile.cwd)
            .env_clear()
            .envs(profile.environment.iter().cloned())
            .env("TERM", "xterm-256color")
            .env("NO_COLOR", "1")
            .env("DISABLE_AUTOUPDATER", "1")
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_TELEMETRY", "1")
            .env("DISABLE_ERROR_REPORTING", "1")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave))
            .kill_on_drop(true);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let group = child
            .id()
            .ok_or_else(|| io::Error::other("missing child id"))? as i32;
        Ok(Self {
            child,
            terminal: Some(terminal),
            group,
        })
    }
    async fn read(&self, bytes: &mut [u8]) -> io::Result<usize> {
        let terminal = self.terminal.as_ref().expect("session is active");
        loop {
            let mut ready = terminal.readable().await?;
            match ready.try_io(|fd| fd.get_ref().read(bytes)) {
                Ok(Err(e)) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                Ok(value) => return value,
                Err(_) => {}
            }
        }
    }
    async fn write(&self, mut bytes: &[u8]) -> io::Result<()> {
        let terminal = self.terminal.as_ref().expect("session is active");
        while !bytes.is_empty() {
            let mut ready = terminal.writable().await?;
            match ready.try_io(|fd| fd.get_ref().write(bytes)) {
                Ok(Ok(0)) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(Ok(n)) => bytes = &bytes[n..],
                Ok(Err(e)) => return Err(e),
                Err(_) => {}
            }
        }
        Ok(())
    }
    async fn cleanup(&mut self) -> io::Result<std::process::ExitStatus> {
        unsafe {
            libc::kill(-self.group, libc::SIGTERM);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
        drop(self.terminal.take());
        let status = self.child.wait().await?;
        self.group = 0;
        Ok(status)
    }
}

#[cfg(target_os = "macos")]
async fn run(
    profile: &Profile,
    args: &[&str],
    mode: Mode,
    cancel: &Cancellation,
    deadline: Instant,
) -> Result<String, ProbeFailure> {
    if Instant::now() >= deadline {
        return Err(FetchError::TemporarilyUnavailable.into());
    }
    let session = Session::spawn(profile, args).map_err(|_| FetchError::Unsupported)?;
    run_session(session, mode, cancel, deadline, &profile.output_bytes).await
}

async fn run_session(
    mut session: Session,
    mode: Mode,
    cancel: &Cancellation,
    deadline: Instant,
    output_bytes: &std::sync::atomic::AtomicUsize,
) -> Result<String, ProbeFailure> {
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(FetchError::TemporarilyUnavailable),
        result = timeout_at(deadline, interact(&session, mode, output_bytes)) => result.unwrap_or(Err(FetchError::TemporarilyUnavailable)),
    };
    let status = session
        .cleanup()
        .await
        .map_err(|_| FetchError::TemporarilyUnavailable)?;
    if matches!(mode, Mode::Text) && result.is_ok() && !status.success() {
        return Err(FetchError::Unsupported.into());
    }
    result.map_err(Into::into)
}

async fn interact(
    session: &Session,
    mode: Mode,
    output_bytes: &std::sync::atomic::AtomicUsize,
) -> Result<String, FetchError> {
    let mut terminal = vt100::Parser::new(
        NonZeroU16::new(50).unwrap(),
        NonZeroU16::new(140).unwrap(),
        0,
    );
    let mut bytes = [0u8; 8192];
    let mut text = Vec::new();
    let mut requested = false;
    let mut dsr = Vec::new();
    let mut stable = None;
    loop {
        let read = if let Some(at) = stable {
            match timeout_at(at, session.read(&mut bytes)).await {
                Ok(result) => result,
                Err(_) => return Ok(terminal.screen().contents()),
            }
        } else {
            session.read(&mut bytes).await
        };
        let n = read.map_err(|_| FetchError::InvalidResponse)?;
        if n == 0 {
            return match mode {
                Mode::Text => String::from_utf8(text).map_err(|_| FetchError::InvalidResponse),
                Mode::Usage => Err(FetchError::InvalidResponse),
            };
        }
        if output_bytes.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n > LIMIT {
            return Err(FetchError::InvalidResponse);
        }
        if matches!(mode, Mode::Text) {
            text.extend_from_slice(&bytes[..n]);
            continue;
        }
        for &byte in &bytes[..n] {
            terminal.process(&[byte]);
            dsr.push(byte);
            if dsr.ends_with(b"\x1b[6n") || dsr.ends_with(b"\x1b[?6n") {
                let private = dsr.ends_with(b"\x1b[?6n");
                let (row, col) = terminal.screen().cursor_position();
                let response = format!(
                    "\x1b[{}{};{}R",
                    if private { "?" } else { "" },
                    row + 1,
                    col + 1
                );
                session
                    .write(response.as_bytes())
                    .await
                    .map_err(|_| FetchError::InvalidResponse)?;
            }
            if dsr.len() > 6 {
                dsr.remove(0);
            }
        }
        let screen = terminal.screen().contents();
        stable = None;
        match parser::screen_state(&screen) {
            ScreenState::Authentication => return Err(FetchError::NeedsAuthentication),
            ScreenState::ApprovalRequired => return Err(FetchError::NeedsVerification),
            ScreenState::Ready if !requested => {
                session
                    .write(b"/usage\r")
                    .await
                    .map_err(|_| FetchError::InvalidResponse)?;
                requested = true;
            }
            ScreenState::Usage if requested && parser::parse_screen(&screen).is_ok() => {
                stable = Some(Instant::now() + Duration::from_millis(350));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    async fn fixture(
        script: &str,
        mode: Mode,
        cancel_after: Option<Duration>,
        budget: Duration,
    ) -> Result<String, ProbeFailure> {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("fake-claude");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\necho $$ > child-pid\n{script}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = Profile {
            output_bytes: std::sync::atomic::AtomicUsize::new(0),
            executable,
            cwd: dir.path().to_owned(),
            environment: vec![],
        };
        let cancel = Cancellation::default();
        let trigger = cancel.clone();
        let session = Session::spawn(&profile, &[]).unwrap();
        let pid = session.group;
        let marker = dir.path().join(if script.contains("descendant-pid") {
            "descendant-pid"
        } else {
            "child-pid"
        });
        timeout_at(Instant::now() + Duration::from_secs(10), async {
            while !std::fs::read_to_string(&marker)
                .is_ok_and(|pid| pid.trim().parse::<i32>().is_ok())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not finish startup");
        let cancellation = tokio::spawn(async move {
            if let Some(delay) = cancel_after {
                tokio::time::sleep(delay).await;
                trigger.cancel();
            }
        });
        let result = run_session(
            session,
            mode,
            &cancel,
            Instant::now() + budget,
            &profile.output_bytes,
        )
        .await;
        cancellation.await.unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "child still exists");
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert_eq!(
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
            -1,
            "child was not reaped"
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        if let Ok(descendant) = std::fs::read_to_string(dir.path().join("descendant-pid")) {
            let descendant = descendant.trim().parse::<i32>().unwrap();
            assert_eq!(
                unsafe { libc::kill(descendant, 0) },
                -1,
                "descendant still exists"
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            assert_eq!(
                unsafe { libc::kill(-pid, 0) },
                -1,
                "owned group still exists"
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        }
        result
    }

    #[test]
    fn descendants_are_gone_before_runtime_shutdown() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for (cancel_after, ready) in [
                (None, false),
                (Some(Duration::from_millis(100)), false),
                (None, true),
            ] {
                let script = format!(r#"/bin/sleep 30 &
 descendant=$!
 echo "$descendant" > descendant-pid
 trap 'wait "$descendant"; exit 0' TERM
 {}
 wait "$descendant""#, if ready {
                    "stty -echo\nprintf 'Claude Code\\r\\n❯\\r\\n'\nread answer\n[ \"$answer\" = /usage ] || exit 99\nprintf '\\033[2J\\033[HCurrent session\\r\\n25%% used\\r\\n'"
                } else { "" });
                let result = fixture(&script, Mode::Usage, cancel_after, Duration::from_secs(1)).await;
                if ready {
                    assert!(result.unwrap().contains("25% used"));
                } else {
                    assert_eq!(result.unwrap_err().error, FetchError::TemporarilyUnavailable);
                }
            }
        });
        drop(runtime);
    }

    #[tokio::test]
    async fn eof_reaps_child() {
        assert_eq!(
            fixture("printf '2.1.312'", Mode::Text, None, Duration::from_secs(3))
                .await
                .unwrap(),
            "2.1.312"
        );
        assert_eq!(
            fixture("exit 0", Mode::Usage, None, Duration::from_secs(3))
                .await
                .unwrap_err()
                .error,
            FetchError::InvalidResponse
        );
    }

    #[tokio::test]
    async fn cancellation_and_timeout_reap_term_resistant_child() {
        for cancellation in [None, Some(Duration::from_millis(80))] {
            let error = fixture(
                "trap '' TERM\nwhile :; do :; done",
                Mode::Usage,
                cancellation,
                Duration::from_secs(3),
            )
            .await
            .unwrap_err();
            assert_eq!(error.error, FetchError::TemporarilyUnavailable);
        }
    }

    #[tokio::test]
    async fn output_limit_reaps_child() {
        assert_eq!(
            fixture(
                "exec /usr/bin/yes x",
                Mode::Usage,
                None,
                Duration::from_secs(10)
            )
            .await
            .unwrap_err()
            .error,
            FetchError::InvalidResponse
        );
    }

    #[tokio::test]
    async fn authentication_and_approval_never_receive_input() {
        for (screen, expected) in [
            ("Not logged in", FetchError::NeedsAuthentication),
            (
                "Do you trust the files in this folder?",
                FetchError::NeedsVerification,
            ),
        ] {
            let script = format!("printf '{screen}\\r\\n'\nread answer\nexit 99");
            assert_eq!(
                fixture(&script, Mode::Usage, None, Duration::from_secs(3))
                    .await
                    .unwrap_err()
                    .error,
                expected
            );
        }
    }

    #[tokio::test]
    async fn fragmented_cursor_query_is_answered_before_ready() {
        let script = r#"stty -echo -icanon min 1 time 0
printf '\033['
/bin/sleep 0.02
printf '6n'
response=$(dd bs=1 count=6 2>/dev/null)
[ "$response" = "$(printf '\033[1;1R')" ] || exit 99
printf 'Claude Code\r\n❯\r\n'
read answer
[ "$answer" = /usage ] || exit 99
printf '\033[2J\033[HCurrent session\r\n15%% used\r\n'
read answer"#;
        assert!(
            fixture(script, Mode::Usage, None, Duration::from_secs(3))
                .await
                .unwrap()
                .contains("15% used")
        );
    }

    #[tokio::test]
    async fn loading_and_api_billing_cannot_publish_usage() {
        for screen in ["Loading usage", "API Usage Billing 25% used"] {
            let script = format!("printf '{screen}\\r\\n'\nread answer");
            assert_eq!(
                fixture(&script, Mode::Usage, None, Duration::from_secs(1))
                    .await
                    .unwrap_err()
                    .error,
                FetchError::TemporarilyUnavailable
            );
        }
    }

    #[tokio::test]
    async fn ready_prompt_sends_only_usage_and_returns_redrawn_screen() {
        let script = r#"stty -echo
printf 'Claude Code\r\n❯\r\n'
read answer
[ "$answer" = /usage ] || exit 99
printf '\033[2J\033[HCurrent session\r\n25%% used\r\nResets 2pm\r\nCurrent week (all models)\r\n40%% used\r\n'
read answer
exit 99"#;
        let screen = fixture(script, Mode::Usage, None, Duration::from_secs(3))
            .await
            .unwrap();
        assert!(screen.contains("25% used"));
        assert!(!screen.contains("Claude Code"));
    }
}
