use crate::{
    event_ownership::PanelOwnership,
    lifecycle::Lifecycle,
    placement::{Rect, Screen, place},
};
use block2::RcBlock;
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Render, Subscription, Task, Window,
    WindowAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px, rgb,
    size,
};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::AnyObject, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSRunningApplication, NSScreen, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol, NSString,
};
use std::{io, sync::mpsc, time::Duration};
use usage_display::*;

enum Inbound {
    Host(HostToPanel),
    Dismiss,
    Disconnected,
}
struct MouseMonitor(Option<Retained<AnyObject>>);
impl Drop for MouseMonitor {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            unsafe {
                NSEvent::removeMonitor(&token);
            }
        }
    }
}
struct ActivationState {
    ownership: PanelOwnership,
    sender: async_channel::Sender<Inbound>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ActivationState]
    struct ActivationObserver;
    unsafe impl NSObjectProtocol for ActivationObserver {}
    impl ActivationObserver {
        #[unsafe(method(didActivate:))]
        fn did_activate(&self, notification: &NSNotification) {
            let Some(info) = notification.userInfo() else { return };
            let Some(application) = info.objectForKey(unsafe { NSWorkspaceApplicationKey }) else { return };
            let Some(application) = application.downcast_ref::<NSRunningApplication>() else { return };
            if !self.ivars().ownership.owns_activation(application.processIdentifier()) {
                let _ = self.ivars().sender.try_send(Inbound::Dismiss);
            }
        }
    }
);
struct ActivationMonitor {
    center: Retained<NSNotificationCenter>,
    observer: Option<Retained<ActivationObserver>>,
}
impl ActivationMonitor {
    fn stop(&mut self) {
        if let Some(observer) = self.observer.take() {
            unsafe {
                self.center.removeObserver(&observer);
            }
        }
    }
}
impl Drop for ActivationMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
struct Panel {
    snapshot: DisplaySnapshot,
    output: mpsc::SyncSender<PanelToHost>,
    focus: FocusHandle,
    expanded: Option<DisplayProvider>,
    lifecycle: Lifecycle,
    _monitor: MouseMonitor,
    activation: ActivationMonitor,
    _subscriptions: Vec<Subscription>,
    _ipc: Option<Task<()>>,
}
impl Panel {
    fn new(
        output: mpsc::SyncSender<PanelToHost>,
        input: async_channel::Receiver<Inbound>,
        input_sender: async_channel::Sender<Inbound>,
        session: u64,
        ownership: PanelOwnership,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let theme_subscription = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let mtm = MainThreadMarker::new().unwrap();
        let observer = ActivationObserver::alloc(mtm).set_ivars(ActivationState {
            ownership,
            sender: input_sender.clone(),
        });
        let observer: Retained<ActivationObserver> = unsafe { msg_send![super(observer), init] };
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        unsafe {
            center.addObserver_selector_name_object(
                &observer,
                sel!(didActivate:),
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
            );
        }
        let activation = ActivationMonitor {
            center,
            observer: Some(observer),
        };
        let block = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
            let event = unsafe { event.as_ref() };
            let mtm = MainThreadMarker::new().unwrap();
            let point = event.locationInWindow();
            let point = event
                .window(mtm)
                .map_or(point, |window| window.convertPointToScreen(point));
            let top = NSScreen::screens(mtm)
                .firstObject()
                .map(|screen| screen.frame().origin.y + screen.frame().size.height)
                .unwrap_or(0.);
            let control_pressed = event
                .modifierFlags()
                .contains(NSEventModifierFlags::Control);
            if !ownership.owns_click(
                event.r#type() == NSEventType::LeftMouseDown,
                control_pressed,
                point.x,
                top - point.y,
            ) {
                let _ = input_sender.try_send(Inbound::Dismiss);
            }
        });
        let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown,
            &block,
        );
        let ipc = cx.spawn_in(window, async move |this, cx| {
            let mut ready = false;
            while let Ok(message) = input.recv().await {
                if cx
                    .update(|window, cx| {
                        this.update(cx, |this, cx| match message {
                            Inbound::Host(HostToPanel::Replace { snapshot })
                                if snapshot.revision >= this.snapshot.revision =>
                            {
                                if this.expanded.is_some_and(|id| {
                                    !snapshot
                                        .providers
                                        .iter()
                                        .any(|provider| provider.provider == id)
                                }) {
                                    this.expanded = None;
                                }
                                this.snapshot = snapshot;
                                cx.notify();
                                if !ready {
                                    let output = this.output.clone();
                                    window.on_next_frame(move |_, _| {
                                        let _ = output.try_send(PanelToHost::Ready {
                                            protocol: PROTOCOL,
                                            session,
                                        });
                                    });
                                    ready = true;
                                }
                            }
                            Inbound::Host(HostToPanel::Close {}) | Inbound::Disconnected => {
                                this.close(window)
                            }
                            Inbound::Dismiss => this.dismiss(window),
                            Inbound::Host(HostToPanel::Hello { .. }) => this.close(window),
                            _ => {}
                        })
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            snapshot: empty_snapshot(false),
            output,
            focus,
            expanded: None,
            lifecycle: Lifecycle::Active,
            _monitor: MouseMonitor(monitor),
            activation,
            _subscriptions: vec![theme_subscription],
            _ipc: Some(ipc),
        }
    }
    fn dismiss(&mut self, window: &mut Window) {
        if self.lifecycle.dismiss() {
            self.remove_window(window);
        }
    }
    fn close(&mut self, window: &mut Window) {
        if self.lifecycle.close() {
            self.remove_window(window);
        }
    }
    fn remove_window(&mut self, window: &mut Window) {
        self.activation.stop();
        self._monitor.0.take().inspect(|token| unsafe {
            NSEvent::removeMonitor(token);
        });
        let _ = self.output.try_send(PanelToHost::Closed {});
        window.remove_window();
    }
    fn quit(&mut self) {
        self.lifecycle
            .request_quit(self.output.clone(), PanelToHost::Quit {});
    }
    fn send(&mut self, message: PanelToHost, window: &mut Window) {
        if self.lifecycle == Lifecycle::Active && self.output.try_send(message).is_err() {
            self.close(window);
        }
    }
}
#[derive(Clone, Copy)]
struct Palette {
    background: gpui::Hsla,
    card: gpui::Hsla,
    text: gpui::Hsla,
    secondary: gpui::Hsla,
    border: gpui::Hsla,
    accent: gpui::Hsla,
    hover: gpui::Hsla,
    pressed: gpui::Hsla,
}
impl Palette {
    fn new(dark: bool) -> Self {
        let color = |light, dark_color| rgb(if dark { dark_color } else { light }).into();
        Self {
            background: color(0xf5f6f8, 0x1c1c1e),
            card: color(0xffffff, 0x272729),
            text: color(0x20242c, 0xf2f2f7),
            secondary: color(0x606875, 0xb7b7bf),
            border: color(0xe1e4ea, 0x3a3a3c),
            accent: color(0x536bd7, 0x9aabff),
            hover: color(0xeceffb, 0x383c50),
            pressed: color(0xdde3f8, 0x474e6b),
        }
    }
}
fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .px(px(8.))
        .py(px(4.))
        .rounded(px(5.))
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(p.secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(p.hover).text_color(p.text))
        .active(move |style| style.bg(p.pressed))
        .child(label)
}
fn action_id(provider: DisplayProvider, action: &'static str) -> gpui::ElementId {
    let key = match provider {
        DisplayProvider::Claude => "claude",
        DisplayProvider::Codex => "codex",
        DisplayProvider::GrokBot => "grok-bot",
        DisplayProvider::Antigravity => "antigravity",
    };
    gpui::ElementId::NamedChild(Box::new(gpui::ElementId::Name(key.into())), action.into())
}
fn quota_row(quota: &crate::presentation::QuotaRow<'_>, p: Palette) -> gpui::Div {
    use crate::presentation::{Measurement, QuotaRow, measurement, reset};
    let mut row = div().flex().flex_col().gap(px(3.)).min_w(px(0.));
    let heading = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(p.secondary)
                .child(quota.label().to_owned()),
        );
    let QuotaRow::Reported { window, .. } = quota else {
        return row.child(heading.child(div().text_color(p.secondary).child("Usage unavailable")));
    };
    match measurement(&window.measurement) {
        Measurement::Unknown => {
            row =
                row.child(heading.child(div().text_color(p.secondary).child("Usage unavailable")));
        }
        Measurement::Known { text, fraction } => {
            row = row.child(
                heading.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(text),
                ),
            );
            if let Some(fraction) = fraction {
                row = row.child(
                    div().h(px(4.)).w_full().rounded_full().bg(p.border).child(
                        div()
                            .h_full()
                            .w(gpui::relative(fraction))
                            .rounded_full()
                            .bg(p.accent),
                    ),
                );
            }
        }
    }
    row.child(
        div()
            .text_size(px(10.))
            .line_height(px(14.))
            .text_color(p.secondary)
            .child(reset(window.resets_at_ms)),
    )
}
impl Panel {
    fn provider_card(
        &self,
        provider: &ProviderDisplay,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use crate::presentation::{details, fetch, freshness, quota_rows};
        let id = provider.provider;
        let enabled = provider.enabled;
        let expanded = self.expanded == Some(id);
        let rows = quota_rows(provider);
        let total = rows.len();
        let visible = if expanded { total } else { total.min(2) };
        let disclosure = if expanded {
            "Hide details".to_owned()
        } else if total > 2 {
            format!("Details · +{} windows", total - 2)
        } else {
            "Details".to_owned()
        };
        let mut card =
            div()
                .id(action_id(id, "card"))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .gap(px(5.))
                .p(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(p.border)
                .bg(p.card)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(5.)).rounded_full().bg(p.accent))
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(id.name()),
                                ),
                        )
                        .child(button(action_id(id, "details"), disclosure, p).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.expanded = if this.expanded == Some(id) {
                                    None
                                } else {
                                    Some(id)
                                };
                                cx.notify();
                            }),
                        )),
                );
        let fetch_label = fetch(provider.fetch);
        let status = if fetch_label.is_empty() {
            freshness(provider).to_owned()
        } else {
            format!("{} · {fetch_label}", freshness(provider))
        };
        card = card.child(
            div()
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(p.secondary)
                .child(status),
        );
        if total == 0 && provider.last_success.is_some() {
            card = card.child(
                div()
                    .text_color(p.secondary)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .child("No quota windows reported"),
            );
        } else {
            for quota in &rows[..visible] {
                card = card.child(quota_row(quota, p));
            }
        }
        if expanded {
            let mut metadata = div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .pt(px(7.))
                .border_t_1()
                .border_color(p.border)
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(p.secondary);
            for line in details(provider, self.snapshot.demo) {
                metadata = metadata.child(line);
            }
            card =
                card.child(metadata).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .child(button(action_id(id, "refresh"), "Refresh", p).on_click(
                            cx.listener(move |this, _, window, _| {
                                this.send(PanelToHost::Refresh { provider: Some(id) }, window);
                            }),
                        ))
                        .child(
                            button(
                                action_id(id, "enabled"),
                                if enabled { "Disable" } else { "Enable" },
                                p,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, _| {
                                    this.send(
                                        PanelToHost::SetEnabled {
                                            provider: id,
                                            enabled: !enabled,
                                        },
                                        window,
                                    );
                                },
                            )),
                        )
                        .child(button(action_id(id, "auth"), "Sign-in help", p).on_click(
                            cx.listener(move |this, _, window, _| {
                                this.send(PanelToHost::AuthGuidance { provider: id }, window);
                            }),
                        )),
                );
        } else if matches!(
            provider.fetch,
            DisplayFetch::NeedsAuthentication | DisplayFetch::PermissionDenied
        ) {
            card = card.child(button(action_id(id, "auth"), "Sign-in help", p).on_click(
                cx.listener(move |this, _, window, _| {
                    this.send(PanelToHost::AuthGuidance { provider: id }, window);
                }),
            ));
        }
        card
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::new(matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ));
        let mut header = div()
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .line_height(px(23.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Light Usage"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(p.secondary)
                            .child(if self.snapshot.demo {
                                "Synthetic demo data · No account connected"
                            } else {
                                "Subscription usage at a glance"
                            }),
                    ),
            );
        if self.snapshot.demo {
            header = header.child(
                div()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(5.))
                    .bg(p.hover)
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(p.accent)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Demo"),
            );
        }
        let mut list = div()
            .id("provider-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(8.))
            .overflow_y_scroll();
        for provider in &self.snapshot.providers {
            list = list.child(self.provider_card(provider, p, cx));
        }
        div()
            .id("usage-panel")
            .track_focus(&self.focus)
            .key_context("UsagePanel")
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(14.))
            .size_full()
            .overflow_hidden()
            .bg(p.background)
            .text_color(p.text)
            .text_size(px(13.))
            .line_height(px(18.))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, _| {
                match event.keystroke.key.as_str() {
                    "escape" => this.dismiss(window),
                    "q" if event.keystroke.modifiers.platform => this.quit(),
                    "r" if event.keystroke.modifiers.platform => {
                        this.send(PanelToHost::Refresh { provider: None }, window)
                    }
                    _ => {}
                }
            }))
            .child(header)
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .pt(px(9.))
                    .border_t_1()
                    .border_color(p.border)
                    .child(
                        button("refresh-all", "Refresh all · ⌘R", p).on_click(cx.listener(
                            |this, _, window, _| {
                                this.send(PanelToHost::Refresh { provider: None }, window);
                            },
                        )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(3.))
                            .child(
                                button("close", "Close · Esc", p).on_click(
                                    cx.listener(|this, _, window, _| this.dismiss(window)),
                                ),
                            )
                            .child(
                                button("quit", "Quit · ⌘Q", p)
                                    .on_click(cx.listener(|this, _, _, _| this.quit())),
                            ),
                    ),
            )
    }
}
fn placement(anchor: ScreenAnchor, cx: &App) -> (Option<gpui::DisplayId>, Bounds<gpui::Pixels>) {
    let native_screens = NSScreen::screens(MainThreadMarker::new().unwrap());
    let primary_top = native_screens
        .firstObject()
        .map(|s| s.frame().origin.y + s.frame().size.height)
        .unwrap_or(0.);
    let convert = |r: objc2_foundation::NSRect| {
        Rect {
            x: r.origin.x,
            y: r.origin.y,
            width: r.size.width,
            height: r.size.height,
        }
        .to_top_left(primary_top)
    };
    let displays = cx.displays();
    let screens: Vec<_> = native_screens
        .iter()
        .filter_map(|screen| {
            let description = screen.deviceDescription();
            let number = description.objectForKey(&NSString::from_str("NSScreenNumber"))?;
            let id = number.downcast_ref::<NSNumber>()?.unsignedIntValue();
            displays
                .iter()
                .any(|display| u32::from(display.id()) == id)
                .then(|| Screen {
                    id,
                    frame: convert(screen.frame()),
                    visible: convert(screen.visibleFrame()),
                })
        })
        .collect();
    let anchor = Rect {
        x: anchor.x,
        y: anchor.y,
        width: anchor.width,
        height: anchor.height,
    };
    if let Some((id, bounds)) = place(anchor, &screens) {
        let display_id = displays
            .iter()
            .find(|display| u32::from(display.id()) == id)
            .map(|display| display.id());
        return (
            display_id,
            Bounds {
                origin: point(px(bounds.x as f32), px(bounds.y as f32)),
                size: size(px(bounds.width as f32), px(bounds.height as f32)),
            },
        );
    }
    (None, Bounds::centered(None, size(px(420.), px(560.)), cx))
}
pub fn run() {
    unsafe extern "C" {
        fn getppid() -> i32;
    }
    let host_pid = unsafe { getppid() };
    if std::env::args().len() != 1 {
        eprintln!("panel: pipe launch only");
        std::process::exit(2);
    }
    let (input_tx, input_rx) = async_channel::bounded(8);
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let reader_tx = input_tx.clone();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        let hello: io::Result<HostToPanel> = read_frame(&mut input);
        match hello {
            Ok(HostToPanel::Hello {
                protocol,
                session,
                anchor,
            }) if protocol == PROTOCOL => {
                if startup_tx.send((session, anchor)).is_err() {
                    return;
                }
            }
            _ => return,
        }
        loop {
            match read_frame(&mut input) {
                Ok(message) => {
                    if reader_tx.send_blocking(Inbound::Host(message)).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    let _ = reader_tx.send_blocking(Inbound::Disconnected);
                    return;
                }
            }
        }
    });
    let Ok((session, anchor)) = startup_rx.recv_timeout(Duration::from_secs(5)) else {
        eprintln!("panel: handshake failed");
        std::process::exit(1);
    };
    let (output_tx, output_rx) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut output = io::stdout().lock();
        while let Ok(message) = output_rx.recv() {
            if write_frame(&mut output, &message).is_err() {
                return;
            }
        }
    });
    Application::new().run(move |cx: &mut App| {
        NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let (display_id, bounds) = placement(anchor, cx);
        let options = WindowOptions {
            display_id,
            kind: WindowKind::PopUp,
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        };
        if cx
            .open_window(options, move |window, cx| {
                cx.new(|cx| {
                    Panel::new(
                        output_tx,
                        input_rx,
                        input_tx,
                        session,
                        PanelOwnership {
                            anchor,
                            host_pid,
                            helper_pid: std::process::id() as i32,
                        },
                        window,
                        cx,
                    )
                })
            })
            .is_err()
        {
            eprintln!("panel: window failed");
            cx.quit();
        }
        cx.activate(true);
    });
}
