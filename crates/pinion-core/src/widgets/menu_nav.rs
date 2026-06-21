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
/// unchanged. Every item is navigable — see [`nav_move_skip`] for the
/// R805 separator-/disabled-skipping variant.
#[must_use]
pub(crate) fn nav_move(active: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    nav_move_skip(active, count, forward, |_| true)
}

/// R805 — like [`nav_move`] but skips indices where `navigable(i)` is
/// `false` (separators / disabled menu items). Scans at most `count`
/// positions from the next slot; if no index is navigable it leaves the
/// cursor unchanged (`active`). From `None`, scanning starts at the first
/// slot (`forward`) or the last (`!forward`) so the edge itself counts.
#[must_use]
pub(crate) fn nav_move_skip(
    active: Option<usize>,
    count: usize,
    forward: bool,
    navigable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if count == 0 {
        return active;
    }
    let mut idx = match active {
        Some(a) => step(a, forward, count),
        None => {
            if forward {
                0
            } else {
                count - 1
            }
        }
    };
    for _ in 0..count {
        if navigable(idx) {
            return Some(idx);
        }
        idx = step(idx, forward, count);
    }
    active
}

/// Jump the active cursor to the first (`!last`) or last (`last`) item.
/// An empty list yields `None` (leave the caller's cursor untouched).
/// Every item is navigable — see [`nav_edge_skip`] for the R805 variant.
#[must_use]
pub(crate) fn nav_edge(count: usize, last: bool) -> Option<usize> {
    nav_edge_skip(count, last, |_| true)
}

/// R805 — like [`nav_edge`] but lands on the first / last index where
/// `navigable(i)` is `true`, scanning inward from the requested edge.
/// `None` when the list is empty or has no navigable index.
#[must_use]
pub(crate) fn nav_edge_skip(
    count: usize,
    last: bool,
    navigable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let (mut idx, forward) = if last { (count - 1, false) } else { (0, true) };
    for _ in 0..count {
        if navigable(idx) {
            return Some(idx);
        }
        idx = step(idx, forward, count);
    }
    None
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
        assert_eq!(
            nav_move(None, 4, true),
            Some(0),
            "Down from nothing -> first"
        );
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
