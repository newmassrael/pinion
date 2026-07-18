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
//! R51.141 added [`command`] — the §5.23 [`Handler`] trait +
//! [`HandlerRegistry`] async dispatch surface that completes the
//! `Command` two-layer effects model (R51.139 substrate + R51.141
//! dispatch). R51.156 extended [`command`] with the [`Executor`] +
//! [`IntentSink`] traits and the [`CommandExecutor`] composite that
//! closes the R27 dispatch loop: `Command → Handler future →
//! Executor::spawn → Intent → IntentSink → UI thread re-dispatch`.
//! R51.145 added [`frame_pacing`] — the §5.28 per-frame
//! `dt` clamp helper that protects the spring solver against
//! background-resume / debugger-pause `dt` spikes.

pub mod command;
pub mod core_shell;
pub mod focus;
pub mod frame_pacing;
pub mod frame_timing;
#[cfg(feature = "vello")]
pub mod image_cache;
pub mod input;
pub mod intent_queue;
pub mod layout;
#[cfg(feature = "vello")]
pub mod paint_adapter;
pub mod paint_cache_stats;
pub mod render_fidelity;
#[cfg(feature = "vello")]
pub mod text_engine;
pub mod window;

pub use command::{
    BlockOnExecutor, BoxFuture, CommandExecutor, CommandTaskHandle, Executor, Handler,
    HandlerFuture, HandlerRegistry, IntentSink, VecSink,
};
pub use core_shell::{CoreShell, DEFAULT_WINDOW, DispatchTail, StateChange};
pub use focus::FocusManager;
pub use frame_pacing::{FixedTimestep, MAX_FRAME_DT_SECS, PacingState, clamp_frame_dt, substep};
pub use frame_timing::{
    FRAME_TIMING_WINDOW, FRAME_TIMINGS, FrameTiming, FrameTimingStats, FrameTimingsHolder,
    FrameTimingsSnapshot, FrameTimingsView, use_frame_timings,
};
pub use input::{
    CrossWindowDrop, InputRouter, Modifiers, PanRelease, PointerId, Touch, TouchPhase,
    rect_for_tag, resolve_cross_window_drop,
};
pub use intent_queue::{IntentQueue, walk_scene_and_drain, walk_scene_and_drain_immediate};
pub use layout::{
    TextBox, TextMeasure, compute_layout, compute_layout_with_scroll_dirty,
    compute_layout_with_text_measure,
};
pub use paint_cache_stats::FragmentCacheStats;
/// R1344 §5.36 — re-export of the cache every `compute_layout*` entry takes by
/// `&mut`. Without it a caller cannot name the parameter type of this crate's
/// own public functions.
///
/// This is API completeness, NOT dependency relief: `pinion-text` is a
/// non-optional dependency of this crate, so every consumer already links
/// parley / swash transitively — the re-export saves a `Cargo.toml` line, not
/// weight.
///
/// **Known wart** (R1344, deferred): `compute_layout_with_text_measure` takes
/// `&mut LayoutCache` unconditionally, even when the supplied [`TextMeasure`]
/// never defers to parley. The TUI backend therefore constructs and carries a
/// parley shaping LRU it provably never populates ([`TextBox`]-returning cell
/// measure always answers `Some`). The textbook seam is `Option<&mut
/// LayoutCache>`, or a measure that owns its own cache; both change every
/// `compute_layout*` call site across both backends, so it wants its own round
/// rather than a drive-by.
pub use pinion_text::LayoutCache;
pub use render_fidelity::{GridFidelity, RenderFidelity};
pub use window::WindowRouter;
