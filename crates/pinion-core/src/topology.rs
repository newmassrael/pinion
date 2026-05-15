//! Generic window-topology helpers over `StatePolicy` (§5.17 §5.18, R16 slice 4).
//!
//! These functions encode the parallel-root vs flat-root branch ratified in
//! §5.17:
//!
//!   - `<parallel>` root → windows are `get_parallel_regions(root)`; the
//!     "first window" per §5.18 short-circuit is the region with lowest
//!     `get_document_order`.
//!   - Non-parallel root → the sole root state itself is the only window.
//!
//! Reverse-mapping is linear-scan today (acceptable for N ≲ 16). The §5.18
//! caveat about build-time perfect-hash is a future codegen optimization
//! that can land in `pinion-core/build.rs` without changing this surface.

use sce_rust_runtime::StatePolicy;

/// First declared window — §5.18 absent-prefix short-circuit target.
///
/// Returns the lowest-doc-order parallel region when the root is parallel,
/// otherwise the root itself. Panics under the §5.17 invariant violation
/// of a parallel root with zero regions (forbidden by SCE Forge emit).
pub fn initial_window<P: StatePolicy>() -> P::State
where
    P::State: Copy,
{
    let root = P::initial_state();
    if P::is_parallel_state(root) {
        let regions = P::get_parallel_regions(root);
        *regions
            .iter()
            .min_by_key(|&&s| P::get_document_order(s))
            .expect("§5.17 invariant: parallel root has at least one region")
    } else {
        root
    }
}

/// Reverse map: SCE-emit window-id string → state variant.
///
/// Iterates parallel regions of root when present; otherwise compares the
/// sole root state. Returns `None` when `name` matches no declared window.
pub fn window_from_name<P: StatePolicy>(name: &str) -> Option<P::State>
where
    P::State: Copy,
{
    let root = P::initial_state();
    if P::is_parallel_state(root) {
        for &s in P::get_parallel_regions(root) {
            if P::get_state_name(s) == name {
                return Some(s);
            }
        }
        None
    } else if P::get_state_name(root) == name {
        Some(root)
    } else {
        None
    }
}
