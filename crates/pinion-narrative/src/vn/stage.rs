//! The visual stage: a reactive background + sprite director.
//!
//! A VN set-piece is not just dialogue — it is a *staged* image: a
//! background, and named character sprites shown at positions with a z-order,
//! their expressions swapped as the scene plays (the-tide Ren'Py-parity
//! `claudedocs/the-tide-vn-renpy-parity-requirements.md` §3 "스프라이트
//! 디렉터"). This module is that director.
//!
//! **It honours the §2 invariants the same way the dialogue does.** The stage
//! is held as structured *data* — a background asset *reference* and a list of
//! sprites (id / source reference / position / layer), never pixels — so it
//! projects into `Scene::Image` nodes (§2 #1, no opaque paint) whose `source`
//! is an asset key the backend resolves, and it is fully queryable as data
//! (§2 #7: an AI reads "무녀 is shown at centre on layer 1", not a bitmap).
//!
//! The director is **imperative** (`show` / `hide` / `move` /
//! `set_background`, mutating a persistent stage) rather than derived per
//! dialogue step — that stays correct across the [`crate::vn`] branching
//! (the stage is independent state, not a function of the linear step index),
//! and it is captured in the save state so a load restores exactly what was
//! on screen.

use pinion_core::reactive::Signal;
use serde::{Deserialize, Serialize};

/// The standard VN stage positions — Ren'Py's `left` / `center` / `right`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpritePos {
    /// Stage left.
    Left,
    /// Stage centre (the default).
    #[default]
    Center,
    /// Stage right.
    Right,
}

impl SpritePos {
    /// The stable wire string for this position.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    /// Parse a wire string into a position, `None` if unrecognised.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// The sprite centre's x in pixels for a stage `width` — the deterministic
    /// layout position the [`crate::vn::view`] projection reads (0.2 / 0.5 /
    /// 0.8 of the width, in integer math so there is no float cast).
    #[must_use]
    pub const fn center_x(self, width: u32) -> u32 {
        match self {
            Self::Left => width / 5,
            Self::Center => width / 2,
            Self::Right => width * 4 / 5,
        }
    }
}

/// One sprite on the stage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnSprite {
    /// Stable handle used to move / hide / re-express this sprite
    /// (e.g. `"mudang"`). Unique on the stage.
    #[serde(default)]
    pub id: String,
    /// The asset reference painted for this sprite — swapping it is an
    /// expression change (e.g. `"mudang_calm"` -> `"mudang_grave"`). A
    /// reference, never pixels.
    #[serde(default)]
    pub source: String,
    /// Where on the stage the sprite stands.
    #[serde(default)]
    pub at: SpritePos,
    /// Paint order: higher layers paint in front of lower ones.
    #[serde(default)]
    pub layer: u8,
}

impl VnSprite {
    /// Build a sprite handle `id` painting `source` at `at` on `layer`.
    #[must_use]
    pub fn new(id: impl Into<String>, source: impl Into<String>, at: SpritePos, layer: u8) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            at,
            layer,
        }
    }
}

/// The complete visual stage — the save-able, queryable model. Sprites are
/// kept sorted by `(layer, id)` so the projection paint order and the wire
/// listing are deterministic.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct StageData {
    /// The background asset reference, if any (a full-stage image).
    #[serde(default)]
    pub background: Option<String>,
    /// The sprites currently on stage, sorted back-to-front by `(layer, id)`.
    #[serde(default)]
    pub sprites: Vec<VnSprite>,
}

impl StageData {
    /// Re-establish the `(layer, id)` sort invariant after a mutation.
    fn sort(&mut self) {
        self.sprites
            .sort_by(|a, b| a.layer.cmp(&b.layer).then_with(|| a.id.cmp(&b.id)));
    }
}

/// The reactive stage director. One `Signal<StageData>`; the view reads it,
/// the [`VnExternal`](crate::vn::VnExternal) director verbs mutate it — the
/// same reactive-holder pattern the dialogue runner uses.
#[derive(Debug)]
pub struct VnStage {
    data: Signal<StageData>,
}

impl Default for VnStage {
    fn default() -> Self {
        Self::new()
    }
}

impl VnStage {
    /// A fresh, empty stage (no background, no sprites).
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Signal::new(StageData::default()),
        }
    }

    /// The current stage snapshot.
    #[must_use]
    pub fn data(&self) -> StageData {
        self.data.get()
    }

    /// Replace the whole stage (the load half of save/load).
    pub fn set_data(&self, data: StageData) {
        let mut data = data;
        data.sort();
        self.data.set(data);
    }

    /// The current background reference, if any.
    #[must_use]
    pub fn background(&self) -> Option<String> {
        self.data.get().background
    }

    /// Set the background to `source`.
    pub fn set_background(&self, source: impl Into<String>) {
        let mut d = self.data.get();
        d.background = Some(source.into());
        self.data.set(d);
    }

    /// Clear the background. Returns whether there was one to clear.
    #[must_use]
    pub fn clear_background(&self) -> bool {
        let mut d = self.data.get();
        if d.background.take().is_some() {
            self.data.set(d);
            true
        } else {
            false
        }
    }

    /// The sprites currently on stage (back-to-front order).
    #[must_use]
    pub fn sprites(&self) -> Vec<VnSprite> {
        self.data.get().sprites
    }

    /// Number of sprites on stage.
    #[must_use]
    pub fn sprite_count(&self) -> usize {
        self.data.get().sprites.len()
    }

    /// Show `sprite`, replacing any existing sprite with the same `id`
    /// (idempotent by id — a re-show updates source / position / layer).
    pub fn show(&self, sprite: VnSprite) {
        let mut d = self.data.get();
        if let Some(existing) = d.sprites.iter_mut().find(|s| s.id == sprite.id) {
            *existing = sprite;
        } else {
            d.sprites.push(sprite);
        }
        d.sort();
        self.data.set(d);
    }

    /// Hide the sprite handle `id`. Returns whether it was on stage.
    #[must_use]
    pub fn hide(&self, id: &str) -> bool {
        let mut d = self.data.get();
        let before = d.sprites.len();
        d.sprites.retain(|s| s.id != id);
        let removed = d.sprites.len() != before;
        if removed {
            self.data.set(d);
        }
        removed
    }

    /// Move the sprite `id` to position `at`. Returns whether it was found.
    #[must_use]
    pub fn move_to(&self, id: &str, at: SpritePos) -> bool {
        self.mutate_sprite(id, |s| s.at = at)
    }

    /// Swap the sprite `id`'s source (an expression change). Returns whether
    /// it was found.
    #[must_use]
    pub fn set_source(&self, id: &str, source: impl Into<String>) -> bool {
        let source = source.into();
        self.mutate_sprite(id, |s| s.source = source)
    }

    /// Apply `f` to the sprite `id` if present, re-sorting after. Returns
    /// whether it was found.
    fn mutate_sprite(&self, id: &str, f: impl FnOnce(&mut VnSprite)) -> bool {
        let mut d = self.data.get();
        let Some(sprite) = d.sprites.iter_mut().find(|s| s.id == id) else {
            return false;
        };
        f(sprite);
        d.sort();
        self.data.set(d);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    fn with_stage<R>(body: impl FnOnce(&VnStage) -> R) -> R {
        let owner = Owner::new();
        owner.run(|| body(&VnStage::new()))
    }

    #[test]
    fn background_set_and_clear() {
        with_stage(|s| {
            assert_eq!(s.background(), None);
            s.set_background("tideflat");
            assert_eq!(s.background().as_deref(), Some("tideflat"));
            assert!(s.clear_background());
            assert_eq!(s.background(), None);
            assert!(!s.clear_background(), "nothing to clear");
        });
    }

    #[test]
    fn show_is_idempotent_by_id_and_sorts_by_layer() {
        with_stage(|s| {
            s.show(VnSprite::new("b", "b0", SpritePos::Right, 2));
            s.show(VnSprite::new("a", "a0", SpritePos::Left, 1));
            let sprites = s.sprites();
            let ids: Vec<&str> = sprites.iter().map(|x| x.id.as_str()).collect();
            assert_eq!(ids, ["a", "b"], "sorted back-to-front by layer");
            // Re-showing the same id updates in place, not a duplicate.
            s.show(VnSprite::new("a", "a1", SpritePos::Center, 5));
            assert_eq!(s.sprite_count(), 2, "no duplicate");
            let sprites = s.sprites();
            let ids: Vec<&str> = sprites.iter().map(|x| x.id.as_str()).collect();
            assert_eq!(ids, ["b", "a"], "a moved to front (layer 5)");
        });
    }

    #[test]
    fn move_hide_and_reexpress() {
        with_stage(|s| {
            s.show(VnSprite::new("m", "calm", SpritePos::Center, 0));
            assert!(s.move_to("m", SpritePos::Left));
            assert_eq!(s.sprites()[0].at, SpritePos::Left);
            assert!(s.set_source("m", "grave"));
            assert_eq!(s.sprites()[0].source, "grave");
            assert!(!s.move_to("missing", SpritePos::Right), "unknown id");
            assert!(s.hide("m"));
            assert_eq!(s.sprite_count(), 0);
            assert!(!s.hide("m"), "already hidden");
        });
    }

    #[test]
    fn set_data_restores_and_re_sorts() {
        with_stage(|s| {
            let data = StageData {
                background: Some("bg".to_string()),
                sprites: vec![
                    VnSprite::new("z", "z", SpritePos::Right, 9),
                    VnSprite::new("a", "a", SpritePos::Left, 0),
                ],
            };
            s.set_data(data);
            assert_eq!(s.background().as_deref(), Some("bg"));
            let sprites = s.sprites();
            let ids: Vec<&str> = sprites.iter().map(|x| x.id.as_str()).collect();
            assert_eq!(ids, ["a", "z"], "restored data is re-sorted");
        });
    }

    #[test]
    fn position_center_x_is_ordered_and_parses() {
        assert!(SpritePos::Left.center_x(800) < SpritePos::Center.center_x(800));
        assert!(SpritePos::Center.center_x(800) < SpritePos::Right.center_x(800));
        assert_eq!(SpritePos::parse("left"), Some(SpritePos::Left));
        assert_eq!(SpritePos::parse("nope"), None);
    }
}
