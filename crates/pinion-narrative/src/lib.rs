//! Read-side consumption of Mnemosyne narrative reports.
//!
//! `pinion-narrative` is the pinion end of a CQRS story pipeline. An
//! authoring tool (Mnemosyne) is the deterministic **write-side** SSOT of a
//! telling; it emits a report — `report-playable-world` (fork topology +
//! per-world-line scene walk + disclosure pointers). This crate is the
//! **read-side**: it deserializes that report tolerantly, holds it as a
//! reactive cursor model, exposes it as an AI-drivable `External`, and
//! projects it into a queryable pinion [`Scene`](pinion_core::scene::Scene)
//! that renders on GUI and TUI alike.
//!
//! It is the substrate the text→2D story-game stages stand on: the same
//! model feeds a TUI scene walk today and a 2D relation map later — the
//! renderer is additive, the data is not.
//!
//! ## Pieces
//!
//! - [`model`] — the tolerant deserialize contract ([`PlayableWorld`] and
//!   friends). Forward/backward compatible with an evolving upstream.
//! - [`load`] — parse from string / file, and the [`resolve_report`]
//!   env-or-bundled acquisition seam.
//! - [`state`] — [`NarrativeState`], the reactive cursor read-model, and
//!   the [`use_narrative_state`] one-Rc SSOT hook.
//! - [`external`] — [`NarrativeExternal`], the §5.15 AI-first drive surface
//!   (query / intervene / invoke over the walk).
//! - [`view`] — [`narrative_scene`], the read-side scene projection.
//! - [`a11y`] — [`narrative_access_nodes`], the AT projection.
//!
//! ## What stays out (by design)
//!
//! No LLM / generation loop runs here — that is an external agent's job.
//! No story-specific rules (a tide clock, a ritual's knots) live here —
//! those belong to the game built *on* this substrate. This crate only
//! consumes a report and makes it drivable + queryable + renderable.

pub mod a11y;
pub mod external;
pub mod load;
pub mod model;
pub mod place;
pub mod state;
pub mod view;

pub use a11y::narrative_access_nodes;
pub use external::{MOVED_INTENT, NarrativeExternal};
pub use load::{
    ReportError, load_report, parse_place_graph, parse_report, resolve_place_graph, resolve_report,
};
pub use model::{Branch, Disclosure, Fork, ForkTree, PlayableWorld, SceneNode, WorldLine};
pub use place::{
    Adjacency, Direction, Place, PlaceGraph, PlaceLayout, PlacedNode, place_map_access_nodes,
    place_map_scene, solve_layout,
};
pub use state::{NarrativeCursor, NarrativeState, use_narrative_state};
pub use view::narrative_scene;
