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
//! The runner is a retained structured-scene surface — the dialogue is
//! queryable text (§2 #1 / #7), not opaque paint — driven deterministically
//! by a `tick` step-verb so it proves end-to-end over the JSON-RPC wire with
//! zero flakiness, exactly as the audio arc's `render` step-verb did (R1293).
//!
//! ## SSOT posture — an honest caveat (this is NOT the CQRS read-side yet)
//!
//! The rest of this crate ([`crate::model`] / [`crate::state`]) is the CQRS
//! **read-side** of Mnemosyne: it projects a `report-playable-world` report
//! and never authors story truth. **The VN runner does not follow that
//! contract yet.** [`VnScript`] / [`VnStep`] / [`VnOption`] are a *separate,
//! hand-authored* structure — the report carries scene title / intent /
//! disclosure but **no dialogue lines, speakers, or timed choices**, so the
//! VN's narrative truth currently has no write-side home and is authored in
//! the example (`hello-vn-tide`'s `tide_script`) as a stand-in. This is a
//! **parallel SSOT**, not the additive same-pipeline projection the
//! requirements doc §6 envisions. Making it textbook needs the write-side
//! (Mnemosyne, a different repo) to grow VN shape plus a `report → VnScript`
//! projection here, and [`VnOption::goto`] reconciled with the report's
//! [`ForkTree`](crate::model::ForkTree). Until then the hand-authored script
//! is a deliberate, explicitly-tracked stand-in, not the intended authoring
//! path.
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
//! ## What stays out (honestly scoped)
//!
//! Landed: typewriter + timed-choice (R1295), branching (R1296), save/load
//! (R1297), and the sprite/background director *data model* (R1298).
//!
//! Deferred follow-ups, with their real reasons (no false boundaries):
//!
//! - **Live real-time play** (the typewriter revealing / the countdown
//!   draining on the wall clock). This is a small retained
//!   `Tickable` — the `CaretBlink` pattern,
//!   *existing* Phase-A/B substrate (`Owner::register_animation_once` +
//!   `any_animation_active` drive retained widgets per-frame). It is **not**
//!   Phase-C and does **not** need an immediate-mode node; it is left to a
//!   follow-up round only because a zero-flake wire demo must be driven by the
//!   deterministic `tick` verb, not the wall clock.
//! - **Positioned sprite *pixels*.** The director's data (which sprite, where,
//!   layered) is queryable and projects to `Scene::Image` reference nodes, but
//!   those nodes carry no `LayoutStyle`, so the paint layout currently zeroes
//!   their rects — a positioned sprite does not yet reach the screen. Fixing
//!   this needs a stage layout (fixed size + absolute position) and real asset
//!   files; the R1298 director is **data-level**, not yet a pixel render.
//! - **Script-driven staging.** [`VnStep`] has no stage-directive channel, so a
//!   script (or a future projection) yields a blank stage — staging is only
//!   the out-of-band [`VnExternal`] director verbs today.
//! - **Transitions** (dissolve / fade / shake) and **outcome→world-line branch
//!   mapping** (connecting a chosen outcome to a Mnemosyne world-line).

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
