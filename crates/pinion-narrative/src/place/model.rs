//! The relation place-graph: the consumer-side contract for a Mnemosyne
//! spatial report.
//!
//! The story's world is authored as **relations, not coordinates**
//! (containment / adjacency / direction). Per the pipeline principle
//! "관계=SSOT / 좌표=휘발" the coordinates are not authored at all — the
//! renderer solves them ([`super::layout`]). This module is only the
//! relation data pinion consumes; where the narrative report gives the
//! scene walk, this spatial report gives the map.
//!
//! As with [`crate::model`], deserialize is tolerant (every field
//! `#[serde(default)]`, no `deny_unknown_fields`) because the upstream
//! spatial report is a Mnemosyne debt (R586) whose schema will evolve.

use serde::{Deserialize, Serialize};

/// A cardinal direction relation between two places.
///
/// `North` is toward the top of the map (decreasing `y`), matching screen
/// coordinates — a place `North` of another solves to a smaller `y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    /// The unit lattice step for this direction: `(dx, dy)` with `North`
    /// stepping to a smaller `y`.
    #[must_use]
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Self::North => (0, -1),
            Self::South => (0, 1),
            Self::East => (1, 0),
            Self::West => (-1, 0),
        }
    }

    /// The opposite direction — used to read an edge from the other
    /// endpoint's perspective (`B` is `East` of `A` ⇒ `A` is `West` of `B`).
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
        }
    }
}

/// One place (node) in the world map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Place {
    /// Stable place identifier (referenced by adjacencies / containment).
    #[serde(default)]
    pub id: String,
    /// Human-readable label rendered on the map.
    #[serde(default)]
    pub label: String,
    /// The `id` of the place that spatially contains this one, if any
    /// (e.g. a shrine inside a village). `None` = a top-level place.
    #[serde(default)]
    pub contained_by: Option<String>,
}

/// One adjacency (edge) between two places — you can move `from` ⇄ `to`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Adjacency {
    /// The source place `id`.
    #[serde(default)]
    pub from: String,
    /// The destination place `id`.
    #[serde(default)]
    pub to: String,
    /// The direction of `to` relative to `from`, if the author pinned one.
    /// `None` = adjacency with no compass constraint (placed near, any side).
    #[serde(default)]
    pub direction: Option<Direction>,
}

/// The relation place-graph: places plus the adjacencies between them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlaceGraph {
    /// Every place in the map, in author order (the deterministic tiebreak
    /// the solver walks).
    #[serde(default)]
    pub places: Vec<Place>,
    /// Every adjacency between places.
    #[serde(default)]
    pub adjacencies: Vec<Adjacency>,
}

impl PlaceGraph {
    /// The index of the place with `id`, if present.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.places.iter().position(|p| p.id == id)
    }

    /// Number of places.
    #[must_use]
    pub fn place_count(&self) -> usize {
        self.places.len()
    }

    /// `true` when the graph has no places.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_offset_and_opposite_are_consistent() {
        for d in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let (dx, dy) = d.offset();
            let (ox, oy) = d.opposite().offset();
            assert_eq!((dx + ox, dy + oy), (0, 0), "opposite cancels {d:?}");
        }
        assert_eq!(Direction::North.offset(), (0, -1), "north is up");
    }

    #[test]
    fn direction_deserializes_lowercase() {
        let adj: Adjacency =
            serde_json::from_str(r#"{ "from": "a", "to": "b", "direction": "east" }"#).unwrap();
        assert_eq!(adj.direction, Some(Direction::East));
    }

    #[test]
    fn index_of_resolves_places() {
        let graph = PlaceGraph {
            places: vec![
                Place {
                    id: "village".to_string(),
                    ..Place::default()
                },
                Place {
                    id: "shrine".to_string(),
                    ..Place::default()
                },
            ],
            adjacencies: vec![],
        };
        assert_eq!(graph.index_of("shrine"), Some(1));
        assert!(graph.index_of("nope").is_none());
    }
}
