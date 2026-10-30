use std::{
    io,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use usage_display::{
    DisplayProvider, DisplaySnapshot, HostToPanel, MAX_FRAME, PROTOCOL, PanelToHost, ScreenAnchor,
    decode, encode,
};

const FRAME_DEADLINE: Duration = Duration::from_secs(5);
const CLOSE_DEADLINE: Duration = Duration::from_secs(2);
#[derive(Clone, Debug)]
pub struct Intent {
    pub open: bool,
    pub anchor: ScreenAnchor,
    pub refresh: [u64; 5],
    pub wake: u64,
    pub enabled: [bool; 4],
    pub enabled_revision: [u64; 4],
    pub quit: bool,
    pub auth: Option<DisplayProvider>,
}
impl Default for Intent {
    fn default() -> Self {
        Self {
            open: false,
            anchor: ScreenAnchor {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            refresh: [0; 5],
            wake: 0,
            enabled: [true; 4],
            enabled_revision: [0; 4],
            quit: false,
            auth: None,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Refresh(Option<DisplayProvider>),
    SetEnabled(DisplayProvider, bool),
    AuthGuidance(DisplayProvider),
    Wake,
    Shutdown,
}
#[derive(Clone, Debug)]
pub enum UiUpdate {
    Snapshot(DisplaySnapshot),
    PanelClosed,
    Fault(&'static str),
    Stopped,
}
#[derive(Default)]
struct Pending {
    enabled: [Option<bool>; 4],
    refresh: [bool; 5],
    auth: Option<DisplayProvider>,
    wake: bool,
}
impl Pending {
    fn any(&self) -> bool {
        self.enabled.iter().any(Option::is_some)
            || self.refresh.iter().any(|v| *v)
            || self.auth.is_some()
            || self.wake
    }
    fn flush(&mut self, actions: &mpsc::Sender<Action>) {
        for (index, provider) in DisplayProvider::ALL.into_iter().enumerate() {
            if let Some(enabled) = self.enabled[index] {
                if actions
                    .try_send(Action::SetEnabled(provider, enabled))
                    .is_err()
                {
                    return;
                }
                self.enabled[index] = None;
            }
        }
        for index in 0..5 {
            if self.refresh[index] {
                if actions
                    .try_send(Action::Refresh(DisplayProvider::ALL.get(index).copied()))
                    .is_err()
                {
                    return;
                }
                self.refresh[index] = false;
            }
        }
        if self.wake {
            if actions.try_send(Action::Wake).is_err() {
                return;
            }
            self.wake = false;
        }
        if let Some(provider) = self.auth
            && actions.try_send(Action::AuthGuidance(provider)).is_ok()
        {
            self.auth = None;
        }
    }
}
#[derive(Debug)]
enum Phase {
    Starting { wanted: bool, deadline: Instant },
    Open,
    Closing { reopen: bool, deadline: Instant },
}
struct Panel {
    child: Child,
    phase: Phase,
    events: mpsc::Receiver<io::Result<Option<PanelToHost>>>,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<io::Result<()>>,
    latest: watch::Sender<DisplaySnapshot>,
    close: watch::Sender<bool>,
    session: u64,
    reading: bool,
    exited: Option<(io::Result<ExitStatus>, Instant)>,
}
impl Drop for Panel {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}
async fn read_message(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Option<PanelToHost>> {
    let mut header = [0; 4];
    if reader.read(&mut header[..1]).await? == 0 {
        return Ok(None);
    }
    timeout(FRAME_DEADLINE, async {
        reader.read_exact(&mut header[1..]).await?;
        let len = u32::from_be_bytes(header) as usize;
        if len == 0 || len > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frame size",
            ));
        }
        let mut bytes = vec![0; len];
        reader.read_exact(&mut bytes).await?;
        decode(&bytes).map(Some)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "frame timeout"))?
}
async fn write_message(writer: &mut ChildStdin, message: &HostToPanel) -> io::Result<()> {
    let bytes = encode(message)?;
    timeout(FRAME_DEADLINE, async {
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await?;
        writer.write_all(&bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "frame timeout"))?
}
impl Panel {
    fn launch(
        path: &PathBuf,
        snapshot: DisplaySnapshot,
        anchor: ScreenAnchor,
        session: u64,
        smoke: bool,
    ) -> io::Result<Self> {
        let mut command = Command::new(path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if smoke {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .kill_on_drop(true)
            .env_clear();
        for key in [
            "HOME",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "__CF_USER_TEXT_ENCODING",
        ] {
            if let Some(v) = std::env::var_os(key) {
                command.env(key, v);
            }
        }
        let mut child = command.spawn()?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing child stdout"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing child stdin"))?;
        let (events_tx, events) = mpsc::channel(8);
        let reader = tokio::spawn(async move {
            loop {
                let event = read_message(&mut stdout).await;
                let failed = !matches!(&event, Ok(Some(_)));
                if events_tx.send(event).await.is_err() || failed {
                    break;
                }
            }
        });
        let (latest, mut snapshots) = watch::channel(snapshot);
        let (close, mut closing) = watch::channel(false);
        let writer = tokio::spawn(async move {
            write_message(
                &mut stdin,
                &HostToPanel::Hello {
                    protocol: PROTOCOL,
                    session,
                    anchor,
                },
            )
            .await?;
            let initial = snapshots.borrow_and_update().clone();
            write_message(&mut stdin, &HostToPanel::Replace { snapshot: initial }).await?;
            loop {
                tokio::select! { biased;
                    result = closing.changed() => { if result.is_err() || *closing.borrow_and_update() { return write_message(&mut stdin, &HostToPanel::Close {}).await; } }
                    result = snapshots.changed() => { if result.is_err() { return Ok(()); } let snapshot = snapshots.borrow_and_update().clone(); write_message(&mut stdin, &HostToPanel::Replace { snapshot }).await?; }
                }
            }
        });
        Ok(Self {
            child,
            phase: Phase::Starting {
                wanted: true,
                deadline: Instant::now() + FRAME_DEADLINE,
            },
            events,
            reader,
            writer,
            latest,
            close,
            session,
            reading: true,
            exited: None,
        })
    }
    fn desire(&mut self, wanted: bool) {
        match &mut self.phase {
            Phase::Starting {
                wanted: current, ..
            } => *current = wanted,
            Phase::Open if !wanted => self.begin_close(),
            Phase::Closing { reopen, .. } => *reopen = wanted,
            _ => {}
        }
    }
    fn begin_close(&mut self) {
        if !matches!(self.phase, Phase::Closing { .. }) {
            self.close.send_replace(true);
            self.phase = Phase::Closing {
                reopen: false,
                deadline: Instant::now() + CLOSE_DEADLINE,
            };
        }
    }
    fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Starting { deadline, .. } | Phase::Closing { deadline, .. } => Some(deadline),
            Phase::Open => None,
        }
    }
    async fn kill_and_reap(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

pub async fn run(
    helper: PathBuf,
    intents: watch::Sender<Intent>,
    mut snapshots: watch::Receiver<DisplaySnapshot>,
    actions: mpsc::Sender<Action>,
    update: impl Fn(UiUpdate),
    smoke: bool,
) -> io::Result<()> {
    let mut intent_rx = intents.subscribe();
    let mut last = Intent::default();
    let mut panel: Option<Panel> = None;
    let mut session = 0u64;
    let mut smoke_ready = false;
    let mut pending = Pending::default();
    update(UiUpdate::Snapshot(snapshots.borrow().clone()));
    loop {
        let intent = intent_rx.borrow_and_update().clone();
        if intent.quit {
            if let Some(mut active) = panel.take() {
                active.begin_close();
                if timeout(CLOSE_DEADLINE, active.child.wait()).await.is_err() {
                    active.kill_and_reap().await;
                }
            }
            let _ = actions.try_send(Action::Shutdown);
            update(UiUpdate::Stopped);
            return Ok(());
        }
        for index in 0..4 {
            if intent.enabled_revision[index] != last.enabled_revision[index] {
                pending.enabled[index] = Some(intent.enabled[index]);
            }
            if intent.refresh[index] != last.refresh[index] {
                pending.refresh[index] = true;
            }
        }
        if intent.refresh[4] != last.refresh[4] {
            pending.refresh[4] = true;
        }
        if intent.auth != last.auth
            && let Some(provider) = intent.auth
        {
            pending.auth = Some(provider);
        }
        if intent.wake != last.wake {
            pending.wake = true;
        }
        last = intent.clone();
        pending.flush(&actions);
        if let Some(active) = &mut panel {
            active.desire(intent.open);
        } else if intent.open {
            session = session
                .checked_add(1)
                .ok_or_else(|| io::Error::other("session exhausted"))?;
            match Panel::launch(
                &helper,
                snapshots.borrow().clone(),
                intent.anchor,
                session,
                smoke,
            ) {
                Ok(active) => {
                    if smoke {
                        eprintln!("smoke: spawned");
                    }
                    panel = Some(active);
                }
                Err(_) => {
                    intents.send_modify(|v| v.open = false);
                    update(UiUpdate::Fault("Panel launch failed"));
                    if smoke {
                        return Err(io::Error::other("smoke launch failed"));
                    }
                }
            }
        }
        if let Some(active) = &mut panel
            && active.exited.is_some()
            && !active.reading
        {
            let (status, _) = active.exited.take().unwrap();
            let reopen = matches!(active.phase, Phase::Closing { reopen: true, .. });
            let expected = status.as_ref().is_ok_and(|s| s.success());
            panel.take();
            if smoke {
                eprintln!("smoke: reaped");
                if !smoke_ready || !status.as_ref().is_ok_and(|s| s.success()) {
                    return Err(io::Error::other("smoke failed"));
                }
                intents.send_modify(|v| v.quit = true);
            } else if !reopen {
                intents.send_modify(|v| v.open = false);
                update(UiUpdate::PanelClosed);
            }
            if !expected {
                update(UiUpdate::Fault("Panel exited"));
            }
            continue;
        }
        if let Some(active) = &mut panel {
            let deadline = active
                .exited
                .as_ref()
                .map(|(_, deadline)| *deadline)
                .or_else(|| active.deadline());
            tokio::select! { biased;
                status = active.child.wait(), if active.exited.is_none() => {
                    active.exited = Some((status, Instant::now() + CLOSE_DEADLINE));
                }
                result = intent_rx.changed() => { if result.is_err() { return Err(io::Error::other("intent channel closed")); } }
                result = actions.reserve(), if pending.any() => { if result.is_err() { intents.send_modify(|v| v.quit = true); } }
                result = snapshots.changed() => {
                    if result.is_err() { intents.send_modify(|v| v.quit = true); }
                    else { let snapshot = snapshots.borrow_and_update().clone(); active.latest.send_replace(snapshot.clone()); update(UiUpdate::Snapshot(snapshot)); }
                }
                event = active.events.recv(), if active.reading => {
                    match event {
                        Some(Ok(Some(PanelToHost::Ready { protocol, session }))) if protocol == PROTOCOL && session == active.session => {
                            if let Phase::Starting { wanted, .. } = active.phase { if wanted { active.phase = Phase::Open; } else { active.begin_close(); } }
                            if smoke { eprintln!("smoke: ready"); smoke_ready = true; active.begin_close(); }
                        }
                        Some(Ok(Some(PanelToHost::Refresh { provider }))) if matches!(active.phase, Phase::Open) => { let index = provider.and_then(|p| DisplayProvider::ALL.iter().position(|v| *v==p)).unwrap_or(4); pending.refresh[index] = true; }
                        Some(Ok(Some(PanelToHost::SetEnabled { provider, enabled }))) if matches!(active.phase, Phase::Open) => { let index = DisplayProvider::ALL.iter().position(|p| *p == provider).unwrap(); intents.send_modify(|v| { v.enabled[index] = enabled; v.enabled_revision[index] = v.enabled_revision[index].wrapping_add(1); }); }
                        Some(Ok(Some(PanelToHost::AuthGuidance { provider }))) if matches!(active.phase, Phase::Open) => { pending.auth = Some(provider); }
                        Some(Ok(Some(PanelToHost::Quit {}))) => { intents.send_modify(|v| v.quit = true); }
                        Some(Ok(Some(PanelToHost::Closed {}))) => { if !matches!(active.phase, Phase::Closing { .. }) { active.begin_close(); intents.send_modify(|v| v.open = false); } }
                        Some(Ok(None)) | None => { active.reading = false; if !matches!(active.phase, Phase::Closing { .. }) { active.begin_close(); intents.send_modify(|v| v.open = false); } }
                        _ => { active.reading = false; if !matches!(active.phase, Phase::Closing { .. }) { active.begin_close(); intents.send_modify(|v| v.open = false); } update(UiUpdate::Fault("Panel protocol failed")); }
                    }
                }
                _ = async { match deadline { Some(when) => tokio::time::sleep_until(when).await, None => std::future::pending().await } } => {
                    active.kill_and_reap().await; panel.take(); intents.send_modify(|v| v.open = false); update(UiUpdate::Fault("Panel deadline exceeded"));
                    if smoke { return Err(io::Error::new(io::ErrorKind::TimedOut,"smoke timeout")); }
                }
            }
        } else {
            tokio::select! {
                result = intent_rx.changed() => { if result.is_err() { return Err(io::Error::other("intent channel closed")); } }
                result = actions.reserve(), if pending.any() => { if result.is_err() { intents.send_modify(|v| v.quit = true); } }
                result = snapshots.changed() => { if result.is_err() { intents.send_modify(|v| v.quit = true); } else { update(UiUpdate::Snapshot(snapshots.borrow_and_update().clone())); } }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, fs, os::unix::fs::PermissionsExt};

    static PROCESS_FIXTURES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(name: &str, body: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut attempt = 0u64;
            let directory = loop {
                let candidate = std::env::temp_dir().join(format!(
                    "usage-app-{}-{stamp}-{attempt}",
                    std::process::id()
                ));
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
                    Err(error) => panic!("fixture directory: {error}"),
                }
            };
            let path = directory.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_dir(self.0.parent().unwrap());
        }
    }
    struct Channels {
        intent: watch::Sender<Intent>,
        _snapshots: watch::Sender<DisplaySnapshot>,
        receiver: watch::Receiver<DisplaySnapshot>,
        actions: mpsc::Sender<Action>,
        _commands: mpsc::Receiver<Action>,
    }
    fn setup() -> Channels {
        let (intent, _) = watch::channel(Intent {
            open: true,
            ..Default::default()
        });
        let (snapshots, receiver) = watch::channel(usage_display::demo_snapshot());
        let (actions, commands) = mpsc::channel(8);
        Channels {
            intent,
            _snapshots: snapshots,
            receiver,
            actions,
            _commands: commands,
        }
    }
    #[tokio::test]
    async fn helper_handshake_close_and_reap_complete_without_polling() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let ready = encode(&PanelToHost::Ready {
            protocol: PROTOCOL,
            session: 1,
        })
        .unwrap();
        let frame = (ready.len() as u32)
            .to_be_bytes()
            .into_iter()
            .chain(ready)
            .map(|byte| format!("\\{byte:03o}"))
            .collect::<String>();
        let helper = Fixture::new(
            "ready",
            &format!("printf '{frame}'\nexec /bin/cat >/dev/null"),
        );
        let Channels {
            intent,
            _snapshots,
            receiver,
            actions,
            _commands,
        } = setup();
        let updates = RefCell::new(Vec::new());
        timeout(
            Duration::from_secs(4),
            run(
                helper.0.clone(),
                intent,
                receiver,
                actions,
                |v| updates.borrow_mut().push(v),
                true,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            updates
                .borrow()
                .iter()
                .any(|v| matches!(v, UiUpdate::Stopped))
        );
    }
    #[tokio::test]
    async fn malformed_helper_is_killed_and_reaped_after_close_deadline() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let helper = Fixture::new(
            "malformed",
            "printf '\\000\\004\\000\\001'\nexec /bin/sleep 60",
        );
        let Channels {
            intent,
            _snapshots,
            receiver,
            actions,
            _commands,
        } = setup();
        let updates = RefCell::new(Vec::new());
        assert!(
            timeout(
                Duration::from_secs(4),
                run(
                    helper.0.clone(),
                    intent,
                    receiver,
                    actions,
                    |v| updates.borrow_mut().push(v),
                    true
                )
            )
            .await
            .unwrap()
            .is_err()
        );
        assert!(
            updates
                .borrow()
                .iter()
                .any(|v| matches!(v, UiUpdate::Fault("Panel protocol failed")))
        );
        assert!(
            updates
                .borrow()
                .iter()
                .any(|v| matches!(v, UiUpdate::Fault("Panel deadline exceeded")))
        );
    }
    #[tokio::test]
    async fn quit_written_before_helper_exit_always_stops_host() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let frames = [
            PanelToHost::Ready {
                protocol: PROTOCOL,
                session: 1,
            },
            PanelToHost::Quit {},
        ]
        .into_iter()
        .flat_map(|message| {
            let bytes = encode(&message).unwrap();
            (bytes.len() as u32).to_be_bytes().into_iter().chain(bytes)
        })
        .map(|byte| format!("\\{byte:03o}"))
        .collect::<String>();
        let helper = Fixture::new("quit-exit", &format!("printf '{frames}'"));
        for _ in 0..50 {
            let Channels {
                intent,
                _snapshots,
                receiver,
                actions,
                mut _commands,
            } = setup();
            timeout(
                Duration::from_secs(1),
                run(helper.0.clone(), intent, receiver, actions, |_| {}, false),
            )
            .await
            .expect("host lost Quit before helper exit")
            .unwrap();
            assert!(matches!(_commands.recv().await, Some(Action::Shutdown)));
        }
    }

    #[tokio::test]
    async fn clean_close_and_eof_leave_no_error_status() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let frames = [
            PanelToHost::Ready {
                protocol: PROTOCOL,
                session: 1,
            },
            PanelToHost::Closed {},
        ]
        .into_iter()
        .flat_map(|message| {
            let bytes = encode(&message).unwrap();
            (bytes.len() as u32).to_be_bytes().into_iter().chain(bytes)
        })
        .map(|byte| format!("\\{byte:03o}"))
        .collect::<String>();
        let helper = Fixture::new("clean-close", &format!("printf '{frames}'"));
        let Channels {
            intent,
            _snapshots,
            receiver,
            actions,
            _commands,
        } = setup();
        let observed = intent.clone();
        timeout(
            Duration::from_secs(3),
            run(
                helper.0.clone(),
                intent,
                receiver,
                actions,
                |update| {
                    assert!(!matches!(update, UiUpdate::Fault(_)), "{update:?}");
                    if matches!(update, UiUpdate::PanelClosed) {
                        observed.send_modify(|intent| intent.quit = true);
                    }
                },
                false,
            ),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn truncated_frame_is_not_a_clean_eof() {
        let mut empty = &b""[..];
        assert!(read_message(&mut empty).await.unwrap().is_none());
        let mut partial = &b"\0\0"[..];
        assert_eq!(
            read_message(&mut partial).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn unsuccessful_helper_exit_is_reported_after_clean_eof() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let helper = Fixture::new("failed-exit", "exit 1");
        let Channels {
            intent,
            _snapshots,
            receiver,
            actions,
            _commands,
        } = setup();
        let observed = intent.clone();
        let faults = RefCell::new(Vec::new());
        timeout(
            Duration::from_secs(3),
            run(
                helper.0.clone(),
                intent,
                receiver,
                actions,
                |update| {
                    if let UiUpdate::Fault(fault) = update {
                        faults.borrow_mut().push(fault);
                    }
                    if matches!(update, UiUpdate::PanelClosed) {
                        observed.send_modify(|intent| intent.quit = true);
                    }
                },
                false,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*faults.borrow(), vec!["Panel exited"]);
    }

    #[tokio::test]
    async fn exited_helper_clears_open_intent_without_relaunching() {
        let _process_guard = PROCESS_FIXTURES.lock().await;
        let helper = Fixture::new("exit", "exit 0");
        let Channels {
            intent,
            _snapshots,
            receiver,
            actions,
            _commands,
        } = setup();
        let observed = intent.clone();
        timeout(
            Duration::from_secs(1),
            run(
                helper.0.clone(),
                intent,
                receiver,
                actions,
                |update| {
                    if matches!(update, UiUpdate::PanelClosed) {
                        assert!(!observed.borrow().open);
                        observed.send_modify(|intent| intent.quit = true);
                    }
                },
                false,
            ),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[test]
    fn saturated_action_queue_retains_latest_enable_and_coalesced_refresh() {
        let (sender, mut receiver) = mpsc::channel(1);
        sender.try_send(Action::Refresh(None)).unwrap();
        let mut pending = Pending::default();
        pending.enabled[0] = Some(false);
        pending.refresh[4] = true;
        pending.flush(&sender);
        assert!(pending.any());
        receiver.try_recv().unwrap();
        pending.flush(&sender);
        assert!(matches!(
            receiver.try_recv(),
            Ok(Action::SetEnabled(DisplayProvider::Claude, false))
        ));
        pending.flush(&sender);
        assert!(matches!(receiver.try_recv(), Ok(Action::Refresh(None))));
        assert!(!pending.any());
    }
}
