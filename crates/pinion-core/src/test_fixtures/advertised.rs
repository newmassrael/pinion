//! R1819 §5.32 §5.40 — **every gesture a screen ADVERTISES does something**,
//! and the population it is asked of is never empty.
//!
//! # The defect this exists for, measured
//!
//! Screen A of the analysis tool printed `wheel → zoom` on its hint strip for
//! its whole life and the wheel was dead. There was already a gate over the
//! screen's operation table, and it could not see this in principle: the hint
//! strip is a **different population**. R1703 put a gate on that population and
//! recorded, in the same round, the thing that makes this module necessary —
//! only ONE of the tool's three screens had a gesture list at all, so on the
//! other two the gate ran over the empty set and passed.
//!
//! ⇒ **an empty population is indistinguishable from a kept promise**, which is
//! why the non-emptiness is part of the rule here rather than something each
//! caller remembers.
//!
//! # Why it is a fixture rather than a third copy
//!
//! R1707 gave screen B a list and a gate, by writing the gate a second time.
//! The round after it registered that the gate now existed twice — and screen C
//! still had none, so the honest next step was a THIRD copy. Three copies of a
//! rule is how a rule starts differing: the two that existed already disagreed
//! about whether to compare the advertised set with the driver set, and only
//! one of them checked that the promised EFFECT matched.
//!
//! What differs per screen is the **driving** and the **witness**; the shape —
//! non-empty, every advertised gesture has a driver, every driver moves
//! something, and the report names which one did not — is the framework's.
//!
//! # What the witness is, and why the caller chooses it
//!
//! A hint strip claims an effect in PROSE (*pan*, *author a link*), so a gate
//! that picked one slot to watch would be deciding what the prose meant. The
//! honest reading of *this gesture does something* is that the screen's whole
//! published state moved, and that is what a caller should hand over. Pinning
//! each effect to its own witness is a different gate, and a screen that has
//! one keeps it.

use std::collections::BTreeSet;
use std::fmt::Debug;

/// Assert that every gesture `advertised` has a driver and that driving it
/// moves the screen.
///
/// * `screen` names the screen in failure messages.
/// * `advertised` is what the screen tells a person it does — `(gesture,
///   effect)`, the pair a hint strip or a published list carries.
/// * `witness` answers the screen's state; it is called before and after each
///   gesture and compared. Hand over the WHOLE published state unless the
///   screen has a per-effect gate elsewhere.
/// * `drive` performs one gesture and answers whether it knew how. A `false`
///   is a failure, not a skip: a screen advertising something no driver can
///   perform is exactly the defect this module exists for, and a silent skip
///   would restore it.
///
/// # Panics
///
/// When `advertised` is empty, when a gesture has no driver, or when a driven
/// gesture leaves the witness unchanged. All inert gestures are collected and
/// named together, because a report that stops at the first makes the second
/// cost another run.
pub fn assert_every_advertised_gesture_acts<W, F, D>(
    screen: &str,
    advertised: &[(&str, &str)],
    mut witness: F,
    mut drive: D,
) where
    W: PartialEq + Debug,
    F: FnMut() -> W,
    D: FnMut(&str) -> bool,
{
    assert!(
        !advertised.is_empty(),
        "{screen} advertises no gesture at all, so this gate would pass over \
         the empty set — and an empty population is indistinguishable from a \
         kept promise, which is the defect this fixture exists for"
    );
    assert_distinct(screen, advertised);

    let mut inert = Vec::new();
    for (gesture, effect) in advertised {
        let before = witness();
        assert!(
            drive(gesture),
            "{screen} advertises {gesture:?} and no driver here performs it — a \
             promise with nothing behind it is how a dead wheel stayed \
             advertised for a screen's whole life"
        );
        let after = witness();
        if before == after {
            inert.push(format!("{gesture:?} claims {effect:?} and moved nothing"));
        }
    }
    assert!(
        inert.is_empty(),
        "{screen} advertises {} gesture(s) that do nothing: {}",
        inert.len(),
        inert.join("; ")
    );
}

/// No gesture is advertised twice, and no two promise different effects under
/// one name.
///
/// ★ A duplicate would make the loop above drive it twice and report it twice,
/// which reads as two defects; and one name with two effects is a list that
/// cannot be believed whichever line a reader happens to see.
fn assert_distinct(screen: &str, advertised: &[(&str, &str)]) {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (gesture, _) in advertised {
        assert!(
            seen.insert(gesture),
            "{screen} advertises {gesture:?} more than once"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::assert_every_advertised_gesture_acts;
    use std::cell::Cell;

    /// The ordinary case: two gestures, both drivable, both moving something.
    #[test]
    fn r1819_a_screen_whose_gestures_all_act_passes() {
        let moved = Cell::new(0u32);
        assert_every_advertised_gesture_acts(
            "a screen",
            &[("click a row", "opens it"), ("type", "narrows the list")],
            || moved.get(),
            |_| {
                moved.set(moved.get() + 1);
                true
            },
        );
    }

    /// ★★★★★ An EMPTY list is refused, which is the whole reason this is a
    /// fixture: two of three screens had no list, so the gate they ran passed
    /// over nothing and read exactly like a kept promise.
    #[test]
    #[should_panic(expected = "advertises no gesture at all")]
    fn r1819_an_empty_population_is_refused() {
        assert_every_advertised_gesture_acts("a screen", &[], || 0u32, |_| true);
    }

    /// A gesture nothing can perform is a failure, not a skip.
    #[test]
    #[should_panic(expected = "no driver here performs it")]
    fn r1819_an_undrivable_gesture_is_refused() {
        assert_every_advertised_gesture_acts("a screen", &[("wheel", "zooms")], || 0u32, |_| false);
    }

    /// ★ The original defect, in one line: the gesture is driven, the driver
    /// says it knew how, and nothing moved.
    #[test]
    #[should_panic(expected = "claims \"zooms\" and moved nothing")]
    fn r1819_a_driven_gesture_that_moves_nothing_is_refused() {
        assert_every_advertised_gesture_acts("a screen", &[("wheel", "zooms")], || 0u32, |_| true);
    }

    /// Every inert gesture is named, not just the first.
    #[test]
    #[should_panic(expected = "advertises 2 gesture(s) that do nothing")]
    fn r1819_all_the_inert_ones_are_named_together() {
        assert_every_advertised_gesture_acts(
            "a screen",
            &[("wheel", "zooms"), ("drag", "pans")],
            || 0u32,
            |_| true,
        );
    }

    /// A list that says one name twice cannot be believed.
    #[test]
    #[should_panic(expected = "more than once")]
    fn r1819_a_duplicated_gesture_is_refused() {
        assert_every_advertised_gesture_acts(
            "a screen",
            &[("wheel", "zooms"), ("wheel", "pans")],
            || 0u32,
            |_| true,
        );
    }
}
