//! R772 §5.38 — shared active-item navigation for command-menu lists.
//!
//! The wrap-around "move the highlighted item" arithmetic is identical
//! for every command-menu surface: the R691 [`MenuBar`](super::menu)
//! dropdown and the R772 [`ContextMenu`](super::context_menu) popup both
//! drive an `active: Option<usize>` cursor over a fixed item count. The
//! two consumers must stay byte-identical (a divergence would be a
//! keyboard-navigation bug, not a style choice), so the arithmetic is
//! lifted here at the second consumer rather than copied — the
//! `decode == inverse(encode)` / "divergence-is-a-bug" lift class
//! (R743.1 / R745), not the opinionated Rule-of-Three deferral.

/// One-step wrap-around over `0..n` (`n > 0`). `forward` advances,
/// `!forward` retreats; both wrap at the ends.
#[must_use]
pub(crate) fn step(current: usize, forward: bool, n: usize) -> usize {
    if forward {
        (current + 1) % n
    } else {
        (current + n - 1) % n
    }
}

/// Move the active cursor over `count` items with wrap. From `Some(a)`
/// it steps once; from `None` it lands on the first item (`forward`) or
/// the last (`!forward`). An empty list (`count == 0`) leaves the cursor
/// unchanged.
#[must_use]
pub(crate) fn nav_move(active: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    if count == 0 {
        return active;
    }
    Some(match active {
        Some(a) => step(a, forward, count),
        None => {
            if forward {
                0
            } else {
                count - 1
            }
        }
    })
}

/// Jump the active cursor to the first (`!last`) or last (`last`) item.
/// An empty list yields `None` (leave the caller's cursor untouched).
#[must_use]
pub(crate) fn nav_edge(count: usize, last: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(if last { count - 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_wraps_both_directions() {
        assert_eq!(step(0, true, 3), 1);
        assert_eq!(step(2, true, 3), 0, "forward wraps at the end");
        assert_eq!(step(0, false, 3), 2, "backward wraps at the start");
    }

    #[test]
    fn nav_move_from_none_lands_on_edge() {
        assert_eq!(nav_move(None, 4, true), Some(0), "Down from nothing -> first");
        assert_eq!(nav_move(None, 4, false), Some(3), "Up from nothing -> last");
    }

    #[test]
    fn nav_move_steps_and_wraps() {
        assert_eq!(nav_move(Some(1), 4, true), Some(2));
        assert_eq!(nav_move(Some(3), 4, true), Some(0));
        assert_eq!(nav_move(Some(0), 4, false), Some(3));
    }

    #[test]
    fn nav_move_empty_is_inert() {
        assert_eq!(nav_move(Some(0), 0, true), Some(0));
        assert_eq!(nav_move(None, 0, true), None);
    }

    #[test]
    fn nav_edge_picks_first_or_last() {
        assert_eq!(nav_edge(5, false), Some(0));
        assert_eq!(nav_edge(5, true), Some(4));
        assert_eq!(nav_edge(0, true), None, "empty list -> no edge");
    }
}
