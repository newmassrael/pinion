//! R949 §3 §5.53 §5.15 — system-tray substrate: the own-rendered tray
//! model + the platform backend contract.
//!
//! A self-hosted editor needs a system tray (status item) for long-running
//! background work — asset baking, game builds, a running server. pinion
//! **owns** the tray model (id / icon / tooltip / status / menu); a
//! [`TrayBackend`] only *exposes* it to the OS. Mirrors the R665
//! [`Storage`](crate::storage) and R833 [`PrintBackend`](crate::print)
//! patterns exactly:
//!
//! - [`TrayBackend`] — the thin contract (publish the model, drain events,
//!   report whether a tray host is present).
//! - [`InMemoryTrayBackend`] — pure-Rust, records the published model and a
//!   scripted event queue; the canonical test / headless fixture and the
//!   fallback a binding uses when no real tray host exists.
//! - `pinion_platform_tray::SniTrayBackend` — the real Linux bridge (a
//!   `StatusNotifierItem` over pure D-Bus, **no gtk**), the `FileStorage` /
//!   CupsPrintBackend-equivalent (a documented follow-up round; see
//!   `docs/cross-platform-native-strategy.md` staged-path step ①).
//!
//! The model is **scene-as-data** (§2 #7): the icon is a *themed icon name*
//! (freedesktop), never pixels, so the whole tray is introspectable as text
//! over the §5.12 RPC plane — an AI agent reads `tray.title` / the menu and
//! drives a menu activation without a panel.
//!
//! Capability honesty (§5.53): a backend that cannot host a tray reports it
//! ([`TrayBackend::is_available`]) and a publish returns
//! [`TrayError::Unavailable`] — it never silently no-ops (the
//! `divergence-is-a-bug` failure the cross-platform strategy guards against).

use std::cell::RefCell;

/// The tray item's status — a mirror of the `StatusNotifierItem` `Status`
/// enum, so the real D-Bus bridge maps it 1:1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TrayStatus {
    /// The normal, visible state (the default).
    #[default]
    Active,
    /// The host may hide the item (nothing needs the user right now).
    Passive,
    /// The item demands attention (the host may highlight / animate it).
    NeedsAttention,
}

impl TrayStatus {
    /// Stable token — the `StatusNotifierItem` `Status` string + the
    /// introspect wire form. The one home of the vocabulary so the D-Bus
    /// bridge and the RPC read cannot drift ([[wire-form-read-write-symmetry]]).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Passive => "Passive",
            Self::NeedsAttention => "NeedsAttention",
        }
    }

    /// Parse the canonical token — the read inverse of [`Self::as_str`].
    /// `None` for any other string.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "Active" => Some(Self::Active),
            "Passive" => Some(Self::Passive),
            "NeedsAttention" => Some(Self::NeedsAttention),
            _ => None,
        }
    }
}

/// One entry in the tray's context menu (the model pinion owns; the backend
/// exports it, e.g. as a `com.canonical.dbusmenu` layout on Linux).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// A clickable command. Activating it emits [`TrayEvent::MenuItem`] with
    /// `id`. A disabled item is shown greyed and emits nothing.
    Action {
        /// Stable activation id (the `TrayEvent::MenuItem` payload).
        id: String,
        /// The label rendered in the menu.
        label: String,
        /// Whether the item can be activated.
        enabled: bool,
    },
    /// A toggle (checkbox) command. Activating it emits
    /// [`TrayEvent::MenuItem`] with `id`; the app flips `checked` and
    /// re-publishes (the menu state is app-owned, never backend-guessed).
    Check {
        /// Stable activation id.
        id: String,
        /// The label rendered in the menu.
        label: String,
        /// The current checkmark state.
        checked: bool,
        /// Whether the item can be activated.
        enabled: bool,
    },
    /// A horizontal separator (no id, never activates).
    Separator,
    /// A nested submenu.
    SubMenu {
        /// The submenu's label.
        label: String,
        /// The submenu's entries.
        items: Vec<TrayMenuItem>,
    },
}

impl TrayMenuItem {
    /// An enabled [`TrayMenuItem::Action`].
    #[must_use]
    pub fn action(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Action {
            id: id.into(),
            label: label.into(),
            enabled: true,
        }
    }

    /// An enabled [`TrayMenuItem::Check`] with the given state.
    #[must_use]
    pub fn check(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self::Check {
            id: id.into(),
            label: label.into(),
            checked,
            enabled: true,
        }
    }

    /// The activation id, if this item has one (an `Action` / `Check`).
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Action { id, .. } | Self::Check { id, .. } => Some(id),
            Self::Separator | Self::SubMenu { .. } => None,
        }
    }

    /// Whether this item (or, for a submenu, any descendant) can be
    /// activated with `id`. The validation an event-router uses so an
    /// activation for an unknown / disabled id is rejected, not silently
    /// dispatched.
    #[must_use]
    pub fn can_activate(&self, id: &str) -> bool {
        match self {
            Self::Action { id: i, enabled, .. } | Self::Check { id: i, enabled, .. } => {
                *enabled && i == id
            }
            Self::SubMenu { items, .. } => items.iter().any(|it| it.can_activate(id)),
            Self::Separator => false,
        }
    }
}

/// The whole tray model pinion owns. The icon is a *themed name*
/// (freedesktop icon-naming), not pixels — so the model is pure scene-as-data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayModel {
    /// Application id (the `StatusNotifierItem` `Id`; stable per app).
    pub id: String,
    /// Human-readable title (the SNI `Title`, shown on hover by some hosts).
    pub title: String,
    /// Themed icon name (freedesktop, e.g. `"applications-games"`); the SNI
    /// `IconName`. Never a pixel buffer — keeps the model introspectable.
    pub icon_name: String,
    /// Tooltip text shown on hover.
    pub tooltip: String,
    /// The item's status.
    pub status: TrayStatus,
    /// The context menu (empty for an icon-only item).
    pub menu: Vec<TrayMenuItem>,
}

impl TrayModel {
    /// A minimal `Active` model: id + title + themed icon, no tooltip / menu.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        icon_name: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon_name: icon_name.into(),
            tooltip: String::new(),
            status: TrayStatus::Active,
            menu: Vec::new(),
        }
    }

    /// Builder: set the tooltip.
    #[must_use]
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }

    /// Builder: set the status.
    #[must_use]
    pub const fn with_status(mut self, status: TrayStatus) -> Self {
        self.status = status;
        self
    }

    /// Builder: set the context menu.
    #[must_use]
    pub fn with_menu(mut self, menu: Vec<TrayMenuItem>) -> Self {
        self.menu = menu;
        self
    }

    /// Whether `id` names an enabled, activatable menu item anywhere in the
    /// menu tree — the guard a [`TrayEvent::MenuItem`] router applies before
    /// dispatching (an unknown / disabled id is not a valid activation).
    #[must_use]
    pub fn can_activate(&self, id: &str) -> bool {
        self.menu.iter().any(|it| it.can_activate(id))
    }
}

/// An event from the tray back to the app, drained via
/// [`TrayBackend::poll_events`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// The icon was activated (a primary click).
    Activated,
    /// A context-menu item with this activation id was chosen.
    MenuItem(String),
}

/// Why a [`TrayBackend::publish`] failed. Total surface — nothing panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayError {
    /// No tray host is present (e.g. no `StatusNotifierWatcher`); the model
    /// cannot be shown. The caller degrades gracefully (the editor keeps
    /// running without a tray) — the capability-honest absent report.
    Unavailable,
    /// The platform bridge failed; carries its diagnostic.
    Backend(String),
}

/// R949 §3 §5.53 — the system-tray backend contract: publish the
/// pinion-owned [`TrayModel`] to the OS and surface the events it generates.
/// Mirrors [`PrintBackend`](crate::print::PrintBackend) (a small total trait
/// with an in-memory + a platform impl).
///
/// Every method takes `&self` (not `&mut self`) — mirroring
/// [`Storage`](crate::storage::Storage) / [`PrintBackend`](crate::print::PrintBackend)
/// — so a runtime-selected `Box<dyn TrayBackend>` (a binding's "SNI if a host
/// is present, else in-memory" choice, the `AppStorage` shape) stays
/// object-safe and shareable; a backend that needs to mutate (the in-memory
/// fixture, the real SNI handle's event queue) uses interior mutability.
/// `Debug` is a super-trait so that `Box<dyn TrayBackend>` can derive `Debug`.
pub trait TrayBackend: core::fmt::Debug {
    /// Whether a tray host is actually present (a `StatusNotifierWatcher` on
    /// Linux). A capability flag — the binding queries it to decide whether
    /// to bother publishing (never a silent no-op).
    fn is_available(&self) -> bool;

    /// Publish (or update) the whole model. Idempotent — the caller invokes
    /// it on a model *change*, never at frame rate (the control-plane
    /// discipline; the real D-Bus bridge does a few round-trips per call).
    ///
    /// # Errors
    ///
    /// - [`TrayError::Unavailable`] when no tray host is present.
    /// - [`TrayError::Backend`] when the platform bridge fails.
    fn publish(&self, model: &TrayModel) -> Result<(), TrayError>;

    /// Drain the events accumulated since the last poll (icon activations,
    /// menu-item choices). The shell pumps this each frame and routes the
    /// events into the app the same way it drains the JSON-RPC inbox.
    fn poll_events(&self) -> Vec<TrayEvent>;
}

/// R949 §3 §5.53 — pure-Rust in-memory [`TrayBackend`]: records the most
/// recently published [`TrayModel`] and serves a scripted event queue. The
/// canonical test / headless fixture (the `SniTrayBackend` equivalent of
/// [`InMemoryStorage`](crate::storage::InMemoryStorage) /
/// [`InMemoryPrintBackend`](crate::print::InMemoryPrintBackend)) — and the
/// fallback a binding uses when no real tray host exists, so the own-rendered
/// tray model stays demonstrable + RPC-introspectable headlessly.
#[derive(Debug)]
pub struct InMemoryTrayBackend {
    available: bool,
    published: RefCell<Option<TrayModel>>,
    publish_count: RefCell<u32>,
    pending: RefCell<Vec<TrayEvent>>,
}

impl Default for InMemoryTrayBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTrayBackend {
    /// An available backend with no model published yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            available: true,
            published: RefCell::new(None),
            publish_count: RefCell::new(0),
            pending: RefCell::new(Vec::new()),
        }
    }

    /// A backend reporting **no** tray host — the "publish returns
    /// `Unavailable`" path (a headless box with no `StatusNotifierWatcher`).
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            available: false,
            ..Self::new()
        }
    }

    /// The most recently published model, if any. Test / introspection read.
    #[must_use]
    pub fn published(&self) -> Option<TrayModel> {
        self.published.borrow().clone()
    }

    /// How many successful [`TrayBackend::publish`] calls have landed.
    #[must_use]
    pub fn publish_count(&self) -> u32 {
        *self.publish_count.borrow()
    }

    /// Script an event the next [`TrayBackend::poll_events`] will drain — the
    /// headless stand-in for a real click / menu activation (the demo + tests
    /// drive the tray this way, no panel needed).
    pub fn push_event(&self, event: TrayEvent) {
        self.pending.borrow_mut().push(event);
    }
}

impl TrayBackend for InMemoryTrayBackend {
    fn is_available(&self) -> bool {
        self.available
    }

    fn publish(&self, model: &TrayModel) -> Result<(), TrayError> {
        if !self.available {
            return Err(TrayError::Unavailable);
        }
        *self.published.borrow_mut() = Some(model.clone());
        *self.publish_count.borrow_mut() += 1;
        Ok(())
    }

    fn poll_events(&self) -> Vec<TrayEvent> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_menu() -> Vec<TrayMenuItem> {
        vec![
            TrayMenuItem::action("show", "Show window"),
            TrayMenuItem::check("dark", "Dark mode", false),
            TrayMenuItem::Separator,
            TrayMenuItem::SubMenu {
                label: "Build".to_owned(),
                items: vec![
                    TrayMenuItem::action("bake", "Bake assets"),
                    TrayMenuItem::Action {
                        id: "ship".to_owned(),
                        label: "Ship (disabled)".to_owned(),
                        enabled: false,
                    },
                ],
            },
            TrayMenuItem::action("quit", "Quit"),
        ]
    }

    fn sample_model() -> TrayModel {
        TrayModel::new("pinion.hello-tray", "Pinion", "applications-games")
            .with_tooltip("Pinion editor")
            .with_status(TrayStatus::Active)
            .with_menu(sample_menu())
    }

    #[test]
    fn status_tokens_round_trip() {
        for s in [
            TrayStatus::Active,
            TrayStatus::Passive,
            TrayStatus::NeedsAttention,
        ] {
            assert_eq!(
                TrayStatus::from_wire(s.as_str()),
                Some(s),
                "round-trip {s:?}"
            );
        }
        assert_eq!(TrayStatus::from_wire("active"), None, "strict casing");
        assert_eq!(TrayStatus::default(), TrayStatus::Active);
    }

    #[test]
    fn menu_item_ids_and_activation_guard() {
        let action = TrayMenuItem::action("show", "Show");
        assert_eq!(action.id(), Some("show"));
        assert!(action.can_activate("show"));
        assert!(!action.can_activate("hide"), "wrong id");
        assert_eq!(TrayMenuItem::Separator.id(), None);
        assert!(
            !TrayMenuItem::Separator.can_activate("show"),
            "a separator never activates"
        );
        // A disabled action cannot be activated.
        let disabled = TrayMenuItem::Action {
            id: "ship".to_owned(),
            label: "Ship".to_owned(),
            enabled: false,
        };
        assert_eq!(disabled.id(), Some("ship"), "id is still addressable");
        assert!(
            !disabled.can_activate("ship"),
            "but a disabled item rejects activation"
        );
    }

    #[test]
    fn model_can_activate_walks_submenus() {
        let model = sample_model();
        assert!(model.can_activate("show"), "top-level action");
        assert!(model.can_activate("dark"), "top-level check");
        assert!(model.can_activate("bake"), "nested submenu action");
        assert!(!model.can_activate("ship"), "nested but disabled");
        assert!(!model.can_activate("nope"), "unknown id");
    }

    #[test]
    fn in_memory_records_published_model() {
        let b = InMemoryTrayBackend::new();
        assert!(b.is_available());
        assert_eq!(b.published(), None, "nothing published at construction");
        assert_eq!(b.publish_count(), 0);
        let model = sample_model();
        b.publish(&model).expect("publish on an available host");
        assert_eq!(b.publish_count(), 1);
        assert_eq!(
            b.published().as_ref(),
            Some(&model),
            "the published model is recorded"
        );
        // Re-publishing an updated model overwrites + counts.
        let updated = model.clone().with_status(TrayStatus::NeedsAttention);
        b.publish(&updated).expect("re-publish");
        assert_eq!(b.publish_count(), 2);
        assert_eq!(
            b.published().map(|m| m.status),
            Some(TrayStatus::NeedsAttention)
        );
    }

    #[test]
    fn unavailable_backend_rejects_publish() {
        let b = InMemoryTrayBackend::unavailable();
        assert!(!b.is_available());
        assert_eq!(b.publish(&sample_model()), Err(TrayError::Unavailable));
        assert_eq!(b.publish_count(), 0, "a rejected publish records nothing");
        assert_eq!(b.published(), None);
    }

    #[test]
    fn poll_drains_scripted_events_once() {
        let b = InMemoryTrayBackend::new();
        assert!(b.poll_events().is_empty(), "no events at start");
        b.push_event(TrayEvent::Activated);
        b.push_event(TrayEvent::MenuItem("show".to_owned()));
        let drained = b.poll_events();
        assert_eq!(
            drained,
            vec![TrayEvent::Activated, TrayEvent::MenuItem("show".to_owned())]
        );
        assert!(
            b.poll_events().is_empty(),
            "a second poll drains nothing (events consumed once)"
        );
    }

    #[test]
    fn backend_is_object_safe() {
        let b: Box<dyn TrayBackend> = Box::new(InMemoryTrayBackend::new());
        assert!(b.is_available());
        assert!(b.publish(&sample_model()).is_ok());
    }
}
