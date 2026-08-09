//! R1550 §5.16 §5.36 §5.7 §2 #2 §2 #7 — [`MemoryCensus`]: every arena this
//! process holds, and what each one is holding.
//!
//! # Why a census and not a total
//!
//! One number is unactionable. "The process is using 180 MB" does not say
//! whether to shrink a cache, drop a decoded image, or leave it alone. The
//! answer an agent can act on is per-arena, which is the same reason
//! `scene/frame_timings` reports phases rather than a frame total.
//!
//! It is a **census** in R1538's sense: every arena the shell holds appears,
//! including the ones holding nothing. An arena missing from this list is a
//! defect, not a zero — which is what makes `r1550_arena_states_its_bytes.py`
//! able to gate coverage by comparing the published rows against the source.
//!
//! # Per-window rows
//!
//! A shell with three windows holds three paint-fragment arenas and three
//! image arenas, and they can differ by orders of magnitude — a DCC viewport
//! against a palette. So [`ArenaFootprint::window`] names the window a per-window arena belongs to,
//! and is `None` for the shell-wide ones. The toolkit has no equivalent to
//! attribute against, because it has no memory report at all.
//!
//! # The unattributed remainder
//!
//! [`MemoryCensus::process_rss_bytes`] is what the OS says the process is resident for. The arenas will
//! never sum to it — the widget tree, the font collection, the GPU driver's
//! own allocations and the binary itself are all in there — and stating both
//! is what keeps the arena numbers from being read as a process total. The
//! engine's `stat memory` reports platform stats beside its allocator's; the toolkit
//! reports neither.

use crate::footprint::Footprint;

/// R1550 — which arena a row describes.
///
/// Exhaustive rather than open: adding an arena to the framework is a
/// deliberate act that should have to be written down here, so that the
/// census's coverage is a property of the type rather than of whoever
/// remembered to publish a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arena {
    /// The §5.16 paint fragment cache — encoded scene fragments, one per
    /// cacheable container. Per window.
    PaintFragments,
    /// The §5.36 shape cache — shaped text layouts with their derived draw
    /// lists and background bands. Per shell.
    TextShapes,
    /// Decoded images, held so a source is read and decoded once. Per window.
    Images,
}

impl Arena {
    /// The name this arena carries on the wire. Stable — an agent matches on
    /// it, so it is chosen once and not derived from the Rust identifier.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PaintFragments => "paint-fragments",
            Self::TextShapes => "text-shapes",
            Self::Images => "images",
        }
    }

    /// Every arena, in the order a census lists them. The census's own
    /// definition of complete.
    pub const ALL: [Self; 3] = [Self::PaintFragments, Self::TextShapes, Self::Images];
}

/// R1550 — how completely a row's [`ArenaFootprint::bytes`] describes what the
/// arena holds.
///
/// **Derived, never stored.** [`ArenaFootprint::basis`] computes it from
/// [`ArenaFootprint::unmeasured`], so a row cannot claim to be exact while
/// naming values it could not size — the two would be one fact stated twice,
/// free to disagree after the next foreign type arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FootprintBasis {
    /// Every byte the arena holds is counted.
    Exact,
    /// The bytes pinion allocated are counted; the arena also holds values
    /// whose interiors no API outside their own crate can size. Which ones,
    /// and how many, is in [`ArenaFootprint::unmeasured`].
    Partial,
}

impl FootprintBasis {
    /// The name this basis carries on the wire.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Partial => "partial",
        }
    }
}

/// R1550 — values an arena holds whose interior cannot be sized from outside
/// the crate that owns them.
///
/// Named rather than counted anonymously: "300 values are unmeasured" is a
/// footnote, "300 `parley::Layout`" is an attributable limit an agent can act
/// on — and an upstream issue can be filed against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmeasuredValues {
    /// The type's path, as a reader would search for it.
    pub type_name: &'static str,
    /// How many of them the arena holds.
    pub count: u64,
}

impl UnmeasuredValues {
    /// `count` values of `type_name`.
    #[must_use]
    pub const fn new(type_name: &'static str, count: u64) -> Self {
        Self { type_name, count }
    }
}

/// R1550 — one arena's row in a [`MemoryCensus`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArenaFootprint {
    /// Which arena this row describes.
    pub arena: Arena,
    /// The window this arena belongs to, or `None` for a shell-wide arena.
    pub window: Option<String>,
    /// Heap bytes the arena is holding, per the [`Footprint`] contract.
    pub bytes: u64,
    /// Live entries — the number `scene/cache_stats` and
    /// `scene/text_cache_stats` already report. Here so that bytes-per-entry
    /// is one division rather than two round trips.
    pub entries: u64,
    /// What [`Self::bytes`] leaves out, by type. Empty for an arena measured
    /// to the byte.
    pub unmeasured: Vec<UnmeasuredValues>,
    /// The byte budget this arena evicts to stay under, or `None` when it is
    /// bounded by something other than bytes (or not at all).
    ///
    /// `cacheLimit()` is the toolkit equivalent and is the whole of what the toolkit
    /// publishes; the number beside it — how much of the budget is in use —
    /// has a toolkit accessor at all.
    pub budget_bytes: Option<u64>,
}

impl ArenaFootprint {
    /// A row for an arena whose every byte is counted.
    #[must_use]
    pub fn exact(arena: Arena, bytes: u64, entries: u64) -> Self {
        Self {
            arena,
            window: None,
            bytes,
            entries,
            unmeasured: Vec::new(),
            budget_bytes: None,
        }
    }

    /// A row for an arena that also holds values whose interiors cannot be
    /// sized from here.
    #[must_use]
    pub fn partial(
        arena: Arena,
        bytes: u64,
        entries: u64,
        unmeasured: Vec<UnmeasuredValues>,
    ) -> Self {
        Self {
            arena,
            window: None,
            bytes,
            entries,
            unmeasured,
            budget_bytes: None,
        }
    }

    /// Whether every byte this arena holds is in [`Self::bytes`].
    #[must_use]
    pub fn basis(&self) -> FootprintBasis {
        if self.unmeasured.iter().all(|u| u.count == 0) {
            FootprintBasis::Exact
        } else {
            FootprintBasis::Partial
        }
    }

    /// Attribute this row to `window`.
    #[must_use]
    pub fn in_window(mut self, window: &str) -> Self {
        self.window = Some(window.to_owned());
        self
    }

    /// State the byte budget this arena evicts to stay under.
    #[must_use]
    pub fn with_budget(mut self, budget_bytes: u64) -> Self {
        self.budget_bytes = Some(budget_bytes);
        self
    }
}

/// R1550 — what the whole process is holding, arena by arena.
///
/// Published as `scene/memory`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryCensus {
    /// One row per arena per owner. See the module docs — every arena the
    /// shell holds appears, including empty ones.
    pub arenas: Vec<ArenaFootprint>,
    /// Resident set size the OS reports for this process, or `None` where the
    /// platform has no reader wired (see `pinion-platform-memory`).
    pub process_rss_bytes: Option<u64>,
}

impl MemoryCensus {
    /// Total bytes across every row.
    ///
    /// A derived number rather than a stored one: a stored total is a second
    /// statement of the same fact, free to disagree with the rows after the
    /// next arena is added.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.arenas.iter().map(|a| a.bytes).sum()
    }

    /// Whether every row's bytes are exact.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.arenas
            .iter()
            .all(|a| a.basis() == FootprintBasis::Exact)
    }
}

/// R1550 — an arena that can state what it is holding.
///
/// The row is built by the arena itself rather than at the census's assembly
/// point, so the arena's name, its entry count and its unmeasured values are
/// stated once, where the fields they describe live. A census that composed
/// rows from loose numbers would let a cache and its row disagree about what
/// the cache holds.
pub trait MeasuredArena: Footprint {
    /// This arena's census row, unattributed — the assembler adds the window.
    fn arena_footprint(&self) -> ArenaFootprint;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1550_arena_names_are_stable_and_distinct() {
        let names: Vec<&str> = Arena::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["paint-fragments", "text-shapes", "images"]);
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "arena names collide");
    }

    #[test]
    fn r1550_total_is_derived_from_the_rows() {
        let census = MemoryCensus {
            arenas: vec![
                ArenaFootprint::exact(Arena::PaintFragments, 4_000, 12).in_window("main"),
                ArenaFootprint::exact(Arena::Images, 1_000_000, 2).in_window("main"),
                ArenaFootprint::partial(
                    Arena::TextShapes,
                    96_000,
                    300,
                    vec![UnmeasuredValues::new("parley::Layout", 300)],
                ),
            ],
            process_rss_bytes: Some(180_000_000),
        };
        assert_eq!(census.total_bytes(), 1_100_000);
        assert!(
            !census.is_exact(),
            "one partial row makes the census partial",
        );
    }

    /// A row that names unmeasured values must name their type — the point of
    /// the field is attributability, and a count with no type is a footnote.
    #[test]
    fn r1550_partial_row_names_what_it_could_not_measure() {
        let row = ArenaFootprint::partial(
            Arena::TextShapes,
            10,
            5,
            vec![UnmeasuredValues::new("parley::Layout", 5)],
        );
        assert_eq!(row.basis(), FootprintBasis::Partial);
        assert_eq!(row.unmeasured[0].type_name, "parley::Layout");
        let exact = ArenaFootprint::exact(Arena::Images, 10, 5);
        assert!(exact.unmeasured.is_empty());
        assert_eq!(exact.basis(), FootprintBasis::Exact);
    }

    /// The basis is derived, so an arena that *can* hold foreign values but
    /// holds none right now is exactly measured — which is the state a fresh
    /// shell is in, and the one a stored flag would get wrong.
    #[test]
    fn r1550_an_empty_partial_arena_is_exactly_measured() {
        let row = ArenaFootprint::partial(
            Arena::TextShapes,
            0,
            0,
            vec![UnmeasuredValues::new("parley::Layout", 0)],
        );
        assert_eq!(row.basis(), FootprintBasis::Exact);
    }

    #[test]
    fn r1550_an_empty_census_is_a_valid_answer() {
        let census = MemoryCensus::default();
        assert_eq!(census.total_bytes(), 0);
        assert!(census.is_exact(), "no rows are all exact, vacuously");
        assert_eq!(census.process_rss_bytes, None);
    }
}
