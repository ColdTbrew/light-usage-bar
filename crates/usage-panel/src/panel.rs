use crate::{
    lifecycle::Lifecycle,
    placement::{Rect, Screen, place},
};
use block2::RcBlock;
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Render, Subscription, Task, Window,
    WindowAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px, rgb,
    size,
};
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask, NSScreen};
use objc2_foundation::{NSNumber, NSString};
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
struct Panel {
    snapshot: DisplaySnapshot,
    output: mpsc::SyncSender<PanelToHost>,
    focus: FocusHandle,
    active_once: bool,
    lifecycle: Lifecycle,
    _monitor: MouseMonitor,
    _subscriptions: Vec<Subscription>,
    _ipc: Option<Task<()>>,
}
impl Panel {
    fn new(
        snapshot: DisplaySnapshot,
        output: mpsc::SyncSender<PanelToHost>,
        input: async_channel::Receiver<Inbound>,
        input_sender: async_channel::Sender<Inbound>,
        session: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let focus_subscription = cx.observe_window_activation(window, |this, window, _| {
            if window.is_window_active() {
                this.active_once = true;
            } else if this.active_once {
                this.dismiss(window);
            }
        });
        let theme_subscription = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let block = RcBlock::new(move |_: std::ptr::NonNull<NSEvent>| {
            let _ = input_sender.try_send(Inbound::Dismiss);
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
            snapshot,
            output,
            focus,
            active_once: false,
            lifecycle: Lifecycle::Active,
            _monitor: MouseMonitor(monitor),
            _subscriptions: vec![focus_subscription, theme_subscription],
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
fn button(label: impl Into<gpui::SharedString>) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(label.clone())
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x7a8599))
        .cursor_pointer()
        .child(label)
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let mut layout = div()
            .id("usage-panel")
            .track_focus(&self.focus)
            .key_context("UsagePanel")
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(if dark { 0x171b24 } else { 0xf6f7fb }))
            .text_color(rgb(if dark { 0xe8edf7 } else { 0x1e293b }))
            .text_sm()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, _| {
                match event.keystroke.key.as_str() {
                    "escape" => this.dismiss(window),
                    "q" if event.keystroke.modifiers.platform => {
                        this.quit();
                    }
                    "r" if event.keystroke.modifiers.platform => {
                        this.send(PanelToHost::Refresh { provider: None }, window)
                    }
                    _ => {}
                }
            }))
            .child(div().text_xl().child("Light Usage Bar"))
            .child(if self.snapshot.demo {
                "SYNTHETIC DEMO · No account data"
            } else {
                "Subscription quota · Missing values remain unknown"
            });
        for provider in &self.snapshot.providers {
            let id = provider.provider;
            let enabled = provider.enabled;
            let mut card = div()
                .id(id.name())
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(if dark { 0x394255 } else { 0xcdd5e3 }));
            for line in provider_lines(provider) {
                card = card.child(line);
            }
            card = card.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("Refresh").on_click(cx.listener(move |this, _, window, _| {
                            this.send(PanelToHost::Refresh { provider: Some(id) }, window)
                        })),
                    )
                    .child(button(if enabled { "Disable" } else { "Enable" }).on_click(
                        cx.listener(move |this, _, window, _| {
                            this.send(
                                PanelToHost::SetEnabled {
                                    provider: id,
                                    enabled: !enabled,
                                },
                                window,
                            )
                        }),
                    ))
                    .child(button("Sign-in help").on_click(cx.listener(
                        move |this, _, window, _| {
                            this.send(PanelToHost::AuthGuidance { provider: id }, window)
                        },
                    ))),
            );
            layout = layout.child(card);
        }
        layout.child(
            div()
                .flex()
                .gap_2()
                .child(
                    button("Refresh all · ⌘R").on_click(cx.listener(|this, _, window, _| {
                        this.send(PanelToHost::Refresh { provider: None }, window)
                    })),
                )
                .child(
                    button("Close · Esc")
                        .on_click(cx.listener(|this, _, window, _| this.dismiss(window))),
                )
                .child(button("Quit · ⌘Q").on_click(cx.listener(|this, _, _, _| {
                    this.quit();
                }))),
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
    (None, Bounds::centered(None, size(px(460.), px(620.)), cx))
}
pub fn run() {
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
                        empty_snapshot(false),
                        output_tx,
                        input_rx,
                        input_tx,
                        session,
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
