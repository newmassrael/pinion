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
use crate::reactive::Signal;
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
        Some(Self {
            order,
            sizes,
            hidden,
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
        Self {
            count,
            sections: ReorderModel::new(count, ReorderAxis::Horizontal),
            sizes,
            hidden: Signal::new(vec![false; count]),
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

    /// Size of logical section `logical` — Qt's `sectionSize()`. `0` for an
    /// unknown section (Qt's answer too), which is *not* the same as a hidden
    /// one: a hidden section keeps its size and gets it back when shown.
    #[must_use]
    pub fn section_size(&self, logical: usize) -> u32 {
        if logical >= self.count {
            return 0;
        }
        self.sizes.width(logical)
    }

    /// Resize logical section `logical` — Qt's `resizeSection()`. Returns the
    /// applied size after the width model's minimum-width clamp (`0` when the
    /// section does not exist), so an AI client learns the outcome in the same
    /// round-trip it asked for the change.
    pub fn resize_section(&self, logical: usize, size: u32) -> u32 {
        if logical >= self.count {
            return 0;
        }
        self.sizes.set_width(logical, size)
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
    /// Hiding is applied here and nowhere else, and the cumulative `x` is
    /// summed here and nowhere else — every other derived answer below reads
    /// this walk instead of repeating it, so a consumer painting a body cell
    /// under its header cannot compute a different offset than the header did.
    #[must_use]
    pub fn visible_placements(&self) -> Vec<SectionPlacement> {
        let hidden = self.hidden.get();
        let mut x = 0;
        let mut out = Vec::with_capacity(self.count);
        for (visual, logical) in self.sections.order().into_iter().enumerate() {
            if hidden.get(logical).copied().unwrap_or(false) {
                continue;
            }
            let size = self.sizes.width(logical);
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
            sizes: (0..self.count).map(|l| self.sizes.width(l)).collect(),
            hidden: self.hidden.get(),
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
        if state.sizes.len() != self.count || state.hidden.len() != self.count {
            return false;
        }
        if !self.sections.set_order(&state.order) {
            return false;
        }
        self.sizes.set_widths(state.sizes.clone());
        self.hidden.set(state.hidden.clone());
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
            _ => self.sections.invoke(method, args),
        }
    }
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
    use super::{ColumnLayout, ColumnLayoutState, SectionPlacement, read_column_layout};
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
        };
        assert!(!l.restore_state(&bad_order));
        assert_eq!(l.save_state(), before, "no size or flag was written");

        // Wrong vector length — rejected before the order is even considered.
        let short = ColumnLayoutState {
            order: vec![3, 2, 1, 0],
            sizes: vec![10, 20, 30],
            hidden: vec![true; 4],
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
