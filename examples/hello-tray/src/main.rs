//! `hello-tray` — R949 §3 §5.53 system-tray substrate.
//!
//! ## What this demonstrates
//!
//! A self-hosted editor needs a **system tray** (status item) for
//! long-running background work — asset baking, game builds, a running
//! server. pinion *owns* the tray model (id / icon / tooltip / status /
//! menu); a [`TrayBackend`] only exposes it to the OS. This example wires
//! the headless [`InMemoryTrayBackend`] (the `Storage` / `PrintBackend`
//! house pattern) so the whole tray is demonstrable + RPC-introspectable
//! without a panel.
//!
//! The context menu drives app state: *Show / Hide window*, a *Dark mode*
//! checkbox, a *Build ▸ Bake assets* submenu (with a disabled *Ship build*),
//! and *Quit*. Every activation flips the app's own state and **re-publishes**
//! the model — the menu state is app-owned, never backend-guessed.
//!
//! ## R1362 / R1363 §5.55 — the two dead menu items
//!
//! This example shipped **two menu items that did nothing**, both for the same
//! structural reason: a binding had no verb for what they meant.
//!
//! * ***Quit*** set a `quit_requested` signal nothing consumed. R1362 gave
//!   bindings a window-control seam; R1363 (§5.55) split **app lifecycle** out
//!   of it, because `Close`-on-primary secretly meant "kill the process". *Quit*
//!   now calls [`QuitSink::request_quit`], which is offered to
//!   [`pinion_core::WidgetCore::app_quit_requested`]
//!   — the same veto `Escape`, the last-window policy, and the `app/quit` RPC
//!   pass. This example casts no veto, which is what a Quit item means.
//! * ***Hide window*** flipped a signal that relabelled the menu and printed a
//!   word. It did not hide the window: `WindowControl` had no `Hide`, and
//!   `Minimize` was a one-way door with no `Restore`. Both now exist, and
//!   *Show / Hide* rides [`WindowControlSink`] — a WINDOW op, addressed to a
//!   window id, unlike *Quit*.
//!
//! The two seams side by side is the point: `quit_sink` takes no window id
//! because quitting addresses nothing, while `window_control` takes
//! [`DEFAULT_WINDOW`] because a window op must say which window. Their signals
//! stay as the AI-observable record of each request (`query quit_requested` /
//! `window_visible`); the sinks are the action.
//!
//! The seams' other consumer is off-thread (sprag's socket poll thread closing
//! the client when its daemon dies, PR-65); both handles are `Send + Sync` for
//! exactly that. This example exercises the UI-thread caller.
//!
//! The real `pinion_platform_tray::SniTrayBackend` (a `StatusNotifierItem` over
//! pure D-Bus, no gtk) is the follow-up *platform* round — exactly as
//! `FileStorage` (R665 → later) and `CupsPrintBackend` (R833) followed their
//! in-memory firsts. See `docs/cross-platform-native-strategy.md` step ①.
//!
//! ## §2 AI-first
//!
//! `query available` / `publish_count` / `published_in_sync` / `title` /
//! `icon` / `status` / `tooltip` / `menu` (a one-line summary) /
//! `item.<id>.{label,enabled,checked}` read the whole tray as scene-as-data.
//! `invoke menu_item "<id>"` is the RPC twin of clicking a menu entry (an
//! unknown / disabled id is rejected); `invoke activate` is an icon click;
//! `invoke republish` forces a publish. The drive path routes through the
//! backend's event queue + [`TrayBackend::poll_events`] — the same shape the
//! real shell uses to drain OS events.
//!
//! ## Verification
//!
//! `tools/demos/r949_tray.py` drives it over RPC: publish the model → the
//! backend records it; activate menu items → app state flips + the model
//! re-publishes; a disabled / unknown id is rejected. Deterministic.

use std::rc::Rc;
use std::sync::Arc;

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::query_proxy_external_impl;
use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::tray::{
    InMemoryTrayBackend, TrayBackend, TrayEvent, TrayMenuItem, TrayModel, TrayStatus,
};
use pinion_core::{Frame, Owner, QuitSink, Scene, Signal, use_quit_sink};
use pinion_derive::widget;
#[cfg(test)]
use pinion_shell::provide_window_control_sink;
use pinion_shell::{
    DEFAULT_WINDOW, WindowControl, WindowControlSink, use_window_control_sink, vello_renderer_impl,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTrayRenderer, HelloTrayRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 360;

const APP_ID: &str = "pinion.hello-tray";
const ROOT_TAG: &str = "tray_panel";

const TITLE_PX: u32 = 18;
const ROW_PX: u32 = 14;

// ─── Shared reactive app state (the menu drives these) ────────────

fn use_window_visible() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("Owner")
        .cache("tray.window_visible", || Signal::new(true))
}
fn use_dark_mode() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("Owner")
        .cache("tray.dark_mode", || Signal::new(false))
}
fn use_build_running() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("Owner")
        .cache("tray.build_running", || Signal::new(false))
}
fn use_quit_requested() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("Owner")
        .cache("tray.quit_requested", || Signal::new(false))
}
fn use_tray_backend() -> Rc<InMemoryTrayBackend> {
    // R949.1 — the backend's `TrayBackend` methods are `&self` (interior
    // mutability), so no outer `RefCell` is needed (the print/storage shape).
    Owner::current()
        .expect("Owner")
        .cache("tray.backend", InMemoryTrayBackend::new)
}

// ─── Model SSOT (built from app state; view + External share it) ──

/// Build the tray model from the current app state — the one place the
/// model is assembled, so the published model, the view, and the RPC reads
/// can never disagree about labels / checkmarks.
fn build_tray_model(visible: bool, dark: bool, building: bool) -> TrayModel {
    let menu = vec![
        TrayMenuItem::action(
            "toggle_window",
            if visible {
                "Hide window"
            } else {
                "Show window"
            },
        ),
        TrayMenuItem::check("dark", "Dark mode", dark),
        TrayMenuItem::Separator,
        TrayMenuItem::SubMenu {
            label: "Build".to_owned(),
            items: vec![
                TrayMenuItem::action(
                    "bake",
                    if building {
                        "Cancel bake"
                    } else {
                        "Bake assets"
                    },
                ),
                // Disabled until a bake completes — exercises the
                // can_activate guard (a disabled id is never dispatched).
                TrayMenuItem::Action {
                    id: "ship".to_owned(),
                    label: "Ship build".to_owned(),
                    enabled: false,
                },
            ],
        },
        TrayMenuItem::action("quit", "Quit"),
    ];
    let (icon, tip, status) = if building {
        (
            "emblem-synchronizing",
            "Pinion — baking assets…",
            TrayStatus::NeedsAttention,
        )
    } else {
        ("applications-games", "Pinion editor", TrayStatus::Active)
    };
    TrayModel::new(APP_ID, "Pinion", icon)
        .with_tooltip(tip)
        .with_status(status)
        .with_menu(menu)
}

/// Find a menu item by activation id (walks one submenu level) — the read
/// path for `item.<id>.*` introspection.
fn find_item<'a>(items: &'a [TrayMenuItem], id: &str) -> Option<&'a TrayMenuItem> {
    for it in items {
        if it.id() == Some(id) {
            return Some(it);
        }
        if let TrayMenuItem::SubMenu { items, .. } = it {
            if let Some(found) = find_item(items, id) {
                return Some(found);
            }
        }
    }
    None
}

/// The raw label of an `Action` / `Check` item (empty for a structural item).
fn menu_item_label(it: &TrayMenuItem) -> String {
    match it {
        TrayMenuItem::Action { label, .. } | TrayMenuItem::Check { label, .. } => label.clone(),
        TrayMenuItem::Separator | TrayMenuItem::SubMenu { .. } => String::new(),
    }
}

/// Whether an item is an enabled `Action` / `Check` (structural items: false).
fn menu_item_enabled(it: &TrayMenuItem) -> bool {
    matches!(
        it,
        TrayMenuItem::Action { enabled: true, .. } | TrayMenuItem::Check { enabled: true, .. }
    )
}

/// A one-line human/AI summary of the top-level menu (`label` + a `✓` for a
/// checked check, `▸` for a submenu, `—` for a separator).
fn menu_summary(model: &TrayModel) -> String {
    model
        .menu
        .iter()
        .map(|it| match it {
            TrayMenuItem::Action { label, enabled, .. } => {
                if *enabled {
                    label.clone()
                } else {
                    format!("{label} (disabled)")
                }
            }
            TrayMenuItem::Check { label, checked, .. } => {
                if *checked {
                    format!("{label} ✓")
                } else {
                    label.clone()
                }
            }
            TrayMenuItem::Separator => "—".to_owned(),
            TrayMenuItem::SubMenu { label, .. } => format!("{label} ▸"),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

// ─── External ─────────────────────────────────────────────────────

struct TrayExternal {
    visible: Rc<Signal<bool>>,
    dark: Rc<Signal<bool>>,
    building: Rc<Signal<bool>>,
    quit: Rc<Signal<bool>>,
    backend: Rc<InMemoryTrayBackend>,
    /// R1363 §5.55 — the APP-lifecycle seam that makes `Quit` quit.
    ///
    /// Pre-R1362 `Quit` set [`Self::quit`] and nothing consumed it: pinion
    /// shipped a tray Quit item that could not quit, because a binding had no
    /// way to request its own window's close. The signal stays as the
    /// AI-observable record of the request (`quit_requested`); this sink is the
    /// action, and it lands on the same `apply_window_control` arm the window's
    /// X button takes — so `window_close_requested` still gets its veto.
    window_control: Arc<dyn WindowControlSink>,
    /// R1363 §5.55 — the app-lifecycle seam. `Quit` is not a window op, so it
    /// does not ride `window_control`: it is offered to `app_quit_requested`,
    /// the veto a terminal binding shares (§2 #6).
    quit_sink: Arc<dyn QuitSink>,
}

impl TrayExternal {
    /// The live model from current app state.
    fn model(&self) -> TrayModel {
        build_tray_model(self.visible.get(), self.dark.get(), self.building.get())
    }

    /// Publish the live model to the backend (the control-plane update; the
    /// real bridge would push a few D-Bus round-trips here).
    fn publish(&self) {
        let model = self.model();
        // InMemory is always available; a real backend may report Unavailable
        // (no tray host) — the app degrades gracefully, never panics.
        let _ = self.backend.publish(&model);
    }

    /// Route one tray event into app state.
    fn route(&self, event: &TrayEvent) {
        match event {
            TrayEvent::Activated => self.visible.set(!self.visible.get()),
            TrayEvent::MenuItem(id) => match id.as_str() {
                "toggle_window" => {
                    // R1363 §5.55 — the OTHER dead menu item. Pre-R1363 this
                    // flipped a Signal that relabelled the menu and printed a
                    // word: "Hide window" did not hide the window, for want of a
                    // verb. `Show`/`Hide` are window ops, so they ride
                    // `WindowControlSink`; `Quit` is app lifecycle and rides
                    // `QuitSink`. The signal stays as the observable record.
                    let now_visible = !self.visible.get();
                    self.visible.set(now_visible);
                    self.window_control.request_window_control(
                        DEFAULT_WINDOW,
                        if now_visible {
                            WindowControl::Show
                        } else {
                            WindowControl::Hide
                        },
                    );
                }
                "dark" => self.dark.set(!self.dark.get()),
                "bake" => self.building.set(!self.building.get()),
                "quit" => {
                    // The observable record of the request...
                    self.quit.set(true);
                    // ...and the request itself. R1363 §5.55 — Quit is an APP
                    // act, not a window op: it rides `QuitSink` and is offered
                    // to `app_quit_requested`, NOT to `window_close_requested`.
                    // Pre-R1363 this had to ask for `WindowControl::Close` and
                    // rely on the close falling through to an app exit — the
                    // conflation this round split. Queued, not immediate, so
                    // this `invoke` still returns its RPC result before the
                    // process ends. `hello-tray` casts no veto, which is what a
                    // Quit item means.
                    self.quit_sink.request_quit();
                }
                _ => {}
            },
        }
    }

    /// Drain the backend's event queue, route each event, and re-publish if
    /// anything changed — the per-frame shape the real shell uses to feed OS
    /// tray events back into the app.
    fn pump(&self) {
        let events = self.backend.poll_events();
        if events.is_empty() {
            return;
        }
        for ev in &events {
            self.route(ev);
        }
        self.publish();
    }

    /// Deliver a tray event through the backend queue (the headless stand-in
    /// for the OS) and pump it — the RPC twin of a real click.
    fn deliver(&self, event: TrayEvent) {
        self.backend.push_event(event);
        self.pump();
    }
}

impl core::fmt::Debug for TrayExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TrayExternal")
            .field("title", &self.model().title)
            .field("publish_count", &self.backend.publish_count())
            .finish_non_exhaustive()
    }
}

query_proxy_external_impl!(TrayExternal);

impl ExternalIntrospect for TrayExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("available", "bool"),
                    SchemaField::new("publish_count", "int"),
                    SchemaField::new("published_in_sync", "bool"),
                    SchemaField::new("title", "string"),
                    SchemaField::new("icon", "string"),
                    SchemaField::new("status", "string"),
                    SchemaField::new("tooltip", "string"),
                    SchemaField::new("menu", "string"),
                    SchemaField::new("menu_count", "int"),
                    SchemaField::new("window_visible", "bool"),
                    SchemaField::new("dark_mode", "bool"),
                    SchemaField::new("build_running", "bool"),
                    SchemaField::new("quit_requested", "bool"),
                    // (R1353.1) Keyed by a tray-item ID, not an index: `menu_count`
                    // bounds the menu list, not these ids, and no path LISTS them —
                    // so neither `IndexOf` nor `ValuesOf` would be true. Declared
                    // unknown rather than pointed at a bound it does not have.
                    SchemaField::parametric(
                        "item.<id>.label",
                        "string",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                    SchemaField::parametric(
                        "item.<id>.enabled",
                        "bool",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                    SchemaField::parametric(
                        "item.<id>.checked",
                        "bool",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                    SchemaField::parametric(
                        "item.<id>.activatable",
                        "bool",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                    SchemaField::new("activate", "json"),
                    SchemaField::new("menu_item", "string"),
                    SchemaField::new("republish", "json"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let model = self.model();
        match path {
            "available" => Some(IntrospectValue::Bool(self.backend.is_available())),
            "publish_count" => Some(IntrospectValue::Int(i64::from(
                self.backend.publish_count(),
            ))),
            // Whether the last published model matches the live model — the
            // AI-first "is the OS showing what the app thinks it is" read.
            "published_in_sync" => Some(IntrospectValue::Bool(
                self.backend.published().as_ref() == Some(&model),
            )),
            "title" => Some(IntrospectValue::Text(model.title)),
            "icon" => Some(IntrospectValue::Text(model.icon_name)),
            "status" => Some(IntrospectValue::Text(model.status.as_str().to_owned())),
            "tooltip" => Some(IntrospectValue::Text(model.tooltip)),
            "menu" => Some(IntrospectValue::Text(menu_summary(&model))),
            "menu_count" => Some(IntrospectValue::Int(i64::try_from(model.menu.len()).ok()?)),
            "window_visible" => Some(IntrospectValue::Bool(self.visible.get())),
            "dark_mode" => Some(IntrospectValue::Bool(self.dark.get())),
            "build_running" => Some(IntrospectValue::Bool(self.building.get())),
            "quit_requested" => Some(IntrospectValue::Bool(self.quit.get())),
            _ => {
                // `item.<id>.<field>` — id-addressed menu introspection.
                let rest = path.strip_prefix("item.")?;
                let (id, field) = rest.rsplit_once('.')?;
                // `activatable` answers for any id (including unknown / disabled).
                if field == "activatable" {
                    return Some(IntrospectValue::Bool(model.can_activate(id)));
                }
                // The other fields require the id to name a real item.
                let item = find_item(&model.menu, id)?;
                Some(match field {
                    "label" => IntrospectValue::Text(menu_item_label(item)),
                    "checked" => IntrospectValue::Bool(matches!(
                        item,
                        TrayMenuItem::Check { checked: true, .. }
                    )),
                    "enabled" => IntrospectValue::Bool(menu_item_enabled(item)),
                    _ => return None,
                })
            }
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // The whole tray is driven by activations (`invoke`), not direct state
        // writes — the menu is the contract. No intervene axes.
        Err(InterveneError::ReadOnly)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // An icon (primary) click.
            "activate" => {
                self.deliver(TrayEvent::Activated);
                Ok(IntrospectValue::Bool(true))
            }
            // A context-menu activation by id (the RPC twin of a click). An
            // unknown / disabled id is rejected (returns `false`, no dispatch).
            "menu_item" => match args {
                IntrospectValue::Text(id) => {
                    if !self.model().can_activate(&id) {
                        return Ok(IntrospectValue::Bool(false));
                    }
                    self.deliver(TrayEvent::MenuItem(id));
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Force a publish (returns the new publish count).
            "republish" => {
                self.publish();
                Ok(IntrospectValue::Int(i64::from(
                    self.backend.publish_count(),
                )))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn make_tray_external() -> TrayExternal {
    TrayExternal {
        visible: use_window_visible(),
        dark: use_dark_mode(),
        building: use_build_running(),
        quit: use_quit_requested(),
        backend: use_tray_backend(),
        quit_sink: use_quit_sink(),
        // R1362 PR-65 — resolved at wiring time inside `create_external` (which
        // the shell runs in the root `Owner`, after it seeded the live sink and
        // before any frame). Off the live event loop — an `Owner::new().run(..)`
        // unit test — this resolves the `NullWindowControlSink`, so a reducer
        // test can activate `quit` without exiting the test process; a test that
        // wants to OBSERVE the request seeds the owner first
        // (`provide_window_control_sink`) and reaches this same line.
        window_control: use_window_control_sink(),
    }
}

// ─── View (paints the own-rendered tray model as a panel) ─────────

fn label_row(text: String, fg: Color, px: u32) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new().with_size_px(px).with_fg(fg),
        ))])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(0, 2, 0, 2)),
        ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_dark: bool, _frame: &Frame) -> Scene {
    let dark = use_dark_mode().get();
    let visible = use_window_visible().get();
    let building = use_build_running().get();
    let model = build_tray_model(visible, dark, building);

    // Light / dark panel theming driven by the tray's own Dark-mode toggle.
    let (bg, fg, muted, accent) = if dark {
        (
            Color::rgb(0x1e, 0x1e, 0x24),
            Color::rgb(0xe8, 0xe8, 0xee),
            Color::rgb(0x9a, 0x9a, 0xa6),
            Color::rgb(0x6c, 0xb6, 0xff),
        )
    } else {
        (
            Color::rgb(0xf6, 0xf6, 0xfa),
            Color::rgb(0x1c, 0x1c, 0x22),
            Color::rgb(0x66, 0x66, 0x70),
            Color::rgb(0x16, 0x6c, 0xff),
        )
    };

    let mut rows: Vec<Scene> = Vec::new();
    rows.push(label_row(
        format!("System Tray — {}", model.title),
        fg,
        TITLE_PX,
    ));
    rows.push(label_row(
        format!(
            "icon: {}   status: {}",
            model.icon_name,
            model.status.as_str()
        ),
        muted,
        ROW_PX,
    ));
    rows.push(label_row(model.tooltip.clone(), muted, ROW_PX));
    rows.push(label_row("Menu:".to_owned(), accent, ROW_PX));
    for it in &model.menu {
        let (text, color) = match it {
            TrayMenuItem::Action { label, enabled, .. } => {
                (format!("  {label}"), if *enabled { fg } else { muted })
            }
            TrayMenuItem::Check { label, checked, .. } => (
                format!("  [{}] {label}", if *checked { "x" } else { " " }),
                fg,
            ),
            TrayMenuItem::Separator => ("  ────────".to_owned(), muted),
            TrayMenuItem::SubMenu { label, items } => {
                (format!("  {label} ▸ ({} items)", items.len()), fg)
            }
        };
        rows.push(label_row(text, color, ROW_PX));
    }
    rows.push(label_row(
        format!(
            "window: {}   dark: {}   building: {}",
            if visible { "shown" } else { "hidden" },
            if dark { "on" } else { "off" },
            if building { "yes" } else { "no" },
        ),
        muted,
        ROW_PX,
    ));

    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(ROOT_TAG)
            .with_style(BoxStyle::filled(bg))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_gap(2)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_padding(Rect::new(16, 16, 16, 16)),
            ),
    )
}

// ─── Binding ──────────────────────────────────────────────────────

#[widget(
    tag = "tray_panel",
    state = bool,
    event = (),
    title = "pinion hello-tray (R949 §5.53 system-tray substrate)",
    renderer = HelloTrayRenderer,
    initial_size = (WIN_W, WIN_H),
    external = make_tray_external,
    role = Group,
)]
struct TrayView;

impl TrayView {
    fn read_state(scene: &Scene) -> bool {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Bool(b)) = intro.query("dark_mode") {
                    return b;
                }
            }
        }
        false
    }

    fn view(state: bool, frame: Frame) -> Scene {
        view(state, &frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }
}

fn main() {
    pinion_shell::run::<TrayView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn ext() -> TrayExternal {
        Owner::new().run(make_tray_external)
    }

    /// R1362 PR-65 — records what the binding asked the shell to do, standing in
    /// for the live `ProxyWindowControlSink`.
    #[derive(Debug, Default)]
    struct RecordingSink(Mutex<Vec<(String, WindowControl)>>);

    impl RecordingSink {
        /// Drain the recorded requests (so a test can assert "nothing since
        /// last time" without index arithmetic).
        fn taken(&self) -> Vec<(String, WindowControl)> {
            std::mem::take(&mut *self.0.lock().expect("recording sink poisoned"))
        }
    }

    /// R1363 §5.55 — records app-quit requests, the peer of [`RecordingSink`].
    #[derive(Debug, Default)]
    struct RecordingQuitSink(Mutex<usize>);

    impl RecordingQuitSink {
        fn count(&self) -> usize {
            *self.0.lock().expect("recording quit sink poisoned")
        }
    }

    impl QuitSink for RecordingQuitSink {
        fn request_quit(&self) {
            *self.0.lock().expect("recording quit sink poisoned") += 1;
        }
    }

    impl WindowControlSink for RecordingSink {
        fn request_window_control(&self, window_id: &str, control: WindowControl) {
            self.0
                .lock()
                .expect("recording sink poisoned")
                .push((window_id.to_owned(), control));
        }
    }

    #[test]
    fn r949_boot_model_is_active_with_full_menu() {
        let e = ext();
        assert_eq!(e.query("available"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            e.query("title"),
            Some(IntrospectValue::Text("Pinion".to_owned()))
        );
        assert_eq!(
            e.query("status"),
            Some(IntrospectValue::Text("Active".to_owned()))
        );
        assert_eq!(
            e.query("icon"),
            Some(IntrospectValue::Text("applications-games".to_owned()))
        );
        assert_eq!(e.query("menu_count"), Some(IntrospectValue::Int(5)));
        // Nothing published until an explicit publish / activation.
        assert_eq!(e.query("publish_count"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("published_in_sync"),
            Some(IntrospectValue::Bool(false))
        );
    }

    #[test]
    fn r949_republish_records_the_live_model() {
        let mut e = ext();
        assert_eq!(
            e.invoke("republish", IntrospectValue::Null),
            Ok(IntrospectValue::Int(1))
        );
        assert_eq!(
            e.query("published_in_sync"),
            Some(IntrospectValue::Bool(true))
        );
        assert_eq!(
            e.invoke("republish", IntrospectValue::Null),
            Ok(IntrospectValue::Int(2))
        );
    }

    #[test]
    fn r949_dark_toggle_flips_check_and_republishes() {
        let mut e = ext();
        assert_eq!(e.query("dark_mode"), Some(IntrospectValue::Bool(false)));
        assert_eq!(
            e.query("item.dark.checked"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            e.invoke("menu_item", IntrospectValue::Text("dark".to_owned())),
            Ok(IntrospectValue::Bool(true))
        );
        assert_eq!(
            e.query("dark_mode"),
            Some(IntrospectValue::Bool(true)),
            "menu flipped app state"
        );
        assert_eq!(
            e.query("item.dark.checked"),
            Some(IntrospectValue::Bool(true)),
            "the check reflects it"
        );
        assert_eq!(
            e.query("publish_count"),
            Some(IntrospectValue::Int(1)),
            "the activation re-published"
        );
        assert_eq!(
            e.query("published_in_sync"),
            Some(IntrospectValue::Bool(true))
        );
    }

    #[test]
    fn r949_toggle_window_relabels_and_activate_is_icon_click() {
        let mut e = ext();
        assert_eq!(e.query("window_visible"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            e.query("item.toggle_window.label"),
            Some(IntrospectValue::Text("Hide window".to_owned()))
        );
        e.invoke(
            "menu_item",
            IntrospectValue::Text("toggle_window".to_owned()),
        )
        .unwrap();
        assert_eq!(
            e.query("window_visible"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            e.query("item.toggle_window.label"),
            Some(IntrospectValue::Text("Show window".to_owned())),
            "label tracks state"
        );
        // The icon (primary) click toggles the window too.
        e.invoke("activate", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.query("window_visible"),
            Some(IntrospectValue::Bool(true)),
            "activate is an icon click"
        );
    }

    #[test]
    fn r949_bake_sets_needs_attention_status_and_icon() {
        let mut e = ext();
        e.invoke("menu_item", IntrospectValue::Text("bake".to_owned()))
            .unwrap();
        assert_eq!(e.query("build_running"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            e.query("status"),
            Some(IntrospectValue::Text("NeedsAttention".to_owned()))
        );
        assert_eq!(
            e.query("icon"),
            Some(IntrospectValue::Text("emblem-synchronizing".to_owned()))
        );
        assert_eq!(
            e.query("item.bake.label"),
            Some(IntrospectValue::Text("Cancel bake".to_owned()))
        );
    }

    #[test]
    fn r949_disabled_and_unknown_ids_are_rejected() {
        let mut e = ext();
        // `ship` is present but disabled.
        assert_eq!(
            e.query("item.ship.activatable"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            e.invoke("menu_item", IntrospectValue::Text("ship".to_owned())),
            Ok(IntrospectValue::Bool(false))
        );
        // An unknown id.
        assert_eq!(
            e.invoke("menu_item", IntrospectValue::Text("nope".to_owned())),
            Ok(IntrospectValue::Bool(false))
        );
        assert_eq!(
            e.query("publish_count"),
            Some(IntrospectValue::Int(0)),
            "a rejected activation publishes nothing"
        );
    }

    #[test]
    fn r949_quit_sets_quit_requested() {
        let mut e = ext();
        assert_eq!(
            e.query("quit_requested"),
            Some(IntrospectValue::Bool(false))
        );
        e.invoke("menu_item", IntrospectValue::Text("quit".to_owned()))
            .unwrap();
        assert_eq!(e.query("quit_requested"), Some(IntrospectValue::Bool(true)));
    }

    /// R1363 §5.55 — `Quit` asks the APP to end; it is not a window op.
    ///
    /// The discriminator is the whole round: a `Quit` must NOT reach the
    /// window-control sink. Pre-R1363 it had to — `WindowControl::Close` was the
    /// only verb that could end the app, which is exactly the conflation §5.55
    /// splits. If someone re-welds them, this test fails on the second assert.
    #[test]
    fn r1363_quit_requests_an_app_quit_not_a_window_close() {
        let quit = Arc::new(RecordingQuitSink::default());
        let wc = Arc::new(RecordingSink::default());
        let owner = Owner::new();
        owner.provide_quit_sink(quit.clone());
        provide_window_control_sink(&owner, wc.clone());
        let mut e = owner.run(make_tray_external);

        e.invoke("menu_item", IntrospectValue::Text("quit".to_owned()))
            .unwrap();
        assert_eq!(quit.count(), 1, "Quit asks the APP to end");
        assert!(
            wc.taken().is_empty(),
            "Quit must NOT ride the window-control sink — app lifecycle is not a \
             window operation (§5.55)",
        );
    }

    /// R1363 §5.55 — the OTHER dead menu item: `Hide window` now hides the
    /// window, and it IS a window op (addressed to a window id, unlike Quit).
    #[test]
    fn r1363_toggle_window_requests_hide_then_show_through_the_window_sink() {
        let quit = Arc::new(RecordingQuitSink::default());
        let wc = Arc::new(RecordingSink::default());
        let owner = Owner::new();
        owner.provide_quit_sink(quit.clone());
        provide_window_control_sink(&owner, wc.clone());
        let mut e = owner.run(make_tray_external);

        // Boot state is visible → the item reads "Hide window" → Hide.
        e.invoke(
            "menu_item",
            IntrospectValue::Text("toggle_window".to_owned()),
        )
        .unwrap();
        assert_eq!(
            wc.taken(),
            vec![(DEFAULT_WINDOW.to_owned(), WindowControl::Hide)],
            "Hide window must actually hide the window, not just relabel a menu",
        );
        // ...and back.
        e.invoke(
            "menu_item",
            IntrospectValue::Text("toggle_window".to_owned()),
        )
        .unwrap();
        assert_eq!(
            wc.taken(),
            vec![(DEFAULT_WINDOW.to_owned(), WindowControl::Show)],
        );
        assert_eq!(quit.count(), 0, "a window op must never ask the app to end");
    }

    /// A non-lifecycle menu item touches neither seam — the negative control
    /// that keeps both tests above honest.
    #[test]
    fn r1363_an_ordinary_menu_item_touches_neither_lifecycle_seam() {
        let quit = Arc::new(RecordingQuitSink::default());
        let wc = Arc::new(RecordingSink::default());
        let owner = Owner::new();
        owner.provide_quit_sink(quit.clone());
        provide_window_control_sink(&owner, wc.clone());
        let mut e = owner.run(make_tray_external);
        e.invoke("menu_item", IntrospectValue::Text("dark".to_owned()))
            .unwrap();
        assert!(wc.taken().is_empty());
        assert_eq!(quit.count(), 0);
    }

    #[test]
    fn r949_menu_summary_reflects_state() {
        let mut e = ext();
        assert_eq!(
            e.query("menu"),
            Some(IntrospectValue::Text(
                "Hide window | Dark mode | — | Build ▸ | Quit".to_owned()
            )),
        );
        e.invoke("menu_item", IntrospectValue::Text("dark".to_owned()))
            .unwrap();
        assert_eq!(
            e.query("menu"),
            Some(IntrospectValue::Text(
                "Hide window | Dark mode ✓ | — | Build ▸ | Quit".to_owned()
            )),
            "the summary shows the dark check",
        );
    }

    fn has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| has_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn view_renders_title_and_menu() {
        let owner = Owner::new();
        let scene = owner.run(|| view(false, &Frame::new()));
        assert!(has_text(&scene, "System Tray"), "header rendered");
        assert!(has_text(&scene, "Hide window"), "menu action rendered");
        assert!(
            has_text(&scene, "[ ] Dark mode"),
            "unchecked check rendered"
        );
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TrayView>(
            false,
            &Frame::new(),
        );
    }

    #[test]
    fn tray_root_reports_group_role() {
        let nodes = <TrayView as WidgetA11y>::access_node(&false, None);
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].tag, ROOT_TAG);
    }
}
