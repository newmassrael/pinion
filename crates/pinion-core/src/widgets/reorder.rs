//! R743 §5.51 — reusable **drag-to-reorder model**, lifted from the
//! `hello-dnd` first consumer (R742) when the reorderable tab strip
//! (`hello-tab-reorder`) became the second.
//!
//! ## What this owns
//!
//! The mechanical reorder coordinator state every reorderable collection
//! shares, independent of *what* is being reordered and *which way* the
//! items flow:
//!
//! - `order[visual] = item id` — the visual permutation, mutated only on
//!   a committed drop or keyboard move.
//! - `pressed` — the visual index whose `PointerDown` last landed, read
//!   by [`ReorderModel::begin_drag_payload`] to arm a drag.
//! - `preview` — the in-flight [`DragPreview`] (dragged visual + target
//!   gap) the consumer's view reads to dim the source and draw an
//!   insertion line.
//! - `focused` — the keyboard cursor / WAI-ARIA active descendant.
//! - `grab_snapshot` — the pre-grab order an `Escape` reverts to (APG
//!   keyboard drag).
//!
//! ## What this does *not* own
//!
//! Selection, paint, the WAI-ARIA role tree, and the keyboard *policy*
//! (which key does what) all stay in the consuming binding — they diverge
//! between a plain list (`hello-dnd`: arrows move the cursor) and a tab
//! strip (`hello-tab-reorder`: arrows *select*, Space grabs). Orientation
//! is the one axis the model parameterises ([`ReorderAxis`]): the drop
//! classification reads `x_rel` for a horizontal strip, `y_rel` for a
//! vertical list. Everything else is identical, which is exactly why the
//! lift is a clean composition rather than a flagged abstraction
//! ([[abstraction-needs-second-consumer]], [[coreshell-composition-lift]]).
//!
//! ## Composition, not inheritance
//!
//! A consumer embeds a `ReorderModel` as a field and delegates the three
//! R742 [`External`](crate::external::External) hooks
//! ([`begin_drag_payload`](ReorderModel::begin_drag_payload) /
//! [`drag_to`](ReorderModel::drag_to) /
//! [`drag_release`](ReorderModel::drag_release)) plus the
//! introspection slots ([`query`](ReorderModel::query) /
//! [`intervene`](ReorderModel::intervene) /
//! [`invoke`](ReorderModel::invoke)) to it, layering its own selection /
//! a11y / paint on top. All methods take `&self` (interior mutability) so
//! the embedding `External`'s `&self` accessors can reach them.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use crate::composite_tag::{require_pair, require_parsed_send_payload, split_subindex};
use crate::external::{
    DragPayload, DropPoint, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
    SchemaField,
};
use crate::input::PointerWireEvent;

/// The flow direction of a reorderable collection — selects which axis of
/// the [`DropPoint`] the drop classification reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReorderAxis {
    /// Items flow left-to-right (a tab strip); the drop gap is decided by
    /// `DropPoint::x_rel` (`<0.5` = insert before, `>=0.5` = after).
    Horizontal,
    /// Items flow top-to-bottom (a list); the drop gap is decided by
    /// `DropPoint::y_rel`.
    Vertical,
}

/// A live drag preview: which visual position is being dragged and the
/// gap index (`0..=count`) the cursor currently targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragPreview {
    /// Visual index of the item being dragged (dimmed in the view).
    pub from_visual: usize,
    /// Gap index the drop would insert at (`0` = before the first item,
    /// `count` = after the last), drawn as the insertion line.
    pub insert_at: usize,
}

/// R743 §5.51 — the reusable reorder coordinator model. See the module
/// docs for the ownership split. Construct one per reorderable
/// collection; the embedding `External` delegates its drag hooks and
/// introspection slots here.
#[derive(Debug)]
pub struct ReorderModel {
    /// Fixed item count. The order is always a permutation of `0..count`.
    count: usize,
    /// Flow direction — selects the drop-classification axis.
    axis: ReorderAxis,
    /// `order[visual] = item id`. Mutated on a committed drop / move.
    order: RefCell<Vec<usize>>,
    /// Visual index whose `PointerDown` last landed (arms `begin_drag`).
    pressed: Cell<Option<usize>>,
    /// Live drag preview, set by `drag_to`, cleared by `drag_release`.
    preview: RefCell<Option<DragPreview>>,
    /// Keyboard cursor / AT active descendant (visual index).
    focused: Cell<Option<usize>>,
    /// `Some(order)` while a keyboard grab is in flight — the snapshot an
    /// `Escape` (`grab_cancel`) reverts to.
    grab_snapshot: RefCell<Option<Vec<usize>>>,
}

/// R1565.1 §5.15 — an `order` write must carry exactly one entry per
/// position, and the refusal states both counts.
///
/// [`ReorderModel::set_order`] answers `false` for this AND for a
/// non-permutation, so a caller composing one sentence out of that bool
/// fuses two facts a client acts on differently. Its two consumers
/// (`intervene("order")` here and `ColumnLayout::restore_state`) each check
/// the length first, and this is the one place that words it.
fn order_len(given: usize, want: usize) -> InterveneError {
    InterveneError::out_of_range(format!(
        "order: this model has {want} positions, so an order needs {want} \
         entries, not {given}"
    ))
}

impl ReorderModel {
    /// Build a model for `count` items in identity order (`[0, 1, …]`),
    /// flowing along `axis`.
    #[must_use]
    pub fn new(count: usize, axis: ReorderAxis) -> Self {
        Self {
            count,
            axis,
            order: RefCell::new((0..count).collect()),
            pressed: Cell::new(None),
            preview: RefCell::new(None),
            focused: Cell::new(None),
            grab_snapshot: RefCell::new(None),
        }
    }

    /// A snapshot of the current visual order (`order[visual] = item id`).
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        self.order.borrow().clone()
    }

    /// R1450 §5.51 — move the item at visual index `from` **to** visual index
    /// `to`, both clamped. The toolkit's `moveSection`: `to` is the
    /// destination *index*, not an insertion gap, so a caller says where the
    /// item should end up and never has to reason about the shift the removal
    /// introduces.
    ///
    /// The keyboard funnel ([`invoke`](Self::invoke)'s `"move"`) and the
    /// explicit move now share this one conversion; before R1450 the
    /// index → gap step existed only inside the keyboard path, so an explicit
    /// move had nowhere to land.
    pub fn move_section(&self, from: usize, to: usize) {
        let last = self.count.saturating_sub(1);
        let (from, to) = (from.min(last), to.min(last));
        if from == to {
            return;
        }
        let gap = if to > from { to + 1 } else { to };
        Self::apply_move(&mut self.order.borrow_mut(), from, gap);
    }

    /// R1450 §5.51 — replace the whole visual order. `false` (and no change)
    /// when `next` is not a permutation of `0..count` — the invariant this
    /// type's [`order`](Self::order) field documents, checked in the one place
    /// that owns it rather than at each caller.
    ///
    /// The restore half of the layout round-trip (the toolkit's
    /// `restoreState`, minus the sizes and hidden flags that live
    /// on other axes). A readable order that could not be written back would
    /// break the read/write symmetry every other pinion wire slot keeps.
    pub fn set_order(&self, next: &[usize]) -> bool {
        if next.len() != self.count {
            return false;
        }
        let mut seen = vec![false; self.count];
        for &id in next {
            match seen.get_mut(id) {
                // A duplicate or an out-of-range id would leave the model
                // holding something that is not a permutation.
                Some(slot) if !*slot => *slot = true,
                _ => return false,
            }
        }
        *self.order.borrow_mut() = next.to_vec();
        true
    }

    /// The keyboard cursor / AT active descendant (visual index), if any.
    #[must_use]
    pub fn focused(&self) -> Option<usize> {
        self.focused.get()
    }

    /// Set the keyboard cursor to `visual` (clamped to a valid index).
    pub fn set_focused(&self, visual: usize) {
        self.focused
            .set(Some(visual.min(self.count.saturating_sub(1))));
    }

    /// Whether a keyboard grab (APG pick-up) is currently in flight.
    #[must_use]
    pub fn grabbed(&self) -> bool {
        self.grab_snapshot.borrow().is_some()
    }

    /// Arm a drag from the most-recently-pressed item. The payload carries
    /// the dragged item's **stable id** (not its visual index) under
    /// `kind`, so the in-flight drag is introspectable and a target can
    /// match on it; the visual index the reorder needs is recovered from
    /// `pressed` on commit. `None` when no item is pressed.
    #[must_use]
    pub fn begin_drag_payload(&self, kind: Cow<'static, str>) -> Option<DragPayload> {
        let visual = self.pressed.get()?;
        let item = self.order.borrow().get(visual).copied()?;
        Some(DragPayload {
            kind,
            value: IntrospectValue::Int(i64::try_from(item).unwrap_or(0)),
        })
    }

    /// Live update: classify the cursor's drop gap (axis-aware) and store
    /// it as the preview the view reads. When the cursor sits over no item
    /// (a gap / the background), **hold** the last resolved gap rather
    /// than snapping back to the dragged item — otherwise the indicator
    /// jumps to the dragged item's own gap each time the cursor crosses an
    /// inter-item gap (the R742.2 "up then down" flicker fix). Falls back
    /// to the dragged item only on the first frame.
    pub fn drag_to(&self, _payload: &DragPayload, over: Option<&DropPoint>) {
        let Some(from_visual) = self.pressed.get() else {
            return;
        };
        let last = self.preview.borrow().map(|p| p.insert_at);
        let insert_at = self.drop_slot(over).or(last).unwrap_or(from_visual);
        *self.preview.borrow_mut() = Some(DragPreview {
            from_visual,
            insert_at,
        });
    }

    /// Commit: move the source item to the final gap (honouring the held
    /// preview when the release is over no item), then clear the transient
    /// drag state. A drop over the source's own gap is a no-op, so a
    /// press-release-in-place never reorders.
    pub fn drag_release(&self, _payload: &DragPayload, over: Option<&DropPoint>) {
        if let Some(from_visual) = self.pressed.get() {
            let last = self.preview.borrow().map(|p| p.insert_at);
            let insert_at = self.drop_slot(over).or(last).unwrap_or(from_visual);
            Self::apply_move(&mut self.order.borrow_mut(), from_visual, insert_at);
        }
        *self.preview.borrow_mut() = None;
        self.pressed.set(None);
    }

    /// R1501 — every path this model answers, declared where it is answered.
    ///
    /// A consumer that layers its own slots over these composes with
    /// [`SchemaField::concat`] instead of restating them, so a slot added here
    /// reaches every embedding surface without any of them being edited. Before
    /// R1501 the nearest thing to this list was a sentence in
    /// [`query`](Self::query)'s doc, and each embedder hand-copied the names
    /// out of it.
    ///
    /// Reads and actions sit in one list because [`SchemaField`] does not
    /// distinguish them yet (its own doc says so): `send` / `move` / `grab` /
    /// `grab_cancel` / `move_section` are `invoke` channels that read as
    /// nothing, and `order` / `focused_index` additionally take an
    /// `intervene`.
    pub const SCHEMA_FIELDS: &'static [SchemaField] = &[
        SchemaField::new("order", "json"),
        SchemaField::new("preview", "json"),
        SchemaField::new("focused_index", "int"),
        SchemaField::new("grabbed", "boolean"),
        SchemaField::action("send", "string"),
        SchemaField::action("move", "int"),
        SchemaField::action("move_section", "string"),
        SchemaField::action("grab", "boolean"),
        SchemaField::action("grab_cancel", "string"),
    ];

    /// Reorder slots for [`ExternalIntrospect::query`]:
    /// `order` / `preview` / `focused_index` / `grabbed`. Returns `None`
    /// for any other path so an embedding consumer's own slots take
    /// precedence.
    #[must_use]
    pub fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "order" => {
                let arr: Vec<serde_json::Value> = self
                    .order
                    .borrow()
                    .iter()
                    .map(|&id| serde_json::Value::from(id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "preview" => Some(match *self.preview.borrow() {
                Some(p) => IntrospectValue::Json(serde_json::json!({
                    "from_visual": p.from_visual,
                    "insert_at": p.insert_at,
                })),
                None => IntrospectValue::Null,
            }),
            "focused_index" => Some(match self.focused.get() {
                Some(i) => IntrospectValue::Int(i64::try_from(i).unwrap_or(0)),
                None => IntrospectValue::Null,
            }),
            "grabbed" => Some(IntrospectValue::Bool(self.grabbed())),
            _ => None,
        }
    }

    /// Reorder slots for [`ExternalIntrospect::intervene`]:
    /// `focused_index` is the writable keyboard cursor; `order` (R1450) takes
    /// a whole permutation back in the same JSON-array shape
    /// [`query`](Self::query) hands out. Any other path is unknown here.
    ///
    /// # Errors
    ///
    /// [`InterveneError::TypeMismatch`] when `focused_index` is not an
    /// integer or `order` is not a JSON array of integers,
    /// [`InterveneError::OutOfRange`] when `focused_index` is `>= count` or
    /// `order` is not a permutation of `0..count`, and
    /// [`InterveneError::UnknownPath`] otherwise.
    pub fn intervene(&self, path: &str, value: &IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "focused_index" => {
                let i = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                if i >= self.count {
                    return Err(InterveneError::out_of_range(format!(
                        "no position {i} in this model (it has {}, so 0..{})",
                        self.count, self.count
                    )));
                }
                self.focused.set(Some(i));
                Ok(())
            }
            // R1450 — the restore half of the layout round-trip. A malformed
            // array is a TypeMismatch (wrong shape); a well-formed one that is
            // not a permutation is OutOfRange (right shape, impossible value),
            // so a client learns which of the two it got wrong.
            "order" => {
                let IntrospectValue::Json(serde_json::Value::Array(items)) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let next: Option<Vec<usize>> = items
                    .iter()
                    .map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
                    .collect();
                let next = next.ok_or(InterveneError::TypeMismatch)?;
                // R1565.1 — the LENGTH is checked here rather than left to
                // `set_order`, which answers `false` for it AND for a
                // non-permutation. Saying "not a permutation" about a
                // three-element array handed to a five-section model is true
                // and useless: the caller needs the count, and a census of
                // this round's own work found this fusion surviving inside it.
                if next.len() != self.count {
                    return Err(order_len(next.len(), self.count));
                }
                if self.set_order(&next) {
                    Ok(())
                } else {
                    Err(InterveneError::out_of_range(format!(
                        "{next:?} is not a permutation of 0..{}: an id repeats \
                         or is out of range",
                        self.count
                    )))
                }
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// Reorder actions for [`ExternalIntrospect::invoke`]:
    ///
    /// - `send` — composite `"{visual}:{EventName}"` wire form (parsed via
    ///   the shared [`require_parsed_send_payload`] SSOT). A `PointerDown` records
    ///   the pressed visual so `begin_drag` can arm it.
    /// - `move` — move the focused item by the integer delta (clamped,
    ///   cursor following); returns the new focused index or `Null`.
    /// - `grab` — toggle the keyboard pick-up of the focused item
    ///   (`Bool(b)` sets explicitly, anything else toggles); returns the
    ///   new grabbed state.
    /// - `grab_cancel` — revert to the snapshotted order and drop.
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when an argument variant is wrong,
    /// [`InvokeError::Rejected`] when a `send` payload does not parse, and
    /// [`InvokeError::UnknownPath`] for any other method.
    pub fn invoke(
        &self,
        method: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match method {
            "send" => {
                let IntrospectValue::Text(payload) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                // R781 — modifiers ignored (reorder press has no modifier axis).
                let (visual, crate::composite_tag::SendPayload { event, .. }): (usize, _) =
                    require_parsed_send_payload("reorder.send", payload)?;
                if event == PointerWireEvent::Down.as_wire_name() && visual < self.count {
                    self.pressed.set(Some(visual));
                }
                Ok(IntrospectValue::Null)
            }
            "move" => {
                let delta = args.as_i64().ok_or(InvokeError::TypeMismatch)?;
                match self.move_by(delta) {
                    Some(i) => Ok(IntrospectValue::Int(i64::try_from(i).unwrap_or(0))),
                    None => Ok(IntrospectValue::Null),
                }
            }
            "grab" => {
                if self.focused.get().is_none() {
                    return Ok(IntrospectValue::Bool(false));
                }
                let want = match args {
                    IntrospectValue::Bool(b) => Some(*b),
                    _ => None,
                };
                Ok(IntrospectValue::Bool(self.grab_toggle(want)))
            }
            "grab_cancel" => {
                if let Some(snap) = self.grab_snapshot.borrow_mut().take() {
                    *self.order.borrow_mut() = snap;
                }
                Ok(IntrospectValue::Null)
            }
            // R1450 — the toolkit `moveSection(from, to)`. The wire form is
            // the composite `"{from}:{to}"` the rest of this model already
            // speaks for a pair, and the return is the resulting order so one
            // round-trip both moves and reports.
            "move_section" => {
                let IntrospectValue::Text(payload) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                // R1451 — the typed-pair argument codec, shared with
                // `ColumnLayout`'s three pair invokes (the inline
                // `split_once` this replaced was its first site).
                let (from, to) = require_pair::<usize, usize>("reorder.move", payload, ':')?;
                if from >= self.count || to >= self.count {
                    return Err(InvokeError::rejected(format!(
                        "reorder.move: {from} -> {to} is outside this model \
                         (it has {} positions)",
                        self.count
                    )));
                }
                self.move_section(from, to);
                Ok(self.query("order").unwrap_or(IntrospectValue::Null))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    // ----- internals -------------------------------------------------

    /// Classify a [`DropPoint`] into the gap index (`0..=count`) the drop
    /// targets. The leading half of item `j` inserts before it (gap `j`);
    /// the trailing half inserts after (gap `j + 1`) — the axis selects
    /// which fraction is read. `None` when the cursor is over no item.
    fn drop_slot(&self, over: Option<&DropPoint>) -> Option<usize> {
        let p = over?;
        let j = split_subindex(&p.tag)
            .1
            .and_then(|s| s.parse::<usize>().ok())?;
        if j >= self.count {
            return Some(self.count);
        }
        let frac = match self.axis {
            ReorderAxis::Horizontal => p.x_rel,
            ReorderAxis::Vertical => p.y_rel,
        };
        Some(if frac < 0.5 { j } else { j + 1 })
    }

    /// Move the item at visual index `from` to gap `insert_at`, accounting
    /// for the shift the removal introduces. A move onto the source's own
    /// gap leaves the order unchanged.
    fn apply_move(order: &mut Vec<usize>, from: usize, insert_at: usize) {
        if from >= order.len() {
            return;
        }
        let item = order.remove(from);
        let dest = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        };
        let dest = dest.min(order.len());
        order.insert(dest, item);
    }

    /// Move the focused item to `target` (clamped), cursor following. The
    /// keyboard / AT reorder funnel. `None` when no item is focused.
    fn move_focused_to(&self, target: usize) -> Option<usize> {
        let from = self.focused.get()?;
        let target = target.min(self.count.saturating_sub(1));
        self.move_section(from, target);
        self.focused.set(Some(target));
        Some(target)
    }

    /// Move the focused item by `delta` slots (clamped to the ends).
    fn move_by(&self, delta: i64) -> Option<usize> {
        let from = self.focused.get()?;
        let max = i64::try_from(self.count.saturating_sub(1)).unwrap_or(0);
        let target = (i64::try_from(from).unwrap_or(0) + delta).clamp(0, max);
        self.move_focused_to(usize::try_from(target).unwrap_or(0))
    }

    /// Toggle the grab on the focused item. `want` sets it explicitly;
    /// `None` toggles. Entering a grab snapshots the order; leaving keeps
    /// the live order. Returns the new grabbed state.
    fn grab_toggle(&self, want: Option<bool>) -> bool {
        let want = want.unwrap_or(!self.grabbed());
        if want && !self.grabbed() {
            let snap = self.order.borrow().clone();
            *self.grab_snapshot.borrow_mut() = Some(snap);
        } else if !want && self.grabbed() {
            *self.grab_snapshot.borrow_mut() = None;
        }
        self.grabbed()
    }
}

/// A decoded snapshot of a [`ReorderModel`]'s introspection slots — the
/// **deserialize peer** of [`ReorderModel::query`]. A consumer's
/// `read_state` decodes the reorder wire shape through this instead of
/// hand-matching the JSON in every binding, so the encode (the model's
/// `query`) and the decode live in one module: a slot rename can't
/// silently break a binding's hand-decode. Bindings map this into their
/// own `Copy` projection (and add widget-specific slots like a tab's
/// selection). `order` is a `Vec` (count-agnostic, like the model); a
/// fixed-`N` binding converts with `try_into().unwrap_or(IDENTITY)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReorderView {
    /// Current visual order (`order[visual] = item id`); empty on a
    /// shape mismatch.
    pub order: Vec<usize>,
    /// In-flight drag preview, if any.
    pub preview: Option<DragPreview>,
    /// Keyboard cursor / AT active descendant (visual index).
    pub focused: Option<usize>,
    /// Whether a keyboard grab is in flight.
    pub grabbed: bool,
}

/// Decode the reorder slots (`order` / `preview` / `focused_index` /
/// `grabbed`) from an introspection surface that delegates them to a
/// [`ReorderModel`] (the binding's `External` wrapper). The inverse of
/// [`ReorderModel::query`]; keep the two in lockstep.
#[must_use]
pub fn read_reorder(intro: &dyn ExternalIntrospect) -> ReorderView {
    let order = match intro.query("order") {
        Some(IntrospectValue::Json(serde_json::Value::Array(a))) => a
            .iter()
            .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
            .collect(),
        _ => Vec::new(),
    };
    let preview = match intro.query("preview") {
        Some(IntrospectValue::Json(v)) => {
            let from = v.get("from_visual").and_then(serde_json::Value::as_u64);
            let at = v.get("insert_at").and_then(serde_json::Value::as_u64);
            match (from, at) {
                (Some(f), Some(a)) => Some(DragPreview {
                    from_visual: usize::try_from(f).unwrap_or(0),
                    insert_at: usize::try_from(a).unwrap_or(0),
                }),
                _ => None,
            }
        }
        _ => None,
    };
    let focused = match intro.query("focused_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    let grabbed = matches!(intro.query("grabbed"), Some(IntrospectValue::Bool(true)));
    ReorderView {
        order,
        preview,
        focused,
        grabbed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

    fn drop_h(visual: usize, x_rel: f32) -> DropPoint {
        DropPoint {
            tag: format!("w#{visual}"),
            x_rel,
            y_rel: 0.5,
        }
    }

    fn drop_v(visual: usize, y_rel: f32) -> DropPoint {
        DropPoint {
            tag: format!("w#{visual}"),
            x_rel: 0.5,
            y_rel,
        }
    }

    fn pl() -> DragPayload {
        DragPayload {
            kind: Cow::Borrowed("w"),
            value: IntrospectValue::Int(0),
        }
    }

    fn press(m: &ReorderModel, visual: usize) {
        m.invoke(
            "send",
            &IntrospectValue::Text(format!("{visual}:PointerDown")),
        )
        .expect("send accepted");
    }

    #[test]
    fn horizontal_drop_classifies_left_and_right_halves() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        // y_rel is ignored on the horizontal axis.
        assert_eq!(m.drop_slot(Some(&drop_h(1, 0.2))), Some(1)); // left → before 1
        assert_eq!(m.drop_slot(Some(&drop_h(1, 0.8))), Some(2)); // right → after 1
        assert_eq!(m.drop_slot(None), None);
    }

    #[test]
    fn vertical_drop_classifies_top_and_bottom_halves() {
        let m = ReorderModel::new(4, ReorderAxis::Vertical);
        assert_eq!(m.drop_slot(Some(&drop_v(2, 0.2))), Some(2)); // top → before 2
        assert_eq!(m.drop_slot(Some(&drop_v(2, 0.8))), Some(3)); // bottom → after 2
    }

    #[test]
    fn apply_move_relocates_with_removal_shift() {
        let mut o = vec![0, 1, 2, 3];
        ReorderModel::apply_move(&mut o, 0, 2);
        assert_eq!(o, [1, 0, 2, 3]);
        let mut o = vec![0, 1, 2, 3];
        ReorderModel::apply_move(&mut o, 3, 1);
        assert_eq!(o, [0, 3, 1, 2]);
    }

    #[test]
    fn apply_move_onto_own_gap_is_noop() {
        let mut o = vec![0, 1, 2, 3];
        ReorderModel::apply_move(&mut o, 2, 2);
        assert_eq!(o, [0, 1, 2, 3]);
        let mut o = vec![0, 1, 2, 3];
        ReorderModel::apply_move(&mut o, 2, 3);
        assert_eq!(o, [0, 1, 2, 3]);
    }

    #[test]
    fn begin_drag_arms_only_after_press() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        assert!(m.begin_drag_payload(Cow::Borrowed("w")).is_none());
        press(&m, 2);
        let p = m.begin_drag_payload(Cow::Borrowed("w")).expect("armed");
        assert_eq!(p.value.as_usize(), Some(2));
    }

    #[test]
    fn drag_to_then_release_reorders_and_clears() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        press(&m, 0);
        m.drag_to(&pl(), Some(&drop_h(2, 0.8))); // → gap 3
        assert_eq!(m.preview.borrow().unwrap().insert_at, 3);
        m.drag_release(&pl(), Some(&drop_h(2, 0.8)));
        assert_eq!(m.order(), [1, 2, 0, 3]);
        assert!(m.preview.borrow().is_none());
    }

    #[test]
    fn drag_to_over_gap_holds_last_no_snap_back() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        press(&m, 1);
        m.drag_to(&pl(), Some(&drop_h(2, 0.8)));
        assert_eq!(m.preview.borrow().unwrap().insert_at, 3);
        // Over no item (bare container tag): hold gap 3, no snap to 1.
        let gap = DropPoint {
            tag: String::from("w"),
            x_rel: 0.5,
            y_rel: 0.5,
        };
        m.drag_to(&pl(), Some(&gap));
        assert_eq!(m.preview.borrow().unwrap().insert_at, 3);
        m.drag_to(&pl(), None);
        assert_eq!(m.preview.borrow().unwrap().insert_at, 3);
    }

    #[test]
    fn move_action_reorders_clamps_follows_cursor() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        m.intervene("focused_index", &IntrospectValue::Int(0))
            .expect("focus");
        assert_eq!(
            m.invoke("move", &IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Int(1)
        );
        assert_eq!(m.order(), [1, 0, 2, 3]);
        m.intervene("focused_index", &IntrospectValue::Int(1))
            .expect("focus");
        m.invoke("move", &IntrospectValue::Int(10)).expect("move");
        assert_eq!(m.focused(), Some(3));
        m.invoke("move", &IntrospectValue::Int(-10)).expect("move");
        assert_eq!(m.focused(), Some(0));
    }

    #[test]
    fn grab_snapshots_and_cancel_reverts() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        m.intervene("focused_index", &IntrospectValue::Int(0))
            .expect("focus");
        assert_eq!(
            m.invoke("grab", &IntrospectValue::Bool(true)).unwrap(),
            IntrospectValue::Bool(true)
        );
        assert!(m.grabbed());
        m.invoke("move", &IntrospectValue::Int(1)).expect("move");
        assert_ne!(m.order(), [0, 1, 2, 3]);
        m.invoke("grab_cancel", &IntrospectValue::Null)
            .expect("cancel");
        assert!(!m.grabbed());
        assert_eq!(m.order(), [0, 1, 2, 3]);
        // Grab without a focused item is a no-op.
        let m2 = ReorderModel::new(3, ReorderAxis::Vertical);
        assert_eq!(
            m2.invoke("grab", &IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(false)
        );
    }

    #[test]
    fn intervene_focused_clamps_and_rejects() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        m.intervene("focused_index", &IntrospectValue::Int(2))
            .expect("in range");
        assert_eq!(m.focused(), Some(2));
        assert_out_of_range_saying(
            &m.intervene("focused_index", &IntrospectValue::Int(9)),
            "no position 9 in this model",
        );
        // R1450 — `order` became writable (the toolkit restoreState), so a
        // wrong-shaped value is now a TypeMismatch rather than "you may not
        // write this".
        assert!(matches!(
            m.intervene("order", &IntrospectValue::Int(0)),
            Err(InterveneError::TypeMismatch)
        ));
        assert!(matches!(
            m.intervene("nope", &IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath)
        ));
    }

    #[test]
    fn query_unknown_path_is_none() {
        let m = ReorderModel::new(2, ReorderAxis::Horizontal);
        assert!(m.query("selected_id").is_none());
        assert!(matches!(
            m.query("grabbed"),
            Some(IntrospectValue::Bool(false))
        ));
    }

    /// A minimal `ExternalIntrospect` that delegates `query` to a model —
    /// stands in for a binding's `External` wrapper so `read_reorder` is
    /// tested against the real `query` encode (the round-trip SSOT).
    struct Probe(ReorderModel);

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
    fn read_reorder_round_trips_query_encode() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        m.intervene("focused_index", &IntrospectValue::Int(1))
            .expect("focus");
        press(&m, 0);
        m.drag_to(&pl(), Some(&drop_h(2, 0.8))); // preview from 0 → gap 3
        let v = read_reorder(&Probe(m));
        assert_eq!(v.order, vec![0, 1, 2, 3]);
        assert_eq!(v.focused, Some(1));
        assert_eq!(
            v.preview,
            Some(DragPreview {
                from_visual: 0,
                insert_at: 3
            })
        );
        assert!(!v.grabbed);
    }

    // ----- R1450 explicit move + order restore (the toolkit header view)
    // -----

    #[test]
    fn r1450_move_section_lands_on_the_destination_index_not_a_gap() {
        let m = ReorderModel::new(5, ReorderAxis::Horizontal);
        // The toolkit's moveSection(0, 2): the item ends up AT index 2.
        m.move_section(0, 2);
        assert_eq!(m.order(), [1, 2, 0, 3, 4]);
        assert_eq!(m.order()[2], 0, "the moved item is at the destination");
        // And backwards, where no removal shift applies.
        m.move_section(4, 1);
        assert_eq!(m.order(), [1, 4, 2, 0, 3]);
        assert_eq!(m.order()[1], 4);
        // A move onto its own index is a no-op, and out of range clamps.
        let before = m.order();
        m.move_section(2, 2);
        assert_eq!(m.order(), before);
        m.move_section(9, 9);
        assert_eq!(m.order(), before, "both ends clamp to the last index");
    }

    #[test]
    fn r1450_the_keyboard_move_and_the_explicit_move_agree() {
        // The keyboard funnel is defined in terms of move_section now, so the
        // two paths cannot drift apart on the index-to-gap conversion.
        let keyboard = ReorderModel::new(5, ReorderAxis::Horizontal);
        keyboard.set_focused(0);
        keyboard
            .invoke("move", &IntrospectValue::Int(2))
            .expect("move is a known action");
        let explicit = ReorderModel::new(5, ReorderAxis::Horizontal);
        explicit.move_section(0, 2);
        assert_eq!(keyboard.order(), explicit.order());
    }

    #[test]
    fn r1450_set_order_takes_a_permutation_and_refuses_anything_else() {
        let m = ReorderModel::new(4, ReorderAxis::Vertical);
        assert!(m.set_order(&[3, 1, 0, 2]));
        assert_eq!(m.order(), [3, 1, 0, 2]);
        let good = m.order();
        assert!(!m.set_order(&[0, 1, 2]), "wrong length");
        assert!(!m.set_order(&[0, 0, 1, 2]), "duplicate id");
        assert!(!m.set_order(&[0, 1, 2, 9]), "id outside 0..count");
        assert_eq!(m.order(), good, "a refused restore changes nothing");
    }

    #[test]
    fn r1450_order_round_trips_through_the_wire_form() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        m.move_section(0, 3);
        let read = m.query("order").expect("order is queryable");
        // Read it out, write it into a fresh model: the same order lands.
        let fresh = ReorderModel::new(4, ReorderAxis::Horizontal);
        fresh.intervene("order", &read).expect("order is writable");
        assert_eq!(fresh.order(), m.order());
        // A well-formed array that is not a permutation is OutOfRange, not
        // TypeMismatch: the shape was right, the value was impossible.
        assert_out_of_range_saying(
            &fresh.intervene(
                "order",
                &IntrospectValue::Json(serde_json::json!([0, 0, 1, 2])),
            ),
            "an id repeats or is out of range",
        );
        assert!(matches!(
            fresh.intervene("order", &IntrospectValue::Json(serde_json::json!(["a"]))),
            Err(InterveneError::TypeMismatch)
        ));
    }

    #[test]
    fn r1450_move_section_invoke_reports_the_resulting_order() {
        let m = ReorderModel::new(4, ReorderAxis::Horizontal);
        let out = m
            .invoke("move_section", &IntrospectValue::Text("0:2".into()))
            .expect("move_section is a known action");
        assert_eq!(out, m.query("order").expect("order is queryable"));
        assert_eq!(m.order(), [1, 2, 0, 3]);
        assert_refused_saying(
            &m.invoke("move_section", &IntrospectValue::Text("0:9".into())),
            "0 -> 9 is outside this model",
        );
        assert_refused_saying(
            &m.invoke("move_section", &IntrospectValue::Text("nope".into())),
            "malformed argument \"nope\"",
        );
        assert!(matches!(
            m.invoke("move_section", &IntrospectValue::Int(0)),
            Err(InvokeError::TypeMismatch)
        ));
    }
}
