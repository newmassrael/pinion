//! R735.1 §5.38 — shared **selection-coordinator substrate** for the
//! "select N-of-M leaf cells" widget family.
//!
//! Four coordinators compose a `Vec` of per-cell selection leaves and
//! enforce a selection policy over them:
//!
//! | coordinator | leaf | policy |
//! |---|---|---|
//! | [`RadioGroup`](crate::widgets::radio_group::RadioGroup) | [`Radio`](crate::widgets::radio::Radio) | single (1-of-N exclusion) |
//! | [`DatePicker`](crate::widgets::datepicker::DatePicker) | [`Radio`](crate::widgets::radio::Radio) | single (1-of-N exclusion) |
//! | [`ListBox`](crate::widgets::listbox::ListBox) | [`ListBoxItem`](crate::widgets::listbox_item::ListBoxItem) | single **or** multi |
//! | [`Table`](crate::widgets::table::Table) | [`Radio`](crate::widgets::radio::Radio) | single **or** multi |
//!
//! Before R735.1 each coordinator carried its own byte-identical copy of
//! the exclusion loop, the multi-select bitmap snapshot + `detect`, and
//! the slot-assignment setter. This module is the single source of truth
//! for the **leaf-agnostic, mechanical** parts of that algorithm; the
//! parts that are genuinely leaf-type-bound (the SCXML activation
//! predicate `(Pressed → Hover) | (KeyboardActivate & !Disabled)`, which
//! pattern-matches the leaf's own state / event enums, and the typed
//! selected-value the coordinator stores — `Option<usize>` /
//! `Option<CivilDate>` / a per-row index) stay inline at each call site.
//! Forcing those behind the trait would move ~4 lines into a trait impl
//! with no net reduction — the canonical "don't abstract what diverges"
//! boundary ([[abstraction-needs-second-consumer]]).
//!
//! ## What lives here
//!
//! * [`SelectableLeaf`] — the 2-method trait every leaf already satisfies.
//! * [`select_exclusive`] — the single-select sibling-deselect (4
//!   consumers): on a fresh `false → true` activation, clear every other
//!   leaf and report that the selection landed.
//! * [`toggle_off_if_reselected`] — the multi-select toggle completion (2
//!   consumers): the leaf's set-not-flip value is idempotent on an
//!   already-`true` cell, so a re-activation needs the value forced back
//!   to `false` here.
//! * [`SelectionSnapshot`] + [`capture`] + [`detect_intents`] — the
//!   multi-capable §5.20 transition contract (2 consumers): a per-leaf
//!   selection bitmap whose diff emits one `"selected"` intent per
//!   changed bit, gated in single-select mode to the `false → true`
//!   direction only.
//! * [`replace_selection`] — the slot-assignment setter core (2
//!   consumers): replace the whole selection set, validating indices and
//!   the single-select cardinality cap.

use crate::external::IntrospectValue;
use crate::intent::Intent;

/// R735.1 §5.38 — a per-cell selection leaf: the minimal surface the
/// shared coordinator algorithms need. [`Radio`](crate::widgets::radio::Radio)
/// and [`ListBoxItem`](crate::widgets::listbox_item::ListBoxItem) both
/// satisfy it with their existing inherent methods (the value sidecar is
/// set-not-flip on activation; deselection is the coordinator's job —
/// exactly what these helpers automate).
pub trait SelectableLeaf {
    /// `true` if this leaf is currently selected.
    fn is_selected(&self) -> bool;
    /// Set the selection value directly (the coordinator's deselect /
    /// restore channel; never flips on its own).
    fn set_selected(&mut self, selected: bool);
}

// Both impls delegate to the leaf's identically-named inherent methods.
// Method-call syntax (`self.is_selected()`) resolves to the *inherent*
// method — Rust prefers inherent over trait methods — so this delegates
// rather than recursing; function syntax (`Self::is_selected(self)`)
// would be ambiguous once the trait is in scope.
impl SelectableLeaf for super::radio::Radio {
    fn is_selected(&self) -> bool {
        self.is_selected()
    }
    fn set_selected(&mut self, selected: bool) {
        self.set_selected(selected);
    }
}

impl SelectableLeaf for super::listbox_item::ListBoxItem {
    fn is_selected(&self) -> bool {
        self.is_selected()
    }
    fn set_selected(&mut self, selected: bool) {
        self.set_selected(selected);
    }
}

/// R735.1 §5.38 — single-select sibling-deselect (the 1-of-N exclusion
/// shared by [`RadioGroup`](crate::widgets::radio_group::RadioGroup),
/// [`DatePicker`](crate::widgets::datepicker::DatePicker), single-select
/// [`ListBox`](crate::widgets::listbox::ListBox), and single-select
/// [`Table`](crate::widgets::table::Table)).
///
/// Call **after** driving `leaves[index].send(event)` with the leaf's
/// pre-send `was_selected`. If the leaf just transitioned `false → true`
/// (the activation edge), every *other* leaf is cleared and `true` is
/// returned so the caller can store its typed selected value + sync the
/// roving active descendant (WAI-ARIA "activation moves focus"). Returns
/// `false` (no mutation) when the leaf did not freshly gain selection
/// (idempotent re-activation, a non-activating event, or a disabled
/// leaf), so the caller leaves its selected-value untouched.
///
/// # Panics
/// Panics if `index >= leaves.len()`.
pub fn select_exclusive<L: SelectableLeaf>(
    leaves: &mut [L],
    index: usize,
    was_selected: bool,
) -> bool {
    let now_selected = leaves[index].is_selected();
    if !was_selected && now_selected {
        for (j, leaf) in leaves.iter_mut().enumerate() {
            if j != index {
                leaf.set_selected(false);
            }
        }
        true
    } else {
        false
    }
}

/// R735.1 §5.38 — multi-select toggle completion (shared by multi-select
/// [`ListBox`](crate::widgets::listbox::ListBox) and
/// [`Table`](crate::widgets::table::Table)).
///
/// In multi-select mode an activation *toggles* the addressed leaf. The
/// leaf's value sidecar is set-not-flip (the activation edge always sets
/// it `true`), so toggling **off** an already-selected leaf needs the
/// value forced back to `false` here. Call with the leaf's pre-send
/// `was_selected` and the caller-computed `activation` (the leaf-typed
/// `(Pressed → Hover) | (KeyboardActivate & !Disabled)` predicate). A
/// non-activation, or an activation on a previously-unselected leaf
/// (already `true` from the edge), is left as-is.
pub fn toggle_off_if_reselected<L: SelectableLeaf>(
    leaf: &mut L,
    was_selected: bool,
    activation: bool,
) {
    if activation && was_selected {
        leaf.set_selected(false);
    }
}

/// R735.1 §5.38 — selection-cardinality mode + per-leaf selection bitmap.
/// The [`WidgetTransition::Snapshot`](crate::widgets::WidgetTransition::Snapshot)
/// for every multi-capable coordinator (single-select coordinators that
/// store a typed `Option` value — `RadioGroup` / `DatePicker` — keep
/// their own `Option`-shaped snapshot instead). The `Vec` heap-allocates
/// once per dispatch (once per user input event); negligible at UI / AI
/// frequencies, and the reason `Snapshot: Clone` (not `Copy`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSnapshot {
    multiselect: bool,
    bits: Vec<bool>,
}

/// Capture the current selection bitmap + mode for a leaf vector.
#[must_use]
pub fn capture<L: SelectableLeaf>(leaves: &[L], multiselect: bool) -> SelectionSnapshot {
    SelectionSnapshot {
        multiselect,
        bits: leaves.iter().map(SelectableLeaf::is_selected).collect(),
    }
}

/// R735.1 §5.38 — the multi-capable §5.20 `detect` rule: emit one
/// `"selected"` intent (payload = the leaf index as
/// [`IntrospectValue::Int`]) for every leaf whose selection bit changed,
/// gated in **single-select** mode to the `false → true` direction only.
///
/// * Single-select — siblings deselect silently; only the newly-gained
///   leaf raises an intent (the pre-R735.1 RadioGroup-class emit shape).
/// * Multi-select — every flip emits (toggle-on and toggle-off each carry
///   information the AI client needs; a follow-up `selected.<i>` query
///   learns the new boolean).
///
/// `before` / `after` must share length + mode (debug-asserted; the inner
/// leaf vector cannot change length mid-dispatch by construction).
#[must_use]
pub fn detect_intents(before: &SelectionSnapshot, after: &SelectionSnapshot) -> Vec<Intent> {
    debug_assert_eq!(before.bits.len(), after.bits.len());
    debug_assert_eq!(before.multiselect, after.multiselect);
    let multi = after.multiselect;
    let mut out = Vec::new();
    for (i, (b, a)) in before.bits.iter().zip(after.bits.iter()).enumerate() {
        if b == a {
            continue;
        }
        if multi || (!*b && *a) {
            if let Ok(idx) = i64::try_from(i) {
                out.push(Intent::new_static("selected", IntrospectValue::Int(idx)));
            }
        }
    }
    out
}

/// R735.1 §5.38 — slot-assignment setter core (shared by
/// [`ListBox::set_selected_indices`](crate::widgets::listbox::ListBox::set_selected_indices)
/// and [`Table::set_selected_rows`](crate::widgets::table::Table::set_selected_rows)).
/// Replaces the entire selection set with `indices` (persisted-preference
/// restore / form default / programmatic clear); silent on the §5.20
/// channel — restoration is admin, not interaction.
///
/// Returns the canonical single-select index (`indices.first().copied()`)
/// so a single-select caller can update its `Option<usize>` mirror; a
/// multi-select caller ignores the return (it has no single index).
///
/// # Panics
/// * Panics if any index in `indices` is `>= leaves.len()` (message
///   contains "out of range").
/// * In single-select mode, panics if `indices.len() > 1` (message
///   contains "single-select mode").
pub fn replace_selection<L: SelectableLeaf>(
    leaves: &mut [L],
    indices: &[usize],
    multiselect: bool,
) -> Option<usize> {
    for &i in indices {
        assert!(
            i < leaves.len(),
            "selection index {i} out of range (len={})",
            leaves.len()
        );
    }
    if !multiselect {
        assert!(
            indices.len() <= 1,
            "single-select mode accepts at most one index, got {}",
            indices.len()
        );
    }
    for (j, leaf) in leaves.iter_mut().enumerate() {
        leaf.set_selected(indices.contains(&j));
    }
    indices.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal leaf fixture — a bare selection bit, exercising the
    /// helpers independently of the SCXML-backed production leaves.
    #[derive(Default)]
    struct Bit(bool);
    impl SelectableLeaf for Bit {
        fn is_selected(&self) -> bool {
            self.0
        }
        fn set_selected(&mut self, selected: bool) {
            self.0 = selected;
        }
    }

    fn bits(vals: &[bool]) -> Vec<Bit> {
        vals.iter().map(|&v| Bit(v)).collect()
    }

    #[test]
    fn select_exclusive_clears_siblings_on_fresh_gain() {
        let mut leaves = bits(&[false, false, false]);
        leaves[1].set_selected(true); // simulate the leaf's activation edge
        let landed = select_exclusive(&mut leaves, 1, /* was */ false);
        assert!(landed, "fresh false->true reports landed");
        assert!(leaves[1].is_selected());
        assert!(!leaves[0].is_selected() && !leaves[2].is_selected());
    }

    #[test]
    fn select_exclusive_noop_on_reactivation() {
        let mut leaves = bits(&[false, true, false]);
        // Re-activating the already-selected leaf: was=true, still true.
        let landed = select_exclusive(&mut leaves, 1, /* was */ true);
        assert!(!landed, "idempotent re-activation does not re-land");
        assert!(leaves[1].is_selected(), "selection retained");
    }

    #[test]
    fn toggle_off_completes_multi_reselection() {
        let mut leaf = Bit(true); // leaf edge set it true again
        toggle_off_if_reselected(&mut leaf, /* was */ true, /* activation */ true);
        assert!(
            !leaf.0,
            "re-activating a selected multi leaf toggles it off"
        );
    }

    #[test]
    fn toggle_off_leaves_fresh_selection_on() {
        let mut leaf = Bit(true);
        toggle_off_if_reselected(&mut leaf, /* was */ false, /* activation */ true);
        assert!(leaf.0, "fresh selection stays on");
    }

    #[test]
    fn detect_multi_emits_every_flip() {
        let before = SelectionSnapshot {
            multiselect: true,
            bits: vec![false, true, false],
        };
        let after = SelectionSnapshot {
            multiselect: true,
            bits: vec![true, false, false],
        };
        let intents = detect_intents(&before, &after);
        // bit 0 false->true, bit 1 true->false: two flips, two intents.
        assert_eq!(intents.len(), 2);
    }

    #[test]
    fn detect_single_emits_only_gains() {
        let before = SelectionSnapshot {
            multiselect: false,
            bits: vec![true, false],
        };
        let after = SelectionSnapshot {
            multiselect: false,
            bits: vec![false, true],
        };
        let intents = detect_intents(&before, &after);
        // bit 0 lost, bit 1 gained: single-select emits only the gain.
        assert_eq!(intents.len(), 1);
    }

    #[test]
    fn detect_no_change_is_silent() {
        let s = SelectionSnapshot {
            multiselect: true,
            bits: vec![true, false],
        };
        assert!(detect_intents(&s, &s).is_empty());
    }

    #[test]
    fn replace_selection_sets_exact_set_multi() {
        let mut leaves = bits(&[true, false, true]);
        let first = replace_selection(&mut leaves, &[1, 2], true);
        assert_eq!(first, Some(1));
        assert!(!leaves[0].0 && leaves[1].0 && leaves[2].0);
    }

    #[test]
    fn replace_selection_clears_on_empty() {
        let mut leaves = bits(&[true, true]);
        assert_eq!(replace_selection(&mut leaves, &[], true), None);
        assert!(!leaves[0].0 && !leaves[1].0);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn replace_selection_rejects_out_of_range() {
        let mut leaves = bits(&[false, false]);
        replace_selection(&mut leaves, &[5], true);
    }

    #[test]
    #[should_panic(expected = "single-select mode")]
    fn replace_selection_rejects_multi_in_single_mode() {
        let mut leaves = bits(&[false, false, false]);
        replace_selection(&mut leaves, &[0, 1], false);
    }
}
