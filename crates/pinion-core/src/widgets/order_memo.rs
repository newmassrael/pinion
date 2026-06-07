//! R780 §5.40 — shared **view-order proxy** machinery.
//!
//! The 1-D list sort/filter proxy ([`view_order`](crate::widgets::view_order))
//! and the data-grid column-sort proxy ([`grid_sort`](crate::widgets::grid_sort))
//! are deliberate **peers**, not one merged type: their config (`Option<bool>` +
//! filter vs `Option<(col, bool)>`), comparator (generic [`Ord`] key-extract vs
//! pairwise numeric `cell_cmp`), and wire vocabulary genuinely diverge, and
//! pinion deliberately has no retained `Model` trait to unify them over.
//!
//! But two pieces of each proxy are **mechanically identical**, and a
//! divergence in either would be a bug, not a style choice — so they live here
//! once (the R780 audit lift, on the second proxy consumer):
//!
//! - [`OrderMemo`] — the single-entry memo of a derived view value
//!   (`Rc<T>`), keyed on a config snapshot. The cache-invalidation dance
//!   (recompute only when the key changes) is the error-prone part; one
//!   correct copy serves all the proxies. The two sort proxies memoize a
//!   visual→source permutation (`T = Vec<usize>`, the default); the R821
//!   tree filter proxy is the **third** consumer, memoizing its filtered
//!   `Vec<VisibleRow>` flattening on the query string — the same
//!   invalidation contract over a different derived value.
//! - [`source_at_value`] — the `source_at.<pos>` introspect projection both
//!   sort proxy externals expose (out-of-range → `Null`, never absence).
//!   (The tree filter's visible rows are addressed by id, not source index,
//!   so it does not reuse this — its peerhood, like the permutation's.)

use std::rc::Rc;

use crate::external::IntrospectValue;

/// A single-entry memo of a derived view value (`Rc<T>`) keyed on a config
/// snapshot `K`. [`get`](Self::get) returns the cached value when the key is
/// unchanged, else recomputes and re-keys. `Option<K>` subsumes the "never
/// computed yet" state, so there is no separate `valid` flag to forget to set
/// (the bug the per-proxy `OrderCache` structs risked).
///
/// `T` defaults to `Vec<usize>` — the visual→source permutation the two sort
/// proxies memoize — so `OrderMemo<K>` keeps their meaning unchanged. The
/// R821 tree filter proxy instantiates `OrderMemo<String, Vec<VisibleRow>>`:
/// the *invalidation* contract is identical, only the derived value differs.
pub(crate) struct OrderMemo<K, T = Vec<usize>> {
    key: Option<K>,
    value: Rc<T>,
}

impl<K: PartialEq, T: Default> OrderMemo<K, T> {
    /// An empty memo (no key computed yet).
    pub(crate) fn new() -> Self {
        Self { key: None, value: Rc::new(T::default()) }
    }

    /// The memoized value for `key`: a cheap `Rc` clone on a hit, else
    /// `recompute()` re-keyed. The single source of truth for "recompute the
    /// derived view value only when the config (sort/filter/query) changed".
    pub(crate) fn get(&mut self, key: K, recompute: impl FnOnce() -> T) -> Rc<T> {
        if self.key.as_ref() != Some(&key) {
            self.value = Rc::new(recompute());
            self.key = Some(key);
        }
        Rc::clone(&self.value)
    }
}

/// The `source_at.<pos>` introspect projection shared by both proxy externals:
/// resolve a visual position (`rest`, the part after `"source_at."`) to its
/// source data index via `lookup` (the state's `source_at`). An out-of-range
/// or unparseable position reports [`IntrospectValue::Null`] (present-but-empty),
/// never absence — the caller has already matched the `source_at.` prefix.
pub(crate) fn source_at_value(
    rest: &str,
    lookup: impl Fn(usize) -> Option<usize>,
) -> IntrospectValue {
    rest.parse::<usize>()
        .ok()
        .and_then(lookup)
        .and_then(|src| i64::try_from(src).ok())
        .map_or(IntrospectValue::Null, IntrospectValue::Int)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_recomputes_only_on_key_change() {
        let mut memo: OrderMemo<u8> = OrderMemo::new();
        let mut calls = 0;
        let a = memo.get(1, || {
            calls += 1;
            vec![0, 1, 2]
        });
        assert_eq!(*a, vec![0, 1, 2]);
        assert_eq!(calls, 1);
        // Same key → cache hit, no recompute, same Rc.
        let b = memo.get(1, || {
            calls += 1;
            vec![9]
        });
        assert_eq!(calls, 1, "same key does not recompute");
        assert!(Rc::ptr_eq(&a, &b), "cache hit returns the same Rc");
        // New key → recompute.
        let c = memo.get(2, || {
            calls += 1;
            vec![2, 1, 0]
        });
        assert_eq!(*c, vec![2, 1, 0]);
        assert_eq!(calls, 2, "a changed key recomputes");
    }

    #[test]
    fn source_at_value_projects_and_null_pads() {
        let order = [3usize, 0, 2];
        let lookup = |p: usize| order.get(p).copied();
        assert_eq!(source_at_value("0", lookup), IntrospectValue::Int(3));
        assert_eq!(source_at_value("2", lookup), IntrospectValue::Int(2));
        assert_eq!(source_at_value("9", lookup), IntrospectValue::Null, "out of range → Null");
        assert_eq!(source_at_value("x", lookup), IntrospectValue::Null, "unparseable → Null");
    }
}
