//! R1836 §5.32 §5.45 — **a sweep's stride, measured against the window it must
//! not step over.**
//!
//! # The defect this ends
//!
//! A gate that drives a gesture in steps can only see a rule whose window is at
//! least as wide as the step. R1704 measured that the hard way on the node lab:
//! two spellings of one guard — *leave it alone once the WHOLE column is off the
//! canvas* and *…once ANY PART of it is* — differ only while the canvas edge
//! falls between two parts of a clustered affordance, and that window is about
//! **26 px**. The sweep written to tell them apart strode **90 px**, so it
//! stepped straight over the only pans where the two answers differ, and the
//! counterfactual for the WRONG spelling **passed twice** before anyone measured
//! the stride against the geometry it samples.
//!
//! A counterfactual that passes is worse than a missing test: it is a test
//! reporting that a defect is impossible.
//!
//! # Why this is a fixture and not a comment
//!
//! R1704 repaired its own call site and wrote the reason beside it, which is
//! where the rule stayed. Nothing carried it to the next sweep, and the next
//! sweep is written by whoever needs one — so the rule was one comment away
//! from being re-learned at the next clustered affordance. The debt that
//! recorded this (`a screen is not looked at before it is declared done`) had
//! already ruled out the obvious remedy: *take a screenshot and look* does not
//! catch it either, because a still frame says nothing about a rule that only
//! fires once a gesture has been driven a long way.
//!
//! # The rule, and why it is exactly this
//!
//! Samples sit `stride` apart. A window of width `w` is GUARANTEED to contain a
//! sample if and only if `w >= stride`: with `stride <= w` no window of width
//! `w` can fall entirely between two consecutive samples, and with
//! `stride > w` one always can — put the window in the gap. So the rule is
//! `stride <= narrowest_window`, and it is an iff rather than a heuristic,
//! which is why it can be asserted rather than advised.
//!
//! ⚠ **What it does NOT claim.** It says the sweep cannot step OVER the window;
//! it does not say the sweep is long enough to REACH it. Those are two
//! different bounds and this is only the first — R1704 needed both, and the
//! reach half is what [`clamp::ClampCensus`](super::clamp::ClampCensus) already
//! answers by asserting that both sides of a guard were actually seen.

/// The widest stride that cannot step over a window `narrowest_window` wide.
///
/// Use it at the call site instead of writing a number: a stride that is
/// derived from the geometry it samples cannot drift away from it when the
/// geometry moves, which is exactly what a hand-picked 90 did.
///
/// Clamped to at least 1, because a zero stride advances nothing and a sweep
/// that never moves is not a sweep.
#[must_use]
pub const fn stride_for(narrowest_window: u32) -> u32 {
    if narrowest_window < 1 {
        1
    } else {
        narrowest_window
    }
}

/// Refuse a stride that could step over `narrowest_window`.
///
/// `what` names the window in the failure, because the number alone does not
/// tell a reader which affordance stopped being measurable.
///
/// # Panics
///
/// If `stride` is zero, or greater than `narrowest_window`.
pub fn assert_stride_cannot_skip(what: &str, narrowest_window: u32, stride: u32) {
    assert!(
        stride >= 1,
        "the sweep over {what} strides 0 — it never advances, so it samples one \
         point however many times it runs",
    );
    assert!(
        stride <= narrowest_window,
        "★ the sweep over {what} strides {stride} px across a window only \
         {narrowest_window} px wide, so it can step straight over it and report \
         that the rule inside is unreachable. Derive the stride with \
         `stride_for({narrowest_window})` rather than choosing it.",
    );
}

#[cfg(test)]
mod tests {
    use super::{assert_stride_cannot_skip, stride_for};

    #[test]
    fn a_stride_no_wider_than_the_window_is_accepted() {
        assert_stride_cannot_skip("a window", 26, 26);
        assert_stride_cannot_skip("a window", 26, 8);
        assert_stride_cannot_skip("a window", 1, 1);
    }

    /// ★★★★★ The measured case: R1704's 90 px stride against its 26 px window.
    #[test]
    #[should_panic(expected = "strides 90 px across a window only 26 px wide")]
    fn the_stride_that_stepped_over_r1704s_window_is_refused() {
        assert_stride_cannot_skip("the picked link's column", 26, 90);
    }

    /// ★ One past the window is refused, which is where an off-by-one would
    /// hide: a rule spelled `<` instead of `<=` would reject the exact fit
    /// above, and one spelled `<= w + 1` would admit this.
    #[test]
    #[should_panic(expected = "strides 27 px across a window only 26 px wide")]
    fn one_wider_than_the_window_is_refused() {
        assert_stride_cannot_skip("a window", 26, 27);
    }

    #[test]
    #[should_panic(expected = "strides 0")]
    fn a_stride_of_zero_is_refused() {
        assert_stride_cannot_skip("a window", 26, 0);
    }

    #[test]
    fn the_derived_stride_is_always_accepted_by_the_rule() {
        // ★ The two halves are a pair: whatever `stride_for` returns must pass
        // `assert_stride_cannot_skip` for that same window, or a caller doing
        // the right thing would be refused. Swept rather than sampled, because
        // a pair asserted at one width is a pair asserted nowhere.
        for w in 0..512 {
            assert_stride_cannot_skip("derived", stride_for(w).max(w), stride_for(w));
        }
    }
}
