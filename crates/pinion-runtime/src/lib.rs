//! pinion runtime — render loop and SCE hierarchical state machinery.
//!
//! Consumes the app-level statechart emitted from `app.scxml` (§5.19) and
//! exposes a window routing surface per §5.17 §5.18. Per-frame intent
//! collection (§5.20) lives in [`intent_queue`]; the §5.21 layout pass
//! lives in [`layout`]; the §5.35 input dispatch primitive (R48) lives
//! in [`input`] — its `InputRouter` is the framework-side surface that
//! replaces application-level hit-test wiring. R46.3.1 added
//! [`paint_adapter`] (feature `vello`) — the Scene → `vello::Scene`
//! framework primitive that replaced inline paint walkers in
//! consumer examples. R51.52 added [`focus`] — the §5.39 focus model
//! primitive that owns the focused-widget identity for key dispatch
//! and ARIA visual indication. R51.122 added [`core_shell`] — the
//! §5.41 backend-agnostic dispatch substrate `CoreShell<V: WidgetCore>`
//! that the Vello + TUI backends compose to share the
//! `scene` + `cached_state` + `router` + `intent_queue` plumbing.

pub mod core_shell;
pub mod focus;
pub mod input;
pub mod intent_queue;
pub mod layout;
#[cfg(feature = "vello")]
pub mod paint_adapter;
pub mod window;

pub use core_shell::{CoreShell, DispatchTail, StateChange};
pub use focus::FocusManager;
pub use input::{rect_for_tag, InputRouter, Modifiers, PointerId, Touch, TouchPhase};
pub use intent_queue::{walk_scene_and_drain, IntentQueue};
pub use layout::compute_layout;
pub use window::WindowRouter;
