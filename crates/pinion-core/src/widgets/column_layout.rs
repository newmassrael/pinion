//! R1451 §5.27 §5.51 — **header section layout**: the one place a grid's
//! column *order*, *size*, and *visibility* are held together, keyed the way
//! Qt's `QHeaderView` keys them.
//!
//! ## The composition that had no home
//!
//! Every column axis was already in tree — width (R785/R786), visibility
//! (R990), sort (R778), filter (R783/R997), frozen panes (R859), and section
//! order (R1450) — but each lived in its own binding or holder, and the
//! *composition* of the first three did not exist. The consequence was not a
//! missing convenience but a wrong answer: [`ColumnWidths`] indexes widths by
//! **screen position**, so moving a column left the widths behind. Qt keys
//! `sectionSize` and `isSectionHidden` by the **logical** section, which is
//! exactly why a resized column in a Qt view keeps its width when dragged
//! elsewhere.
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
//! ## Hidden sections keep their place (Qt's rule, not a simplification)
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
//! ## AI clients (§2 #7 + §2 #2 — where Qt cannot follow)
//!
//! Qt persists a header as `QHeaderView::saveState()`, an **opaque versioned
//! `QByteArray`**: an agent can round-trip it but can neither read "how wide
//! is the third column now" out of it nor author one without a live widget.
//! Here the same state is [`ColumnLayoutState`] — typed, readable field by
//! field through [`query`](ColumnLayout::query) (`state`, `sizes`, `hidden`,
//! `visible_sections`, `section_position.<logical>`, `logical_index_at.<x>`,
//! …) and writable whole through
//! [`intervene`](ColumnLayout::intervene)`("state", …)`, the restore half.
//! Section mutation is Qt's own vocabulary over the wire:
//! `move_section` / `swap_sections` / `resize_section` /
//! `set_section_hidden`.

use std::rc::Rc;

use crate::composite_tag::parse_pair;
use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
use crate::reactive::{Owner, Signal};
use crate::widgets::column_widths::ColumnWidths;
use crate::widgets::reorder::{ReorderAxis, ReorderModel};

/// R1451 §5.27 — a whole header layout as data: the peer of Qt's
/// `QHeaderView::saveState()` / `restoreState()`, except every field is
/// readable and authorable instead of an opaque byte blob.
///
/// `order` is `order[visual] = logical`; `sizes` and `hidden` are indexed by
/// **logical** section, which is what makes the snapshot survive a reorder —
/// restoring it into a layout whose columns have since been moved puts each
/// section's size and visibility back on the section, not on the position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLayoutState {
    /// The visual permutation (`order[visual] = logical`).
    pub order: Vec<usize>,
    /// Per-**logical**-section size in logical pixels.
    pub sizes: Vec<u32>,
    /// Per-**logical**-section hidden flag.
    pub hidden: Vec<bool>,
    /// R1452 — per-**logical**-section sizing policy. Qt's `saveState` carries
    /// the modes too; a snapshot without them (one taken before R1452) decodes
    /// as all-`Interactive`, so an older saved layout still restores.
    pub modes: Vec<SectionResizeMode>,
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
        Some(Self {
            order,
            sizes,
            hidden,
            modes,
        })
    }
}

/// R1451 §5.27 — one painted section: its place in the permutation, the
/// column it shows, and where it lands. Produced by
/// [`ColumnLayout::visible_placements`], which is the only walk that applies
/// hiding and sums the cumulative offset.
///
/// `visual` is the section's index in the **full** permutation, hidden
/// sections included (Qt's rule), so it is the identity a hit test and a drag
/// drop-classification speak; it is deliberately *not* the position in this
/// vector, which shifts as neighbours are hidden.
/// `Default` is the empty placement — meaningful only as the filler a
/// fixed-`N` consumer pads a `[SectionPlacement; N]` buffer with (a
/// `WidgetCore::State` is `Copy`, so a binding cannot hold the `Vec`).
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

/// R1454 §5.36 — how many rows a `ResizeToContents` consumer measures by
/// default, matching Qt's `QHeaderView::resizeContentsPrecision` default.
pub const DEFAULT_CONTENTS_PRECISION: usize = 1000;

/// R1452 §5.27 — where a section's size **comes from**: Qt's
/// `QHeaderView::setSectionResizeMode`.
///
/// Before this, every pinion grid had exactly one policy — a stored number —
/// so a column could not fill the viewport and could not fit its content. The
/// mode is per **logical** section, like the size it governs.
///
/// The two questions the rest of the module asks are separate, because Qt
/// answers them differently: [`stores_size`](Self::stores_size) decides whether
/// the size is the stored one or a derived one, and
/// [`user_resizable`](Self::user_resizable) decides whether a *human gesture*
/// may change it. `Fixed` is the mode where those differ — a program may
/// resize it, a drag may not.
// `Signal` snapshots its value (`Owner::snapshot`), so a mode vector held in
// one must be serde round-trippable — the `GridSortState::SortDir` precedent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SectionResizeMode {
    /// The stored size, and the user may drag it. Qt's default.
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
    /// derived modes ignore what [`resize_section`](ColumnLayout::resize_section)
    /// was last given, exactly as Qt does.
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

/// R1451 §5.27 §5.51 — order × size × visibility for one grid's columns,
/// keyed as `QHeaderView` keys them. See the [module docs](self) for the
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
    /// Supplied by the consumer, because that is where the answer is: Qt's
    /// `QHeaderView` does not measure either — `sectionSizeFromContents()`
    /// asks the model / delegate for a `sizeHint`. A grid that measures its
    /// cells feeds the measurement in here; one that knows its content
    /// (fixed-format columns, a monospace grid) computes it directly.
    content_widths: Signal<Vec<u32>>,
    /// R1452 — the width [`SectionResizeMode::Stretch`] sections divide.
    /// `None` until a consumer publishes its viewport, in which case a
    /// `Stretch` section falls back to its stored size rather than collapsing.
    available_width: Signal<Option<u32>>,
    /// R1454 — how many rows a `ResizeToContents` consumer should measure.
    ///
    /// Reactive, and the first draft got that wrong: it was a plain `Cell` on
    /// the reasoning that a sampling bound is "policy, not painted state". But
    /// the bound is an INPUT to a painted result — the consumer reads it in its
    /// view fn to decide what to measure — so a write that did not re-run the
    /// view could not reach the hints at all. The demo caught it: the knob read
    /// back its new value and every content width stayed put.
    contents_precision: Signal<usize>,
}

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
            contents_precision: Signal::new(DEFAULT_CONTENTS_PRECISION),
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

    /// Where logical section `logical` currently sits — Qt's
    /// `visualIndex()`. Counts hidden sections, which keep their place.
    #[must_use]
    pub fn visual_index(&self, logical: usize) -> Option<usize> {
        self.sections.order().iter().position(|&l| l == logical)
    }

    /// Which logical section sits at visual position `visual` — Qt's
    /// `logicalIndex()`.
    #[must_use]
    pub fn logical_index(&self, visual: usize) -> Option<usize> {
        self.sections.order().get(visual).copied()
    }

    /// Move the section at visual `from` to visual `to` — Qt's
    /// `moveSection()`. Sizes and hidden flags are keyed by logical section,
    /// so they travel with it and nothing else has to be updated.
    pub fn move_section(&self, from: usize, to: usize) {
        self.sections.move_section(from, to);
    }

    /// Exchange the sections at two visual positions — Qt's `swapSections()`.
    /// Distinct from [`move_section`](Self::move_section): a swap displaces
    /// exactly one other section, a move shifts every section in between.
    /// Out-of-range indices are ignored.
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

    /// R1452 — where logical section `logical` takes its size from — Qt's
    /// `sectionResizeMode()`.
    #[must_use]
    pub fn resize_mode(&self, logical: usize) -> SectionResizeMode {
        self.modes.get().get(logical).copied().unwrap_or_default()
    }

    /// R1452 — set one section's sizing policy — Qt's
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

    /// R1452 — set every section's policy at once — Qt's
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
    /// a [`ResizeToContents`](SectionResizeMode::ResizeToContents) hint: Qt's
    /// `QHeaderView::resizeContentsPrecision`, default `1000` like Qt's.
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
    /// It lives here, on the header, because that is where Qt puts it and
    /// because it is then readable and writable as data (`query` /
    /// `intervene`) rather than a constant buried in a binding. The *consumer*
    /// honours it, exactly as it supplies the hints themselves — and reads it
    /// inside its view fn, which is why it subscribes.
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

    /// R1452 — publish the per-**logical**-section content size hints (Qt's
    /// delegate `sizeHint`). A vector of the wrong length is ignored, because a
    /// partially-applied hint set would size some columns to another grid's
    /// content.
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

    /// Size of logical section `logical` — Qt's `sectionSize()`, resolved
    /// through the section's [`resize_mode`](Self::resize_mode). `0` for an
    /// unknown section (Qt's answer too).
    ///
    /// A hidden section reports the size it will have when shown rather than
    /// Qt's `0` — [`section_position`](Self::section_position) is the slot that
    /// says "painted nowhere", so reporting the size here is strictly more
    /// information and no ambiguity. A hidden `Stretch` section reports its
    /// stored size: it takes part in no division, because there is no share to
    /// take when it occupies no width.
    #[must_use]
    pub fn section_size(&self, logical: usize) -> u32 {
        if logical >= self.count {
            return 0;
        }
        if let Some(p) = self
            .visible_placements()
            .iter()
            .find(|p| p.logical == logical)
        {
            return p.size;
        }
        self.base_size(logical, self.resize_mode(logical))
    }

    /// The size a section brings to the division: the stored one, or the
    /// content hint. `Stretch` has no size of its own — it gets what is left —
    /// so it falls back to the stored size, which is what it reports when
    /// there is nothing to divide.
    fn base_size(&self, logical: usize, mode: SectionResizeMode) -> u32 {
        match mode {
            SectionResizeMode::ResizeToContents => {
                self.content_width(logical).max(self.sizes.min_width())
            }
            _ => self.sizes.width(logical),
        }
    }

    /// Resize logical section `logical` — Qt's `resizeSection()`. Returns the
    /// applied size after the width model's minimum-width clamp (`0` when the
    /// section does not exist), so an AI client learns the outcome in the same
    /// round-trip it asked for the change.
    ///
    /// R1452 — writes the stored size whatever the mode, but a `Stretch` or
    /// `ResizeToContents` section keeps deriving its size, so the write is only
    /// visible after a switch back. That is Qt (`resizeSection` "has no
    /// effect" outside `Interactive` / `Fixed`), plus the stored value kept
    /// rather than discarded; the return is the size the section actually has,
    /// so a client is never told a number the grid is not painting.
    pub fn resize_section(&self, logical: usize, size: u32) -> u32 {
        if logical >= self.count {
            return 0;
        }
        self.sizes.set_width(logical, size);
        self.section_size(logical)
    }

    /// Whether logical section `logical` is hidden — Qt's
    /// `isSectionHidden()`.
    #[must_use]
    pub fn is_section_hidden(&self, logical: usize) -> bool {
        self.hidden.get().get(logical).copied().unwrap_or(false)
    }

    /// Show or hide logical section `logical` — Qt's `setSectionHidden()`.
    /// The section keeps its visual place and its size while hidden. An
    /// out-of-range section is a silent no-op.
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

    /// How many sections are hidden — Qt's `hiddenSectionCount()`.
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
        // Pass 1 — who is painted, in what mode, at what size of their own.
        let mut painted: Vec<(usize, usize, SectionResizeMode, u32)> =
            Vec::with_capacity(self.count);
        for (visual, logical) in self.sections.order().into_iter().enumerate() {
            if hidden.get(logical).copied().unwrap_or(false) {
                continue;
            }
            let mode = modes.get(logical).copied().unwrap_or_default();
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
                    (share + bonus).max(self.sizes.min_width())
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

    /// The x offset logical section `logical` is painted at — Qt's
    /// `sectionPosition()`. `None` when the section is hidden or unknown
    /// (a hidden section is painted nowhere, so it has no position).
    #[must_use]
    pub fn section_position(&self, logical: usize) -> Option<u32> {
        self.visible_placements()
            .iter()
            .find(|p| p.logical == logical)
            .map(|p| p.x)
    }

    /// Which logical section covers header x offset `x` — Qt's
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

    /// The whole layout as data — Qt's `saveState()`, readable.
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
        }
    }

    /// Restore a saved layout — Qt's `restoreState()`. `false` (and **no
    /// change at all**) when `state` does not describe this header: a wrong
    /// vector length, or an `order` that is not a permutation of
    /// `0..count`.
    ///
    /// Atomic by construction rather than by a pre-check copy: the length
    /// tests are cheap and total, and
    /// [`ReorderModel::set_order`] is itself validate-then-apply, so a
    /// rejected permutation returns before any size or flag is written. The
    /// permutation rule is therefore still checked in exactly one place.
    pub fn restore_state(&self, state: &ColumnLayoutState) -> bool {
        if state.sizes.len() != self.count
            || state.hidden.len() != self.count
            || state.modes.len() != self.count
        {
            return false;
        }
        if !self.sections.set_order(&state.order) {
            return false;
        }
        self.sizes.set_widths(state.sizes.clone());
        self.hidden.set(state.hidden.clone());
        self.modes.set(state.modes.clone());
        true
    }

    /// Header-layout slots for [`ExternalIntrospect::query`], layered over the
    /// reorder slots (`order` / `preview` / `focused_index` / `grabbed`),
    /// which fall through to the embedded [`ReorderModel`]:
    ///
    /// - `state` — the whole [`ColumnLayoutState`] (Qt `saveState`, readable)
    /// - `sizes` / `hidden` — the logical-keyed vectors
    /// - `visible_sections` / `visible_widths` / `visible_total`
    /// - `placements` — the painted geometry ([`SectionPlacement`] per section)
    /// - `hidden_count`
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

        match path {
            "state" => Some(IntrospectValue::Json(self.save_state().to_json())),
            "sizes" => Some(json_of((0..self.count).map(|l| self.sizes.width(l)))),
            "hidden" => Some(json_of(self.hidden.get())),
            "visible_sections" => Some(json_of(self.visible_sections())),
            "visible_widths" => Some(json_of(self.visible_widths())),
            // The painted geometry as data — an agent aims a drag or a click
            // at a section from this without re-deriving a single offset, and
            // without a screenshot. Qt exposes the equivalent only through
            // per-section C++ calls against a live widget.
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
            "content_widths" => Some(json_of(self.content_widths.get())),
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
            _ => match path.split_once('.') {
                Some((head, arg)) => match head {
                    "visual_index" => arg.parse().ok().map(|l| opt_int(self.visual_index(l))),
                    "logical_index" => arg.parse().ok().map(|v| opt_int(self.logical_index(v))),
                    "section_size" => arg
                        .parse()
                        .ok()
                        .map(|l| IntrospectValue::Int(i64::from(self.section_size(l)))),
                    "section_hidden" => arg
                        .parse()
                        .ok()
                        .map(|l| IntrospectValue::Bool(self.is_section_hidden(l))),
                    "section_position" => arg.parse().ok().map(|l| {
                        self.section_position(l).map_or(IntrospectValue::Null, |x| {
                            IntrospectValue::Int(i64::from(x))
                        })
                    }),
                    "logical_index_at" => {
                        arg.parse().ok().map(|x| opt_int(self.logical_index_at(x)))
                    }
                    "resize_mode" => arg.parse().ok().map(|l: usize| {
                        IntrospectValue::Text(self.resize_mode(l).as_wire().to_string())
                    }),
                    "content_width" => arg
                        .parse()
                        .ok()
                        .map(|l| IntrospectValue::Int(i64::from(self.content_width(l)))),
                    _ => None,
                },
                None => None,
            },
        }
        .or_else(|| self.sections.query(path))
    }

    /// Header-layout slots for [`ExternalIntrospect::intervene`]: `state` is
    /// the restore half of the round-trip (Qt's `restoreState`, authorable),
    /// and `sizes` / `hidden` write one vector each. `focused_index` and
    /// `order` fall through to the embedded [`ReorderModel`].
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
                if self.restore_state(&state) {
                    Ok(())
                } else {
                    Err(InterveneError::OutOfRange)
                }
            }
            "sizes" => {
                let sizes: Vec<u32> = json_u64_array(value)
                    .ok_or(InterveneError::TypeMismatch)?
                    .into_iter()
                    .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
                    .collect();
                if sizes.len() != self.count {
                    return Err(InterveneError::OutOfRange);
                }
                self.sizes.set_widths(sizes);
                Ok(())
            }
            "hidden" => {
                let IntrospectValue::Json(serde_json::Value::Array(items)) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let flags: Vec<bool> = items
                    .iter()
                    .map(serde_json::Value::as_bool)
                    .collect::<Option<_>>()
                    .ok_or(InterveneError::TypeMismatch)?;
                if flags.len() != self.count {
                    return Err(InterveneError::OutOfRange);
                }
                self.hidden.set(flags);
                Ok(())
            }
            // R1452 — the two inputs the derived modes read. A grid publishes
            // its measured content and its viewport here; over the wire an
            // agent can do the same to explore a layout without a real grid.
            "content_widths" => {
                let widths: Vec<u32> = json_u64_array(value)
                    .ok_or(InterveneError::TypeMismatch)?
                    .into_iter()
                    .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
                    .collect();
                if widths.len() != self.count {
                    return Err(InterveneError::OutOfRange);
                }
                self.content_widths.set(widths);
                Ok(())
            }
            // R1454 — the row-sampling bound a `ResizeToContents` consumer
            // honours; writable so an agent can shrink it and watch the hints
            // change without rebuilding the grid.
            "resize_contents_precision" => {
                let rows = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                self.set_resize_contents_precision(rows);
                Ok(())
            }
            "available_width" => {
                // `Null` clears the published viewport — the writable peer of
                // the `Null` this slot reads back when nothing is published.
                if matches!(value, IntrospectValue::Null) {
                    self.set_available_width(None);
                } else {
                    let w = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                    self.set_available_width(Some(
                        u32::try_from(w).map_err(|_| InterveneError::OutOfRange)?,
                    ));
                }
                Ok(())
            }
            _ => self.sections.intervene(path, value),
        }
    }

    /// Header-layout actions for [`ExternalIntrospect::invoke`] — Qt's own
    /// section vocabulary, each taking the typed pair wire form
    /// ([`parse_pair`]):
    ///
    /// - `swap_sections` — `"<visual_a>:<visual_b>"`; returns the new order
    /// - `resize_section` — `"<logical>:<px>"`; returns the applied size
    ///   after the minimum-width clamp
    /// - `set_section_hidden` — `"<logical>:<bool>"`; returns the resulting
    ///   visible-section projection, so one round-trip both hides and reports
    ///   what is now painted
    ///
    /// `send` / `move` / `grab` / `grab_cancel` / `move_section` fall through
    /// to the embedded [`ReorderModel`].
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
                let (a, b) = parse_pair::<usize, usize>(&text, ':').ok_or(InvokeError::Rejected)?;
                if a >= self.count || b >= self.count {
                    return Err(InvokeError::Rejected);
                }
                self.swap_sections(a, b);
                Ok(self.query("order").unwrap_or(IntrospectValue::Null))
            }
            "resize_section" => {
                let text = pair_text(args)?;
                let (logical, size) =
                    parse_pair::<usize, u32>(&text, ':').ok_or(InvokeError::Rejected)?;
                if logical >= self.count {
                    return Err(InvokeError::Rejected);
                }
                Ok(IntrospectValue::Int(i64::from(
                    self.resize_section(logical, size),
                )))
            }
            "set_section_hidden" => {
                let text = pair_text(args)?;
                let (logical, hide) =
                    parse_pair::<usize, bool>(&text, ':').ok_or(InvokeError::Rejected)?;
                if logical >= self.count {
                    return Err(InvokeError::Rejected);
                }
                self.set_section_hidden(logical, hide);
                Ok(self
                    .query("visible_sections")
                    .unwrap_or(IntrospectValue::Null))
            }
            // R1452 — Qt's setSectionResizeMode, both overloads. Each returns
            // the resulting painted widths, because changing one section's
            // policy re-sizes every `Stretch` section sharing the row with it —
            // the outcome an agent needs is the row, not the section.
            "set_resize_mode" => {
                let text = pair_text(args)?;
                let (logical, mode) = parse_pair::<usize, SectionResizeMode>(&text, ':')
                    .ok_or(InvokeError::Rejected)?;
                if logical >= self.count {
                    return Err(InvokeError::Rejected);
                }
                self.set_resize_mode(logical, mode);
                Ok(self
                    .query("visible_widths")
                    .unwrap_or(IntrospectValue::Null))
            }
            "set_all_resize_modes" => {
                let IntrospectValue::Text(text) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let mode: SectionResizeMode =
                    text.trim().parse().map_err(|()| InvokeError::Rejected)?;
                self.set_all_resize_modes(mode);
                Ok(self
                    .query("visible_widths")
                    .unwrap_or(IntrospectValue::Null))
            }
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
    Owner::current()
        .expect("use_column_layout requires an active Owner scope")
        .cache(key, || ColumnLayout::new(sizes()))
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
    use super::{
        ColumnLayout, ColumnLayoutState, DEFAULT_CONTENTS_PRECISION, SectionPlacement,
        SectionResizeMode, read_column_layout,
    };
    use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use crate::widgets::column_widths::DEFAULT_MIN_COL_WIDTH;

    /// Four sections wide enough to tell apart by width alone.
    fn layout() -> ColumnLayout {
        ColumnLayout::new(vec![100, 120, 140, 160])
    }

    fn text(s: &str) -> IntrospectValue {
        IntrospectValue::Text(s.to_string())
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
        // Qt's rule: hiding does not remove a section from the permutation,
        // so showing it again puts it back rather than appending it.
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
        // Qt has both because they are different operations; a test that only
        // checked "the order changed" would not tell them apart.
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
        assert!(l.restore_state(&saved));

        assert_eq!(l.save_state(), saved);
        assert_eq!(l.order(), vec![2, 0, 1, 3]);
        assert_eq!(l.section_size(2), 250);
        assert!(l.is_section_hidden(3));
        assert!(!l.is_section_hidden(0));
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
        };
        assert!(!l.restore_state(&bad_order));
        assert_eq!(l.save_state(), before, "no size or flag was written");

        // Wrong vector length — rejected before the order is even considered.
        let short = ColumnLayoutState {
            order: vec![3, 2, 1, 0],
            sizes: vec![10, 20, 30],
            hidden: vec![true; 4],
            modes: vec![SectionResizeMode::Interactive; 4],
        };
        assert!(!l.restore_state(&short));
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
        assert!(other.restore_state(&saved));
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
        assert!(matches!(
            l.invoke("resize_section", &text("140")),
            Err(InvokeError::Rejected)
        ));
        // A pair naming a section that does not exist.
        assert!(matches!(
            l.invoke("swap_sections", &text("0:9")),
            Err(InvokeError::Rejected)
        ));
        assert!(matches!(
            l.invoke("set_section_hidden", &text("9:true")),
            Err(InvokeError::Rejected)
        ));
        // A pair whose second half is the wrong type.
        assert!(matches!(
            l.invoke("set_section_hidden", &text("0:yes")),
            Err(InvokeError::Rejected)
        ));
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
        assert!(matches!(
            l.intervene("sizes", &IntrospectValue::Json(serde_json::json!([10, 20]))),
            Err(InterveneError::OutOfRange)
        ));
        assert!(matches!(
            l.intervene("hidden", &IntrospectValue::Json(serde_json::json!([true]))),
            Err(InterveneError::OutOfRange)
        ));
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
            Err(InterveneError::OutOfRange)
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
        fn schema(&self) -> crate::external::IntrospectSchema {
            crate::external::IntrospectSchema::new(const { &[] })
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
        // Qt: resizeSection has no effect outside Interactive / Fixed. The
        // value is not discarded though, so switching back reveals it.
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
        assert!(l.restore_state(&decoded));
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

        assert!(matches!(
            l.invoke("set_resize_mode", &text("0:sideways")),
            Err(InvokeError::Rejected)
        ));
        assert!(matches!(
            l.invoke("set_all_resize_modes", &text("sideways")),
            Err(InvokeError::Rejected)
        ));
        assert!(matches!(
            l.invoke("set_resize_mode", &text("9:fixed")),
            Err(InvokeError::Rejected)
        ));
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
            Err(InterveneError::OutOfRange)
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
        // R1454 — the bound the measurement demands. Qt's default, and a `0`
        // clamped to `1`: measuring nothing would leave a content-fitted
        // column with no content to fit, and a silent floor-sized column is
        // the kind of answer a caller cannot tell from a bug.
        let l = layout();
        assert_eq!(
            l.resize_contents_precision(),
            DEFAULT_CONTENTS_PRECISION,
            "Qt's default"
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
        // It does not touch the saved layout — it decides what a consumer
        // MEASURES, not what the header IS.
        assert_eq!(l.save_state(), layout().save_state());
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
}
