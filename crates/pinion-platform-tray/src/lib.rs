//! R950 §3 §5.53 §5.15 — Linux system-tray bridge: the real platform impl of
//! the [`pinion_core::tray::TrayBackend`] trait.
//!
//! [`SniTrayBackend`] publishes the pinion-owned
//! [`TrayModel`](pinion_core::tray::TrayModel) as a **`StatusNotifierItem`**
//! over pure D-Bus via the `ksni` crate (zbus-based, **no gtk**), so it
//! composes with pinion's winit shell where the Tauri `tray-icon` crate's gtk
//! backend does not (`docs/cross-platform-native-strategy.md`). It is the
//! second consumer that validates the R949 `TrayBackend` trait surface — the
//! `FileStorage` / `CupsPrintBackend`-equivalent platform impl; the in-memory
//! [`InMemoryTrayBackend`](pinion_core::tray::InMemoryTrayBackend) stays the
//! headless / CI default and the fallback when no tray host is present.
//!
//! The SNI service runs on `ksni`'s own background thread (its blocking API
//! spawns + drives a private executor); [`SniTrayBackend`] holds the sync
//! [`Handle`](ksni::blocking::Handle) to update the model and an
//! `Arc<Mutex<_>>` event sink the menu / icon callbacks push into, which
//! [`TrayBackend::poll_events`] drains — the "backend → channel → UI" shape
//! Win/macOS native trays also use.

use std::sync::{Arc, Mutex};

use ksni::blocking::TrayMethods;
use pinion_core::tray::{TrayBackend, TrayError, TrayEvent, TrayMenuItem, TrayModel, TrayStatus};

/// The well-known bus name a `StatusNotifierWatcher` (the panel that hosts
/// tray items) owns; [`SniTrayBackend::is_available`] checks for its owner.
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

/// Events the `ksni` callbacks push and [`TrayBackend::poll_events`] drains.
/// Shared (Arc) between the background SNI service's tray state and the
/// front [`SniTrayBackend`].
type EventSink = Arc<Mutex<Vec<TrayEvent>>>;

/// The `ksni::Tray` state living on the SNI service thread: the current model
/// (re-queried by `ksni` whenever it needs a property / the menu) plus the
/// shared event sink.
struct SniState {
    model: TrayModel,
    events: EventSink,
}

impl SniState {
    /// Push an event into the shared sink (a callback is `Fn`, so it never
    /// holds the lock across the push).
    fn emit(&self, event: TrayEvent) {
        if let Ok(mut q) = self.events.lock() {
            q.push(event);
        }
    }
}

fn map_status(status: TrayStatus) -> ksni::Status {
    match status {
        TrayStatus::Active => ksni::Status::Active,
        TrayStatus::Passive => ksni::Status::Passive,
        TrayStatus::NeedsAttention => ksni::Status::NeedsAttention,
    }
}

/// Lower one pinion menu item to its `ksni` counterpart. An `Action` / `Check`
/// activation pushes [`TrayEvent::MenuItem`] with the item's id into the sink
/// (the app routes it + re-publishes — the menu state is app-owned).
fn lower_item(item: &TrayMenuItem) -> ksni::MenuItem<SniState> {
    match item {
        TrayMenuItem::Action { id, label, enabled } => {
            let id = id.clone();
            ksni::menu::StandardItem {
                label: label.clone(),
                enabled: *enabled,
                activate: Box::new(move |s: &mut SniState| s.emit(TrayEvent::MenuItem(id.clone()))),
                ..Default::default()
            }
            .into()
        }
        TrayMenuItem::Check { id, label, checked, enabled } => {
            let id = id.clone();
            ksni::menu::CheckmarkItem {
                label: label.clone(),
                checked: *checked,
                enabled: *enabled,
                activate: Box::new(move |s: &mut SniState| s.emit(TrayEvent::MenuItem(id.clone()))),
                ..Default::default()
            }
            .into()
        }
        TrayMenuItem::Separator => ksni::menu::MenuItem::Separator,
        TrayMenuItem::SubMenu { label, items } => ksni::menu::SubMenu {
            label: label.clone(),
            submenu: items.iter().map(lower_item).collect(),
            ..Default::default()
        }
        .into(),
    }
}

impl ksni::Tray for SniState {
    fn id(&self) -> String {
        self.model.id.clone()
    }

    fn title(&self) -> String {
        self.model.title.clone()
    }

    fn icon_name(&self) -> String {
        self.model.icon_name.clone()
    }

    fn status(&self) -> ksni::Status {
        map_status(self.model.status)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: self.model.tooltip.clone(), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit(TrayEvent::Activated);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        self.model.menu.iter().map(lower_item).collect()
    }
}

/// R950 — the real Linux [`TrayBackend`]: publishes the pinion `TrayModel` as a
/// `StatusNotifierItem` over D-Bus (no gtk) and drains its activations.
pub struct SniTrayBackend {
    handle: ksni::blocking::Handle<SniState>,
    events: EventSink,
}

impl core::fmt::Debug for SniTrayBackend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SniTrayBackend").finish_non_exhaustive()
    }
}

impl SniTrayBackend {
    /// Register a `StatusNotifierItem` for `initial` and start its background
    /// SNI service. The item is owned on the session bus immediately; a host
    /// (panel) displays it if/when one is present (see [`Self::is_available`]).
    ///
    /// # Errors
    ///
    /// [`TrayError::Backend`] if the SNI service cannot start (e.g. no D-Bus
    /// session bus is reachable).
    pub fn new(initial: &TrayModel) -> Result<Self, TrayError> {
        let events: EventSink = Arc::new(Mutex::new(Vec::new()));
        let state = SniState { model: initial.clone(), events: Arc::clone(&events) };
        let handle = state.spawn().map_err(|e| TrayError::Backend(e.to_string()))?;
        Ok(Self { handle, events })
    }
}

impl TrayBackend for SniTrayBackend {
    fn is_available(&self) -> bool {
        // A blocking `NameHasOwner` for the StatusNotifierWatcher — present
        // means a panel will host the item. Any bus / name error degrades to
        // "absent" (capability-honest, never a panic).
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return false;
        };
        let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
            return false;
        };
        let Ok(name) = zbus::names::BusName::try_from(WATCHER_NAME) else {
            return false;
        };
        proxy.name_has_owner(name).unwrap_or(false)
    }

    fn publish(&self, model: &TrayModel) -> Result<(), TrayError> {
        let next = model.clone();
        // `update` returns `None` once the service thread has shut down; the
        // SNI item itself is always registered while the service is alive, so
        // a live publish never reports `Unavailable` (that is the watcher's
        // business, read via `is_available`).
        match self.handle.update(move |state: &mut SniState| state.model = next.clone()) {
            Some(()) => Ok(()),
            None => Err(TrayError::Backend("tray service has shut down".to_owned())),
        }
    }

    fn poll_events(&self) -> Vec<TrayEvent> {
        self.events.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }
}
