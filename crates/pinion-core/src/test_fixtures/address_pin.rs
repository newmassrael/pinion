//! Holding a screen's published addresses to their VALUES.
//!
//! # What this is for
//!
//! The address campaign gives each family a declaring site, so a screen's
//! paint, its specification, the wire, the walks that drive it and its own
//! tests all DERIVE one address from one constant. That is the repair, and it
//! means those five move TOGETHER when the constant moves.
//!
//! R2137.3 measured what that costs. One family's 14 literals were renamed
//! consistently — one file, nothing stale left, which the campaign itself had
//! made possible — and the tree passed: four censuses, the screen's own 103
//! tests, the host's 687, and the walk that reads the address off the wire.
//! **790 tests moved with the rename and nothing refused.**
//!
//! ⚠ The distinction that measurement sharpened: renaming ONE constant DOES
//! fail, because the declaration then contradicts itself. What had no check was
//! the VALUE. Those are two different claims, and a weakly-injected
//! counterfactual only reaches the first — which is how the first attempt at
//! that measurement nearly concluded a pin already existed.
//!
//! ⇒ the campaign improves the LIKELIHOOD of a typo (one site instead of
//! fifty) and creates no DETECTION. The hole widens as families convert, so
//! this is a precondition of continuing rather than a finishing touch.
//!
//! # ★ Why the caller hands over TAGS rather than a scene
//!
//! Measured across this workspace's screens before choosing: five carry a
//! `Painted` struct and two hand their sweep a bare `Scene`, and the five have
//! DIVERGED — 3, 4, 7 and 8 fields. More decisively, `Painted` also collects
//! REACHABLE tags, the marks a scroll would bring into view, and a screen's pin
//! includes them. A helper that re-derived tags from the scene would silently
//! under-count exactly on the screens that scroll.
//!
//! So collection stays with the screen, which is where the knowledge of what
//! counts as painted lives, and this module owns only what is genuinely common:
//! the fold, the comparison, the report, and the regeneration path. Lifting
//! `Painted` itself would be the premature extraction R935 warns against.
//!
//! # ★ Why the set is FOLDED
//!
//! [`repeating_site`](crate::containment::repeating_site) turns a grid cell's
//! row-and-column address into one starred site, so a pin is stable against
//! fixture data while a RENAME still moves it. Unfolded, a pin file is a
//! transcript of whatever sample the tests happen to load, and it churns on
//! every data change — which is how a pin stops being read.

use std::collections::BTreeSet;

use crate::containment::repeating_site;

/// The environment variable that rewrites a pin instead of checking it.
///
/// One name for every screen: a round that renames an address regenerates every
/// affected pin with one command rather than remembering a name per screen.
pub const REGEN: &str = "PINION_REGEN_ADDRESS_PIN";

/// Fold a screen's painted tags into the set a pin holds.
///
/// Takes everything the screen considers painted — including whatever it counts
/// as reachable — because only the screen knows which that is.
pub fn fold<'a>(tags: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    tags.into_iter().map(repeating_site).collect()
}

/// Render a pin file's body for `seen`.
///
/// Separate from [`check`] so a caller can write it wherever its own pin lives,
/// and so the format has one definition rather than one per screen.
#[must_use]
pub fn render(seen: &BTreeSet<String>) -> String {
    let mut out = String::from(
        "# Every address this screen paints, folded by `repeating_site`.\n\
         # Rewritten by setting PINION_REGEN_ADDRESS_PIN and running this\n\
         # screen's address-pin test; do not hand-edit.\n",
    );
    for site in seen {
        out.push_str(site);
        out.push('\n');
    }
    out
}

/// The addresses a pin file holds, ignoring comments and blank lines.
#[must_use]
pub fn parse(pinned: &str) -> BTreeSet<String> {
    pinned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Hold `seen` to `pinned` — the whole rule, and it touches nothing.
///
/// `path` names the pin in the failure and is not read. `floor` is the smallest
/// number of addresses this screen can legitimately paint.
///
/// ★ The floor is not decoration. A sweep that painted nothing would otherwise
/// pass against an empty pin, which is the shape this tree has paid for before:
/// a gate that is green for having asked about nothing.
///
/// ★★ PURE, and separate from [`check`] for the reason R1884 records: a rule
/// and the thing that runs it are two gates, and the one that gets tested is
/// the pure one. Every refusal below is exercised by this module's own tests
/// without an environment variable or a filesystem in the way — so a suite run
/// with [`REGEN`] set cannot turn those tests into writes.
///
/// # Panics
///
/// When the sets differ, naming what is no longer painted AND what is newly
/// painted — both directions, because a pin left holding a repaid row claims a
/// debt that is gone, and a pin missing a new row is the defect itself.
pub fn judge(seen: &BTreeSet<String>, pinned: &str, path: &str, floor: usize) {
    assert!(
        seen.len() >= floor,
        "the sweep collected only {} address(es), below this screen's floor of \
         {floor}; the comparison below would be asserting almost nothing",
        seen.len()
    );
    let pinned = parse(pinned);
    let added: Vec<&String> = seen.difference(&pinned).collect();
    let gone: Vec<&String> = pinned.difference(seen).collect();
    assert!(
        added.is_empty() && gone.is_empty(),
        "★★★★★ this screen's published addresses changed. A reader outside \
         this repository addresses marks by these strings, so a change is a \
         PUBLISHED change and has to be deliberate.\n  \
         pin:               {path}\n  \
         no longer painted: {gone:?}\n  \
         newly painted:     {added:?}\n  \
         If the change is intended, set {REGEN} and re-run this test."
    );
}

/// [`judge`], or rewrite the pin at `path` when [`REGEN`] is set.
///
/// The thin half: the environment and the filesystem live here and nowhere
/// else, so what a screen calls is one line and what is tested is [`judge`].
///
/// # Panics
///
/// As [`judge`], or when the pin cannot be written during a regeneration.
pub fn check(seen: &BTreeSet<String>, pinned: &str, path: &str, floor: usize) {
    if std::env::var_os(REGEN).is_some() {
        std::fs::write(path, render(seen)).expect("the pin is writable");
        return;
    }
    judge(seen, pinned, path, floor);
}

#[cfg(test)]
mod tests {
    use super::{REGEN, fold, judge, parse, render};
    use std::collections::BTreeSet;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The fold is what makes a pin stable against fixture data.
    #[test]
    fn the_fold_collapses_an_index_and_keeps_a_name() {
        let seen = fold(["a.grid.cell.3_2", "a.grid.cell.9_2", "a.grid.head"]);
        assert_eq!(seen.len(), 2, "two rows of one column are one site");
        assert!(seen.contains("a.grid.head"), "a name is not an index");
    }

    /// ★ And it is not a constant — the failing path a fold needs.
    #[test]
    fn the_fold_distinguishes_two_families() {
        assert_ne!(fold(["a.one.0"]), fold(["a.two.0"]));
    }

    /// A pin round-trips through its own rendering.
    #[test]
    fn render_and_parse_are_inverse() {
        let seen = set(&["x.a", "x.b.*"]);
        assert_eq!(parse(&render(&seen)), seen);
    }

    /// Comments and blank lines are not addresses.
    #[test]
    fn parse_ignores_comments_and_blanks() {
        assert_eq!(parse("# note\n\n  x.a  \n"), set(&["x.a"]));
    }

    /// The floor refuses a sweep that collected almost nothing.
    #[test]
    #[should_panic(expected = "below this screen's floor")]
    fn a_sweep_that_painted_nothing_cannot_pass_against_an_empty_pin() {
        judge(&BTreeSet::new(), "", "unused", 1);
    }

    /// A newly painted address is named.
    #[test]
    #[should_panic(expected = "newly painted")]
    fn an_unpinned_address_is_refused() {
        judge(&set(&["x.a", "x.b"]), "x.a\n", "unused", 1);
    }

    /// So is one that stopped being painted — the other direction.
    #[test]
    #[should_panic(expected = "no longer painted")]
    fn a_pinned_address_that_vanished_is_refused() {
        judge(&set(&["x.a"]), "x.a\nx.b\n", "unused", 1);
    }

    /// Agreement passes, which is what makes the three refusals above mean
    /// something.
    #[test]
    fn an_agreeing_pin_passes() {
        judge(&set(&["x.a", "x.b"]), "# c\nx.a\nx.b\n", "unused", 2);
    }

    /// The regeneration switch has one name for every screen.
    #[test]
    fn the_regeneration_variable_is_named_once() {
        assert_eq!(REGEN, concat!("PINION_REGEN_", "ADDRESS_PIN"));
    }
}
