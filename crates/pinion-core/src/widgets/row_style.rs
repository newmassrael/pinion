//! R998 §5.40 — **declarative per-row style rules** (Wireshark "coloring
//! rules" / dlt-viewer filter-colours) for a data grid at scale.
//!
//! A Wireshark packet list paints each row by the first **coloring rule** whose
//! display filter the packet matches; a dlt-viewer colours log lines by ECU /
//! app / level filters. This module is the grid analog: an ordered list of
//! [`RowStyleRule`]s, each a (predicate → tint) pair, and a row paints with the
//! **first** rule it matches (Wireshark's first-match precedence).
//!
//! ## The match half REUSES the R997 [`GridFilter`]
//!
//! The decisive reuse: a rule's predicate **is** the R997
//! [`GridFilter`](crate::widgets::grid_sort::GridFilter) conjunction — the same
//! multi-facet, multi-operator filter the grid's *filter* axis carries, sharing
//! [`GridFilter::matches`](crate::widgets::grid_sort::GridFilter::matches) and the
//! whole wire vocab. Coloring and filtering speak the same language: a rule
//! `"1>=500;#FF3030;#FFFFFF"` is "rows whose column 1 is ≥ 500 paint red-on-white",
//! reading the `1>=500` exactly as the filter axis would. The Model/View
//! separation holds — the rules are a **display** overlay computed from the
//! source cells, orthogonal to sort / filter / selection (a coloured row stays
//! coloured when re-sorted, and selection still wins the highlight).
//!
//! ## One reactive source of truth ([`RowStyleState`] + [`use_row_style`])
//!
//! The rule list lives **once** in a reactive [`RowStyleState`] — the
//! `ScrollState` / `GridSortState` pattern. The [`RowStyleExternal`] *mutates*
//! it (add / clear / restore over RPC); the view's row builder *reads* it
//! (`resolve` the first matching rule's tint per row) through the same `Rc`, so
//! the colours the view paints are the colours this external reports — by
//! construction.

use std::rc::Rc;

use crate::external::{
    query_proxy_external_impl, ExternalIntrospect, InterveneError, IntrospectSchema,
    IntrospectValue, InvokeError,
};
use crate::reactive::{Owner, Signal};
use crate::style::Color;
use crate::widgets::grid_sort::{grid_filter_from_str, grid_filter_str, GridFilter};

/// R998 §5.40 — the background + foreground colour a matched [`RowStyleRule`]
/// paints a row with. The wire form is `"<bg-hex>;<fg-hex>"` (each
/// [`Color::to_hex`] / [`Color::from_hex`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowTint {
    /// Row background fill.
    pub bg: Color,
    /// Row text colour.
    pub fg: Color,
}

impl RowTint {
    /// Construct from a background + foreground.
    #[must_use]
    pub fn new(bg: Color, fg: Color) -> Self {
        Self { bg, fg }
    }

    /// Render as the wire form `"<bg-hex>;<fg-hex>"`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("{};{}", self.bg.to_hex(), self.fg.to_hex())
    }

    /// Parse `"<bg-hex>;<fg-hex>"`; `None` if either half is not a hex colour.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        let (bg, fg) = s.split_once(';')?;
        Some(Self { bg: Color::from_hex(bg.trim())?, fg: Color::from_hex(fg.trim())? })
    }
}

/// R998 §5.40 — one row-coloring rule: rows whose cells satisfy
/// [`filter`](Self::filter) paint with [`tint`](Self::tint). The grid analog of
/// a Wireshark coloring rule; the predicate is the R997
/// [`GridFilter`](crate::widgets::grid_sort::GridFilter) conjunction (shared
/// wire vocab with the filter axis). The wire form is
/// `"<filter>;<bg-hex>;<fg-hex>"` (the filter half may itself contain `;` only
/// in a value — split takes the **last two** `;`-fields as the tint, the rest
/// is the filter; in practice a filter wire never contains `;`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowStyleRule {
    /// The predicate a row must satisfy to paint with this rule.
    pub filter: GridFilter,
    /// The colours a matching row paints with.
    pub tint: RowTint,
}

impl RowStyleRule {
    /// Construct a rule from a predicate + tint.
    #[must_use]
    pub fn new(filter: GridFilter, tint: RowTint) -> Self {
        Self { filter, tint }
    }

    /// Render as the wire form `"<filter>;<bg-hex>;<fg-hex>"`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("{};{}", grid_filter_str(Some(&self.filter)), self.tint.to_wire())
    }

    /// Parse `"<filter>;<bg-hex>;<fg-hex>"`; `None` when the filter does not
    /// parse or the trailing two `;`-fields are not hex colours. The tint is
    /// the **last two** `;`-separated fields, so a filter wire (which never
    /// contains `;`) is taken verbatim as the leading field.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        let (filter_wire, tint_wire) = s.rsplit_once(';').and_then(|(head, fg)| {
            let (filter_wire, bg) = head.rsplit_once(';')?;
            Some((filter_wire, format!("{bg};{fg}")))
        })?;
        let filter = grid_filter_from_str(filter_wire)?;
        let tint = RowTint::from_wire(&tint_wire)?;
        Some(Self { filter, tint })
    }
}

/// R998 §5.40 — the reactive **single source of truth** for a grid's row
/// coloring rules: an ordered [`Vec<RowStyleRule>`] held in a [`Signal`].
/// Created once via [`use_row_style`], shared by the [`RowStyleExternal`]
/// (which mutates it) and the view's row builder (which calls
/// [`resolve`](Self::resolve)) through the same `Rc`. Reading the rules inside
/// a view-fn auto-subscribes, so adding / clearing a rule repaints exactly like
/// a sort or filter change.
pub struct RowStyleState {
    tag: Option<&'static str>,
    rules: Signal<Vec<RowStyleRule>>,
}

impl Default for RowStyleState {
    fn default() -> Self {
        Self::new()
    }
}

impl RowStyleState {
    /// An empty rule list (no coloring).
    #[must_use]
    pub fn new() -> Self {
        Self { tag: None, rules: Signal::new(Vec::new()) }
    }

    /// As [`new`](Self::new) but records the `use_row_style` cache key.
    #[must_use]
    pub fn with_tag(key: &'static str) -> Self {
        Self { tag: Some(key), ..Self::new() }
    }

    /// The `use_row_style` cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// The current rules (a clone). Subscribes when read inside a view-fn.
    #[must_use]
    pub fn rules(&self) -> Vec<RowStyleRule> {
        self.rules.get()
    }

    /// The number of rules.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.get().len()
    }

    /// Append a rule (the lowest precedence — earlier rules match first). The
    /// resulting count is read back with [`rule_count`](Self::rule_count).
    pub fn add_rule(&self, rule: RowStyleRule) {
        let mut rules = self.rules.get();
        rules.push(rule);
        self.rules.set(rules);
    }

    /// Replace the whole rule list (admin / restore). A `Signal` write repaints
    /// every subscribed view.
    pub fn set_rules(&self, rules: Vec<RowStyleRule>) {
        self.rules.set(rules);
    }

    /// Clear every rule (rows revert to the default zebra fill).
    pub fn clear(&self) {
        self.rules.set(Vec::new());
    }

    /// The index of the **first** rule a row matches, or `None` — Wireshark's
    /// first-match precedence. `cell` yields the text of the row's column
    /// `col`. Subscribes to the rule `Signal` (reads it once per call).
    #[must_use]
    pub fn match_index<'a, F: Fn(usize) -> &'a str>(&self, cell: F) -> Option<usize> {
        Self::match_index_in(&self.rules.get(), cell)
    }

    /// The tint of the first rule a row matches, or `None` (no rule → default
    /// fill). The single-row convenience: `resolve(|c| cells[c])`.
    #[must_use]
    pub fn resolve<'a, F: Fn(usize) -> &'a str>(&self, cell: F) -> Option<RowTint> {
        let rules = self.rules.get();
        Self::match_index_in(&rules, cell).map(|i| rules[i].tint)
    }

    /// R998.1 — the **pure** first-match over a rule snapshot (no `Signal`
    /// read): the index of the first rule in `rules` a row matches, or `None`.
    /// A loop over many rows reads the `Signal` **once** (via [`rules`](Self::rules))
    /// then calls this per row — the [`GridSortState::order`](crate::widgets::grid_sort::GridSortState::order)
    /// read-once pattern, so coloring at scale never clones the rule list per
    /// row. The per-row matcher SSOT shared by the paint resolver, the
    /// instance [`match_index`](Self::match_index) / [`resolve`](Self::resolve),
    /// and the external's `matched_rows`.
    #[must_use]
    pub fn match_index_in<'a, F: Fn(usize) -> &'a str>(
        rules: &[RowStyleRule],
        cell: F,
    ) -> Option<usize> {
        rules.iter().position(|r| r.filter.matches(&cell))
    }
}

/// R998 §5.40 — resolve the shared [`RowStyleState`] for `key`, building it once
/// (empty). Mirrors [`use_grid_sort`](crate::widgets::grid_sort::use_grid_sort):
/// the `External` and the view both call this with the same `key` and receive
/// the same `Rc`, so the rule list is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook).
#[must_use]
pub fn use_row_style(key: &'static str) -> Rc<RowStyleState> {
    Owner::current()
        .expect("use_row_style requires an active Owner scope")
        .cache(key, || RowStyleState::with_tag(key))
}

/// A per-row cell source: `source(row)` yields that source row's cells, so the
/// [`RowStyleExternal`] can answer "which rule matches row R" over RPC (the
/// AI-first introspection the painted colours mirror). The binding wires this
/// from its dataset.
type RowCellSource = Rc<dyn Fn(usize) -> Vec<String>>;

/// R998 §5.40 — the **RPC adapter** over a shared [`RowStyleState`] (the
/// row-coloring peer of
/// [`GridSortExternal`](crate::widgets::grid_sort::GridSortExternal)).
///
/// Manages the rule list (`add_rule` / `clear` / restore the whole list) and —
/// when constructed with a [cell source](Self::with_source) — answers which
/// rule a given row matches (`match.<row>` / `tint.<row>`), so an AI client can
/// read the coloring without sampling pixels (§2 #7 scene-as-data). All state
/// lives in the shared [`RowStyleState`]; the view paints the same rules this
/// external reports.
#[derive(Clone)]
pub struct RowStyleExternal {
    state: Rc<RowStyleState>,
    /// Optional dataset accessor for per-row match introspection + the row
    /// count. `None` keeps the external rule-management-only.
    source: Option<RowCellSource>,
    row_count: usize,
}

impl core::fmt::Debug for RowStyleExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RowStyleExternal")
            .field("rules", &self.state.rule_count())
            .field("rows", &self.row_count)
            .finish_non_exhaustive()
    }
}

impl RowStyleExternal {
    /// Rule-management-only adapter (no per-row match introspection).
    #[must_use]
    pub fn new(state: Rc<RowStyleState>) -> Self {
        Self { state, source: None, row_count: 0 }
    }

    /// Attach a dataset (`row_count` rows, `source(row)` → its cells) so the
    /// external can answer `match.<row>` / `tint.<row>` / `matched_rows`.
    #[must_use]
    pub fn with_source(
        mut self,
        row_count: usize,
        source: impl Fn(usize) -> Vec<String> + 'static,
    ) -> Self {
        self.row_count = row_count;
        self.source = Some(Rc::new(source));
        self
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_row_style`]).
    #[must_use]
    pub fn state(&self) -> &Rc<RowStyleState> {
        &self.state
    }

    /// Render the whole rule list as the wire form (rules joined by `"|"`),
    /// or `"none"` when empty — the `rules` query / restore payload. The rule
    /// separator `"|"` (like the facet separator `"&"`) is reserved: a facet
    /// value must not contain `"|"` or `"&"`, else [`parse_rules_wire`](Self::parse_rules_wire)
    /// mis-splits it (a single rule round-trips through
    /// [`RowStyleRule::from_wire`] regardless of `";"` in the value, but the
    /// list layer has no escaping — coloring-rule values are identifiers /
    /// numbers in practice).
    fn rules_wire(&self) -> String {
        let rules = self.state.rules();
        if rules.is_empty() {
            "none".to_string()
        } else {
            rules.iter().map(RowStyleRule::to_wire).collect::<Vec<_>>().join("|")
        }
    }

    /// Parse a `"|"`-joined rule list (`"none"` / empty → no rules); any
    /// malformed rule is dropped. The restore peer of [`rules_wire`](Self::rules_wire).
    fn parse_rules_wire(s: &str) -> Vec<RowStyleRule> {
        s.split('|').filter_map(RowStyleRule::from_wire).collect()
    }

    /// The first rule a row matches (or `None`), via the attached source.
    fn match_index(&self, row: usize) -> Option<usize> {
        let source = self.source.as_ref()?;
        let cells = source(row);
        self.state.match_index(|c| cells.get(c).map_or("", String::as_str))
    }

    /// The number of rows matching any rule (0 without a source). Reads the
    /// rule `Signal` **once** for the whole sweep (the `GridSortState::order`
    /// read-once pattern — never one clone of the rule list per row).
    fn matched_rows(&self) -> usize {
        let Some(source) = self.source.as_ref() else {
            return 0;
        };
        let rules = self.state.rules();
        (0..self.row_count)
            .filter(|&r| {
                let cells = source(r);
                RowStyleState::match_index_in(&rules, |c| cells.get(c).map_or("", String::as_str))
                    .is_some()
            })
            .count()
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — a rule-list `Signal` write already repaints every subscribed view.
query_proxy_external_impl!(RowStyleExternal);

impl ExternalIntrospect for RowStyleExternal {
    fn schema(&self) -> IntrospectSchema {
        // `rules`        — whole list "<rule>|<rule>" / "none" (query + intervene).
        // `rule_count`   — number of rules (query only).
        // `rows`         — dataset row count (query only).
        // `matched_rows` — rows matching any rule (query only).
        // `rule`         — `rule.<i>` the i-th rule's wire form (query only).
        // `match`        — `match.<row>` first matching rule index / -1 (query only).
        // `tint`         — `tint.<row>` the matched tint "<bg>;<fg>" / "none" (query).
        // `add_rule`/`clear` — invoke channels.
        IntrospectSchema::new(&[
            ("rules", "string"),
            ("rule_count", "int"),
            ("rows", "int"),
            ("matched_rows", "int"),
            ("rule", "string"),
            ("match", "int"),
            ("tint", "string"),
            ("add_rule", "string"),
            ("clear", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        if let Some(rest) = path.strip_prefix("rule.") {
            let i = rest.parse::<usize>().ok()?;
            return Some(
                self.state
                    .rules()
                    .get(i)
                    .map_or(IntrospectValue::Null, |r| IntrospectValue::Text(r.to_wire())),
            );
        }
        if let Some(rest) = path.strip_prefix("match.") {
            let row = rest.parse::<usize>().ok()?;
            let idx = self.match_index(row).and_then(|i| i64::try_from(i).ok());
            return Some(IntrospectValue::Int(idx.unwrap_or(-1)));
        }
        if let Some(rest) = path.strip_prefix("tint.") {
            let row = rest.parse::<usize>().ok()?;
            let wire = self
                .match_index(row)
                .and_then(|i| self.state.rules().get(i).map(|r| r.tint.to_wire()))
                .unwrap_or_else(|| "none".to_string());
            return Some(IntrospectValue::Text(wire));
        }
        match path {
            "rules" => Some(IntrospectValue::Text(self.rules_wire())),
            "rule_count" => {
                Some(IntrospectValue::Int(i64::try_from(self.state.rule_count()).unwrap_or(i64::MAX)))
            }
            "rows" => Some(IntrospectValue::Int(i64::try_from(self.row_count).unwrap_or(i64::MAX))),
            "matched_rows" => {
                Some(IntrospectValue::Int(i64::try_from(self.matched_rows()).unwrap_or(i64::MAX)))
            }
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the whole rule list from its compound wire,
            // or clear it with Null.
            "rules" => match value {
                IntrospectValue::Text(ref s) => {
                    self.state.set_rules(Self::parse_rules_wire(s));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.state.clear();
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "rule_count" | "rows" | "matched_rows" | "rule" | "match" | "tint" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first: append a rule from its wire form, returning the new count.
            "add_rule" => match args {
                IntrospectValue::Text(ref s) => {
                    let rule = RowStyleRule::from_wire(s).ok_or(InvokeError::TypeMismatch)?;
                    self.state.add_rule(rule);
                    Ok(IntrospectValue::Int(i64::try_from(self.state.rule_count()).unwrap_or(i64::MAX)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Clear every rule, returning 0.
            "clear" => {
                self.state.clear();
                Ok(IntrospectValue::Int(0))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::grid_sort::{ColumnFacet, FilterOp};

    fn red_on_white() -> RowTint {
        RowTint::new(Color::rgb(255, 48, 48), Color::rgb(255, 255, 255))
    }

    fn green_on_black() -> RowTint {
        RowTint::new(Color::rgb(48, 255, 48), Color::rgb(0, 0, 0))
    }

    // A 4-row × 2-column grid: col 0 a category, col 1 numeric.
    fn cells() -> Vec<Vec<String>> {
        vec![
            vec!["A".to_string(), "30".to_string()], // 0
            vec!["B".to_string(), "10".to_string()], // 1
            vec!["A".to_string(), "5".to_string()],  // 2
            vec!["C".to_string(), "700".to_string()], // 3
        ]
    }

    fn cell_at(rows: &[Vec<String>], row: usize, col: usize) -> &str {
        rows.get(row).and_then(|r| r.get(col)).map_or("", String::as_str)
    }

    #[test]
    fn row_tint_wire_round_trips() {
        let t = red_on_white();
        assert_eq!(RowTint::from_wire(&t.to_wire()), Some(t));
        assert_eq!(RowTint::from_wire("#FF3030;#FFFFFF"), Some(red_on_white()));
        assert_eq!(RowTint::from_wire("nope"), None, "no ';' → not a tint");
        assert_eq!(RowTint::from_wire("#FF3030;zzz"), None, "bad fg hex");
    }

    #[test]
    fn rule_wire_round_trips_with_multi_facet_filter() {
        let rule = RowStyleRule::new(
            GridFilter::all(vec![
                ColumnFacet::new(0, FilterOp::Eq, "A"),
                ColumnFacet::new(1, FilterOp::Ge, "20"),
            ]),
            red_on_white(),
        );
        let wire = rule.to_wire();
        assert_eq!(wire, "0=A&1>=20;#ff3030;#ffffff", "to_hex emits lowercase");
        assert_eq!(RowStyleRule::from_wire(&wire), Some(rule));
        // from_wire folds hex case (so an uppercase admin payload restores).
        assert_eq!(
            RowStyleRule::from_wire("0=A&1>=20;#FF3030;#FFFFFF"),
            RowStyleRule::from_wire(&wire),
        );
        // A bare filter that fails to parse → no rule.
        assert_eq!(RowStyleRule::from_wire("garbage;#FF3030"), None, "needs filter;bg;fg");
    }

    #[test]
    fn resolve_first_match_wins() {
        let rows = cells();
        let st = RowStyleState::new();
        // Rule 0: col0 == A → red. Rule 1: col1 >= 100 → green.
        st.add_rule(RowStyleRule::new(GridFilter::eq(0, "A"), red_on_white()));
        st.add_rule(RowStyleRule::new(
            GridFilter::single(ColumnFacet::new(1, FilterOp::Ge, "100")),
            green_on_black(),
        ));
        // Row 0 (A,30): matches rule 0 only → red.
        assert_eq!(st.resolve(|c| cell_at(&rows, 0, c)), Some(red_on_white()));
        assert_eq!(st.match_index(|c| cell_at(&rows, 0, c)), Some(0));
        // Row 3 (C,700): matches rule 1 only → green.
        assert_eq!(st.resolve(|c| cell_at(&rows, 3, c)), Some(green_on_black()));
        // Row 1 (B,10): matches neither → no tint.
        assert_eq!(st.resolve(|c| cell_at(&rows, 1, c)), None);
    }

    #[test]
    fn first_match_precedence_when_two_rules_overlap() {
        let rows = cells();
        let st = RowStyleState::new();
        // Both rules match row 0 (A,30): col0==A AND col1>=20. First wins.
        st.add_rule(RowStyleRule::new(GridFilter::eq(0, "A"), red_on_white()));
        st.add_rule(RowStyleRule::new(
            GridFilter::single(ColumnFacet::new(1, FilterOp::Ge, "20")),
            green_on_black(),
        ));
        assert_eq!(st.resolve(|c| cell_at(&rows, 0, c)), Some(red_on_white()), "earlier rule wins");
        assert_eq!(st.match_index(|c| cell_at(&rows, 0, c)), Some(0));
    }

    #[test]
    fn external_rule_management_and_match_introspection() {
        let owner = Owner::new();
        owner.run(|| {
            let rows = cells();
            let st = use_row_style("rs");
            let mut ext = RowStyleExternal::new(Rc::clone(&st))
                .with_source(rows.len(), move |row| rows[row].clone());
            // Boot: empty.
            assert_eq!(ext.query("rule_count"), Some(IntrospectValue::Int(0)));
            assert_eq!(ext.query("rules"), Some(IntrospectValue::Text("none".into())));
            assert_eq!(ext.query("rows"), Some(IntrospectValue::Int(4)));
            // add_rule returns the new count.
            assert_eq!(
                ext.invoke("add_rule", IntrospectValue::Text("0=A;#FF3030;#FFFFFF".into())),
                Ok(IntrospectValue::Int(1)),
            );
            assert_eq!(
                ext.invoke("add_rule", IntrospectValue::Text("1>=100;#30FF30;#000000".into())),
                Ok(IntrospectValue::Int(2)),
            );
            assert_eq!(ext.query("rule.0"), Some(IntrospectValue::Text("0=A;#ff3030;#ffffff".into())));
            // match.<row>: row 0 (A,30) → rule 0; row 3 (C,700) → rule 1; row 1 → -1.
            assert_eq!(ext.query("match.0"), Some(IntrospectValue::Int(0)));
            assert_eq!(ext.query("match.3"), Some(IntrospectValue::Int(1)));
            assert_eq!(ext.query("match.1"), Some(IntrospectValue::Int(-1)));
            assert_eq!(ext.query("tint.0"), Some(IntrospectValue::Text("#ff3030;#ffffff".into())));
            assert_eq!(ext.query("tint.1"), Some(IntrospectValue::Text("none".into())));
            // matched_rows: rows 0,2 (A) + row 3 (700) = 3.
            assert_eq!(ext.query("matched_rows"), Some(IntrospectValue::Int(3)));
            // rules wire round-trips through intervene (to_hex emits lowercase).
            let wire = match ext.query("rules") {
                Some(IntrospectValue::Text(s)) => s,
                other => panic!("expected text, got {other:?}"),
            };
            assert_eq!(wire, "0=A;#ff3030;#ffffff|1>=100;#30ff30;#000000");
            ext.intervene("rules", IntrospectValue::Null).unwrap();
            assert_eq!(ext.query("rule_count"), Some(IntrospectValue::Int(0)));
            ext.intervene("rules", IntrospectValue::Text(wire.clone())).unwrap();
            assert_eq!(ext.query("rule_count"), Some(IntrospectValue::Int(2)), "restore");
            // clear via invoke.
            assert_eq!(ext.invoke("clear", IntrospectValue::Null), Ok(IntrospectValue::Int(0)));
            assert_eq!(ext.query("rule_count"), Some(IntrospectValue::Int(0)));
            // read-only slots reject intervene.
            assert_eq!(ext.intervene("rule_count", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
        });
    }
}
