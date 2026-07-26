//! `pinion-graph` — layered graph layout, as pure arithmetic (R1442, Phase B).
//!
//! # Why this crate exists
//!
//! A node graph is one of the shapes a professional tool draws constantly: a
//! visual-scripting canvas, a material graph, a build-dependency map, a live
//! service topology. Placing one by hand is the user's job in an *editor* and
//! nobody's job in a *view* — a topology that arrives over the wire has no
//! coordinates at all, so the tool has to invent them, and invent them again
//! every time the graph changes.
//!
//! R1383 built the first layered layout inside `hello-node-editor`, and R1441
//! finished it (a proper layering plus the Brandes–Köpf coordinate solver),
//! deliberately writing the algorithm against abstract vertices `0..n` so that a
//! second consumer would make the crate lift a file move rather than a rewrite.
//! R1442 is that consumer — a live topology view — and this crate is the move.
//!
//! # Shape
//!
//! Everything here is **pure arithmetic**: vertices are indices, sizes and
//! coordinates are `i32`, and there is no dependency on `pinion-core` or on any
//! renderer. A caller maps its own node type onto `0..n`, calls
//! [`Sugiyama::run`], and maps the answer back. That keeps the algorithm
//! testable on hand-written graphs — which is where its adversarial fixtures
//! live — and keeps it reachable from any backend, GUI or TUI or headless.
//!
//! The pipeline is Sugiyama's: break cycles, assign layers by longest path, make
//! the layering *proper* by splitting long edges over bends, order each layer,
//! then solve the within-layer coordinate.
//!
//! # Laying out the same graph twice
//!
//! The two entry points differ only in how each layer is ORDERED, and the
//! difference is the whole reason a view is not an editor:
//!
//! * [`Sugiyama::run`] minimises crossings — the right answer for a one-shot
//!   "tidy this up".
//! * [`Sugiyama::run_seeded`] keeps the order of the drawing the viewer already
//!   has, so a graph that changes under them stays recognisable.
//!
//! Both publish what they cost — [`Layout::crossings`] and
//! [`Layout::order_changes`] — because the trade between a tidy drawing and a
//! stable one is a judgement the caller has to make with numbers, not prose.
//!
//! ```
//! use pinion_graph::Sugiyama;
//!
//! // A diamond: 0 feeds 1 and 2, which both feed 3.
//! let sizes = [30, 30, 30, 30];
//! let edges = [(0, 1), (0, 2), (1, 3), (2, 3)];
//! let layout = Sugiyama::default().run(&sizes, &edges);
//!
//! assert_eq!(layout.layers(), &[0, 1, 1, 2]);
//! assert_ne!(layout.centres()[1], layout.centres()[2]);
//!
//! // Lay it out again keeping what the viewer has seen.
//! let seed: Vec<Option<i32>> = layout.centres().iter().map(|&c| Some(c)).collect();
//! let again = Sugiyama::default().run_seeded(&sizes, &edges, &seed);
//! assert_eq!(again.order_changes(&seed), 0);
//! ```

mod layering;
mod sugiyama;

pub use layering::Layering;
pub use sugiyama::{Layout, Sugiyama};
