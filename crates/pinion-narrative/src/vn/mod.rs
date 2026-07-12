//! The **VN render axis** — a visual-novel presentation layer over the
//! narrative substrate.
//!
//! Where [`crate::model`] / [`crate::state`] / [`crate::view`] are the story
//! *skeleton* (a discrete scene walk: title / intent / disclosure), this
//! module is the VN *presentation* of one set-piece: authored dialogue
//! revealed by a typewriter, and timed choices the player answers before a
//! countdown expires — the-tide's "급박함" (urgency) core
//! (`claudedocs/the-tide-vn-renpy-parity-requirements.md` §7).
//!
//! It is additive, in the crate's standing posture: the renderer grows, the
//! data pipeline does not. The runner is deliberately a retained
//! structured-scene surface — the dialogue is queryable text (§2 #1 / #7),
//! not opaque paint — driven deterministically by a `tick` step-verb so it
//! proves end-to-end over the JSON-RPC wire with zero flakiness, exactly as
//! the audio arc's `render` step-verb did (R1293).
//!
//! ## Pieces
//!
//! - [`model`] — the authored [`VnScript`] contract ([`VnStep`] /
//!   [`VnOption`]), `serde`-derived and tolerant.
//! - [`state`] — [`VnState`], the reactive deterministic play-head, the
//!   [`VnSave`] save state, and the [`use_vn_state`] one-Rc SSOT hook.
//! - [`stage`] — [`VnStage`], the imperative background + sprite director
//!   ([`VnSprite`] / [`SpritePos`] / [`StageData`]), projected into
//!   `Scene::Image` nodes.
//! - [`external`] — [`VnExternal`], the §5.15 AI-first drive surface
//!   (query / intervene / invoke `tick` / `advance` / `choose` / `save` /
//!   `load` + the stage director verbs).
//! - [`view`] — [`vn_scene`], the read-side structured-scene projection
//!   (stage images + dialogue box).
//!
//! ## What stays out (by design)
//!
//! Transitions (dissolve / fade / shake), real-time frame-driven play (wiring
//! the runner to the shell's frame delta / the game-loop `scene/tick`), and
//! outcome→world-line branch *mapping* (connecting a chosen outcome to a
//! Mnemosyne world-line) are deferred follow-ups. Typewriter + timed-choice
//! (R1295), branching (R1296), save/load (R1297), and the sprite/background
//! director (R1298) are landed.

pub mod external;
pub mod model;
pub mod stage;
pub mod state;
pub mod view;

pub use external::{ADVANCED_INTENT, CHOSEN_INTENT, TIMEOUT_INTENT, VnExternal};
pub use model::{VnOption, VnScript, VnStep};
pub use stage::{SpritePos, StageData, VnSprite, VnStage};
pub use state::{
    ChooseError, VnCursor, VnMode, VnResolution, VnRuntime, VnSave, VnState, use_vn_state,
};
pub use view::vn_scene;
