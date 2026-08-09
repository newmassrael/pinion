//! R1559 §5.36 §5.40 — the **list** format and the numbering derived from it
//! (the toolkit text list / text list format; CSS Counter Styles Level 3).
//!
//! # What a list is, and why it cannot be hand-composed
//!
//! Everything else a list has can be written by hand. The indent is a margin.
//! The bullet is a glyph. What cannot be written by hand is the **number**,
//! because an item's number is not a property of the item: it is a property of
//! the item's *place among its siblings*. Insert one item and every item after
//! it is renumbered; delete one and they renumber back; nest one and the inner
//! sequence restarts while the outer one carries on underneath it.
//!
//! So a list is the one text structure whose content is a function of the
//! sequence, and that is what this module derives. The author states which
//! blocks are items and at what depth; the marker each one shows is computed
//! from that ([`crate::text_list::number_blocks`]).
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has the same concept and reaches it through a different door:
//! text list is a text block group **owned by a text document**, so a list
//! cannot exist outside a document, membership is by object identity, and
//! `itemNumber()` / `itemText()` are in-process C++ calls on that
//! object. Four things here go past it:
//!
//! * **The counter styles have RANGES, and outside them a marker falls back**
//!   rather than becoming a question mark. Roman numerals have no standard
//!   form past 3999 — a fact about the notation, not about any toolkit — so
//!   the toolkit's `itemText()` answers `"?"` there and the reader loses the number
//!   entirely. CSS Counter Styles Level 3 says a counter style outside its
//!   range renders through its *fallback* style, and predefined
//!   alphabetic/roman styles fall back to `decimal`. So item 4000 of an
//!   upper-roman list reads `4000.` here and `?.` in the toolkit, and
//!   [`RenderedMarker::rendered_as`] names which style actually produced the
//!   text.
//! * **A bullet is text.** the toolkit's text document layout draws `ListDisc` /
//!   `ListCircle` / `ListSquare` as painted geometry — an ellipse or a
//!   rectangle — so a toolkit API answers what an unordered item's marker looks
//!   like, it does not participate in text layout, and it cannot be copied,
//!   searched or announced. Here every style renders to a string
//!   ([`ListStyle::render`]), so one code path draws every marker, on both
//!   backends (§2 #6), and the terminal is not a special case.
//! * **The suffix's default is a property of the STYLE.** the toolkit keeps
//!   `numberSuffix()` as a string whose null state means "use the default",
//!   which is a distinction a string cannot show a reader.
//!   [`ListFormat::number_suffix`] is an `Option`, and what `None` resolves to
//!   is [`ListStyle::default_suffix`] — `"."` after a number, nothing after a
//!   bullet.
//! * **The numbering is data.** `itemNumber()` is a C++ call; the derivation
//!   here rides the painted scene ([`ListPlacement`]) and is published by
//!   `scene/text_lists`, so an agent can read a document's numbering without
//!   pixels and without being in-process (§2 #7).
//!
//! # What this module does NOT decide
//!
//! Where a marker is *painted* is the composing view's business
//! (`pinion_widget_paint::document`), and which blocks are grouped into one
//! list is [`number_blocks`]'s. This module owns the vocabulary and the
//! per-value rendering.

/// The marker vocabulary of a list — the toolkit `Style`, whose eight
/// arms are exactly CSS's `disc` / `circle` / `square` / `decimal` /
/// `lower-alpha` / `upper-alpha` / `lower-roman` / `upper-roman`.
///
/// The two families behave differently and the type says which is which
/// ([`Self::is_ordered`]): an **unordered** style shows the same glyph for
/// every item and ignores the counter, an **ordered** style renders the
/// counter and therefore has a range outside which it cannot.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ListStyle {
    /// A filled round bullet, CSS `disc` (`U+2022 BULLET`). The default, as it
    /// is for HTML `<ul>`.
    #[default]
    Disc,
    /// A hollow round bullet, CSS `circle` (`U+25E6 WHITE BULLET`).
    Circle,
    /// A filled square bullet, CSS `square` (`U+25AA BLACK SMALL SQUARE`).
    Square,
    /// Arabic numerals, CSS `decimal`. The only style with no range: every
    /// counter value has a decimal form, which is why it is what everything
    /// else falls back to.
    Decimal,
    /// `a`, `b`, … `z`, `aa`, `ab`, … — CSS `lower-alpha`.
    LowerAlpha,
    /// `A`, `B`, … `Z`, `AA`, `AB`, … — CSS `upper-alpha`.
    UpperAlpha,
    /// `i`, `ii`, `iii`, … — CSS `lower-roman`.
    LowerRoman,
    /// `I`, `II`, `III`, … — CSS `upper-roman`.
    UpperRoman,
}

/// The largest value Roman numerals have a standard form for.
///
/// Not a limit of this implementation: the notation itself stops here.
/// `MMMCMXCIX` is 3999, and 4000 needs a vinculum (an overline multiplying by
/// a thousand) that no plain-text sequence of `I V X L C D M` can express. CSS
/// Counter Styles Level 3 states the same bound as the predefined roman
/// styles' `range`.
const ROMAN_MAX: i32 = 3999;

/// The subtractive Roman table, largest first — the standard form, so 4 is
/// `IV` rather than `IIII` and 900 is `CM` rather than `DCCCC`.
const ROMAN_TABLE: [(i32, &str); 13] = [
    (1000, "M"),
    (900, "CM"),
    (500, "D"),
    (400, "CD"),
    (100, "C"),
    (90, "XC"),
    (50, "L"),
    (40, "XL"),
    (10, "X"),
    (9, "IX"),
    (5, "V"),
    (4, "IV"),
    (1, "I"),
];

/// How many letters the alphabetic styles cycle through.
const ALPHABET: i32 = 26;

impl ListStyle {
    /// Whether this style renders the item's counter (rather than the same
    /// glyph for every item).
    ///
    /// The one place the two families are told apart, so a caller never
    /// pattern-matches three bullet arms for itself and then forgets one when
    /// a fourth arrives.
    #[must_use]
    pub const fn is_ordered(self) -> bool {
        match self {
            Self::Disc | Self::Circle | Self::Square => false,
            Self::Decimal
            | Self::LowerAlpha
            | Self::UpperAlpha
            | Self::LowerRoman
            | Self::UpperRoman => true,
        }
    }

    /// The glyph an unordered style shows, or `None` for an ordered one.
    ///
    /// The characters are CSS Counter Styles Level 3's own: `disc` is `U+2022`, `circle` is
    /// `U+25E6`, `square` is `U+25AA`. The toolkit draws these three as geometry inside text
    /// document layout instead, which is why a toolkit bullet has no string
    /// form to return here.
    #[must_use]
    pub const fn bullet(self) -> Option<&'static str> {
        match self {
            Self::Disc => Some("\u{2022}"),
            Self::Circle => Some("\u{25e6}"),
            Self::Square => Some("\u{25aa}"),
            Self::Decimal
            | Self::LowerAlpha
            | Self::UpperAlpha
            | Self::LowerRoman
            | Self::UpperRoman => None,
        }
    }

    /// What follows the marker when [`ListFormat::number_suffix`] states
    /// nothing: `"."` after a counter, nothing after a bullet.
    ///
    /// CSS's predefined styles put `". "` after `decimal` and `" "` after
    /// `disc`; the trailing space is dropped here because the marker sits in
    /// its own gutter box and the gap is that box's, not the string's. A
    /// trailing space inside an end-aligned marker would push the glyph off
    /// its own right edge — the separation belongs to the layout.
    #[must_use]
    pub const fn default_suffix(self) -> &'static str {
        if self.is_ordered() { "." } else { "" }
    }

    /// The inclusive range of counter values this style can represent, or
    /// `None` when it represents all of them.
    ///
    /// * bullets — `None`: the glyph does not depend on the value at all;
    /// * `decimal` — `None`: every integer has a decimal form;
    /// * alphabetic — `1 ..= i32::MAX`: the bijective base-26 sequence starts
    ///   at `a` and has no zero or negative member;
    /// * roman — `1 ..= 3999`, the largest value Roman numerals have a
    ///   standard form for.
    #[must_use]
    pub const fn range(self) -> Option<(i32, i32)> {
        match self {
            Self::Disc | Self::Circle | Self::Square | Self::Decimal => None,
            Self::LowerAlpha | Self::UpperAlpha => Some((1, i32::MAX)),
            Self::LowerRoman | Self::UpperRoman => Some((1, ROMAN_MAX)),
        }
    }

    /// Whether [`Self::render`] can represent `value`.
    #[must_use]
    pub const fn represents(self, value: i32) -> bool {
        match self.range() {
            None => true,
            Some((lo, hi)) => value >= lo && value <= hi,
        }
    }

    /// The style used when this one cannot represent a value — CSS Counter
    /// Styles Level 3's `fallback` descriptor, which the predefined
    /// alphabetic and roman styles set to `decimal`.
    ///
    /// A style that represents everything is its own fallback, so the
    /// resolution always terminates and a caller never has to bound the walk.
    #[must_use]
    pub const fn fallback(self) -> Self {
        if self.range().is_none() {
            self
        } else {
            Self::Decimal
        }
    }

    /// The counter representation of `value` in this style, or `None` when
    /// this style has no form for it.
    ///
    /// The representation only — [`ListFormat::marker`] adds the prefix and
    /// the suffix, because those belong to the list and this belongs to the
    /// notation.
    #[must_use]
    pub fn render(self, value: i32) -> Option<String> {
        if let Some(glyph) = self.bullet() {
            return Some(glyph.to_owned());
        }
        if !self.represents(value) {
            return None;
        }
        Some(match self {
            Self::Decimal => value.to_string(),
            Self::LowerAlpha => alphabetic(value, b'a'),
            Self::UpperAlpha => alphabetic(value, b'A'),
            Self::LowerRoman => roman(value).to_lowercase(),
            Self::UpperRoman => roman(value),
            // Handled by the bullet short-circuit above; restated rather than
            // wildcarded so a new arm has to be classified here.
            Self::Disc | Self::Circle | Self::Square => return None,
        })
    }

    /// The wire spelling, and the only one (the [`crate::style::TextAlign`]
    /// rule: two hand-written copies of a mapping is one that drifts).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Disc => "Disc",
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Decimal => "Decimal",
            Self::LowerAlpha => "LowerAlpha",
            Self::UpperAlpha => "UpperAlpha",
            Self::LowerRoman => "LowerRoman",
            Self::UpperRoman => "UpperRoman",
        }
    }
}

/// `value` in the bijective base-26 sequence starting at `first` (`a` or `A`).
///
/// Bijective, not positional: there is no zero digit, so 26 is `z` and 27 is
/// `aa` rather than `ba`. That is CSS's `alphabetic` system and the toolkit's
/// `ListLowerAlpha` alike; a positional base-26 would make item 26 read `ba`.
fn alphabetic(value: i32, first: u8) -> String {
    debug_assert!(value >= 1, "the alphabetic range starts at 1");
    let mut out = Vec::new();
    let mut n = value;
    while n > 0 {
        n -= 1;
        let digit = u8::try_from(n % ALPHABET).unwrap_or(0);
        out.push(first + digit);
        n /= ALPHABET;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `value` as an upper-case Roman numeral in standard subtractive form.
fn roman(value: i32) -> String {
    debug_assert!(
        (1..=ROMAN_MAX).contains(&value),
        "the roman range is 1..=3999",
    );
    let mut out = String::new();
    let mut n = value;
    for (weight, numeral) in ROMAN_TABLE {
        while n >= weight {
            out.push_str(numeral);
            n -= weight;
        }
    }
    out
}

/// The gutter a list inserts between its container's start edge and its items'
/// text — the HTML user-agent stylesheet's `padding-inline-start: 40px` on
/// `<ul>` and `<ol>`, and the width the marker is laid out in.
pub const DEFAULT_INDENT_PX: u32 = 40;

/// A list's declared format — the toolkit text list format.
///
/// # This is the list's identity
///
/// The toolkit identifies a list by object: two text lists are different lists
/// because they are different objects, whatever they declare. Here a list is a
/// *run* of consecutive item blocks at one depth, and this format is what
/// tells one run from the next — changing the style, the start, the affixes or
/// the indent between two adjacent items starts a second list, and the second
/// one begins again at its own [`Self::start`].
///
/// That is not a pinion invention: it is the rule every document format with
/// lists already uses. `CommonMark` starts a new list when the bullet character
/// or the delimiter changes, and HTML expresses two adjacent lists as two
/// elements. See [`number_blocks`] for the whole grouping rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ListFormat {
    /// The marker vocabulary. The toolkit `setStyle`.
    pub style: ListStyle,
    /// The counter value of the list's first item. The toolkit `setStart` (the
    /// toolkit 6.6+), HTML `<ol start>`.
    ///
    /// Signed, and free to be zero or negative: `decimal` represents those and
    /// the styles that do not fall back to it, so a list can be numbered from
    /// `0` without the type refusing what CSS allows.
    pub start: i32,
    /// Text before the counter. The toolkit `setNumberPrefix`.
    pub number_prefix: String,
    /// Text after the counter, or `None` for [`ListStyle::default_suffix`].
    ///
    /// `Some(String::new())` is a real answer distinct from `None` — "no suffix" rather than "the
    /// style's". The toolkit spells the same distinction as a null string
    /// versus an empty one, which is invisible in a debugger and in any
    /// serialization.
    pub number_suffix: Option<String>,
    /// The width of the gutter the marker is laid out in, and the distance a
    /// nested list is inset by. The toolkit `setIndent`, in units of
    /// `indentWidth`; px here, for the reason
    /// [`crate::style::BlockFormat`] gives — one unit, so a number read off a
    /// format says what it measures.
    pub indent_px: u32,
}

impl ListFormat {
    /// A list of `style`, numbered from 1, with the style's default affixes and
    /// the default indent.
    #[must_use]
    pub fn new(style: ListStyle) -> Self {
        Self {
            style,
            start: 1,
            number_prefix: String::new(),
            number_suffix: None,
            indent_px: DEFAULT_INDENT_PX,
        }
    }

    /// An unordered list of round bullets — CSS `disc`, HTML `<ul>`.
    #[must_use]
    pub fn bulleted() -> Self {
        Self::new(ListStyle::Disc)
    }

    /// An ordered list of arabic numerals — CSS `decimal`, HTML `<ol>`.
    #[must_use]
    pub fn numbered() -> Self {
        Self::new(ListStyle::Decimal)
    }

    /// Builder: the counter value of the first item (HTML `<ol start>`).
    #[must_use]
    pub fn with_start(mut self, start: i32) -> Self {
        self.start = start;
        self
    }

    /// Builder: text before the counter.
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.number_prefix = prefix.into();
        self
    }

    /// Builder: text after the counter, overriding
    /// [`ListStyle::default_suffix`].
    #[must_use]
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.number_suffix = Some(suffix.into());
        self
    }

    /// Builder: the marker gutter's width in px.
    #[must_use]
    pub fn with_indent_px(mut self, px: u32) -> Self {
        self.indent_px = px;
        self
    }

    /// The suffix this list actually shows — the declared one, or the style's.
    #[must_use]
    pub fn suffix(&self) -> &str {
        self.number_suffix
            .as_deref()
            .unwrap_or_else(|| self.style.default_suffix())
    }

    /// The counter value of the item at 1-based `position` in this list.
    #[must_use]
    pub fn ordinal(&self, position: u32) -> i32 {
        let offset = i32::try_from(position.saturating_sub(1)).unwrap_or(i32::MAX);
        self.start.saturating_add(offset)
    }

    /// Render the marker for counter `value`: prefix, the counter in this
    /// style (or in its fallback), then the suffix.
    ///
    /// The prefix and suffix are applied whatever the style, because CSS
    /// treats a bullet as a counter representation like any other and a
    /// binding that wants parenthesised bullets should not need a second
    /// mechanism to get them.
    #[must_use]
    pub fn marker(&self, value: i32) -> RenderedMarker {
        let (text, rendered_as) = if let Some(text) = self.style.render(value) {
            (text, self.style)
        } else {
            let fallback = self.style.fallback();
            (
                fallback
                    .render(value)
                    // `fallback()` never returns a style with a range, so this
                    // arm is unreachable; answering with the decimal form
                    // rather than panicking keeps a marker derivation total
                    // for every `i32`.
                    .unwrap_or_else(|| value.to_string()),
                fallback,
            )
        };
        RenderedMarker {
            text: format!("{}{text}{}", self.number_prefix, self.suffix()),
            rendered_as,
        }
    }
}

impl Default for ListFormat {
    fn default() -> Self {
        Self::bulleted()
    }
}

/// A marker, and the style that actually produced it.
///
/// The pair rather than the string, because they differ exactly where a reader
/// needs to know: an upper-roman list's item 4000 shows `4000.`, and without [`Self::rendered_as`]
/// that is indistinguishable from a list that declared `decimal`. The toolkit has no
/// way to report it — its `itemText()` answers `"?"` and says nothing about why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMarker {
    /// The marker as painted: prefix, counter, suffix.
    pub text: String,
    /// The style whose notation produced the counter — the declared style, or
    /// its CSS fallback when the declared one has no form for the value.
    pub rendered_as: ListStyle,
}

/// Where a painted block sits in the document's list structure — the derived
/// half, computed by [`number_blocks`] and carried on the painted
/// [`TextNode`](crate::scene::TextNode).
///
/// # Why the list's own facts are repeated on every item
///
/// A scene has one carrier for a text declaration — the text node — and the
/// a11y pass and the `scene/text_lists` census both read leaves. So a list's
/// [`Self::count`], its [`Self::format`] and its parent are restated on each
/// of its items rather than held once somewhere neither surface can see them.
/// They cannot disagree: one derivation writes all of them, and the census
/// folds them back into a single row per list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ListPlacement {
    /// The paint tag of the list this item belongs to.
    pub list_tag: String,
    /// The paint tag of the list this one is nested inside, or `None` at the
    /// top level.
    pub parent_list_tag: Option<String>,
    /// Nesting depth: `0` is a top-level list. The toolkit's `indent`
    /// counts from 1 and doubles as the indent multiplier; these are separate
    /// here ([`ListFormat::indent_px`] carries the distance).
    pub level: u8,
    /// The counter value this item was numbered with —
    /// [`ListFormat::start`] plus its offset in the list.
    pub ordinal: i32,
    /// 1-based position among the list's items (the toolkit `itemNumber() + 1`), which is
    /// what `aria-posinset` reports.
    pub position: u32,
    /// How many items the list has (the toolkit `count()`), which is what
    /// `aria-setsize` reports.
    pub count: u32,
    /// The marker as painted (the toolkit `itemText()`, which has no answer for the
    /// unordered styles).
    pub marker: String,
    /// The style that produced [`Self::marker`], after the CSS range fallback.
    pub rendered_as: ListStyle,
    /// The list's declared format, kept beside the numbering it produced for
    /// [`crate::style::BlockFormat`]'s reason: a marker string cannot be read
    /// back as the declaration that made it.
    pub format: ListFormat,
}

/// A block's declared list membership — the authored half: which depth, and
/// under what format.
///
/// Grouping consecutive members into lists, and numbering them, is
/// [`number_blocks`]'s job. An author never states a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSpec {
    /// The format the enclosing list declares.
    pub format: ListFormat,
    /// Nesting depth; `0` is a top-level list.
    pub level: u8,
}

impl ListSpec {
    /// Membership of a top-level list with `format`.
    #[must_use]
    pub fn new(format: ListFormat) -> Self {
        Self { format, level: 0 }
    }

    /// Builder: the nesting depth (`0` is top level).
    #[must_use]
    pub fn at_level(mut self, level: u8) -> Self {
        self.level = level;
        self
    }
}

/// One list the numbering discovered — the object the toolkit calls a text
/// list.
///
/// Reported beside the per-item placements because a list has facts that are
/// not any item's: its extent, its depth, and which list encloses it. A
/// composing view rebuilds the document's nesting from these
/// ([`ListRun::parent_tag`] is walkable to the root), and an empty run is a
/// real answer — a document that nests from depth 0 straight to depth 2 has a
/// list at depth 1 with no items of its own, exactly as `<ul><ul>` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRun {
    /// The list's paint tag, as the caller's `list_tag` function named it.
    pub tag: String,
    /// The enclosing list's tag, or `None` at the top level.
    pub parent_tag: Option<String>,
    /// Nesting depth; `0` is a top-level list.
    pub level: u8,
    /// The format every item of this list shares — the run's identity.
    pub format: ListFormat,
    /// How many items it holds.
    pub count: u32,
}

/// What [`number_blocks`] derived: one placement per item block, and one run
/// per list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListNumbering {
    /// Parallel to the input: entry `i` is `Some` exactly when block `i`
    /// declared membership.
    pub placements: Vec<Option<ListPlacement>>,
    /// Every list, in the order the walk discovered them (so a run's index is
    /// the `k` its tag was named with).
    pub runs: Vec<ListRun>,
}

impl ListNumbering {
    /// The run with `tag`, or `None` when no list has it.
    #[must_use]
    pub fn run(&self, tag: &str) -> Option<&ListRun> {
        self.runs.iter().find(|run| run.tag == tag)
    }

    /// The tags of `tag`'s enclosing lists and then `tag` itself, outermost
    /// first — the chain a view opens to place an item.
    ///
    /// Empty when no list has `tag`. Bounded by the run count rather than
    /// trusting the parent links to be acyclic, because a walk that can loop
    /// on malformed input is a hang rather than a wrong answer.
    #[must_use]
    pub fn ancestry(&self, tag: &str) -> Vec<&str> {
        let mut chain = Vec::new();
        let mut cursor = self.run(tag);
        while let Some(run) = cursor {
            chain.push(run.tag.as_str());
            if chain.len() > self.runs.len() {
                break;
            }
            cursor = run.parent_tag.as_deref().and_then(|p| self.run(p));
        }
        chain.reverse();
        chain
    }
}

/// Number a document's blocks: turn a flat sequence of optional
/// [`ListSpec`]s into one [`ListPlacement`] per item.
///
/// `specs[i]` is block `i`'s membership, `None` for an ordinary paragraph.
/// `list_tag(k)` names the `k`-th list the derivation discovers, in document
/// order, so the caller owns the tag vocabulary
/// ([`DocumentTag`](crate::composite_tag::DocumentTag) supplies it) and this
/// function owns the structure.
///
/// # The grouping rule
///
/// Walking the blocks in order, with a stack of open lists (one per depth):
///
/// * an **ordinary block** closes every open list — a paragraph between two
///   items ends the first list, as it does in HTML and in `CommonMark`;
/// * an item at a **shallower** depth closes every deeper list, and a deeper
///   list therefore restarts if one opens again later;
/// * an item at the **same** depth as an open list continues it when the
///   formats are equal, and starts a new one when they differ
///   ([`ListFormat`] is the list's identity);
/// * an item **deeper** than the open stack opens a list at each intervening
///   depth, so a document that jumps from depth 0 to depth 2 is well-formed
///   rather than a panic or a silent flattening. The intervening lists have no
///   items of their own, which is exactly what an HTML `<ul><ul>` produces.
///
/// A **deeper list does not interrupt a shallower one**: the outer list's next
/// item continues its own numbering, which is the property that makes nesting
/// usable and the one an author cannot maintain by hand.
///
/// [`ListNumbering::placements`] is parallel to `specs`: entry `i` is `Some`
/// exactly when `specs[i]` is.
#[must_use]
pub fn number_blocks(
    specs: &[Option<ListSpec>],
    list_tag: impl Fn(usize) -> String,
) -> ListNumbering {
    /// One open list while the walk is inside it.
    struct Open {
        /// Discovery order — lists close innermost-first, which is not the
        /// order they opened in, so the run order is restored from this.
        index: usize,
        tag: String,
        parent_tag: Option<String>,
        level: u8,
        format: ListFormat,
        /// Indices into the output of the items placed in this list so far,
        /// so the final `count` can be written back once it is known.
        items: Vec<usize>,
    }

    let mut stack: Vec<Open> = Vec::new();
    let mut out: Vec<Option<ListPlacement>> = vec![None; specs.len()];
    let mut next_list = 0usize;
    // Every list the walk has closed, so `count` can be stamped at the end
    // rather than guessed while the list is still growing.
    let mut closed: Vec<Open> = Vec::new();

    for (i, spec) in specs.iter().enumerate() {
        let Some(spec) = spec else {
            closed.append(&mut stack);
            continue;
        };
        let depth = usize::from(spec.level);
        // Close every list deeper than this item.
        while stack.len() > depth + 1 {
            if let Some(open) = stack.pop() {
                closed.push(open);
            }
        }
        // A same-depth list with a different format is a different list.
        let format_changed =
            stack.len() == depth + 1 && stack.last().is_some_and(|open| open.format != spec.format);
        if format_changed {
            if let Some(open) = stack.pop() {
                closed.push(open);
            }
        }
        // Open a list at every depth from here down to the item's.
        while stack.len() <= depth {
            let index = next_list;
            let tag = list_tag(index);
            next_list += 1;
            let parent_tag = stack.last().map(|open| open.tag.clone());
            let level = u8::try_from(stack.len()).unwrap_or(u8::MAX);
            stack.push(Open {
                index,
                tag,
                parent_tag,
                level,
                format: spec.format.clone(),
                items: Vec::new(),
            });
        }

        let parent_list_tag = stack.last().and_then(|open| open.parent_tag.clone());
        let Some(open) = stack.last_mut() else {
            continue;
        };
        let position = u32::try_from(open.items.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        let ordinal = open.format.ordinal(position);
        let marker = open.format.marker(ordinal);
        out[i] = Some(ListPlacement {
            list_tag: open.tag.clone(),
            parent_list_tag,
            level: spec.level,
            ordinal,
            position,
            // Provisional: stamped with the list's real length below, because
            // a list does not know how long it is until it ends.
            count: 0,
            marker: marker.text,
            rendered_as: marker.rendered_as,
            format: open.format.clone(),
        });
        open.items.push(i);
    }
    closed.append(&mut stack);

    let mut runs = Vec::with_capacity(closed.len());
    for open in closed {
        let count = u32::try_from(open.items.len()).unwrap_or(u32::MAX);
        for i in open.items {
            if let Some(placement) = out.get_mut(i).and_then(Option::as_mut) {
                placement.count = count;
            }
        }
        runs.push((
            open.index,
            ListRun {
                tag: open.tag,
                parent_tag: open.parent_tag,
                level: open.level,
                format: open.format,
                count,
            },
        ));
    }
    runs.sort_by_key(|(index, _)| *index);
    let runs = runs.into_iter().map(|(_, run)| run).collect();
    ListNumbering {
        placements: out,
        runs,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ListFormat, ListNumbering, ListPlacement, ListSpec, ListStyle, ROMAN_MAX, number_blocks,
    };

    fn tag(k: usize) -> String {
        format!("l{k}")
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the input type IS Option<ListSpec>; unwrapping it here would                   make every call site restate `Some(...)`"
    )]
    fn spec(style: ListStyle, level: u8) -> Option<ListSpec> {
        Some(ListSpec::new(ListFormat::new(style)).at_level(level))
    }

    fn markers(out: &[Option<ListPlacement>]) -> Vec<String> {
        out.iter()
            .map(|p| p.as_ref().map_or_else(String::new, |p| p.marker.clone()))
            .collect()
    }

    /// The eight styles render what their notation says, at the boundaries
    /// that are easy to get wrong: bijective base-26 has no zero digit, and
    /// the roman table is subtractive.
    #[test]
    fn each_style_renders_its_own_notation() {
        assert_eq!(ListStyle::Decimal.render(7).as_deref(), Some("7"));
        assert_eq!(ListStyle::LowerAlpha.render(1).as_deref(), Some("a"));
        assert_eq!(ListStyle::LowerAlpha.render(26).as_deref(), Some("z"));
        assert_eq!(ListStyle::LowerAlpha.render(27).as_deref(), Some("aa"));
        assert_eq!(ListStyle::UpperAlpha.render(28).as_deref(), Some("AB"));
        assert_eq!(ListStyle::UpperRoman.render(4).as_deref(), Some("IV"));
        assert_eq!(ListStyle::UpperRoman.render(1990).as_deref(), Some("MCMXC"));
        assert_eq!(
            ListStyle::LowerRoman.render(ROMAN_MAX).as_deref(),
            Some("mmmcmxcix"),
        );
        assert_eq!(ListStyle::Disc.render(99).as_deref(), Some("\u{2022}"));
        assert_eq!(ListStyle::Square.render(-4).as_deref(), Some("\u{25aa}"));
    }

    /// The counterfactual for the range: a style REFUSES what it cannot
    /// represent, and only the styles with a range refuse anything.
    #[test]
    fn a_style_refuses_what_its_notation_cannot_write() {
        assert_eq!(ListStyle::UpperRoman.render(ROMAN_MAX + 1), None);
        assert_eq!(ListStyle::LowerRoman.render(0), None);
        assert_eq!(ListStyle::LowerAlpha.render(0), None);
        assert_eq!(ListStyle::UpperAlpha.render(-1), None);
        assert!(ListStyle::Decimal.render(-99).is_some());
        assert!(ListStyle::Decimal.render(i32::MAX).is_some());
        assert!(ListStyle::Disc.render(i32::MIN).is_some());
    }

    /// CSS's fallback rule, which is what the toolkit's `"?"` gives up: the
    /// number survives, and the report says which notation wrote it.
    #[test]
    fn an_out_of_range_value_falls_back_to_decimal() {
        let fmt = ListFormat::new(ListStyle::UpperRoman);
        let within = fmt.marker(ROMAN_MAX);
        assert_eq!(within.text, "MMMCMXCIX.");
        assert_eq!(within.rendered_as, ListStyle::UpperRoman);
        let beyond = fmt.marker(ROMAN_MAX + 1);
        assert_eq!(beyond.text, "4000.", "the value is not lost");
        assert_eq!(
            beyond.rendered_as,
            ListStyle::Decimal,
            "and the report names the notation that wrote it",
        );
    }

    /// The suffix's default belongs to the style; an explicit empty suffix is
    /// a different answer from an absent one.
    #[test]
    fn the_suffix_default_comes_from_the_style() {
        assert_eq!(ListFormat::numbered().suffix(), ".");
        assert_eq!(ListFormat::bulleted().suffix(), "");
        assert_eq!(ListFormat::numbered().with_suffix("").suffix(), "");
        assert_eq!(ListFormat::numbered().with_suffix(")").marker(3).text, "3)");
        assert_eq!(
            ListFormat::numbered().with_prefix("§").marker(3).text,
            "§3.",
        );
    }

    /// The defining property: an item's number is its place among its
    /// siblings, so inserting one renumbers every item after it and none
    /// before it.
    #[test]
    fn inserting_an_item_renumbers_the_ones_after_it() {
        let before = vec![spec(ListStyle::Decimal, 0); 3];
        assert_eq!(
            markers(&number_blocks(&before, tag).placements),
            ["1.", "2.", "3."]
        );
        let mut after = before.clone();
        after.insert(1, spec(ListStyle::Decimal, 0));
        assert_eq!(
            markers(&number_blocks(&after, tag).placements),
            ["1.", "2.", "3.", "4."],
        );
        let placements = number_blocks(&after, tag).placements;
        for p in placements.iter().flatten() {
            assert_eq!(p.count, 4, "and every item knows the new length");
        }
    }

    /// A nested list restarts, and does NOT interrupt the list it is nested
    /// in — the property that makes nesting usable.
    #[test]
    fn a_nested_list_restarts_without_interrupting_its_parent() {
        let specs = vec![
            spec(ListStyle::Decimal, 0),
            spec(ListStyle::Disc, 1),
            spec(ListStyle::Disc, 1),
            spec(ListStyle::Decimal, 0),
        ];
        let numbering = number_blocks(&specs, tag);
        let out = &numbering.placements;
        assert_eq!(
            markers(out),
            ["1.", "\u{2022}", "\u{2022}", "2."],
            "the outer list carries on under the inner one",
        );
        let outer = out[0].as_ref().expect("an item");
        let inner = out[1].as_ref().expect("an item");
        assert_eq!(outer.count, 2);
        assert_eq!(inner.count, 2);
        assert_eq!(inner.level, 1);
        assert_eq!(
            inner.parent_list_tag.as_deref(),
            Some(outer.list_tag.as_str())
        );
        assert_eq!(outer.parent_list_tag, None);
        assert_ne!(outer.list_tag, inner.list_tag);
    }

    /// A paragraph between two runs ends the first list, so the second starts
    /// again at its own `start` — HTML's rule and `CommonMark`'s.
    #[test]
    fn an_ordinary_paragraph_ends_a_list() {
        let specs = vec![
            spec(ListStyle::Decimal, 0),
            spec(ListStyle::Decimal, 0),
            None,
            spec(ListStyle::Decimal, 0),
        ];
        let numbering = number_blocks(&specs, tag);
        let out = &numbering.placements;
        assert_eq!(markers(out), ["1.", "2.", "", "1."]);
        assert_eq!(out[0].as_ref().expect("an item").count, 2);
        assert_eq!(out[3].as_ref().expect("an item").count, 1);
        assert_ne!(
            out[0].as_ref().expect("an item").list_tag,
            out[3].as_ref().expect("an item").list_tag,
        );
    }

    /// The format IS the list's identity: a changed style at the same depth
    /// starts a second list, which begins again.
    #[test]
    fn a_changed_format_starts_a_second_list() {
        let specs = vec![
            spec(ListStyle::Decimal, 0),
            spec(ListStyle::Decimal, 0),
            spec(ListStyle::LowerAlpha, 0),
        ];
        let numbering = number_blocks(&specs, tag);
        let out = &numbering.placements;
        assert_eq!(markers(out), ["1.", "2.", "a."]);
        assert_ne!(
            out[1].as_ref().expect("an item").list_tag,
            out[2].as_ref().expect("an item").list_tag,
        );
    }

    /// A declared start offsets the whole sequence, and `position` stays the
    /// structural fact — the two differ exactly here.
    #[test]
    fn a_declared_start_offsets_the_counter_not_the_position() {
        let fmt = ListFormat::numbered().with_start(5);
        let specs = vec![Some(ListSpec::new(fmt.clone())); 3];
        let numbering = number_blocks(&specs, tag);
        let out = &numbering.placements;
        assert_eq!(markers(out), ["5.", "6.", "7."]);
        let third = out[2].as_ref().expect("an item");
        assert_eq!(third.position, 3, "third of three");
        assert_eq!(third.ordinal, 7, "numbered seven");
    }

    /// A jump past a depth is well-formed: the intervening list opens with no
    /// items, exactly as `<ul><ul>` nests in HTML.
    #[test]
    fn a_skipped_depth_opens_an_empty_intervening_list() {
        let specs = vec![spec(ListStyle::Disc, 2)];
        let numbering = number_blocks(&specs, tag);
        let out = &numbering.placements;
        let only = out[0].as_ref().expect("an item");
        assert_eq!(only.level, 2);
        assert_eq!(only.position, 1);
        assert_eq!(only.count, 1);
        assert_eq!(
            only.parent_list_tag.as_deref(),
            Some("l1"),
            "nested under the list opened at the intervening depth",
        );
    }

    /// A document with no items derives nothing — the negative control that
    /// separates "numbered the items" from "numbered every block".
    #[test]
    fn a_document_with_no_items_derives_nothing() {
        let out = number_blocks(&[None, None], tag);
        assert!(out.placements.iter().all(Option::is_none));
        assert!(out.runs.is_empty(), "and no list was discovered");
        assert_eq!(number_blocks(&[], tag), ListNumbering::default());
    }

    /// The runs report each list once, in discovery order, with the nesting a
    /// composing view rebuilds the document from. Discovery order is asserted
    /// past ten lists because lists CLOSE innermost-first, so restoring the
    /// order by sorting their tags as strings would put `l10` before `l2`.
    #[test]
    fn the_runs_report_each_list_once_in_discovery_order() {
        let mut specs = Vec::new();
        for _ in 0..12 {
            specs.push(spec(ListStyle::Decimal, 0));
            specs.push(spec(ListStyle::Disc, 1));
            specs.push(None);
        }
        let numbering = number_blocks(&specs, tag);
        assert_eq!(
            numbering.runs.len(),
            24,
            "two lists per group, twelve times"
        );
        let names: Vec<&str> = numbering.runs.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(&names[..4], ["l0", "l1", "l2", "l3"]);
        assert_eq!(&names[20..], ["l20", "l21", "l22", "l23"]);
        let deep = numbering.run("l21").expect("the nested list");
        assert_eq!(deep.level, 1);
        assert_eq!(deep.count, 1);
        assert_eq!(deep.parent_tag.as_deref(), Some("l20"));
        assert_eq!(numbering.ancestry("l21"), ["l20", "l21"]);
        assert_eq!(numbering.ancestry("l20"), ["l20"]);
        assert!(numbering.ancestry("nosuch").is_empty());
    }
}
