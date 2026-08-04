//! R1561 §5.27 §5.40 — **a set of indices held as the runs it is made of**.
//!
//! A selection over a virtualized collection is not a set of rows. It is a set
//! of *runs*: "rows 0 through 999 999" is **one fact**, and holding it as a
//! million facts makes the cost of storing it, comparing it, and — this being a
//! framework whose §2 #7 invariant is that the scene is queryable as data —
//! **saying** it proportional to the model rather than to the statement.
//!
//! Measured before this type existed, on the shipped `hello-multi-select`
//! binding (10 000 rows, `VirtualSelect` backed by a `BTreeSet<usize>`):
//!
//! | wire call | answer | bytes | round-trip |
//! |---|---|---|---|
//! | `invoke("select_all")` | 10 000 integers | 58 890 | 14.3 ms |
//! | `query("selection")` | 10 000 integers | 58 890 | 10.9 ms |
//!
//! The fact those 58 890 bytes state is `[[0, 9999]]` — thirteen. On the
//! 1 000 000-row model this axis is *named* for, the same two calls are ~5.9 MB
//! and ~1.1 s each, per query, forever.
//!
//! # The invariant, and why it is the whole type
//!
//! [`IndexRuns`] keeps its runs sorted, disjoint **and non-adjacent**: `{0..=4,
//! 5..=9}` is not a value this type can hold, because it *is* `{0..=9}`. So the
//! representation is **canonical** — two `IndexRuns` are equal exactly when they
//! contain the same indices — and that is load-bearing rather than tidy.
//! [`VirtualSelect`](super::virtual_select::VirtualSelect) reports whether an
//! interaction changed anything by comparing the new selection with the old; a
//! representation that could spell one selection two ways would report a change
//! that did not happen, and every consumer downstream of the `changed` bool —
//! the intent channel, the repaint, the demo assertion — would act on it.
//!
//! The invariant also survives the wire: [`IndexRuns`] deserializes through
//! [`From<Vec<(usize, usize)>>`](IndexRuns#impl-From<Vec<(usize,+usize)>>-for-IndexRuns),
//! which canonicalises, so a hand-written snapshot, a restored session or a
//! malformed `intervene` payload cannot put the type into a state its own
//! methods refuse to build.
//!
//! # Against Qt 6.11
//!
//! Qt's selection is already range-based — `QItemSelection` is a
//! `QList<QItemSelectionRange>` — so the *idea* is Qt's floor, not this
//! round's invention. Four things here are past it, and each is a consequence
//! of the canonical invariant rather than an extra feature:
//!
//! - **The count is answerable.** `QItemSelectionModel` has `hasSelection()`
//!   and no count accessor at all, so "how many rows are selected" is
//!   `selectedRows().size()` — a `QModelIndexList` with one `QModelIndex` per
//!   selected row, built and thrown away to read its length. [`IndexRuns::len`]
//!   sums run lengths.
//! - **The extremes are O(1).** `QItemSelectionModel::selectedIndexes()` is
//!   documented to return a list that "contains no duplicates, and is not
//!   sorted", so the *first* selected row — the target of every
//!   scroll-to-selection and find-next-selected — costs a scan of the whole
//!   selection. [`IndexRuns::first`] and [`IndexRuns::last`] read the ends
//!   of the run vector.
//! - **Equality is set equality.** Only `QItemSelection::merge` is documented
//!   to guarantee non-overlapping ranges; the container itself permits them,
//!   which is why `selectedIndexes()` has to promise de-duplication. Two Qt
//!   selections covering the same rows can therefore differ as values,
//!   depending on the order the `select()` calls arrived in.
//! - **Materialising is a written decision.** [`IndexRuns::iter`] is the only
//!   way to get one index per row out of this type, and it is lazy and named,
//!   so a call site that pays the model's size is greppable. `indexes()` is how
//!   a Qt consumer reads a selection at all.
//!
//! Deliberately **not** adopted: Qt's two-dimensional range
//! (`QItemSelectionRange` spans rows *and* columns, because `QItemSelectionModel`
//! selects `QModelIndex`es). The selection this framework windows is the **row**
//! axis — [`VirtualSelect`](super::virtual_select::VirtualSelect) is keyed by
//! data index — and a second dimension nothing selects on would be a field every
//! consumer had to fill in with "all columns". The *capability* is Qt's floor;
//! the *shape* is chosen each time.

use core::fmt;

/// R1561 — one inclusive run of indices, `first ..= last`.
///
/// Inclusive rather than half-open because the end is a **row that is
/// selected**: `Run { first: 0, last: 999_999 }` names row 999 999, and the
/// half-open spelling would name a row one past the model's last, which
/// [`VirtualSelect`](super::virtual_select::VirtualSelect)'s bound would then
/// have to special-case. It also makes an empty run unrepresentable — `first <=
/// last` always holds — so "a run that selects nothing" is not a value anyone
/// has to handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Run {
    /// The first selected index.
    pub first: usize,
    /// The last selected index, inclusive.
    pub last: usize,
}

impl Run {
    /// How many indices this run covers. **Never zero** — `first <= last`
    /// always holds — which is why it is `count` rather than `len`: a `len`
    /// invites an `is_empty` peer, and "an empty run" is not a state this type
    /// has.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.last - self.first + 1
    }

    /// Whether `index` falls inside this run.
    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }
}

/// R1561 §5.27 §5.40 — a canonical set of `usize` indices, stored as runs.
///
/// See the [module documentation](self) for the invariant and what rests on it.
///
/// The methods divide into three groups by what they cost:
///
/// - **O(1)**: [`is_empty`](Self::is_empty), [`run_count`](Self::run_count),
///   [`first`](Self::first), [`last`](Self::last), [`runs`](Self::runs).
/// - **O(runs)**: [`len`](Self::len), [`insert_run`](Self::insert_run),
///   [`insert`](Self::insert), [`remove`](Self::remove),
///   [`clamped_below`](Self::clamped_below), equality.
/// - **O(log runs)**: [`contains`](Self::contains).
///
/// Not one of them is O(indices) — except [`iter`](Self::iter), which is that
/// by definition and is named so the cost is visible at the call site.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Vec<(usize, usize)>", into = "Vec<(usize, usize)>")]
pub struct IndexRuns {
    /// Sorted by `first`, pairwise disjoint, and separated by at least one
    /// unselected index — the canonical form every constructor and every
    /// mutator restores before returning.
    runs: Vec<Run>,
}

impl IndexRuns {
    /// The empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self { runs: Vec::new() }
    }

    /// The set covering exactly `first ..= last`. `last < first` is the empty
    /// set — the one place a caller can name a backwards run, and it means
    /// "nothing", not "one index".
    #[must_use]
    pub fn run(first: usize, last: usize) -> Self {
        if last < first {
            return Self::new();
        }
        Self {
            runs: vec![Run { first, last }],
        }
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// How many **indices** are in the set — Qt's missing
    /// `QItemSelectionModel::count()`.
    ///
    /// O(runs), so a whole-model selection answers from one addition rather
    /// than by building the list Qt has to build to call `.size()` on it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.iter().map(Run::count).sum()
    }

    /// How many **runs** the set is made of: the size of the representation,
    /// and the number a scale claim is stated in.
    ///
    /// One run is not a smaller selection than a thousand — it is a selection
    /// that is *cheaper to hold*, which is the property
    /// [`select_all`](super::virtual_select::VirtualSelect::select_all) has and
    /// a `BTreeSet` could not.
    #[must_use]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// The runs, ascending and canonical.
    #[must_use]
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The lowest selected index, or `None`.
    #[must_use]
    pub fn first(&self) -> Option<usize> {
        self.runs.first().map(|r| r.first)
    }

    /// The highest selected index, or `None`.
    #[must_use]
    pub fn last(&self) -> Option<usize> {
        self.runs.last().map(|r| r.last)
    }

    /// Whether `index` is selected. Binary search over the runs — the runs are
    /// sorted and disjoint, so the one that could contain `index` is the first
    /// whose `last` reaches it.
    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        let at = self.runs.partition_point(|r| r.last < index);
        self.runs.get(at).is_some_and(|r| r.first <= index)
    }

    /// Every selected index, ascending.
    ///
    /// **The materialising accessor**, and the only one. A call site that uses
    /// it pays the selection's size in indices rather than in runs, which is
    /// the cost this whole type exists to avoid — so it is a lazy iterator with
    /// a name a census can find, not a `Vec` returned from something that reads
    /// like a getter.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.runs.iter().flat_map(|r| r.first..=r.last)
    }

    /// Deselect everything.
    pub fn clear(&mut self) {
        self.runs.clear();
    }

    /// Add `first ..= last` to the set, merging with anything it touches or
    /// abuts. `last < first` is a no-op. Returns whether the set changed.
    ///
    /// Abutting counts as touching: inserting `5..=9` next to `0..=4` yields
    /// `0..=9`, one run. That is the canonical invariant being maintained at
    /// the only place that can break it.
    pub fn insert_run(&mut self, first: usize, last: usize) -> bool {
        if last < first {
            return false;
        }
        // The first run that is not entirely below the new one, allowing for
        // abutment: a run ending at `first - 1` must merge, so it is "not
        // below". Saturating because `first` may be 0.
        let lo = self
            .runs
            .partition_point(|r| r.last < first.saturating_sub(1));
        // The first run entirely above the new one, again allowing abutment.
        let hi = self
            .runs
            .partition_point(|r| r.first <= last.saturating_add(1));
        if lo == hi {
            self.runs.insert(lo, Run { first, last });
            return true;
        }
        let merged = Run {
            first: first.min(self.runs[lo].first),
            last: last.max(self.runs[hi - 1].last),
        };
        if hi - lo == 1 && self.runs[lo] == merged {
            return false;
        }
        self.runs.splice(lo..hi, core::iter::once(merged));
        true
    }

    /// Add one index. Returns whether the set changed.
    pub fn insert(&mut self, index: usize) -> bool {
        self.insert_run(index, index)
    }

    /// Remove one index, splitting the run it was in if it was in the middle of
    /// one. Returns whether the set changed.
    pub fn remove(&mut self, index: usize) -> bool {
        let at = self.runs.partition_point(|r| r.last < index);
        let Some(&run) = self.runs.get(at) else {
            return false;
        };
        if run.first > index {
            return false;
        }
        match (run.first == index, run.last == index) {
            (true, true) => {
                self.runs.remove(at);
            }
            (true, false) => self.runs[at].first = index + 1,
            (false, true) => self.runs[at].last = index - 1,
            (false, false) => {
                self.runs[at].last = index - 1;
                self.runs.insert(
                    at + 1,
                    Run {
                        first: index + 1,
                        last: run.last,
                    },
                );
            }
        }
        true
    }

    /// The set with every index `>= bound` dropped — the model's validity
    /// clamp, applied per **run** rather than per index.
    ///
    /// [`VirtualSelect`](super::virtual_select::VirtualSelect) bounds every
    /// selection by its `item_count` so a malformed wire payload can never
    /// select a row that does not exist. Before this type that was a filter
    /// over every index; a run is clamped by writing one number.
    #[must_use]
    pub fn clamped_below(&self, bound: usize) -> Self {
        let keep = self.runs.partition_point(|r| r.first < bound);
        let mut runs = self.runs[..keep].to_vec();
        if let Some(tail) = runs.last_mut() {
            tail.last = tail.last.min(bound - 1);
        }
        Self { runs }
    }
}

/// Runs, not indices — so a `{:?}` of a whole-model selection is one pair
/// rather than the model.
impl fmt::Debug for IndexRuns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IndexRuns")?;
        f.debug_list()
            .entries(self.runs.iter().map(|r| (r.first, r.last)))
            .finish()
    }
}

/// The wire and snapshot form: `[[first, last], …]`.
///
/// **Canonicalising**, which is what makes the invariant hold through serde.
/// The pairs may arrive in any order, overlapping, abutting, or backwards — a
/// hand-written `intervene` payload, an older snapshot, a client that built the
/// list by appending as the user clicked — and the value that results is the
/// same one the constructors build. A backwards pair (`last < first`) selects
/// nothing rather than being an error, matching [`IndexRuns::run`]: the wire
/// says which rows are selected, and a pair naming no rows names no rows.
impl From<Vec<(usize, usize)>> for IndexRuns {
    fn from(pairs: Vec<(usize, usize)>) -> Self {
        let mut out = Self::new();
        for (first, last) in pairs {
            out.insert_run(first, last);
        }
        out
    }
}

impl From<IndexRuns> for Vec<(usize, usize)> {
    fn from(value: IndexRuns) -> Self {
        value.runs.iter().map(|r| (r.first, r.last)).collect()
    }
}

impl FromIterator<usize> for IndexRuns {
    /// Build from loose indices — the adapter for a caller that genuinely has
    /// them (a test, a click history, a decoded legacy payload). Ascending
    /// input costs one `insert_run` per run; arbitrary order costs one per
    /// index, which is the price of not having runs to begin with.
    fn from_iter<I: IntoIterator<Item = usize>>(iter: I) -> Self {
        let mut out = Self::new();
        for i in iter {
            out.insert(i);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical invariant, asserted directly rather than through the
    /// accessors that assume it: sorted, non-empty, and separated by at least
    /// one gap.
    fn assert_canonical(s: &IndexRuns) {
        for r in s.runs() {
            assert!(r.first <= r.last, "empty run {r:?} in {s:?}");
        }
        for w in s.runs().windows(2) {
            assert!(
                w[0].last + 1 < w[1].first,
                "runs {:?} and {:?} are adjacent or overlapping in {s:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn r1561_abutting_runs_merge_into_one() {
        let mut s = IndexRuns::run(0, 4);
        assert!(s.insert_run(5, 9), "abutting insert changes the set");
        assert_canonical(&s);
        assert_eq!(s.run_count(), 1, "0..=4 and 5..=9 are one run");
        assert_eq!(s.runs()[0], Run { first: 0, last: 9 });
    }

    #[test]
    fn r1561_equal_selections_compare_equal_however_they_were_built() {
        let a = IndexRuns::run(0, 9);
        let b: IndexRuns = (0..10).collect();
        let mut c = IndexRuns::new();
        // built backwards, in three overlapping pieces
        c.insert_run(5, 9);
        c.insert_run(0, 2);
        c.insert_run(2, 6);
        assert_eq!(a, b, "one run equals ten inserts");
        assert_eq!(a, c, "one run equals three overlapping inserts");
        assert_canonical(&c);
        assert_eq!(c.run_count(), 1);
    }

    #[test]
    fn r1561_removing_a_middle_index_splits_the_run() {
        let mut s = IndexRuns::run(0, 9);
        assert!(s.remove(5));
        assert_canonical(&s);
        assert_eq!(s.run_count(), 2, "a hole makes two runs");
        assert_eq!(s.len(), 9);
        assert!(!s.contains(5));
        assert!(s.contains(4) && s.contains(6));
        // and putting it back restores the single run — canonicality is not
        // one-way
        assert!(s.insert(5));
        assert_eq!(s, IndexRuns::run(0, 9), "re-inserting closes the hole");
        assert_eq!(s.run_count(), 1);
    }

    #[test]
    fn r1561_remove_at_the_ends_shrinks_rather_than_splitting() {
        let mut s = IndexRuns::run(3, 5);
        assert!(s.remove(3));
        assert_eq!(s, IndexRuns::run(4, 5));
        assert!(s.remove(5));
        assert_eq!(s, IndexRuns::run(4, 4));
        assert!(s.remove(4));
        assert!(s.is_empty(), "removing the last index empties the set");
        assert!(!s.remove(4), "removing what is not there changes nothing");
    }

    #[test]
    fn r1561_whole_model_selection_is_one_run_and_len_is_o_of_runs() {
        let s = IndexRuns::run(0, 999_999);
        assert_eq!(s.run_count(), 1, "a million rows, one run");
        assert_eq!(s.len(), 1_000_000, "the count is exact");
        assert_eq!(s.first(), Some(0));
        assert_eq!(s.last(), Some(999_999));
        assert!(s.contains(500_000));
        assert!(!s.contains(1_000_000));
    }

    #[test]
    fn r1561_serde_round_trip_is_runs_and_canonicalises_on_the_way_in() {
        let s = IndexRuns::run(0, 999_999);
        let json = serde_json::to_string(&s).expect("serializes");
        assert_eq!(json, "[[0,999999]]", "the wire form is the runs");
        // The uncanonical spellings a client could send, all decoding to the
        // value the constructors build.
        for raw in [
            "[[0,4],[5,9]]", // abutting
            "[[5,9],[0,4]]", // out of order
            "[[0,6],[3,9]]", // overlapping
            "[[0,9],[9,3]]", // trailing backwards pair selects nothing
            "[[0,2],[3,5],[6,9]]",
        ] {
            let decoded: IndexRuns = serde_json::from_str(raw).expect("decodes");
            assert_canonical(&decoded);
            assert_eq!(decoded, IndexRuns::run(0, 9), "{raw} is 0..=9");
        }
    }

    #[test]
    fn r1561_clamp_below_trims_the_straddling_run_rather_than_dropping_it() {
        let mut s = IndexRuns::run(0, 4);
        s.insert_run(10, 20);
        let clamped = s.clamped_below(15);
        assert_canonical(&clamped);
        assert_eq!(clamped.run_count(), 2);
        assert_eq!(clamped.last(), Some(14), "the straddling run is trimmed");
        assert_eq!(clamped.len(), 10);
        assert!(
            s.clamped_below(10).runs().iter().all(|r| r.last < 10),
            "a run starting at the bound is dropped whole"
        );
        assert!(IndexRuns::run(0, 9).clamped_below(0).is_empty());
    }

    #[test]
    fn r1561_insert_reports_no_change_when_already_covered() {
        let mut s = IndexRuns::run(0, 9);
        assert!(!s.insert(5), "inserting a covered index changes nothing");
        assert!(!s.insert_run(2, 7), "a covered run changes nothing");
        assert!(!s.insert_run(0, 9), "the identical run changes nothing");
        assert!(s.insert_run(0, 10), "growing by one does change it");
        assert!(!s.insert_run(9, 3), "a backwards run changes nothing");
    }

    #[test]
    fn r1561_iter_is_the_only_materialising_accessor() {
        let mut s = IndexRuns::run(0, 2);
        s.insert_run(7, 8);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 1, 2, 7, 8]);
        assert_eq!(s.len(), 5, "len agrees with iter");
        assert_eq!(s.iter().count(), s.len());
    }

    #[test]
    fn r1561_debug_prints_runs_not_indices() {
        let s = IndexRuns::run(0, 999_999);
        assert_eq!(format!("{s:?}"), "IndexRuns[(0, 999999)]");
    }
}
