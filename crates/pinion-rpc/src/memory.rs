//! `scene/memory` RPC method dispatch — R1550 §5.16 §5.36 §5.7 §2 #2 §2 #7.
//!
//! Publishes [`pinion_core::memory_census::MemoryCensus`] — every arena this
//! process holds, what each is holding in bytes, and what the OS says the
//! process is resident for.
//!
//! # Why a method of its own
//!
//! `scene/cache_stats` and `scene/text_cache_stats` each already answer for
//! one cache, and `scene/text_cache_stats`'s own module doc states the rule
//! this follows: **one method per axis**. Memory is an axis neither of them
//! covers — both answer in *entries*, and entries are not the resource. The
//! §5.36 shape cache holds one entry for `"OK"` and one for a 10,000-character
//! paragraph and reports `2`; the image arena holds a 16x16 icon and a 4K
//! screenshot and reports `2`.
//!
//! It is also an axis that costs a **walk**: pricing the shape cache means
//! visiting every cached draw list. `stats()` is called on every dispatch
//! unconditionally, so putting bytes there would bill every `scene/click` for
//! a measurement nobody asked for. A separate method is billed only when
//! asked.
//!
//! # Wire form
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/memory",
//!   "id": 1
//! }
//! ```
//!
//! answers with
//!
//! ```json
//! {
//!   "arenas": [
//!     {"arena": "paint-fragments", "window": "main", "bytes": 141312,
//!      "entries": 36, "basis": "exact", "unmeasured": [], "budget_bytes": null},
//!     {"arena": "images", "window": "main", "bytes": 0, "entries": 0,
//!      "basis": "exact", "unmeasured": [], "budget_bytes": 10485760},
//!     {"arena": "text-shapes", "window": null, "bytes": 208992, "entries": 512,
//!      "basis": "partial",
//!      "unmeasured": [{"type": "parley::Layout", "count": 512}],
//!      "budget_bytes": null}
//!   ],
//!   "total_bytes": 350304,
//!   "process_rss_bytes": 187392000
//! }
//! ```
//!
//! # Reading it
//!
//! - `basis` says whether `bytes` is the whole of what that arena holds.
//!   `partial` is not an estimate — the measured bytes are exact — it means
//!   the arena also holds the listed foreign values, whose interiors are
//!   behind a private field in the crate that owns them.
//! - `budget_bytes` is the ceiling the arena evicts toward, or `null` for an
//!   arena bounded by something else (the shape cache bounds entries) or not
//!   at all.
//! - `total_bytes` is the sum of the rows, and will always be well below
//!   `process_rss_bytes`: the widget tree, taffy's nodes, the font collection
//!   and the GPU driver's buffers are outside every arena here.

use pinion_core::memory_census::MemoryCensus;
use serde::Serialize;

/// Typed errors the [`memory`] dispatcher can return. The variant name rides
/// in `error.data` so an agent pattern-matches rather than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    /// The embedder installed no census on the dispatch context.
    ///
    /// Production embedders build one on every `scene/memory` dispatch. The
    /// variant exists for a host that owns no arenas at all.
    MemoryCensusUnavailable,
}

/// One arena's row on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryArena {
    /// Which arena — `"paint-fragments"`, `"text-shapes"` or `"images"`.
    pub arena: &'static str,
    /// The window this arena belongs to; `null` for a shell-wide arena.
    pub window: Option<String>,
    /// Heap bytes the arena is holding.
    pub bytes: u64,
    /// Live entries, so bytes-per-entry is one division.
    pub entries: u64,
    /// `"exact"` or `"partial"` — see [`Self::unmeasured`].
    pub basis: &'static str,
    /// What `bytes` leaves out, by type. Empty when `basis` is `"exact"`.
    pub unmeasured: Vec<MemoryUnmeasured>,
    /// The byte budget this arena evicts toward, or `null`.
    pub budget_bytes: Option<u64>,
}

/// Values an arena holds whose interiors cannot be sized from outside the
/// crate that owns them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryUnmeasured {
    /// The type's path, as a reader would search for it.
    #[serde(rename = "type")]
    pub type_name: &'static str,
    /// How many the arena holds.
    pub count: u64,
}

/// Snapshot returned by [`memory`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryOutcome {
    /// One row per arena per owner — a census, so an arena holding nothing
    /// still appears.
    pub arenas: Vec<MemoryArena>,
    /// Sum of every row's `bytes`. Derived from the rows rather than stored,
    /// so it cannot disagree with them.
    pub total_bytes: u64,
    /// What the OS says this process is resident for, or `null` where the
    /// platform has no reader (everything but Linux today).
    pub process_rss_bytes: Option<u64>,
}

/// Project a [`MemoryCensus`] onto the wire-shaped [`MemoryOutcome`].
///
/// # Errors
///
/// - [`MemoryError::MemoryCensusUnavailable`] — the embedder registered no
///   census on the dispatch context.
pub fn memory(census: Option<&MemoryCensus>) -> Result<MemoryOutcome, MemoryError> {
    let Some(census) = census else {
        return Err(MemoryError::MemoryCensusUnavailable);
    };
    Ok(MemoryOutcome {
        arenas: census
            .arenas
            .iter()
            .map(|a| MemoryArena {
                arena: a.arena.name(),
                window: a.window.clone(),
                bytes: a.bytes,
                entries: a.entries,
                basis: a.basis().name(),
                unmeasured: a
                    .unmeasured
                    .iter()
                    .filter(|u| u.count > 0)
                    .map(|u| MemoryUnmeasured {
                        type_name: u.type_name,
                        count: u.count,
                    })
                    .collect(),
                budget_bytes: a.budget_bytes,
            })
            .collect(),
        total_bytes: census.total_bytes(),
        process_rss_bytes: census.process_rss_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::memory_census::{Arena, ArenaFootprint, UnmeasuredValues};

    fn census() -> MemoryCensus {
        MemoryCensus {
            arenas: vec![
                ArenaFootprint::exact(Arena::PaintFragments, 141_312, 36).in_window("main"),
                ArenaFootprint::exact(Arena::Images, 0, 0)
                    .in_window("main")
                    .with_budget(10_485_760),
                ArenaFootprint::partial(
                    Arena::TextShapes,
                    208_992,
                    512,
                    vec![
                        UnmeasuredValues::new("parley::Layout", 512),
                        UnmeasuredValues::new("parley::FontContext", 0),
                    ],
                ),
            ],
            process_rss_bytes: Some(187_392_000),
        }
    }

    #[test]
    fn r1550_missing_census_errors() {
        assert_eq!(
            memory(None).unwrap_err(),
            MemoryError::MemoryCensusUnavailable
        );
    }

    #[test]
    fn r1550_rows_carry_their_owner_and_basis() {
        let out = memory(Some(&census())).unwrap();
        assert_eq!(out.arenas.len(), 3);
        assert_eq!(out.arenas[0].arena, "paint-fragments");
        assert_eq!(out.arenas[0].window.as_deref(), Some("main"));
        assert_eq!(out.arenas[0].basis, "exact");
        assert_eq!(out.arenas[1].budget_bytes, Some(10_485_760));
        assert_eq!(out.arenas[2].window, None, "shell-wide arena has no window");
        assert_eq!(out.arenas[2].basis, "partial");
    }

    /// A zero-count unmeasured entry is dropped rather than published: an
    /// arena that *could* hold a `FontContext` but has not built one holds
    /// nothing unmeasured, and the row says so by being exact.
    #[test]
    fn r1550_unmeasured_lists_only_what_is_held() {
        let out = memory(Some(&census())).unwrap();
        let text = &out.arenas[2];
        assert_eq!(text.unmeasured.len(), 1);
        assert_eq!(text.unmeasured[0].type_name, "parley::Layout");
        assert_eq!(text.unmeasured[0].count, 512);
    }

    #[test]
    fn r1550_total_is_the_sum_of_the_rows() {
        let out = memory(Some(&census())).unwrap();
        let summed: u64 = out.arenas.iter().map(|a| a.bytes).sum();
        assert_eq!(out.total_bytes, summed);
        assert_eq!(out.total_bytes, 350_304);
    }

    /// The unmeasured type rides as `"type"`, not `"type_name"` — the wire
    /// name is what an agent matches on and is not the Rust identifier.
    #[test]
    fn r1550_wire_names_the_unmeasured_type_as_type() {
        let out = memory(Some(&census())).unwrap();
        let json = serde_json::to_string(&out.arenas[2]).unwrap();
        assert!(json.contains(r#""type":"parley::Layout""#), "{json}");
    }
}
