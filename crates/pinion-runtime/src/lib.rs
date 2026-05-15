//! pinion runtime — render loop and SCE hierarchical state machinery.
//!
//! Consumes the app-level statechart emitted from `app.scxml` (§5.19) and
//! exposes a window routing surface per §5.17 §5.18.

pub mod window;

pub use window::WindowRouter;
