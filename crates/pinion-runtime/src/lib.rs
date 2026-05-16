//! pinion runtime — render loop and SCE hierarchical state machinery.
//!
//! Consumes the app-level statechart emitted from `app.scxml` (§5.19) and
//! exposes a window routing surface per §5.17 §5.18. Per-frame intent
//! collection (§5.20) lives in [`intent_queue`].

pub mod intent_queue;
pub mod window;

pub use intent_queue::{walk_scene_and_drain, IntentQueue};
pub use window::WindowRouter;
