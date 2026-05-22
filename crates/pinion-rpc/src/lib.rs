//! pinion-rpc — JSON-RPC 2.0 server with typed-hybrid method shape (§5.7).
//!
//! Path resolution against the SCE-emitted window topology lives in
//! [`path`] per §5.18 (optional `/window[id]/` prefix with single-window
//! short-circuit). Per-method dispatchers ([`query`], [`click`],
//! [`rewind`], [`snapshot`], [`dry_run`], [`wait_for`], [`screenshot`],
//! [`invoke`], [`intents`]; §5.12 ratified 7, R17 bidirectional-RPC
//! extended to 8, R18 §5.20 extended to 9) each live in their own
//! module. The JSON-RPC 2.0 wire envelope and method routing entry
//! point live in [`dispatch`].

pub mod commands;
pub mod dispatch;
pub mod dry_run;
pub mod focus;
pub mod font;
pub mod intents;
pub mod intervene;
pub mod invoke;
pub mod layout_query;
pub mod locate;
pub mod path;
pub mod preview;
pub mod query;
pub mod resize;
pub mod rewind;
pub mod screenshot;
pub mod snapshot;
pub mod text;
pub mod wait_for;

pub use commands::{
    list_in_flight_commands, list_pending_commands, CommandsError, PendingCommandView,
};
pub use dispatch::{
    dispatch, DeferredInput, DispatchContext, Request, RequestId, Response, RpcError,
};
pub use dry_run::{dry_run, DryRunError};
pub use focus::{
    focus_get, focus_next, focus_prev, focus_set, FocusError, FocusSetParams, FocusState,
};
pub use font::{
    cmap_subtables as font_cmap_subtables, dispose as font_dispose,
    family_name as font_family_name, full_name as font_full_name,
    glyph_id_for as font_glyph_id_for, glyph_outline as font_glyph_outline,
    list as font_list, metrics as font_metrics, parse as font_parse,
    postscript_name as font_postscript_name, subfamily_name as font_subfamily_name,
    CmapSubtableInfo, CmapSubtablesOutcome, CmapSubtablesParams, ComponentArgsInfo,
    ComponentInfo, ComponentTransformInfo, DisposeOutcome, DisposeParams, FamilyNameOutcome,
    FamilyNameParams, FontError, FontRegistry, FullNameOutcome, GlyphHeaderInfo,
    GlyphIdForOutcome, GlyphIdForParams, GlyphOutlineOutcome, GlyphOutlineParams,
    GlyphPointInfo, ListOutcome, MetricsOutcome, MetricsParams, NameAccessorParams,
    ParseOutcome, ParseParams, PostscriptNameOutcome, SubfamilyNameOutcome,
};
pub use intents::{drain_intents, IntentsError};
pub use invoke::{invoke, InvokeError};
pub use layout_query::{
    build_layout_node, layout_query, LayoutKind, LayoutNode, LayoutQueryError,
    LayoutQueryParams, LayoutRect, ViewportSize,
};
pub use locate::{
    bbox, locate, locate_region, BboxError, LocateError, LocateOutcome, LocateRegionOutcome,
};
pub use path::{resolve, PathError, ResolvedPath};
pub use preview::{
    apply_preview, cancel_preview, list_previews, propose_change, ApplyContext, ApplyError,
    ApplyOutcome, Entry as PreviewEntry, PreviewId, PreviewLedger, PreviewView, Proposal,
    ProposeError, ProposeOutcome, SweepReport, TypedProposal, ViewBlueprint, DEFAULT_CAPACITY,
    DEFAULT_TTL, MAX_TTL,
};
pub use query::{query, QueryError};
pub use resize::{resize, ResizeError, ResizeOutcome, ResizeParams};
pub use rewind::{rewind, RewindError};
pub use screenshot::{screenshot, Screenshot, ScreenshotError};
pub use snapshot::{
    snapshot, BoxSnapshot, ContainerSnapshot, ExternalSnapshot, ImageSnapshot, PathSnapshot,
    ScrollSnapshot, SnapshotError, SnapshotNode, TextSnapshot,
};
pub use text::{text_normalize, NormalizeForm, NormalizeOutcome};
pub use wait_for::{wait_for, WaitForError, WaitOutcome};
