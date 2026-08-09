//! R1451 §5.27 §5.51 — **header section layout**: the one place a grid's
//! column *order*, *size*, and *visibility* are held together, keyed the way
//! the toolkit's header view keys them.
//!
//! ## The composition that had no home
//!
//! Every column axis was already in tree — width (R785/R786), visibility
//! (R990), sort (R778), filter (R783/R997), frozen panes (R859), and section
//! order (R1450) — but each lived in its own binding or holder, and the
//! *composition* of the first three did not exist. The consequence was not a
//! missing convenience but a wrong answer: [`ColumnWidths`] indexes widths by **screen
//! position**, so moving a column left the widths behind. The toolkit keys `sectionSize`
//! and `isSectionHidden` by the **logical** section, which is exactly why a resized column
//! in a toolkit view keeps its width when dragged elsewhere.
//!
//! `ColumnLayout` is that keying:
//!
//! - `order[visual] = logical` — the permutation, owned by an embedded
//!   [`ReorderModel`] (R743) so the drag session, the APG keyboard grab, and
//!   the move arithmetic are the proven ones rather than a fifth copy.
//! - `sizes[logical]` — held in a shared [`ColumnWidths`] (R785), which
//!   already owns the minimum-width floor and the live resize-drag wire. The
//!   layout does not copy it; it **re-keys** it, and hands the same `Rc` back
//!   ([`widths`](ColumnLayout::widths)) so a border grabber writes the one
//!   store.
//! - `hidden[logical]` — the one flag vector this module adds.
//!
//! Nothing is stored twice. A permutation lives only in the `ReorderModel`, a
//! size only in the `ColumnWidths`; every derived answer
//! ([`visible_sections`](ColumnLayout::visible_sections),
//! [`visible_widths`](ColumnLayout::visible_widths),
//! [`section_position`](ColumnLayout::section_position),
//! [`logical_index_at`](ColumnLayout::logical_index_at)) is computed from
//! those, never mirrored into a field that a forgotten write path could leave
//! stale ([[r1449-completion-model]]: a rule that both derives and writes
//! diverges on the path that forgot the write).
//!
//! ## Hidden sections keep their place (the toolkit's rule, not a simplification)
//!
//! Hiding a section does **not** remove it from the permutation — its visual
//! index survives, so showing it again puts it back where it was rather than
//! at the end. [`visible_sections`](ColumnLayout::visible_sections) is the
//! projection that drops hidden sections at paint time, and it is the only
//! place that filtering happens.
//!
//! ## The paint seam is already the right shape
//!
//! [`visible_widths`](ColumnLayout::visible_widths) returns widths in *visual*
//! order with hidden sections dropped — precisely
//! `TableData::col_widths`' contract — and
//! [`visible_sections`](ColumnLayout::visible_sections) is the source-column
//! projection a binding feeds its headers, cells, and a11y tree through. So a
//! grid composes the whole header state with no paint-layer change at all.
//!
//! ## AI clients (§2 #7 + §2 #2 — where the toolkit cannot follow)
//!
//! The toolkit persists a header as `saveState()`, an **opaque versioned
//! byte array**: an agent can round-trip it but can neither read "how wide
//! is the third column now" out of it nor author one without a live widget.
//! Here the same state is [`ColumnLayoutState`] — typed, readable field by
//! field through [`query`](ColumnLayout::query) (`state`, `sizes`, `hidden`,
//! `visible_sections`, `section_position.<logical>`, `logical_index_at.<x>`,
//! …) and writable whole through
//! [`intervene`](ColumnLayout::intervene)`("state", …)`, the restore half.
//! Section mutation is the toolkit's own vocabulary over the wire:
//! `move_section` / `swap_sections` / `resize_section` /
//! `set_section_hidden`.

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use crate::composite_tag::{require_pair, split_send_payload};
use crate::external::{
    DragPayload, DropPoint, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue,
    InvokeError, SchemaArg, SchemaField, read_only_or_unknown,
};
use crate::input::PointerWireEvent;
use crate::reactive::{Owner, Signal};
use crate::style::TextAlign;
use crate::widgets::column_widths::{ColumnWidths, DEFAULT_MAX_COL_WIDTH, DEFAULT_MIN_COL_WIDTH};
use crate::widgets::grid_sort::{grid_sort_parse, grid_sort_str};
use crate::widgets::reorder::{ReorderAxis, ReorderModel};
use crate::widgets::table::cycle_col_sort;
use crate::widgets::view_order::sort_dir_str;

/// R1451 §5.27 — a whole header layout as data: the peer of the toolkit's
/// `saveState()` / `restoreState()`, except every field is
/// readable and authorable instead of an opaque byte blob.
///
/// `order` is `order[visual] = logical`; `sizes` and `hidden` are indexed by
/// **logical** section, which is what makes the snapshot survive a reorder —
/// restoring it into a layout whose columns have since been moved puts each
/// section's size and visibility back on the section, not on the position.
///
/// `clippy::struct_excessive_bools` is intentionally suppressed, for the reason
/// [`Modifiers`](crate::input::Modifiers) suppresses it: the field set is not ours to
/// shape. This is the peer of a specific external serialisation, and `write()`
/// carries `sortIndicatorShown`, `movableSections`, `clickableSections` and `cascadingResizing` as four independent booleans. Bundling them
/// into sub-structs would make this type stop looking like the thing it is the
/// peer of, and would put a shape between `to_json` and the flat wire object it has
/// to produce.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ColumnLayoutState {
    /// The visual permutation (`order[visual] = logical`).
    pub order: Vec<usize>,
    /// Per-**logical**-section size in logical pixels.
    pub sizes: Vec<u32>,
    /// Per-**logical**-section hidden flag.
    pub hidden: Vec<bool>,
    /// R1452 — per-**logical**-section sizing policy. The toolkit's `saveState`
    /// carries the modes too; a snapshot without them (one taken before R1452)
    /// decodes as all-`Interactive`, so an older saved layout still restores.
    pub modes: Vec<SectionResizeMode>,
    /// R1491 — which section carries the sort indicator, and in which
    /// direction: the toolkit's `sortIndicatorSection()` / `sortIndicatorOrder()`, which `saveState()` carries and this snapshot
    /// did not until now. `true` is ascending. Keyed by **logical** section like
    /// `sizes` and `hidden`, which is the whole reason it belongs here: an indicator
    /// keyed by screen position points at a different column the moment one is
    /// dragged.
    pub sort_indicator: Option<(usize, bool)>,
    /// R1491 — whether the indicator is painted at all: the toolkit's `sortIndicatorShown`.
    /// Separate from *which* section carries it, because the toolkit keeps the
    /// section while a view hides the arrow, and a restore has to put both
    /// back.
    pub sort_indicator_shown: bool,
    /// R1493 — the size a section takes when nothing else determined it: the
    /// toolkit's `defaultSectionSize`, which `saveState()` carries. Scalar, not per section: it is the
    /// header's rule, and the per-section outcome is already in
    /// [`sizes`](Self::sizes).
    pub default_section_size: u32,
    /// R1493 — the resize floor: the toolkit's `minimumSectionSize`, another field `saveState()` carries
    /// and R1492 added to the header without adding here. A snapshot that
    /// restored sizes but not the bounds that shape them could hand back
    /// widths the restored header immediately re-clamps.
    pub min_section_size: u32,
    /// R1493 — the resize ceiling: the toolkit's `maximumSectionSize`, the peer of
    /// [`min_section_size`](Self::min_section_size).
    pub max_section_size: u32,
    /// R1494 — whether an interactive resize takes its space from the
    /// following sections: the toolkit's `cascadingSectionResizes`, which `saveState()` carries too. The
    /// cascade *in flight* is not here and should not be — that is gesture
    /// state, not layout state, and the toolkit does not save it either.
    pub cascading_section_resizes: bool,
    /// R1498 — whether the section painted last absorbs the leftover viewport:
    /// The toolkit's `stretchLastSection`, which `write()`
    /// serialises. It belongs here rather than in [`modes`](Self::modes)
    /// because it is keyed by position and the modes are keyed by column: a
    /// restore that replayed the modes alone would put the fill back on
    /// whichever column happened to be last when the snapshot was taken.
    pub stretch_last_section: bool,
    /// R1496 — whether the user may drag a section to a new position: the
    /// toolkit's `sectionsMovable`, which `write()` serialises as `movableSections`. A saved layout that
    /// restored the permutation but not the permission hands back an order the
    /// restored header would never have let the user reach.
    pub sections_movable: bool,
    /// R1496 — whether a press-release on a section is reported as a click:
    /// The toolkit's `sectionsClickable`, serialised as `clickableSections`. Independent of
    /// [`sections_movable`](Self::sections_movable) in the toolkit and here, which is the
    /// whole reason both are needed: a header can be sortable and pinned, or
    /// reorderable and inert.
    pub sections_clickable: bool,
    /// R1496 — how many rows a `ResizeToContents` consumer should measure: The toolkit's `resizeContentsPrecision`,
    /// which `saveState()` carries. R1454 put it on the header and did not put it here,
    /// so a restore replayed every content-fitted width while dropping the
    /// sampling bound that produced them — the same omission R1493 found for
    /// the size bounds.
    pub resize_contents_precision: usize,
    /// R1504 — where a section's label sits along the row: the toolkit's
    /// `defaultAlignment`, which `write()`
    /// serialises. Scalar, not per section, because it is the header's rule —
    /// the toolkit keeps the per-section exception in the **model**
    /// (`headerData(TextAlignmentRole)`) and its `saveState()` does not carry
    /// it, so neither does this. A snapshot is a header's state, and a model's
    /// answers are not the header's to replay.
    ///
    /// Only the horizontal axis. The toolkit bundles both into one `Alignment` flag
    /// word; pinion's [`TextAlign`] is the CSS split, where the cross-axis placement
    /// is a layout property rather than a text one. The consequence is stated
    /// rather than hidden: a toolkit `AlignVCenter` has no counterpart here and none is
    /// invented.
    pub default_alignment: TextAlign,
    /// R1510 — whether a section the selection reaches is highlighted: the
    /// toolkit's `highlightSections`, which `write()` serialises as `highlightSelected`.
    ///
    /// The RULE is here; the selection is not. The toolkit's header holds a
    /// pointer to the view's selection model and only ever reads it, so a
    /// header snapshot carries the permission to highlight and never the thing
    /// highlighted — the same division R1504 drew between this header's
    /// [`default_alignment`](Self::default_alignment) and the model's per-section exceptions.
    /// Restoring a layout into a view whose selection has moved on must not
    /// put the old selection back.
    pub highlight_sections: bool,
}

impl ColumnLayoutState {
    /// The JSON object form `query("state")` hands out and
    /// `intervene("state", …)` takes back — the two are inverses, so a client
    /// reads a layout, stores it, and writes it back verbatim.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "order": self.order,
            "sizes": self.sizes,
            "hidden": self.hidden,
            "modes": self.modes.iter().map(|m| m.as_wire()).collect::<Vec<_>>(),
            // R1491 — the same `"none"` / `"<logical>:<dir>"` string the grid
            // sort proxy speaks, so a client that already reads one sort state
            // does not learn a second vocabulary for the header's.
            "sort_indicator": grid_sort_str(self.sort_indicator),
            "sort_indicator_shown": self.sort_indicator_shown,
            // R1493 — the three scalars that shape every size in `sizes`.
            // Without them a restore replays the outcomes and drops the rule
            // that produced them, so the next resize in the restored header
            // obeys a different one.
            "default_section_size": self.default_section_size,
            "min_section_size": self.min_section_size,
            "max_section_size": self.max_section_size,
            "cascading_section_resizes": self.cascading_section_resizes,
            // R1498 — the other layout rule the toolkit's `saveState()` carries. Without
            // it a restore replays sizes that were never painted: under this
            // rule the last section's stored width is not its width.
            "stretch_last_section": self.stretch_last_section,
            // R1496 — the two permissions and the sampling bound the toolkit's
            // `saveState()` carries and this snapshot did not. Without them a
            // restore hands back a permutation the restored header may forbid,
            // and content widths measured under a bound it no longer has.
            "sections_movable": self.sections_movable,
            "sections_clickable": self.sections_clickable,
            "resize_contents_precision": self.resize_contents_precision,
            // R1504 — the label rule. Spelled the way `TextAlign` is spelled
            // everywhere else on the wire (`"Start"`, not `"start"`), which
            // differs from the lower-case vocabulary `modes` above uses: the
            // type owns its spelling, and this one had two live consumers
            // before this header wanted it.
            "default_alignment": self.default_alignment.as_wire(),
            // R1510 — the highlight rule. The toolkit serialises this one and
            // not the selection that satisfies it, because the selection is
            // the view's.
            "highlight_sections": self.highlight_sections,
        })
    }

    /// Decode the [`to_json`](Self::to_json) shape. `None` when a field is
    /// missing or is not an array of the right primitive — a *shape* error,
    /// which the wire maps to `TypeMismatch`, as distinct from a well-shaped
    /// state that is not a valid layout (`OutOfRange`, decided by
    /// [`ColumnLayout::restore_state`]).
    #[must_use]
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        fn usizes(v: Option<&serde_json::Value>) -> Option<Vec<usize>> {
            v?.as_array()?
                .iter()
                .map(|x| usize::try_from(x.as_u64()?).ok())
                .collect()
        }
        let order = usizes(value.get("order"))?;
        let sizes: Vec<u32> = value
            .get("sizes")?
            .as_array()?
            .iter()
            .map(|x| u32::try_from(x.as_u64()?).ok())
            .collect::<Option<_>>()?;
        let hidden: Vec<bool> = value
            .get("hidden")?
            .as_array()?
            .iter()
            .map(serde_json::Value::as_bool)
            .collect::<Option<_>>()?;
        // R1452 — absent `modes` is the pre-R1452 snapshot shape and decodes as
        // all-`Interactive`; PRESENT but malformed is still an error, so a
        // client that meant to set a mode and misspelled it is told so.
        let modes: Vec<SectionResizeMode> = match value.get("modes") {
            None => vec![SectionResizeMode::default(); hidden.len()],
            Some(v) => v
                .as_array()?
                .iter()
                .map(|m| m.as_str()?.parse().ok())
                .collect::<Option<_>>()?,
        };
        // R1491 — same rule as `modes`: absent is the older snapshot shape and
        // decodes as "no indicator, not shown"; present but not the grammar is
        // an error. `grid_sort_parse` exists to keep those apart — the lenient
        // `grid_sort_from_str` would restore a misspelled direction as
        // *unsorted* and report success.
        let (sort_indicator, sort_indicator_shown) = match value.get("sort_indicator") {
            None => (None, false),
            Some(v) => (
                grid_sort_parse(v.as_str()?)?,
                // Shown travels with the section: a snapshot old enough to lack
                // the indicator lacks this too, and one that carries an
                // indicator defaults to painting it.
                match value.get("sort_indicator_shown") {
                    None => true,
                    Some(b) => b.as_bool()?,
                },
            ),
        };
        // R1493 — same absent-is-the-older-shape rule the three fields above
        // use: a snapshot taken before this round carries no bounds and no
        // default, and decodes to the constants rather than to zero. Present
        // but not a `u32` is still an error.
        let scalar = |key: &str, fallback: u32| -> Option<u32> {
            match value.get(key) {
                None => Some(fallback),
                Some(v) => u32::try_from(v.as_u64()?).ok(),
            }
        };
        Some(Self {
            order,
            sizes,
            hidden,
            modes,
            sort_indicator,
            sort_indicator_shown,
            default_section_size: scalar("default_section_size", DEFAULT_SECTION_SIZE)?,
            min_section_size: scalar("min_section_size", DEFAULT_MIN_COL_WIDTH)?,
            max_section_size: scalar("max_section_size", DEFAULT_MAX_COL_WIDTH)?,
            // R1494 — same absent-is-the-older-shape rule; the toolkit's own
            // default is `false`, so an older snapshot decodes to a header that
            // does not cascade.
            cascading_section_resizes: match value.get("cascading_section_resizes") {
                None => false,
                Some(v) => v.as_bool()?,
            },
            // R1498 — absent decodes to `false`, and here the toolkit's default
            // and the older header AGREE, unlike the two permissions below.
            // Measured before the round: the pre-R1498 header left 70px of its
            // 640-wide viewport unpainted, so "did not fill" is what an older
            // snapshot describes as well as what the toolkit starts at.
            stretch_last_section: match value.get("stretch_last_section") {
                None => false,
                Some(v) => v.as_bool()?,
            },
            // R1496 — absent decodes to **`true`**, which is deliberately NOT the
            // construction default. The other absent-field fallbacks above all
            // name a toolkit default because that is also what the older
            // header did; here the two diverge. A pre-R1496 header had no such
            // rule and was unconditionally movable and clickable — measured
            // over the wire before the round — so `true` is what the snapshot
            // describes. Decoding it as the toolkit's `false` would silently strip
            // interaction from every layout saved before this round.
            sections_movable: match value.get("sections_movable") {
                None => true,
                Some(v) => v.as_bool()?,
            },
            sections_clickable: match value.get("sections_clickable") {
                None => true,
                Some(v) => v.as_bool()?,
            },
            // R1496 — absent is the pre-R1454 shape and decodes to the
            // constant, like the three `scalar` fields; it is `usize` rather
            // than `u32`, which is why it does not go through that closure.
            resize_contents_precision: match value.get("resize_contents_precision") {
                None => DEFAULT_CONTENTS_PRECISION,
                Some(v) => usize::try_from(v.as_u64()?).ok()?,
            },
            // R1504 — the second field whose absent-value is deliberately NOT
            // the construction default, for the same reason `sections_movable` above is: `Start`
            // is what a pre-R1504 header PAINTED (its labels sat at a fixed
            // 12px inset from the section's left edge, measured on the real
            // paint), while a fresh header starts at the toolkit's `Center`.
            // Decoding absent as `Center` would move every label in every layout
            // saved before this round.
            //
            // An unknown spelling is a shape error like any other here, not a
            // silent `Start`: `from_wire` is the strict reader, and this
            // decoder's contract is that a well-formed field decodes or the
            // whole state is refused.
            default_alignment: match value.get("default_alignment") {
                None => TextAlign::Start,
                Some(v) => TextAlign::from_wire(v.as_str()?)?,
            },
            // R1510 — absent decodes to `false`, and here the toolkit's default
            // and the older header AGREE, as they did for `stretch_last_section` and unlike the
            // two permissions and the alignment above. Measured over the wire
            // before the round: a pre-R1510 header painted all five labels at
            // weight 400 whatever was selected — it had no selection input at
            // all — so "did not highlight" is both what an older snapshot
            // describes and where the toolkit starts.
            highlight_sections: match value.get("highlight_sections") {
                None => false,
                Some(v) => v.as_bool()?,
            },
        })
    }
}

/// R1504 — the `"<logical>:<align>"` payload `set_section_alignment` takes,
/// where the value half is a [`TextAlign`] spelling or the literal `default`.
///
/// The arm answers the EFFECTIVE row rather than the section it changed:
/// setting one exception does not move the others, but a client that just
/// learned it can clear one wants to see what is painted now.
///
/// `default` is the spelling that hands a section BACK to the header's rule,
/// and it is the reason this cannot be `parse_pair::<usize, TextAlign>`: the
/// sentinel is not a member of the enum, and making it one would put a
/// "no opinion" variant into a type whose whole job is to name one.
fn section_and_alignment(text: &str) -> Result<(usize, Option<TextAlign>), InvokeError> {
    let (logical, spelling) = require_pair::<usize, String>("set_section_alignment", text, ':')?;
    let align = if spelling == "default" {
        None
    } else {
        Some(TextAlign::from_wire(&spelling).ok_or_else(|| {
            InvokeError::rejected(format!(
                "set_section_alignment: {spelling:?} is not an alignment spelling, \
                 and is not the \"default\" sentinel that hands the section back to the header"
            ))
        })?)
    };
    Ok((logical, align))
}

impl Default for ColumnLayoutState {
    /// The empty header — no sections, and the three scalar rules at their
    /// constants. Deriving this would put **zero** in the bounds, which is a
    /// header whose sections may be zero-wide and at most zero-wide; the
    /// vectors are the only fields whose empty value is their default.
    fn default() -> Self {
        Self {
            order: Vec::new(),
            sizes: Vec::new(),
            hidden: Vec::new(),
            modes: Vec::new(),
            sort_indicator: None,
            sort_indicator_shown: false,
            default_section_size: DEFAULT_SECTION_SIZE,
            min_section_size: DEFAULT_MIN_COL_WIDTH,
            max_section_size: DEFAULT_MAX_COL_WIDTH,
            cascading_section_resizes: false,
            stretch_last_section: false,
            // R1496 — the toolkit's defaults, and the ones a fresh `ColumnLayout` has.
            // `from_json` decodes an ABSENT field as `true` instead; the two answer
            // different questions — this is the state of a new header, that is
            // the state of an old one.
            sections_movable: false,
            sections_clickable: false,
            resize_contents_precision: DEFAULT_CONTENTS_PRECISION,
            // R1504 — the toolkit's horizontal-header default
            // (`setDefaultValues` centres it), and the same
            // new-vs-old split `sections_movable` above carries: `from_json`
            // decodes ABSENT as `Start`.
            default_alignment: DEFAULT_HEADER_ALIGNMENT,
            // R1510 — the toolkit's default, and the same value `from_json` decodes an
            // absent field as: a header that has never been told to highlight
            // and an older header that could not are the same header.
            highlight_sections: false,
        }
    }
}

/// R1451 §5.27 — one painted section: its place in the permutation, the
/// column it shows, and where it lands. Produced by
/// [`ColumnLayout::visible_placements`], which is the only walk that applies
/// hiding and sums the cumulative offset.
///
/// `visual` is the section's index in the **full** permutation, hidden sections
/// included (the toolkit's rule), so it is the identity a hit test and a drag
/// drop-classification speak; it is deliberately *not* the position in this
/// vector, which shifts as neighbours are hidden. `Default` is the empty placement —
/// meaningful only as the filler a fixed-`N` consumer pads a `[SectionPlacement; N]` buffer with
/// (a `WidgetCore::State` is `Copy`, so a binding cannot hold the `Vec`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionPlacement {
    /// Index in the full visual permutation — the section's hit identity.
    pub visual: usize,
    /// The logical column this section shows.
    pub logical: usize,
    /// Cumulative x offset of the section's leading edge, in logical pixels.
    pub x: u32,
    /// The section's painted width.
    pub size: u32,
}

/// R1493 §5.27 — the size a section takes when nothing else determined it:
/// The toolkit's `defaultSectionSize`, whose own default is style-derived
/// and lands at 100 logical pixels in the toolkit's common styles.
///
/// It is the fourth way a section acquires a size, and the one that was
/// missing. The other three — a stored width, a `ResizeToContents` hint, a
/// `Stretch` share — all answer "how wide is this section *now*"; this one
/// answers "how wide before anyone said". Without it a header could not be
/// reset, and a section that had never been sized was indistinguishable from
/// one deliberately set to the floor.
pub const DEFAULT_SECTION_SIZE: u32 = 100;

/// R1454 §5.36 — how many rows a `ResizeToContents` consumer measures by
/// default, matching the toolkit's `resizeContentsPrecision` default.
pub const DEFAULT_CONTENTS_PRECISION: usize = 1000;

/// R1504 §5.27 — where a fresh header's labels sit: the toolkit centres a
/// horizontal header (`setDefaultValues` sets `AlignCenter | AlignVCenter`), and this is the horizontal half of that.
///
/// Deliberately **not** what [`ColumnLayoutState::from_json`] gives an absent
/// field. A snapshot taken before this round describes a header whose labels
/// were painted flush left, so that decodes as [`TextAlign::Start`] — the same
/// new-header-vs-old-snapshot split R1496 drew for `sections_movable`.
pub const DEFAULT_HEADER_ALIGNMENT: TextAlign = TextAlign::Center;

/// R1452 §5.27 — where a section's size **comes from**: the toolkit's
/// `setSectionResizeMode`.
///
/// Before this, every pinion grid had exactly one policy — a stored number —
/// so a column could not fill the viewport and could not fit its content. The
/// mode is per **logical** section, like the size it governs.
///
/// The two questions the rest of the module asks are separate, because the
/// toolkit answers them differently: [`stores_size`](Self::stores_size) decides whether
/// the size is the stored one or a derived one, and
/// [`user_resizable`](Self::user_resizable) decides whether a *human gesture* may change
/// it. `Fixed` is the mode where those differ — a program may resize it, a drag
/// may not.
// `Signal` snapshots its value (`Owner::snapshot`), so a mode vector held in
// one must be serde round-trippable — the `GridSortState::SortDir` precedent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SectionResizeMode {
    /// The stored size, and the user may drag it. The toolkit's default.
    #[default]
    Interactive,
    /// The stored size, but only a program may change it.
    Fixed,
    /// The section divides whatever width the other sections leave over.
    Stretch,
    /// The section takes its content's size hint
    /// ([`set_content_widths`](ColumnLayout::set_content_widths)).
    ResizeToContents,
}

impl SectionResizeMode {
    /// Whether the size is the **stored** one rather than derived. The two
    /// derived modes ignore what [`resize_section`](ColumnLayout::resize_section) was last
    /// given, exactly as the toolkit does.
    #[must_use]
    pub fn stores_size(self) -> bool {
        matches!(self, Self::Interactive | Self::Fixed)
    }

    /// Whether a **user gesture** may change the size. Only `Interactive` —
    /// `Fixed` is precisely the mode that is programmatically settable and
    /// interactively frozen.
    #[must_use]
    pub fn user_resizable(self) -> bool {
        matches!(self, Self::Interactive)
    }

    /// The wire spelling, and the inverse of [`FromStr`](std::str::FromStr).
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Fixed => "fixed",
            Self::Stretch => "stretch",
            Self::ResizeToContents => "resize_to_contents",
        }
    }
}

impl std::str::FromStr for SectionResizeMode {
    type Err = ();

    /// One spelling per mode, no aliases: a client that guessed wrong gets an
    /// error rather than a silently different policy.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "interactive" => Ok(Self::Interactive),
            "fixed" => Ok(Self::Fixed),
            "stretch" => Ok(Self::Stretch),
            "resize_to_contents" => Ok(Self::ResizeToContents),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SectionResizeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// R1510 §5.27 — how much of a section the selection covers: the two
/// predicates `paintSection` asks, as one value.
///
/// The toolkit resolves a selection into two independent style flags — `State_On`
/// when `sectionIntersectsSelection(logical)` and `State_Sunken` when `isSectionSelected(logical)`, the second being the whole section — and both
/// are gated on `highlightSections`. Two predicates over the same selection cannot disagree in
/// only one direction (a covered section always intersects), so they are three
/// states rather than two booleans: the pair `(false, true)` does not exist, and a type
/// that can express it invites a caller to build it.
///
/// **Who computes this is the point.** the toolkit's header does, because it
/// has the selection model *and* the row count. [`ColumnLayout`] has neither — it is a
/// header, not a view — so the consumer that owns the rows publishes the
/// answer through [`set_section_selection`](ColumnLayout::set_section_selection), exactly as it
/// already publishes the content widths the toolkit's header gets from `sectionSizeFromContents()`.
/// Deriving it here would mean growing a selection model inside a column
/// header.
// `Signal` snapshots its value, so this must round-trip serde for the same
// reason `SectionResizeMode` above must.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SectionSelection {
    /// The selection does not reach this section: neither the toolkit
    /// predicate holds.
    #[default]
    Unselected,
    /// Some of the section's cells are selected — the toolkit's
    /// `sectionIntersectsSelection` alone.
    Partial,
    /// Every one of the section's cells is selected — the toolkit's
    /// `isSectionSelected`, which implies the intersection as well.
    Full,
}

impl SectionSelection {
    /// Whether the selection reaches the section at all — the toolkit's
    /// `sectionIntersectsSelection`, the predicate behind `State_On`.
    #[must_use]
    pub fn intersects(self) -> bool {
        matches!(self, Self::Partial | Self::Full)
    }

    /// Whether the selection covers the section entirely — the toolkit's
    /// `isSectionSelected`, the predicate behind `State_Sunken`.
    #[must_use]
    pub fn covers(self) -> bool {
        matches!(self, Self::Full)
    }

    /// The wire spelling, and the inverse of [`FromStr`](std::str::FromStr).
    ///
    /// Lower-case, like the [`SectionResizeMode`] vocabulary next door rather
    /// than the capitalised [`TextAlign`] one, and `"none"` for
    /// [`Unselected`](Self::Unselected) because this module already spells an
    /// absent thing that way (`sort_indicator` answers `"none"`). A client
    /// reading this header does not learn a third spelling convention.
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Unselected => "none",
            Self::Partial => "partial",
            Self::Full => "full",
        }
    }
}

impl std::str::FromStr for SectionSelection {
    type Err = ();

    /// One spelling per state, no aliases — the [`SectionResizeMode`] rule.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::Unselected),
            "partial" => Ok(Self::Partial),
            "full" => Ok(Self::Full),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SectionSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// R1498 §5.27 — the toolkit's `stretchLastSection` override, stated once.
///
/// "If this value is set to true, this property will override the resize mode
/// set on the last section in the header" — so the last painted section becomes
/// a [`SectionResizeMode::Stretch`] section, and the division that already
/// exists gives it what the others leave over. There is no second sizing
/// algorithm: a header with three stretching sections and this rule on has four
/// of them, sharing exactly as they always did.
///
/// A free function because its two callers know the two facts by different
/// means. [`ColumnLayout::visible_placements`] hoists them once for the whole
/// walk; [`ColumnLayout::effective_resize_mode`] reads them for one section.
/// Asking the getter from inside the walk would re-read both signals per
/// section — and stating the rule twice would leave one of the copies to learn
/// the next correction alone.
fn stretch_last_override(
    stored: SectionResizeMode,
    is_last_visible: bool,
    stretch_last: bool,
) -> SectionResizeMode {
    if stretch_last && is_last_visible {
        SectionResizeMode::Stretch
    } else {
        stored
    }
}

/// R1494 §5.27 — one interactive resize gesture's debt to the sections that
/// paid for it.
///
/// `anchor` is the **logical** section being resized; `victims` are the
/// followers that gave up width, in the visual order they were taken from,
/// each with the size it held *before* it paid. Repaying walks the vector
/// backwards, so the last section to be squeezed is the first to be let go —
/// the order that makes a drag out and back land exactly where it started.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Cascade {
    anchor: usize,
    victims: Vec<(usize, u32)>,
}

/// R1451 §5.27 §5.51 — order × size × visibility for one grid's columns,
/// keyed as header view keys them. See the [module docs](self) for the
/// ownership split; construct one per grid and let the header `External`
/// delegate its drag hooks to [`sections`](Self::sections).
#[derive(Debug)]
pub struct ColumnLayout {
    /// Fixed section count. The permutation is always over `0..count`, and a
    /// length change in the shared [`ColumnWidths`] does not move it — the
    /// header's structure is the layout's, not the width model's.
    count: usize,
    /// `order[visual] = logical` **and** the live drag / keyboard-grab
    /// session. The single permutation store.
    sections: ReorderModel,
    /// `sizes[logical]`. Shared so a live border-drag resize
    /// ([`ColumnResizeExternal`](crate::widgets::column_widths::ColumnResizeExternal))
    /// writes the same store the layout reads.
    sizes: Rc<ColumnWidths>,
    /// `hidden[logical]` — reactive, so a view-fn that reads the projection
    /// re-runs when a column is hidden.
    hidden: Signal<Vec<bool>>,
    /// R1452 — `modes[logical]`: where each section's size comes from.
    modes: Signal<Vec<SectionResizeMode>>,
    /// R1452 — `content_widths[logical]`: the size hint a
    /// [`SectionResizeMode::ResizeToContents`] section takes.
    ///
    /// Supplied by the consumer, because that is where the answer is: the
    /// toolkit's header view does not measure either — `sectionSizeFromContents()` asks the model /
    /// delegate for a `sizeHint`. A grid that measures its cells feeds the
    /// measurement in here; one that knows its content (fixed-format columns,
    /// a monospace grid) computes it directly.
    content_widths: Signal<Vec<u32>>,
    /// R1452 — the width [`SectionResizeMode::Stretch`] sections divide.
    /// `None` until a consumer publishes its viewport, in which case a
    /// `Stretch` section falls back to its stored size rather than collapsing.
    available_width: Signal<Option<u32>>,
    /// R1491 — `(logical, ascending)`: the toolkit's `sortIndicatorSection()` paired with `sortIndicatorOrder()`. Reactive, because a
    /// header repaints its glyph and re-announces its `aria-sort` when the indicator
    /// moves.
    ///
    /// It lives with the permutation rather than in the sorting model for the
    /// reason the toolkit puts it in header view: it is *header* state. It has
    /// to survive `saveState` / `restoreState` with no model attached, and it has to be keyed the
    /// way the sizes and hidden flags are keyed, so dragging a section carries
    /// its arrow along instead of leaving it on the position. What is sorted
    /// is still the model's answer — a consumer connects the two exactly as
    /// the toolkit connects `sortIndicatorChanged` to `sortByColumn`.
    sort_indicator: Signal<Option<(usize, bool)>>,
    /// R1491 — the toolkit's `sortIndicatorShown`. Default `false`, as in the toolkit, where the
    /// view turns it on (`setSortingEnabled`) rather than the header assuming it.
    sort_indicator_shown: Signal<bool>,
    /// R1454 — how many rows a `ResizeToContents` consumer should measure.
    ///
    /// Reactive, and the first draft got that wrong: it was a plain `Cell` on
    /// the reasoning that a sampling bound is "policy, not painted state". But
    /// the bound is an INPUT to a painted result — the consumer reads it in its
    /// view fn to decide what to measure — so a write that did not re-run the
    /// view could not reach the hints at all. The demo caught it: the knob read
    /// back its new value and every content width stayed put.
    contents_precision: Signal<usize>,
    /// R1494 — the toolkit's `cascadingSectionResizes`. Default `false`, as in the toolkit.
    cascading: Signal<bool>,
    /// R1498 — the toolkit's `stretchLastSection`: whether the section painted **last** takes
    /// whatever the viewport has left over. Default `false`, as in the toolkit,
    /// where the view opts in (tree view's header does; table view's does
    /// not).
    ///
    /// Keyed by *position*, which is the whole reason it is not
    /// [`SectionResizeMode::Stretch`] on the last column. Measured on this very
    /// header before the round: with the last column set to `Stretch`, hiding
    /// it dropped the fill entirely (the row went back to 470 of a 640-wide
    /// viewport) and moving it to the front painted the fill at the front.
    /// A mode belongs to a column and travels with it; this rule belongs to the
    /// header and stays where it is.
    stretch_last: Signal<bool>,
    /// R1504 — the toolkit's `defaultAlignment`: where a section's label sits when the model
    /// has no opinion. Reactive, because it is painted — the mistake
    /// [`contents_precision`](Self::contents_precision) documents above.
    default_alignment: Signal<TextAlign>,
    /// R1504 — the per-section exceptions, `None` meaning "take the header's
    /// rule". The toolkit keeps these in the **model** (`headerData(section, orientation, TextAlignmentRole)`) rather than the
    /// header, and its `saveState()` does not carry them; this vector is the same
    /// separation, which is why it is a field here and not a member of [`ColumnLayoutState`].
    ///
    /// Keyed by **logical** section, like `sizes` and `hidden`: an exception
    /// belongs to a column and has to travel with it when the column is
    /// dragged.
    section_alignments: Signal<Vec<Option<TextAlign>>>,
    /// R1510 — the toolkit's `highlightSections`. Default `false`, as in the toolkit. Reactive
    /// because it is painted: turning it off has to un-bold every label that
    /// was bold, which is only possible if the write reaches the view.
    highlight: Signal<bool>,
    /// R1510 — `selection[logical]`: how much of each section the selection
    /// covers, as published by the consumer.
    ///
    /// Supplied rather than derived, for the reason [`SectionSelection`] documents: the
    /// toolkit's header reads a selection model this widget does not have, and
    /// a header that grew one would be a view. The peer of
    /// [`content_widths`](Self::content_widths) — both are answers only the consumer can
    /// give, and both are inputs to what gets painted.
    ///
    /// Keyed by **logical** section, like `sizes` and the alignment exceptions:
    /// a selection is a fact about a *column*, so dragging that column to a new
    /// position has to carry its highlight along.
    ///
    /// Not part of [`ColumnLayoutState`], for the same reason the alignment exceptions are
    /// not: the toolkit's `saveState()` carries the rule and never the selection.
    selection: Signal<Vec<SectionSelection>>,
    /// R1494 — the cascade currently in flight, if any.
    ///
    /// A cascade has to be *undoable* or it is not a drag: pulling a section
    /// wide and back must leave the row where it started, which means
    /// remembering what each follower was before it paid. The toolkit keeps
    /// the same memory (`cascadingSectionSize`) and clears it when the drag ends.
    ///
    /// pinion has no drag session on this widget (the pointer grabber is a
    /// different binding — see [`interactive_resize_section`](ColumnLayout::interactive_resize_section)),
    /// so the anchor stands in for one: resizing a different section is a new
    /// gesture and drops the old memory, as does any write that invalidates the
    /// sizes it remembers.
    cascade: Signal<Option<Cascade>>,
    /// R1493 — the toolkit's `defaultSectionSize`: the size a section takes when nothing else
    /// determined it.
    ///
    /// Stored raw and clamped **at read**
    /// ([`default_section_size`](Self::default_section_size)) rather than on
    /// write, so moving a bound moves the default with it and there is no
    /// second write path to forget — the [[r1449-completion-model]] rule this
    /// module already follows for every derived answer. The alternative,
    /// re-clamping it from `ColumnWidths::set_min_width`, cannot even be
    /// written: the bounds live in the shared width model, which does not know
    /// this layout exists.
    default_size: Signal<u32>,
    /// R1496 — the toolkit's `sectionsMovable`. Default `false`, as in the toolkit, where the
    /// view opts in (table view does not; a reorderable header is a deliberate
    /// affordance, not the baseline).
    ///
    /// Reactive because a header that stops being movable paints differently —
    /// the readout naming the rule, and anything that dresses a draggable
    /// section — and because a write arriving over the wire has to reach the
    /// view that reads it (the R1454 lesson: a rule read inside a view fn is
    /// an input to a painted result, not "policy").
    movable: Signal<bool>,
    /// R1496 — the toolkit's `sectionsClickable`. Default `false`, as in the toolkit, where `setSortingEnabled(true)` is
    /// what turns it on.
    clickable: Signal<bool>,
    /// R1496 — the **visual** section a `PointerDown` last landed on, held until the
    /// matching release so the click can test the toolkit's rule that a press
    /// and its release must be on the same section.
    ///
    /// The [`ReorderModel`]'s own `pressed` cannot answer this: the drag
    /// machinery consumes it, clearing it in `drag_release` — which the router
    /// calls BEFORE it dispatches the trailing `PointerUp` — so by the time the
    /// click arrives the model has already forgotten. It is a plain `Cell`
    /// rather than a `Signal` because nothing paints from it; a press that
    /// invalidated the view would repaint the whole strip for a value no view
    /// fn reads.
    pressed_section: Cell<Option<usize>>,
}

/// R1501 — the paths [`ColumnLayout`] answers itself, ahead of the ones it
/// inherits from the embedded [`ReorderModel`]. Composed into
/// [`ColumnLayout::SCHEMA_FIELDS`], which is the list callers should read; this
/// half exists only because the composition needs two operands.
///
/// Ordered the way [`ColumnLayout::query`]'s doc reads: the whole-state
/// round-trip, the stored/effective pairs, the header-wide rules, the
/// projections, then the parametric families and the actions.
const OWN_SCHEMA_FIELDS: &[SchemaField] = &[
    SchemaField::new("state", "json"),
    SchemaField::new("count", "int"),
    // The stored/effective pairs (R1493, R1498). Declared adjacently because
    // the pair is the contract: the first is what a restore replays, the second
    // is what the header paints, and under `Stretch` / `ResizeToContents` /
    // `stretchLastSection` they differ.
    SchemaField::new("sizes", "json"),
    SchemaField::new("section_sizes", "json"),
    SchemaField::new("resize_modes", "json"),
    SchemaField::new("effective_resize_modes", "json"),
    // The header-wide rules the toolkit's `saveState()` carries.
    SchemaField::new("default_section_size", "int"),
    SchemaField::new("min_section_size", "int"),
    SchemaField::new("max_section_size", "int"),
    SchemaField::new("cascading_section_resizes", "boolean"),
    SchemaField::new("stretch_last_section", "boolean"),
    SchemaField::new("sections_movable", "boolean"),
    SchemaField::new("sections_clickable", "boolean"),
    SchemaField::new("resize_contents_precision", "int"),
    // R1504 — the label rule. The toolkit's `defaultAlignment` is a header scalar its `saveState()`
    // carries; the per-section exception below is the model's and is not
    // saved, which is why only this one sits among the saved rules.
    SchemaField::new("default_alignment", "string"),
    // R1510 — the highlight rule, the last field the toolkit's `saveState()` carries
    // that this header did not have.
    SchemaField::new("highlight_sections", "boolean"),
    SchemaField::new("sort_indicator", "string"),
    SchemaField::new("sort_indicator_section", "int"),
    SchemaField::new("sort_indicator_order", "string"),
    SchemaField::new("sort_indicator_shown", "boolean"),
    // The projections, and the two inputs a consumer publishes.
    SchemaField::new("hidden", "json"),
    SchemaField::new("hidden_count", "int"),
    SchemaField::new("visible_sections", "json"),
    SchemaField::new("visible_widths", "json"),
    SchemaField::new("visible_total", "int"),
    SchemaField::new("placements", "json"),
    SchemaField::new("content_widths", "json"),
    SchemaField::new("available_width", "int"),
    // R1504 — the EFFECTIVE alignment per logical section: each
    // section's own exception where it has one, the header's rule where
    // it does not. The peer of `section_sizes` against `sizes`.
    SchemaField::new("alignments", "json"),
    // R1510 — the third input a consumer publishes, and what the header makes
    // of it. Two words on purpose: the SELECTION is the view's answer (which
    // cells are picked, collapsed per column), the HIGHLIGHT is this header's
    // (the selection, gated on `highlight_sections`). A client that reads only
    // `highlights` can tell what is painted; one that reads both can tell
    // whether an unhighlighted section is unselected or merely un-permitted.
    SchemaField::new("selections", "json"),
    SchemaField::new("highlights", "json"),
    // The parametric families. Every one is an index into `count`, except the
    // last: `logical_index_at` takes a **pixel** offset along the painted row,
    // whose bound the surface publishes as `visible_total`. R1501 — it was
    // declared as a plain scalar spelling `<x>` in its path, which is neither
    // half of a declaration: `$schema` rendered it exactly like `visible_total`
    // and a client had nothing to enumerate the argument from. It escaped both
    // halves of the R1353.1 audit — the static scan reads `parametric(` call
    // sites, and the dynamic one only reaches widgets `pinion-core` links.
    SchemaField::parametric(
        "resize_mode.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "effective_resize_mode.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "content_width.<logical>",
        "int",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "section_size.<logical>",
        "int",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "section_hidden.<logical>",
        "boolean",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "section_position.<logical>",
        "int",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    // R1504 — the pair, declared adjacently like every other stored/effective
    // pair in this list: the first is what the label is painted with, the
    // second is the model's exception alone and answers `Null` where the
    // section defers to the header.
    SchemaField::parametric(
        "section_alignment.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "section_alignment_override.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    // R1510 — the same pair per section: what the consumer published, and what
    // the header paints from it. Unlike the alignment pair above, neither half
    // answers `Null` — a section the selection never reached is `"none"`, which
    // is a state and not an absence.
    SchemaField::parametric(
        "section_selection.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "section_highlight.<logical>",
        "string",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "visual_index.<logical>",
        "int",
        const { &[SchemaArg::index("logical", "count")] },
    ),
    SchemaField::parametric(
        "logical_index.<visual>",
        "int",
        const { &[SchemaArg::index("visual", "count")] },
    ),
    SchemaField::parametric(
        "logical_index_at.<x>",
        "int",
        const { &[SchemaArg::index("x", "visible_total")] },
    ),
    // The toolkit's section vocabulary, as `invoke` channels.
    SchemaField::action("swap_sections", "string"),
    SchemaField::action("resize_section", "string"),
    SchemaField::action("interactive_resize_section", "string"),
    SchemaField::action("set_section_hidden", "string"),
    SchemaField::action("set_section_alignment", "string"),
    // R1510 — the per-section peer of the whole-row `selections` write.
    SchemaField::action("set_section_selection", "string"),
    SchemaField::action("set_resize_mode", "string"),
    SchemaField::action("set_all_resize_modes", "string"),
    SchemaField::action("set_sort_indicator", "string"),
    SchemaField::action("cycle_sort_indicator", "int"),
    SchemaField::action("clear_sort_indicator", "string"),
    SchemaField::action("reset_default_section_size", "string"),
];

/// R1501 — the composed declaration, in a `static` so it outlives the
/// `&'static` slice [`ColumnLayout::SCHEMA_FIELDS`] hands out. The length is an
/// expression over its own operands, so adding a path at either end cannot
/// leave a blank row behind.
static SCHEMA_FIELD_STORAGE: [SchemaField;
    OWN_SCHEMA_FIELDS.len() + ReorderModel::SCHEMA_FIELDS.len()] =
    SchemaField::concat(OWN_SCHEMA_FIELDS, ReorderModel::SCHEMA_FIELDS);

impl ColumnLayout {
    /// Build a layout for the given per-logical-section `sizes`, in identity
    /// order with every section shown.
    #[must_use]
    pub fn new(sizes: Vec<u32>) -> Self {
        Self::with_widths(Rc::new(ColumnWidths::new(sizes)))
    }

    /// Build a layout over an **existing** [`ColumnWidths`] — the composition
    /// a grid uses when the R786 resize grabber already drives that model, so
    /// dragging a border and reading `section_size` cannot disagree.
    #[must_use]
    pub fn with_widths(sizes: Rc<ColumnWidths>) -> Self {
        let count = sizes.col_count();
        let content = sizes.widths();
        Self {
            count,
            sections: ReorderModel::new(count, ReorderAxis::Horizontal),
            sizes,
            hidden: Signal::new(vec![false; count]),
            modes: Signal::new(vec![SectionResizeMode::default(); count]),
            // Seeded from the initial sizes so a section switched to
            // `ResizeToContents` before its consumer has published a hint
            // keeps its width instead of collapsing to the floor.
            content_widths: Signal::new(content),
            available_width: Signal::new(None),
            sort_indicator: Signal::new(None),
            sort_indicator_shown: Signal::new(false),
            contents_precision: Signal::new(DEFAULT_CONTENTS_PRECISION),
            default_size: Signal::new(DEFAULT_SECTION_SIZE),
            cascading: Signal::new(false),
            stretch_last: Signal::new(false),
            // R1504 — the toolkit's horizontal default, and no exceptions: a
            // fresh header has a rule and the model has said nothing.
            default_alignment: Signal::new(DEFAULT_HEADER_ALIGNMENT),
            section_alignments: Signal::new(vec![None; count]),
            // R1510 — the toolkit's default, and an empty selection: a fresh
            // header may not highlight, and nothing has been selected to
            // highlight.
            highlight: Signal::new(false),
            selection: Signal::new(vec![SectionSelection::Unselected; count]),
            cascade: Signal::new(None),
            movable: Signal::new(false),
            clickable: Signal::new(false),
            pressed_section: Cell::new(None),
        }
    }

    /// Number of sections (hidden ones included — hiding does not remove a
    /// section, it stops painting it).
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The embedded reorder model — the drag hooks
    /// (`begin_drag_payload` / `drag_to` / `drag_release`) an owning
    /// `External` delegates to, and the keyboard grab state.
    #[must_use]
    pub fn sections(&self) -> &ReorderModel {
        &self.sections
    }

    /// The shared size store, for handing to
    /// [`column_resize_externals`](crate::widgets::column_widths::column_resize_externals).
    /// Its index is the **logical** section here; a binding that paints in
    /// visual order maps through [`logical_index`](Self::logical_index)
    /// before registering, so a grabber on the third *column* resizes the
    /// section that is actually third.
    #[must_use]
    pub fn widths(&self) -> &Rc<ColumnWidths> {
        &self.sizes
    }

    /// The visual permutation (`order[visual] = logical`).
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        self.sections.order()
    }

    /// Where logical section `logical` currently sits — the toolkit's
    /// `visualIndex()`. Counts hidden sections, which keep their place.
    #[must_use]
    pub fn visual_index(&self, logical: usize) -> Option<usize> {
        self.sections.order().iter().position(|&l| l == logical)
    }

    /// Which logical section sits at visual position `visual` — the toolkit's
    /// `logicalIndex()`.
    #[must_use]
    pub fn logical_index(&self, visual: usize) -> Option<usize> {
        self.sections.order().get(visual).copied()
    }

    /// Move the section at visual `from` to visual `to` — the toolkit's
    /// `moveSection()`. Sizes and hidden flags are keyed by logical section,
    /// so they travel with it and nothing else has to be updated.
    pub fn move_section(&self, from: usize, to: usize) {
        self.sections.move_section(from, to);
    }

    /// Exchange the sections at two visual positions — the toolkit's `swapSections()`.
    /// Distinct from [`move_section`](Self::move_section): a swap displaces exactly one
    /// other section, a move shifts every section in between. Out-of-range
    /// indices are ignored.
    pub fn swap_sections(&self, a: usize, b: usize) {
        if a >= self.count || b >= self.count || a == b {
            return;
        }
        let mut order = self.sections.order();
        order.swap(a, b);
        // A swap of two valid positions is still a permutation, so the
        // validated setter cannot reject it — routing through it anyway keeps
        // one write path into the order.
        self.sections.set_order(&order);
    }

    /// R1491 — which **logical** section carries the sort indicator and in
    /// which direction (`true` ascending) — the toolkit's `sortIndicatorSection()` and `sortIndicatorOrder()` in one
    /// read, because the two are never useful apart and a pair cannot go out
    /// of step with itself.
    ///
    /// Pair it with
    /// [`col_sort_dir`](crate::widgets::grid_sort::col_sort_dir) to ask the
    /// per-header question ("does THIS section show the glyph"), which is the
    /// same SSOT an unmoved grid header already asks.
    #[must_use]
    pub fn sort_indicator(&self) -> Option<(usize, bool)> {
        self.sort_indicator.get()
    }

    /// R1491 — put the indicator on a section — the toolkit's `setSortIndicator()`. Out of
    /// range is a no-op, so a stale column index cannot move the arrow onto a
    /// section that does not exist.
    pub fn set_sort_indicator(&self, logical: usize, ascending: bool) {
        if logical >= self.count {
            return;
        }
        self.sort_indicator.set(Some((logical, ascending)));
    }

    /// R1491 — take the indicator off every section, leaving the header
    /// unsorted. The toolkit spells this `setSortIndicator(-1, …)`; a `usize` section cannot carry that
    /// sentinel, and a named method says what the sentinel meant.
    pub fn clear_sort_indicator(&self) {
        self.sort_indicator.set(None);
    }

    /// R1491 — advance one section through ascending → descending → unsorted,
    /// which is what a click on a clickable header does. Delegates the cycle
    /// rule to [`cycle_col_sort`] so the header and the sorting model agree on
    /// what a repeated click means.
    pub fn cycle_sort_indicator(&self, logical: usize) {
        self.sort_indicator.set(cycle_col_sort(
            self.sort_indicator.get(),
            logical,
            self.count,
        ));
    }

    /// R1491 — whether the indicator is painted — the toolkit's `isSortIndicatorShown()`.
    #[must_use]
    pub fn is_sort_indicator_shown(&self) -> bool {
        self.sort_indicator_shown.get()
    }

    /// R1491 — the toolkit's `setSortIndicatorShown()`. Turning it off keeps *which* section is
    /// sorted, exactly as the toolkit does: the arrow stops being drawn, the
    /// sort does not stop being the sort.
    pub fn set_sort_indicator_shown(&self, shown: bool) {
        self.sort_indicator_shown.set(shown);
    }

    /// R1496 — whether the user may drag a section to a new position — the
    /// toolkit's `sectionsMovable()`.
    #[must_use]
    pub fn sections_movable(&self) -> bool {
        self.movable.get()
    }

    /// R1496 — the toolkit's `setSectionsMovable()`. Governs the **interactive** move only:
    /// [`move_section`](Self::move_section) and `swap_sections` keep working, exactly as the
    /// toolkit's `moveSection()` does on a header the user cannot drag. The split is the
    /// one R1494 already drew between `resize_section` and `interactive_resize_section` — a permission is about the
    /// gesture, not about the model.
    pub fn set_sections_movable(&self, movable: bool) {
        self.movable.set(movable);
    }

    /// R1496 — whether a press-release on a section is reported as a click —
    /// the toolkit's `sectionsClickable()`.
    #[must_use]
    pub fn sections_clickable(&self) -> bool {
        self.clickable.get()
    }

    /// R1496 — the toolkit's `setSectionsClickable()`. Independent of
    /// [`set_sections_movable`](Self::set_sections_movable): a header may be
    /// clickable and pinned (the common sortable table) or movable and inert.
    pub fn set_sections_clickable(&self, clickable: bool) {
        self.clickable.set(clickable);
    }

    /// R1496 — arm a section drag, or refuse — the movable-gated peer of
    /// [`ReorderModel::begin_drag_payload`], and what an owning `External`'s
    /// `begin_drag` should call.
    ///
    /// `None` on a header that is not movable, which is what makes the refusal
    /// real: with no payload the router opens no session, so nothing previews
    /// a drop and nothing commits one. The press is still recorded, so the
    /// release is still a click — the toolkit keeps those two independent and
    /// so does this.
    #[must_use]
    pub fn begin_section_drag(&self, kind: Cow<'static, str>) -> Option<DragPayload> {
        if !self.sections_movable() {
            return None;
        }
        self.sections.begin_drag_payload(kind)
    }

    /// R1496 — commit a section drop. Supersedes R1491's `release_section`,
    /// which also reported the **click**; that half was wrong twice over.
    ///
    /// It re-derived a determination the framework already owns. R794 §5.51 is
    /// the click-vs-drag SSOT — it withholds the trailing `PointerUp` after a drag
    /// that travelled past `DRAG_CLICK_THRESHOLD_PX`, and says in as many words that no drag source
    /// re-derives this per binding. R1491 re-derived it, from the permutation,
    /// and got a different answer: a section dragged across the strip and
    /// dropped back into its own gap leaves the permutation untouched, so that
    /// rule called it a click and sorted the column the user had just decided
    /// not to move. The toolkit calls it a move, by the same `startDragDistance` the router
    /// already applies here.
    ///
    /// So the click now arrives where every other draggable-and-clickable
    /// widget in this workspace takes it — the trailing `PointerUp`, decoded in
    /// [`handle_send`](Self::handle_send) — and this method only commits the
    /// drop.
    pub fn end_section_drag(&self, payload: &DragPayload, over: Option<&DropPoint>) {
        self.sections.drag_release(payload, over);
    }

    /// R1496 — the `send` arm of [`invoke`](Self::invoke): decode the pointer
    /// edge here and then hand it DOWN, so the reorder model still records the
    /// press its `begin_drag_payload` reads.
    ///
    /// The press is remembered on both sides on purpose: the model's copy arms
    /// the drag and is consumed by it, this one outlives the drag so the
    /// release can still be a click.
    ///
    /// Answers the **logical** section a click landed on — `Null` otherwise —
    /// because the router discards this return on the real pointer path. A
    /// binding therefore acts on what it gets back from *this* call, inside its
    /// own `invoke`, and an RPC client driving the same edges is told the same
    /// thing.
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when the argument is not text.
    fn invoke_send(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(payload) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let clicked = self.handle_send(payload);
        self.sections.invoke("send", args)?;
        Ok(match clicked {
            Some(logical) => IntrospectValue::Int(i64::try_from(logical).unwrap_or(0)),
            None => IntrospectValue::Null,
        })
    }

    /// R1496 — decode a `send` payload's pointer edge and report a **click**.
    ///
    /// Returns the **logical** section a press-release landed on, or `None`. The
    /// caller decides what a click means — this type will not cycle its own
    /// indicator, because what is sorted is the model's answer and a header
    /// can be clickable without a sort attached (the toolkit splits it the
    /// same way, through `sortIndicatorChanged`).
    ///
    /// Two rules, both the toolkit's:
    ///
    /// - the press and the release must be on the **same** section, so a press
    ///   that slid onto a neighbour activates nothing;
    /// - a moved drag is not a click — which needs no code here, because R794
    ///   does not dispatch the release at all in that case.
    ///
    /// `PointerUp` only, not the broader [`is_activation_event`](crate::input::is_activation_event): the
    /// header's click is a press-release *pair*, and the pair is what the
    /// same-section rule tests. A keyboard activation has no press to pair
    /// with, and the toolkit has no keyboard peer of `sectionClicked` to copy.
    #[must_use]
    pub fn handle_send(&self, payload: &str) -> Option<usize> {
        let crate::composite_tag::SendPayload {
            key: sub, event, ..
        } = split_send_payload(payload)?;
        let visual: usize = sub.parse().ok()?;
        if event == PointerWireEvent::Down.as_wire_name() {
            if visual < self.count {
                self.pressed_section.set(Some(visual));
            }
            return None;
        }
        // The two edges that end a press WITHOUT activating it, named rather
        // than assumed: a stray that wanders off the strip must not sort on
        // some later release. The first draft inverted this — it dropped the
        // press on every event it did not recognise — and the wire had one it
        // had not enumerated. A second in-place gesture on the same section
        // arrives as `PointerDown`, **`DoubleClick`**, `PointerUp`, so every
        // other click was silently discarded. `DoubleClick` is not an
        // abandonment; it is a second notification about the very press that is
        // in flight.
        if event == PointerWireEvent::Leave.as_wire_name()
            || event == PointerWireEvent::Cancel.as_wire_name()
        {
            self.pressed_section.set(None);
            return None;
        }
        if event != PointerWireEvent::Up.as_wire_name() {
            return None;
        }
        let pressed = self.pressed_section.replace(None)?;
        if pressed != visual || !self.sections_clickable() {
            return None;
        }
        self.logical_index(visual)
    }

    /// R1492 — the smallest any section may be — the toolkit's
    /// `minimumSectionSize()`.
    ///
    /// Header-level in the toolkit and delegated to the shared width model
    /// here, because that is where the clamp lives and a bound with two homes
    /// is a bound two paths can disagree about.
    #[must_use]
    pub fn minimum_section_size(&self) -> u32 {
        self.sizes.min_width()
    }

    /// R1492 — the toolkit's `setMinimumSectionSize()`. Every stored width is re-clamped, and a
    /// floor above the current ceiling carries the ceiling with it.
    pub fn set_minimum_section_size(&self, size: u32) {
        self.sizes.set_min_width(size);
    }

    /// R1492 — the largest any section may be — the toolkit's
    /// `maximumSectionSize()`.
    /// [`DEFAULT_MAX_COL_WIDTH`] means unbounded, which is what every pinion
    /// header was before this. (R1493 brought the constant into scope here, so
    /// the explicit link target it used to need is now redundant.)
    #[must_use]
    pub fn maximum_section_size(&self) -> u32 {
        self.sizes.max_width()
    }

    /// R1492 — the toolkit's `setMaximumSectionSize()`. Applies to **every** way a section gets a
    /// size: a stored width, a content hint, and a stretch share. A ceiling
    /// that only one of the three honoured would be worse than none, because
    /// the row would fill differently depending on the mode.
    pub fn set_maximum_section_size(&self, size: u32) {
        self.sizes.set_max_width(size);
    }

    /// R1452 — where logical section `logical` takes its size from — the toolkit's
    /// `sectionResizeMode()`.
    #[must_use]
    pub fn resize_mode(&self, logical: usize) -> SectionResizeMode {
        self.modes.get().get(logical).copied().unwrap_or_default()
    }

    /// R1452 — set one section's sizing policy — the toolkit's
    /// `setSectionResizeMode(logicalIndex, mode)`. Out of range is a no-op.
    pub fn set_resize_mode(&self, logical: usize, mode: SectionResizeMode) {
        if logical >= self.count {
            return;
        }
        self.modes.set_with(|m| {
            let mut next = m.clone();
            if let Some(slot) = next.get_mut(logical) {
                *slot = mode;
            }
            next
        });
    }

    /// R1452 — set every section's policy at once — the toolkit's
    /// `setSectionResizeMode(mode)`.
    pub fn set_all_resize_modes(&self, mode: SectionResizeMode) {
        self.modes.set(vec![mode; self.count]);
    }

    /// R1452 — the content size hint of logical section `logical`, what a
    /// [`SectionResizeMode::ResizeToContents`] section sizes to.
    #[must_use]
    pub fn content_width(&self, logical: usize) -> u32 {
        self.content_widths.get().get(logical).copied().unwrap_or(0)
    }

    /// R1454 §5.36 — how many rows a consumer should measure when it computes
    /// a [`ResizeToContents`](SectionResizeMode::ResizeToContents) hint: the toolkit's `resizeContentsPrecision`,
    /// default `1000` like the toolkit's.
    ///
    /// Not a nicety — a bound the measurement demands. A shape **miss costs
    /// 18.5 us** against a **118 ns** cache hit
    /// ([`LayoutCache::shapes`](../../../pinion_text/struct.LayoutCache.html)
    /// is the counter that showed it), and the measurement cache is LRU-bounded
    /// at 256 layouts, so a consumer that measures every row of a large grid
    /// each frame exceeds the cache, re-shapes the whole set every pass, and
    /// pays **5.6 ms per 300 strings** — a third of a 60fps frame, forever.
    /// Sampling a bounded prefix keeps the working set warm.
    ///
    /// It lives here, on the header, because that is where the toolkit puts it
    /// and because it is then readable and writable as data (`query` / `intervene`) rather
    /// than a constant buried in a binding. The *consumer* honours it, exactly
    /// as it supplies the hints themselves — and reads it inside its view fn,
    /// which is why it subscribes.
    ///
    /// R1496 — and why it is saved. R1454 held that it "decides what a
    /// consumer MEASURES, not what the header IS" and kept it out of
    /// [`save_state`](Self::save_state); the toolkit serialises it (`write()`), and the
    /// argument that made it a `Signal` is the argument that saves it — a bound
    /// that is an input to a painted width belongs with the widths it
    /// produced.
    #[must_use]
    pub fn resize_contents_precision(&self) -> usize {
        self.contents_precision.get()
    }

    /// R1454 — set the row-sampling bound. `0` is clamped to `1`: measuring
    /// nothing would leave a content-fitted column with no content to fit, and
    /// silently sizing it to the floor is the kind of answer a caller cannot
    /// tell from a bug.
    pub fn set_resize_contents_precision(&self, rows: usize) {
        self.contents_precision.set(rows.max(1));
    }

    /// R1452 — publish the per-**logical**-section content size hints (the
    /// toolkit's delegate `sizeHint`). A vector of the wrong length is ignored,
    /// because a partially-applied hint set would size some columns to another
    /// grid's content.
    pub fn set_content_widths(&self, widths: Vec<u32>) {
        if widths.len() == self.count {
            self.content_widths.set(widths);
        }
    }

    /// R1452 — the width [`SectionResizeMode::Stretch`] sections divide,
    /// usually the grid's viewport. `None` until a consumer publishes one.
    #[must_use]
    pub fn available_width(&self) -> Option<u32> {
        self.available_width.get()
    }

    /// R1452 — publish the width `Stretch` sections divide.
    pub fn set_available_width(&self, width: Option<u32>) {
        self.available_width.set(width);
    }

    /// Size of logical section `logical` — the toolkit's `sectionSize()`, resolved through the
    /// section's [`resize_mode`](Self::resize_mode). `0` for an unknown section (the
    /// toolkit's answer too).
    ///
    /// A hidden section reports the size it will have when shown rather than
    /// the toolkit's `0` — [`section_position`](Self::section_position) is the slot that says
    /// "painted nowhere", so reporting the size here is strictly more
    /// information and no ambiguity. A hidden `Stretch` section reports its stored
    /// size: it takes part in no division, because there is no share to take
    /// when it occupies no width.
    #[must_use]
    pub fn section_size(&self, logical: usize) -> u32 {
        if logical >= self.count {
            return 0;
        }
        self.section_sizes()[logical]
    }

    /// R1493 — every section's **effective** size, keyed by logical section:
    /// the plural of [`section_size`](Self::section_size), and the number the
    /// header actually paints.
    ///
    /// This is not [`save_state`](Self::save_state)'s `sizes`, and the
    /// difference is the whole reason it exists. A section has a size it was
    /// *given* (stored, restorable, what `saveState` carries) and a size it
    /// *has* (resolved through its [`SectionResizeMode`]). Under `Interactive`
    /// and `Fixed` the two are equal; under `Stretch` and `ResizeToContents`
    /// they are not, and before this round the only logical-keyed plural on the
    /// wire was the stored one. A client reading it under a stretch header was
    /// handed `[150, 90, …]` for a row painting `[128, 128, …]` — the exact
    /// thing [`resize_section`](Self::resize_section) already promises never to
    /// do ("the return is the size the section actually has, so a client is
    /// never told a number the grid is not painting"). The single-section write
    /// path kept that promise; the plural read path did not.
    ///
    /// The toolkit keeps one number per section and re-derives on relayout, so
    /// it needs no such pair; pinion keeps the size you asked for across a
    /// mode switch, and having kept two numbers must say which one it is
    /// handing you.
    ///
    /// Hidden sections are painted nowhere, so they report the size they bring
    /// to the division — the same fallback the singular has always used.
    #[must_use]
    pub fn section_sizes(&self) -> Vec<u32> {
        let mut out: Vec<u32> = (0..self.count)
            .map(|l| self.base_size(l, self.resize_mode(l)))
            .collect();
        for p in self.visible_placements() {
            out[p.logical] = p.size;
        }
        out
    }

    /// The size a section brings to the division: the stored one, or the
    /// content hint. `Stretch` has no size of its own — it gets what is left —
    /// so it falls back to the stored size, which is what it reports when
    /// there is nothing to divide.
    fn base_size(&self, logical: usize, mode: SectionResizeMode) -> u32 {
        match mode {
            // R1492 — through the width model's own clamp, both bounds at
            // once. Before, this said `.max(min_width())`: it knew the floor
            // and there was no ceiling to know, so a content hint sized the
            // section however long its longest cell was.
            SectionResizeMode::ResizeToContents => self.sizes.clamp(self.content_width(logical)),
            // The stored size is already clamped by the width model on write.
            _ => self.sizes.width(logical),
        }
    }

    /// Resize logical section `logical` — the toolkit's `resizeSection()`. Returns the applied
    /// size after the width model's minimum-width clamp (`0` when the section
    /// does not exist), so an AI client learns the outcome in the same
    /// round-trip it asked for the change.
    ///
    /// R1452 — writes the stored size whatever the mode, but a `Stretch` or `ResizeToContents`
    /// section keeps deriving its size, so the write is only visible after a
    /// switch back. That is the toolkit (`resizeSection` "has no effect" outside `Interactive` /
    /// `Fixed`), plus the stored value kept rather than discarded; the return is
    /// the size the section actually has, so a client is never told a number
    /// the grid is not painting.
    pub fn resize_section(&self, logical: usize, size: u32) -> u32 {
        if logical >= self.count {
            return 0;
        }
        // R1494 — a programmatic resize is not part of a gesture, and it can
        // move a section the cascade remembers, which would make the
        // remembered size a lie. Ending the gesture is cheaper and more honest
        // than tracking which writes invalidate which entry.
        self.cascade.set(None);
        self.sizes.set_width(logical, size);
        self.section_size(logical)
    }

    /// R1493 — the size a section takes when nothing else determined it —
    /// the toolkit's `defaultSectionSize`.
    ///
    /// Clamped into the R1492 bounds on the way out, so it can never name a
    /// size the header would refuse: lower the ceiling under the default and
    /// the default comes down with it, with no write path that could have
    /// forgotten to. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn default_section_size(&self) -> u32 {
        self.sizes.clamp(self.default_size.get())
    }

    /// R1493 — set the default and apply it — the toolkit's `setDefaultSectionSize()`, which does
    /// both: the new default governs sections acquired later, *and* every
    /// section already there takes it now.
    ///
    /// Hidden sections keep their size, which is the toolkit's rule and
    /// already this module's ("the section keeps its visual place and its size
    /// while hidden") — a bulk resize a user cannot see is a resize they
    /// cannot undo.
    ///
    /// Returns the applied default after the bound clamp, so a client learns
    /// the outcome in the round-trip it asked for the change.
    pub fn set_default_section_size(&self, size: u32) -> u32 {
        // R1494 — a bulk write past every remembered size (see `resize_section`).
        self.cascade.set(None);
        self.default_size.set(size);
        let applied = self.default_section_size();
        let hidden = self.hidden.get();
        for logical in 0..self.count {
            if !hidden.get(logical).copied().unwrap_or(false) {
                self.sizes.set_width(logical, applied);
            }
        }
        applied
    }

    /// R1493 — back to [`DEFAULT_SECTION_SIZE`] — the toolkit's `resetDefaultSectionSize()`. The toolkit needs the
    /// separate call because its default is style-derived and a caller cannot
    /// name it; the same holds here for a caller that never learned the
    /// constant.
    pub fn reset_default_section_size(&self) -> u32 {
        self.set_default_section_size(DEFAULT_SECTION_SIZE)
    }

    /// R1504 — where a section's label sits when the model has no opinion:
    /// The toolkit's `defaultAlignment`. [`DEFAULT_HEADER_ALIGNMENT`] on a
    /// fresh header.
    #[must_use]
    pub fn default_alignment(&self) -> TextAlign {
        self.default_alignment.get()
    }

    /// R1504 — set the header's rule — the toolkit's `setDefaultAlignment`. Sections carrying an
    /// exception keep it; this is the fallback, not an override.
    pub fn set_default_alignment(&self, align: TextAlign) {
        self.default_alignment.set(align);
    }

    /// R1504 — the exception this **logical** section carries, if any: the
    /// answer the toolkit's model gives to `headerData(section, …, TextAlignmentRole)`. `None` means the section defers
    /// to [`default_alignment`](Self::default_alignment).
    ///
    /// Out of range answers `None` rather than a guess — a section that does
    /// not exist carries no exception, and inventing one would be a value from
    /// outside the domain, which this surface refuses to do (R1501).
    #[must_use]
    pub fn section_alignment_override(&self, logical: usize) -> Option<TextAlign> {
        self.section_alignments
            .get()
            .get(logical)
            .copied()
            .flatten()
    }

    /// R1504 — what the section's label is actually painted with: its own
    /// exception if the model gave one, otherwise the header's rule.
    ///
    /// The stored/effective pair this file already speaks in `sizes` /
    /// `section_sizes` and `resize_modes` / `effective_resize_modes`. `None`
    /// for a section outside `0..count`, for the reason above.
    #[must_use]
    pub fn section_alignment(&self, logical: usize) -> Option<TextAlign> {
        (logical < self.count).then(|| {
            self.section_alignment_override(logical)
                .unwrap_or_else(|| self.default_alignment())
        })
    }

    /// R1504 — give a **logical** section its own alignment, or `None` to hand
    /// it back to the header's rule. `false` when the index is out of range,
    /// which is how every other per-section writer here reports it.
    pub fn set_section_alignment(&self, logical: usize, align: Option<TextAlign>) -> bool {
        if logical >= self.count {
            return false;
        }
        let mut v = self.section_alignments.get();
        if v.len() < self.count {
            v.resize(self.count, None);
        }
        v[logical] = align;
        self.section_alignments.set(v);
        true
    }

    /// R1504 — the `set_section_alignment` channel's body, lifted out of
    /// [`invoke`](Self::invoke) so that match stays inside the line budget the
    /// workspace lints for. Same split R1494 made for `section_and_size`.
    ///
    /// # Errors
    ///
    /// [`InvokeError::Rejected`] when the payload does not parse or names a
    /// section that does not exist.
    fn invoke_set_section_alignment(&self, text: &str) -> Result<IntrospectValue, InvokeError> {
        let (logical, align) = section_and_alignment(text)?;
        // R1564 — the precondition is checked BEFORE the call rather than
        // inferred from the returned `bool` afterwards: the setter answers
        // `false` on exactly one condition, so that condition is the reason,
        // and reading it off a bool would leave the sentence guessing at what
        // the setter already knew.
        self.require_section("set_section_alignment", logical)?;
        let applied = self.set_section_alignment(logical, align);
        debug_assert!(applied, "require_section is the setter's only false path");
        Ok(self.query("alignments").unwrap_or(IntrospectValue::Null))
    }

    /// R1510 — whether a section the selection reaches is highlighted: the
    /// toolkit's `highlightSections`. `false` on a fresh header, as in the toolkit.
    #[must_use]
    pub fn highlight_sections(&self) -> bool {
        self.highlight.get()
    }

    /// R1510 — set the rule — the toolkit's `setHighlightSections`. The published selection is
    /// untouched: turning the rule off stops the header painting a selection
    /// it still knows about, which is exactly what makes
    /// [`section_selection`](Self::section_selection) and [`section_highlight`](Self::section_highlight) two
    /// different questions.
    pub fn set_highlight_sections(&self, highlight: bool) {
        self.highlight.set(highlight);
    }

    /// R1510 — how much of this **logical** section the consumer says the
    /// selection covers, whether or not the header may paint it.
    ///
    /// Out of range answers `None`, not
    /// [`Unselected`](SectionSelection::Unselected): a section that does not
    /// exist has no coverage, and answering with a member of the domain would
    /// be the R1501 defect — a plausible value from outside it.
    #[must_use]
    pub fn section_selection(&self, logical: usize) -> Option<SectionSelection> {
        self.selection.get().get(logical).copied()
    }

    /// R1510 — what the header paints for this **logical** section: the
    /// published selection when the rule permits it, and
    /// [`Unselected`](SectionSelection::Unselected) when it does not.
    ///
    /// The effective half of the pair, like [`section_alignment`](Self::section_alignment)
    /// against its override and `section_size` against `sizes`. The toolkit gates both of its
    /// flags on `highlightSections` in `paintSection` for the same reason: the selection is still there,
    /// the header is simply not dressing it.
    #[must_use]
    pub fn section_highlight(&self, logical: usize) -> Option<SectionSelection> {
        let published = self.section_selection(logical)?;
        Some(if self.highlight_sections() {
            published
        } else {
            SectionSelection::Unselected
        })
    }

    /// R1510 — publish one **logical** section's coverage. `false` when the
    /// index is out of range, like every other per-section writer here.
    pub fn set_section_selection(&self, logical: usize, selection: SectionSelection) -> bool {
        if logical >= self.count {
            return false;
        }
        let mut v = self.selection.get();
        if v.len() < self.count {
            v.resize(self.count, SectionSelection::Unselected);
        }
        v[logical] = selection;
        self.selection.set(v);
        true
    }

    /// R1510 — publish the whole row at once, the way a consumer that just
    /// recomputed its selection has it. A vector of the wrong length is
    /// ignored, for the reason
    /// [`set_content_widths`](Self::set_content_widths) ignores one: half an
    /// answer from another grid's selection is worse than none.
    pub fn set_selections(&self, selections: Vec<SectionSelection>) {
        if selections.len() == self.count {
            self.selection.set(selections);
        }
    }

    /// R1510 — one of the two selection rows as wire words, in logical order.
    ///
    /// Both rows are the same walk over the same domain and differ only in which
    /// accessor answers, so they are one function: a second copy is where the
    /// published row and the painted row would learn to disagree about their
    /// length or their ordering.
    fn selection_row(
        &self,
        of: impl Fn(&Self, usize) -> Option<SectionSelection>,
    ) -> IntrospectValue {
        IntrospectValue::Json(serde_json::Value::Array(
            (0..self.count)
                .filter_map(|l| of(self, l))
                .map(|s| serde_json::Value::String(s.as_wire().to_string()))
                .collect(),
        ))
    }

    /// R1510 — the `set_section_selection` channel's body, lifted out of
    /// [`invoke`](Self::invoke) so that match stays inside the line budget the
    /// workspace lints for. Same split R1504 made for `set_section_alignment`
    /// and R1494 for `section_and_size`.
    ///
    /// Answers the EFFECTIVE row, not the published one: a client that just
    /// selected a column wants to know whether the header is painting it, and
    /// under a `false` rule the two rows differ.
    ///
    /// # Errors
    ///
    /// [`InvokeError::Rejected`] when the payload does not parse, names a
    /// spelling this build does not know, or names a section that does not
    /// exist.
    fn invoke_set_section_selection(&self, text: &str) -> Result<IntrospectValue, InvokeError> {
        let (logical, spelling) =
            require_pair::<usize, String>("set_section_selection", text, ':')?;
        let selection: SectionSelection = spelling.parse().map_err(|()| {
            InvokeError::rejected(format!(
                "set_section_selection: {spelling:?} is not a section-selection spelling"
            ))
        })?;
        // R1564 — precondition before the call; see `invoke_set_section_alignment`.
        self.require_section("set_section_selection", logical)?;
        let applied = self.set_section_selection(logical, selection);
        debug_assert!(applied, "require_section is the setter's only false path");
        Ok(self.query("highlights").unwrap_or(IntrospectValue::Null))
    }

    /// R1494 — whether an interactive resize takes its space from the
    /// following sections instead of from the row's width — the toolkit's
    /// `cascadingSectionResizes`. `false` by default, as in the toolkit.
    #[must_use]
    pub fn cascading_section_resizes(&self) -> bool {
        self.cascading.get()
    }

    /// R1494 — turn cascading on or off. Turning it **off** also drops the
    /// cascade in flight: the memory exists to repay followers during a
    /// gesture, and a gesture that is no longer cascading has no debt to
    /// settle. Leaving it would repay on the next resize, long after the rule
    /// that incurred it was withdrawn.
    pub fn set_cascading_section_resizes(&self, on: bool) {
        self.cascading.set(on);
        if !on {
            self.cascade.set(None);
        }
    }

    /// R1498 — whether the section painted last absorbs the leftover viewport
    /// width — the toolkit's `stretchLastSection`. `false` by default, as in the toolkit.
    #[must_use]
    pub fn stretch_last_section(&self) -> bool {
        self.stretch_last.get()
    }

    /// R1498 — turn the rule on or off.
    ///
    /// Nothing is written to any section: the fill is resolved in
    /// [`visible_placements`](Self::visible_placements) from the stored widths, so turning the
    /// rule off restores what the last section had by construction. The
    /// toolkit has to remember a `lastSectionSize` because the toolkit writes the stretched
    /// width into the section; this module keeps the stored size and the
    /// painted size apart already (R1493), and that split is what makes the
    /// memory unnecessary here.
    pub fn set_stretch_last_section(&self, on: bool) {
        self.stretch_last.set(on);
    }

    /// The logical section painted **last** — the one
    /// [`stretch_last_section`](Self::stretch_last_section) gives the leftover
    /// to. `None` when every section is hidden.
    ///
    /// Walks the permutation from the end rather than reading
    /// [`visible_placements`](Self::visible_placements), which is the walk that
    /// asks this question: going through the placements would be infinite
    /// recursion. It needs no sizes to answer, only the order and the hidden
    /// flags, which is why it can be answered first.
    fn last_visible_section(&self) -> Option<usize> {
        let hidden = self.hidden.get();
        self.sections
            .order()
            .into_iter()
            .rev()
            .find(|&l| !hidden.get(l).copied().unwrap_or(false))
    }

    /// R1498 — the mode the layout actually applies to a section, as distinct
    /// from the one that was **set** on it ([`resize_mode`](Self::resize_mode)).
    ///
    /// The two differ only under `stretchLastSection`, whose documented the toolkit behaviour
    /// is exactly this: "this property will override the resize mode set on
    /// the last section in the header". Readable for the R1492 reason the
    /// bounds are — a client that watches an interactive resize come back
    /// unchanged can otherwise not tell a `Fixed` section from a filled one — and
    /// it is the same stored/effective pair `sizes` and `section_sizes` already form.
    #[must_use]
    pub fn effective_resize_mode(&self, logical: usize) -> SectionResizeMode {
        stretch_last_override(
            self.resize_mode(logical),
            self.last_visible_section() == Some(logical),
            self.stretch_last.get(),
        )
    }

    /// Whether logical section `logical` can be squeezed to pay for a neighbour's
    /// growth: the toolkit takes only from `Interactive` sections, and a hidden section
    /// is painted nowhere so it has no width to give.
    ///
    /// R1498 — against the **effective** mode. A section the last-section rule
    /// is filling derives its width like any other `Stretch` section, so
    /// squeezing its stored size would move no pixels while the cascade counted
    /// the debt as paid.
    fn is_cascadable(&self, logical: usize) -> bool {
        self.effective_resize_mode(logical) == SectionResizeMode::Interactive
            && !self.is_section_hidden(logical)
    }

    /// R1494 — resize a section the way a **drag** does — the toolkit's
    /// interactive resize, which is where `cascadingSectionResizes` applies.
    ///
    /// With cascading off this is exactly [`resize_section`](Self::resize_section), which
    /// is also the toolkit: `resizeSection()` never cascades, and the property governs
    /// "interactive resizing" only. With it on, the space comes from the
    /// **following** sections rather than from the row's total width — growing
    /// a section squeezes the ones after it, each down to the floor, in visual
    /// order; shrinking it hands that space back to the same sections,
    /// most-recently-squeezed first, never past what they held before they
    /// paid.
    ///
    /// Only `Interactive`, visible sections pay. A `Fixed` section is fixed
    /// against a neighbour's drag as much as against its own, a `Stretch` or
    /// `ResizeToContents` section derives its size and has no stored width to
    /// give, and a hidden section is painted nowhere.
    ///
    /// The followers pay **as far as they can**. When they are all at the
    /// floor the section still grows and the row grows with it, which is
    /// honest rather than silently refusing a resize the user asked for;
    /// [`visible_total`](Self::visible_total) reports the result either way.
    ///
    /// Returns the size the anchor actually has afterwards, the same
    /// read-outcome contract [`resize_section`](Self::resize_section) has.
    ///
    /// This mirrors the *documented* behaviour of the toolkit's property — the
    /// space a resize needs is taken from the following sections — not the
    /// toolkit's private multi-anchor bookkeeping, which pinion has no gesture
    /// to exercise.
    pub fn interactive_resize_section(&self, logical: usize, size: u32) -> u32 {
        if logical >= self.count {
            return 0;
        }
        if !self.cascading.get() {
            return self.resize_section(logical, size);
        }
        let before = self.sizes.width(logical);
        let target = self.sizes.clamp(size);
        // A different anchor is a different gesture: the old debt belonged to
        // the section that incurred it, and repaying it out of this one's
        // travel would move sections the user is no longer touching.
        let mut cascade = match self.cascade.get() {
            Some(c) if c.anchor == logical => c,
            _ => Cascade {
                anchor: logical,
                victims: Vec::new(),
            },
        };
        match target.cmp(&before) {
            core::cmp::Ordering::Greater => {
                self.take_from_followers(logical, target - before, &mut cascade);
            }
            core::cmp::Ordering::Less => {
                self.repay_followers(before - target, &mut cascade);
            }
            core::cmp::Ordering::Equal => {}
        }
        self.sizes.set_width(logical, target);
        self.cascade.set(Some(cascade));
        self.section_size(logical)
    }

    /// Squeeze `owed` pixels out of the sections after `anchor`, in visual
    /// order, each down to the floor, recording what every one of them held
    /// before it paid. Stops as soon as the debt is covered.
    fn take_from_followers(&self, anchor: usize, owed: u32, cascade: &mut Cascade) {
        let Some(from) = self.visual_index(anchor) else {
            return;
        };
        let floor = self.sizes.min_width();
        let order = self.sections.order();
        let mut owed = owed;
        for &victim in order.iter().skip(from + 1) {
            if owed == 0 {
                break;
            }
            if !self.is_cascadable(victim) {
                continue;
            }
            let held = self.sizes.width(victim);
            let can_give = held.saturating_sub(floor);
            if can_give == 0 {
                continue;
            }
            let given = can_give.min(owed);
            // Remember the FIRST size this section held in this gesture. A
            // second squeeze must not overwrite the original with the
            // already-reduced one, or the repayment would stop short of where
            // the drag began.
            if !cascade.victims.iter().any(|(l, _)| *l == victim) {
                cascade.victims.push((victim, held));
            }
            self.sizes.set_width(victim, held - given);
            owed -= given;
        }
    }

    /// Hand `freed` pixels back to the sections that paid, most-recently
    /// squeezed first, none of them past the size it held before it paid.
    fn repay_followers(&self, freed: u32, cascade: &mut Cascade) {
        let mut freed = freed;
        while freed > 0 {
            let Some(&(victim, owed_size)) = cascade.victims.last() else {
                break;
            };
            let held = self.sizes.width(victim);
            let wanted = owed_size.saturating_sub(held);
            if wanted == 0 {
                cascade.victims.pop();
                continue;
            }
            let given = wanted.min(freed);
            self.sizes.set_width(victim, held + given);
            freed -= given;
            if given == wanted {
                cascade.victims.pop();
            }
        }
    }

    /// Whether logical section `logical` is hidden — the toolkit's
    /// `isSectionHidden()`.
    #[must_use]
    pub fn is_section_hidden(&self, logical: usize) -> bool {
        self.hidden.get().get(logical).copied().unwrap_or(false)
    }

    /// Show or hide logical section `logical` — the toolkit's `setSectionHidden()`. The section keeps
    /// its visual place and its size while hidden. An out-of-range section is
    /// a silent no-op.
    pub fn set_section_hidden(&self, logical: usize, hidden: bool) {
        if logical >= self.count {
            return;
        }
        self.hidden.set_with(|h| {
            let mut next = h.clone();
            if let Some(slot) = next.get_mut(logical) {
                *slot = hidden;
            }
            next
        });
    }

    /// How many sections are hidden — the toolkit's `hiddenSectionCount()`.
    #[must_use]
    pub fn hidden_section_count(&self) -> usize {
        self.hidden.get().iter().filter(|h| **h).count()
    }

    /// **The** projection: every painted section, in visual order, with the
    /// three facts a consumer needs about it — where it sits in the
    /// permutation (`visual`, its hit-test identity), which column it is
    /// (`logical`, its data), and the geometry the header, the body cells, the
    /// insertion line, and the a11y tree all place themselves by.
    ///
    /// Hiding is applied here and nowhere else, the cumulative `x` is summed
    /// here and nowhere else, and (R1452) the resize modes are resolved here
    /// and nowhere else — every other derived answer below reads this walk
    /// instead of repeating it, so a consumer painting a body cell under its
    /// header cannot compute a different offset than the header did.
    ///
    /// The `Stretch` division needs the whole painted row at once (a share
    /// depends on what every other section took), which is why the sizes are
    /// resolved in this walk rather than per section.
    #[must_use]
    pub fn visible_placements(&self) -> Vec<SectionPlacement> {
        let hidden = self.hidden.get();
        let modes = self.modes.get();
        // R1498 — both facts the last-section rule needs, read once for the
        // whole walk rather than per section.
        let stretch_last = self.stretch_last.get();
        let last_visible = self.last_visible_section();
        // Pass 1 — who is painted, in what mode, at what size of their own.
        let mut painted: Vec<(usize, usize, SectionResizeMode, u32)> =
            Vec::with_capacity(self.count);
        for (visual, logical) in self.sections.order().into_iter().enumerate() {
            if hidden.get(logical).copied().unwrap_or(false) {
                continue;
            }
            let mode = stretch_last_override(
                modes.get(logical).copied().unwrap_or_default(),
                last_visible == Some(logical),
                stretch_last,
            );
            painted.push((visual, logical, mode, self.base_size(logical, mode)));
        }

        // Pass 2 — what the stretch sections have to divide. `None` available
        // width means nothing was published to divide, so a `Stretch` section
        // keeps its stored size instead of collapsing.
        let stretch_count = painted
            .iter()
            .filter(|(_, _, m, _)| *m == SectionResizeMode::Stretch)
            .count();
        let shares = self
            .available_width
            .get()
            .filter(|_| stretch_count > 0)
            .map(|available| {
                let taken: u32 = painted
                    .iter()
                    .filter(|(_, _, m, _)| *m != SectionResizeMode::Stretch)
                    .map(|(_, _, _, s)| *s)
                    .sum();
                let left = available.saturating_sub(taken);
                let n = u32::try_from(stretch_count).unwrap_or(1).max(1);
                // The remainder cannot be dropped or the row would not fill the
                // width it was told to fill; it goes to the leading stretch
                // sections, one pixel each, so the result is deterministic.
                (left / n, left % n)
            });

        // Pass 3 — place them.
        let mut x = 0;
        let mut stretch_seen = 0u32;
        let mut out = Vec::with_capacity(painted.len());
        for (visual, logical, mode, own) in painted {
            let size = match (mode, shares) {
                (SectionResizeMode::Stretch, Some((share, extra))) => {
                    let bonus = u32::from(stretch_seen < extra);
                    stretch_seen += 1;
                    // R1492 — the same clamp the other two paths use. A share
                    // is a derived size like any other, and this site is why
                    // the rule had to be lifted rather than repeated: it knew
                    // the floor and would not have learned the ceiling.
                    self.sizes.clamp(share + bonus)
                }
                _ => own,
            };
            out.push(SectionPlacement {
                visual,
                logical,
                x,
                size,
            });
            x += size;
        }
        out
    }

    /// The logical sections that are actually painted, in visual order.
    #[must_use]
    pub fn visible_sections(&self) -> Vec<usize> {
        self.visible_placements()
            .iter()
            .map(|p| p.logical)
            .collect()
    }

    /// The painted widths, in visual order with hidden sections dropped —
    /// exactly `TableData::col_widths`' contract, so the paint layer needs no
    /// knowledge of the header layout at all.
    #[must_use]
    pub fn visible_widths(&self) -> Vec<u32> {
        self.visible_placements().iter().map(|p| p.size).collect()
    }

    /// Sum of the painted widths — the grid's content width, what the R784
    /// horizontal scroll measures against.
    #[must_use]
    pub fn visible_total(&self) -> u32 {
        self.visible_placements().last().map_or(0, |p| p.x + p.size)
    }

    /// The x offset logical section `logical` is painted at — the toolkit's
    /// `sectionPosition()`. `None` when the section is hidden or unknown
    /// (a hidden section is painted nowhere, so it has no position).
    #[must_use]
    pub fn section_position(&self, logical: usize) -> Option<u32> {
        self.visible_placements()
            .iter()
            .find(|p| p.logical == logical)
            .map(|p| p.x)
    }

    /// Which logical section covers header x offset `x` — the toolkit's
    /// `logicalIndexAt()`. Reads the painted geometry, so it is correct for
    /// non-uniform widths and steps over hidden sections; `None` past the last
    /// painted section.
    #[must_use]
    pub fn logical_index_at(&self, x: u32) -> Option<usize> {
        self.visible_placements()
            .iter()
            .find(|p| x >= p.x && x < p.x + p.size)
            .map(|p| p.logical)
    }

    /// The whole layout as data — the toolkit's `saveState()`, readable.
    #[must_use]
    pub fn save_state(&self) -> ColumnLayoutState {
        ColumnLayoutState {
            order: self.sections.order(),
            // The STORED sizes, not the resolved ones: a saved layout has to
            // restore what the user set, and a `Stretch` section's painted
            // width belongs to the viewport it was painted in, not to the
            // layout.
            sizes: (0..self.count).map(|l| self.sizes.width(l)).collect(),
            hidden: self.hidden.get(),
            modes: self.modes.get(),
            sort_indicator: self.sort_indicator.get(),
            sort_indicator_shown: self.sort_indicator_shown.get(),
            // R1493 — the rules, saved beside the outcomes they produced. The
            // clamped default, for the same reason `sizes` are the clamped
            // widths: a snapshot records what the header will do, not what it
            // was asked for.
            default_section_size: self.default_section_size(),
            min_section_size: self.sizes.min_width(),
            max_section_size: self.sizes.max_width(),
            cascading_section_resizes: self.cascading.get(),
            stretch_last_section: self.stretch_last.get(),
            // R1496 — the permissions travel with the layout they permit. The
            // press in flight does not: that is gesture state, like the
            // cascade above, and the toolkit saves neither.
            sections_movable: self.movable.get(),
            sections_clickable: self.clickable.get(),
            resize_contents_precision: self.contents_precision.get(),
            // R1504 — the header's rule travels; the per-section exceptions do
            // NOT, because they are the model's and the toolkit's `saveState()` does not
            // carry them either. A restore therefore hands back a header whose
            // sections all defer to the rule, which is exactly what restoring
            // a header without its model should mean.
            default_alignment: self.default_alignment.get(),
            // R1510 — the permission to highlight, and never the selection it
            // would highlight. The toolkit's `write()` carries `highlightSelected` and has no access
            // to the view's selection model at all.
            highlight_sections: self.highlight.get(),
        }
    }

    /// Restore a saved layout — the toolkit's `restoreState()`. Refused, with **no change
    /// at all**, when `state` does not describe this header.
    ///
    /// Atomic by construction rather than by a pre-check copy: the length
    /// tests are cheap and total, and
    /// [`ReorderModel::set_order`] is itself validate-then-apply, so a
    /// rejected permutation returns before any size or flag is written. The
    /// permutation rule is therefore still checked in exactly one place.
    ///
    /// # Errors
    ///
    /// R1565 — [`InterveneError::OutOfRange`] naming **which** of the five guards refused: a wrong `sizes`
    /// / `hidden` / `modes` length, a sort indicator on a section this header lacks, a
    /// crossed bound pair, or an `order` that is not a permutation of `0..count`. The
    /// toolkit's `restoreState()` answers `bool` for the same five, which is the shape this
    /// returned until R1565 and the reason a refused restore told a client
    /// nothing about a seven-field snapshot.
    pub fn restore_state(&self, state: &ColumnLayoutState) -> Result<(), InterveneError> {
        // R1565 — `Result` rather than `bool`. This function refuses for FIVE
        // distinct reasons, each already explained by a comment beside its
        // guard, and every one of them left by the same `false` — so a client
        // whose restore was refused learned only that something about a
        // seven-field snapshot was wrong. That is PINION-PR82's complaint one
        // level below the wire, and the sentence it now returns rides straight
        // out through `intervene`.
        for (what, len) in [
            ("sizes", state.sizes.len()),
            ("hidden", state.hidden.len()),
            ("modes", state.modes.len()),
        ] {
            if len != self.count {
                return Err(row_len(what, len, self.count));
            }
        }
        // R1491 — an indicator on a section this header does not have is the
        // same class of error as a wrong vector length, and is checked here
        // with them so the restore stays atomic. `from_json` cannot make this
        // call: it decodes a snapshot without knowing which header it is for.
        if let Some((logical, _)) = state.sort_indicator.filter(|(l, _)| *l >= self.count) {
            return Err(InterveneError::out_of_range(format!(
                "the saved sort indicator names section {logical}, and this \
                 header has {}",
                self.count
            )));
        }
        // R1493 — an inverted bound pair describes no header, and is refused
        // here with the other shape errors rather than repaired on the way in.
        // Repairing it would make the restore order-dependent: the two setters
        // drag each other to stay ordered, so applying a crossed pair floor-
        // first and ceiling-first land on different bounds. A restore that
        // depends on the order its own fields are written is not a restore.
        if state.min_section_size > state.max_section_size {
            return Err(InterveneError::out_of_range(format!(
                "the saved bounds cross: min {} is above max {}",
                state.min_section_size, state.max_section_size
            )));
        }
        // R1565.1 — the length first, because `set_order` answers `false` for
        // it AND for a non-permutation; the two are different repairs.
        if state.order.len() != self.count {
            return Err(row_len("order", state.order.len(), self.count));
        }
        if !self.sections.set_order(&state.order) {
            return Err(InterveneError::out_of_range(format!(
                "the saved order {:?} is not a permutation of 0..{}: an id \
                 repeats or is out of range",
                state.order, self.count
            )));
        }
        // The bounds go in BEFORE the widths they shape. The other order
        // clamps the restored sizes through the *outgoing* header's bounds and
        // loses whatever fell outside them — the incoming ceiling can no longer
        // widen a width that was already truncated to the old one.
        self.sizes.set_min_width(state.min_section_size);
        self.sizes.set_max_width(state.max_section_size);
        self.default_size.set(state.default_section_size);
        // R1494 — through the setter, so turning cascading off here drops the
        // cascade in flight the same way it does anywhere else. The sizes it
        // remembered are being replaced wholesale regardless, so it is cleared
        // either way.
        self.set_cascading_section_resizes(state.cascading_section_resizes);
        self.cascade.set(None);
        // R1498 — read-time only, so it needs no place in the ordering the
        // bounds above have to keep: it changes what is painted, never what is
        // stored.
        self.stretch_last.set(state.stretch_last_section);
        // R1504 — the rule comes back, and the exceptions are DROPPED rather
        // than kept: the snapshot does not carry them (the toolkit's does not
        // either), so leaving the outgoing header's exceptions in place would
        // let a restore paint a column with an alignment the restored state
        // never mentioned. Restoring a header without its model means every
        // section defers to the rule.
        self.default_alignment.set(state.default_alignment);
        self.section_alignments.set(vec![None; self.count]);
        // R1510 — the rule comes back, and the published selection is KEPT,
        // which is the opposite of what happens to the alignment exceptions
        // one line up. The two look alike — neither is in the snapshot — and
        // are owned by different objects. An alignment exception is *header*
        // data (the toolkit's model answers `headerData(TextAlignmentRole)`), so a header restored without
        // its model has none. A selection is not the header's at all: it
        // belongs to the view's selection model, which the toolkit's `restoreState()`
        // cannot reach and does not disturb. Clearing the user's selection
        // because they reloaded a column layout would be a surprise the
        // toolkit does not produce — and it would be inert as well as wrong,
        // because the consumer that owns the rows republishes the coverage on
        // the next frame.
        self.highlight.set(state.highlight_sections);
        self.sizes.set_widths(state.sizes.clone());
        self.hidden.set(state.hidden.clone());
        self.modes.set(state.modes.clone());
        self.sort_indicator.set(state.sort_indicator);
        self.sort_indicator_shown.set(state.sort_indicator_shown);
        // R1496 — through the setter, so a restore that revokes the sampling
        // bound cannot install a zero the header would never accept from a
        // caller.
        self.set_resize_contents_precision(state.resize_contents_precision);
        self.movable.set(state.sections_movable);
        self.clickable.set(state.sections_clickable);
        // A restore replaces the layout wholesale, so a press taken against the
        // outgoing one has nothing left to activate.
        self.pressed_section.set(None);
        Ok(())
    }

    /// R1501 — every path this layout answers, declared beside the dispatch
    /// that answers it, and composed with the embedded [`ReorderModel`]'s own
    /// declaration rather than restating it.
    ///
    /// **Why it is here and not in the consumer.** It was in the consumer:
    /// `hello-column-reorder` spelled ~40 of these names into its own
    /// `IntrospectSchema`, and three consecutive rounds that added a path to
    /// *this* module left that copy behind. Measured over the real wire before
    /// R1501, five surfaces answered that `$schema` did not mention —
    /// `stretch_last_section`, `effective_resize_modes`,
    /// `effective_resize_mode.<logical>` (R1498), `resize_contents_precision`
    /// (R1496) and `reset_default_section_size` (R1493). §2 #2 makes RPC the
    /// AI's primary path and `$schema` its discovery primitive, so a feature
    /// the surface will not admit to is a feature no client can find.
    ///
    /// The list is load-bearing, not documentary: [`query`](Self::query) is
    /// gated on it, so an arm added without a field here answers nothing and
    /// the round that adds it fails its own test instead of shipping an
    /// undiscoverable path.
    ///
    /// Reads and actions share the list because [`SchemaField`] does not yet
    /// distinguish them — its own doc says so, and inventing the distinction
    /// here would put a second vocabulary beside the one every other widget
    /// declares in.
    pub const SCHEMA_FIELDS: &'static [SchemaField] =
        &SCHEMA_FIELD_STORAGE as &'static [SchemaField];

    /// The declaration as the type the introspection surface consumes.
    /// [`query`](Self::query) reads it to decide whether a path is one of ours,
    /// and [`intervene`](Self::intervene) reads it to tell a path that is not
    /// writable from a path that does not exist.
    pub const SCHEMA: IntrospectSchema = IntrospectSchema::new(Self::SCHEMA_FIELDS);

    /// Header-layout slots for [`ExternalIntrospect::query`], layered over the
    /// reorder slots (`order` / `preview` / `focused_index` / `grabbed`),
    /// which fall through to the embedded [`ReorderModel`]:
    ///
    /// - `state` — the whole [`ColumnLayoutState`] (the toolkit `saveState`, readable)
    /// - `sizes` / `hidden` — the logical-keyed vectors. `sizes` is the
    ///   **stored** size — the one a restore replays; `section_sizes` (R1493)
    ///   is the **effective** one the header paints, and under `Stretch` /
    ///   `ResizeToContents` the two differ
    /// - `section_sizes` — the effective plural (R1493)
    /// - `default_section_size` (R1493)
    /// - `visible_sections` / `visible_widths` / `visible_total`
    /// - `placements` — the painted geometry ([`SectionPlacement`] per section)
    /// - `hidden_count`
    /// - `sort_indicator` / `sort_indicator_section` / `sort_indicator_order` /
    ///   `sort_indicator_shown` (R1491)
    /// - `min_section_size` / `max_section_size` (R1492)
    /// - `sections_movable` / `sections_clickable` (R1496) — the toolkit's two
    ///   interaction permissions, both of which `saveState()` carries
    /// - `stretch_last_section` (R1498) — the toolkit's rule that the last painted
    ///   section absorbs the leftover viewport
    /// - `highlight_sections` (R1510) — the toolkit's rule that a section the selection
    ///   reaches is highlighted, plus `selections` /
    ///   `section_selection.<logical>` (what the consumer published) and
    ///   `highlights` / `section_highlight.<logical>` (what the rule makes of
    ///   it, so an unhighlighted section can be told from an unselected one)
    /// - `resize_modes` / `resize_mode.<logical>` — the mode that was **set**;
    ///   `effective_resize_modes` / `effective_resize_mode.<logical>` (R1498)
    ///   is the one the layout applies, and the two differ under
    ///   `stretch_last_section`
    /// - `visual_index.<logical>` / `logical_index.<visual>`
    /// - `section_size.<logical>` / `section_hidden.<logical>` /
    ///   `section_position.<logical>` / `logical_index_at.<x>`
    ///
    /// `None` for anything else, so an embedding consumer's own slots take
    /// precedence exactly as they do over the reorder model's.
    #[must_use]
    pub fn query(&self, path: &str) -> Option<IntrospectValue> {
        fn json_of<T: Into<serde_json::Value>>(
            items: impl IntoIterator<Item = T>,
        ) -> IntrospectValue {
            IntrospectValue::Json(serde_json::Value::Array(
                items.into_iter().map(Into::into).collect(),
            ))
        }
        fn int(v: usize) -> IntrospectValue {
            IntrospectValue::Int(i64::try_from(v).unwrap_or(0))
        }
        fn opt_int(v: Option<usize>) -> IntrospectValue {
            v.map_or(IntrospectValue::Null, int)
        }

        // R1501 — the declaration decides what this surface answers. An arm
        // below that no [`SCHEMA_FIELDS`](Self::SCHEMA_FIELDS) entry addresses
        // is unreachable, so a path added without being declared fails in the
        // round that adds it rather than shipping as a surface `$schema` denies
        // exists. Costs one linear pass over ~50 `&'static str` per read, which
        // is a wire-rate path, not a frame-rate one.
        Self::SCHEMA.field_for(path)?;

        match path {
            "state" => Some(IntrospectValue::Json(self.save_state().to_json())),
            // R1501 — the section count, answered here rather than left to the
            // consumer so the parametric families above can name a domain this
            // surface publishes. A consumer that also answers it wins, and
            // agrees by construction: both report the same sections.
            "count" => Some(int(self.count)),
            // The STORED sizes — what a restore replays. Its effective peer is
            // `section_sizes`, and a client that wants the painted row wants
            // that one (R1493).
            "sizes" => Some(json_of((0..self.count).map(|l| self.sizes.width(l)))),
            "section_sizes" => Some(json_of(self.section_sizes())),
            "default_section_size" => {
                Some(IntrospectValue::Int(i64::from(self.default_section_size())))
            }
            "default_alignment" => Some(IntrospectValue::Text(
                self.default_alignment().as_wire().to_string(),
            )),
            // The effective alignment of every section, in logical order — the
            // peer of `section_sizes` against `sizes`.
            "alignments" => {
                Some(json_of((0..self.count).filter_map(|l| {
                    self.section_alignment(l).map(TextAlign::as_wire)
                })))
            }
            // R1510 — the rule, and the two rows it stands between: what the
            // consumer published, and what this header makes of it.
            "highlight_sections" => Some(IntrospectValue::Bool(self.highlight_sections())),
            "selections" => Some(self.selection_row(ColumnLayout::section_selection)),
            "highlights" => Some(self.selection_row(ColumnLayout::section_highlight)),
            "cascading_section_resizes" => {
                Some(IntrospectValue::Bool(self.cascading_section_resizes()))
            }
            // R1498 — the layout rule that is keyed by position rather than by
            // column, which is why no per-section slot can report it.
            "stretch_last_section" => Some(IntrospectValue::Bool(self.stretch_last_section())),
            // R1496 — the two permissions, readable so a client can tell a
            // header that refused a drag from one that has no drag to give.
            "sections_movable" => Some(IntrospectValue::Bool(self.sections_movable())),
            "sections_clickable" => Some(IntrospectValue::Bool(self.sections_clickable())),
            "hidden" => Some(json_of(self.hidden.get())),
            "visible_sections" => Some(json_of(self.visible_sections())),
            "visible_widths" => Some(json_of(self.visible_widths())),
            // The painted geometry as data — an agent aims a drag or a click
            // at a section from this without re-deriving a single offset, and
            // without a screenshot. The toolkit exposes the equivalent only
            // through per-section C++ calls against a live widget.
            "placements" => Some(IntrospectValue::Json(serde_json::Value::Array(
                self.visible_placements()
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "visual": p.visual,
                            "logical": p.logical,
                            "x": p.x,
                            "size": p.size,
                        })
                    })
                    .collect(),
            ))),
            "visible_total" => Some(IntrospectValue::Int(i64::from(self.visible_total()))),
            "hidden_count" => Some(int(self.hidden_section_count())),
            // R1452 — the sizing policy, and the two inputs the derived modes
            // read. `sizes` above is what is STORED; these say where a painted
            // width actually came from.
            "resize_modes" => Some(IntrospectValue::Json(serde_json::Value::Array(
                self.modes
                    .get()
                    .iter()
                    .map(|m| serde_json::Value::from(m.as_wire()))
                    .collect(),
            ))),
            // R1498 — the effective plural, beside the stored one for the same
            // reason `section_sizes` sits beside `sizes`: a client reading only
            // the plural was the R1493 defect, and a rule that overrides a mode
            // would have re-created it in the vocabulary next door.
            "effective_resize_modes" => Some(IntrospectValue::Json(serde_json::Value::Array(
                (0..self.count)
                    .map(|l| serde_json::Value::from(self.effective_resize_mode(l).as_wire()))
                    .collect(),
            ))),
            "content_widths" => Some(json_of(self.content_widths.get())),
            // R1491 — the header's own sort state. `sort_indicator` is the compound string
            // the grid proxy already speaks; `sort_indicator_ section` and `_order` are the toolkit's two
            // separate getters, kept because an agent filtering on "which
            // column" should not have to parse.
            "sort_indicator" => Some(IntrospectValue::Text(grid_sort_str(self.sort_indicator()))),
            "sort_indicator_section" => Some(opt_int(self.sort_indicator().map(|(l, _)| l))),
            "sort_indicator_order" => Some(IntrospectValue::Text(
                sort_dir_str(self.sort_indicator().map(|(_, d)| d)).to_string(),
            )),
            "sort_indicator_shown" => Some(IntrospectValue::Bool(self.is_sort_indicator_shown())),
            // R1492 — the bounds every size path applies. Readable is the
            // point: before this, a client could watch a resize get clamped and
            // had no way to learn the rule, so "you asked 5 and got 40" and
            // "you asked 300 of a stretch section and got 40" were the same
            // answer. With these two slots and `resize_mode`, the three causes
            // are distinguishable without a new channel.
            "min_section_size" => {
                Some(IntrospectValue::Int(i64::from(self.minimum_section_size())))
            }
            "max_section_size" => {
                Some(IntrospectValue::Int(i64::from(self.maximum_section_size())))
            }
            "resize_contents_precision" => Some(int(self.resize_contents_precision())),
            "available_width" => Some(
                self.available_width
                    .get()
                    .map_or(IntrospectValue::Null, |w| {
                        IntrospectValue::Int(i64::from(w))
                    }),
            ),
            // NB: no `?` in this arm — an early return here would skip the
            // reorder fall-through below, which is exactly how `order` first
            // came back `None` from a layout that holds one.
            _ => self.query_parametric(path),
        }
        .or_else(|| self.sections.query(path))
    }

    /// The `<slot>.<arg>` half of [`query`](Self::query) — every per-section
    /// read, each one `<head>.<index>`.
    ///
    /// (R1494) Split out because the two halves answer different shapes of
    /// question and only one of them grows when a per-section fact is added.
    /// `None` for an unknown head or an unparsable argument, so the caller's
    /// reorder fall-through still runs.
    fn query_parametric(&self, path: &str) -> Option<IntrospectValue> {
        fn opt_int(v: Option<usize>) -> IntrospectValue {
            v.map_or(IntrospectValue::Null, |n| {
                IntrospectValue::Int(i64::try_from(n).unwrap_or(0))
            })
        }
        // R1501 — every section-keyed family answers through this, so the
        // declared domain (`IndexOf("count")`) is enforced once instead of nine
        // times. Measured on a 3-section header before the round, five of the
        // nine answered a plausible value for section 3: `section_size` → `0`
        // (a width below this header's own floor), `section_hidden` → `false`
        // (a column that does not exist, reported visible), `resize_mode` and
        // `effective_resize_mode` → `"interactive"`, `content_width` → `0`.
        // The other four already answered `Null`, so the module knew the rule
        // and the accessors' `unwrap_or_default` quietly broke it on the way
        // out — the same read-forgot-what-the-write-kept shape R1487 and R1493
        // found in this file, now caught by the R1353.1 audit, which could only
        // reach the layout once the declaration lived here (R1501).
        let per_section = |arg: &str, f: &dyn Fn(usize) -> IntrospectValue| {
            arg.parse::<usize>().ok().map(|l| {
                if l < self.count {
                    f(l)
                } else {
                    IntrospectValue::Null
                }
            })
        };
        let (head, arg) = path.split_once('.')?;
        match head {
            "visual_index" => per_section(arg, &|l| opt_int(self.visual_index(l))),
            "logical_index" => per_section(arg, &|v| opt_int(self.logical_index(v))),
            "section_size" => per_section(arg, &|l| {
                IntrospectValue::Int(i64::from(self.section_size(l)))
            }),
            "section_hidden" => {
                per_section(arg, &|l| IntrospectValue::Bool(self.is_section_hidden(l)))
            }
            "section_position" => per_section(arg, &|l| {
                self.section_position(l).map_or(IntrospectValue::Null, |x| {
                    IntrospectValue::Int(i64::from(x))
                })
            }),
            // R1504 — the effective alignment, and the model's exception on its
            // own. `section_alignment` cannot answer `Null` for an in-range
            // section (every section is painted with something); the override
            // answers `Null` exactly when the section defers to the header,
            // which is the one bit a client cannot derive from the other.
            "section_alignment" => per_section(arg, &|l| {
                self.section_alignment(l)
                    .map_or(IntrospectValue::Null, |a| {
                        IntrospectValue::Text(a.as_wire().to_string())
                    })
            }),
            "section_alignment_override" => per_section(arg, &|l| {
                self.section_alignment_override(l)
                    .map_or(IntrospectValue::Null, |a| {
                        IntrospectValue::Text(a.as_wire().to_string())
                    })
            }),
            // R1510 — the published coverage, and the highlight the rule makes
            // of it. Neither answers `Null` in range: a section the selection
            // never reached is `"none"`, a state rather than an absence, which
            // is why this pair needs no sentinel the way the alignment override
            // above does.
            "section_selection" => per_section(arg, &|l| {
                self.section_selection(l)
                    .map_or(IntrospectValue::Null, |s| {
                        IntrospectValue::Text(s.as_wire().to_string())
                    })
            }),
            "section_highlight" => per_section(arg, &|l| {
                self.section_highlight(l)
                    .map_or(IntrospectValue::Null, |s| {
                        IntrospectValue::Text(s.as_wire().to_string())
                    })
            }),
            // Keyed by a PIXEL offset along the painted row, not by a section,
            // so `count` is not its bound — the declaration names
            // `visible_total`, and the accessor already answers `Null` for a
            // coordinate no section covers.
            "logical_index_at" => arg.parse().ok().map(|x| opt_int(self.logical_index_at(x))),
            "resize_mode" => per_section(arg, &|l| {
                IntrospectValue::Text(self.resize_mode(l).as_wire().to_string())
            }),
            "effective_resize_mode" => per_section(arg, &|l| {
                IntrospectValue::Text(self.effective_resize_mode(l).as_wire().to_string())
            }),
            "content_width" => per_section(arg, &|l| {
                IntrospectValue::Int(i64::from(self.content_width(l)))
            }),
            _ => None,
        }
    }

    /// One `count`-long width vector off the wire, for the two slots that take
    /// one (`sizes` and `content_widths`).
    ///
    /// (R1493) Extracted because the two arms were byte-identical but for the
    /// signal they wrote — the same "a wrong length is `OutOfRange`, a wrong
    /// shape is `TypeMismatch`" decision made twice. Two copies of a wire
    /// contract are two places for it to drift, and the second one is always
    /// the one that does not learn the next correction.
    /// A `"<logical>:<px>"` resize payload, range-checked against this header.
    ///
    /// (R1494) The two resize methods — the programmatic one and the
    /// interactive one cascading governs — differ only in which method they
    /// then call, so they take their argument through one parser. A second
    /// copy is a second place for `Rejected` to become `TypeMismatch`.
    fn section_and_size(&self, args: &IntrospectValue) -> Result<(usize, u32), InvokeError> {
        let IntrospectValue::Text(text) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (logical, size) = require_pair::<usize, u32>("resize_section", text, ':')?;
        self.require_section("resize_section", logical)?;
        Ok((logical, size))
    }

    /// R1565 — the `hidden` whole-row write's decode, lifted out of
    /// [`intervene`](Self::intervene) to keep that dispatch inside the
    /// workspace line ceiling (this round's reasons pushed it over). Peer of
    /// [`width_vector`](Self::width_vector), which the two width rows share.
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when the payload is not an array of
    /// booleans, and [`InterveneError::OutOfRange`] naming both counts when it
    /// is the wrong length for this header.
    fn hidden_vector(&self, value: &IntrospectValue) -> Result<Vec<bool>, InterveneError> {
        let IntrospectValue::Json(serde_json::Value::Array(items)) = value else {
            return Err(InterveneError::TypeMismatch);
        };
        let flags: Vec<bool> = items
            .iter()
            .map(serde_json::Value::as_bool)
            .collect::<Option<_>>()
            .ok_or(InterveneError::TypeMismatch)?;
        if flags.len() == self.count {
            Ok(flags)
        } else {
            Err(row_len("hidden flags", flags.len(), self.count))
        }
    }

    /// R1564 §5.15 (PINION-PR82) — the section-exists precondition every
    /// section-addressed `invoke` arm shares, stating what it found rather than
    /// that it found something wrong.
    ///
    /// Seven arms wrote `if logical >= self.count { return Err(Rejected) }`
    /// inline. That was tolerable while the refusal carried nothing — the seven
    /// copies were byte-identical, so they could not disagree. Once each has to
    /// compose a *sentence* they can, and the section count is exactly the fact
    /// a client cannot see from the refusal it is holding: `no section 7`
    /// leaves open whether the header has six or none.
    ///
    /// # Errors
    ///
    /// [`InvokeError::Rejected`] naming the method, the absent section and the
    /// header's extent.
    fn require_section(&self, method: &str, logical: usize) -> Result<(), InvokeError> {
        if logical >= self.count {
            return Err(InvokeError::rejected(format!(
                "{method}: no section {logical} in this header (it has {})",
                self.count
            )));
        }
        Ok(())
    }

    fn width_vector(&self, value: &IntrospectValue) -> Result<Vec<u32>, InterveneError> {
        let widths: Vec<u32> = json_u64_array(value)
            .ok_or(InterveneError::TypeMismatch)?
            .into_iter()
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
            .collect();
        if widths.len() == self.count {
            Ok(widths)
        } else {
            Err(row_len("widths", widths.len(), self.count))
        }
    }

    /// Header-layout slots for [`ExternalIntrospect::intervene`]: `state` is the restore half of the
    /// round-trip (the toolkit's `restoreState`, authorable), and `sizes` / `hidden` / `sort_indicator` / `sort_indicator_shown`
    /// write one field each. `focused_index` and `order` fall through to the embedded [`ReorderModel`].
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when the value is not the JSON shape
    /// the matching [`query`](Self::query) hands out,
    /// [`InterveneError::OutOfRange`] when it is well-shaped but not a valid
    /// layout (wrong length, or an `order` that is not a permutation), and
    /// [`InterveneError::UnknownPath`] otherwise.
    pub fn intervene(&self, path: &str, value: &IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "state" => {
                let IntrospectValue::Json(json) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let state =
                    ColumnLayoutState::from_json(json).ok_or(InterveneError::TypeMismatch)?;
                // R1565 — forwarded verbatim: `restore_state` knows which of
                // its five guards refused, and this arm used to replace that
                // with one vague sentence about the whole snapshot.
                self.restore_state(&state)
            }
            "sizes" => {
                self.sizes.set_widths(self.width_vector(value)?);
                Ok(())
            }
            "hidden" => {
                self.hidden.set(self.hidden_vector(value)?);
                Ok(())
            }
            // R1452 — the two inputs the derived modes read. A grid publishes
            // its measured content and its viewport here; over the wire an
            // agent can do the same to explore a layout without a real grid.
            "content_widths" => {
                self.content_widths.set(self.width_vector(value)?);
                Ok(())
            }
            // R1510 — the third such input, writable for exactly that reason.
            // header view has no selection setter — a toolkit client drives
            // the view's selection model — but the class boundary the toolkit
            // draws is not this surface's contract: §2 #2 makes the wire an
            // agent's primary path, and the two inputs above are already
            // writable here so an agent can explore a layout without a real
            // grid. An input it could read and never move would make the whole
            // rule unexplorable.
            "selections" => {
                let IntrospectValue::Json(serde_json::Value::Array(items)) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let row: Vec<SectionSelection> = items
                    .iter()
                    .map(|v| v.as_str()?.parse().ok())
                    .collect::<Option<_>>()
                    .ok_or(InterveneError::TypeMismatch)?;
                if row.len() != self.count {
                    return Err(row_len("selections", row.len(), self.count));
                }
                self.selection.set(row);
                Ok(())
            }
            // R1491 — the restore half for the header's own sort, both as the
            // compound string and as the shown flag. Strict on a malformed
            // string, unlike the older `GridSortExternal::intervene("sort")`, which reads one as "unsorted": the
            // two doors of THIS header must agree, and its other door (`state`)
            // reports the error. R1504 — the toolkit's `setDefaultAlignment`. A spelling this
            // build does not know is a TYPE error rather than a silent `Start`:
            // the strict reader is `TextAlign::from_wire`, and the lenient behaviour that exists
            // elsewhere is one decoder's documented choice, not this
            // channel's.
            "default_alignment" => {
                let IntrospectValue::Text(s) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let align = TextAlign::from_wire(s).ok_or(InterveneError::TypeMismatch)?;
                self.set_default_alignment(align);
                Ok(())
            }
            "sort_indicator" => {
                let IntrospectValue::Text(s) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let parsed = grid_sort_parse(s).ok_or(InterveneError::TypeMismatch)?;
                match parsed {
                    None => self.clear_sort_indicator(),
                    Some((logical, _)) if logical >= self.count => {
                        return Err(InterveneError::out_of_range(format!(
                            "no section {logical} in this header (it has {})",
                            self.count
                        )));
                    }
                    Some((logical, ascending)) => self.set_sort_indicator(logical, ascending),
                }
                Ok(())
            }
            "sort_indicator_shown" => {
                let IntrospectValue::Bool(shown) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                self.set_sort_indicator_shown(*shown);
                Ok(())
            }
            "available_width" => {
                // `Null` clears the published viewport — the writable peer of
                // the `Null` this slot reads back when nothing is published.
                if matches!(value, IntrospectValue::Null) {
                    self.set_available_width(None);
                } else {
                    let w = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                    self.set_available_width(Some(px_ceiling("available_width", w)?));
                }
                Ok(())
            }
            // NB: the scalar rules first, then the reorder model — a rule name
            // this header does not know is still the embedded model's to claim.
            //
            // R1501 — and what neither claims is now answered from the
            // declaration instead of as a flat "unknown". A path this surface
            // reads but does not write (`placements`, `visible_total`, `count`)
            // is `ReadOnly`; only a path nothing declares is `UnknownPath`. The
            // distinction was unavailable before there was a declaration to
            // consult, which is why every unwritable read reported itself as
            // nonexistent — the §2 #7 lie [`read_only_or_unknown`] exists for.
            _ => self.intervene_rule(path, value).unwrap_or_else(|| {
                match self.sections.intervene(path, value) {
                    Err(InterveneError::UnknownPath) => {
                        Err(read_only_or_unknown(&Self::SCHEMA, path))
                    }
                    other => other,
                }
            }),
        }
    }

    /// The scalar-rule half of [`intervene`](Self::intervene) — every write
    /// that sets one header-wide policy, each a decoder and a setter.
    /// `None` for anything else, so the caller's reorder fall-through runs.
    ///
    /// (R1498) Split out on the axis [`query_parametric`](Self::query_parametric) was: the
    /// two halves take different shapes of value, and only this one grows when
    /// the toolkit's next header property is added. Every round from R1492 on
    /// has added exactly one arm here.
    ///
    /// # Errors
    ///
    /// Propagates the decoders' [`InterveneError::TypeMismatch`] /
    /// [`InterveneError::OutOfRange`].
    fn intervene_rule(
        &self,
        path: &str,
        value: &IntrospectValue,
    ) -> Option<Result<(), InterveneError>> {
        // Each arm is `decode?` then a setter, so the `?` needs a fallible
        // body of its own; the closure is that body, run once.
        let apply = || -> Result<(), InterveneError> {
            match path {
                // R1492 — the toolkit's two setters. Writable for the same
                // reason the modes are: an agent explores a layout by moving
                // the rule, not only the numbers the rule applies to.
                "min_section_size" => self.set_minimum_section_size(px_bound(value)?),
                "max_section_size" => self.set_maximum_section_size(px_bound(value)?),
                // R1493 — the toolkit's `setDefaultSectionSize`, through the same door its two
                // bound siblings use, because it is the same kind of thing: a
                // scalar rule that shapes every section's size.
                "default_section_size" => {
                    self.set_default_section_size(px_bound(value)?);
                }
                // R1494 — the toolkit's `cascadingSectionResizes`, writable like the modes and the
                // bounds.
                "cascading_section_resizes" => {
                    self.set_cascading_section_resizes(bool_rule(value)?);
                }
                // R1498 — the toolkit's `setStretchLastSection`. The effective modes and every
                // painted width follow from it, so an agent moves the rule and
                // reads the consequence in one round trip.
                "stretch_last_section" => self.set_stretch_last_section(bool_rule(value)?),
                // R1496 — the toolkit's `setSectionsMovable` / `setSectionsClickable`. A permission an agent can
                // read but not move is one it cannot explore.
                "sections_movable" => self.set_sections_movable(bool_rule(value)?),
                "sections_clickable" => self.set_sections_clickable(bool_rule(value)?),
                // R1510 — the toolkit's `setHighlightSections`. The rule and the selection it
                // gates are written through different doors on purpose: this
                // one, and the `selections` vector beside `content_widths`, because they are
                // different kinds of thing — a permission the header owns, and
                // an input a consumer feeds it.
                "highlight_sections" => self.set_highlight_sections(bool_rule(value)?),
                // R1454 — the row-sampling bound a `ResizeToContents` consumer
                // honours; writable so an agent can shrink it and watch the
                // hints change without rebuilding the grid.
                "resize_contents_precision" => {
                    let rows = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                    self.set_resize_contents_precision(rows);
                }
                _ => return Err(InterveneError::UnknownPath),
            }
            Ok(())
        };
        match apply() {
            // The one error this half invents rather than decodes means "not
            // mine", and is the caller's cue to keep looking.
            Err(InterveneError::UnknownPath) => None,
            other => Some(other),
        }
    }

    /// Header-layout actions for [`ExternalIntrospect::invoke`] — the toolkit's own section vocabulary,
    /// each taking the typed pair wire form ([`require_pair`]):
    ///
    /// - `swap_sections` — `"<visual_a>:<visual_b>"`; returns the new order
    /// - `resize_section` — `"<logical>:<px>"`; returns the applied size
    ///   after the minimum-width clamp
    /// - `set_section_hidden` — `"<logical>:<bool>"`; returns the resulting
    ///   visible-section projection, so one round-trip both hides and reports
    ///   what is now painted
    /// - `set_sort_indicator` — `"<logical>:<bool>"` (R1491) /
    ///   `cycle_sort_indicator` — `<logical>` / `clear_sort_indicator`; each
    ///   returns the resulting indicator string
    ///
    /// - `send` — `"<visual>:<PointerEvent>"` (R1496); handled here for the
    ///   click and then passed down, so the reorder model still records the
    ///   press. Returns the **logical** section a press-release landed on,
    ///   `Null` otherwise
    ///
    /// `move` / `grab` / `grab_cancel` / `move_section` fall through to the
    /// embedded [`ReorderModel`].
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when the argument is not text,
    /// [`InvokeError::Rejected`] when the pair does not parse or names a
    /// section that does not exist, and [`InvokeError::UnknownPath`] for any
    /// other method.
    pub fn invoke(
        &self,
        method: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Only this module's three methods need the pair; anything else is the
        // reorder model's, so the text check stays inside each arm.
        let pair_text = |args: &IntrospectValue| match args {
            IntrospectValue::Text(t) => Ok(t.clone()),
            _ => Err(InvokeError::TypeMismatch),
        };
        match method {
            "swap_sections" => {
                let text = pair_text(args)?;
                let (a, b) = require_pair::<usize, usize>("swap_sections", &text, ':')?;
                self.require_section("swap_sections", a)?;
                self.require_section("swap_sections", b)?;
                self.swap_sections(a, b);
                Ok(self.query("order").unwrap_or(IntrospectValue::Null))
            }
            "resize_section" => {
                let (logical, size) = self.section_and_size(args)?;
                Ok(IntrospectValue::Int(i64::from(
                    self.resize_section(logical, size),
                )))
            }
            // R1494 — the drag's resize, as distinct from the programmatic one
            // above: this is the entry point `cascading_section_resizes` governs, because in the
            // toolkit the property applies to interactive resizing and `resizeSection()`
            // never cascades.
            "interactive_resize_section" => {
                let (logical, size) = self.section_and_size(args)?;
                Ok(IntrospectValue::Int(i64::from(
                    self.interactive_resize_section(logical, size),
                )))
            }
            "set_section_hidden" => {
                let text = pair_text(args)?;
                let (logical, hide) =
                    require_pair::<usize, bool>("set_section_hidden", &text, ':')?;
                self.require_section("set_section_hidden", logical)?;
                self.set_section_hidden(logical, hide);
                Ok(self
                    .query("visible_sections")
                    .unwrap_or(IntrospectValue::Null))
            }
            // R1504 — the toolkit's per-section `TextAlignmentRole`, as a channel. The value
            // half is a `TextAlign` spelling or the literal `default`, which is how a client
            // hands a section BACK to the header's rule; without it an
            // exception could be set and never cleared.
            "set_section_alignment" => self.invoke_set_section_alignment(&pair_text(args)?),
            // R1510 — one section's coverage, the per-section door beside the
            // whole-row `selections`.
            "set_section_selection" => self.invoke_set_section_selection(&pair_text(args)?),
            // R1452 — the toolkit's setSectionResizeMode, both overloads. Each
            // returns the resulting painted widths, because changing one
            // section's policy re-sizes every `Stretch` section sharing the row with
            // it — the outcome an agent needs is the row, not the section.
            "set_resize_mode" => {
                let text = pair_text(args)?;
                let (logical, mode) =
                    require_pair::<usize, SectionResizeMode>("set_resize_mode", &text, ':')?;
                self.require_section("set_resize_mode", logical)?;
                self.set_resize_mode(logical, mode);
                Ok(self
                    .query("visible_widths")
                    .unwrap_or(IntrospectValue::Null))
            }
            // R1491 — the toolkit's setSortIndicator, and the cycle a header
            // click performs. Both return the resulting indicator string
            // rather than nothing, so one round-trip both sorts and reports —
            // which matters most for the cycle, whose whole point is that the
            // caller does not know the direction it lands on.
            "set_sort_indicator" => {
                let text = pair_text(args)?;
                let (logical, ascending) =
                    require_pair::<usize, bool>("set_sort_indicator", &text, ':')?;
                self.require_section("set_sort_indicator", logical)?;
                self.set_sort_indicator(logical, ascending);
                Ok(self
                    .query("sort_indicator")
                    .unwrap_or(IntrospectValue::Null))
            }
            "cycle_sort_indicator" => {
                let logical = args.as_usize().ok_or(InvokeError::TypeMismatch)?;
                self.require_section("cycle_sort_indicator", logical)?;
                self.cycle_sort_indicator(logical);
                Ok(self
                    .query("sort_indicator")
                    .unwrap_or(IntrospectValue::Null))
            }
            "clear_sort_indicator" => {
                self.clear_sort_indicator();
                Ok(self
                    .query("sort_indicator")
                    .unwrap_or(IntrospectValue::Null))
            }
            "set_all_resize_modes" => {
                let IntrospectValue::Text(text) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let spelling = text.trim();
                let mode: SectionResizeMode = spelling.parse().map_err(|()| {
                    InvokeError::rejected(format!(
                        "set_all_resize_modes: {spelling:?} is not a section resize mode"
                    ))
                })?;
                self.set_all_resize_modes(mode);
                Ok(self
                    .query("visible_widths")
                    .unwrap_or(IntrospectValue::Null))
            }
            // R1493 — the toolkit's `resetDefaultSectionSize()`. An `invoke` rather than a second `intervene` slot
            // because it carries no value: the point is to reach the constant
            // WITHOUT naming it, which is exactly the caller who does not know
            // what it is.
            //
            // Answers `section_sizes`, not the default it just applied — a
            // bulk resize's outcome is the row, and under a `Stretch` header
            // the row is not the number that was written.
            "reset_default_section_size" => {
                self.reset_default_section_size();
                Ok(self.query("section_sizes").unwrap_or(IntrospectValue::Null))
            }
            "send" => self.invoke_send(args),
            _ => self.sections.invoke(method, args),
        }
    }
}

/// R1452 §5.27 — resolve the shared [`ColumnLayout`] for `key`, building it
/// once from `sizes` (the initial per-logical-section sizes). Mirrors
/// [`use_column_widths`](crate::widgets::column_widths::use_column_widths).
///
/// The header layout has **two** readers that must be the same instance: the
/// `External` that mutates it, and the view fn that publishes what only the
/// view knows — the measured content hints
/// ([`set_content_widths`](ColumnLayout::set_content_widths)) and the viewport
/// a `Stretch` row divides
/// ([`set_available_width`](ColumnLayout::set_available_width)). Owning it by
/// value inside the `External` would put those inputs out of reach; the
/// scope-id-keyed [`Owner::cache`] home is how every other interactive axis in
/// this crate is shared.
///
/// # Panics
///
/// When called outside an active [`Owner`] scope (a view fn or an `External`
/// factory both run inside one).
#[must_use]
pub fn use_column_layout(key: &'static str, sizes: impl FnOnce() -> Vec<u32>) -> Rc<ColumnLayout> {
    use_column_layout_with(key, || ColumnLayout::new(sizes()))
}

/// R1496 §5.27 — [`use_column_layout`] for a header that needs configuring at
/// **construction**: the general form, with the sizes-only one defined on top
/// of it.
///
/// The permissions R1496 added ([`set_sections_movable`](ColumnLayout::set_sections_movable),
/// [`set_sections_clickable`](ColumnLayout::set_sections_clickable)) are the reason it exists. They
/// default to the toolkit's `false`, so a header that wants them has to say so —
/// and it cannot say so from inside a view fn, because that runs on every pass
/// and would overwrite whatever the user or an agent had since written. `build`
/// runs once, on the pass that creates the layout.
///
/// # Panics
///
/// When called outside an active [`Owner`] scope, like [`use_column_layout`].
#[must_use]
pub fn use_column_layout_with(
    key: &'static str,
    build: impl FnOnce() -> ColumnLayout,
) -> Rc<ColumnLayout> {
    Owner::current()
        .expect("use_column_layout requires an active Owner scope")
        .cache(key, build)
}

/// R1492 — decode a pixel bound written over the wire. The two size bounds are
/// the same decode, and separating the *shape* error from the *value* error is
/// the part worth having once: a client that sent a string learns something
/// different from one that sent a number no width could be.
/// R1496 — decode a boolean header rule written over the wire. The peer of
/// [`px_bound`] for the flags, lifted on its third writer
/// (`cascading_section_resizes` + the two permissions): a rule that silently
/// accepted `1` on one path and refused it on another would be a wire the
/// client cannot learn.
fn bool_rule(value: &IntrospectValue) -> Result<bool, InterveneError> {
    match value {
        IntrospectValue::Bool(on) => Ok(*on),
        _ => Err(InterveneError::TypeMismatch),
    }
}

fn px_bound(value: &IntrospectValue) -> Result<u32, InterveneError> {
    let px = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
    px_ceiling("width", px)
}

/// R1565 §5.15 — a pixel value that fits the `u32` a section width is measured
/// in, stating the ceiling when it does not.
///
/// # Errors
///
/// [`InterveneError::OutOfRange`] naming the slot and the ceiling.
fn px_ceiling(slot: &str, px: usize) -> Result<u32, InterveneError> {
    u32::try_from(px).map_err(|_| {
        InterveneError::out_of_range(format!(
            "{slot}: {px} px exceeds the {} px a section width is measured in",
            u32::MAX
        ))
    })
}

/// R1565 §5.15 — a WHOLE-ROW write must carry exactly one entry per section,
/// and the refusal states both counts.
///
/// Three slots (`sizes` / `hidden` / `selections`) share the rule, and shared
/// it because the header's extent is the fact a client is missing: "wrong
/// length" alone leaves it guessing which of the two numbers to change.
///
/// # Errors
///
/// [`InterveneError::OutOfRange`] naming the row, the length given and the
/// length required.
fn row_len(what: &str, given: usize, want: usize) -> InterveneError {
    InterveneError::out_of_range(format!(
        "{what}: this header has {want} sections, so a whole-row write needs \
         {want} entries, not {given}"
    ))
}

/// Decode a JSON array of non-negative integers out of an
/// [`IntrospectValue`]; `None` for any other shape.
fn json_u64_array(value: &IntrospectValue) -> Option<Vec<u64>> {
    let IntrospectValue::Json(serde_json::Value::Array(items)) = value else {
        return None;
    };
    items.iter().map(serde_json::Value::as_u64).collect()
}

/// A decoded snapshot of a [`ColumnLayout`]'s introspection slots — the
/// **deserialize peer** of [`ColumnLayout::query`], mirroring
/// [`read_reorder`](crate::widgets::reorder::read_reorder) for the layout
/// slots. A binding decodes the header wire shape through this rather than
/// hand-matching the JSON, so a slot rename cannot silently break a consumer's
/// read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLayoutView {
    /// The saved layout (order + logical-keyed sizes + hidden flags).
    pub state: ColumnLayoutState,
    /// The painted sections. Carried instead of separate section / width
    /// vectors because those are derivable from it — a decoded view that held
    /// both could disagree with itself.
    pub placements: Vec<SectionPlacement>,
}

/// Decode the header-layout slots (`state` / `placements`) from an
/// introspection surface that delegates them to a [`ColumnLayout`]. The
/// inverse of [`ColumnLayout::query`]; keep the two in lockstep.
#[must_use]
pub fn read_column_layout(intro: &dyn ExternalIntrospect) -> ColumnLayoutView {
    let state = match intro.query("state") {
        Some(IntrospectValue::Json(v)) => ColumnLayoutState::from_json(&v).unwrap_or_default(),
        _ => ColumnLayoutState::default(),
    };
    let placements = match intro.query("placements") {
        Some(IntrospectValue::Json(serde_json::Value::Array(a))) => a
            .iter()
            .filter_map(|p| {
                let field = |k: &str| p.get(k)?.as_u64();
                Some(SectionPlacement {
                    visual: usize::try_from(field("visual")?).ok()?,
                    logical: usize::try_from(field("logical")?).ok()?,
                    x: u32::try_from(field("x")?).ok()?,
                    size: u32::try_from(field("size")?).ok()?,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    ColumnLayoutView { state, placements }
}

#[cfg(test)]
mod tests {
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;
    use std::borrow::Cow;

    use super::{
        ColumnLayout, ColumnLayoutState, DEFAULT_CONTENTS_PRECISION, DEFAULT_HEADER_ALIGNMENT,
        DEFAULT_SECTION_SIZE, SectionPlacement, SectionResizeMode, SectionSelection, TextAlign,
        read_column_layout,
    };
    use crate::external::{
        ArgDomain, DragPayload, DropPoint, ExternalIntrospect, InterveneError, IntrospectValue,
        InvokeError, SchemaChannel,
    };
    use crate::widgets::column_widths::{DEFAULT_MAX_COL_WIDTH, DEFAULT_MIN_COL_WIDTH};
    use crate::widgets::grid_sort::col_sort_dir;
    use crate::widgets::reorder::ReorderModel;

    /// Four sections wide enough to tell apart by width alone.
    fn layout() -> ColumnLayout {
        ColumnLayout::new(vec![100, 120, 140, 160])
    }

    fn text(s: &str) -> IntrospectValue {
        IntrospectValue::Text(s.to_string())
    }

    /// R1501 — the five paths that answered while `$schema` denied them. Each
    /// was added by a round that edited this module (R1493 / R1496 / R1498) and
    /// left the consumer's hand-written copy of the list behind.
    const FORMERLY_UNDECLARED: [&str; 5] = [
        "stretch_last_section",
        "effective_resize_modes",
        "effective_resize_mode.<logical>",
        "resize_contents_precision",
        "reset_default_section_size",
    ];

    #[test]
    fn r1501_the_declaration_names_every_path_the_layout_answers() {
        for p in FORMERLY_UNDECLARED {
            assert!(
                ColumnLayout::SCHEMA.field_for(p).is_some()
                    || ColumnLayout::SCHEMA_FIELDS.iter().any(|f| f.path == p),
                "{p:?} answers but is not declared — the R1501 defect",
            );
        }
    }

    /// R1501 — the declared paths that are `invoke` channels. They read as nothing
    /// by design, because [`SchemaField`] does not separate a readable value from an
    /// action yet (its own doc says so), so the walk below names them rather
    /// than probing them. R1504 — the toolkit centres a horizontal header; a
    /// snapshot taken before this round describes one painted flush left. The
    /// two are different questions and the module answers them differently,
    /// which is the R1496 split.
    #[test]
    fn r1504_a_new_header_centres_and_an_old_snapshot_does_not() {
        assert_eq!(DEFAULT_HEADER_ALIGNMENT, TextAlign::Center);
        assert_eq!(layout().default_alignment(), TextAlign::Center);

        // An older snapshot: every other field present, this one absent.
        let mut older = layout().save_state().to_json();
        older
            .as_object_mut()
            .expect("state is an object")
            .remove("default_alignment");
        let decoded = ColumnLayoutState::from_json(&older).expect("older shape decodes");
        assert_eq!(
            decoded.default_alignment,
            TextAlign::Start,
            "absent describes the header that painted its labels flush left",
        );
    }

    /// R1504 — the stored/effective pair. A section paints with its own
    /// exception where the model gave one and with the header's rule where it
    /// did not, and only the override can answer "nothing".
    #[test]
    fn r1504_a_section_paints_its_exception_or_the_headers_rule() {
        let l = layout();
        l.set_default_alignment(TextAlign::End);

        assert_eq!(l.section_alignment(0), Some(TextAlign::End));
        assert_eq!(l.section_alignment_override(0), None);

        assert!(l.set_section_alignment(1, Some(TextAlign::Start)));
        assert_eq!(l.section_alignment(1), Some(TextAlign::Start));
        assert_eq!(l.section_alignment_override(1), Some(TextAlign::Start));
        assert_eq!(
            l.section_alignment(0),
            Some(TextAlign::End),
            "one section's exception is not the header's rule",
        );

        // Handing it back.
        assert!(l.set_section_alignment(1, None));
        assert_eq!(l.section_alignment(1), Some(TextAlign::End));
        assert_eq!(l.section_alignment_override(1), None);
    }

    /// R1504 — a section outside `0..count` carries no alignment and is not
    /// given a plausible one, the rule R1501 found five accessors breaking.
    #[test]
    fn r1504_an_absent_section_has_no_alignment() {
        let l = layout();
        let out = l.count();
        assert_eq!(l.section_alignment(out), None);
        assert_eq!(l.section_alignment_override(out), None);
        assert!(!l.set_section_alignment(out, Some(TextAlign::Center)));
        assert_eq!(
            l.query(&format!("section_alignment.{out}")),
            Some(IntrospectValue::Null),
        );
        assert_eq!(
            l.query(&format!("section_alignment_override.{out}")),
            Some(IntrospectValue::Null),
        );
    }

    /// R1504 — both channels over the wire, including the spelling that hands a
    /// section back to the rule and the one this build does not know.
    #[test]
    fn r1504_the_header_reads_and_writes_its_alignment_over_the_wire() {
        let l = layout();
        assert_eq!(l.query("default_alignment"), Some(text("Center")));

        l.intervene("default_alignment", &text("End"))
            .expect("a known spelling is accepted");
        assert_eq!(l.query("default_alignment"), Some(text("End")));
        assert!(
            l.intervene("default_alignment", &text("middle")).is_err(),
            "an unknown spelling is refused, not silently defaulted",
        );
        assert_eq!(l.query("default_alignment"), Some(text("End")));

        l.invoke("set_section_alignment", &text("2:Start"))
            .expect("a section takes an exception");
        assert_eq!(l.query("section_alignment.2"), Some(text("Start")));
        assert_eq!(l.query("section_alignment_override.2"), Some(text("Start")));

        // The projection reports what every section paints with.
        let IntrospectValue::Json(all) = l.query("alignments").expect("alignments answers") else {
            panic!("alignments is json");
        };
        assert_eq!(
            all,
            serde_json::json!(["End", "End", "Start", "End"]),
            "the effective row, exception included",
        );

        l.invoke("set_section_alignment", &text("2:default"))
            .expect("`default` hands the section back");
        assert_eq!(
            l.query("section_alignment_override.2"),
            Some(IntrospectValue::Null)
        );
        assert!(
            l.invoke("set_section_alignment", &text("2:middle"))
                .is_err(),
            "an unknown spelling is refused here too",
        );
    }

    /// R1504 — the header's rule is saved and the model's exceptions are not,
    /// which is what the toolkit's `saveState()` carries. A restore therefore hands back
    /// a header whose sections all defer.
    #[test]
    fn r1504_the_rule_survives_a_restore_and_the_exceptions_do_not() {
        let l = layout();
        l.set_default_alignment(TextAlign::End);
        assert!(l.set_section_alignment(1, Some(TextAlign::Start)));
        let saved = l.save_state();
        assert_eq!(saved.default_alignment, TextAlign::End);

        let other = layout();
        assert!(other.set_section_alignment(0, Some(TextAlign::Center)));
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(other.default_alignment(), TextAlign::End);
        assert_eq!(
            other.section_alignment_override(0),
            None,
            "the outgoing header's exception does not survive a state that never mentioned it",
        );
        assert_eq!(other.section_alignment(1), Some(TextAlign::End));
    }

    /// R1510 — the rule gates what the header paints and never what the
    /// consumer published, which is the whole reason both are readable.
    #[test]
    fn r1510_the_rule_gates_the_highlight_not_the_selection() {
        let l = layout();
        assert!(!l.highlight_sections(), "off by default, as in the toolkit");
        assert!(l.set_section_selection(1, SectionSelection::Partial));
        assert!(l.set_section_selection(2, SectionSelection::Full));

        // The published selection is the same in both postures.
        for on in [false, true] {
            l.set_highlight_sections(on);
            assert_eq!(l.section_selection(1), Some(SectionSelection::Partial));
            assert_eq!(l.section_selection(2), Some(SectionSelection::Full));
        }

        l.set_highlight_sections(false);
        assert_eq!(
            (0..l.count())
                .filter_map(|s| l.section_highlight(s))
                .collect::<Vec<_>>(),
            vec![SectionSelection::Unselected; l.count()],
            "no rule, no highlight — the selection is still there and unpainted",
        );
        l.set_highlight_sections(true);
        assert_eq!(l.section_highlight(1), Some(SectionSelection::Partial));
        assert_eq!(l.section_highlight(2), Some(SectionSelection::Full));
    }

    /// R1510 — the two the toolkit predicates, and the pair that cannot exist.
    #[test]
    fn r1510_coverage_implies_intersection() {
        assert!(!SectionSelection::Unselected.intersects());
        assert!(!SectionSelection::Unselected.covers());
        assert!(SectionSelection::Partial.intersects());
        assert!(!SectionSelection::Partial.covers());
        assert!(SectionSelection::Full.intersects());
        assert!(SectionSelection::Full.covers());
        for s in [
            SectionSelection::Unselected,
            SectionSelection::Partial,
            SectionSelection::Full,
        ] {
            assert!(
                !s.covers() || s.intersects(),
                "{s} covers without intersecting, which is not a state a \
                 selection can be in",
            );
            assert_eq!(
                s.as_wire().parse::<SectionSelection>(),
                Ok(s),
                "the wire spelling round-trips",
            );
        }
        assert!("Full".parse::<SectionSelection>().is_err(), "no aliases");
        assert!(
            "unselected".parse::<SectionSelection>().is_err(),
            "the wire word for `Unselected` is `none`, and only that",
        );
    }

    /// R1510 — a section outside the header has no coverage, rather than a
    /// plausible `"none"` from inside the domain (the R1501 defect).
    #[test]
    fn r1510_out_of_range_has_no_coverage() {
        let l = layout();
        let past = l.count();
        assert_eq!(l.section_selection(past), None);
        assert_eq!(l.section_highlight(past), None);
        assert!(!l.set_section_selection(past, SectionSelection::Full));
        assert_eq!(
            l.query(&format!("section_selection.{past}")),
            Some(IntrospectValue::Null),
        );
        assert_eq!(
            l.query(&format!("section_highlight.{past}")),
            Some(IntrospectValue::Null),
        );
    }

    /// R1510 — a bulk publish takes the whole row or none of it, like
    /// `set_content_widths`.
    #[test]
    fn r1510_a_wrong_length_publish_is_ignored() {
        let l = layout();
        l.set_selections(vec![SectionSelection::Full; l.count()]);
        assert_eq!(l.section_selection(0), Some(SectionSelection::Full));
        l.set_selections(vec![SectionSelection::Unselected; l.count() - 1]);
        assert_eq!(
            l.section_selection(0),
            Some(SectionSelection::Full),
            "a short vector is another grid's selection, not half of this one's",
        );
    }

    /// R1510 — the rule travels with the layout and the selection does not,
    /// which is the opposite of what happens to the alignment exceptions.
    #[test]
    fn r1510_a_restore_replays_the_rule_and_keeps_the_selection() {
        let l = layout();
        l.set_highlight_sections(true);
        let saved = l.save_state();
        assert!(saved.highlight_sections);
        assert_eq!(
            saved.to_json().get("highlight_sections"),
            Some(&serde_json::Value::Bool(true)),
            "the toolkit serialises this one as `highlightSelected`",
        );

        let other = layout();
        assert!(other.set_section_selection(0, SectionSelection::Full));
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert!(other.highlight_sections(), "the rule came back");
        assert_eq!(
            other.section_selection(0),
            Some(SectionSelection::Full),
            "and the selection survived: it belongs to the view's selection \
             model, which a header restore cannot reach",
        );
    }

    /// R1510 — an older snapshot decodes as `false`, which is both the toolkit's
    /// default and what a pre-R1510 header did.
    #[test]
    fn r1510_an_older_snapshot_does_not_highlight() {
        let l = layout();
        l.set_highlight_sections(true);
        let mut older = l.save_state().to_json();
        older
            .as_object_mut()
            .expect("state is an object")
            .remove("highlight_sections");
        let decoded = ColumnLayoutState::from_json(&older).expect("the rest still decodes");
        assert!(
            !decoded.highlight_sections,
            "absent decodes to `false` — the old header had no selection input \
             at all, and the toolkit starts here too",
        );
        assert_eq!(
            ColumnLayoutState::default().highlight_sections,
            decoded.highlight_sections,
            "so for this field the new-header and old-snapshot answers agree, \
             unlike `sections_movable` and `default_alignment`",
        );
    }

    /// R1510 — the wire doors: the rule, the whole row, and one section.
    #[test]
    fn r1510_the_rule_is_readable_and_writable_over_the_wire() {
        let l = layout();
        assert_eq!(
            l.query("highlight_sections"),
            Some(IntrospectValue::Bool(false)),
        );
        l.intervene("highlight_sections", &IntrospectValue::Bool(true))
            .expect("the rule is writable");
        assert!(l.highlight_sections());
        assert!(
            l.intervene("highlight_sections", &IntrospectValue::Text("yes".into()))
                .is_err(),
            "a non-bool is a type error, not a silent `true`",
        );

        assert!(l.set_section_selection(0, SectionSelection::Partial));
        assert!(l.set_section_selection(1, SectionSelection::Full));
        let wire = |path: &str| match l.query(path) {
            Some(IntrospectValue::Json(v)) => v.to_string(),
            other => panic!("{path} answers json, got {other:?}"),
        };
        // Built from the fixture's own count: a hardcoded length measures the
        // fixture rather than the rule.
        let published = || {
            let mut row = vec!["none"; l.count()];
            row[0] = "partial";
            row[1] = "full";
            serde_json::json!(row).to_string()
        };
        let suppressed = serde_json::json!(vec!["none"; l.count()]).to_string();
        assert_eq!(wire("selections"), published());
        assert_eq!(wire("highlights"), published());
        l.set_highlight_sections(false);
        assert_eq!(
            wire("selections"),
            published(),
            "the published row is unchanged by the rule",
        );
        assert_eq!(
            wire("highlights"),
            suppressed,
            "and the painted row is entirely the rule's to suppress",
        );

        // The input is writable too, like the two consumer-published inputs
        // beside it — §2 #2, not the toolkit's class boundary. A wrong length
        // is the same `OutOfRange` `content_widths` reports, and an unknown spelling the same `TypeMismatch`.
        assert_out_of_range_saying(
            &l.intervene(
                "selections",
                &IntrospectValue::Json(serde_json::json!(["full"])),
            ),
            "needs 4 entries, not 1",
        );
        assert_eq!(
            l.intervene(
                "selections",
                &IntrospectValue::Json(serde_json::json!(vec!["everything"; l.count()])),
            ),
            Err(InterveneError::TypeMismatch),
            "and a spelling this build does not know is refused, not defaulted",
        );
        l.set_highlight_sections(true);
        l.intervene(
            "selections",
            &IntrospectValue::Json(serde_json::json!(vec!["full"; l.count()])),
        )
        .expect("the whole row is writable");
        assert_eq!(l.section_highlight(0), Some(SectionSelection::Full));

        // And the per-section door answers the EFFECTIVE row, so a client
        // learns in one round-trip whether the rule let its write show.
        let painted = l
            .invoke(
                "set_section_selection",
                &IntrospectValue::Text("1:partial".into()),
            )
            .expect("one section is settable");
        assert_eq!(l.section_selection(1), Some(SectionSelection::Partial));
        let IntrospectValue::Json(row) = painted else {
            panic!("the action answers the painted row");
        };
        assert_eq!(row[1], serde_json::Value::String("partial".into()));
        assert!(
            l.invoke(
                "set_section_selection",
                &IntrospectValue::Text("1:everything".into()),
            )
            .is_err(),
            "an unknown spelling is rejected here too",
        );
        assert!(
            l.invoke(
                "set_section_selection",
                &IntrospectValue::Text(format!("{}:full", l.count())),
            )
            .is_err(),
            "and so is a section this header does not have",
        );
    }

    #[test]
    fn r1501_every_declared_read_path_answers() {
        let l = layout();
        let mut probed = 0;
        let mut actions = 0;
        for f in ColumnLayout::SCHEMA_FIELDS {
            // R1504 — the declaration says which channel it is. This used to be
            // a hand-written list of fifteen names here, and R1504 was about to
            // make it sixteen: an action added upstream would have been probed
            // as a read and failed, and one added here would have needed the
            // list edited in step. Neither is a thing a test should be asked to
            // remember.
            if f.channel == SchemaChannel::Invoke {
                assert!(
                    l.query(f.path).is_none(),
                    "{:?} is declared as an invoke channel but answers a read",
                    f.path,
                );
                actions += 1;
                continue;
            }
            // A family is addressed by its members, never by its template.
            let probe = if f.args.is_empty() {
                f.path.to_string()
            } else {
                format!("{}0", f.literal_prefix())
            };
            assert!(
                l.query(&probe).is_some(),
                "{:?} is declared but {probe:?} does not answer",
                f.path,
            );
            probed += 1;
        }
        assert_eq!(
            probed + actions,
            ColumnLayout::SCHEMA_FIELDS.len(),
            "every declared field is either probed or declared an action",
        );
        assert!(actions > 0, "the surface declares invoke channels");
    }

    #[test]
    fn r1501_a_section_keyed_read_outside_the_header_answers_null() {
        // Measured before the round on a 3-section header: five of these nine
        // answered a plausible value for section 3 — `0` widths, a `false`
        // hidden flag, and `"interactive"` modes for a column that is not
        // there. A client that trusts the declared domain cannot tell those
        // from real answers.
        let l = ColumnLayout::new(vec![150, 90, 100]);
        for p in [
            "visual_index.3",
            "logical_index.3",
            "section_size.3",
            "section_hidden.3",
            "section_position.3",
            "resize_mode.3",
            "effective_resize_mode.3",
            "content_width.3",
        ] {
            assert_eq!(
                l.query(p),
                Some(IntrospectValue::Null),
                "{p:?} is outside the declared domain and must not read as a value",
            );
        }
        // In range, all nine still answer for real.
        assert_eq!(l.query("section_size.2"), Some(IntrospectValue::Int(100)));
        assert_eq!(l.query("resize_mode.2"), Some(text("interactive")));
    }

    #[test]
    fn r1501_the_layout_publishes_the_bound_its_families_declare() {
        // `IndexOf("count")` is a dead end unless this surface answers `count`.
        let l = layout();
        assert_eq!(l.query("count"), Some(IntrospectValue::Int(4)));
        // And `logical_index_at` is bounded by pixels, not sections, which is
        // the domain it declares.
        let f = ColumnLayout::SCHEMA
            .field_for("logical_index_at.0")
            .expect("declared");
        assert_eq!(f.args.len(), 1, "it takes an argument — it was a scalar");
        assert!(matches!(
            f.args[0].domain,
            ArgDomain::IndexOf("visible_total")
        ));
        assert_eq!(l.query("visible_total"), Some(IntrospectValue::Int(520)));
    }

    #[test]
    fn r1501_an_unwritable_read_is_read_only_not_unknown() {
        let l = layout();
        assert_eq!(
            l.intervene("placements", &IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
            "declared and readable, so refusing it as unknown is the §2 #7 lie",
        );
        assert_eq!(
            l.intervene("count", &IntrospectValue::Int(9)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            l.intervene("no_such_path", &IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
            "and a path nothing declares is still unknown",
        );
    }

    #[test]
    fn r1501_the_declaration_composes_the_reorder_models_verbatim() {
        let tail = &ColumnLayout::SCHEMA_FIELDS
            [ColumnLayout::SCHEMA_FIELDS.len() - ReorderModel::SCHEMA_FIELDS.len()..];
        assert_eq!(
            tail,
            ReorderModel::SCHEMA_FIELDS,
            "the embedded model's paths are borrowed, not restated",
        );
        assert!(
            ColumnLayout::SCHEMA_FIELDS
                .iter()
                .all(|f| !f.path.is_empty()),
            "a blank row means a length that stopped matching its operands",
        );
    }

    fn ints(v: &IntrospectValue) -> Vec<u64> {
        match v {
            IntrospectValue::Json(serde_json::Value::Array(a)) => {
                a.iter().filter_map(serde_json::Value::as_u64).collect()
            }
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_resized_section_keeps_its_width_where_it_is_moved() {
        // THE claim of this module. Before it, widths were keyed by screen
        // position, so this assertion could not even be written: moving a
        // column left its width behind on the old position.
        let l = layout();
        l.resize_section(0, 200);
        l.move_section(0, 2);

        assert_eq!(l.order(), vec![1, 2, 0, 3], "section 0 moved to position 2");
        // The discriminator: a position-keyed width model answers
        // [200, 120, 140, 160] here — unchanged, because it never learned the
        // column moved. Section 0's 200 has to be third.
        assert_eq!(l.visible_widths(), vec![120, 140, 200, 160]);
        assert_eq!(l.section_size(0), 200, "size is keyed by logical section");
        assert_eq!(l.section_position(0), Some(260), "120 + 140 precede it");
    }

    #[test]
    fn a_hidden_section_keeps_its_place_and_its_size() {
        // The toolkit's rule: hiding does not remove a section from the
        // permutation, so showing it again puts it back rather than appending
        // it.
        let l = layout();
        l.resize_section(1, 300);
        l.set_section_hidden(1, true);

        assert_eq!(l.visible_sections(), vec![0, 2, 3]);
        assert_eq!(l.visible_widths(), vec![100, 140, 160]);
        assert_eq!(l.visual_index(1), Some(1), "visual index survives hiding");
        assert_eq!(l.section_size(1), 300, "and so does the size");
        assert_eq!(l.section_position(1), None, "but it is painted nowhere");
        assert_eq!(l.hidden_section_count(), 1);

        l.set_section_hidden(1, false);
        assert_eq!(l.visible_sections(), vec![0, 1, 2, 3], "back in its place");
        assert_eq!(l.visible_widths(), vec![100, 300, 140, 160]);
    }

    #[test]
    fn hiding_composes_with_reordering() {
        // The composition the three separate axes could not express: hide one
        // section, move another, and the projection is right for both.
        let l = layout();
        l.move_section(0, 3); // [1, 2, 3, 0]
        l.set_section_hidden(2, true);
        assert_eq!(l.visible_sections(), vec![1, 3, 0]);
        assert_eq!(l.visible_widths(), vec![120, 160, 100]);
        assert_eq!(l.logical_index(1), Some(2), "hidden keeps its visual slot");
        assert_eq!(l.section_position(0), Some(280), "120 + 160 precede it");
    }

    #[test]
    fn logical_index_at_walks_non_uniform_widths() {
        // A uniform-width hit test (x / col_width) gets every one of these
        // wrong once the columns differ — the assumption this replaces.
        let l = layout();
        assert_eq!(l.logical_index_at(0), Some(0));
        assert_eq!(l.logical_index_at(99), Some(0));
        assert_eq!(l.logical_index_at(100), Some(1), "boundary is exclusive");
        assert_eq!(l.logical_index_at(219), Some(1));
        assert_eq!(l.logical_index_at(220), Some(2));
        assert_eq!(l.logical_index_at(519), Some(3));
        assert_eq!(l.logical_index_at(520), None, "past the last section");

        // A hidden section occupies no width, so the hit test steps over it.
        l.set_section_hidden(1, true);
        assert_eq!(l.logical_index_at(100), Some(2));
    }

    #[test]
    fn swap_displaces_one_section_where_a_move_shifts_the_span() {
        // The toolkit has both because they are different operations; a test
        // that only checked "the order changed" would not tell them apart.
        let swapped = layout();
        swapped.swap_sections(0, 3);
        assert_eq!(swapped.order(), vec![3, 1, 2, 0]);

        let moved = layout();
        moved.move_section(0, 3);
        assert_eq!(moved.order(), vec![1, 2, 3, 0]);

        // Out of range and self-swap are no-ops, not panics.
        let l = layout();
        l.swap_sections(0, 9);
        l.swap_sections(2, 2);
        assert_eq!(l.order(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn save_state_round_trips_through_restore() {
        let l = layout();
        l.resize_section(2, 250);
        l.set_section_hidden(3, true);
        l.move_section(2, 0);
        let saved = l.save_state();

        // Drift far from the saved layout, then restore.
        l.move_section(0, 3);
        l.resize_section(2, 60);
        l.set_section_hidden(3, false);
        l.set_section_hidden(0, true);
        l.restore_state(&saved)
            .expect("the snapshot describes this header");

        assert_eq!(l.save_state(), saved);
        assert_eq!(l.order(), vec![2, 0, 1, 3]);
        assert_eq!(l.section_size(2), 250);
        assert!(l.is_section_hidden(3));
        assert!(!l.is_section_hidden(0));
    }

    #[test]
    fn a_restore_replays_the_permissions_and_the_sampling_bound() {
        // R1496 — the three fields the toolkit's `write()` carries
        // and this snapshot did not. Measured over the wire before the round:
        // the header answered `resize_contents_precision` = 1000, saved a
        // state, was moved to 7, restored — and stayed at 7. A snapshot that
        // replays every content-fitted width while dropping the bound that
        // produced them describes a header that never existed.
        let l = interactive_layout();
        l.set_resize_contents_precision(1000);
        let saved = l.save_state();

        l.set_sections_movable(false);
        l.set_sections_clickable(false);
        l.set_resize_contents_precision(7);
        l.restore_state(&saved)
            .expect("the snapshot describes this header");

        assert!(l.sections_movable(), "the permission came back");
        assert!(l.sections_clickable(), "and so did the other one");
        assert_eq!(l.resize_contents_precision(), 1000, "and the bound");
        assert_eq!(l.save_state(), saved, "the whole object round-trips");
    }

    #[test]
    fn a_restore_drops_a_press_taken_against_the_outgoing_layout() {
        // R1496 — a restore replaces the permutation wholesale, so the section
        // the press named may not be the section that is there now. Holding it
        // would let the next release sort whatever landed in that position.
        let l = interactive_layout();
        let saved = l.save_state();
        press(&l, 1);
        l.restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(release(&l, 1), None, "the press did not survive");
    }

    #[test]
    fn an_older_snapshot_restores_a_header_that_still_interacts() {
        // R1496 — the one absent-field fallback in this decoder that is NOT a
        // toolkit default. A layout saved before this round came from a header
        // with no such rule, which was unconditionally movable and clickable;
        // decoding the absence as the toolkit's `false` would silently strip the
        // interaction out of every layout anyone had already saved.
        let mut older = interactive_layout().save_state().to_json();
        for key in [
            "sections_movable",
            "sections_clickable",
            "resize_contents_precision",
        ] {
            older.as_object_mut().expect("a json object").remove(key);
        }

        let decoded = ColumnLayoutState::from_json(&older).expect("the older shape still decodes");
        assert!(
            decoded.sections_movable,
            "not the toolkit's default: the old header's"
        );
        assert!(decoded.sections_clickable);
        assert_eq!(
            decoded.resize_contents_precision, DEFAULT_CONTENTS_PRECISION,
            "the bound falls back to the constant, like the other scalars"
        );
    }

    #[test]
    fn a_fresh_header_and_a_defaulted_state_agree_on_the_permissions() {
        // The other side of the asymmetry above: `Default` is the state of a
        // NEW header, so it has to be what one actually reports — or a consumer
        // that diffs against it sees a change nobody made.
        let fresh = layout().save_state();
        let d = ColumnLayoutState::default();
        assert_eq!(
            (fresh.sections_movable, fresh.sections_clickable),
            (d.sections_movable, d.sections_clickable),
        );
        assert!(!d.sections_movable, "and both are the toolkit's `false`");
        assert!(!d.sections_clickable);
    }

    #[test]
    fn a_rejected_restore_changes_nothing() {
        // Atomicity is the contract: a client that authored a bad layout must
        // not be left with half of it applied.
        let l = layout();
        l.resize_section(1, 210);
        l.set_section_hidden(2, true);
        let before = l.save_state();

        // Well-shaped, but `order` is not a permutation (1 twice, 3 missing).
        let bad_order = ColumnLayoutState {
            order: vec![0, 1, 1, 2],
            sizes: vec![10, 20, 30, 40],
            hidden: vec![true, true, true, true],
            modes: vec![SectionResizeMode::Stretch; 4],
            sort_indicator: Some((0, true)),
            sort_indicator_shown: true,
            // R1493 — the scalar rules are well-formed here on purpose: this
            // fixture is about the permutation, so the bound check must not be
            // what rejects it.
            ..ColumnLayoutState::default()
        };
        assert!(
            l.restore_state(&bad_order).is_err(),
            "the snapshot is refused"
        );
        assert_eq!(l.save_state(), before, "no size or flag was written");

        // Wrong vector length — rejected before the order is even considered.
        let short = ColumnLayoutState {
            order: vec![3, 2, 1, 0],
            sizes: vec![10, 20, 30],
            hidden: vec![true; 4],
            modes: vec![SectionResizeMode::Interactive; 4],
            sort_indicator: None,
            sort_indicator_shown: false,
            ..ColumnLayoutState::default()
        };
        assert!(l.restore_state(&short).is_err(), "the snapshot is refused");
        assert_eq!(l.save_state(), before, "the order was not applied either");
    }

    #[test]
    fn a_restored_size_lands_on_the_section_not_the_position() {
        // The reason the snapshot is logical-keyed: restoring into a header
        // whose columns have since moved must put each size back on its own
        // section.
        let l = layout();
        l.resize_section(0, 200);
        let saved = l.save_state();
        assert_eq!(saved.sizes, vec![200, 120, 140, 160]);

        let other = layout();
        other.move_section(0, 3); // [1, 2, 3, 0]
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(other.order(), vec![0, 1, 2, 3], "the order came back too");
        assert_eq!(other.section_size(0), 200);
    }

    #[test]
    fn state_round_trips_over_the_wire() {
        // query("state") and intervene("state", ..) are inverses — the
        // read/write symmetry every pinion wire slot keeps.
        let l = layout();
        l.resize_section(1, 180);
        l.set_section_hidden(0, true);
        l.swap_sections(0, 2);
        let Some(IntrospectValue::Json(json)) = l.query("state") else {
            panic!("state query");
        };

        let other = layout();
        other
            .intervene("state", &IntrospectValue::Json(json.clone()))
            .expect("restore");
        assert_eq!(other.save_state(), l.save_state());
        assert_eq!(
            ColumnLayoutState::from_json(&json).expect("decode"),
            l.save_state(),
            "the decoded shape is the state itself"
        );
    }

    #[test]
    fn wire_reads_answer_each_derived_question() {
        let l = layout();
        l.resize_section(0, 200);
        l.move_section(0, 2);
        l.set_section_hidden(3, true);

        assert_eq!(ints(&l.query("order").expect("order")), vec![1, 2, 0, 3]);
        assert_eq!(
            ints(&l.query("sizes").expect("sizes")),
            vec![200, 120, 140, 160]
        );
        assert_eq!(
            ints(&l.query("visible_sections").expect("visible")),
            vec![1, 2, 0]
        );
        assert_eq!(
            ints(&l.query("visible_widths").expect("widths")),
            vec![120, 140, 200]
        );
        assert!(matches!(
            l.query("visible_total"),
            Some(IntrospectValue::Int(460))
        ));
        assert!(matches!(
            l.query("hidden_count"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            l.query("visual_index.0"),
            Some(IntrospectValue::Int(2))
        ));
        assert!(matches!(
            l.query("logical_index.0"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(matches!(
            l.query("section_size.0"),
            Some(IntrospectValue::Int(200))
        ));
        assert!(matches!(
            l.query("section_hidden.3"),
            Some(IntrospectValue::Bool(true))
        ));
        assert!(matches!(
            l.query("section_position.0"),
            Some(IntrospectValue::Int(260))
        ));
        assert!(
            matches!(l.query("section_position.3"), Some(IntrospectValue::Null)),
            "a hidden section is painted nowhere"
        );
        assert!(matches!(
            l.query("logical_index_at.300"),
            Some(IntrospectValue::Int(0))
        ));
        assert!(matches!(
            l.query("logical_index_at.900"),
            Some(IntrospectValue::Null)
        ));
        // Reorder slots fall through, and an unknown path is still None.
        assert!(matches!(
            l.query("grabbed"),
            Some(IntrospectValue::Bool(false))
        ));
        assert!(l.query("selected_id").is_none());
        assert!(l.query("section_size.zz").is_none());
    }

    #[test]
    fn section_invokes_speak_qts_vocabulary_and_report_the_outcome() {
        let l = layout();

        // resize reports the applied size, so the clamp is observable in the
        // same round-trip that asked for the change.
        let applied = l.invoke("resize_section", &text("0:10")).expect("resize");
        assert!(
            matches!(applied, IntrospectValue::Int(n)
                if n == i64::from(DEFAULT_MIN_COL_WIDTH)),
            "clamped up to the floor, and said so: {applied:?}"
        );

        // hide reports what is now painted.
        let shown = l
            .invoke("set_section_hidden", &text("1:true"))
            .expect("hide");
        assert_eq!(ints(&shown), vec![0, 2, 3]);

        // swap reports the new order.
        let order = l.invoke("swap_sections", &text("0:3")).expect("swap");
        assert_eq!(ints(&order), vec![3, 1, 2, 0]);

        // move_section falls through to the reorder model.
        let order = l.invoke("move_section", &text("0:2")).expect("move");
        assert_eq!(ints(&order), vec![1, 2, 3, 0]);
    }

    #[test]
    fn malformed_section_invokes_are_rejected_by_kind() {
        let l = layout();
        // Not text at all.
        assert!(matches!(
            l.invoke("resize_section", &IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch)
        ));
        // Text, but not a pair.
        assert_refused_saying(
            &l.invoke("resize_section", &text("140")),
            "malformed argument \"140\"",
        );
        // A pair naming a section that does not exist. R1564 — this and the
        // malformed pair above were the SAME value before the reason existed,
        // and they are different mistakes with different fixes.
        assert_refused_saying(
            &l.invoke("swap_sections", &text("0:9")),
            "no section 9 in this header (it has 4)",
        );
        assert_refused_saying(
            &l.invoke("set_section_hidden", &text("9:true")),
            "no section 9 in this header (it has 4)",
        );
        // A pair whose second half is the wrong type.
        assert_refused_saying(
            &l.invoke("set_section_hidden", &text("0:yes")),
            "malformed argument \"0:yes\"",
        );
        assert!(matches!(
            l.invoke("hide_everything", &text("0:1")),
            Err(InvokeError::UnknownPath)
        ));
        // Nothing above changed the layout.
        assert_eq!(
            l.save_state(),
            ColumnLayout::new(vec![100, 120, 140, 160]).save_state()
        );
    }

    #[test]
    fn vector_intervenes_separate_shape_errors_from_value_errors() {
        let l = layout();
        // Right shape, wrong length.
        assert_out_of_range_saying(
            &l.intervene("sizes", &IntrospectValue::Json(serde_json::json!([10, 20]))),
            "needs 4 entries, not 2",
        );
        assert_out_of_range_saying(
            &l.intervene("hidden", &IntrospectValue::Json(serde_json::json!([true]))),
            "needs 4 entries, not 1",
        );
        // Wrong shape entirely.
        assert!(matches!(
            l.intervene(
                "hidden",
                &IntrospectValue::Json(serde_json::json!([1, 2, 3, 4]))
            ),
            Err(InterveneError::TypeMismatch)
        ));
        assert!(matches!(
            l.intervene("state", &IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        ));
        // A well-shaped state that is not a valid layout.
        assert!(matches!(
            l.intervene(
                "state",
                &IntrospectValue::Json(serde_json::json!({
                    "order": [0, 0, 2, 3],
                    "sizes": [1, 2, 3, 4],
                    "hidden": [false, false, false, false],
                }))
            ),
            Err(InterveneError::OutOfRange(_))
        ));
        assert_eq!(l.save_state(), layout().save_state(), "all rejected");

        // The good paths do land.
        l.intervene(
            "sizes",
            &IntrospectValue::Json(serde_json::json!([90, 90, 90, 90])),
        )
        .expect("sizes");
        l.intervene(
            "hidden",
            &IntrospectValue::Json(serde_json::json!([false, true, false, false])),
        )
        .expect("hidden");
        assert_eq!(l.visible_widths(), vec![90, 90, 90]);
        // `order` still falls through to the reorder model.
        l.intervene(
            "order",
            &IntrospectValue::Json(serde_json::json!([3, 2, 1, 0])),
        )
        .expect("order");
        assert_eq!(l.order(), vec![3, 2, 1, 0]);
    }

    /// A minimal `ExternalIntrospect` that delegates to a layout — stands in
    /// for a binding's `External` wrapper so `read_column_layout` is tested
    /// against the real `query` encode (the round-trip SSOT).
    struct Probe(ColumnLayout);

    impl ExternalIntrospect for Probe {
        /// R1501 — the layout's own declaration, not an empty one. A standin
        /// for a binding's wrapper has to declare what it forwards, or it
        /// stands in for the defect this round removed rather than for the
        /// consumer.
        fn schema(&self) -> crate::external::IntrospectSchema {
            ColumnLayout::SCHEMA
        }
        fn query(&self, path: &str) -> Option<IntrospectValue> {
            self.0.query(path)
        }
        fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
            self.0.intervene(path, &value)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            self.0.invoke(method, &args)
        }
    }

    #[test]
    fn stretch_divides_what_the_others_leave_over() {
        // Not an equal split of the whole width: `Stretch` takes the REMAINDER
        // after the fixed sections. An equal split would answer 300 here.
        let l = layout(); // 100 120 140 160
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_resize_mode(3, SectionResizeMode::Stretch);
        l.set_available_width(Some(600));

        assert_eq!(l.visible_widths(), vec![100, 120, 190, 190]);
        assert_eq!(
            l.visible_total(),
            600,
            "the row fills exactly what it was given"
        );
        assert_eq!(l.section_size(2), 190, "and section_size says so too");
    }

    #[test]
    fn a_stretch_remainder_is_dealt_out_not_dropped() {
        // 381 across two sections is 190 and a half. Dropping the odd pixel
        // would leave the row one short of the width it was told to fill.
        let l = layout();
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_resize_mode(3, SectionResizeMode::Stretch);
        l.set_available_width(Some(601));
        assert_eq!(l.visible_widths(), vec![100, 120, 191, 190]);
        assert_eq!(l.visible_total(), 601);
    }

    #[test]
    fn stretch_without_a_published_width_keeps_the_stored_size() {
        // Nothing to divide is not the same as nothing to show.
        let l = layout();
        l.set_all_resize_modes(SectionResizeMode::Stretch);
        assert_eq!(l.available_width(), None);
        assert_eq!(l.visible_widths(), vec![100, 120, 140, 160]);
        // And a width too small for the fixed sections does not underflow.
        l.set_resize_mode(0, SectionResizeMode::Interactive);
        l.set_available_width(Some(10));
        assert_eq!(
            l.visible_widths(),
            vec![100, 40, 40, 40],
            "the stretch shares floor at the minimum width"
        );
    }

    #[test]
    fn resize_to_contents_takes_the_hint_and_floors_it() {
        let l = layout();
        l.set_content_widths(vec![200, 30, 140, 160]);
        l.set_resize_mode(0, SectionResizeMode::ResizeToContents);
        l.set_resize_mode(1, SectionResizeMode::ResizeToContents);
        assert_eq!(l.section_size(0), 200, "sized to its content");
        assert_eq!(
            l.section_size(1),
            DEFAULT_MIN_COL_WIDTH,
            "a content narrower than the floor still gets the floor"
        );
        // A hint vector of the wrong length is ignored whole — a partial hint
        // set would size some columns to another grid's content.
        l.set_content_widths(vec![1, 2]);
        assert_eq!(l.section_size(0), 200, "the bad hint set was dropped");
    }

    #[test]
    fn a_derived_section_stores_the_resize_but_keeps_deriving() {
        // The toolkit: resizeSection has no effect outside Interactive /
        // Fixed. The value is not discarded though, so switching back reveals
        // it.
        let l = layout();
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_available_width(Some(600));
        // 600 less the three interactive sections (100 + 120 + 160) is 220.
        let reported = l.resize_section(2, 500);
        assert_eq!(reported, 220, "the answer is the size it actually has");
        assert_eq!(l.visible_widths(), vec![100, 120, 220, 160]);
        l.set_resize_mode(2, SectionResizeMode::Interactive);
        assert_eq!(l.section_size(2), 500, "the stored write was kept");
    }

    #[test]
    fn the_two_mode_predicates_differ_exactly_at_fixed() {
        // Fixed is the whole reason there are two questions rather than one.
        for (mode, stores, user) in [
            (SectionResizeMode::Interactive, true, true),
            (SectionResizeMode::Fixed, true, false),
            (SectionResizeMode::Stretch, false, false),
            (SectionResizeMode::ResizeToContents, false, false),
        ] {
            assert_eq!(mode.stores_size(), stores, "{mode} stores_size");
            assert_eq!(mode.user_resizable(), user, "{mode} user_resizable");
            // The wire spelling round-trips, which is what the invoke parses.
            assert_eq!(mode.as_wire().parse(), Ok(mode));
        }
        assert_eq!(
            "Stretch".parse::<SectionResizeMode>(),
            Err(()),
            "no aliases"
        );
    }

    #[test]
    fn a_hidden_stretch_section_takes_no_share() {
        let l = layout();
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_resize_mode(3, SectionResizeMode::Stretch);
        l.set_available_width(Some(600));
        l.set_section_hidden(3, true);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 380],
            "the remaining stretch section takes the whole leftover"
        );
        assert_eq!(
            l.section_size(3),
            160,
            "and the hidden one reports its stored size, having no share"
        );
    }

    #[test]
    fn a_stretch_share_survives_a_reorder_but_its_place_does_not() {
        // The composition: the mode is keyed by logical section like the size
        // it replaces, so moving the section moves the policy with it.
        let l = layout();
        l.set_resize_mode(0, SectionResizeMode::Stretch);
        l.set_available_width(Some(600));
        assert_eq!(l.visible_widths(), vec![180, 120, 140, 160]);
        l.move_section(0, 3);
        assert_eq!(l.order(), vec![1, 2, 3, 0]);
        assert_eq!(
            l.visible_widths(),
            vec![120, 140, 160, 180],
            "the stretch section is last now, and still takes the leftover"
        );
        assert_eq!(l.section_position(0), Some(420));
    }

    #[test]
    fn modes_round_trip_through_state_and_an_older_snapshot_still_restores() {
        let l = layout();
        l.set_resize_mode(1, SectionResizeMode::Fixed);
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        let saved = l.save_state();
        assert_eq!(
            saved.modes,
            vec![
                SectionResizeMode::Interactive,
                SectionResizeMode::Fixed,
                SectionResizeMode::Stretch,
                SectionResizeMode::Interactive,
            ]
        );
        let json = saved.to_json();
        assert_eq!(ColumnLayoutState::from_json(&json).expect("decode"), saved);

        // A pre-R1452 snapshot has no `modes` at all and decodes as the
        // default, so an older saved layout still restores.
        let older = serde_json::json!({
            "order": [3, 2, 1, 0],
            "sizes": [50, 60, 70, 80],
            "hidden": [false, false, false, false],
        });
        let decoded = ColumnLayoutState::from_json(&older).expect("older shape decodes");
        assert_eq!(decoded.modes, vec![SectionResizeMode::Interactive; 4]);
        l.restore_state(&decoded)
            .expect("the snapshot describes this header");
        assert_eq!(l.visible_widths(), vec![80, 70, 60, 50]);

        // Present but misspelled is an error, not a silent default — a client
        // that meant to set a mode has to be told it did not.
        assert_eq!(
            ColumnLayoutState::from_json(&serde_json::json!({
                "order": [0, 1, 2, 3],
                "sizes": [50, 60, 70, 80],
                "hidden": [false, false, false, false],
                "modes": ["interactive", "Stretch", "fixed", "fixed"],
            })),
            None
        );
    }

    #[test]
    fn mode_invokes_report_the_row_they_resized() {
        // Changing one section's policy re-sizes every stretch section sharing
        // the row, so the useful answer is the row.
        let l = layout();
        l.set_available_width(Some(600));
        let widths = l
            .invoke("set_resize_mode", &text("3:stretch"))
            .expect("set_resize_mode");
        assert_eq!(ints(&widths), vec![100, 120, 140, 240]);
        assert!(matches!(
            l.query("resize_mode.3"),
            Some(IntrospectValue::Text(ref m)) if m == "stretch"
        ));

        let widths = l
            .invoke("set_all_resize_modes", &text("stretch"))
            .expect("set_all");
        assert_eq!(
            ints(&widths),
            vec![150, 150, 150, 150],
            "600 split four ways"
        );

        assert_refused_saying(
            &l.invoke("set_resize_mode", &text("0:sideways")),
            "malformed argument \"0:sideways\"",
        );
        assert_refused_saying(
            &l.invoke("set_all_resize_modes", &text("sideways")),
            "\"sideways\" is not a section resize mode",
        );
        assert_refused_saying(
            &l.invoke("set_resize_mode", &text("9:fixed")),
            "no section 9 in this header",
        );
    }

    #[test]
    fn the_derived_inputs_are_readable_and_writable_over_the_wire() {
        let l = layout();
        assert!(matches!(
            l.query("available_width"),
            Some(IntrospectValue::Null)
        ));
        l.intervene("available_width", &IntrospectValue::Int(600))
            .expect("publish a viewport");
        assert!(matches!(
            l.query("available_width"),
            Some(IntrospectValue::Int(600))
        ));
        l.intervene(
            "content_widths",
            &IntrospectValue::Json(serde_json::json!([210, 20, 30, 40])),
        )
        .expect("publish hints");
        assert_eq!(
            ints(&l.query("content_widths").expect("read back")),
            vec![210, 20, 30, 40]
        );
        assert!(matches!(
            l.query("content_width.0"),
            Some(IntrospectValue::Int(210))
        ));
        // Wrong length is a value error, wrong shape is a type error.
        assert!(matches!(
            l.intervene(
                "content_widths",
                &IntrospectValue::Json(serde_json::json!([1]))
            ),
            Err(InterveneError::OutOfRange(_))
        ));
        assert!(matches!(
            l.intervene("content_widths", &IntrospectValue::Text("wide".into())),
            Err(InterveneError::TypeMismatch)
        ));
        // Null clears the published viewport, so a stretch row falls back.
        l.intervene("available_width", &IntrospectValue::Null)
            .expect("clear");
        assert_eq!(l.available_width(), None);
        // The mode vector reads as its wire spellings.
        l.set_resize_mode(0, SectionResizeMode::ResizeToContents);
        let Some(IntrospectValue::Json(serde_json::Value::Array(modes))) = l.query("resize_modes")
        else {
            panic!("resize_modes")
        };
        assert_eq!(modes[0], serde_json::Value::from("resize_to_contents"));
        assert_eq!(l.section_size(0), 210, "and the hint is what it sizes to");
    }

    #[test]
    fn the_contents_precision_bound_is_readable_writable_and_never_zero() {
        // R1454 — the bound the measurement demands. The toolkit's default,
        // and a `0` clamped to `1`: measuring nothing would leave a
        // content-fitted column with no content to fit, and a silent
        // floor-sized column is the kind of answer a caller cannot tell from a
        // bug.
        let l = layout();
        assert_eq!(
            l.resize_contents_precision(),
            DEFAULT_CONTENTS_PRECISION,
            "the toolkit's default"
        );
        assert!(matches!(
            l.query("resize_contents_precision"),
            Some(IntrospectValue::Int(1000))
        ));

        l.set_resize_contents_precision(0);
        assert_eq!(l.resize_contents_precision(), 1, "zero clamps to one");
        l.intervene("resize_contents_precision", &IntrospectValue::Int(50))
            .expect("writable");
        assert_eq!(l.resize_contents_precision(), 50);
        assert!(matches!(
            l.intervene(
                "resize_contents_precision",
                &IntrospectValue::Text("many".into())
            ),
            Err(InterveneError::TypeMismatch)
        ));
        assert_eq!(
            l.resize_contents_precision(),
            50,
            "the refusal changed nothing"
        );
        // R1496 REVERSES R1454 here. This assertion read `assert_eq!(l.save_state(), layout().save_state())` — "it does not
        // touch the saved layout; it decides what a consumer MEASURES, not
        // what the header IS" — and that line does not hold. The toolkit saves
        // it: `write()` serialises `resizeContentsPrecision` beside the section items, and reads it back
        // conditionally so older streams still load, which is the same
        // absent-is-the-older-shape rule this decoder already follows.
        //
        // R1454 had in fact already abandoned "policy, not state" once — it
        // made this field a `Signal` on the grounds that the bound is an INPUT
        // to a painted result — and kept the half that persistence depended on.
        // A snapshot that replays every content-fitted width while dropping the
        // rule that sized them restores an outcome without its cause.
        assert_ne!(l.save_state(), layout().save_state());
        assert_eq!(l.save_state().resize_contents_precision, 50);
        // But it is reactive, because a consumer reads it in its view fn: a
        // write that did not re-run the view could never reach the hints.
        // (The first draft used a plain `Cell` and the demo caught exactly
        // that — the knob read back its new value and nothing moved.)
        let seen = std::rc::Rc::new(std::cell::Cell::new(0_usize));
        let owner = crate::reactive::Owner::new();
        let probe = std::rc::Rc::clone(&seen);
        let l2 = ColumnLayout::new(vec![100, 120]);
        owner.run(|| probe.set(l2.resize_contents_precision()));
        assert_eq!(seen.get(), DEFAULT_CONTENTS_PRECISION);
        assert_eq!(
            l2.contents_precision.revision(),
            0,
            "reading inside a scope subscribes without writing"
        );
        l2.set_resize_contents_precision(25);
        assert_eq!(
            l2.contents_precision.revision(),
            1,
            "and a write advances the revision the subscriber wakes on"
        );
    }

    #[test]
    fn read_column_layout_round_trips_query_encode() {
        let l = layout();
        l.resize_section(0, 200);
        l.move_section(0, 2);
        l.set_section_hidden(3, true);
        let expected = l.save_state();

        let v = read_column_layout(&Probe(l));
        assert_eq!(v.state, expected);
        assert_eq!(
            v.placements,
            vec![
                SectionPlacement {
                    visual: 0,
                    logical: 1,
                    x: 0,
                    size: 120
                },
                SectionPlacement {
                    visual: 1,
                    logical: 2,
                    x: 120,
                    size: 140
                },
                // Section 0 kept the 200 it was resized to before it moved.
                SectionPlacement {
                    visual: 2,
                    logical: 0,
                    x: 260,
                    size: 200
                },
                // Logical 3 is hidden, so visual 3 is painted nowhere — and
                // the surviving entries keep their FULL-order visual indices.
            ]
        );
    }

    #[test]
    fn a_placement_carries_the_full_order_visual_index_not_its_slot() {
        // The distinction a hit test depends on: hiding section 0 does not
        // renumber the sections after it, so a drop classified from a tag
        // still names the position the permutation knows.
        let l = layout();
        l.set_section_hidden(0, true);
        let p = l.visible_placements();
        assert_eq!(
            p.iter().map(|p| p.visual).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(p[0].x, 0, "the first painted section starts at the edge");
        assert_eq!(p[1].x, 120, "offsets close the gap the hidden one left");
        assert_eq!(l.visible_total(), 420);
    }

    // ----- R1491: the sort indicator is header state -----

    /// Press `visual` so [`ReorderModel::begin_drag_payload`] can arm a drag, then hand back the payload the
    /// router would carry. R1496 — a header with the toolkit's two permissions
    /// turned on, the posture a reorderable, sortable view configures. `layout()`
    /// keeps the toolkit's defaults (both off) so that the tests below which
    /// assert a refusal are asserting the *boot* header rather than one this
    /// helper disarmed.
    fn interactive_layout() -> ColumnLayout {
        let l = layout();
        l.set_sections_movable(true);
        l.set_sections_clickable(true);
        l
    }

    /// R1496 — a `PointerDown` on the section painted at `visual`, through the
    /// layout's own `invoke` so BOTH presses are recorded: the reorder model's,
    /// which the drag consumes, and the layout's, which outlives it.
    fn press(l: &ColumnLayout, visual: usize) -> DragPayload {
        l.invoke("send", &text(&format!("{visual}:PointerDown")))
            .expect("send accepts a pointer payload");
        l.begin_section_drag(Cow::Borrowed("col"))
            .expect("a pressed section of a movable header arms a drag")
    }

    /// R1496 — the trailing `PointerUp` the R794 router synthesizes for a
    /// press-release that never became a drag. Answers the clicked **logical**
    /// section, or `None`.
    fn release(l: &ColumnLayout, visual: usize) -> Option<usize> {
        match l.invoke("send", &text(&format!("{visual}:PointerUp"))) {
            Ok(IntrospectValue::Int(logical)) => usize::try_from(logical).ok(),
            _ => None,
        }
    }

    /// A drop over the section painted at `visual`, on its trailing half — the
    /// point the router resolves for a cursor released past that section's
    /// midpoint.
    fn drop_after(visual: usize) -> DropPoint {
        DropPoint {
            tag: format!("colhdr#{visual}"),
            x_rel: 0.9,
            y_rel: 0.5,
        }
    }

    #[test]
    fn a_moved_section_carries_its_sort_indicator() {
        // THE claim. The toolkit keys `sortIndicatorSection` logically for exactly this reason,
        // and it is why the indicator belongs to the header rather than to
        // whatever is doing the sorting.
        let l = layout();
        l.set_sort_indicator(0, true);
        l.move_section(0, 2);

        assert_eq!(l.order(), vec![1, 2, 0, 3]);
        assert_eq!(
            l.sort_indicator(),
            Some((0, true)),
            "the indicator names the column, which did not change"
        );
        assert_eq!(l.visual_index(0), Some(2));
        // The discriminator, and the reason a visual-keyed indicator is not
        // merely a different spelling: it would paint the arrow on whatever
        // section is now FIRST. Ask the placements the question a header
        // builder asks, and the arrow has to be on the third painted one.
        let arrowed: Vec<usize> = l
            .visible_placements()
            .iter()
            .filter(|p| col_sort_dir(l.sort_indicator(), p.logical).is_some())
            .map(|p| p.visual)
            .collect();
        assert_eq!(arrowed, vec![2], "the glyph travelled with its section");
    }

    #[test]
    fn a_hidden_sort_section_keeps_the_indicator_it_stops_painting() {
        // The toolkit's rule: hiding a section does not unsort the view. The
        // indicator survives so that showing the section again restores the
        // arrow instead of silently landing on nothing.
        let l = layout();
        l.set_sort_indicator(2, false);
        l.set_section_hidden(2, true);

        assert_eq!(l.sort_indicator(), Some((2, false)));
        assert!(
            l.visible_placements()
                .iter()
                .all(|p| col_sort_dir(l.sort_indicator(), p.logical).is_none()),
            "no painted section shows the arrow while its section is hidden"
        );
        l.set_section_hidden(2, false);
        assert_eq!(l.visual_index(2), Some(2), "it kept its place too");
    }

    #[test]
    fn a_click_is_a_press_and_a_release_on_the_same_section() {
        // R1496 — the toolkit's rule (`logicalIndexAt(pos) == d->pressed`). Both halves are asserted because
        // either alone passes a broken implementation — "always Some" and
        // "always None" each satisfy one.
        let l = interactive_layout();

        press(&l, 1);
        assert_eq!(
            release(&l, 1),
            Some(1),
            "released on the section it was pressed on: a click on logical 1"
        );
        assert_eq!(l.order(), vec![0, 1, 2, 3], "and it moved nothing");

        press(&l, 0);
        assert_eq!(
            release(&l, 2),
            None,
            "a press that slid onto a neighbour activates neither of them"
        );
    }

    #[test]
    fn a_drag_release_reports_no_click_of_its_own() {
        // R1496 — the regression R1491 shipped. Its `release_section` derived the click from
        // the permutation, so a section dragged across the strip and dropped
        // back into its own gap read as a click and sorted the column the user
        // had just decided not to move. The toolkit calls that a move, by `startDragDistance`;
        // so does this workspace, in ONE place (R794 withholds the trailing
        // `PointerUp`). The drop commit therefore has no click to report — it cannot,
        // because it does not know how far the cursor travelled, which is
        // exactly why it must not be asked.
        let l = interactive_layout();

        let payload = press(&l, 0);
        l.end_section_drag(&payload, Some(&drop_after(2)));
        assert_eq!(l.order(), vec![1, 2, 0, 3], "the drag committed");

        let payload = press(&l, 0);
        l.end_section_drag(&payload, None);
        assert_eq!(
            l.order(),
            vec![1, 2, 0, 3],
            "and a drop back into its own gap commits nothing"
        );
        assert_eq!(
            l.sort_indicator(),
            None,
            "neither release sorted anything: the drop commit is not a click"
        );
    }

    #[test]
    fn a_click_on_a_moved_section_names_the_column_not_the_position() {
        // The composition the two halves exist for: after a reorder, the
        // section painted third IS logical 0, and clicking it has to sort
        // logical 0. A click that answered the visual index would answer 2
        // here and sort the wrong column.
        let l = interactive_layout();
        l.move_section(0, 2);

        press(&l, 2);
        assert_eq!(release(&l, 2), Some(0));
    }

    #[test]
    fn the_two_permissions_are_independent() {
        // R1496 — THE claim, and the reason both properties exist rather than
        // one. The toolkit keeps `sectionsMovable` and `sectionsClickable` apart, and a sortable-but-pinned
        // header is the commoner of the two shapes.
        let l = layout();
        assert!(!l.sections_movable(), "off by default, as in the toolkit");
        assert!(!l.sections_clickable(), "off by default, as in the toolkit");

        l.set_sections_clickable(true);
        l.invoke("send", &text("1:PointerDown"))
            .expect("send accepts a pointer payload");
        assert!(
            l.begin_section_drag(Cow::Borrowed("col")).is_none(),
            "a header that is not movable arms no drag session at all"
        );
        assert_eq!(
            release(&l, 1),
            Some(1),
            "and the press it refused to drag is still a click"
        );

        let l = layout();
        l.set_sections_movable(true);
        press(&l, 1);
        assert_eq!(
            release(&l, 1),
            None,
            "the mirror: a movable header that is not clickable reports no click"
        );
    }

    #[test]
    fn the_programmatic_move_ignores_the_movable_rule() {
        // R1496 — the R1494 split, applied to the other gesture: the property
        // governs what a HAND may do. The toolkit's `moveSection()` reorders a header the
        // user cannot drag, and so does this.
        let l = layout();
        assert!(!l.sections_movable());
        l.move_section(0, 2);
        assert_eq!(
            l.order(),
            vec![1, 2, 0, 3],
            "moveSection is not the gesture"
        );
        l.swap_sections(0, 1);
        assert_eq!(l.order(), vec![2, 1, 0, 3], "nor is swapSections");
    }

    #[test]
    fn a_press_that_wanders_off_the_strip_activates_nothing() {
        // R1496 — a leave or a cancel ends the press. Without this the pressed
        // section outlives the gesture and the NEXT release anywhere on the
        // strip sorts it.
        let l = interactive_layout();

        press(&l, 1);
        l.invoke("send", &text("1:PointerLeave"))
            .expect("send accepts a pointer payload");
        assert_eq!(release(&l, 1), None, "the press was abandoned");

        press(&l, 1);
        l.invoke("send", &text("1:PointerCancel"))
            .expect("send accepts a pointer payload");
        assert_eq!(release(&l, 1), None, "and cancel abandons it too");
    }

    #[test]
    fn a_second_click_in_place_is_not_swallowed_by_the_double_click_edge() {
        // Measured on the real router: the SECOND in-place gesture on one
        // section arrives as `PointerDown`, `DoubleClick`, `PointerUp`. The
        // first draft of `handle_send` dropped the press on any event it did
        // not recognise, so every other click vanished and the indicator could
        // never be cycled past `ascending` by hand.
        let l = interactive_layout();

        press(&l, 1);
        assert_eq!(release(&l, 1), Some(1));
        l.cycle_sort_indicator(1);
        assert_eq!(l.sort_indicator(), Some((1, true)));

        press(&l, 1);
        l.invoke("send", &text("1:DoubleClick"))
            .expect("send accepts a pointer payload");
        assert_eq!(
            release(&l, 1),
            Some(1),
            "the second notification about a live press does not end it"
        );
    }

    #[test]
    fn a_click_cycles_the_indicator_through_qt_s_three_states() {
        let l = layout();
        l.cycle_sort_indicator(1);
        assert_eq!(l.sort_indicator(), Some((1, true)));
        l.cycle_sort_indicator(1);
        assert_eq!(l.sort_indicator(), Some((1, false)));
        l.cycle_sort_indicator(1);
        assert_eq!(l.sort_indicator(), None, "the third click unsorts");
        // A different section starts its own cycle at ascending rather than
        // continuing the previous section's.
        l.cycle_sort_indicator(1);
        l.cycle_sort_indicator(3);
        assert_eq!(l.sort_indicator(), Some((3, true)));
    }

    #[test]
    fn an_out_of_range_section_cannot_take_the_indicator() {
        let l = layout();
        l.set_sort_indicator(2, true);
        l.set_sort_indicator(4, false);
        assert_eq!(
            l.sort_indicator(),
            Some((2, true)),
            "a stale column index leaves the arrow where it was"
        );
        l.cycle_sort_indicator(9);
        assert_eq!(l.sort_indicator(), Some((2, true)));
    }

    #[test]
    fn hiding_the_arrow_keeps_which_section_is_sorted() {
        // The toolkit separates `sortIndicatorShown` from `sortIndicatorSection`, and a consumer that conflated
        // them would lose the sort on a view toggle.
        let l = layout();
        l.set_sort_indicator(1, true);
        l.set_sort_indicator_shown(true);
        l.set_sort_indicator_shown(false);

        assert!(!l.is_sort_indicator_shown());
        assert_eq!(l.sort_indicator(), Some((1, true)));
    }

    #[test]
    fn a_saved_layout_restores_the_sort_it_was_saved_with() {
        let l = layout();
        l.set_sort_indicator(2, false);
        l.set_sort_indicator_shown(true);
        l.move_section(0, 3);
        let saved = l.save_state();

        let other = layout();
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(other.sort_indicator(), Some((2, false)));
        assert!(other.is_sort_indicator_shown());
        assert_eq!(other.order(), saved.order);
    }

    #[test]
    fn a_pre_r1491_snapshot_restores_as_an_unsorted_header() {
        // The `modes` precedent: a snapshot taken before this field existed is
        // a valid older shape, not a malformed one.
        let older = serde_json::json!({
            "order": [1, 0, 2, 3],
            "sizes": [100, 120, 140, 160],
            "hidden": [false, false, false, false],
            "modes": ["interactive", "interactive", "interactive", "interactive"],
        });
        let decoded =
            ColumnLayoutState::from_json(&older).expect("an older snapshot still decodes");
        assert_eq!(decoded.sort_indicator, None);
        assert!(!decoded.sort_indicator_shown);
    }

    #[test]
    fn the_header_reads_and_writes_its_permissions_over_the_wire() {
        // R1496 — measured absent before the round: both paths answered
        // `UnknownIntrospectPath`, so a client could not tell a header that
        // refused a drag from one that had no drag to give.
        let l = layout();
        assert_eq!(
            l.query("sections_movable"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            l.query("sections_clickable"),
            Some(IntrospectValue::Bool(false))
        );

        l.intervene("sections_movable", &IntrospectValue::Bool(true))
            .expect("the permission is writable");
        assert!(l.sections_movable());
        assert!(
            !l.sections_clickable(),
            "and the write hit only one of them"
        );

        l.intervene("sections_clickable", &IntrospectValue::Bool(true))
            .expect("so is the other");
        assert!(l.sections_clickable());

        assert_eq!(
            l.intervene("sections_movable", &IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
            "a permission is a boolean, and a client that sent 1 is told so"
        );
    }

    #[test]
    fn a_misspelled_sort_direction_is_reported_not_swallowed() {
        // The distinction `grid_sort_parse` exists for. The lenient
        // `grid_sort_from_str` reads this as "unsorted", which would restore a
        // header the client did not ask for and report success.
        let mut state = layout().save_state().to_json();
        state["sort_indicator"] = serde_json::json!("1:asending");
        assert_eq!(ColumnLayoutState::from_json(&state), None);

        state["sort_indicator"] = serde_json::json!("1:ascending");
        assert_eq!(
            ColumnLayoutState::from_json(&state).map(|s| s.sort_indicator),
            Some(Some((1, true))),
            "the correctly spelled one still decodes"
        );
    }

    #[test]
    fn a_restore_naming_a_section_this_header_lacks_changes_nothing() {
        // Atomicity: the indicator's range check has to join the length checks
        // ahead of the first write, or a rejected restore leaves the order
        // moved and reports failure.
        let l = layout();
        l.resize_section(0, 200);
        let before = l.save_state();

        let mut bad = before.clone();
        bad.order = vec![3, 2, 1, 0];
        bad.sizes = vec![10, 20, 30, 40];
        bad.sort_indicator = Some((4, true));

        assert!(l.restore_state(&bad).is_err(), "the snapshot is refused");
        assert_eq!(l.save_state(), before, "not one field was written");
    }

    #[test]
    fn the_header_reads_and_writes_its_sort_over_the_wire() {
        let l = layout();
        assert_eq!(l.query("sort_indicator"), Some(text("none")));
        assert_eq!(
            l.query("sort_indicator_section"),
            Some(IntrospectValue::Null)
        );
        assert_eq!(l.query("sort_indicator_order"), Some(text("none")));
        assert_eq!(
            l.query("sort_indicator_shown"),
            Some(IntrospectValue::Bool(false))
        );

        // The cycle reports where it landed, which is the whole reason it
        // returns anything: the caller does not know the direction in advance.
        assert_eq!(
            l.invoke("cycle_sort_indicator", &IntrospectValue::Int(2)),
            Ok(text("2:ascending"))
        );
        assert_eq!(
            l.invoke("cycle_sort_indicator", &IntrospectValue::Int(2)),
            Ok(text("2:descending"))
        );
        assert_eq!(
            l.query("sort_indicator_section"),
            Some(IntrospectValue::Int(2))
        );
        assert_eq!(l.query("sort_indicator_order"), Some(text("descending")));

        assert_eq!(
            l.invoke("set_sort_indicator", &text("0:true")),
            Ok(text("0:ascending"))
        );
        assert_eq!(
            l.invoke("clear_sort_indicator", &IntrospectValue::Null),
            Ok(text("none"))
        );

        l.intervene("sort_indicator", &text("3:descending"))
            .expect("the compound string is the restore half");
        assert_eq!(l.sort_indicator(), Some((3, false)));
        l.intervene("sort_indicator_shown", &IntrospectValue::Bool(true))
            .expect("shown is writable");
        assert!(l.is_sort_indicator_shown());
    }

    #[test]
    fn the_wire_refuses_a_sort_it_cannot_honour() {
        let l = layout();
        // Malformed: the same strictness the `state` door applies, so neither
        // door can do something the other cannot.
        assert_eq!(
            l.intervene("sort_indicator", &text("1:asending")),
            Err(InterveneError::TypeMismatch)
        );
        // Well-formed but not this header's section — a different error,
        // because the client's mistake is a different mistake.
        assert_out_of_range_saying(
            &l.intervene("sort_indicator", &text("9:ascending")),
            "no section 9 in this header",
        );
        assert_refused_saying(
            &l.invoke("cycle_sort_indicator", &IntrospectValue::Int(9)),
            "no section 9 in this header",
        );
        assert_eq!(l.sort_indicator(), None, "no refusal moved the arrow");
    }

    // ----- R1492: a section says what its size is allowed to be -----

    #[test]
    fn every_size_path_honours_the_ceiling() {
        // THE claim, and the reason the clamp had to be lifted rather than
        // repeated: a section gets its size three different ways, and a ceiling
        // only one of them honoured would make the row fill differently
        // depending on a mode the ceiling has nothing to do with.
        let l = layout();
        l.set_maximum_section_size(110);

        // (1) the stored size
        assert_eq!(l.resize_section(0, 400), 110, "stored: clamped down");
        // (2) the content hint
        l.set_resize_mode(1, SectionResizeMode::ResizeToContents);
        l.set_content_widths(vec![100, 900, 100, 100]);
        assert_eq!(l.section_size(1), 110, "content hint: clamped down");
        // (3) the stretch share
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_available_width(Some(5_000));
        assert_eq!(l.section_size(2), 110, "stretch share: clamped down");
        // The row consequently does NOT fill the width it was given — which is
        // correct and is the toolkit's behaviour: a bound the user set
        // outranks a division, and `visible_total` reports the truth rather than a number
        // the grid is not painting.
        assert!(
            l.visible_total() < 5_000,
            "a bounded row cannot fill an unbounded viewport: {}",
            l.visible_total()
        );
    }

    #[test]
    fn the_bounds_are_readable_so_two_identical_sizes_can_be_told_apart() {
        // Measured on the real wire before this round: `resize_section 0:5`
        // (interactive, clamped up) and `resize_section 2:300` (stretch, size
        // derived) BOTH answered 40, and nothing readable distinguished them.
        // The fix is not a new channel — it is that the rule became legible.
        let l = layout();
        l.set_resize_mode(2, SectionResizeMode::Stretch);
        l.set_available_width(Some(240));

        let clamped = l.resize_section(0, 5);
        let derived = l.resize_section(2, 300);
        assert_eq!(clamped, derived, "the two answers are still identical");

        // And now each can be named, from readable state alone.
        assert_eq!(l.minimum_section_size(), DEFAULT_MIN_COL_WIDTH);
        assert_eq!(l.resize_mode(0), SectionResizeMode::Interactive);
        assert_eq!(
            clamped,
            l.minimum_section_size(),
            "asked 5, got the floor: the FLOOR shaped it"
        );
        assert_eq!(l.resize_mode(2), SectionResizeMode::Stretch);
        assert!(
            300 > l.minimum_section_size() && 300 < l.maximum_section_size(),
            "300 was inside the bounds, so no bound shaped THIS one"
        );
    }

    #[test]
    fn the_header_reads_and_writes_both_bounds_over_the_wire() {
        let l = layout();
        assert_eq!(
            l.query("min_section_size"),
            Some(IntrospectValue::Int(i64::from(DEFAULT_MIN_COL_WIDTH)))
        );
        assert_eq!(
            l.query("max_section_size"),
            Some(IntrospectValue::Int(i64::from(DEFAULT_MAX_COL_WIDTH))),
            "unbounded, said out loud rather than by omitting the slot"
        );
        l.intervene("max_section_size", &IntrospectValue::Int(120))
            .expect("the toolkit's setMaximumSectionSize has a wire peer");
        assert_eq!(l.maximum_section_size(), 120);
        assert_eq!(
            l.invoke("resize_section", &text("3:900")),
            Ok(IntrospectValue::Int(120)),
            "and the resize reports the size the ceiling left it"
        );
        l.intervene("min_section_size", &IntrospectValue::Int(80))
            .expect("and so does setMinimumSectionSize");
        assert_eq!(l.minimum_section_size(), 80);
        assert_eq!(
            l.intervene("max_section_size", &text("wide")),
            Err(InterveneError::TypeMismatch),
            "a bound is a number, and a non-number is told so"
        );
    }

    #[test]
    fn a_bound_moved_after_the_fact_re_sizes_the_sections_it_governs() {
        // The bounds are settings, not construction arguments, so they have to
        // reach widths that already exist — including through the layout's own
        // saved state.
        let l = layout();
        assert_eq!(l.visible_widths(), vec![100, 120, 140, 160]);
        l.set_maximum_section_size(130);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 130, 130],
            "the two sections over the new ceiling came down"
        );
        assert_eq!(
            l.save_state().sizes,
            vec![100, 120, 130, 130],
            "and the snapshot records what the header will actually paint"
        );
    }

    #[test]
    fn the_decoded_view_carries_the_sort_the_wire_reported() {
        // `read_column_layout` is the deserialize peer of `query`; a slot the
        // encoder learned and the decoder did not is exactly the drift it
        // exists to prevent.
        let l = layout();
        l.set_sort_indicator(1, true);
        l.set_sort_indicator_shown(true);
        let view = read_column_layout(&Probe(l));
        assert_eq!(view.state.sort_indicator, Some((1, true)));
        assert!(view.state.sort_indicator_shown);
    }

    // ----- R1493: the size a section was given vs the size it has -----

    #[test]
    fn r1493_the_stored_plural_and_the_effective_plural_are_different_reads() {
        // The measurement this round entered on. Under `Interactive` the two
        // agree, which is why one name sufficed for 42 rounds; under `Stretch`
        // they do not, and only one of them is on screen.
        let l = layout();
        l.set_available_width(Some(600));
        assert_eq!(
            ints(&l.query("sizes").expect("sizes")),
            vec![100, 120, 140, 160]
        );
        assert_eq!(
            ints(&l.query("section_sizes").expect("section_sizes")),
            vec![100, 120, 140, 160],
            "interactive: the two reads agree, so neither is wrong yet"
        );

        l.set_all_resize_modes(SectionResizeMode::Stretch);
        assert_eq!(
            ints(&l.query("sizes").expect("sizes")),
            vec![100, 120, 140, 160],
            "the stored sizes survive the mode switch — that is what they are for"
        );
        assert_eq!(
            ints(&l.query("section_sizes").expect("section_sizes")),
            vec![150, 150, 150, 150],
            "and the effective plural reports the shares the header paints"
        );
        // The two plurals now disagree, and the effective one is the one the
        // painted walk produced.
        assert_eq!(
            l.section_sizes(),
            l.visible_placements()
                .iter()
                .map(|p| p.size)
                .collect::<Vec<_>>(),
            "the effective plural IS the painted geometry, not a second derivation"
        );
        // Singular and plural cannot disagree: the singular reads the plural.
        for logical in 0..l.count() {
            assert_eq!(l.section_size(logical), l.section_sizes()[logical]);
        }
    }

    #[test]
    fn r1493_a_hidden_section_reports_the_size_it_would_bring() {
        // A hidden section is painted nowhere, so it has no share; it reports
        // its stored size, which is the fallback the singular always used.
        let l = layout();
        l.set_available_width(Some(600));
        l.set_all_resize_modes(SectionResizeMode::Stretch);
        l.set_section_hidden(3, true);
        let sizes = l.section_sizes();
        assert_eq!(
            sizes.len(),
            4,
            "a hidden section keeps its slot in the plural"
        );
        assert_eq!(&sizes[..3], &[200, 200, 200], "three shares, not four");
        assert_eq!(sizes[3], 160, "and the hidden one reports its stored size");
    }

    #[test]
    fn r1493_the_default_governs_every_shown_section_and_spares_the_hidden() {
        // The toolkit's `setDefaultSectionSize`: the new default applies now, to the sections a
        // user can see it happen to.
        let l = layout();
        assert_eq!(l.default_section_size(), DEFAULT_SECTION_SIZE);
        l.set_section_hidden(2, true);
        assert_eq!(l.set_default_section_size(90), 90, "the applied default");
        assert_eq!(
            l.save_state().sizes,
            vec![90, 90, 140, 90],
            "the hidden section kept the size it will come back at"
        );
        assert_eq!(l.reset_default_section_size(), DEFAULT_SECTION_SIZE);
        assert_eq!(l.save_state().sizes, vec![100, 100, 140, 100]);
    }

    #[test]
    fn r1493_the_default_cannot_name_a_size_the_header_would_refuse() {
        // Derived at read rather than clamped at write, so a bound that moves
        // afterwards takes the default with it — there is no second write path
        // that could have been forgotten.
        let l = layout();
        l.set_default_section_size(300);
        assert_eq!(l.default_section_size(), 300);
        l.set_maximum_section_size(120);
        assert_eq!(
            l.default_section_size(),
            120,
            "the ceiling moved after the default was set, and the default followed"
        );
        l.set_minimum_section_size(200);
        assert_eq!(
            l.default_section_size(),
            200,
            "and the floor does the same, from the other side"
        );
        // The raw value is not lost: widen the bounds and it is back.
        l.set_minimum_section_size(40);
        l.set_maximum_section_size(DEFAULT_MAX_COL_WIDTH);
        assert_eq!(l.default_section_size(), 300);
    }

    #[test]
    fn r1493_a_snapshot_carries_the_rules_that_shaped_its_sizes() {
        // `ColumnLayoutState` calls itself the peer of the toolkit's `saveState()`, and `saveState()` carries
        // these three. Restoring outcomes without the rules that produced them
        // means the next resize obeys a different one.
        let l = layout();
        l.set_minimum_section_size(60);
        l.set_maximum_section_size(150);
        l.set_default_section_size(80);
        let saved = l.save_state();
        assert_eq!(saved.min_section_size, 60);
        assert_eq!(saved.max_section_size, 150);
        assert_eq!(saved.default_section_size, 80);

        let other = layout();
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(
            other.query("min_section_size"),
            Some(IntrospectValue::Int(60))
        );
        assert_eq!(
            other.query("max_section_size"),
            Some(IntrospectValue::Int(150))
        );
        assert_eq!(
            other.query("default_section_size"),
            Some(IntrospectValue::Int(80))
        );
        assert_eq!(
            other.save_state(),
            saved,
            "the restore is total: the whole snapshot came back, rules included"
        );
        // And because the rule arrived, the restored header refuses the same
        // widths the saved one did.
        assert_eq!(other.resize_section(0, 9999), 150);
    }

    #[test]
    fn r1493_a_restore_widens_past_the_outgoing_ceiling() {
        // The ordering assertion: bounds before widths. Restoring a wide
        // layout into a narrowly-bounded header must apply the INCOMING
        // ceiling — the other order truncates each width on the way in and no
        // later bound can widen it back.
        let wide = layout();
        wide.resize_section(0, 400);
        let saved = wide.save_state();
        assert_eq!(saved.sizes[0], 400);

        let narrow = layout();
        narrow.set_maximum_section_size(110);
        assert_eq!(narrow.save_state().sizes, vec![100, 110, 110, 110]);
        narrow
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert_eq!(
            narrow.section_size(0),
            400,
            "the incoming ceiling governs, so the width survived the restore"
        );
        assert_eq!(narrow.save_state(), saved);
    }

    #[test]
    fn r1493_an_inverted_bound_pair_describes_no_header() {
        // Refused with the other shape errors rather than repaired, because
        // repairing it is order-dependent: the two setters drag each other.
        let l = layout();
        let before = l.save_state();
        let mut crossed = before.clone();
        crossed.min_section_size = 200;
        crossed.max_section_size = 100;
        assert!(
            l.restore_state(&crossed).is_err(),
            "the snapshot is refused"
        );
        assert_eq!(l.save_state(), before, "and nothing at all was written");
    }

    #[test]
    fn r1493_an_older_snapshot_decodes_to_the_constants_not_to_zero() {
        // Absent means "taken before this round", the rule `modes` and
        // `sort_indicator` already use. Zero bounds would be a header whose
        // sections must be at most zero wide.
        let older = serde_json::json!({
            "order": [0, 1, 2, 3],
            "sizes": [100, 120, 140, 160],
            "hidden": [false, false, false, false],
        });
        let decoded = ColumnLayoutState::from_json(&older).expect("older shape decodes");
        assert_eq!(decoded.default_section_size, DEFAULT_SECTION_SIZE);
        assert_eq!(decoded.min_section_size, DEFAULT_MIN_COL_WIDTH);
        assert_eq!(decoded.max_section_size, DEFAULT_MAX_COL_WIDTH);
        // Present but not a number is still an error — a client that meant to
        // set a bound and mistyped it is told so.
        let mut malformed = older.clone();
        malformed["max_section_size"] = serde_json::json!("wide");
        assert_eq!(ColumnLayoutState::from_json(&malformed), None);
        // And `Default` is the same shape, not a derived zero.
        let d = ColumnLayoutState::default();
        assert_eq!(d.min_section_size, DEFAULT_MIN_COL_WIDTH);
        assert_eq!(d.max_section_size, DEFAULT_MAX_COL_WIDTH);
        assert_eq!(d.default_section_size, DEFAULT_SECTION_SIZE);
    }

    #[test]
    fn r1493_the_bulk_reset_answers_the_row_it_produced() {
        // Under a stretch header the number written is not the number painted,
        // so the reset reports `section_sizes` — the outcome, not the input.
        let l = layout();
        l.set_available_width(Some(600));
        l.set_all_resize_modes(SectionResizeMode::Stretch);
        let out = l
            .invoke("reset_default_section_size", &IntrospectValue::Null)
            .expect("reset is invokable");
        assert_eq!(
            ints(&out),
            vec![150, 150, 150, 150],
            "the row it produced, not the 100 it wrote"
        );
        assert_eq!(
            ints(&l.query("sizes").expect("sizes")),
            vec![100, 100, 100, 100],
            "and the stored sizes did take the default"
        );
        // The intervene door is the value setter, and reports through the same
        // read-back.
        l.intervene("default_section_size", &IntrospectValue::Int(70))
            .expect("the default is writable");
        assert_eq!(
            l.query("default_section_size"),
            Some(IntrospectValue::Int(70))
        );
        assert_eq!(
            l.intervene("default_section_size", &text("wide")),
            Err(InterveneError::TypeMismatch)
        );
    }

    // ----- R1494: a resize pays the sections after it -----

    /// Boot widths of [`layout`], for the round-trip assertions.
    const BOOT: [u32; 4] = [100, 120, 140, 160];

    #[test]
    fn r1494_off_by_default_and_off_means_the_plain_resize() {
        // The toolkit's default, and the toolkit's split: `resizeSection()` never cascades,
        // so with the property off the interactive path must be the same call.
        let l = layout();
        assert!(!l.cascading_section_resizes(), "off, as in the toolkit");
        assert_eq!(l.interactive_resize_section(0, 200), 200);
        assert_eq!(
            l.save_state().sizes,
            vec![200, 120, 140, 160],
            "nobody else paid: the row simply grew"
        );
        assert_eq!(
            l.visible_total(),
            620,
            "which is the measurement this round entered on"
        );
    }

    #[test]
    fn r1494_a_grow_is_paid_for_by_the_sections_after_it() {
        let l = layout();
        l.set_cascading_section_resizes(true);
        let before = l.visible_total();
        assert_eq!(l.interactive_resize_section(0, 200), 200);
        assert_eq!(
            l.save_state().sizes,
            vec![200, 40, 120, 160],
            "the follower nearest paid first, down to the floor, then the next"
        );
        assert_eq!(
            l.visible_total(),
            before,
            "so the row is exactly as wide as it was — the point of the property"
        );
    }

    #[test]
    fn r1494_a_drag_out_and_back_lands_where_it_started() {
        // Without the memory a cascade is destructive, and a drag that returns
        // to its start would leave the followers squeezed.
        let l = layout();
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 200);
        assert_ne!(l.save_state().sizes, BOOT.to_vec(), "the row did move");
        l.interactive_resize_section(0, 100);
        assert_eq!(
            l.save_state().sizes,
            BOOT.to_vec(),
            "and came back to the exact widths it left"
        );
        // Half way back returns the most-recently-squeezed section first.
        l.interactive_resize_section(0, 200);
        l.interactive_resize_section(0, 180);
        assert_eq!(
            l.save_state().sizes,
            vec![180, 40, 140, 160],
            "the last section to be squeezed is the first to be let go"
        );
    }

    #[test]
    fn r1494_only_interactive_visible_sections_pay() {
        // A `Fixed` section is fixed against a neighbour's drag as much as
        // against its own; a `Stretch` one has no stored width to give; a
        // hidden one is painted nowhere.
        let l = layout();
        l.set_cascading_section_resizes(true);
        l.set_resize_mode(1, SectionResizeMode::Fixed);
        l.interactive_resize_section(0, 200);
        assert_eq!(
            l.save_state().sizes,
            vec![200, 120, 40, 160],
            "the Fixed follower was skipped and the next one paid instead"
        );

        let h = layout();
        h.set_cascading_section_resizes(true);
        h.set_section_hidden(1, true);
        h.interactive_resize_section(0, 200);
        assert_eq!(
            h.save_state().sizes,
            vec![200, 120, 40, 160],
            "and a hidden follower keeps its width too"
        );
    }

    #[test]
    fn r1494_when_the_followers_are_spent_the_row_grows_and_says_so() {
        // The honest limit: the followers pay as far as they can. Refusing the
        // resize instead would be a user asking for a width and not getting
        // it, with nothing to say why.
        let l = layout();
        l.set_cascading_section_resizes(true);
        let floor = l.minimum_section_size();
        let slack: u32 = BOOT[1..].iter().map(|w| w - floor).sum();
        assert_eq!(slack, 300, "what the three followers can give between them");

        assert_eq!(l.interactive_resize_section(0, BOOT[0] + slack), 400);
        assert_eq!(
            l.save_state().sizes,
            vec![400, 40, 40, 40],
            "exactly spent — every follower at the floor"
        );
        assert_eq!(l.visible_total(), 520, "and the row still has not grown");

        assert_eq!(
            l.interactive_resize_section(0, 500),
            500,
            "one pixel further"
        );
        assert_eq!(
            l.save_state().sizes,
            vec![500, 40, 40, 40],
            "nobody could pay, so the section grew alone"
        );
        assert_eq!(
            l.visible_total(),
            620,
            "and visible_total reports the row that is actually painted"
        );
    }

    #[test]
    fn r1494_a_different_section_is_a_different_gesture() {
        // The debt belongs to the section that incurred it. Repaying it out of
        // another section's travel would move sections the user is not
        // touching.
        let l = layout();
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 200);
        assert_eq!(l.save_state().sizes, vec![200, 40, 120, 160]);
        // Now shrink a DIFFERENT section. Section 0's victims must not be
        // repaid out of it.
        l.interactive_resize_section(2, 100);
        assert_eq!(
            l.save_state().sizes,
            vec![200, 40, 100, 160],
            "only the new anchor moved"
        );
        // And shrinking section 0 now has no memory to repay, because the
        // gesture that built it ended.
        l.interactive_resize_section(0, 100);
        assert_eq!(
            l.save_state().sizes,
            vec![100, 40, 100, 160],
            "the old debt did not follow the new gesture"
        );
    }

    #[test]
    fn r1494_a_programmatic_resize_ends_the_gesture() {
        // `resize_section` can move a section the cascade remembers, which
        // would make the remembered size a lie.
        let l = layout();
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 200);
        l.resize_section(1, 90);
        l.interactive_resize_section(0, 100);
        assert_eq!(
            l.save_state().sizes,
            vec![100, 90, 120, 160],
            "no stale repayment overwrote the width just written"
        );
    }

    #[test]
    fn r1494_turning_it_off_drops_the_debt_it_created() {
        let l = layout();
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 200);
        l.set_cascading_section_resizes(false);
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 100);
        assert_eq!(
            l.save_state().sizes,
            vec![100, 40, 120, 160],
            "the followers were not repaid by a rule that had been withdrawn"
        );
    }

    #[test]
    fn r1494_the_rule_is_readable_writable_and_saved() {
        let l = layout();
        assert_eq!(
            l.query("cascading_section_resizes"),
            Some(IntrospectValue::Bool(false))
        );
        l.intervene("cascading_section_resizes", &IntrospectValue::Bool(true))
            .expect("the toolkit's property has a wire peer");
        assert!(l.cascading_section_resizes());
        assert!(
            l.save_state().cascading_section_resizes,
            "and saveState carries it"
        );
        assert_eq!(
            l.intervene("cascading_section_resizes", &IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        );

        // Restore into a header that does not cascade, and the rule travels.
        let saved = l.save_state();
        let other = layout();
        assert!(!other.cascading_section_resizes());
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert!(
            other.cascading_section_resizes(),
            "a restore replays the rule, not only the widths it produced"
        );
        // An older snapshot has no such field and decodes to the toolkit's
        // default.
        let older = serde_json::json!({
            "order": [0, 1, 2, 3],
            "sizes": BOOT,
            "hidden": [false, false, false, false],
        });
        let decoded = ColumnLayoutState::from_json(&older).expect("older shape decodes");
        assert!(!decoded.cascading_section_resizes);
    }

    #[test]
    fn r1494_the_wire_tells_the_two_resizes_apart() {
        // The same payload through the two methods, one cascading and one not,
        // is the assertion that they are genuinely different entry points.
        let l = layout();
        l.set_cascading_section_resizes(true);
        assert_eq!(
            l.invoke("resize_section", &text("0:200")),
            Ok(IntrospectValue::Int(200))
        );
        assert_eq!(
            l.save_state().sizes,
            vec![200, 120, 140, 160],
            "the programmatic one never cascades, cascading on or not"
        );

        let c = layout();
        c.set_cascading_section_resizes(true);
        assert_eq!(
            c.invoke("interactive_resize_section", &text("0:200")),
            Ok(IntrospectValue::Int(200))
        );
        assert_eq!(
            c.save_state().sizes,
            vec![200, 40, 120, 160],
            "and the interactive one does"
        );
        // "an out-of-range section is refused by both, through one parser" —
        // and R1564 makes that assertable rather than commented: both arms
        // reach the one `require_section`, so both say the same sentence.
        assert_refused_saying(
            &c.invoke("interactive_resize_section", &text("9:200")),
            "no section 9 in this header",
        );
    }

    /// The viewport the R1498 tests publish. `BOOT` sums to 520, so the three
    /// leading sections take 360 and there are 240 left for whoever is last.
    const VIEWPORT_W: u32 = 600;

    fn filled() -> ColumnLayout {
        let l = layout();
        l.set_available_width(Some(VIEWPORT_W));
        l
    }

    fn effective_modes(l: &ColumnLayout) -> Vec<String> {
        match l.query("effective_resize_modes") {
            Some(IntrospectValue::Json(serde_json::Value::Array(a))) => a
                .iter()
                .map(|m| m.as_str().unwrap_or_default().to_string())
                .collect(),
            other => panic!("effective_resize_modes: {other:?}"),
        }
    }

    #[test]
    fn r1498_off_by_default_and_the_row_falls_short_of_its_viewport() {
        let l = filled();
        assert!(!l.stretch_last_section(), "the toolkit's default");
        assert_eq!(
            l.visible_total(),
            520,
            "the entry measurement: the row does not fill the width it was given"
        );
        assert_eq!(l.available_width(), Some(VIEWPORT_W));
        assert_eq!(l.visible_widths(), vec![100, 120, 140, 160]);
    }

    #[test]
    fn r1498_the_last_painted_section_absorbs_the_leftover() {
        let l = filled();
        l.set_stretch_last_section(true);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 140, 240],
            "the last section takes what the other three left over"
        );
        assert_eq!(
            l.visible_total(),
            VIEWPORT_W,
            "so the row fills its viewport"
        );
        assert_eq!(
            l.section_position(3),
            Some(360),
            "and the geometry every hit test reads agrees"
        );
    }

    #[test]
    fn r1498_the_rule_is_keyed_by_position_not_by_column() {
        // The discriminator against `Stretch` on the last column, measured on
        // the real binding before the round: with a mode, hiding that column
        // dropped the fill entirely and moving it painted the fill wherever the
        // column went. The rule belongs to the header, so it stays put.
        let l = filled();
        l.set_stretch_last_section(true);

        l.set_section_hidden(3, true);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 380],
            "hiding the filled section promotes the one now painted last"
        );
        assert_eq!(l.visible_total(), VIEWPORT_W, "the row still fills");

        l.set_section_hidden(3, false);
        l.move_section(3, 0);
        assert_eq!(l.order(), vec![3, 0, 1, 2]);
        assert_eq!(
            l.visible_widths(),
            vec![160, 100, 120, 220],
            "dragged to the front it is an ordinary section again, and the \
             fill stayed at the end of the row"
        );
        assert_eq!(l.visible_total(), VIEWPORT_W);
    }

    #[test]
    fn r1498_it_overrides_the_mode_set_on_the_last_section() {
        // The toolkit states this on the property itself: "this property will
        // override the resize mode set on the last section in the header".
        let l = filled();
        l.set_resize_mode(3, SectionResizeMode::Fixed);
        l.set_stretch_last_section(true);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 140, 240],
            "a Fixed last section is filled anyway"
        );
        assert_eq!(
            l.resize_mode(3),
            SectionResizeMode::Fixed,
            "the mode that was SET is still the one reported"
        );
        assert_eq!(
            l.effective_resize_mode(3),
            SectionResizeMode::Stretch,
            "and the one the layout applies is a separate read"
        );
        assert_eq!(
            effective_modes(&l),
            vec!["interactive", "interactive", "interactive", "stretch"],
            "the plural says the same as the singular — the R1493 rule"
        );
        assert_eq!(
            l.query("effective_resize_mode.3"),
            Some(text("stretch")),
            "and both faces are on the wire"
        );
        assert_eq!(
            l.query("resize_mode.3"),
            Some(text("fixed")),
            "beside the stored one, which a mode cycle still reads"
        );
    }

    #[test]
    fn r1498_the_last_section_shares_with_the_other_stretch_sections() {
        // Overriding the last section's mode to `Stretch` means the division
        // that already exists does the work: four sections, two of them
        // stretching, and no second sizing algorithm.
        let l = filled();
        l.set_resize_mode(1, SectionResizeMode::Stretch);
        l.set_stretch_last_section(true);
        assert_eq!(
            l.visible_widths(),
            vec![100, 180, 140, 180],
            "600 less the 240 the fixed pair take, split two ways"
        );
        assert_eq!(l.visible_total(), VIEWPORT_W);
    }

    #[test]
    fn r1498_the_stored_width_is_untouched_so_withdrawing_the_rule_restores_it() {
        // The toolkit has to remember a `lastSectionSize` because the toolkit writes the
        // stretched width into the section. Nothing here writes, so there is
        // nothing to remember.
        let l = filled();
        l.set_stretch_last_section(true);
        assert_eq!(l.section_size(3), 240, "painted");
        assert_eq!(
            l.save_state().sizes,
            vec![100, 120, 140, 160],
            "stored — the snapshot records what the user set, not the fill"
        );
        l.set_stretch_last_section(false);
        assert_eq!(
            l.visible_widths(),
            vec![100, 120, 140, 160],
            "withdrawing the rule hands the section back its own width"
        );
    }

    #[test]
    fn r1498_a_filled_section_does_not_pay_for_a_neighbours_resize() {
        // R1494's cascade takes from `Interactive` sections. A filled section
        // derives its width, so squeezing its stored size would move no pixels
        // while the cascade counted the debt as paid.
        let l = filled();
        l.set_stretch_last_section(true);
        l.set_cascading_section_resizes(true);
        l.interactive_resize_section(0, 400);
        assert_eq!(
            l.save_state().sizes,
            vec![400, 40, 40, 160],
            "the two interactive followers paid to the floor; the filled one \
             was skipped and kept the width it will come back at"
        );
        assert_eq!(
            l.visible_widths(),
            vec![400, 40, 40, 120],
            "and the fill absorbed what the followers could not cover"
        );
        assert_eq!(l.visible_total(), VIEWPORT_W);
    }

    #[test]
    fn r1498_with_no_published_viewport_the_last_section_keeps_its_size() {
        // The same answer `Stretch` already gives when there is nothing to
        // divide: a section with no leftover to take keeps its stored size
        // rather than collapsing.
        let l = layout();
        l.set_stretch_last_section(true);
        assert_eq!(l.available_width(), None);
        assert_eq!(l.visible_widths(), vec![100, 120, 140, 160]);
        assert_eq!(l.visible_total(), 520);
    }

    #[test]
    fn r1498_when_the_others_overflow_the_last_section_falls_to_the_floor() {
        // Honest rather than refusing: the leading sections already exceed the
        // viewport, so there is no leftover and the fill clamps to the floor
        // every other derived width clamps to (R1492).
        let l = layout();
        l.set_available_width(Some(300));
        l.set_stretch_last_section(true);
        assert_eq!(l.visible_widths(), vec![100, 120, 140, 40]);
        assert_eq!(
            l.visible_total(),
            400,
            "and visible_total reports the row that is actually painted"
        );
    }

    #[test]
    fn r1498_every_section_hidden_leaves_nothing_to_fill() {
        let l = filled();
        l.set_stretch_last_section(true);
        for c in 0..4 {
            l.set_section_hidden(c, true);
        }
        assert!(l.visible_placements().is_empty());
        assert_eq!(l.visible_total(), 0);
        assert_eq!(
            l.effective_resize_mode(3),
            SectionResizeMode::Interactive,
            "a hidden section is painted nowhere, so it is not the last one"
        );
    }

    #[test]
    fn r1498_the_rule_is_readable_writable_and_saved() {
        let l = filled();
        assert_eq!(
            l.query("stretch_last_section"),
            Some(IntrospectValue::Bool(false))
        );
        l.intervene("stretch_last_section", &IntrospectValue::Bool(true))
            .expect("the toolkit's property has a wire peer");
        assert!(l.stretch_last_section());
        assert!(
            l.save_state().stretch_last_section,
            "and saveState carries it"
        );
        assert_eq!(
            l.intervene("stretch_last_section", &IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        );

        let saved = l.save_state();
        let other = filled();
        assert!(!other.stretch_last_section());
        other
            .restore_state(&saved)
            .expect("the snapshot describes this header");
        assert!(
            other.stretch_last_section(),
            "a restore replays the rule, not only the widths it produced"
        );
        assert_eq!(
            other.visible_widths(),
            vec![100, 120, 140, 240],
            "and the restored header fills, from the stored widths it was given"
        );

        // An older snapshot has no such field. Here the toolkit's default and
        // the pre-R1498 header agree — that header did not fill either — so
        // unlike the R1496 permissions there is no divergence to encode.
        let older = serde_json::json!({
            "order": [0, 1, 2, 3],
            "sizes": BOOT,
            "hidden": [false, false, false, false],
        });
        let decoded = ColumnLayoutState::from_json(&older).expect("older shape decodes");
        assert!(!decoded.stretch_last_section);
    }

    #[test]
    fn r1498_splitting_the_rule_writes_out_kept_both_halves_of_the_fall_through() {
        // `intervene_rule` signals "not mine" with `UnknownPath`, which is safe
        // only because no decoder it calls can produce that error. This asserts
        // the three outcomes stayed apart across the split: a rule applies, a
        // non-rule path still reaches the embedded reorder model, and an
        // unknown one is still unknown.
        let l = layout();
        assert_eq!(
            l.intervene("max_section_size", &IntrospectValue::Int(500)),
            Ok(())
        );
        assert_eq!(l.maximum_section_size(), 500);
        assert_eq!(
            l.intervene("focused_index", &IntrospectValue::Int(2)),
            Ok(()),
            "a reorder slot still falls through"
        );
        assert_eq!(l.query("focused_index"), Some(IntrospectValue::Int(2)));
        assert_eq!(
            l.intervene("no_such_rule", &IntrospectValue::Bool(true)),
            Err(InterveneError::UnknownPath)
        );
        assert_eq!(
            l.intervene("stretch_last_section", &IntrospectValue::Text("on".into())),
            Err(InterveneError::TypeMismatch),
            "a rule that IS this header's reports the decode error, not \
             'unknown' — the two must not collapse into one answer"
        );
    }
}
