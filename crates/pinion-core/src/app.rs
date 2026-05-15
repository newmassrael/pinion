//! Window topology entry point (§5.19, R16).
//!
//! `app.scxml` at the consumer crate root is compiled by `sce-build` and
//! included here. The wrapping `mod sm` block absorbs the inner attributes
//! that `include!()` does not permit in expansion position.
//!
//! Re-exports surface the SCE-emitted topology types (`AppState`, etc.)
//! so `pinion-runtime` can consume them once R16 wires the routing layer.

#[allow(
    unsafe_code,
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all,
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/app_sm.rs"));
}

pub use sm::{AppEvent, AppState};
