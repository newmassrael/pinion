//! `scene/snapshot` RPC method dispatch (§5.12 method 4 of 7, R16 slice 13).
//!
//! Captures the scene tree shape and, for `Scene::External` that opts
//! in to §5.15 item 8 introspection, dumps every `(path, value)` pair
//! declared by `ExternalIntrospect::schema`.
//!
//! R51.194 §5.49 §5.45 — the dispatcher recurses into
//! `Scene::Container.children` and `Scene::Scroll.content`.
//!
//! R51.198 §5.49 — leaf primitives (`Box`, `Text`, `Path`, `Image`)
//! and the `Container` / `External` parents now expose `rect` + `tag`
//! (and `content` for `Text`). With this layer the AI-side harness can
//! locate widgets by tag and derive bboxes without hardcoding pixel
//! coordinates per demo (see `tools/demos/hello_toggle_click.py` first
//! consumer). Only `Effect` stays opaque per §3 capability boundary.
//!
//! R55.G.8 / R55.G.10 §5.49 — `Box`, `Text`, and `Container` surface
//! their `BoxStyle` / `TextStyle` sidecars. G.8 landed the visual axis
//! (fill, border, corner radius, font family / size / colour / weight /
//! style); G.10 added the layout axis (line-height, letter-spacing,
//! text-align, decoration, overflow). The wire JSON converts each style
//! to a structured object — `{r, g, b, a}` for colours, named variants
//! for enums — so AI clients can introspect every rendered typography
//! and chrome knob without inspecting pixels (§2 #7 scene-as-data).
//!
//! Surface details:
//!   * path: `/[window[id]/]` only — no scene-path tail, since v0 has
//!     no addressable sub-tree shape (`scene/query` is the typed
//!     descend-by-path channel; snapshot is the whole-tree dump).
//!   * leaf primitives (`Box`, `Text`, `Path`, `Image`) report
//!     `rect` and optional `tag`. `Text` additionally reports
//!     `content` (§2 #7 scene-as-data invariant).
//!   * `Container` exposes `rect`, `tag`, and recurses through
//!     `children`.
//!   * `Scroll` exposes `tag`, `viewport` rect, `(offset_x, offset_y)`,
//!     and recurses through `content` — the §5.45 R55 substrate fields
//!     a Scroll-aware demo needs to assert the visible row window.
//!     `viewport` IS the scroll primitive's geometry (per
//!     `ScrollNode::viewport` doc) so no separate `rect` field.
//!   * `External` exposes `rect`, `tag`, and the
//!     `ExternalIntrospect::schema` fields per §5.15 item 8.
//!   * `Effect` is opaque per §3 — no exposure beyond the discriminator.
//!   * fallback [`SnapshotNode::Unknown`] keeps the dispatcher
//!     forward-compatible with `non_exhaustive` `Scene` additions in
//!     pinion-core.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::Rect;
use pinion_core::style::{BoxStyle, ImageStyle, PathStyle, TextStyle};
use pinion_core::{
    CellAttrs, CellWidth, ColorTarget, CursorShape, GridBuffer, GridCursor, Palette, Scene,
    TermColor,
};

use crate::path::{self, PathError};

/// One scene tree primitive's snapshot shape.
///
/// R51.198 §5.49 — leaf primitives carry a payload struct exposing
/// `rect` and optional `tag`. `Effect` stays unit-marker per §3
/// opaque-primitive boundary, and `Unknown` is the forward-compatible
/// catch-all.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotNode {
    Box(BoxSnapshot),
    Text(TextSnapshot),
    Path(PathSnapshot),
    Image(ImageSnapshot),
    Container(ContainerSnapshot),
    Effect,
    External(ExternalSnapshot),
    Scroll(ScrollSnapshot),
    /// R681 §2 #4 — [`Scene::ImmediateModeNode`] payload. Exposes
    /// the §5.20 tag (so [`find_by_tag`] walks resolve), the
    /// post-layout `viewport`, and the per-paint `last_dt`
    /// sidecar (microseconds — `u64` for stable JSON encoding;
    /// callers convert back to a [`std::time::Duration`] /
    /// fractional seconds at the boundary).
    ImmediateModeNode(ImmediateModeSnapshot),
    /// R972 §5.41 — [`Scene::TextGrid`] geometry payload. Exposes the
    /// pixel `rect`, the node-local cell metric (`cell_w` / `cell_h`),
    /// and the derived winsize `(cols, rows)` so an AI client reads the
    /// cell-native coordinate space as data (§2 #7) — the cell↔pixel
    /// mapping is fully reconstructible from these without OCR.
    TextGrid(TextGridSnapshot),
    /// Catch-all for `Scene` variants added in a later pinion-core
    /// version that this dispatcher predates.
    Unknown,
}

/// `Box` payload of [`SnapshotNode::Box`] (R51.198 §5.49,
/// R55.G.8 added `style`).
///
/// `rect` mirrors `BoxNode.rect`; `tag` mirrors `BoxNode.tag`;
/// `style` mirrors `BoxNode.style` so AI clients can introspect the
/// rendered fill, border, and corner radius without OCR (§2 #7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct BoxSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub style: BoxStyle,
}

/// `Text` payload of [`SnapshotNode::Text`] (R51.198 §5.49,
/// R55.G.8 added `style`).
///
/// `content` mirrors `TextNode.content` — pinion exposes text as data
/// per §2 invariant #7 (scene-as-data), so AI clients can read the
/// rendered text without OCR'ing a screenshot. `style` mirrors
/// `TextNode.style` to surface the visual axis (font family / size /
/// colour / weight / style) on the same scene-as-data principle.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TextSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub content: String,
    pub style: TextStyle,
    /// R713 §5.36 — the styled-run spans (rich / multi-style text).
    /// Empty for a single-style node. Mirrors `TextNode.runs` so an AI
    /// client reads each span's byte range + resolved style as data
    /// (§2 #7 scene-as-data) — `RichText` is introspectable without
    /// OCR'ing per-colour glyphs off a screenshot.
    pub runs: Vec<pinion_core::scene::StyleRun>,
}

/// `Path` payload of [`SnapshotNode::Path`] (R51.198 §5.49 + carry,
/// R55.G.11 added `style`).
///
/// `commands` mirrors `PathNode.commands` — the structured command
/// stream the rasterizer consumes. Exposing the commands keeps
/// `scene-as-data` (§2 #7) complete for vector geometry: an AI
/// agent can introspect path shape without OCR'ing the painted
/// result. `style` mirrors `PathNode.style` so the agent also sees
/// the paint (stroke colour / width / cap, fill colour) — the
/// commands describe the shape, the style describes the ink.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PathSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub commands: Vec<pinion_core::scene::PathCommand>,
    pub style: PathStyle,
}

/// `Image` payload of [`SnapshotNode::Image`] (R51.198 §5.49 + carry,
/// R55.G.11 added `style`).
///
/// `source` mirrors `ImageNode.source` — typically the asset URI or
/// path the application loaded. The string is opaque to the
/// framework but lets an AI agent verify "this is the right icon"
/// without inspecting pixels (§2 #7 scene-as-data). `style` mirrors
/// `ImageNode.style` (fit policy + optional tint overlay) so the
/// agent can verify the rendered size-fitting and recolouring.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub source: String,
    pub style: ImageStyle,
}

/// `Container` payload of [`SnapshotNode::Container`] (R51.194 §5.49,
/// R51.198 added `rect`, R55.G.8 added `style`).
///
/// `tag` mirrors `ContainerNode.tag` (the §5.20 intent-routing handle).
/// `rect` mirrors `ContainerNode.rect`.
/// `style` mirrors `ContainerNode.style` — Container nodes paint their
/// own fill / border / corner radius before recursing into children,
/// so the same §2 #7 scene-as-data surface applies as for `BoxNode`.
/// `children` is a depth-first traversal of `ContainerNode.children`;
/// each entry is itself a `SnapshotNode`, including nested containers
/// and scrolls, so a single root snapshot is the whole tree.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub style: BoxStyle,
    pub children: Vec<SnapshotNode>,
}

/// `Scroll` payload of [`SnapshotNode::Scroll`] (R51.194 §5.49 §5.45).
///
/// Exposes the §5.45 R55 substrate fields a Scroll demo needs:
///   * `tag` mirrors `ScrollNode.tag` (input-router handle, e.g.
///     `"main_listbox"`),
///   * `viewport` is the clip rect in logical pixels / cells — also
///     the scroll primitive's geometry (no separate `rect` field),
///   * `offset_x` / `offset_y` are the current scroll position,
///   * `content` is the (recursive) snapshot of the scene clipped by
///     this scroll — typically a `Container` of widget rows.
///
/// The reactive `state` link on `ScrollNode` is *not* exposed — `Rc`
/// handles are not serialisable, and the declarative fields are
/// sufficient for AI-side assertions (offset / viewport / tree
/// shape).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollSnapshot {
    pub tag: Option<String>,
    pub viewport: Rect,
    pub offset_x: i32,
    pub offset_y: i32,
    /// (R784 §5.45) Which axis this container scrolls — `"vertical"` or
    /// `"horizontal"` (the wire form of
    /// [`ScrollAxis`](pinion_core::scene::ScrollAxis)). An AI client that
    /// sees `offset_x` move knows from this whether the container is the
    /// horizontal scroller (e.g. a frozen-header data-grid's outer scroll)
    /// rather than guessing from the offset alone.
    pub axis: &'static str,
    pub content: Box<SnapshotNode>,
}

/// R681 §2 #4 — `ImmediateModeNode` payload of
/// [`SnapshotNode::ImmediateModeNode`]. Exposes the §5.20 `tag`
/// (so [`find_by_tag`] walks resolve to the immediate-mode subtree),
/// the post-layout `viewport` (the rect the backend bridge paints
/// into), and the substrate-published `last_dt_micros` per-paint
/// delta sidecar — encoded as `u64` microseconds so JSON consumers
/// see a stable integer rather than a struct of `secs` + `nanos`.
///
/// The driver handle itself is not exposed (`Rc<RefCell<dyn
/// ImmediateMode>>` is non-serialisable); AI clients reach the
/// driver state through the `scene/query` / `scene/intervene` /
/// `scene/invoke` triad against the [`ImmediateMode::introspect`]
/// opt-in surface (same channel `External` uses).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmediateModeSnapshot {
    pub tag: Option<String>,
    pub viewport: Rect,
    /// Substrate-published per-paint `dt` (microseconds). `0` is
    /// the sentinel for "no tick yet" (first paint, before the
    /// substrate has driven one).
    pub last_dt_micros: u64,
}

/// R972 §5.41 — `TextGrid` payload of [`SnapshotNode::TextGrid`].
///
/// `rect` mirrors `TextGridNode.rect` (the layout-resolved pixel area);
/// `cell_w` / `cell_h` mirror the node-local `CellMetric`; `cols` /
/// `rows` are the grid's derived winsize dimensions
/// ([`TextGridNode::cols`](pinion_core::scene::TextGridNode::cols) /
/// [`rows`](pinion_core::scene::TextGridNode::rows)). Together they let
/// an AI client read the cell-native coordinate space as data (§2 #7):
/// the cell↔pixel round-trip is reconstructible from `rect` + metric,
/// and `(cols, rows)` is the R969 layout-derived `(rows, cols)` SSOT.
///
/// R973 §5.41 — `grid_rows` is the cell **content** projection: one
/// [`GridRowSnapshot`] per row of the [`GridBuffer`](pinion_core::GridBuffer)
/// the grid currently shows (empty for a geometry-only grid). Cell
/// colours are resolved through the grid's
/// [`Palette`](pinion_core::Palette) here — the R969 "resolve at paint
/// time" contract (a snapshot is a paint-time readback).
///
/// R974.1 §5.41 — `buffer_cols` / `buffer_rows` are the projection's OWN
/// dimensions (the size the producer last sent), made first-class so the
/// R969 two-facts model is fully introspectable: `cols` / `rows` are the
/// layout-derived winsize the producer is *told* to size to, while
/// `buffer_cols` / `buffer_rows` are what it *last delivered*. They
/// converge at steady state; a divergence is a legitimate in-flight
/// resize (or a producer bug) an AI client can now detect directly,
/// rather than inferring the projection size from `grid_rows` lengths. A
/// geometry-only grid reports `buffer_cols == buffer_rows == 0`.
///
/// R975 §5.41 — `cursor` is the grid's [`GridCursorSnapshot`]: a grid has
/// exactly one cursor (position / shape / visibility), always reported.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextGridSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub cell_w: u32,
    pub cell_h: u32,
    pub cols: u16,
    pub rows: u16,
    pub buffer_cols: u16,
    pub buffer_rows: u16,
    pub grid_rows: Vec<GridRowSnapshot>,
    pub cursor: GridCursorSnapshot,
}

/// R973 §5.41 — one row of a [`TextGridSnapshot`] cell projection: the
/// row's `text` (cell grapheme clusters joined left→right) plus its
/// style `runs` (maximal spans of one foreground / background colour —
/// the terminal-canonical run-length style compression).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridRowSnapshot {
    pub text: String,
    pub runs: Vec<GridStyleRun>,
}

/// R973 §5.41 — a maximal run of consecutive cells in a row sharing one
/// `(fg, bg, attrs, width)` tuple, `[start, start + len)` in columns. R974
/// added `attrs` to the run key (an SGR change starts a new run); R976
/// adds `width` (`"narrow"` / `"wide"` / `"trailer"`), so a wide-cluster
/// head and its continuation trailer are each their own run and a client
/// can map glyphs to columns unambiguously even where wide and narrow
/// cells mix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridStyleRun {
    pub start: u16,
    pub len: u16,
    pub fg: TermColorSnapshot,
    pub bg: TermColorSnapshot,
    pub attrs: CellAttrs,
    pub width: &'static str,
}

/// R973 §5.41 — a cell colour in a snapshot. Reports both the *stored*
/// form (`kind` `"default"` / `"indexed"` / `"rgb"` plus the palette
/// `index` for indexed) **and** the palette-resolved 24-bit `rgb` hex,
/// so an AI client reads the cell's semantic colour intent without OCR
/// (§2 #7).
///
/// R974.1 — `rgb` is the palette-resolved colour of the **stored** slot
/// (this cell's fg or bg), *before* any paint-time SGR transform. To
/// derive the colour a painter actually draws, an AI client applies the
/// run's [`attrs`](GridStyleRun::attrs): a reversed run swaps the
/// effective fg / bg (and `dim` / `bold` may shift intensity). The
/// snapshot deliberately does NOT pre-apply the swap — that would lose
/// the stored intent and break the resolve-at-paint-only contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermColorSnapshot {
    pub kind: &'static str,
    pub index: Option<u8>,
    pub rgb: String,
}

/// R975 §5.41 — the grid cursor in a snapshot: its `(col, row)` cell
/// position, its `shape` (`"block"` / `"bar"` / `"underline"`), and
/// whether it is `visible`. Always present — a grid has exactly one
/// cursor; a geometry-only grid (or one whose producer has not set a
/// cursor) reports the default: hidden, home `(0, 0)`, block. Compare
/// `(col, row)` against the grid's `(cols, rows)` to tell whether the
/// cursor sits in bounds — both are first-class facts (R974.1 dual-fact
/// discipline), so no in-bounds flag is pre-computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCursorSnapshot {
    pub col: u16,
    pub row: u16,
    pub shape: &'static str,
    pub visible: bool,
}

/// `External` payload of [`SnapshotNode::External`] (R51.198 added
/// `rect` + `tag`).
///
/// `rect` mirrors `ExternalNode.rect`; `tag` mirrors `ExternalNode.tag`
/// (the §5.20 intent-routing handle used by widgets like
/// `main_toggle`). `introspect` is `Some(fields)` when the `External`
/// opted in to §5.15 item 8 and reported a schema; `None` otherwise.
/// Order matches the schema's declared field order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub introspect: Option<Vec<(String, IntrospectValue)>>,
}

/// Reasons [`snapshot`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path is not v0-supported (must be empty after window
    /// prefix).
    UnsupportedPath,
}

impl From<PathError> for SnapshotError {
    fn from(err: PathError) -> Self {
        SnapshotError::Path(err)
    }
}

/// Build a snapshot of `scene` at the resolved window root.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the window prefix is malformed or the
/// scene path carries an unsupported tail.
pub fn snapshot(scene: &Scene, raw_path: &str) -> Result<SnapshotNode, SnapshotError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // v0: scene path must be empty. Tails like /external/count belong
    // to scene/query, not scene/snapshot.
    if !resolved.scene_path.is_empty() {
        return Err(SnapshotError::UnsupportedPath);
    }

    Ok(snapshot_root(scene))
}

fn cow_to_owned(tag: Option<&std::borrow::Cow<'static, str>>) -> Option<String> {
    tag.map(|t| t.as_ref().to_string())
}

fn snapshot_root(scene: &Scene) -> SnapshotNode {
    match scene {
        Scene::Box(node) => SnapshotNode::Box(BoxSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            style: node.style.clone(),
        }),
        Scene::Text(node) => SnapshotNode::Text(TextSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            content: node.content.clone(),
            style: node.style.clone(),
            runs: node.runs.clone(),
        }),
        Scene::Path(node) => SnapshotNode::Path(PathSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            commands: node.commands.clone(),
            style: node.style.clone(),
        }),
        Scene::Image(node) => SnapshotNode::Image(ImageSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            source: node.source.clone(),
            style: node.style,
        }),
        Scene::Container(node) => SnapshotNode::Container(ContainerSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            style: node.style.clone(),
            children: node.children.iter().map(snapshot_root).collect(),
        }),
        Scene::Effect(_) => SnapshotNode::Effect,
        Scene::External(node) => {
            let introspect = node.handle.introspect().map(|intro| {
                intro
                    .schema()
                    .fields
                    .iter()
                    .filter_map(|(name, _ty)| intro.query(name).map(|v| ((*name).to_string(), v)))
                    .collect()
            });
            SnapshotNode::External(ExternalSnapshot {
                rect: node.rect,
                tag: cow_to_owned(node.tag.as_ref()),
                introspect,
            })
        }
        Scene::Scroll(node) => SnapshotNode::Scroll(ScrollSnapshot {
            tag: cow_to_owned(node.tag.as_ref()),
            viewport: node.viewport,
            offset_x: node.offset_x,
            offset_y: node.offset_y,
            // R784 — wire form of the scroll axis (vertical default). The
            // spelling is owned by ScrollAxis (R785.1), not re-spelled here.
            axis: node.axis.as_wire_name(),
            content: Box::new(snapshot_root(node.content.as_ref())),
        }),
        Scene::ImmediateModeNode(node) => {
            // `Duration → u64 microseconds` clamps at u64::MAX after
            // ~584,554 years of accumulated wall-clock — well past
            // any realistic per-frame delta. The substrate clamps
            // `dt` against the §5.28 `MAX_FRAME_DT_SECS` ceiling
            // (~33 ms) before reaching the immediate-mode tick, so
            // the realistic range is `[0, 33333]` micros.
            let micros = u64::try_from(node.last_dt().as_micros())
                .unwrap_or(u64::MAX);
            SnapshotNode::ImmediateModeNode(ImmediateModeSnapshot {
                tag: cow_to_owned(node.tag.as_ref()),
                viewport: node.viewport,
                last_dt_micros: micros,
            })
        }
        // R972 §5.41 — text-grid geometry as scene-as-data. `cols` /
        // `rows` are read through the node's derivation (the metric's
        // winsize floor) so the snapshot reports the authoritative
        // dimensions, not a re-derived copy.
        Scene::TextGrid(node) => {
            let metric = node.cell_metric();
            SnapshotNode::TextGrid(TextGridSnapshot {
                rect: node.rect,
                tag: cow_to_owned(node.tag.as_ref()),
                cell_w: metric.cell_w(),
                cell_h: metric.cell_h(),
                cols: node.cols(),
                rows: node.rows(),
                buffer_cols: node.cells().cols(),
                buffer_rows: node.cells().rows(),
                grid_rows: text_grid_rows(node.cells(), &node.palette()),
                cursor: grid_cursor_snapshot(node.cells().cursor()),
            })
        }
        // `Scene` is non_exhaustive; future variants surface as Unknown
        // until this dispatcher is updated for them.
        _ => SnapshotNode::Unknown,
    }
}

/// R973 §5.41 — project a [`GridBuffer`] into per-row snapshots, resolving
/// each cell's colours through `palette` (the R969 "resolve at paint
/// time" contract — a snapshot is a paint-time readback). Consecutive
/// cells with an identical `(fg, bg, attrs, width)` tuple collapse into
/// one [`GridStyleRun`] (run-length style compression). A
/// [`CellWidth::Trailer`] carries the empty cluster, so a wide cluster
/// contributes its glyph to `text` exactly once even across its two
/// columns (R976). An empty buffer yields no rows.
fn text_grid_rows(buffer: &GridBuffer, palette: &Palette) -> Vec<GridRowSnapshot> {
    (0..buffer.rows())
        .map(|row| {
            let mut text = String::new();
            let mut runs: Vec<GridStyleRun> = Vec::new();
            let mut prev: Option<(TermColor, TermColor, CellAttrs, CellWidth)> = None;
            for col in 0..buffer.cols() {
                // Every coordinate in `0..cols × 0..rows` is in bounds.
                let cell = buffer.cell(col, row).expect("cell within buffer bounds");
                text.push_str(&cell.cluster);
                let style = (cell.fg, cell.bg, cell.attrs, cell.width);
                match runs.last_mut() {
                    Some(run) if prev == Some(style) => run.len += 1,
                    _ => runs.push(GridStyleRun {
                        start: col,
                        len: 1,
                        fg: term_color_snapshot(cell.fg, ColorTarget::Foreground, palette),
                        bg: term_color_snapshot(cell.bg, ColorTarget::Background, palette),
                        attrs: cell.attrs,
                        width: cell_width_wire(cell.width),
                    }),
                }
                prev = Some(style);
            }
            GridRowSnapshot { text, runs }
        })
        .collect()
}

/// R976 §5.41 — the wire string for a cell's [`CellWidth`] role, used as
/// the [`GridStyleRun::width`] discriminator (mirroring how
/// [`grid_cursor_snapshot`] maps a [`CursorShape`]).
fn cell_width_wire(width: CellWidth) -> &'static str {
    match width {
        CellWidth::Narrow => "narrow",
        CellWidth::Wide => "wide",
        CellWidth::Trailer => "trailer",
    }
}

/// R973 §5.41 — capture a cell's stored [`TermColor`] plus its
/// palette-resolved 24-bit hex. The stored `kind` / `index` preserve the
/// producer's semantic intent; `rgb` is the resolved colour of the
/// stored slot (R974.1 — *before* any paint-time SGR transform such as a
/// `reverse` fg/bg swap, which a client applies from the run's `attrs`).
fn term_color_snapshot(
    color: TermColor,
    target: ColorTarget,
    palette: &Palette,
) -> TermColorSnapshot {
    let rgb = palette.resolve(color, target).to_hex();
    let (kind, index) = match color {
        TermColor::Default => ("default", None),
        TermColor::Indexed(index) => ("indexed", Some(index)),
        TermColor::Rgb(_) => ("rgb", None),
    };
    TermColorSnapshot { kind, index, rgb }
}

/// R975 §5.41 — capture a grid's [`GridCursor`] as a snapshot: the cursor
/// [`shape`](pinion_core::CursorShape) becomes its wire-string
/// discriminator (mirroring [`TermColorSnapshot`]'s `kind`); position and
/// visibility pass through verbatim.
fn grid_cursor_snapshot(cursor: GridCursor) -> GridCursorSnapshot {
    let shape = match cursor.shape {
        CursorShape::Block => "block",
        CursorShape::Bar => "bar",
        CursorShape::Underline => "underline",
    };
    GridCursorSnapshot {
        col: cursor.col,
        row: cursor.row,
        shape,
        visible: cursor.visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn box_root_snapshots_as_box() {
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        match snapshot(&scene, "").unwrap() {
            SnapshotNode::Box(snap) => {
                assert_eq!(snap.rect, Rect::default());
                assert!(snap.tag.is_none());
            }
            other => panic!("expected Box, got {other:?}"),
        }
    }

    #[test]
    fn text_grid_root_snapshots_with_layout_derived_dims() {
        // R972 §5.41 — the snapshot reports the node-local metric and the
        // winsize `(cols, rows)` derived from the layout-resolved rect.
        let mut node = pinion_core::scene::TextGridNode::new(pinion_core::CellMetric::DEFAULT);
        node.rect = Rect::new(0, 0, 640, 384); // 80 × 24 @ 8×16
        node.tag = Some("grid".into());
        let scene = Scene::TextGrid(node);
        match snapshot(&scene, "").unwrap() {
            SnapshotNode::TextGrid(snap) => {
                assert_eq!(snap.rect, Rect::new(0, 0, 640, 384));
                assert_eq!(snap.tag.as_deref(), Some("grid"));
                assert_eq!(snap.cell_w, 8);
                assert_eq!(snap.cell_h, 16);
                assert_eq!(snap.cols, 80);
                assert_eq!(snap.rows, 24);
                // R974.1 — a geometry-only grid has no projection yet: the
                // requested winsize is 80×24 but the received buffer is 0×0.
                assert_eq!((snap.buffer_cols, snap.buffer_rows), (0, 0));
                assert!(snap.grid_rows.is_empty());
                // R975 — a geometry-only grid reports the default cursor:
                // hidden, home, block (no producer has set one).
                assert_eq!((snap.cursor.col, snap.cursor.row), (0, 0));
                assert_eq!(snap.cursor.shape, "block");
                assert!(!snap.cursor.visible);
            }
            other => panic!("expected TextGrid, got {other:?}"),
        }
    }

    #[test]
    fn text_grid_snapshot_marks_wide_and_trailer_runs() {
        // R976 §5.41 — a wide head, its trailer, and a narrow cell each
        // become their own run; the wide cluster contributes one glyph and
        // the trailer none, so `text` has two glyphs over three columns.
        use pinion_core::{GridBuffer, TermCell, TermColor};
        let shi = "\u{4e16}"; // 世 — East Asian Wide
        let head = TermCell::new(shi, TermColor::Indexed(1), TermColor::Default).wide();
        let buf = GridBuffer::new(3, 1).with_row(
            0,
            [
                head.clone(),
                head.trailer(),
                TermCell::new("x", TermColor::Default, TermColor::Default),
            ],
        );
        let mut node = pinion_core::scene::TextGridNode::new(pinion_core::CellMetric::DEFAULT)
            .with_cells(buf);
        node.rect = Rect::new(0, 0, 24, 16); // 3 × 1 @ 8×16
        match snapshot(&Scene::TextGrid(node), "").unwrap() {
            SnapshotNode::TextGrid(snap) => {
                let row = &snap.grid_rows[0];
                assert_eq!(row.text, format!("{shi}x"), "wide glyph once, then narrow");
                assert_eq!(row.runs.len(), 3, "head / trailer / narrow are distinct runs");
                assert_eq!((row.runs[0].start, row.runs[0].len), (0, 1));
                assert_eq!(row.runs[0].width, "wide");
                assert_eq!(row.runs[0].fg.index, Some(1)); // head keeps its colour
                assert_eq!((row.runs[1].start, row.runs[1].len), (1, 1));
                assert_eq!(row.runs[1].width, "trailer");
                // The trailer carries the head's colour (bg paints across).
                assert_eq!(row.runs[1].fg.index, Some(1));
                assert_eq!(row.runs[2].width, "narrow");
            }
            other => panic!("expected TextGrid, got {other:?}"),
        }
    }

    #[test]
    fn text_grid_snapshot_reports_explicit_cursor() {
        // R975 §5.41 — a producer-set cursor is reported verbatim: cell
        // position, the shape's wire string, and visibility.
        use pinion_core::{CursorShape, GridBuffer, GridCursor};
        let buf = GridBuffer::new(8, 2).with_cursor(GridCursor::new(2, 1, CursorShape::Bar, true));
        let mut node = pinion_core::scene::TextGridNode::new(pinion_core::CellMetric::DEFAULT)
            .with_cells(buf);
        node.rect = Rect::new(0, 0, 64, 32); // 8 × 2 @ 8×16
        match snapshot(&Scene::TextGrid(node), "").unwrap() {
            SnapshotNode::TextGrid(snap) => {
                assert_eq!((snap.cursor.col, snap.cursor.row), (2, 1));
                assert_eq!(snap.cursor.shape, "bar");
                assert!(snap.cursor.visible);
                // The cursor (col 2, row 1) sits within the 8×2 grid — a
                // client derives that from the two first-class facts.
                assert!(snap.cursor.col < snap.cols && snap.cursor.row < snap.rows);
            }
            other => panic!("expected TextGrid, got {other:?}"),
        }
    }

    #[test]
    fn text_grid_snapshot_coalesces_style_runs() {
        // R974.1 — the RLE run key is (fg, bg, attrs): adjacent cells with
        // an identical triple collapse into one run; a change in any of the
        // three — OR non-adjacency — starts a new run.
        use pinion_core::{CellAttrs, GridBuffer, TermCell, TermColor};
        let red = || TermCell::new("x", TermColor::Indexed(1), TermColor::Default);
        let buf = GridBuffer::new(5, 1).with_row(
            0,
            [
                red(),                                                          // col0 fg1
                red(),                                                          // col1 fg1 (coalesce)
                TermCell::new("y", TermColor::Indexed(2), TermColor::Default),  // col2 fg2 (new run)
                red(),                                                          // col3 fg1 (new run: non-adjacent)
                red().with_attrs(CellAttrs::empty().with_bold(true)),           // col4 fg1+bold (new run: attrs)
            ],
        );
        let mut node = pinion_core::scene::TextGridNode::new(pinion_core::CellMetric::DEFAULT)
            .with_cells(buf);
        node.rect = Rect::new(0, 0, 40, 16); // 5 × 1 @ 8×16
        match snapshot(&Scene::TextGrid(node), "").unwrap() {
            SnapshotNode::TextGrid(snap) => {
                assert_eq!((snap.buffer_cols, snap.buffer_rows), (5, 1));
                assert_eq!(snap.grid_rows.len(), 1);
                let runs = &snap.grid_rows[0].runs;
                // [A,A,B,A,A+bold] -> 4 runs.
                assert_eq!(runs.len(), 4, "coalesce adjacent; split on colour/attrs/gap");
                assert_eq!((runs[0].start, runs[0].len), (0, 2)); // A,A merged
                assert_eq!((runs[1].start, runs[1].len), (2, 1)); // B
                assert_eq!((runs[2].start, runs[2].len), (3, 1)); // A again (not merged with run 0)
                assert_eq!((runs[3].start, runs[3].len), (4, 1)); // A+bold (attrs break)
                assert!(runs[3].attrs.bold && !runs[2].attrs.bold);
                // R976 — narrow cells report the narrow width role.
                assert!(runs.iter().all(|r| r.width == "narrow"));
            }
            other => panic!("expected TextGrid, got {other:?}"),
        }
    }

    #[test]
    fn counted_external_dumps_introspect_fields() {
        let scene = counted_scene(42);
        let snap = snapshot(&scene, "").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
            }) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "count");
                assert_eq!(fields[0].1, IntrospectValue::Int(42));
            }
            other => panic!("expected External with introspect Some, got {other:?}"),
        }
    }

    #[test]
    fn stub_external_introspect_is_none() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        match snapshot(&scene, "").unwrap() {
            SnapshotNode::External(snap) => {
                assert!(snap.introspect.is_none());
            }
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn window_prefix_short_circuits() {
        let scene = counted_scene(0);
        let snap = snapshot(&scene, "/window[main]").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
            }) => assert_eq!(fields[0].1, IntrospectValue::Int(0)),
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn scene_path_tail_is_unsupported() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/external/count").unwrap_err();
        assert_eq!(err, SnapshotError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/window[main").unwrap_err();
        assert_eq!(err, SnapshotError::Path(PathError::MalformedPrefix));
    }

    mod r51_194 {
        use super::*;
        use pinion_core::scene::{ContainerNode, ScrollNode, TextNode};

        fn leaf_box(tag: Option<&'static str>) -> Scene {
            let node = BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default());
            if let Some(t) = tag {
                Scene::Box(node.with_tag(t))
            } else {
                Scene::Box(node)
            }
        }

        #[test]
        fn container_root_dumps_tag_and_children() {
            let scene = Scene::Container(
                ContainerNode::new(vec![leaf_box(None), leaf_box(Some("inner"))])
                    .with_tag("root"),
            );
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(snap) => {
                    assert_eq!(snap.tag.as_deref(), Some("root"));
                    assert_eq!(snap.children.len(), 2);
                    matches_box(&snap.children[0], None);
                    matches_box(&snap.children[1], Some("inner"));
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn untagged_container_reports_none_tag() {
            let scene = Scene::Container(ContainerNode::new(vec![]));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(snap) => {
                    assert!(snap.tag.is_none());
                    assert!(snap.children.is_empty());
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn nested_container_recurses_depth_first() {
            let inner = Scene::Container(ContainerNode::new(vec![leaf_box(None)]).with_tag("inner"));
            let scene = Scene::Container(ContainerNode::new(vec![inner]).with_tag("outer"));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(outer) => {
                    assert_eq!(outer.tag.as_deref(), Some("outer"));
                    assert_eq!(outer.children.len(), 1);
                    match &outer.children[0] {
                        SnapshotNode::Container(inner) => {
                            assert_eq!(inner.tag.as_deref(), Some("inner"));
                            assert_eq!(inner.children.len(), 1);
                            matches_box(&inner.children[0], None);
                        }
                        other => panic!("expected nested Container, got {other:?}"),
                    }
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn scroll_root_dumps_viewport_offset_tag_content() {
            let content = Scene::Text(TextNode::new("row".to_string(), Rect::new(0, 0, 50, 200)));
            let scroll = ScrollNode::new(Rect::new(0, 0, 50, 80), content)
                .with_tag("listbox_scroll");
            let scene = Scene::Scroll(scroll.with_offset(0, 120));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Scroll(snap) => {
                    assert_eq!(snap.tag.as_deref(), Some("listbox_scroll"));
                    assert_eq!(snap.viewport, Rect::new(0, 0, 50, 80));
                    assert_eq!(snap.offset_x, 0);
                    assert_eq!(snap.offset_y, 120);
                    assert_eq!(snap.axis, "vertical", "default scroll axis is vertical");
                    matches_text(&snap.content, "row", Rect::new(0, 0, 50, 200));
                }
                other => panic!("expected Scroll, got {other:?}"),
            }
        }

        #[test]
        fn horizontal_scroll_reports_axis_horizontal() {
            // R784 — a `ScrollAxis::Horizontal` container surfaces
            // `axis = "horizontal"` so AI clients see which axis scrolls.
            use pinion_core::scene::ScrollAxis;
            let content = Scene::Text(TextNode::new("wide".to_string(), Rect::new(0, 0, 500, 40)));
            let scroll = ScrollNode::new(Rect::new(0, 0, 200, 40), content)
                .with_axis(ScrollAxis::Horizontal)
                .with_tag("grid_h")
                .with_offset(80, 0);
            match snapshot(&Scene::Scroll(scroll), "").unwrap() {
                SnapshotNode::Scroll(snap) => {
                    assert_eq!(snap.axis, "horizontal", "horizontal scroll surfaces its axis");
                    assert_eq!(snap.offset_x, 80);
                    assert_eq!(snap.offset_y, 0);
                }
                other => panic!("expected Scroll, got {other:?}"),
            }
        }

        #[test]
        fn untagged_scroll_reports_none_tag_and_zero_offset() {
            let content = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
            let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 10, 10), content));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Scroll(snap) => {
                    assert!(snap.tag.is_none());
                    assert_eq!(snap.offset_x, 0);
                    assert_eq!(snap.offset_y, 0);
                }
                other => panic!("expected Scroll, got {other:?}"),
            }
        }

        #[test]
        fn scroll_inside_container_traverses_both_layers() {
            let row = leaf_box(Some("row"));
            let scroll = ScrollNode::new(
                Rect::new(0, 0, 50, 80),
                Scene::Container(ContainerNode::new(vec![row]).with_tag("rows")),
            )
            .with_tag("scroll");
            let scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_tag("root"),
            );
            let SnapshotNode::Container(outer) = snapshot(&scene, "").unwrap() else {
                panic!("expected outer Container");
            };
            assert_eq!(outer.tag.as_deref(), Some("root"));
            let SnapshotNode::Scroll(scroll_snap) = &outer.children[0] else {
                panic!("expected Scroll inside Container");
            };
            assert_eq!(scroll_snap.tag.as_deref(), Some("scroll"));
            let SnapshotNode::Container(rows) = &*scroll_snap.content else {
                panic!("expected Container inside Scroll content");
            };
            assert_eq!(rows.tag.as_deref(), Some("rows"));
            assert_eq!(rows.children.len(), 1);
            matches_box(&rows.children[0], Some("row"));
        }

        fn matches_box(node: &SnapshotNode, tag: Option<&str>) {
            match node {
                SnapshotNode::Box(snap) => {
                    assert_eq!(snap.tag.as_deref(), tag);
                }
                other => panic!("expected Box, got {other:?}"),
            }
        }

        fn matches_text(node: &SnapshotNode, content: &str, rect: Rect) {
            match node {
                SnapshotNode::Text(snap) => {
                    assert_eq!(snap.content, content);
                    assert_eq!(snap.rect, rect);
                }
                other => panic!("expected Text, got {other:?}"),
            }
        }
    }

    mod r51_198 {
        //! Leaf primitive `rect` + `tag` exposure tests.
        //!
        //! Each Scene primitive surfaces its geometry and intent-routing
        //! tag through the new `SnapshotNode` payload structs, so demos
        //! can locate widgets by tag and derive click / scroll
        //! coordinates from the snapshot instead of hardcoding pixels.

        use super::*;
        use pinion_core::scene::{
            ContainerNode, ImageNode, PathCommand, PathNode, PathPoint, TextNode,
        };
        use pinion_core::style::PathStyle;

        #[test]
        fn box_carries_rect_and_tag() {
            let node = BoxNode::filled(Rect::new(10, 20, 30, 40), Color::default())
                .with_tag("box_tag");
            let scene = Scene::Box(node);
            let SnapshotNode::Box(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Box");
            };
            assert_eq!(snap.rect, Rect::new(10, 20, 30, 40));
            assert_eq!(snap.tag.as_deref(), Some("box_tag"));
        }

        #[test]
        fn text_carries_rect_tag_and_content() {
            let node = TextNode::new("hello".to_string(), Rect::new(5, 6, 50, 14))
                .with_tag("greeting");
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert_eq!(snap.rect, Rect::new(5, 6, 50, 14));
            assert_eq!(snap.tag.as_deref(), Some("greeting"));
            assert_eq!(snap.content, "hello");
        }

        #[test]
        fn untagged_text_reports_none_tag() {
            let node = TextNode::new(String::new(), Rect::default());
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert!(snap.tag.is_none());
            assert_eq!(snap.content, "");
        }

        #[test]
        fn path_carries_rect_tag_and_commands() {
            let commands = vec![
                PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
                PathCommand::LineTo(PathPoint::new(50.0, 0.0)),
                PathCommand::Close,
            ];
            let node = PathNode::new(
                Rect::new(0, 0, 100, 100),
                commands.clone(),
                PathStyle::default(),
            )
            .with_tag("chevron");
            let scene = Scene::Path(node);
            let SnapshotNode::Path(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Path");
            };
            assert_eq!(snap.rect, Rect::new(0, 0, 100, 100));
            assert_eq!(snap.tag.as_deref(), Some("chevron"));
            assert_eq!(snap.commands, commands);
        }

        #[test]
        fn image_carries_rect_tag_and_source() {
            let node = ImageNode::new("icon.png", Rect::new(8, 8, 16, 16))
                .with_tag("logo");
            let scene = Scene::Image(node);
            let SnapshotNode::Image(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Image");
            };
            assert_eq!(snap.rect, Rect::new(8, 8, 16, 16));
            assert_eq!(snap.tag.as_deref(), Some("logo"));
            assert_eq!(snap.source, "icon.png");
        }

        #[test]
        fn container_carries_rect_alongside_tag_and_children() {
            // `ContainerNode.rect` is layout-derived in production, so
            // the test mutates the field directly after the builder
            // chain — the snapshot pipeline reads whichever value the
            // node currently carries.
            let mut node = ContainerNode::new(vec![]).with_tag("hello_toggle_root");
            node.rect = Rect::new(0, 0, 360, 220);
            let scene = Scene::Container(node);
            let SnapshotNode::Container(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Container");
            };
            assert_eq!(snap.rect, Rect::new(0, 0, 360, 220));
            assert_eq!(snap.tag.as_deref(), Some("hello_toggle_root"));
            assert!(snap.children.is_empty());
        }

        #[test]
        fn external_carries_rect_and_tag_alongside_introspect() {
            let mut node =
                ExternalNode::new(Box::new(CountedExternal::new(5))).with_tag("main_toggle");
            node.rect = Rect::new(100, 50, 64, 32);
            let scene = Scene::External(node);
            let SnapshotNode::External(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected External");
            };
            assert_eq!(snap.rect, Rect::new(100, 50, 64, 32));
            assert_eq!(snap.tag.as_deref(), Some("main_toggle"));
            let fields = snap.introspect.expect("CountedExternal opts into introspect");
            assert_eq!(fields[0].0, "count");
            assert_eq!(fields[0].1, IntrospectValue::Int(5));
        }
    }

    mod r55_g8 {
        //! R55.G.8 §5.49 — `BoxStyle` + `TextStyle` snapshot exposure.
        //!
        //! Box / Text / Container now surface their style sidecars
        //! (fill, border, corner radius, font axis) so AI clients can
        //! verify rendered chrome and typography without OCR.

        use super::*;
        use pinion_core::scene::{ContainerNode, TextNode};
        use pinion_core::style::{
            Border, BorderPlacement, BoxStyle, FontStyle, FontWeight, TextStyle,
        };

        #[test]
        fn box_carries_full_style_with_border_and_corner_radius() {
            let style = BoxStyle::filled(Color::rgba(0x11, 0x22, 0x33, 0xff))
                .with_border(
                    Border::new(Color::rgba(0xaa, 0xbb, 0xcc, 0xff), 3)
                        .with_placement(BorderPlacement::Outside),
                )
                .with_corner_radius(8);
            let node = BoxNode::filled(Rect::new(0, 0, 32, 32), Color::default());
            // BoxNode::filled sets the fill via the constructor; rewrite
            // the whole style sidecar to attach border + corner_radius
            // without going through every with_* builder permutation.
            let mut node = node;
            node.style = style;
            let scene = Scene::Box(node);
            let SnapshotNode::Box(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Box");
            };
            assert_eq!(snap.style.fill, Color::rgba(0x11, 0x22, 0x33, 0xff));
            let border = snap.style.border.expect("border preserved");
            assert_eq!(border.color, Color::rgba(0xaa, 0xbb, 0xcc, 0xff));
            assert_eq!(border.width, 3);
            assert_eq!(border.placement, BorderPlacement::Outside);
            assert_eq!(snap.style.corner_radius, 8);
        }

        #[test]
        fn text_carries_full_visual_style() {
            let style = TextStyle::new()
                .with_size_px(20)
                .with_fg(Color::rgba(0x55, 0x66, 0x77, 0xff))
                .with_weight(FontWeight::BOLD)
                .with_style(FontStyle::Italic);
            let mut node = TextNode::new("hi".to_string(), Rect::new(0, 0, 40, 20));
            node.style = style;
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert_eq!(snap.style.font_size_px, 20);
            assert_eq!(snap.style.fg_color, Color::rgba(0x55, 0x66, 0x77, 0xff));
            assert_eq!(snap.style.font_weight, FontWeight::BOLD);
            assert_eq!(snap.style.font_style, FontStyle::Italic);
        }

        #[test]
        fn path_carries_full_style_with_stroke_and_fill() {
            use pinion_core::scene::{PathCommand, PathNode, PathPoint};
            use pinion_core::style::{PathStyle, Stroke, StrokeCap};
            let mut node = PathNode::new(
                Rect::new(0, 0, 50, 50),
                vec![
                    PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
                    PathCommand::LineTo(PathPoint::new(50.0, 50.0)),
                ],
                PathStyle::default(),
            );
            node.style = PathStyle::stroked(
                Stroke::new(Color::rgba(0x11, 0x22, 0x33, 0xff), 4)
                    .with_cap(StrokeCap::Round),
            )
            .with_fill(Color::rgba(0xaa, 0xbb, 0xcc, 0xff));
            let scene = Scene::Path(node);
            let SnapshotNode::Path(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Path");
            };
            let stroke = snap.style.stroke.expect("stroke preserved");
            assert_eq!(stroke.color, Color::rgba(0x11, 0x22, 0x33, 0xff));
            assert_eq!(stroke.width, 4);
            assert_eq!(stroke.cap, StrokeCap::Round);
            assert_eq!(snap.style.fill, Some(Color::rgba(0xaa, 0xbb, 0xcc, 0xff)));
        }

        #[test]
        fn image_carries_full_style_with_fit_and_tint() {
            use pinion_core::scene::ImageNode;
            use pinion_core::style::{Fit, ImageStyle};
            let mut node = ImageNode::new(
                "asset://icon.png".to_string(),
                Rect::new(0, 0, 64, 64),
            );
            node.style = ImageStyle::default()
                .with_fit(Fit::Cover)
                .with_tint(Color::rgba(0x12, 0x34, 0x56, 0xff));
            let scene = Scene::Image(node);
            let SnapshotNode::Image(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Image");
            };
            assert_eq!(snap.style.fit, Fit::Cover);
            assert_eq!(snap.style.tint, Some(Color::rgba(0x12, 0x34, 0x56, 0xff)));
        }

        #[test]
        fn container_carries_style_alongside_children() {
            let mut node = ContainerNode::new(vec![]).with_tag("frame");
            node.style = BoxStyle::filled(Color::rgba(0x12, 0x34, 0x56, 0x78))
                .with_corner_radius(4);
            let scene = Scene::Container(node);
            let SnapshotNode::Container(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Container");
            };
            assert_eq!(snap.style.fill, Color::rgba(0x12, 0x34, 0x56, 0x78));
            assert_eq!(snap.style.corner_radius, 4);
            assert!(snap.style.border.is_none());
        }
    }

    mod r55_g10 {
        //! R55.G.10 §5.49 — `TextStyle` layout-axis snapshot exposure
        //! (`line_height` / `text_align` / `decoration` / `overflow` /
        //! `letter_spacing`).
        //!
        //! R55.G.8 covered the visual axis (size / fg / weight / style)
        //! with `text_carries_full_visual_style`. R55.G.10 wired the
        //! layout axis through `text_style_to_json` for the dispatch
        //! JSON wire. This module pins the snapshot land side so the
        //! symmetry is observable at the snapshot-struct boundary —
        //! `r55_g8` (visual) + `r55_g11` (Path/Image) + here (layout).

        use super::*;
        use pinion_core::scene::TextNode;
        use pinion_core::style::{
            LineHeight, TextAlign, TextDecoration, TextOverflow, TextStyle,
        };

        #[test]
        fn text_layout_axis_survives_snapshot() {
            // All five layout fields set to non-default values: the
            // snapshot pipeline must carry each through without
            // collapsing to the default.
            let style = TextStyle::new()
                .with_line_height(LineHeight::Px(24))
                .with_align(TextAlign::Center)
                .with_letter_spacing(-2)
                .with_decoration(TextDecoration::both())
                .with_overflow(TextOverflow::Ellipsis);
            let mut node = TextNode::new("hi".to_string(), Rect::new(0, 0, 40, 20));
            node.style = style;
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert_eq!(snap.style.line_height, LineHeight::Px(24));
            assert_eq!(snap.style.text_align, TextAlign::Center);
            assert_eq!(snap.style.letter_spacing, -2);
            assert_eq!(snap.style.decoration, TextDecoration::both());
            assert_eq!(snap.style.overflow, TextOverflow::Ellipsis);
        }

        #[test]
        fn text_letter_spacing_accepts_signed_through_snapshot() {
            // letter_spacing is `i32`; both signs survive the snapshot
            // pass intact (no `u32` widening or clamping).
            for px in [-8_i32, 0, 4] {
                let mut node =
                    TextNode::new("x".to_string(), Rect::new(0, 0, 8, 8));
                node.style = TextStyle::new().with_letter_spacing(px);
                let scene = Scene::Text(node);
                let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                    panic!("expected Text");
                };
                assert_eq!(snap.style.letter_spacing, px);
            }
        }

        #[test]
        fn text_line_height_variants_each_survive_snapshot() {
            // Each LineHeight variant is its own SnapshotNode payload
            // — the snapshot pipeline preserves variant discriminants
            // and values (Px / MultiplierX100 inner data carried).
            for lh in [
                LineHeight::Normal,
                LineHeight::Px(20),
                LineHeight::MultiplierX100(150),
            ] {
                let mut node =
                    TextNode::new("x".to_string(), Rect::new(0, 0, 8, 8));
                node.style = TextStyle::new().with_line_height(lh);
                let scene = Scene::Text(node);
                let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                    panic!("expected Text");
                };
                assert_eq!(snap.style.line_height, lh);
            }
        }
    }
}
