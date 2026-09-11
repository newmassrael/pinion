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
//! families in `draw`, the legend's in `legend`) is composed
//! where it is painted. A round that gives one of those a second speller should
//! move it here rather than spell it twice.
//!
//! # ★★★★★ The overlay is one grammar under two names
//!
//! Eight chart kinds paint an inspect overlay and `timeline` paints
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

/// The prefix a chart paints under when its caller does not choose one.
///
/// ★★★★★ R2143 — **this was spelled eight times before it was declared once.**
/// R2136 lifted the grammar that follows a prefix and missed the prefix itself,
/// because its needle looked for a `}`-placeholder followed by the grammar and
/// `"chart"` is a bare word in a `Default` impl. Eight builders each wrote
/// `tag_prefix: "chart".to_string()`, which is a cross-file grammar by R2136's
/// own criterion — spelled in more than one file — and every reader that wants
/// a default-prefixed address had nothing to reach for either.
pub const DEFAULT_PREFIX: &str = "chart";

/// The overlay's frame, under [`DEFAULT_PREFIX`].
///
/// ★★ A whole address as a `&'static str`, which the composers cannot give:
/// they run `format!`, so none is a `const fn`, and a reader holding a `const`
/// — an accessibility region's tag, a `&'static str` Tab stop — has nothing to
/// call. R2049 met the same wall on a screen and answered it the same way, with
/// a `&'static str` beside the derivation.
///
/// ⚠ It spells its prefix, because `concat!` takes literals and not consts, so
/// [`DEFAULT_PREFIX`] cannot be pasted into it. That is one spelling in the
/// declaring module rather than one per reader, and
/// `the_const_addresses_agree_with_their_composers` holds it to
/// [`inspect`]`(`[`DEFAULT_PREFIX`]`).tooltip()` so the two cannot drift.
pub const DEFAULT_INSPECT_TOOLTIP: &str = "chart.inspect.tooltip";

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
/// ★ Public for the same reason `draw`'s families are not lifted here:
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

// ── R2145: the families R2136's cut left out ────────────────────────────────
//
// ★★★★★ R2136 cut this module at "a grammar spelled in more than one PAINTER",
// on the reasoning that a grammar spelled in exactly one painter is already
// declared there. That is right for stopping painter-side duplication and
// WRONG as a basis for reader conversion, and the measurement says so: a walk
// is another language in another file, so every chart grammar is cross-file
// from its side. The walks spell 27 distinct grammars; before this block the
// declaration held 6, and 154 chart walk sites had nothing to be handed.
//
// ⚠ Two names below are deliberate and a reader should not "tidy" them:
// `box_at` because `box` is a Rust keyword, and `ring_at`/`spoke_at` because
// [`Overlay::ring`] and [`Overlay::spoke`] already own those words for
// DIFFERENT addresses — `<prefix>.inspect.ring.<i>` is the ring drawn round a
// hit mark, while `<prefix>.ring.<k>` is a polar chart's own grid ring. Free
// functions and methods would not collide at the compiler, so nothing but the
// name would have stopped a reader taking the wrong one.

/// The accessibility node for a named landmark of a chart's summary.
///
/// 🟥🟥🟥★★★★★ R2145 — **this family is spelled in FOUR painters (boxplot, bar,
/// candlestick, scatter) and R2136's own criterion would have declared it. It
/// was missed because the inventory's character class was `[a-z_.{}]` and
/// `a11y` CARRIES A DIGIT**, so the token did not fail the group — it failed
/// the whole pattern, and the family never appeared in the count at all. This
/// tree has paid for that shape before: R2130 judged `i32` a regex artifact on
/// the same reasoning and it was a real fifth word.
///
/// ⇒ a pattern is part of the claim a census makes. A family a census cannot
/// spell is a family it reports as absent.
#[must_use]
pub fn a11y_at(prefix: &str, name: &str) -> String {
    format!("{prefix}.a11y.{name}")
}

/// The accessibility node for one row of a chart's data.
///
/// ⚠ The index joins with a bare `r` and NOT a separator — `…a11y.r3`, not
/// `…a11y.r.3`. Declared here so a reader who knows this tree's dotted
/// convention cannot apply it by reflex and find nothing.
#[must_use]
pub fn a11y_row(prefix: &str, index: usize) -> String {
    format!("{prefix}.a11y.r{index}")
}

/// The accessibility node standing for a chart's series as a whole.
#[must_use]
pub fn a11y_series(prefix: &str) -> String {
    part(prefix, "a11y.series")
}

/// The accessibility node for one series.
#[must_use]
pub fn a11y_series_at(prefix: &str, index: usize) -> String {
    indexed(prefix, "a11y.series", index)
}

/// One bar of a bar chart.
#[must_use]
pub fn bar(prefix: &str, index: usize) -> String {
    indexed(prefix, "bar", index)
}

/// One box of a box plot.
///
/// ⚠ `box_at`, not `box`: `box` is a reserved word in Rust.
#[must_use]
pub fn box_at(prefix: &str, index: usize) -> String {
    indexed(prefix, "box", index)
}

/// One session's candle body.
#[must_use]
pub fn candle(prefix: &str, index: usize) -> String {
    indexed(prefix, "candle", index)
}

/// The colour scale's strip.
#[must_use]
pub fn colorbar_strip(prefix: &str) -> String {
    part(prefix, "colorbar.strip")
}

/// One tick of the colour scale.
#[must_use]
pub fn colorbar_tick(prefix: &str, index: usize) -> String {
    indexed(prefix, "colorbar.tick", index)
}

/// One series' in-window overdraw, filled.
#[must_use]
pub fn focus_area(prefix: &str, index: usize) -> String {
    indexed(prefix, "focus.area", index)
}

/// One series' in-window overdraw, stroked.
#[must_use]
pub fn focus_series(prefix: &str, index: usize) -> String {
    indexed(prefix, "focus.series", index)
}

/// One vertical gridline.
#[must_use]
pub fn grid_x(prefix: &str, index: usize) -> String {
    indexed(prefix, "grid.x", index)
}

/// One horizontal gridline.
#[must_use]
pub fn grid_y(prefix: &str, index: usize) -> String {
    indexed(prefix, "grid.y", index)
}

/// One minor vertical gridline.
#[must_use]
pub fn grid_minor_x(prefix: &str, index: usize) -> String {
    indexed(prefix, "grid.minor.x", index)
}

/// One minor horizontal gridline.
#[must_use]
pub fn grid_minor_y(prefix: &str, index: usize) -> String {
    indexed(prefix, "grid.minor.y", index)
}

/// One x-axis tick label.
#[must_use]
pub fn label_x(prefix: &str, index: usize) -> String {
    indexed(prefix, "label.x", index)
}

/// One y-axis tick label.
#[must_use]
pub fn label_y(prefix: &str, index: usize) -> String {
    indexed(prefix, "label.y", index)
}

/// One angular tick label, on a polar chart.
#[must_use]
pub fn label_a(prefix: &str, index: usize) -> String {
    indexed(prefix, "label.a", index)
}

/// One radial tick label, on a polar chart.
#[must_use]
pub fn label_r(prefix: &str, index: usize) -> String {
    indexed(prefix, "label.r", index)
}

/// A timeline lane's name.
#[must_use]
pub fn lane_label(prefix: &str, index: usize) -> String {
    format!("{prefix}.lane.{index}.label")
}

/// One span drawn in a timeline lane.
#[must_use]
pub fn lane_span(prefix: &str, lane: usize, at: usize) -> String {
    format!("{prefix}.lane.{lane}.span.{at}")
}

/// The legend as a whole.
#[must_use]
pub fn legend_root(prefix: &str) -> String {
    part(prefix, "legend")
}

/// What the legend says it could not fit.
#[must_use]
pub fn legend_overflow(prefix: &str) -> String {
    part(prefix, "legend.overflow")
}

/// One legend entry.
#[must_use]
pub fn legend_at(prefix: &str, index: usize) -> String {
    indexed(prefix, "legend", index)
}

/// One legend entry's words.
#[must_use]
pub fn legend_label(prefix: &str, index: usize) -> String {
    format!("{prefix}.legend.{index}.label")
}

/// One legend entry's colour chip.
#[must_use]
pub fn legend_swatch(prefix: &str, index: usize) -> String {
    format!("{prefix}.legend.{index}.swatch")
}

/// One distribution's median line.
#[must_use]
pub fn median(prefix: &str, index: usize) -> String {
    indexed(prefix, "median", index)
}

/// One session's open-high-low-close range.
#[must_use]
pub fn ohlc_range(prefix: &str, index: usize) -> String {
    format!("{prefix}.ohlc.{index}.range")
}

/// One named landmark of a session's range.
#[must_use]
pub fn ohlc_part(prefix: &str, index: usize, part: &str) -> String {
    format!("{prefix}.ohlc.{index}.{part}")
}

/// One datum outside a distribution's whiskers.
#[must_use]
pub fn outlier(prefix: &str, index: usize, at: usize) -> String {
    format!("{prefix}.outlier.{index}.{at}")
}

/// A polar chart's outer boundary.
#[must_use]
pub fn rim(prefix: &str) -> String {
    part(prefix, "rim")
}

/// One of a polar chart's own grid rings.
///
/// ⚠ NOT [`Overlay::ring`], which is `<prefix>.inspect.ring.<i>` — the ring
/// drawn around a mark the reader is pointing at. This is the chart's grid.
#[must_use]
pub fn ring_at(prefix: &str, index: usize) -> String {
    indexed(prefix, "ring", index)
}

/// One timeline rule.
#[must_use]
pub fn rule(prefix: &str, index: usize) -> String {
    indexed(prefix, "rule", index)
}

/// One segment of a series whose colour varies along it.
///
/// ★ Composed ON TOP OF [`series`] rather than beside it, so a segment cannot
/// drift from the series it belongs to.
#[must_use]
pub fn series_seg(prefix: &str, index: usize, at: usize) -> String {
    format!("{}.seg.{at}", series(prefix, index))
}

/// One wedge of a donut.
#[must_use]
pub fn slice(prefix: &str, index: usize) -> String {
    indexed(prefix, "slice", index)
}

/// One of a polar chart's own radial spokes.
///
/// ⚠ NOT [`Overlay::spoke`], which is `<prefix>.inspect.spoke` — the rule
/// dropped at the reader's angle. This is the chart's grid.
#[must_use]
pub fn spoke_at(prefix: &str, index: usize) -> String {
    indexed(prefix, "spoke", index)
}

/// One timeline tick.
#[must_use]
pub fn tick(prefix: &str, index: usize) -> String {
    indexed(prefix, "tick", index)
}

/// One rectangle of a treemap.
#[must_use]
pub fn tile(prefix: &str, index: usize) -> String {
    indexed(prefix, "tile", index)
}

/// One treemap rectangle's words.
#[must_use]
pub fn tile_label(prefix: &str, index: usize) -> String {
    format!("{prefix}.tile.{index}.label")
}

/// One distribution's violin outline.
#[must_use]
pub fn violin(prefix: &str, index: usize) -> String {
    indexed(prefix, "violin", index)
}

/// One end of one distribution's whisker.
#[must_use]
pub fn whisker(prefix: &str, index: usize, end: &str) -> String {
    format!("{prefix}.whisker.{index}.{end}")
}

/// One session's high wick.
#[must_use]
pub fn wick_hi(prefix: &str, index: usize) -> String {
    format!("{prefix}.wick.{index}.hi")
}

/// One session's low wick.
#[must_use]
pub fn wick_lo(prefix: &str, index: usize) -> String {
    format!("{prefix}.wick.{index}.lo")
}

/// One named end of a session's wick.
#[must_use]
pub fn wick_part(prefix: &str, index: usize, end: &str) -> String {
    format!("{prefix}.wick.{index}.{end}")
}

/// One category label under a bar chart.
#[must_use]
pub fn xlabel(prefix: &str, index: usize) -> String {
    indexed(prefix, "xlabel", index)
}

/// A sparkline's filled area.
///
/// ⚠⚠ **NOT [`area`], and the difference is an index.** A line or polar chart
/// fills one area PER SERIES (`<prefix>.area.<i>`); a sparkline has a single
/// series and fills `<prefix>.area` with no index at all. Substituting [`area`]
/// here would compile and paint a DIFFERENT address — the same trap as
/// [`Overlay::value`] against [`Overlay::value_at`], caught at R2145 while
/// converting this very site.
#[must_use]
pub fn spark_area(prefix: &str) -> String {
    part(prefix, "area")
}

/// A sparkline's trailing point.
#[must_use]
pub fn spark_end(prefix: &str) -> String {
    part(prefix, "end")
}

/// A sparkline's stroked path.
#[must_use]
pub fn spark_line(prefix: &str) -> String {
    part(prefix, "line")
}

/// A sparkline's high mark.
#[must_use]
pub fn spark_max(prefix: &str) -> String {
    part(prefix, "max")
}

/// A sparkline's low mark.
#[must_use]
pub fn spark_min(prefix: &str) -> String {
    part(prefix, "min")
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

/// The same overlay, under the part name `timeline` paints it with.
///
/// ⚠ `draw`, `legend` and `timeline` are named here WITHOUT intra-doc links:
/// they are private modules, and a public doc that links into one is a rustdoc
/// error under `-D warnings`. R2134.1 repaired the same class two rounds ago in
/// another crate, and this module reintroduced it — the rule is easy to break
/// because the link reads correctly right up until rustdoc runs.
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

    /// Every hand-composed address in `source`, as `(line, what)`.
    ///
    /// ★★★★★ R2145 — **DERIVED, where R2136 kept a list.** That round named
    /// nine needles, one per cross-file grammar, and a list is exactly what
    /// goes stale: the widening added thirty-odd families, and every one of
    /// them would have needed a row here that nobody would remember to add.
    /// The crate now composes NOTHING by hand, so the rule can be the general
    /// one — *a format string whose first placeholder is followed by a dotted
    /// address is a composition* — and it covers the family added tomorrow.
    ///
    /// ⚠ The discrimination is the dot. `format!("{v} ({pct}%)")` opens with a
    /// placeholder too; what makes a composition is `}` immediately followed by
    /// `.` and then address characters. Sentences fall out, which is why this
    /// can demand zero rather than a budget.
    ///
    /// ⚠ The opening needle is ASSEMBLED by [`concat!`] rather than written
    /// whole, because this file is the one the scan excuses and a literal
    /// `format!("{` here would be a site the gate cannot see in its own source.
    fn hand_composed(source: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (offset, _) in source.match_indices(concat!("format!(\"", "{")) {
            let rest = &source[offset..];
            let Some(close) = rest.find("\")") else {
                continue;
            };
            let body = &rest[..close];
            let Some(brace) = body.find('}') else {
                continue;
            };
            let tail = &body[brace + 1..];
            if !tail.starts_with('.') {
                continue;
            }
            let grammar: String = tail[1..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.')
                .collect();
            if grammar.is_empty() {
                continue;
            }
            found.push((source[..offset].matches('\n').count() + 1, grammar));
        }
        found
    }

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
        let mut spellers: Vec<String> = Vec::new();
        let mut scanned = 0usize;
        for (name, body) in sources.iter().filter(|(name, _)| name != "address.rs") {
            scanned += 1;
            let code: String = body
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for (line, grammar) in hand_composed(&code) {
                spellers.push(format!("{name}:{line} composes `{grammar}`"));
            }
        }
        // ★ Non-vacuity: a population of one file would make the zero below
        // free. This crate has a painter per chart kind and then some.
        assert!(
            scanned > 10,
            "the scan reached {scanned} file(s), which is not this crate — the \
             population is wrong and the assertion below means nothing"
        );
        assert_eq!(
            spellers,
            Vec::<String>::new(),
            "★★★★★ every address this crate paints is declared in `address.rs` \
             and composed through it. These site(s) compose one by hand, which \
             is a second copy of a grammar the declaration already owns — and a \
             wrong letter in it compiles, paints, and makes every query looking \
             for that mark answer nothing"
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

    /// ★★ Every `&'static str` address agrees with the composer it stands in
    /// for, so a const a reader holds cannot drift from the paint.
    ///
    /// This is the whole reason a const may spell its prefix: the spelling is
    /// checked here against the derivation, which is what R2049 established
    /// when a screen hit the same wall.
    #[test]
    fn the_const_addresses_agree_with_their_composers() {
        assert_eq!(
            super::DEFAULT_INSPECT_TOOLTIP,
            super::inspect(super::DEFAULT_PREFIX).tooltip()
        );
    }

    /// The general forms compose what the named ones do.
    #[test]
    fn the_general_forms_agree_with_the_named_ones() {
        assert_eq!(part("chart", "bg"), bg("chart"));
        assert_eq!(indexed("chart", "series", 2), series("chart", 2));
        assert_eq!(indexed("chart", "area", 2), area("chart", 2));
    }
}
