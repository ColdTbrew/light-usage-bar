use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, watch};
use usage_app::engine::{self, Action, Intent, UiUpdate};
use usage_display::*;

thread_local! { static HOST: RefCell<Option<Host>> = const { RefCell::new(None) }; }
struct Host {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<Target>,
    intents: watch::Sender<Intent>,
    terminating: bool,
    termination_pending: bool,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Target;
    unsafe impl NSObjectProtocol for Target {}
    unsafe impl NSApplicationDelegate for Target {
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _: &NSApplication) -> NSApplicationTerminateReply {
            HOST.with_borrow_mut(|host| {
                let host = host.as_mut().unwrap();
                if host.terminating { NSApplicationTerminateReply::TerminateNow } else { host.termination_pending = true; host.intents.send_modify(|v| v.quit = true); NSApplicationTerminateReply::TerminateLater }
            })
        }
    }
    impl Target {
        #[unsafe(method(didWake:))]
        fn did_wake(&self, _: &NSNotification) { HOST.with_borrow(|host| { host.as_ref().unwrap().intents.send_modify(|v| v.wake=v.wake.wrapping_add(1)); }); }
        #[unsafe(method(togglePanel:))]
        fn toggle(&self, _: Option<&NSObject>) { HOST.with_borrow(|host| { let host = host.as_ref().unwrap(); let anchor = anchor(&host.item); host.intents.send_modify(|v| { v.anchor = anchor; v.open = !v.open; }); }); }
        #[unsafe(method(refreshAll:))]
        fn refresh_all(&self, _: Option<&NSObject>) { HOST.with_borrow(|host| { host.as_ref().unwrap().intents.send_modify(|v| v.refresh[4] = v.refresh[4].wrapping_add(1)); }); }
        #[unsafe(method(providerToggle:))]
        fn provider_toggle(&self, sender: &NSMenuItem) { let index = sender.tag() as usize; if index < 4 { HOST.with_borrow(|host| { host.as_ref().unwrap().intents.send_modify(|v| { v.enabled[index] = sender.state() == 0; v.enabled_revision[index] = v.enabled_revision[index].wrapping_add(1); }); }); } }
        #[unsafe(method(providerRefresh:))]
        fn provider_refresh(&self, sender: &NSMenuItem) { let index = sender.tag() as usize; if index < 4 { HOST.with_borrow(|host| { host.as_ref().unwrap().intents.send_modify(|v| v.refresh[index] = v.refresh[index].wrapping_add(1)); }); } }
        #[unsafe(method(providerAuth:))]
        fn provider_auth(&self, sender: &NSMenuItem) { let index = sender.tag() as usize; if let Some(provider) = DisplayProvider::ALL.get(index) { guidance(*provider); } }
        #[unsafe(method(quitApp:))]
        fn quit(&self, _: Option<&NSObject>) { HOST.with_borrow(|host| { host.as_ref().unwrap().intents.send_modify(|v| v.quit = true); }); }
    }
);
fn anchor(item: &NSStatusItem) -> ScreenAnchor {
    let mtm = MainThreadMarker::new().unwrap();
    let button = item.button(mtm).unwrap();
    let window = button.window().unwrap();
    let frame = window.frame();
    let screens = NSScreen::screens(mtm);
    let top = screens
        .firstObject()
        .map(|s| s.frame().origin.y + s.frame().size.height)
        .unwrap_or(0.);
    ScreenAnchor {
        x: frame.origin.x,
        y: top - frame.origin.y - frame.size.height,
        width: frame.size.width.max(1.),
        height: frame.size.height.max(1.),
    }
}
fn menu_item(
    title: &str,
    action: Option<objc2::runtime::Sel>,
    target: &Target,
    tag: usize,
) -> Retained<NSMenuItem> {
    let mtm = MainThreadMarker::new().unwrap();
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::new(),
        )
    };
    item.setTag(tag as isize);
    unsafe {
        item.setTarget(Some(target));
    }
    item
}
fn render(snapshot: &DisplaySnapshot) {
    HOST.with_borrow_mut(|host| {
        let host = host.as_mut().unwrap();
        host.menu.removeAllItems();
        host.menu.addItem(&menu_item(
            if snapshot.demo {
                "Open synthetic demo panel"
            } else {
                "Open usage panel"
            },
            Some(sel!(togglePanel:)),
            &host.target,
            0,
        ));
        for provider in &snapshot.providers {
            let index = DisplayProvider::ALL
                .iter()
                .position(|p| *p == provider.provider)
                .unwrap();
            host.menu
                .addItem(&NSMenuItem::separatorItem(MainThreadMarker::new().unwrap()));
            for line in provider_lines(provider) {
                let item = menu_item(&line, None, &host.target, index);
                item.setEnabled(false);
                host.menu.addItem(&item);
            }
            let enabled = menu_item(
                &format!(
                    "{} {}",
                    if provider.enabled {
                        "Disable"
                    } else {
                        "Enable"
                    },
                    provider.provider.name()
                ),
                Some(sel!(providerToggle:)),
                &host.target,
                index,
            );
            enabled.setState(if provider.enabled { 1 } else { 0 });
            host.menu.addItem(&enabled);
            host.menu.addItem(&menu_item(
                &format!("Refresh {}", provider.provider.name()),
                Some(sel!(providerRefresh:)),
                &host.target,
                index,
            ));
            host.menu.addItem(&menu_item(
                &format!("{} sign-in guidance", provider.provider.name()),
                Some(sel!(providerAuth:)),
                &host.target,
                index,
            ));
        }
        host.menu
            .addItem(&NSMenuItem::separatorItem(MainThreadMarker::new().unwrap()));
        host.menu.addItem(&menu_item(
            "Refresh all",
            Some(sel!(refreshAll:)),
            &host.target,
            0,
        ));
        host.menu.addItem(&menu_item(
            "Quit Light Usage Bar",
            Some(sel!(quitApp:)),
            &host.target,
            0,
        ));
    });
}
fn guidance(provider: DisplayProvider) {
    let mtm = MainThreadMarker::new().unwrap();
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!("{} sign-in", provider.name())));
    alert.setInformativeText(&NSString::from_str("Sign in using the provider's official app or website, then refresh here. This app does not collect passwords. Support and quota semantics remain unverified until an authorized provider observation is available."));
    alert.runModal();
}
#[derive(Default)]
struct Mailbox {
    snapshot: Option<DisplaySnapshot>,
    fault: Option<&'static str>,
    stopped: bool,
    posted: bool,
}
fn post(mailbox: &Arc<Mutex<Mailbox>>, update: UiUpdate) {
    let mut slot = mailbox.lock().unwrap();
    match update {
        UiUpdate::Snapshot(v) => slot.snapshot = Some(v),
        UiUpdate::Fault(v) => slot.fault = Some(v),
        UiUpdate::Stopped => slot.stopped = true,
        UiUpdate::PanelClosed => {}
    }
    if slot.posted {
        return;
    }
    slot.posted = true;
    drop(slot);
    let mailbox = mailbox.clone();
    DispatchQueue::main().exec_async(move || {
        let (snapshot, fault, stopped) = {
            let mut slot = mailbox.lock().unwrap();
            slot.posted = false;
            (slot.snapshot.take(), slot.fault.take(), slot.stopped)
        };
        if let Some(snapshot) = snapshot {
            render(&snapshot);
        }
        if let Some(fault) = fault {
            HOST.with_borrow(|host| {
                if let Some(button) = host
                    .as_ref()
                    .unwrap()
                    .item
                    .button(MainThreadMarker::new().unwrap())
                {
                    button.setTitle(&NSString::from_str("Usage !"));
                    button.setToolTip(Some(&NSString::from_str(fault)));
                }
            });
        }
        if stopped {
            HOST.with_borrow_mut(|host| {
                host.as_mut().unwrap().terminating = true;
            });
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            let pending = HOST.with_borrow(|host| host.as_ref().unwrap().termination_pending);
            if pending {
                app.replyToApplicationShouldTerminate(true);
            } else {
                app.terminate(None);
            }
        }
    });
}
fn helper_path() -> PathBuf {
    let executable = std::env::current_exe().expect("executable path");
    let parent = executable.parent().unwrap();
    if parent.file_name().is_some_and(|v| v == "MacOS") {
        parent.parent().unwrap().join("Helpers/light-usage-panel")
    } else {
        parent.join("light-usage-panel")
    }
}
pub fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let smoke = args.iter().any(|v| v == "--smoke-test");
    let demo = args.iter().any(|v| v == "--demo") || smoke;
    if args.iter().any(|v| v != "--demo" && v != "--smoke-test") {
        eprintln!("usage: light-usage-bar [--demo] [--smoke-test]");
        std::process::exit(2);
    }
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let target: Retained<Target> = unsafe { msg_send![Target::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    let wake_center = NSWorkspace::sharedWorkspace().notificationCenter();
    unsafe {
        wake_center.addObserver_selector_name_object(
            &target,
            sel!(didWake:),
            Some(NSWorkspaceDidWakeNotification),
            None,
        );
    }
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    item.button(mtm)
        .unwrap()
        .setTitle(&NSString::from_str("Usage"));
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    item.setMenu(Some(&menu));
    let (intents, _) = watch::channel(Intent::default());
    let snapshot = if demo {
        demo_snapshot()
    } else {
        empty_snapshot(false)
    };
    HOST.with_borrow_mut(|host| {
        *host = Some(Host {
            item,
            menu,
            target,
            intents: intents.clone(),
            terminating: false,
            termination_pending: false,
        })
    });
    render(&snapshot);
    let (snapshots_tx, snapshots_rx) = watch::channel(snapshot);
    let (actions_tx, mut actions_rx) = mpsc::channel(8);
    let helper = helper_path();
    let mailbox = Arc::new(Mutex::new(Mailbox::default()));
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let (service_tx, service_rx) = mpsc::channel(8);
            let (status_tx, mut status_rx) = watch::channel(usage_service::initial_statuses());
            let service = if demo {
                None
            } else {
                Some(tokio::spawn(usage_service::run(service_rx, status_tx)))
            };
            let publish_snapshots = snapshots_tx.clone();
            let publisher = if demo {
                None
            } else {
                Some(tokio::spawn(async move {
                    let mut revision = 1u64;
                    loop {
                        let statuses = status_rx.borrow_and_update().clone();
                        let providers = statuses
                            .iter()
                            .map(|s| {
                                project_provider(
                                    s.provider,
                                    s.enabled,
                                    s.snapshot.as_ref(),
                                    s.freshness,
                                    s.fetch_state,
                                    s.next_refresh,
                                )
                            })
                            .collect();
                        publish_snapshots.send_replace(DisplaySnapshot {
                            revision,
                            demo: false,
                            providers,
                        });
                        revision = revision.saturating_add(1);
                        if status_rx.changed().await.is_err() {
                            break;
                        }
                    }
                }))
            };
            let action_snapshots = snapshots_tx.clone();
            let service_commands = service_tx.clone();
            let actions = tokio::spawn(async move {
                while let Some(action) = actions_rx.recv().await {
                    let command = match action {
                        Action::SetEnabled(provider, enabled) if !demo => {
                            Some(usage_service::Command::SetEnabled(provider.into(), enabled))
                        }
                        Action::Refresh(provider) if !demo => {
                            Some(usage_service::Command::Refresh(provider.map(Into::into)))
                        }
                        Action::Wake if !demo => Some(usage_service::Command::Wake),
                        Action::Wake => None,
                        Action::AuthGuidance(provider) => {
                            DispatchQueue::main().exec_async(move || guidance(provider));
                            None
                        }
                        Action::SetEnabled(provider, enabled) => {
                            action_snapshots.send_modify(|s| {
                                s.revision = s.revision.saturating_add(1);
                                if let Some(p) =
                                    s.providers.iter_mut().find(|p| p.provider == provider)
                                {
                                    p.enabled = enabled;
                                }
                            });
                            None
                        }
                        Action::Refresh(_) => None,
                        Action::Shutdown => break,
                    };
                    if let Some(command) = command
                        && service_commands.send(command).await.is_err()
                    {
                        break;
                    }
                }
            });
            if smoke {
                intents.send_modify(|v| v.open = true);
            }
            let result = engine::run(
                helper,
                intents.clone(),
                snapshots_rx,
                actions_tx,
                |v| {
                    if smoke && let UiUpdate::Fault(fault) = &v {
                        eprintln!("smoke: {fault}");
                    }
                    if !matches!(v, UiUpdate::Stopped) {
                        post(&mailbox, v);
                    }
                },
                smoke,
            )
            .await;
            if result.is_err() {
                eprintln!("host: engine failed");
            }
            let _ = actions.await;
            let _ = service_tx.send(usage_service::Command::Shutdown).await;
            if let Some(service) = service {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), service).await;
            }
            if let Some(publisher) = publisher {
                publisher.abort();
            }
            if smoke && result.is_err() {
                std::process::exit(1);
            }
            post(&mailbox, UiUpdate::Stopped);
        });
    });
    app.run();
    let _ = thread.join();
    HOST.with_borrow_mut(|host| {
        if let Some(host) = host.take() {
            unsafe {
                wake_center.removeObserver(&host.target);
            }
            NSStatusBar::systemStatusBar().removeStatusItem(&host.item);
        }
    });
}
