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
//! # ★ What is declared here: every family this crate paints
//!
//! R2136 opened this module at a narrower rule — *a grammar spelled by more
//! than one painter* — on the reasoning that a grammar spelled in exactly one
//! painter is already declared there. R2145 measured what that costs a READER:
//! a walk is another language in another file, so every chart grammar is
//! cross-file from its side, and the walks spell **27** distinct grammars
//! against a declaration that held **6**. A reader cannot compose an address
//! the crate never declared, however many painters spell it.
//!
//! ⇒ the unit is not "more than one painter" but **"painted at all"**, and
//! [`GRAMMAR`] is the whole of it.
//!
//! # ★★★★★ The declaration and its table are ONE literal
//!
//! Each family below is written once, as a template, inside the `composers!`
//! invocation. That single literal becomes both the function's `format!` string
//! and its row in [`GRAMMAR`] — so the table cannot drift from the composers,
//! because there is nothing to drift: they are the same token.
//!
//! [`render_grammar`] writes that table to `src/painted_grammar.tsv`, which is
//! committed. A walk is Python and can call none of this; what it can do is
//! read the artifact and format the template, and Rust's `{name}` placeholders
//! are Python's `str.format` placeholders unchanged.
//!
//! ⚠ A template that does not mention one of its own arguments is a COMPILE
//! error (`named argument never used`), so a composer cannot silently drop a
//! coordinate from an address.
//!
//! # ★★★★★ The overlay is one grammar under two names
//!
//! Eight chart kinds paint an inspect overlay and `timeline` paints
//! the same thing under the part name `playhead` — `header`, `tooltip`,
//! `value.{i}`, member for member. A needle looking for `inspect.` cannot see
//! it, which is why the count that opened this module found the timeline's
//! three spellings only after the grammar was named. [`Overlay`] therefore
//! carries the part rather than baking it in, [`OVERLAY_GRAMMAR`] leaves it as
//! a `{part}` placeholder, and [`OVERLAY_PARTS`] is what it can be.
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

/// Declare a family: one template becomes the composer AND its [`GRAMMAR`] row.
///
/// ★★★★★ R2146 — the bridge this removes is a HAND LIST. An artifact emitted
/// from a table, with the table written beside the functions, is two spellings
/// of every grammar and the second one goes stale silently; an artifact emitted
/// by CALLING each composer needs a call per composer, which is the same list
/// wearing a different hat. Passing the literal through to both is the only
/// arrangement with nothing to keep in step.
macro_rules! composers {
    ($(
        $(#[$meta:meta])*
        $name:ident ( $($arg:ident : $ty:ty),* ) = $tpl:literal ;
    )*) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $name(prefix: &str $(, $arg: $ty)*) -> String {
                format!($tpl, prefix = prefix $(, $arg = $arg)*)
            }
        )*

        /// Every family this crate paints, as `(name, template)`.
        ///
        /// The template's placeholders are `{prefix}` and the composer's own
        /// argument names. Formatting one is what a reader outside Rust does
        /// instead of spelling the address.
        pub const GRAMMAR: &[(&str, &str)] = &[ $( (stringify!($name), $tpl) ),* ];
    };
}

/// Declare an overlay member, whose template also carries `{part}`.
macro_rules! overlay_members {
    ($(
        $(#[$meta:meta])*
        $name:ident ( $($arg:ident : $ty:ty),* ) = $tpl:literal ;
    )*) => {
        impl Overlay<'_> {
            $(
                $(#[$meta])*
                #[must_use]
                pub fn $name(&self $(, $arg: $ty)*) -> String {
                    format!(
                        $tpl,
                        prefix = self.prefix,
                        part = self.part
                        $(, $arg = $arg)*
                    )
                }
            )*
        }

        /// Every member grammar an [`Overlay`] paints, as `(name, template)`.
        ///
        /// `{part}` is the overlay's own name — one of [`OVERLAY_PARTS`].
        pub const OVERLAY_GRAMMAR: &[(&str, &str)] = &[ $( (stringify!($name), $tpl) ),* ];
    };
}

/// Declare an overlay part: the constructor AND its [`OVERLAY_PARTS`] row.
macro_rules! overlay_parts {
    ($(
        $(#[$meta:meta])*
        $name:ident = $value:literal ;
    )*) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $name(prefix: &str) -> Overlay<'_> {
                Overlay { prefix, part: $value }
            }
        )*

        /// Every part name an [`Overlay`] is painted under.
        pub const OVERLAY_PARTS: &[&str] = &[ $($value),* ];
    };
}

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

composers! {
    /// The background box behind the whole chart.
    ///
    /// ★ The most-spelled grammar in the crate: ten painters, one per chart
    /// kind.
    bg() = "{prefix}.bg";

    /// The x axis rule.
    axis_x() = "{prefix}.axis.x";

    /// The y axis rule.
    axis_y() = "{prefix}.axis.y";

    /// One series' stroked path.
    series(index: usize) = "{prefix}.series.{index}";

    /// One series' filled area.
    area(index: usize) = "{prefix}.area.{index}";

    /// One datum: series `index`, point `at`.
    ///
    /// ⚠ Both coordinates are in the address because these marks are a grid
    /// rather than a list — the series alone does not name one.
    point(index: usize, at: usize) = "{prefix}.point.{index}.{at}";

    /// One part of one distribution's cap.
    ///
    /// `part` is the painter's own word for which cap this is, and arrives
    /// computed rather than as a literal — which is why it is taken rather than
    /// enumerated.
    cap(index: usize, part: &str) = "{prefix}.cap.{index}.{part}";

    /// The general form: one of this crate's parts, under a chart's prefix.
    ///
    /// ★ Public for the same reason a painter's computed part names are taken
    /// rather than enumerated: a painter can name a part this module does not
    /// declare, and one that does should compose it through here rather than
    /// reach for a `format!`, which is the whole defect.
    part(part: &str) = "{prefix}.{part}";

    /// The general indexed form: one of this crate's parts, numbered.
    indexed(part: &str, index: usize) = "{prefix}.{part}.{index}";

    /// The accessibility node for a named landmark of a chart's summary.
    ///
    /// 🟥🟥🟥★★★★★ R2145 — **this family is spelled in FOUR painters (boxplot,
    /// bar, candlestick, scatter) and R2136's own criterion would have declared
    /// it. It was missed because the inventory's character class was
    /// `[a-z_.{}]` and `a11y` CARRIES A DIGIT**, so the token did not fail the
    /// group — it failed the whole pattern, and the family never appeared in
    /// the count at all. This tree has paid for that shape before: R2130 judged
    /// `i32` a regex artifact on the same reasoning and it was a real fifth
    /// word.
    ///
    /// ⇒ a pattern is part of the claim a census makes. A family a census
    /// cannot spell is a family it reports as absent.
    a11y_at(name: &str) = "{prefix}.a11y.{name}";

    /// The accessibility node for one row of a chart's data.
    ///
    /// ⚠ The index joins with a bare `r` and NOT a separator — `…a11y.r3`, not
    /// `…a11y.r.3`. Declared here so a reader who knows this tree's dotted
    /// convention cannot apply it by reflex and find nothing.
    a11y_row(index: usize) = "{prefix}.a11y.r{index}";

    /// The accessibility node standing for a chart's series as a whole.
    a11y_series() = "{prefix}.a11y.series";

    /// The accessibility node for one series.
    a11y_series_at(index: usize) = "{prefix}.a11y.series.{index}";

    /// One bar of a bar chart.
    bar(index: usize) = "{prefix}.bar.{index}";

    /// One box of a box plot.
    ///
    /// ⚠ `box_at`, not `box`: `box` is a reserved word in Rust.
    box_at(index: usize) = "{prefix}.box.{index}";

    /// One session's candle body.
    candle(index: usize) = "{prefix}.candle.{index}";

    /// The colour scale's strip.
    colorbar_strip() = "{prefix}.colorbar.strip";

    /// One tick of the colour scale.
    colorbar_tick(index: usize) = "{prefix}.colorbar.tick.{index}";

    /// One series' in-window overdraw, filled.
    focus_area(index: usize) = "{prefix}.focus.area.{index}";

    /// One series' in-window overdraw, stroked.
    focus_series(index: usize) = "{prefix}.focus.series.{index}";

    /// One vertical gridline.
    grid_x(index: usize) = "{prefix}.grid.x.{index}";

    /// One horizontal gridline.
    grid_y(index: usize) = "{prefix}.grid.y.{index}";

    /// One minor vertical gridline.
    grid_minor_x(index: usize) = "{prefix}.grid.minor.x.{index}";

    /// One minor horizontal gridline.
    grid_minor_y(index: usize) = "{prefix}.grid.minor.y.{index}";

    /// One x-axis tick label.
    label_x(index: usize) = "{prefix}.label.x.{index}";

    /// One y-axis tick label.
    label_y(index: usize) = "{prefix}.label.y.{index}";

    /// One angular tick label, on a polar chart.
    label_a(index: usize) = "{prefix}.label.a.{index}";

    /// One radial tick label, on a polar chart.
    label_r(index: usize) = "{prefix}.label.r.{index}";

    /// A timeline lane's name.
    lane_label(index: usize) = "{prefix}.lane.{index}.label";

    /// One span drawn in a timeline lane.
    lane_span(lane: usize, at: usize) = "{prefix}.lane.{lane}.span.{at}";

    /// The legend as a whole.
    legend_root() = "{prefix}.legend";

    /// What the legend says it could not fit.
    legend_overflow() = "{prefix}.legend.overflow";

    /// One legend entry.
    legend_at(index: usize) = "{prefix}.legend.{index}";

    /// One legend entry's words.
    legend_label(index: usize) = "{prefix}.legend.{index}.label";

    /// One legend entry's colour chip.
    legend_swatch(index: usize) = "{prefix}.legend.{index}.swatch";

    /// One distribution's median line.
    median(index: usize) = "{prefix}.median.{index}";

    /// One session's open-high-low-close range.
    ohlc_range(index: usize) = "{prefix}.ohlc.{index}.range";

    /// One named landmark of a session's range.
    ohlc_part(index: usize, part: &str) = "{prefix}.ohlc.{index}.{part}";

    /// One datum outside a distribution's whiskers.
    outlier(index: usize, at: usize) = "{prefix}.outlier.{index}.{at}";

    /// A polar chart's outer boundary.
    rim() = "{prefix}.rim";

    /// One of a polar chart's own grid rings.
    ///
    /// ⚠ NOT [`Overlay::ring`], which is `<prefix>.inspect.ring.<i>` — the ring
    /// drawn around a mark the reader is pointing at. This is the chart's grid.
    ring_at(index: usize) = "{prefix}.ring.{index}";

    /// One timeline rule.
    rule(index: usize) = "{prefix}.rule.{index}";

    /// One segment of a series whose colour varies along it.
    ///
    /// ⚠ Its template repeats [`series`]' words rather than nesting the call,
    /// because a template is what makes the artifact derivable. What was
    /// structural is now asserted:
    /// `a_nested_grammar_still_starts_with_the_one_it_extends` holds this to
    /// [`series`], so the two cannot drift apart silently.
    series_seg(index: usize, at: usize) = "{prefix}.series.{index}.seg.{at}";

    /// One wedge of a donut.
    slice(index: usize) = "{prefix}.slice.{index}";

    /// One of a polar chart's own radial spokes.
    ///
    /// ⚠ NOT [`Overlay::spoke`], which is `<prefix>.inspect.spoke` — the rule
    /// dropped at the reader's angle. This is the chart's grid.
    spoke_at(index: usize) = "{prefix}.spoke.{index}";

    /// One timeline tick.
    tick(index: usize) = "{prefix}.tick.{index}";

    /// One rectangle of a treemap.
    tile(index: usize) = "{prefix}.tile.{index}";

    /// One treemap rectangle's words.
    tile_label(index: usize) = "{prefix}.tile.{index}.label";

    /// One distribution's violin outline.
    violin(index: usize) = "{prefix}.violin.{index}";

    /// One end of one distribution's whisker.
    whisker(index: usize, end: &str) = "{prefix}.whisker.{index}.{end}";

    /// One session's high wick.
    wick_hi(index: usize) = "{prefix}.wick.{index}.hi";

    /// One session's low wick.
    wick_lo(index: usize) = "{prefix}.wick.{index}.lo";

    /// One named end of a session's wick.
    wick_part(index: usize, end: &str) = "{prefix}.wick.{index}.{end}";

    /// One category label under a bar chart.
    xlabel(index: usize) = "{prefix}.xlabel.{index}";

    /// A sparkline's filled area.
    ///
    /// ⚠⚠ **NOT [`area`], and the difference is an index.** A line or polar
    /// chart fills one area PER SERIES (`<prefix>.area.<i>`); a sparkline has a
    /// single series and fills `<prefix>.area` with no index at all.
    /// Substituting [`area`] here would compile and paint a DIFFERENT address —
    /// the same trap as [`Overlay::value`] against [`Overlay::value_at`],
    /// caught at R2145 while converting this very site.
    spark_area() = "{prefix}.area";

    /// A sparkline's trailing point.
    spark_end() = "{prefix}.end";

    /// A sparkline's stroked path.
    spark_line() = "{prefix}.line";

    /// A sparkline's high mark.
    spark_max() = "{prefix}.max";

    /// A sparkline's low mark.
    spark_min() = "{prefix}.min";
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

overlay_parts! {
    /// The overlay eight chart kinds paint under the part name `inspect`.
    inspect = "inspect";

    /// The same overlay, under the part name `timeline` paints it with.
    ///
    /// ⚠ `draw`, `legend` and `timeline` are named here WITHOUT intra-doc
    /// links: they are private modules, and a public doc that links into one is
    /// a rustdoc error under `-D warnings`. R2134.1 repaired the same class two
    /// rounds before R2136, and this module reintroduced it — the rule is easy
    /// to break because the link reads correctly right up until rustdoc runs.
    playhead = "playhead";
}

overlay_members! {
    /// The overlay's own node, when the painter gives it one.
    root() = "{prefix}.{part}";

    /// The callout's heading.
    header() = "{prefix}.{part}.header";

    /// The callout's frame.
    tooltip() = "{prefix}.{part}.tooltip";

    /// The callout's value, for a chart whose overlay reads one value.
    ///
    /// ⚠ Not the same grammar as [`value_at`](Self::value_at) — see the table
    /// in this module's header for which chart kinds answer which.
    value() = "{prefix}.{part}.value";

    /// One series' row inside the callout.
    ///
    /// ⚠ Not the same grammar as [`value`](Self::value).
    value_at(index: usize) = "{prefix}.{part}.value.{index}";

    /// The mark drawn over whatever the reader is pointing at.
    highlight() = "{prefix}.{part}.highlight";

    /// The rule dropped through the plot at the reader's x.
    crosshair() = "{prefix}.{part}.crosshair";

    /// The ring drawn around one hit mark.
    ring(index: usize) = "{prefix}.{part}.ring.{index}";

    /// The filled marker drawn on one series at the reader's x.
    marker(index: usize) = "{prefix}.{part}.marker.{index}";

    /// The radial rule a polar chart drops at the reader's angle.
    spoke() = "{prefix}.{part}.spoke";

    /// A member whose name the painter computes.
    ///
    /// ★ Taken rather than enumerated: a distribution's overlay names its parts
    /// from the same vocabulary its marks use, so the set is the chart's and
    /// not this module's.
    member(member: &str) = "{prefix}.{part}.{member}";
}

impl Overlay<'_> {
    /// The member a tag names, or `None` when the tag is not this overlay's.
    #[must_use]
    pub fn member_of<'a>(&self, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(self.prefix)?
            .strip_prefix('.')?
            .strip_prefix(self.part)?
            .strip_prefix('.')
    }
}

/// The committed artifact's body: every grammar this crate paints, as text.
///
/// ★★★★★ R2146 — **a walk is Python and cannot call any of the above.** Seven
/// screens answered that by PUBLISHING their addresses on the wire, and R2143
/// measured that the same answer does not reach here: the chart examples carry
/// no introspection surface at all, and building one into twenty-two minimal
/// examples would be a spec per demo rather than a declaration per grammar.
/// What a crate can do instead is EMIT — the pin pattern, where a test
/// regenerates a tracked file and the gate is a byte comparison.
///
/// The rows are `kind<TAB>name<TAB>value`, sorted, so a diff of the artifact is
/// a diff of the grammar:
///
/// | kind | value |
/// |---|---|
/// | `const` | a whole address, already rendered |
/// | `grammar` | a template taking `prefix` and the composer's own arguments |
/// | `overlay` | a template taking `part` as well |
/// | `part` | one name `part` can be |
#[must_use]
pub fn render_grammar() -> String {
    let mut rows: Vec<(&str, &str, &str)> = vec![
        ("const", "DEFAULT_PREFIX", DEFAULT_PREFIX),
        ("const", "DEFAULT_INSPECT_TOOLTIP", DEFAULT_INSPECT_TOOLTIP),
    ];
    rows.extend(GRAMMAR.iter().map(|(name, tpl)| ("grammar", *name, *tpl)));
    rows.extend(
        OVERLAY_GRAMMAR
            .iter()
            .map(|(name, tpl)| ("overlay", *name, *tpl)),
    );
    rows.extend(OVERLAY_PARTS.iter().map(|part| ("part", *part, *part)));
    rows.sort_unstable();

    let mut out = String::from(
        "# Every address grammar `pinion-chart` paints, emitted from\n\
         # `src/address.rs`. Rewritten by setting PINION_REGEN_ADDRESS_PIN and\n\
         # running this crate's `the_committed_grammar_is_what_the_declaration\n\
         # _composes` test; do not hand-edit.\n\
         #\n\
         # kind<TAB>name<TAB>value -- const | grammar | overlay | part.\n\
         # A `grammar` value is a template: format it with `prefix` and the\n\
         # named placeholders it carries. An `overlay` template takes `part`\n\
         # too, which is one of the `part` rows.\n",
    );
    for (kind, name, value) in rows {
        out.push_str(kind);
        out.push('\t');
        out.push_str(name);
        out.push('\t');
        out.push_str(value);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_INSPECT_TOOLTIP, DEFAULT_PREFIX, GRAMMAR, OVERLAY_GRAMMAR, OVERLAY_PARTS, area,
        axis_x, axis_y, bg, cap, indexed, inspect, legend_at, legend_label, part, part_of,
        playhead, point, render_grammar, series, series_seg,
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

    /// This module's own source, cut off at its test module.
    fn declaration_source() -> String {
        let whole = include_str!("address.rs");
        let end = whole
            .find("\n#[cfg(test)]")
            .expect("this module ends with its own tests");
        whole[..end].to_owned()
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

    /// ★★★★★ R2146 — **and the declaration itself composes only through the
    /// macro**, so [`GRAMMAR`] is the WHOLE of what this crate paints.
    ///
    /// The gate above excuses this file, which is what made it possible to add
    /// a composer here by hand — and a hand-written composer is a family the
    /// emitted artifact does not carry, so every reader outside Rust goes on
    /// spelling it. The population is this module's own source and the
    /// exception is NAMED rather than left as a hole: [`render_grammar`] is the
    /// one function here that builds a `String` without being a composer.
    #[test]
    fn r2146_every_composer_goes_through_the_macro() {
        let source = declaration_source();
        let mut composing: Vec<String> = Vec::new();
        let mut string_fns: Vec<String> = Vec::new();
        // The macro DEFINITIONS are the sites that may compose — they are the
        // declaration's engine. Recognised as blocks rather than by a token on
        // the line, because rustfmt is free to put `format!(` and its template
        // on separate lines and a per-line rule would then read the engine's
        // own `format!` as a hand-written one.
        let mut in_macro = false;
        for line in source.lines() {
            if line.starts_with("macro_rules!") {
                in_macro = true;
            } else if in_macro && line == "}" {
                in_macro = false;
                continue;
            }
            if in_macro {
                continue;
            }
            if line.contains("format!(") {
                composing.push(line.trim().to_owned());
            }
            if let Some(rest) = line.trim().strip_prefix("pub fn ")
                && line.contains("-> String")
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                string_fns.push(name);
            }
        }
        assert!(
            source.lines().count() > 300,
            "the declaration read as {} line(s), which is not this module",
            source.lines().count()
        );
        // ★ And the skipped region is not the whole file: the engine's own
        // `format!` must be in the source, or the first assertion below is
        // green for having read nothing that could compose.
        assert!(
            source.contains("format!("),
            "no `format!` anywhere in the declaration — this scan is reading \
             something other than the module that composes"
        );
        assert_eq!(
            composing,
            Vec::<String>::new(),
            "★ a `format!` outside the macro is a grammar the artifact cannot \
             carry, so a walk would go on spelling it"
        );
        assert_eq!(
            string_fns,
            vec!["render_grammar".to_owned()],
            "★★★★★ every composer is declared through `composers!` or \
             `overlay_members!`, whose single literal is both the `format!` \
             string and the row a reader outside Rust is handed"
        );
    }

    /// ★★★★★ R2146 — the committed artifact is what the declaration composes.
    ///
    /// Regenerate with `PINION_REGEN_ADDRESS_PIN=1 cargo test -p pinion-chart`.
    /// The name is the workspace's one regeneration flag, declared once in
    /// `pinion_core` as `REGEN_ADDRESS_PIN`, so a round that renames an address
    /// rewrites every artifact in the campaign with one command.
    ///
    /// ⚠ **LOCALLY.** Measured at R2146: the remote build wrapper forwards no
    /// environment variable of its caller's, so a regeneration run through it
    /// does not regenerate — and the variable reaching the far side would be
    /// worse, writing the artifact onto the build host where the next sync
    /// cannot see it. The failure is loud either way (this test refuses), which
    /// is why the note is a warning rather than a guard.
    #[test]
    fn the_committed_grammar_is_what_the_declaration_composes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/painted_grammar.tsv");
        let rendered = render_grammar();
        // ★ A floor, not decoration: an empty table would otherwise agree with
        // an empty artifact, which is a gate green for having asked nothing.
        assert!(
            GRAMMAR.len() >= 55 && OVERLAY_GRAMMAR.len() >= 10 && OVERLAY_PARTS.len() == 2,
            "the declaration holds {} families, {} overlay members and {} \
             part(s), which is not this crate",
            GRAMMAR.len(),
            OVERLAY_GRAMMAR.len(),
            OVERLAY_PARTS.len()
        );
        if std::env::var_os(pinion_core::REGEN_ADDRESS_PIN).is_some() {
            std::fs::write(path, &rendered).expect("the artifact is writable");
            return;
        }
        let committed = std::fs::read_to_string(path).expect("the artifact is committed");
        assert_eq!(
            committed, rendered,
            "★★★★★ this crate's painted grammar changed. A reader outside Rust \
             formats these templates instead of spelling an address, so a \
             change here is a PUBLISHED change. If it is intended, set \
             PINION_REGEN_ADDRESS_PIN and re-run this test."
        );
    }

    /// ★ A template that nests another family still starts with it.
    ///
    /// The templates are flat literals so the artifact can be derived from
    /// them; what that gives up is the composition that used to make a segment
    /// unable to drift from its series. This is that guarantee, asserted.
    #[test]
    fn a_nested_grammar_still_starts_with_the_one_it_extends() {
        let seg = series_seg("chart", 1, 2);
        assert!(
            seg.starts_with(&series("chart", 1)),
            "{seg} does not extend {}",
            series("chart", 1)
        );
        let label = legend_label("chart", 3);
        assert!(
            label.starts_with(&legend_at("chart", 3)),
            "{label} does not extend {}",
            legend_at("chart", 3)
        );
        // ★ And not vacuously: a different index is NOT an extension, so the
        // assertions above are about the whole address and not its stem.
        assert!(!seg.starts_with(&series("chart", 2)));
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
        // ★ And the parts the artifact publishes are the parts that exist.
        assert_eq!(OVERLAY_PARTS, ["inspect", "playhead"]);
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
        assert_eq!(DEFAULT_INSPECT_TOOLTIP, inspect(DEFAULT_PREFIX).tooltip());
    }

    /// The general forms compose what the named ones do.
    #[test]
    fn the_general_forms_agree_with_the_named_ones() {
        assert_eq!(part("chart", "bg"), bg("chart"));
        assert_eq!(indexed("chart", "series", 2), series("chart", 2));
        assert_eq!(indexed("chart", "area", 2), area("chart", 2));
    }
}
