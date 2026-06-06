//! pinion-overlay — AI overlay UX layer per §5.33.
//!
//! Sits *between* the §5.32 introspection primitives (which give an AI
//! agent xy↔path conversion) and an end-user scene. Two responsibilities:
//!
//!   1. **Event capture model** — define a transport-agnostic
//!      [`OverlayEvent`] enum that backends (winit / web / tui /
//!      headless test fixture) lower their raw input to. The overlay
//!      itself never touches winit, softbuffer, or any platform layer.
//!
//!   2. **Highlight injection** — pure-function transforms over a
//!      [`Scene`] tree that add or remove `Scene::Box` nodes carrying
//!      the `ai-overlay/` tag prefix. Introspect-friendly by
//!      construction (§5.20 tags survive every existing scene walk)
//!      and dry_run-compatible (immutable transforms, see §2 #3).
//!
//! ## v0 shape (R39.4.1)
//!
//! Functional API only. No `OverlayController` struct, no mutable
//! state owner. Callers — typically the example or runtime — drive the
//! transitions themselves:
//!
//! ```rust,ignore
//! use pinion_overlay::{inject_highlight, clear_highlights, HighlightStyle, OverlayEvent};
//!
//! match event {
//!     OverlayEvent::Click { x, y } => {
//!         let outcome = pinion_rpc::locate(&scene, x, y)?;
//!         scene = inject_highlight(scene, &outcome.path, HighlightStyle::default());
//!     }
//!     OverlayEvent::Escape => {
//!         scene = clear_highlights(scene);
//!     }
//!     _ => {}
//! }
//! ```
//!
//! Controller-pattern promotion (a stateful `OverlayController` owning
//! the current selection / multi-highlight ledger / undo stack) is
//! carry-forward per §5.33 v0 caveat — evidence from
//! `ai-introspect-demo` informs the shape.

#![forbid(unsafe_code)]

pub(crate) mod edge;
pub mod event;
pub mod focus_ring;
pub mod highlight;

pub use event::OverlayEvent;
pub use focus_ring::{inject_focus_ring, FocusRingStyle, FOCUS_RING_TAG};
pub use highlight::{
    clear_highlights, inject_highlight, HighlightStyle, HIGHLIGHT_TAG_PREFIX,
};
