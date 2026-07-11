//! The reactive read-model: a cursor over the loaded [`PlayableWorld`].
//!
//! One `Rc<NarrativeState>` is the single source of truth for "where in the
//! story are we". The immutable report is held behind an `Rc`; the mutable
//! part is a single reactive [`Signal`] cursor `(world, scene)`. The view
//! reads it, the [`NarrativeExternal`](crate::NarrativeExternal) mutates it
//! — the same reactive-holder / one-Rc-SSOT pattern the file-browser's
//! `DirectoryState` uses. Navigation is pure: every transition clamps into
//! range, so an out-of-range request is a no-op, never a panic.

use std::rc::Rc;

use pinion_core::reactive::{Owner, Signal};
use serde::{Deserialize, Serialize};

use crate::model::{PlayableWorld, SceneNode, WorldLine};

/// A position in the story: which world-line, which scene within it.
///
/// `Copy` so it can be a `WidgetCore::State` (the shell snapshots it for
/// change detection); serde-derived so it can live inside a [`Signal`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct NarrativeCursor {
    /// Index into [`PlayableWorld::worlds`].
    pub world: u16,
    /// Index into the current world-line's `scenes`.
    pub scene: u16,
}

/// The reactive narrative read-model.
#[derive(Debug)]
pub struct NarrativeState {
    world: Rc<PlayableWorld>,
    cursor: Signal<NarrativeCursor>,
}

impl NarrativeState {
    /// Build a fresh read-model over `world`, positioned at the first scene
    /// of the first world-line.
    #[must_use]
    pub fn new(world: PlayableWorld) -> Self {
        Self {
            world: Rc::new(world),
            cursor: Signal::new(NarrativeCursor::default()),
        }
    }

    /// The immutable report backing this read-model.
    #[must_use]
    pub fn world(&self) -> &PlayableWorld {
        &self.world
    }

    /// The current cursor `(world, scene)`.
    #[must_use]
    pub fn cursor(&self) -> NarrativeCursor {
        self.cursor.get()
    }

    /// Number of world-lines.
    #[must_use]
    pub fn world_count(&self) -> usize {
        self.world.world_count()
    }

    /// Number of scenes in the currently selected world-line.
    #[must_use]
    pub fn scene_count(&self) -> usize {
        self.scene_count_for(usize::from(self.cursor().world))
    }

    fn scene_count_for(&self, world: usize) -> usize {
        self.world.world(world).map_or(0, |w| w.scenes.len())
    }

    /// The currently selected world-line, if the cursor is in range.
    #[must_use]
    pub fn current_world_line(&self) -> Option<&WorldLine> {
        self.world.world(usize::from(self.cursor().world))
    }

    /// The currently selected scene, if the cursor is in range.
    #[must_use]
    pub fn current_scene(&self) -> Option<&SceneNode> {
        let c = self.cursor();
        self.world.scene(usize::from(c.world), usize::from(c.scene))
    }

    /// Advance to the next scene in the current world-line. Returns whether
    /// the cursor moved (`false` at the last scene).
    #[must_use]
    pub fn next_scene(&self) -> bool {
        let c = self.cursor();
        if usize::from(c.scene) + 1 < self.scene_count() {
            self.cursor.set(NarrativeCursor {
                scene: c.scene.saturating_add(1),
                ..c
            });
            true
        } else {
            false
        }
    }

    /// Step back to the previous scene. Returns whether the cursor moved
    /// (`false` at the first scene).
    #[must_use]
    pub fn prev_scene(&self) -> bool {
        let c = self.cursor();
        if c.scene > 0 {
            self.cursor.set(NarrativeCursor {
                scene: c.scene - 1,
                ..c
            });
            true
        } else {
            false
        }
    }

    /// Switch to the next world-line, resetting to its first scene. Returns
    /// whether the cursor moved.
    #[must_use]
    pub fn next_world(&self) -> bool {
        let c = self.cursor();
        if usize::from(c.world) + 1 < self.world_count() {
            self.cursor.set(NarrativeCursor {
                world: c.world.saturating_add(1),
                scene: 0,
            });
            true
        } else {
            false
        }
    }

    /// Switch to the previous world-line, resetting to its first scene.
    /// Returns whether the cursor moved.
    #[must_use]
    pub fn prev_world(&self) -> bool {
        let c = self.cursor();
        if c.world > 0 {
            self.cursor.set(NarrativeCursor {
                world: c.world - 1,
                scene: 0,
            });
            true
        } else {
            false
        }
    }

    /// Jump the cursor to `(world, scene)`, clamped into range. A `world`
    /// past the last world-line lands on the last; a `scene` past the end
    /// of its world-line lands on that world-line's last scene. Returns
    /// whether the cursor moved.
    #[must_use]
    pub fn goto(&self, world: u16, scene: u16) -> bool {
        let world_count = self.world_count();
        if world_count == 0 {
            return false;
        }
        let max_world = clamp_index(world_count);
        let world = world.min(max_world);
        let scene_count = self.scene_count_for(usize::from(world));
        let scene = if scene_count == 0 {
            0
        } else {
            scene.min(clamp_index(scene_count))
        };
        let next = NarrativeCursor { world, scene };
        if next == self.cursor() {
            return false;
        }
        self.cursor.set(next);
        true
    }

    /// Jump to the first scene of the world-line named `branch_id`. Returns
    /// whether a matching world-line was found and the cursor moved.
    #[must_use]
    pub fn goto_world_id(&self, branch_id: &str) -> bool {
        match self.world.world_index_of(branch_id) {
            Some(index) => self.goto(u16::try_from(index).unwrap_or(u16::MAX), 0),
            None => false,
        }
    }
}

/// The last valid index for a collection of `count` (`count >= 1`)
/// elements, saturating into `u16`.
fn clamp_index(count: usize) -> u16 {
    u16::try_from(count.saturating_sub(1)).unwrap_or(u16::MAX)
}

/// Retrieve (building once) the `Rc<NarrativeState>` for `key` from the
/// current [`Owner`] scope — the one-Rc SSOT shared by the view and the
/// External. `load` produces the report the first time only.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope (i.e. not from a
/// `view` / `create_external` call driven by the shell).
#[must_use]
pub fn use_narrative_state(
    key: &'static str,
    load: impl FnOnce() -> PlayableWorld,
) -> Rc<NarrativeState> {
    Owner::current()
        .expect("use_narrative_state must be called within an active Owner scope")
        .cache(key, || NarrativeState::new(load()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SceneNode;
    use pinion_core::reactive::Owner;

    fn world_with(scene_counts: &[usize]) -> PlayableWorld {
        PlayableWorld {
            worlds: scene_counts
                .iter()
                .enumerate()
                .map(|(w, &n)| WorldLine {
                    branch_id: format!("w{w}"),
                    scenes: (0..n)
                        .map(|s| SceneNode {
                            idx: u32::try_from(s).unwrap_or(0),
                            title: format!("s{s}"),
                            ..SceneNode::default()
                        })
                        .collect(),
                })
                .collect(),
            ..PlayableWorld::default()
        }
    }

    fn with_state<R>(world: PlayableWorld, body: impl FnOnce(&NarrativeState) -> R) -> R {
        // A Signal must be constructed within an Owner scope.
        let owner = Owner::new();
        owner.run(|| {
            let state = NarrativeState::new(world);
            body(&state)
        })
    }

    #[test]
    fn scene_navigation_clamps_at_ends() {
        with_state(world_with(&[3]), |state| {
            assert_eq!(state.cursor(), NarrativeCursor { world: 0, scene: 0 });
            assert!(!state.prev_scene(), "already at first");
            assert!(state.next_scene());
            assert!(state.next_scene());
            assert_eq!(state.cursor().scene, 2);
            assert!(!state.next_scene(), "already at last");
            assert_eq!(state.cursor().scene, 2);
        });
    }

    #[test]
    fn world_switch_resets_scene() {
        with_state(world_with(&[3, 2]), |state| {
            assert!(state.next_scene());
            assert_eq!(state.cursor().scene, 1);
            assert!(state.next_world());
            assert_eq!(state.cursor(), NarrativeCursor { world: 1, scene: 0 });
            assert!(!state.next_world(), "already at last world");
        });
    }

    #[test]
    fn goto_clamps_out_of_range() {
        with_state(world_with(&[3, 2]), |state| {
            assert!(state.goto(9, 9));
            assert_eq!(state.cursor(), NarrativeCursor { world: 1, scene: 1 });
        });
    }

    #[test]
    fn goto_world_id_finds_branch() {
        with_state(world_with(&[1, 1, 1]), |state| {
            assert!(state.goto_world_id("w2"));
            assert_eq!(state.cursor().world, 2);
            assert!(!state.goto_world_id("missing"));
        });
    }

    #[test]
    fn empty_world_is_inert() {
        with_state(PlayableWorld::default(), |state| {
            assert!(!state.next_scene());
            assert!(!state.next_world());
            assert!(!state.goto(0, 0));
            assert!(state.current_scene().is_none());
        });
    }
}
