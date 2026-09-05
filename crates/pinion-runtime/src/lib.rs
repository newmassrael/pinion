//! pinion runtime — render loop and SCE hierarchical state machinery.
//!
//! Consumes the app-level statechart emitted from `app.scxml` (§5.19) and
//! exposes a window routing surface per §5.17 §5.18. Per-frame intent
//! collection (§5.20) lives in [`intent_queue`]; the §5.21 layout pass
//! lives in [`layout`]; the §5.35 input dispatch primitive (R48) lives
//! in [`input`] — its `InputRouter` is the framework-side surface that
//! replaces application-level hit-test wiring. R46.3.1 added
//! [`paint_adapter`][vello-paint-adapter] (feature `vello`) — the Scene → `vello::Scene`
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
//!
// R1945.2 — LINK DEFINITIONS FOR THE `vello`-GATED MODULES.
//
// Three of this crate's modules — `paint_adapter`, `text_engine` and
// `image_cache` — exist only under the `vello` feature, and the crate doc
// above is ungated prose that names one of them. An intra-doc link into a
// gated module is BROKEN in this crate's own default configuration:
// `cargo doc -p pinion-runtime` answered seven `unresolved link` errors across
// this file, `layout.rs` and `paint_cache_stats.rs` on 2026-09-01, and had for
// as long as those links had been written. Nothing saw it — CI documents the
// WORKSPACE with `--features pinion-runtime/vello`, where they all resolve,
// and the per-crate `pre-push` gate (R1916) docs only the crates a push
// touches, which had not included this one since R1905.
//
// The repair keeps ONE copy of the prose: the text above carries a markdown
// reference whose label is KEBAB-CASE, and the label's definition is supplied
// below only when the feature is on. With `vello` the label resolves and the
// reader gets a hyperlink; without it the label is undefined, pulldown-cmark
// renders the text verbatim, and nothing is asked to resolve. A path-shaped
// label does NOT work — rustdoc resolves an undefined one as a path, measured
// — and a `cfg_attr` pair carrying the sentence twice would be one rule with
// two spellings, the exact defect R1945 repaired one file away.
//
// ⚠ A kebab label with no definition is SILENT in both configurations, so the
// pairing is a gate: `tools/feature_gated_doc_links.py`, run by `pre-push`.
#![cfg_attr(feature = "vello", doc = "[vello-paint-adapter]: crate::paint_adapter")]

pub mod command;
pub mod core_shell;
pub mod draw_profile;
pub mod driven_pointer;
pub mod focus;
pub mod frame_pacing;
pub mod frame_timing;
#[cfg(feature = "vello")]
pub mod image_cache;
pub mod input;
pub mod intent_queue;
pub mod introspection_paint;
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
pub use core_shell::{
    ASCII_PROBE_RANGE, AcceleratorRow, CoreShell, DEFAULT_WINDOW, DispatchTail, StateChange,
};
pub use draw_profile::{DrawProfile, DrawProfileNode, DrawProfiler};
pub use driven_pointer::DrivenPointer;
pub use focus::FocusManager;
pub use frame_pacing::{FixedTimestep, MAX_FRAME_DT_SECS, PacingState, clamp_frame_dt, substep};
pub use frame_timing::{
    AdapterFacts, DrawWork, FRAME_TIMING_WINDOW, FRAME_TIMINGS, FocusWork, FocusWorkCell,
    FrameTiming, FrameTimingStats, FrameTimingsHolder, FrameTimingsSnapshot, FrameTimingsView,
    GpuBackend, GpuDeviceClass, MirrorWork, PaintWork, ProduceWork, instant_delta_us,
    use_frame_timings,
};
/// R1404 §5.16 — the producer in-memory image surface, re-exported at the
/// crate root (gated with the `image_cache` module on `vello`): the
/// `memory://<key>` scheme constant, the mutable store, and its resolve
/// hooks, so a producer names `pinion_runtime::use_image_store` the way it
/// names [`use_frame_timings`].
#[cfg(feature = "vello")]
pub use image_cache::{MEMORY_SCHEME, MemoryImageStore, resolve_image_store, use_image_store};
pub use input::{
    AutoRepeatHold, CrossWindowDrop, ExternalSizes, InputRouter, Modifiers, PanRelease, PointerId,
    PointerReach, PointerShadow, Touch, TouchPhase, announce_external_sizes, pointer_reach,
    record_painted_surface, record_painted_surfaces, rect_for_tag, resolve_cross_window_drop,
    resolve_pointer_tag, wheel_intent_at,
};
pub use intent_queue::{IntentQueue, walk_scene_and_drain, walk_scene_and_drain_immediate};
pub use introspection_paint::IntrospectionPaint;
pub use layout::{
    LayoutPass, SETTLE_PASS_BUDGET, Settled, TextBox, TextMeasure, compute_layout,
    compute_layout_with_scroll_dirty, compute_layout_with_text_measure, settle_to_fixed_point,
};
pub use paint_cache_stats::FragmentCacheStats;
/// R1404 §5.16 — the decoded RGBA image a producer registers into the
/// [`MemoryImageStore`][vello-memory-image-store]. Re-exported (ungated —
/// `pinion-asset` is a non-optional dependency) so a consumer can name the
/// `insert` parameter type of this crate's own public store, the
/// [`LayoutCache`] rationale.
// R1945.2 — the store is `vello`-gated and this re-export is not; see the
// crate-doc note above for why the label's definition is supplied only when
// the item it names exists.
#[cfg_attr(
    feature = "vello",
    doc = "[vello-memory-image-store]: crate::image_cache::MemoryImageStore"
)]
pub use pinion_asset::DecodedImage;
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
pub use render_fidelity::{GridFidelity, PresentHealth, RenderFidelity};
pub use window::WindowRouter;
