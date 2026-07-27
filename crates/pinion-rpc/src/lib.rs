//! pinion-rpc — JSON-RPC 2.0 server with typed-hybrid method shape (§5.7).
//!
//! Path resolution against the SCE-emitted window topology lives in
//! [`path`] per §5.18 (optional `/window[id]/` prefix with single-window
//! short-circuit). Per-method dispatchers ([`query`](fn@crate::query), `click`,
//! [`rewind`](fn@crate::rewind), [`snapshot`](fn@crate::snapshot), [`dry_run`](fn@crate::dry_run), [`wait_for`](fn@crate::wait_for), [`screenshot`],
//! [`invoke`](fn@crate::invoke), [`intents`]; §5.12 ratified 7, R17 bidirectional-RPC
//! extended to 8, R18 §5.20 extended to 9) each live in their own
//! module. The JSON-RPC 2.0 wire envelope and method routing entry
//! point live in [`dispatch`](fn@crate::dispatch).

pub mod access;
pub mod animate_control;
pub mod animation_state;
pub mod cache_stats;
pub mod caret_state;
pub mod commands;
pub mod cross_window_drop;
pub mod dispatch;
pub mod dry_run;
pub mod export_pdf;
pub mod focus;
pub mod font;
pub mod frame_timings;
pub mod intents;
pub mod intervene;
pub mod invoke;
pub mod layout_query;
pub mod locate;
pub mod methods;
pub mod path;
pub mod preview;
pub mod query;
pub mod render_fidelity;
pub mod resize;
pub mod resolve;
pub mod rewind;
pub mod screenshot;
pub mod scroll_state;
pub mod simulate;
pub mod snapshot;
pub mod substrate_introspect;
pub mod text;
pub mod text_state;
pub mod theme;
pub mod transport;
pub mod wait_for;
pub mod waiter;
pub mod window_move;

#[cfg(test)]
pub(crate) mod test_fixtures;

pub use access::access_to_json;
pub use animate_control::{
    AnimateControlError, AnimateControlOutcome, animate_cancel, animate_settle,
};
pub use animation_state::{AnimationStateError, AnimationStateOutcome, animation_state};
pub use cache_stats::{CacheStatsError, CacheStatsOutcome, CacheStatsRect, cache_stats};
pub use caret_state::{CaretStateError, CaretStateOutcome, caret_state};
pub use commands::{
    CommandsError, PendingCommandView, list_in_flight_commands, list_pending_commands,
};
pub use cross_window_drop::{
    CrossWindowDropError, CrossWindowDropOutcome, CrossWindowDropParams, ResolvedCrossWindowDrop,
    cross_window_drop,
};
pub use dispatch::{
    ClickButton, DeclaredWindow, DeferredInput, DispatchContext, DragButton, DragPhase,
    KeyWireState, PacingState, Request, RequestId, Response, RpcError, dispatch, dispatch_parsed,
    parse_request, unknown_window_verdict,
};
pub use dry_run::{DryRunError, dry_run};
pub use focus::{
    FocusError, FocusSetParams, FocusState, focus_get, focus_next, focus_prev, focus_set,
};
pub use font::{
    CmapSubtableInfo, CmapSubtablesOutcome, CmapSubtablesParams, ComponentArgsInfo, ComponentInfo,
    ComponentTransformInfo, DisposeOutcome, DisposeParams, FamilyNameOutcome, FamilyNameParams,
    FontError, FontRegistry, FullNameOutcome, GlyphHeaderInfo, GlyphIdForOutcome, GlyphIdForParams,
    GlyphOutlineOutcome, GlyphOutlineParams, GlyphPointInfo, ListOutcome, MetricsOutcome,
    MetricsParams, NameAccessorParams, ParseOutcome, ParseParams, PostscriptNameOutcome,
    SubfamilyNameOutcome, cmap_subtables as font_cmap_subtables, dispose as font_dispose,
    family_name as font_family_name, full_name as font_full_name,
    glyph_id_for as font_glyph_id_for, glyph_outline as font_glyph_outline, list as font_list,
    metrics as font_metrics, parse as font_parse, postscript_name as font_postscript_name,
    subfamily_name as font_subfamily_name,
};
pub use frame_timings::{
    FrameTimingsError, FrameTimingsLast, FrameTimingsOutcome, FrameTimingsProduce,
    FrameTimingsWindow, frame_timings,
};
pub use intents::{IntentsError, drain_intents};
pub use invoke::{InvokeError, invoke};
pub use layout_query::{
    LayoutKind, LayoutNode, LayoutQueryError, LayoutQueryParams, LayoutRect, ViewportSize,
    build_layout_node, layout_query, project_layout,
};
pub use locate::{
    BboxError, LocateError, LocateOutcome, LocateRegionOutcome, bbox, locate, locate_region,
};
pub use methods::{MethodEntry, MethodOcc, OCC_DOC, RPC_METHODS, RpcMethods, rpc_methods};
pub use path::{PathError, ResolvedPath, resolve};
pub use preview::{
    ApplyContext, ApplyError, ApplyOutcome, DEFAULT_CAPACITY, DEFAULT_TTL, Entry as PreviewEntry,
    MAX_TTL, PreviewId, PreviewLedger, PreviewView, Proposal, ProposeError, ProposeOutcome,
    SweepReport, TypedProposal, ViewBlueprint, apply_preview, cancel_preview, list_previews,
    propose_change,
};
pub use query::{QueryError, query};
pub use resize::{ResizeError, ResizeOutcome, ResizeParams, resize};
pub use resolve::{
    ResolveExternalError, introspect_at, introspect_mut_at, resolve_external_introspect,
    resolve_external_introspect_mut, resolve_external_path,
};
pub use rewind::{RewindError, rewind};
pub use screenshot::{Screenshot, ScreenshotError};
pub use scroll_state::{
    ScrollAxisPair, ScrollEdges, ScrollStateError, ScrollStateOutcome, scroll_state,
};
pub use simulate::{SimulateError, SimulateStep, simulate};
pub use snapshot::{
    BoxSnapshot, ContainerSnapshot, ExternalSnapshot, ImageSnapshot, PathSnapshot, ScrollSnapshot,
    SnapshotError, SnapshotNode, TextSnapshot, snapshot,
};
pub use substrate_introspect::{
    SubstrateIntrospectError, introspect_error_to_data, lookup as substrate_lookup,
};
pub use text::{NormalizeForm, NormalizeOutcome, text_normalize};
pub use text_state::{TextSelectionView, TextStateError, TextStateOutcome, text_state};
pub use theme::{
    DEFAULT_THEME_TAG, PaletteCatalogue, PaletteTokens, SetThemeModeError, SetThemeModeOutcome,
    SetThemeModeParams, ThemeTokenView, ThemeTokensError, ThemeTokensOutcome, set_theme_mode,
    theme_tokens,
};
pub use transport::{ConnId, RpcFrame, RpcIngress, RpcReply};
pub use wait_for::{WaitForError, WaitOutcome, wait_for};
pub use waiter::{WaiterRegistry, try_async_wait_for, waiter_response};
pub use window_move::{WindowMoveError, WindowMoveOutcome, WindowMoveParams, window_move};
