//! R1851 §5.40 — an **ordered severity vocabulary** and the *at least this
//! severe* threshold over it.
//!
//! # Why a vocabulary and not a comparison
//!
//! A severity threshold looks like a comparison and is not one. *Warnings*
//! means warnings **and** errors, so the thing being compared is a POSITION in
//! an order, and the order is a fact about the vocabulary rather than about any
//! one row. Write the comparison per consumer and the vocabulary is implied by
//! whichever `if` chain happens to be in front of you; declare the vocabulary
//! and the comparison follows from it.
//!
//! # ★★★★★ What this exists to make impossible
//!
//! **A word the vocabulary does not hold is a REFUSAL that names it, not an
//! empty result.** Measured on the toolkit floor at 6.11.1, filtering a table
//! of six rows whose severity is spelled `err` by the word `error` answers
//! *zero of six* — a correct answer to a question nobody meant to ask, and
//! indistinguishable from *no row is that severe*. The behaviour reference this
//! framework reproduces has exactly that defect in the shipped article: its
//! alarm control offers `info / warn / error` over a feed whose rows are
//! spelled `info / warn / err`, so its most severe setting could never have
//! matched a row.
//!
//! The same probe measured the second half: *at least this severe* has to be
//! spelled out there as a pattern enumerating the words by hand
//! (`^(err|warn)$`), and one word misspelled inside it silently drops rows —
//! two of six survived where four should have. An order written out as an
//! alternation is an order nothing checks.
//!
//! # What is deliberately NOT here
//!
//! The permutation. Filtering and sorting a list is
//! [`compute_order`](crate::widgets::view_order::compute_order)'s job and has
//! been since R747; a second sorter would be a second answer to one question.
//! This module resolves words to RANKS and refuses the ones it does not know,
//! which is precisely the part `compute_order` cannot do because it never sees
//! a vocabulary. The two compose: resolve once, then pass
//! `ranks[i] >= floor` as the `pass` closure.

use core::fmt;

/// An ordered severity vocabulary, **least severe first**.
///
/// Constructed from a static list so a consumer can declare one in a `const`
/// item beside its rows, which is where a vocabulary belongs — the alternative
/// is building it at run time from something that could have been a different
/// list on the previous frame.
///
/// # Refusals are compile failures
///
/// [`SeverityScale::new`] is a `const fn` that panics on a scale that cannot
/// mean anything: empty, or holding one word twice. Declared in a `const` item
/// — which is the only way this type is meant to be built — those panics are
/// **compile errors**, so a vocabulary with two ranks for one word does not
/// reach a running program. See [`SeverityScale::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeverityScale {
    levels: &'static [&'static str],
}

/// Why a threshold could not be applied: a word the scale does not hold.
///
/// Carries the vocabulary as well as the offending word. A refusal that says
/// only *unknown* leaves a caller guessing at spelling, which is the position
/// the toolkit's silent empty result puts them in — the whole point of
/// refusing rather than dropping is that the answer is actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLevel {
    /// The word that is not in the scale.
    pub word: String,
    /// The words that are, least severe first.
    pub known: &'static [&'static str],
}

impl fmt::Display for UnknownLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a severity; they are {} (least severe first)",
            self.word,
            self.known.join(" < ")
        )
    }
}

impl core::error::Error for UnknownLevel {}

/// Byte equality of two `&str` in a `const` context.
///
/// `str::eq` is not `const`, and the duplicate check below has to run at
/// compile time or it is not the guarantee this type advertises.
const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

impl SeverityScale {
    /// Declare a vocabulary, least severe first.
    ///
    /// # Panics
    ///
    /// On an **empty** scale — a threshold over no words has no meaning and
    /// every lookup would refuse — and on a **repeated** word, which would give
    /// one level two ranks so that *at least `w`* answered differently
    /// depending on which occurrence was found. Both panics fire in a `const`
    /// context, where they are compile errors rather than run-time faults; that
    /// is the point of them being here rather than in a validator a caller may
    /// forget to run.
    ///
    /// ```compile_fail
    /// use pinion_core::widgets::severity::SeverityScale;
    /// const REPEATED: SeverityScale = SeverityScale::new(&["info", "warn", "info"]);
    /// ```
    #[must_use]
    pub const fn new(levels: &'static [&'static str]) -> Self {
        assert!(
            !levels.is_empty(),
            "a severity scale with no words cannot answer a threshold"
        );
        let mut i = 0;
        while i < levels.len() {
            let mut j = i + 1;
            while j < levels.len() {
                assert!(
                    !same(levels[i], levels[j]),
                    "a severity scale holds each word once: one word at two ranks makes \
                     `at least` depend on which occurrence is found"
                );
                j += 1;
            }
            i += 1;
        }
        Self { levels }
    }

    /// The vocabulary, least severe first — what a client enumerates instead of
    /// discovering the words from a sample.
    #[must_use]
    pub const fn levels(&self) -> &'static [&'static str] {
        self.levels
    }

    /// How many levels there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.levels.len()
    }

    /// Never true — [`SeverityScale::new`] refuses an empty scale.
    ///
    /// Present because a `len` without an `is_empty` is a clippy lint, and
    /// stating the invariant is better than allowing the lint.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// The rank of a word: `0` is least severe. `None` for a word this scale
    /// does not hold.
    #[must_use]
    pub fn rank(&self, level: &str) -> Option<usize> {
        self.levels.iter().position(|known| *known == level)
    }

    /// The word at a rank, or `None` past the end.
    #[must_use]
    pub fn name(&self, rank: usize) -> Option<&'static str> {
        self.levels.get(rank).copied()
    }

    /// The rank of a word, or a refusal naming it and the vocabulary.
    ///
    /// # Errors
    ///
    /// [`UnknownLevel`] when this scale does not hold `level`. That refusal is
    /// the whole point of the type: the alternative — answering "not severe
    /// enough" — is what the toolkit floor does, and it is indistinguishable
    /// from a row that really is below the threshold.
    pub fn require(&self, level: &str) -> Result<usize, UnknownLevel> {
        self.rank(level).ok_or_else(|| UnknownLevel {
            word: level.to_string(),
            known: self.levels,
        })
    }

    /// Is `level` at least as severe as `floor`?
    ///
    /// Refuses when **either** word is outside the vocabulary, which is the
    /// difference this module exists for: the alternative answers `false` and
    /// looks like a row that is not severe enough.
    ///
    /// # Errors
    ///
    /// [`UnknownLevel`] for the first of `level` and `floor` this scale does not
    /// hold. Both are checked — a table carrying one bad spelling cannot be
    /// silently short, and a threshold nobody can spell cannot pass as *nothing
    /// matched*.
    pub fn at_least(&self, level: &str, floor: &str) -> Result<bool, UnknownLevel> {
        Ok(self.require(level)? >= self.require(floor)?)
    }

    /// Resolve a whole table's words to ranks, refusing on the first one the
    /// vocabulary does not hold.
    ///
    /// The shape a consumer wants: resolve once, then index. Feeding
    /// [`compute_order`](crate::widgets::view_order::compute_order) a closure
    /// that re-resolved per comparison would pay a lookup per compare and — the
    /// sharper problem — would have nowhere to report a bad word from, since a
    /// comparator cannot fail.
    ///
    /// # Errors
    ///
    /// [`UnknownLevel`] for the first word this scale does not hold. Resolving
    /// stops there rather than collecting every bad word, because one
    /// misspelling is already a table this scale cannot grade and a list of them
    /// is no more actionable than the first.
    pub fn ranks<'a>(
        &self,
        levels: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<usize>, UnknownLevel> {
        levels.into_iter().map(|word| self.require(word)).collect()
    }

    /// The threshold as a per-row predicate over already-resolved `ranks`.
    ///
    /// `None` keeps everything — the *All* choice every such control has, which
    /// is an absent floor rather than the least severe word. Those two are the
    /// same set today and stop being the same set the moment a scale grows a
    /// word below its current floor, so they are spelled differently.
    #[must_use]
    pub fn passes(floor: Option<usize>, rank: usize) -> bool {
        floor.is_none_or(|least| rank >= least)
    }
}

#[cfg(test)]
mod tests {
    use super::{SeverityScale, UnknownLevel};
    use crate::widgets::view_order::compute_order;

    const SEV: SeverityScale = SeverityScale::new(&["info", "warn", "error"]);

    #[test]
    fn the_vocabulary_is_ordered_least_severe_first() {
        assert_eq!(SEV.levels(), &["info", "warn", "error"]);
        assert_eq!(SEV.len(), 3);
        assert!(!SEV.is_empty());
        assert_eq!(SEV.rank("info"), Some(0));
        assert_eq!(SEV.rank("error"), Some(2));
        assert_eq!(SEV.name(1), Some("warn"));
        assert_eq!(SEV.name(3), None);
    }

    #[test]
    fn a_threshold_is_an_order_and_not_a_match() {
        // *warnings* means warnings AND errors. Three independent flags could
        // not say this, which is why the scale is ordered rather than a set.
        assert_eq!(SEV.at_least("error", "warn"), Ok(true));
        assert_eq!(SEV.at_least("warn", "warn"), Ok(true));
        assert_eq!(SEV.at_least("info", "warn"), Ok(false));
    }

    /// ★★★★★ The measured difference from the toolkit floor at 6.11.1: there,
    /// filtering rows spelled `err` by the word `error` answers *zero of six*
    /// and says nothing. Here the word itself is refused, and the refusal
    /// carries the vocabulary so the caller can fix the spelling.
    #[test]
    fn a_word_outside_the_vocabulary_is_refused_by_name() {
        let scale = SeverityScale::new(&["info", "warn", "err"]);
        let refused = scale.at_least("err", "error").expect_err("must refuse");
        assert_eq!(
            refused,
            UnknownLevel {
                word: "error".to_string(),
                known: &["info", "warn", "err"],
            }
        );
        let said = refused.to_string();
        assert!(said.contains("\"error\""), "{said}");
        assert!(said.contains("info < warn < err"), "{said}");
        // And the row's own word is refused just as loudly as the floor's, so a
        // table carrying one bad spelling cannot be silently short.
        assert!(scale.at_least("ERROR", "info").is_err());
        assert!(scale.ranks(["info", "nope"]).is_err());
    }

    /// The composition this module is shaped for: it resolves, the existing
    /// permutation SSOT orders. No second sorter.
    #[test]
    fn it_composes_with_the_view_order_permutation() {
        let words = ["err", "info", "warn", "err"];
        let scale = SeverityScale::new(&["info", "warn", "err"]);
        let ranks = scale.ranks(words).expect("every word is in the scale");
        let floor = scale.rank("warn");

        let order = compute_order(
            words.len(),
            Some(false),
            |i| ranks[i],
            |i| SeverityScale::passes(floor, ranks[i]),
        );
        // Most severe first, ties in source order, and `info` filtered out.
        assert_eq!(order, vec![0, 3, 2]);

        let all = compute_order(
            words.len(),
            None,
            |i| ranks[i],
            |i| SeverityScale::passes(None, ranks[i]),
        );
        assert_eq!(all, vec![0, 1, 2, 3], "an absent floor keeps everything");
    }

    #[test]
    fn an_absent_floor_and_the_least_word_are_spelled_differently() {
        // The same set today. Different sentences, so a scale that grows a word
        // below `info` does not silently change what *All* means.
        assert!(SeverityScale::passes(None, 0));
        assert!(SeverityScale::passes(SEV.rank("info"), 0));
        let wider = SeverityScale::new(&["trace", "info", "warn", "error"]);
        assert!(!SeverityScale::passes(wider.rank("info"), 0));
    }
}
