//! `External` primitive integration contract (§5.15, R16 slice 6).
//!
//! Eight-point contract ratified Round 7:
//!
//!   1. Backend support declaration — [`External::backends`]
//!   2. Repaint trigger ownership   — [`External::repaint_ownership`]
//!   3. Thread ownership            — [`External::thread_ownership`]
//!   4. Lifecycle event callbacks   — `on_mount` / `on_unmount` /
//!                                    `on_visibility_change` / `on_focus_change`
//!   5. Input forwarding policy     — [`External::handles_event`]
//!   6. DPI / resize notification   — `on_dpi_change` / `on_resize`
//!   7. Async state change channel  — [`External::poll_state`] (pull form)
//!   8. Symbolic introspection      — *opt-in*, lands as a separate
//!                                    sub-trait in a later slice
//!
//! Items 1-7 are mandatory; items 1-3 are required (no default), items
//! 4-7 ship sensible no-op defaults so authors only override what they
//! need.
//!
//! The trait is **dyn-safe** by construction (all methods take `&self`
//! or `&mut self`, no associated consts, no `Self`-returning methods).
//! This keeps `Box<dyn External>` available for heterogeneous storage
//! when the §5.15 scene-tree integration lands.
//!
//! `StubExternal` is a ref-impl: Gui-only, framework-driven repaint,
//! UI-thread synchronous. It exists to anchor the contract semantically
//! and to give tests/examples a baseline.

use crate::Event;

/// Render backends an `External` may declare support for (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    /// GPU-backed window per §5.9 trait-based Renderer.
    Gui,
    /// Text terminal rendering per §5.9 dual backend.
    Tui,
    /// JSON-RPC symbolic surface per §5.7 §5.12.
    Rpc,
}

/// What the framework should do when a scene targets a backend the
/// `External` does not support (§5.15 item 1 fallback policy).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendFallback {
    /// Reject the scene at composition time — per §5.15 caveat,
    /// non-conforming `External` should not silently break.
    Reject,
    /// Skip the `External` (renders as an empty placeholder); useful
    /// for optional content like a video viewport when running headless.
    Skip,
}

/// Backend support declaration (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSupport {
    /// Backends the `External` can dispatch into. Order is not
    /// significant; uniqueness is the implementor's responsibility.
    pub supported: &'static [Backend],
    /// Policy for unsupported backends.
    pub fallback: BackendFallback,
}

impl BackendSupport {
    #[must_use]
    pub const fn new(supported: &'static [Backend], fallback: BackendFallback) -> Self {
        Self {
            supported,
            fallback,
        }
    }

    /// Returns `true` when this `External` declares support for `backend`.
    #[must_use]
    pub fn supports(&self, backend: Backend) -> bool {
        self.supported.contains(&backend)
    }
}

/// Who drives repaint scheduling for an `External` (§5.15 item 2).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintOwner {
    /// Framework decides when to repaint (layout-driven; default for
    /// static content like styled boxes embedding an SVG).
    Framework,
    /// `External` owns its render loop (game viewport, video player);
    /// the framework just composes the resulting surface.
    External,
}

/// Where the `External` performs its work (§5.15 item 3).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadOwnership {
    /// `External` runs synchronously on the UI thread.
    UiThreadSync,
    /// `External` owns a worker thread; framework communicates via a
    /// sync channel. State pushes use [`External::poll_state`] today;
    /// push-form variant lands when §6.3 async boundary settles.
    OwnThread,
}

/// Opaque state-update payload from an `External` to the framework
/// (§5.15 item 7). The concrete schema is settled in a later slice —
/// today this is a marker so the contract surface stays stable.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct StateUpdate;

/// The 8-point integration contract (§5.15). Items 1-3 are required;
/// items 4-7 have no-op defaults so authors override selectively.
pub trait External {
    // --- 1. Backend support declaration ---

    /// Which backends this `External` dispatches into, and the policy
    /// for unsupported ones.
    fn backends(&self) -> BackendSupport;

    // --- 2. Repaint trigger ownership ---

    /// Whether the framework drives repaints (layout cadence) or the
    /// `External` owns its own render loop.
    fn repaint_ownership(&self) -> RepaintOwner;

    // --- 3. Thread ownership ---

    /// UI-thread synchronous, or `External`-owned worker thread.
    fn thread_ownership(&self) -> ThreadOwnership;

    // --- 4. Lifecycle event callbacks ---

    fn on_mount(&mut self) {}
    fn on_unmount(&mut self) {}
    fn on_visibility_change(&mut self, _visible: bool) {}
    fn on_focus_change(&mut self, _focused: bool) {}

    // --- 5. Input forwarding policy ---

    /// Return `true` to claim the event (framework does *not* forward
    /// it further); `false` lets the framework process normally. Default
    /// is `false` — the framework forwards every event.
    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    // --- 6. DPI / resize notification ---

    fn on_dpi_change(&mut self, _scale: f32) {}
    fn on_resize(&mut self, _width: u32, _height: u32) {}

    // --- 7. Async state change channel (pull form) ---

    /// Poll for a state change pushed by the `External`. Default
    /// returns `None`. Push-form (channel-based) lands when §6.3 async
    /// boundary is settled at the runtime crate edge.
    fn poll_state(&mut self) -> Option<StateUpdate> {
        None
    }
}

/// Reference no-op `External`: Gui only, framework-driven repaint,
/// UI-thread synchronous. Useful as a baseline for tests and as a
/// minimal example for new `External` authors.
#[derive(Debug, Clone, Copy, Default)]
pub struct StubExternal;

impl StubExternal {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::WindowEvent;

    #[test]
    fn stub_declares_gui_only_with_skip_fallback() {
        let stub = StubExternal::new();
        let support = stub.backends();
        assert!(support.supports(Backend::Gui));
        assert!(!support.supports(Backend::Tui));
        assert!(!support.supports(Backend::Rpc));
        assert_eq!(support.fallback, BackendFallback::Skip);
    }

    #[test]
    fn stub_uses_framework_repaint_and_ui_thread() {
        let stub = StubExternal::new();
        assert_eq!(stub.repaint_ownership(), RepaintOwner::Framework);
        assert_eq!(stub.thread_ownership(), ThreadOwnership::UiThreadSync);
    }

    #[test]
    fn stub_does_not_claim_any_event() {
        let stub = StubExternal::new();
        let event = Event::Window(WindowEvent::Close);
        assert!(!stub.handles_event(&event));
    }

    #[test]
    fn stub_lifecycle_and_dpi_callbacks_are_noop() {
        let mut stub = StubExternal::new();
        stub.on_mount();
        stub.on_visibility_change(true);
        stub.on_focus_change(false);
        stub.on_dpi_change(2.0);
        stub.on_resize(800, 600);
        stub.on_unmount();
    }

    #[test]
    fn stub_poll_state_is_none() {
        let mut stub = StubExternal::new();
        assert!(stub.poll_state().is_none());
    }

    #[test]
    fn trait_is_dyn_safe() {
        // Compile-time guard: any future change that loses dyn-safety
        // (associated consts, Self-returning methods, etc.) breaks this.
        let _: Box<dyn External> = Box::new(StubExternal::new());
    }
}
