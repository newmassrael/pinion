//! The consumer-side contract for a Mnemosyne narrative report.
//!
//! Mnemosyne is the deterministic **write-side** SSOT of a story
//! (scenes / facts / quests / world-lines / disclosure plans); pinion is
//! the **read-side** projection (CQRS). A report such as
//! `report-playable-world --telling <t> --json` is the wire between them:
//! a deterministic snapshot pinion deserializes and projects into a
//! queryable scene, never mutates.
//!
//! ## Tolerance is deliberate
//!
//! The upstream report schema is **not frozen** — Mnemosyne adds fields
//! per round. So every field here is `#[serde(default)]` and NO
//! `deny_unknown_fields` is set: an older pinion reads a newer report by
//! ignoring the fields it does not know, and a field the producer has not
//! emitted yet deserializes to its default rather than failing the whole
//! load. This is the forward/backward-compatible posture a read-side
//! consumer of an evolving upstream must take.
//!
//! ## What is authoritative vs. assumed
//!
//! The [`ForkTree`] shape mirrors the report's documented JSON exactly
//! (branch id / description / fork parent+at+placed). The per-world-line
//! **scene walk** ([`WorldLine`] / [`SceneNode`] / [`Disclosure`]) is the
//! pinion-side *assumed* shape: the report renders it human-readable, so
//! this is the structured form pinion consumes, to be reconciled field-by
//! field against the live `--json` when the producer freezes it. Because
//! deserialize is tolerant, a mismatch degrades to defaults rather than a
//! hard failure.

use serde::{Deserialize, Serialize};

/// A deterministic snapshot of one telling, as consumed by pinion.
///
/// The root of a `report-playable-world` projection: the fork topology
/// plus each world-line's ordered scene walk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlayableWorld {
    /// The telling this report was rendered for (e.g. `"reader"`).
    #[serde(default)]
    pub telling: String,
    /// Branch topology across world-lines.
    #[serde(default)]
    pub fork_tree: ForkTree,
    /// One entry per world-line, each carrying its ordered scene walk.
    /// The first entry is conventionally the trunk (`"main"`).
    #[serde(default)]
    pub worlds: Vec<WorldLine>,
}

impl PlayableWorld {
    /// Number of world-lines in this report.
    #[must_use]
    pub fn world_count(&self) -> usize {
        self.worlds.len()
    }

    /// The world-line at `index`, if present.
    #[must_use]
    pub fn world(&self, index: usize) -> Option<&WorldLine> {
        self.worlds.get(index)
    }

    /// Index of the world-line whose `branch_id` equals `id`, if any.
    #[must_use]
    pub fn world_index_of(&self, id: &str) -> Option<usize> {
        self.worlds.iter().position(|w| w.branch_id == id)
    }

    /// The scene at `scene` within world-line `world`, if both are in range.
    #[must_use]
    pub fn scene(&self, world: usize, scene: usize) -> Option<&SceneNode> {
        self.worlds.get(world).and_then(|w| w.scenes.get(scene))
    }

    /// `true` when the report carries no world-lines at all — the view /
    /// navigation degrade to an empty-but-valid state rather than panicking.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }
}

/// Branch topology: how world-lines fork off one another.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ForkTree {
    /// Every declared branch (the trunk is implicit; branches fork from it).
    #[serde(default)]
    pub branches: Vec<Branch>,
    /// Fork points the author declared but has not yet placed at a scene.
    #[serde(default)]
    pub unplaced_fork_points: Vec<String>,
    /// The producer's own branch count (may exceed `branches.len()` if the
    /// report elides some — kept verbatim, not recomputed).
    #[serde(default)]
    pub branch_count: u32,
}

/// One declared world-line branch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Branch {
    /// Stable branch identifier (e.g. `"ending-flee"`).
    #[serde(default)]
    pub branch_id: String,
    /// Human-readable gloss of what this branch is.
    #[serde(default)]
    pub description: String,
    /// Where this branch forks from its parent, if placed.
    #[serde(default)]
    pub fork: Option<Fork>,
}

/// The fork point of a [`Branch`] — which parent world-line it diverges
/// from and at which scene.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Fork {
    /// The parent world-line's branch id (often `"main"`).
    #[serde(default)]
    pub parent: String,
    /// The scene id at which the fork occurs.
    #[serde(default)]
    pub at: String,
    /// Whether the fork has been placed at a concrete scene (`false` = the
    /// author declared the divergence but has not anchored it yet).
    #[serde(default)]
    pub at_placed: bool,
}

/// One world-line and its ordered scene walk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorldLine {
    /// This world-line's branch id (`"main"` for the trunk).
    #[serde(default)]
    pub branch_id: String,
    /// The scenes to walk, in author order.
    #[serde(default)]
    pub scenes: Vec<SceneNode>,
}

/// One scene in a world-line's walk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SceneNode {
    /// The scene's ordinal within its world-line.
    #[serde(default)]
    pub idx: u32,
    /// Short scene title.
    #[serde(default)]
    pub title: String,
    /// The scene's narrative intent (why it exists in the telling).
    #[serde(default)]
    pub intent: String,
    /// Facts this scene plants or reveals — the disclosure pointers an AI
    /// runner reads to decide what the player may now know.
    #[serde(default)]
    pub disclosures: Vec<Disclosure>,
}

/// One disclosure pointer attached to a scene.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Disclosure {
    /// Disclosure mode (e.g. `"plant"`, `"reveal"`).
    #[serde(default)]
    pub mode: String,
    /// The fact being disclosed.
    #[serde(default)]
    pub fact: String,
    /// The scene id at which this fact is first disclosed.
    #[serde(default)]
    pub first_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_are_bounds_safe_on_empty() {
        let world = PlayableWorld::default();
        assert!(world.is_empty());
        assert_eq!(world.world_count(), 0);
        assert!(world.world(0).is_none());
        assert!(world.scene(0, 0).is_none());
        assert!(world.world_index_of("main").is_none());
    }

    #[test]
    fn world_index_of_finds_branch() {
        let world = PlayableWorld {
            worlds: vec![
                WorldLine {
                    branch_id: "main".to_string(),
                    scenes: vec![],
                },
                WorldLine {
                    branch_id: "ending-flee".to_string(),
                    scenes: vec![],
                },
            ],
            ..PlayableWorld::default()
        };
        assert_eq!(world.world_index_of("main"), Some(0));
        assert_eq!(world.world_index_of("ending-flee"), Some(1));
        assert_eq!(world.world_index_of("nope"), None);
    }
}
