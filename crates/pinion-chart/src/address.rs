//! Where this crate's painted addresses are declared.
//!
//! Every node this crate emits carries a tag under the chart's `tag_prefix`
//! (default `"chart"`), and the grammar of what follows that prefix —
//! `.series.{i}`, `.inspect.tooltip`, `.bg` — is what a reader needs in order
//! to find a mark. Before this module the grammar had no declaring site: each
//! painter composed its own with `format!`, and every reader outside the crate
//! composed the same string again.
//!
//! ⇒ **one wrong letter compiles, paints, and makes every query looking for
//! that mark answer nothing.** Silently: the reader concludes the chart did not
//! paint it. That is the defect this module is opened against, and it is the
//! crate-side half of a campaign that has already moved seven screens.
//!
//! # ★ What is declared here, and what is deliberately not
//!
//! The unit of declaration is a **grammar with more than one speller**.
//! Measured over this crate's painters, 67 distinct grammars are composed and
//! 52 of them are spelled in exactly one file — that painter *is* their
//! declaring site, and lifting them here would move a name without removing a
//! copy. The other 15 are spelled in two to ten files apiece, and those are
//! what this module holds.
//!
//! The one exception to that rule is the overlay, declared whole. It is a
//! single grammar with nine members, and declaring seven of them while two
//! stayed behind a `format!` in their painter is the shape this campaign
//! exists to remove — one grammar answering in two places.
//!
//! ⚠ The exemption is stated rather than left to be inferred: a painter's own
//! vocabulary (`box.{i}`, `candle.{i}`, `tile.{i}.label`, the tick and label
//! families in [`crate::draw`], the legend's in [`crate::legend`]) is composed
//! where it is painted. A round that gives one of those a second speller should
//! move it here rather than spell it twice.
//!
//! # ★★★★★ The overlay is one grammar under two names
//!
//! Eight chart kinds paint an inspect overlay and [`crate::timeline`] paints
//! the same thing under the part name `playhead` — `header`, `tooltip`,
//! `value.{i}`, member for member. A needle looking for `inspect.` cannot see
//! it, which is why the count above found the timeline's three spellings only
//! after the grammar was named. [`Overlay`] therefore carries the part rather
//! than baking it in, and [`inspect`] and [`playhead`] are the two that exist.
//!
//! # ⚠⚠ `value` is two grammars, and they split by chart kind
//!
//! [`Overlay::value`] addresses the callout's single value and
//! [`Overlay::value_at`] addresses one series' row inside it. Both are spelled
//! `…inspect.value`, one with an index and one without, and which one a chart
//! answers to is a property of the chart:
//!
//! | form | painted by |
//! |---|---|
//! | [`value`](Overlay::value) — no index | bar, donut, treemap |
//! | [`value_at`](Overlay::value_at) — indexed | line, polar, scatter, timeline |
//!
//! A reader that composes `chart.inspect.value.0` against a bar chart finds
//! nothing and reads it as the chart not painting a value. Declaring both, and
//! saying here which kinds answer which, is what this module can do that a
//! `format!` at the call site cannot.

/// The background box behind the whole chart.
///
/// ★ The most-spelled grammar in the crate: ten painters, one per chart kind.
#[must_use]
pub fn bg(prefix: &str) -> String {
    part(prefix, "bg")
}

/// The x axis rule.
#[must_use]
pub fn axis_x(prefix: &str) -> String {
    part(prefix, "axis.x")
}

/// The y axis rule.
#[must_use]
pub fn axis_y(prefix: &str) -> String {
    part(prefix, "axis.y")
}

/// One series' stroked path.
#[must_use]
pub fn series(prefix: &str, index: usize) -> String {
    indexed(prefix, "series", index)
}

/// One series' filled area.
#[must_use]
pub fn area(prefix: &str, index: usize) -> String {
    indexed(prefix, "area", index)
}

/// One datum: series `index`, point `at`.
///
/// ⚠ Both coordinates are in the address because these marks are a grid rather
/// than a list — the series alone does not name one.
#[must_use]
pub fn point(prefix: &str, index: usize, at: usize) -> String {
    format!("{prefix}.point.{index}.{at}")
}

/// One part of one distribution's cap.
///
/// `part` is the painter's own word for which cap this is, and arrives computed
/// rather than as a literal — which is why it is taken rather than enumerated.
#[must_use]
pub fn cap(prefix: &str, index: usize, part: &str) -> String {
    format!("{prefix}.cap.{index}.{part}")
}

/// The general form: one of this crate's parts, under a chart's prefix.
///
/// ★ Public for the same reason [`crate::draw`]'s families are not lifted here:
/// a painter names more parts than this module declares, and one that needs
/// another should compose it through here rather than reach for a `format!`,
/// which is the whole defect.
#[must_use]
pub fn part(prefix: &str, part: &str) -> String {
    format!("{prefix}.{part}")
}

/// The general indexed form: one of this crate's parts, numbered.
#[must_use]
pub fn indexed(prefix: &str, part: &str, index: usize) -> String {
    format!("{prefix}.{part}.{index}")
}

/// The part a tag names, given the prefix it was painted under.
///
/// ★★ The inverse, here beside the composition rather than at each reader's
/// router. A parse written against a separately-typed prefix is the second
/// speller, and its mismatch is silent in the *other* direction — the reader
/// looks for a mark that was painted and concludes it was not.
#[must_use]
pub fn part_of<'a>(prefix: &str, tag: &'a str) -> Option<&'a str> {
    tag.strip_prefix(prefix)?.strip_prefix('.')
}

/// The overlay a chart paints over its marks while a reader inspects it.
///
/// Obtained from [`inspect`] or [`playhead`] — see this module's header for why
/// the part is carried rather than baked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlay<'a> {
    prefix: &'a str,
    part: &'a str,
}

/// The overlay eight chart kinds paint under the part name `inspect`.
#[must_use]
pub fn inspect(prefix: &str) -> Overlay<'_> {
    Overlay {
        prefix,
        part: "inspect",
    }
}

/// The same overlay, under the part name [`crate::timeline`] paints it with.
#[must_use]
pub fn playhead(prefix: &str) -> Overlay<'_> {
    Overlay {
        prefix,
        part: "playhead",
    }
}

impl Overlay<'_> {
    /// The overlay's own node, when the painter gives it one.
    #[must_use]
    pub fn root(&self) -> String {
        format!("{}.{}", self.prefix, self.part)
    }

    /// The callout's heading.
    #[must_use]
    pub fn header(&self) -> String {
        self.member("header")
    }

    /// The callout's frame.
    #[must_use]
    pub fn tooltip(&self) -> String {
        self.member("tooltip")
    }

    /// The callout's value, for a chart whose overlay reads one value.
    ///
    /// ⚠ Not the same grammar as [`value_at`](Self::value_at) — see the table
    /// in this module's header for which chart kinds answer which.
    #[must_use]
    pub fn value(&self) -> String {
        self.member("value")
    }

    /// One series' row inside the callout.
    ///
    /// ⚠ Not the same grammar as [`value`](Self::value).
    #[must_use]
    pub fn value_at(&self, index: usize) -> String {
        format!("{}.{}.value.{index}", self.prefix, self.part)
    }

    /// The mark drawn over whatever the reader is pointing at.
    #[must_use]
    pub fn highlight(&self) -> String {
        self.member("highlight")
    }

    /// The rule dropped through the plot at the reader's x.
    #[must_use]
    pub fn crosshair(&self) -> String {
        self.member("crosshair")
    }

    /// The ring drawn around one hit mark.
    #[must_use]
    pub fn ring(&self, index: usize) -> String {
        format!("{}.{}.ring.{index}", self.prefix, self.part)
    }

    /// The filled marker drawn on one series at the reader's x.
    #[must_use]
    pub fn marker(&self, index: usize) -> String {
        format!("{}.{}.marker.{index}", self.prefix, self.part)
    }

    /// The radial rule a polar chart drops at the reader's angle.
    #[must_use]
    pub fn spoke(&self) -> String {
        self.member("spoke")
    }

    /// A member whose name the painter computes.
    ///
    /// ★ Taken rather than enumerated: a distribution's overlay names its parts
    /// from the same vocabulary its marks use, so the set is the chart's and
    /// not this module's.
    #[must_use]
    pub fn member(&self, member: &str) -> String {
        format!("{}.{}.{member}", self.prefix, self.part)
    }

    /// The member a tag names, or `None` when the tag is not this overlay's.
    #[must_use]
    pub fn member_of<'a>(&self, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(self.prefix)?
            .strip_prefix('.')?
            .strip_prefix(self.part)?
            .strip_prefix('.')
    }
}

#[cfg(test)]
mod tests {
    use super::{
        area, axis_x, axis_y, bg, cap, indexed, inspect, part, part_of, playhead, point, series,
    };

    /// Every grammar this module declares, as the fragment a painter composing
    /// it by hand would leave in its source.
    ///
    /// ⚠ Each needle is ASSEMBLED rather than written whole. This file is the
    /// one the gate excuses, so its own source could carry them safely — but a
    /// needle written whole here is a *literal* of the grammar, and the next
    /// reader who moves this test moves a speller with it. Assembling costs
    /// nothing and removes that.
    const NEEDLES: &[&str] = &[
        concat!("}", ".bg\""),
        concat!("}", ".axis.x\""),
        concat!("}", ".axis.y\""),
        concat!("}", ".series.{"),
        concat!("}", ".area.{"),
        concat!("}", ".point.{"),
        concat!("}", ".cap.{"),
        concat!("}", ".inspect"),
        concat!("}", ".playhead"),
    ];

    /// This crate's sources, read from the tree rather than listed.
    ///
    /// ★ R2053's rule, met with the filesystem instead of a hand list: a module
    /// added tomorrow is in this population without anybody remembering to add
    /// it, so the gate below catches the next painter to spell a grammar rather
    /// than the round after it.
    fn crate_sources() -> Vec<(String, String)> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
            .expect("the crate's own src/")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .map(|path| {
                let name = path
                    .file_name()
                    .expect("a file")
                    .to_string_lossy()
                    .into_owned();
                (name, std::fs::read_to_string(&path).expect("readable"))
            })
            .collect();
        out.sort();
        assert!(out.len() > 1, "the population must be more than one file");
        out
    }

    /// ★★★★★ R2136 — **a grammar painted by more than one chart is typed in
    /// ONE place, and this counts.**
    ///
    /// The debt: a painted mark's address had no declaring site, so every
    /// painter composed its own and every reader composed it again. One wrong
    /// letter compiles, paints, and makes every query for that mark answer
    /// nothing — and the reader concludes the chart did not paint it. The
    /// repair is a declaration; what makes it stay repaired is counting,
    /// because the next painter to want the overlay will reach for `format!`
    /// unless something refuses.
    ///
    /// ⚠ The needle matches a COMPOSITION — a prefix placeholder followed by
    /// the grammar — and not a whole literal. That is deliberate and it is
    /// what this gate can honestly claim: a test asserting `"chart.series.0"`
    /// is checking a painter's output against a constant, which fails loudly
    /// when the grammar moves. A painter *composing* the grammar is the second
    /// copy that does not.
    ///
    /// ⚠⚠ A COMMENT is not a reader, so a whole-line comment is out of the
    /// population — the same rule the Python half of this census settled on,
    /// and this gate's first run is what demanded it: it found two, both of
    /// them [`crate::line`]'s rustdoc *describing* the segment grammar. Prose
    /// that names an address is documentation and its absence would be a worse
    /// tree. A comment TRAILING code is left in, so the inaccuracy runs toward
    /// demanding a look rather than hiding one.
    #[test]
    fn r2136_a_cross_file_grammar_is_typed_in_one_place() {
        let sources = crate_sources();
        let spellers: Vec<(String, &str, usize)> = sources
            .iter()
            .filter(|(name, _)| name != "address.rs")
            .map(|(name, body)| {
                let code: String = body
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (name, code)
            })
            .flat_map(|(name, code)| {
                NEEDLES.iter().filter_map(move |needle| {
                    let count = code.matches(needle).count();
                    (count > 0).then(|| (name.clone(), *needle, count))
                })
            })
            .collect();
        assert_eq!(
            spellers,
            Vec::new(),
            "★★★★★ a grammar more than one chart paints is declared in \
             `address.rs` and composed through it; these file(s) compose it \
             themselves"
        );
    }

    /// The composition and its inverse agree, in both directions.
    ///
    /// ★★ Without this the inverse is a second speller of the same grammar —
    /// the exact shape the module exists to remove, one level down.
    #[test]
    fn the_inverse_recovers_what_the_composition_wrote() {
        assert_eq!(part_of("chart", &bg("chart")), Some("bg"));
        assert_eq!(part_of("chart", &axis_x("chart")), Some("axis.x"));
        assert_eq!(part_of("chart", &axis_y("chart")), Some("axis.y"));
        assert_eq!(part_of("chart", &series("chart", 3)), Some("series.3"));
        assert_eq!(part_of("chart", &area("chart", 0)), Some("area.0"));
        assert_eq!(part_of("chart", &point("chart", 1, 2)), Some("point.1.2"));
        assert_eq!(part_of("chart", &cap("chart", 4, "hi")), Some("cap.4.hi"));
        // A tag painted under a DIFFERENT prefix is not this chart's.
        assert_eq!(part_of("chart", &bg("spark")), None);
        // ⚠ And a prefix that is a prefix of another's is still not a match:
        // the separator is required, so `chartier.bg` is not `chart`'s.
        assert_eq!(part_of("chart", "chartier.bg"), None);
    }

    /// The overlay is one grammar, and the part is what varies.
    #[test]
    fn the_overlay_carries_its_part() {
        assert_eq!(inspect("chart").tooltip(), "chart.inspect.tooltip");
        assert_eq!(playhead("chart").tooltip(), "chart.playhead.tooltip");
        assert_eq!(playhead("chart").root(), "chart.playhead");
        assert_eq!(
            inspect("chart").member_of("chart.inspect.header"),
            Some("header")
        );
        assert_eq!(inspect("chart").member_of("chart.playhead.header"), None);
        assert_eq!(
            playhead("chart").member_of("chart.playhead.value.2"),
            Some("value.2")
        );
    }

    /// ⚠⚠ The two `value` grammars are DIFFERENT addresses, and this is what
    /// says so.
    ///
    /// A reader that composes the indexed form against a chart painting the
    /// bare one finds nothing — silently, which is this debt's whole failure
    /// mode. The module header carries the table of which kinds paint which;
    /// this refuses the two from being quietly folded together.
    #[test]
    fn value_and_value_at_are_not_the_same_address() {
        let overlay = inspect("chart");
        assert_eq!(overlay.value(), "chart.inspect.value");
        assert_eq!(overlay.value_at(0), "chart.inspect.value.0");
        assert_ne!(overlay.value(), overlay.value_at(0));
        // ★ And the bare form is not a prefix-match away from the indexed one:
        // a reader stripping `value` off `value.0` gets `.0`, not nothing, so
        // the inverse can tell them apart.
        assert_eq!(overlay.member_of(&overlay.value()), Some("value"));
        assert_eq!(overlay.member_of(&overlay.value_at(7)), Some("value.7"));
    }

    /// The general forms compose what the named ones do.
    #[test]
    fn the_general_forms_agree_with_the_named_ones() {
        assert_eq!(part("chart", "bg"), bg("chart"));
        assert_eq!(indexed("chart", "series", 2), series("chart", 2));
        assert_eq!(indexed("chart", "area", 2), area("chart", 2));
    }
}
