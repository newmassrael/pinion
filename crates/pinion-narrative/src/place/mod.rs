//! The spatial read-side: a relation place-graph → a solved, queryable 2D
//! map scene.
//!
//! The peer of the narrative scene walk. Where the walk is the story in
//! time, this is the world in space — and it is where the field report's
//! F3 lives: the coordinates are **not** in the data. Per "관계=SSOT /
//! 좌표=휘발" the author declares only relations (containment / adjacency /
//! direction); pinion's [`solve_layout`] deterministically solves the
//! coordinates, [`place_map_scene`] projects them into a queryable scene,
//! and `impl QuerySource for PlaceLayout` exposes the solved geometry over
//! RPC (wrapped in pinion-core's `QueryOnlyIntrospect`).
//!
//! This is additive to the text stage: text→2D changes the renderer, not
//! the data. The same relation graph a 3D stage will read is solved to 2D
//! here.
//!
//! - [`model`] — the tolerant place-graph deserialize contract.
//! - [`layout`] — the deterministic coordinate solver.
//! - [`view`] — the scene projection (boxes / labels / adjacency lines).
//! - [`external`] — `impl QuerySource for PlaceLayout`, wrapped in
//!   pinion-core's `QueryOnlyIntrospect` for the RPC read surface.

pub mod a11y;
pub mod external;
pub mod layout;
pub mod model;
pub mod view;

pub use a11y::place_map_access_nodes;
pub use layout::{PlaceLayout, PlacedNode, solve_layout};
pub use model::{Adjacency, Direction, Place, PlaceGraph};
pub use view::place_map_scene;
