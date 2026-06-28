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

pub mod drag_image;
pub(crate) mod edge;
pub mod event;
pub mod focus_ring;
pub mod highlight;
pub mod window_chrome;

pub use drag_image::{DRAG_IMAGE_TAG, DragImageStyle, inject_drag_image};
pub use event::OverlayEvent;
pub use focus_ring::{FOCUS_RING_TAG, FocusRingStyle, inject_focus_ring};
pub use highlight::{
    HIGHLIGHT_TAG_PREFIX, HighlightStyle, clear_highlights, inject_highlight, inject_overlay_node,
};
pub use window_chrome::{
    WINDOW_CHROME_CLOSE_TAG, WINDOW_CHROME_GRIP_TAG, WINDOW_CHROME_MAXIMIZE_TAG,
    WINDOW_CHROME_MINIMIZE_TAG, WINDOW_CHROME_TAG, WINDOW_RESIZE_EAST_TAG,
    WINDOW_RESIZE_NORTH_EAST_TAG, WINDOW_RESIZE_NORTH_TAG, WINDOW_RESIZE_NORTH_WEST_TAG,
    WINDOW_RESIZE_SOUTH_EAST_TAG, WINDOW_RESIZE_SOUTH_TAG, WINDOW_RESIZE_SOUTH_WEST_TAG,
    WINDOW_RESIZE_TAG_PREFIX, WINDOW_RESIZE_WEST_TAG, WindowChromeStyle, inject_resize_border,
    inject_window_chrome,
};
