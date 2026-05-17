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
//! consumer examples.

pub mod input;
pub mod intent_queue;
pub mod layout;
#[cfg(feature = "vello")]
pub mod paint_adapter;
pub mod window;

pub use input::InputRouter;
pub use intent_queue::{walk_scene_and_drain, IntentQueue};
pub use layout::compute_layout;
pub use window::WindowRouter;
