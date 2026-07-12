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
//! - [`state`] — [`VnState`], the reactive deterministic play-head, and the
//!   [`use_vn_state`] one-Rc SSOT hook.
//! - [`external`] — [`VnExternal`], the §5.15 AI-first drive surface
//!   (query / intervene / invoke `tick` / `advance` / `choose`).
//! - [`view`] — [`vn_scene`], the read-side structured-scene projection.
//!
//! ## What stays out (this round, by design)
//!
//! Sprite / background director, transitions (dissolve / fade / shake),
//! save/load serialization, real-time frame-driven play (wiring the runner
//! to the shell's frame delta / the game-loop `scene/tick`), and
//! outcome→world-line branching are all deferred follow-ups. This round is
//! the VN heart — typewriter + timed-choice — proven over the wire.

pub mod external;
pub mod model;
pub mod state;
pub mod view;

pub use external::{ADVANCED_INTENT, CHOSEN_INTENT, TIMEOUT_INTENT, VnExternal};
pub use model::{VnOption, VnScript, VnStep};
pub use state::{ChooseError, VnCursor, VnMode, VnResolution, VnRuntime, VnState, use_vn_state};
pub use view::vn_scene;
