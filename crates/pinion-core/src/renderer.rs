//! R51.109 §5.41 — backend-agnostic widget renderer trait.
//!
//! Lives in `pinion-core` (the lowest-level crate, no winit / wgpu /
//! ratatui deps) so multiple backend crates can implement it without
//! pulling cross-backend transitive dependencies. The two visible
//! backends (R51.109.x):
//!
//! | Backend | crate | `Frame` | `Context` |
//! |---|---|---|---|
//! | Vello GUI (§5.16) | `pinion-shell` | `vello::Scene` | `VelloContext` |
//! | ratatui TUI (§5.41) | `pinion-tui` | `ratatui::Buffer` | `TuiContext` |
//!
//! The pinion-shell `VelloRenderer` trait specialises this generic
//! trait at the Vello associated types; the pinion-tui `TuiRenderer`
//! impl plugs in directly. Both backends share the substrate's render
//! call surface (`render(frame, ctx)` + `resize`) so the shell-side
//! render loop is structurally identical across backends — only the
//! paint adapter and the renderer instantiation differ.

/// Backend-agnostic dispatch surface between the shell's render loop
/// and a concrete renderer implementation.
///
/// `Sized` so each binary can store `Box<R: WidgetRenderer>` in its
/// `RenderState::Active` without object-safety constraints; the
/// `WidgetView::Renderer` associated type pins a concrete impl so no
/// `dyn WidgetRenderer` appears in the hot path (§5.16 R45 R51.16
/// zero-virtual-dispatch guarantee).
///
/// `Error: Display` lets the shell surface render failures via
/// `eprintln!` (or any other consumer-side logging) without forcing
/// the application into the concrete error type.
///
/// `Frame` is the backend-specific painted-output type the shell hands
/// in: `vello::Scene` for the GPU pipeline, `ratatui::buffer::Buffer`
/// for the cell pipeline. The shell's paint-adapter walk builds this
/// type before each `render` call.
///
/// `Context: Copy` is the backend-specific frame-level hint (Vello
/// base color, TUI palette, etc.). `Copy` keeps the per-frame plumb
/// allocation-free; the shell constructs one value per `render` call
/// from the live paint scene.
pub trait WidgetRenderer: Sized {
    /// Concrete error type emitted by [`Self::render`]. The Vello
    /// codegen template emits `HelloFooRendererError` (`Debug` +
    /// `Display` + `Error`); TUI implementations surface
    /// `std::io::Error` directly. The `Display` bound is the only
    /// hard requirement — the shell uses it for `eprintln!` and
    /// nothing else.
    type Error: core::fmt::Display;

    /// Backend-specific painted-output type. The shell builds this
    /// via the backend-specific paint adapter (e.g.
    /// `pinion_runtime::paint_adapter::to_vello` for Vello,
    /// `pinion_tui::paint::to_buffer` for TUI) before each `render`
    /// call.
    type Frame;

    /// Backend-specific frame-level render hints (base color for
    /// Vello, palette for TUI, etc.). `Copy` so the shell passes by
    /// value per frame.
    type Context: Copy;

    /// Submit one painted frame against the backend's surface.
    ///
    /// # Errors
    /// Implementation-defined — frame submission failure, swapchain
    /// loss (Vello), IO error writing terminal cells (TUI), etc.
    fn render(&mut self, frame: &Self::Frame, ctx: Self::Context) -> Result<(), Self::Error>;

    /// Resize the backend surface to match a new logical dimension.
    /// Logical units are backend-specific: Vello sees pixels (DPI-aware);
    /// TUI sees cells (column / row count, post-`crossterm::Resize`).
    fn resize(&mut self, width: u32, height: u32);

    /// R1361.1 §5.16 — µs the last [`Self::render`] spent **blocked**
    /// rather than working: the Vello backend's `get_current_texture()`
    /// wait for a swapchain image.
    ///
    /// The shell brackets `render` as one wall-clock span and cannot see
    /// inside it, so a backend that blocks must report the block itself
    /// or the shell bills idle waiting to the render phase. That is the
    /// R1361 defect: under `PresentMode::AutoVsync` the acquire is the
    /// vsync pace-setter, so a window doing 0.4ms of work reported 998ms
    /// of "render" and read exactly like a GPU-bound one.
    ///
    /// **Defaults to `0`** — "this backend never blocks", which is the
    /// truth for TUI (a terminal write) and for GPU-less stub renderers.
    /// Only a backend with a real swapchain overrides it, so no
    /// implementation is forced to care.
    fn last_acquire_us(&self) -> u64 {
        0
    }
}
