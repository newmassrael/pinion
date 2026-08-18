//! R1715.1 (R1306 PR-51) §5.16 §5.41 — a binding with NO primary surface can
//! be painted.
//!
//! `primary_surface() -> None` is a topology, not an edge case: every surface
//! is a dynamic extra (`hello-floating-chart`, `hello-dock-chart`,
//! `hello-no-primary`). Such a binding's `tag()` and `create_external()` are
//! `unreachable!()` BY DESIGN — returning `None` is the declaration that they
//! must never be reached — so any substrate site that reads the binding's
//! identity with a bare `V::tag()` instead of through `primary_surface()`
//! panics here and nowhere else. R1306 made that a standing rule.
//!
//! ## Why this file exists
//!
//! R1714 broke the rule in the paint path: it passed `V::tag()` as an argument
//! to the pan it applies in `window_view`, so the tag was evaluated for all 225
//! bindings including the 224 that declare no pan at all. Nothing local caught
//! it. The topology HAS a dedicated example, `hello-no-primary` — and its four
//! tests never paint, so the panic surfaced only in an unrelated example that
//! happens to be no-primary AND paints, in CI, after the push, where it then
//! held the stop-the-line gate shut against the next round.
//!
//! The gap is the same shape as the one R1715 came from: a rule with no gate at
//! the layer it governs. The paint path is a shell concern, so the gate belongs
//! in the shell's own suite rather than in one example that happens to trip it.

use pinion_core::test_fixtures::{NO_PRIMARY_PANEL, NoPrimaryFixture};
use pinion_shell::ShellCore;

/// The fixture's single panel, sized so the surface is not degenerate.
const SURFACE: (u32, u32) = (80, 40);

/// R1715.1 — painting a no-primary binding reaches no `V::tag()`.
///
/// The assertion is the absence of a panic; the scene check is what keeps the
/// test honest about having actually produced a frame rather than an empty one.
#[test]
fn a_binding_with_no_primary_surface_paints() {
    let mut core = ShellCore::<NoPrimaryFixture>::new();

    let scene = core.compute_paint_scene(SURFACE.0, SURFACE.1);

    assert!(
        scene.contains_tag(NO_PRIMARY_PANEL),
        "the extra surface is painted; a frame without it is not evidence the \
         paint path is safe for this topology",
    );
}

/// R1715.1 — and a full frame does too, not only the paint half.
///
/// `finalize_frame` runs the post-paint arc (`handle_tail`, the external-set
/// reconcile), which is a second family of substrate sites that could read the
/// binding's identity. The failure R1714 shipped was in the paint half; a gate
/// that stopped there would leave the other half exactly as unguarded as the
/// paint half was.
#[test]
fn a_full_frame_of_a_no_primary_binding_completes() {
    let mut core = ShellCore::<NoPrimaryFixture>::new();

    let scene = core.compute_paint_scene(SURFACE.0, SURFACE.1);
    core.finalize_frame(scene);
    // A second frame: the first one populates caches, so a site that reads the
    // identity only on a warm path would be missed by a single-frame gate.
    let scene = core.compute_paint_scene(SURFACE.0, SURFACE.1);
    core.finalize_frame(scene);

    assert!(
        core.focus()
            .tab_order()
            .iter()
            .any(|t| t == NO_PRIMARY_PANEL),
        "the extra surface is enumerated, so the frame ran its focus derivation",
    );
}
