//! R1656 §5.32 §5.36 §2 #7 — **did what the frame painted stay inside the box
//! it was promised?**
//!
//! # The half-fact this closes
//!
//! A rectangle in a scene is a *promise*: this mark will be drawn here, in this
//! much room. Every read in this tree reports the promise. Nothing reported
//! whether it was kept, so a screen could paint a label across the row below it
//! — or past the edge of the card that owns it — and describe itself as
//! correct to every gate, every test and every agent.
//!
//! That is not hypothetical. Measured on the analysis-tool screen at the size
//! it opens in: **seven of its eight node cards paint their last field row
//! outside the card**, three to five pixels below the border, and one link
//! label reaches eighty-two pixels past the right edge. A person saw it
//! immediately. The tree had four checks that could each have caught it and
//! none of them asked this question:
//!
//! * `scene/snapshot` reports the promised rectangles, which are all correct —
//!   the *ink* is what left the box, and the ink is not in the scene.
//! * `scene/text_painted`'s `overflows` compares a run's ink against **its own**
//!   box, which is a different question and answers `true` for 124 of 157 runs
//!   on that screen: an authored line box a few pixels shorter than the shaped
//!   line box is near-universal and benign, so the flag cannot discriminate.
//! * the smear gate groups runs by their nearest **tagged ancestor**, and a
//!   painter that places a card's contents as *siblings* of the card (which is
//!   what an absolutely-positioned canvas painter does) gives every one of them
//!   the canvas as their ancestor. "Is this run inside its owner" then means
//!   "is it inside the canvas", which is true of everything.
//! * `scene/pointer_reach` asks whether a widget can be pressed, which a run
//!   that escapes its card usually still can be.
//!
//! # The owner is the parent, and that is a demand on the tree
//!
//! A node is drawn inside its parent, because that is what a box means. That
//! is the whole rule, and it is only as honest as the tree: a painter that
//! emits `[Container(card), Text(id), Text(field), …]` as one flat sibling list
//! has thrown the containment away, and no walk can recover it.
//!
//! ★ A second rule was written and then **measured and deleted**. Tags in this
//! tree are addresses, so `lab.node.T-01.id` appears to say in its own name
//! that it is part of `lab.node.T-01`, and judging a mark against its longest
//! tag-prefix would have caught a flat painter without any restructuring. Run
//! against the real screen it produced two findings and **both were false**:
//! `lab.toolbar.zoom.out` is the button *beside* the readout `lab.toolbar.zoom`,
//! not content inside it. A dotted tag expresses grouping, not containment, and
//! a rule built on that reads a naming habit as a geometric promise.
//!
//! So the repair for a flat painter is to stop being one — to put the
//! containment back in the scene, where §2 #7 says the description of the
//! screen lives. This module then sees it. The limit is stated rather than
//! papered over: **a scene that lies about its structure gets a clean report
//! here**, and the round that found this fixed the painter rather than adding a
//! second channel to work around it.
//!
//! # Clipped is not better than smeared
//!
//! [`Fate::Smeared`] is a mark drawn on top of whatever is next to it.
//! [`Fate::Clipped`] is a mark whose overhang an enclosing clip cut away. They
//! are reported separately because the repairs differ — one is a layout fix,
//! the other is a policy choice (elide, wrap, scroll) — but neither is
//! acceptable silently: a clip turns "this label is too long" into "this label
//! ends here", and the reader cannot tell the difference.
//!
//! # What the reference toolkit can answer, measured at 6.11
//!
//! The geometry aggregates exist and the ink one does not. A widget's
//! `childrenRect` is the union of its children's **geometry** (measured: a
//! label given a 60x14 box reports 60x14 there while its text measures 251x17),
//! and a scene item's `childrenBoundingRect` likewise reports boxes. Ink is
//! available only one call at a time, from a font-metrics helper the caller has
//! to remember to invoke against a rectangle it has to fetch separately.
//! Nothing compares the two, nothing warns when a child leaves its parent, and
//! a child that does is silently clipped to it — `visibleRegion` reports the
//! *survivor*, not the loss. An external driver cannot ask at all.
//!
//! Here the whole answer is a pure function of the painted scene plus one
//! measurement closure, so it is one wire read, a paint-time warning, and a
//! gate every surface pays at boot.
//!
//! # Why the measurement is a closure
//!
//! The same reason [`crate::text_elide`] takes one: how wide a string is has
//! two legitimate answers in this project — shaped pixels on the GPU path,
//! terminal cells on the §2 #6 dual — and they cannot be reconciled. The
//! *policy* (which box owns which mark, and what counts as escaping it) is
//! shared here; the metric is handed in.

use std::collections::HashMap;

use crate::scene::{Rect, Scene, TextNode};
use crate::style::{Border, BorderPlacement, Chrome, ChromeEdge, ChromeRole};

/// The height a box must have to hold one line of a `px` face without the
/// glyphs leaving it — a **reservation**, not a measurement.
///
/// ★ R1656 — this exists because the commonest authoring mistake this module
/// catches is a box authored at the *font size*. A shaped line box is taller
/// than the face it holds (ascent, descent and leading are all above and below
/// the em), so a 12px label in a 12px box overflows by construction, and that
/// is why `scene/text_painted`'s "the ink is bigger than the box" answered
/// `true` for 124 of 157 runs on the first screen it was pointed at.
///
/// Deliberately **conservative** — it reserves more than any face this project
/// ships needs. That direction is the safe one: reserving too much wastes a
/// pixel, reserving too little paints over the neighbour. A caller that wants
/// the exact number for a real face asks the shaper
/// (`pinion_text::LayoutCache::ink_size`), which the wire read does; this is
/// for a view function, which is sync and has no cache.
#[must_use]
pub const fn line_box(px: u32) -> u32 {
    px * 3 / 2 + 2
}

/// How many pixels short of holding its own text a run's box is. Zero when it
/// is tall enough.
///
/// # This is a different question from the rest of this module
///
/// Everything else here asks **did a mark leave the box that owns it** — a
/// mark against its *parent*. This asks whether a run's own box was authored
/// tall enough for the face the run is set in, which is a mark against
/// *itself*, and the two do not imply each other: a toolbar is easily big
/// enough to hold a 13-pixel button whose own label box is five pixels too
/// short, so `scene/containment` answers *escapes 0* while the descender of
/// every `g` in it is destroyed. A reader reported exactly that twice, eleven
/// days apart, and between the two reports this tree had every number needed
/// to answer them and no predicate that asked.
///
/// # It needs no font, and that is the point
///
/// `line_box` is a reservation computed from the face size alone, so this is a
/// pure function of the scene: no shaper, no host font, no measured ink. That
/// makes it usable where the escape check is not — in a sync `view` function,
/// at boot, and in a gate that cannot disagree between this machine and CI
/// because there is nothing machine-dependent in it.
///
/// # Multi-line
///
/// A box must hold one line box per visual line. `lines` is the measured
/// sidecar and is `0` before any shape pass, which is read as one line: the
/// floor of the demand rather than a guess at it, so an un-laid-out tree is
/// judged conservatively instead of arbitrarily.
#[must_use]
pub const fn short_by(text: &TextNode) -> u32 {
    let lines = if text.line_count == 0 {
        1
    } else {
        text.line_count
    };
    let needs = line_box(text.style.font_size_px).saturating_mul(lines);
    needs.saturating_sub(text.rect.h)
}

/// A box that holds one line of a `px` face — so [`short_by`] of a run placed
/// in it is `0` by construction.
///
/// ★★★★★ R1800 — the rule and the way to satisfy it, in one module, because
/// **the measurement said the rule was the problem**. Pointed at the screen
/// whose clipped descender a reader reported, [`short_boxes`] answered **289 of
/// 290 runs**: not 289 authoring slips but one convention, applied almost
/// everywhere, that never consulted the face. The framework has owned
/// `line_box` since R1656 and exactly one production site in this tree sizes
/// anything with it.
///
/// ⚠ That denominator was measured only because the gate was made to print it.
/// This doc said "289 of 289" first — a numerator with a guessed denominator,
/// written into five files before the closing audit caught it. Two runs on that
/// screen do hold their text.
///
/// A constant nobody can reach for is a constant nobody uses. Reaching for this
/// is easier than writing a number, which is the only reliable way a rule gets
/// kept — the alternative is a gate that scolds 289 times and a person who
/// turns it off.
#[must_use]
pub const fn line_rect(x: u32, y: u32, w: u32, px: u32) -> Rect {
    Rect::new(x, y, w, line_box(px))
}

/// The same box, centred vertically inside `outer`.
///
/// The second half of the same defect: a run's vertical position in this tree
/// is a hand-picked offset too, so a box can be tall enough and still sit low
/// enough to look wrong. Five chips measured on one screen were placed with a
/// `+4` where centring the box wanted `3` and centring the ink wanted `2`, and
/// the reader's words for it were "the text is all pushed to the bottom".
///
/// Takes `outer` rather than a height so the caller cannot centre in the wrong
/// thing by transposing two arguments.
#[must_use]
pub const fn line_rect_in(outer: Rect, x: u32, w: u32, px: u32) -> Rect {
    band_in(outer, x, w, line_box(px))
}

/// ★★★★★ R1874 — **several lines stacked in ONE seat**, each tall enough for
/// its own face, the stack centred in the seat as a single block.
///
/// # The gap this fills, and how it was found
///
/// [`line_rect_in`] answers *one line in a seat*, and the thing this tree
/// actually paints over and over is *two* — a name over its gist, a title over
/// its subtitle, a heading over the row it heads. There was no element for
/// that, so every such pair is two hand-picked offsets, and **two offsets that
/// nothing relates cannot be relied on to agree**: R1873 measured a column
/// heading sitting one pixel below the cells of its own column for exactly that
/// reason, and the node palette's eight role rows carry the same shape with a
/// `+6` and a `+20` in a 40-pixel row.
///
/// Reaching for this is easier than writing two numbers, which is the only
/// reliable way a rule gets kept — the argument [`line_rect`] already makes,
/// one line count further on.
///
/// # Centred as a BLOCK, and rounded once
///
/// The stack's own height is `line_box` of each face summed, and the block is
/// centred with [`band_in`]'s rule — from the seat's centre, rounding **once**
/// — so a seat holding one line through this function and a seat holding one
/// line through [`line_rect_in`] get the same rectangle. That equality is
/// asserted rather than assumed; it is what makes this a generalisation of
/// `line_rect_in` instead of a second, subtly different, answer to the same
/// question. R1873's lesson: a second author of a rule is how the rule breaks.
///
/// # ⚠ A stack taller than its seat
///
/// The lines are laid out from the top of the seat and are allowed to run past
/// its bottom, rather than being squeezed. A caller whose seat is too small has
/// a layout defect, and the containment gates in this module are what report
/// it — silently shrinking the lines would make the boxes short of their faces
/// again, which is the whole class this module exists to remove.
///
/// ```
/// use pinion_core::containment::{line_box, line_rect_in, stacked_line_rects};
/// use pinion_core::scene::Rect;
///
/// let row = Rect::new(10, 100, 160, 40);
/// let [name, gist] = stacked_line_rects(row, row.x + 20, 140, [12, 10]);
/// assert_eq!(name.h, line_box(12));
/// assert_eq!(gist.h, line_box(10));
/// assert_eq!(gist.y, name.y + name.h);
/// // One line is exactly `line_rect_in`.
/// let [only] = stacked_line_rects(row, row.x, 140, [12]);
/// assert_eq!(only, line_rect_in(row, row.x, 140, 12));
/// ```
#[must_use]
pub fn stacked_line_rects<const N: usize>(outer: Rect, x: u32, w: u32, px: [u32; N]) -> [Rect; N] {
    let heights = px.map(line_box);
    let total: u32 = heights.iter().copied().fold(0, u32::saturating_add);
    // `band_in`'s rule, applied to the block: the seat's own centre, rounded
    // once, so a one-line stack lands exactly where `line_rect_in` puts it.
    let mut y = (outer.y + outer.h / 2).saturating_sub(total / 2);
    let mut out = [Rect::new(x, 0, w, 0); N];
    for (slot, h) in out.iter_mut().zip(heights) {
        *slot = Rect::new(x, y, w, h);
        y = y.saturating_add(h);
    }
    out
}

/// ★★★★★ R1862 — a band of height `h`, centred vertically inside `outer`.
///
/// [`line_rect_in`] is this with the height taken from the face, and splitting
/// them is what lets **a run and something that is not a run share a centre**.
///
/// # The defect this comes from
///
/// A legend row 18 pixels tall placed an 11-pixel pin sample and a 12-pixel
/// label with the *same hand-picked* `+3`. Two heights, one offset: the pin's
/// centre landed at `+8.5` and the label's at `+9`, and a reader reported the
/// words as not lining up with the box beside them. The offsets were not wrong
/// by inspection — each is what somebody would pick — they were **unrelated**,
/// and nothing that is unrelated can be relied on to agree.
///
/// Derived from the seat, two elements of a row centre on the same line by
/// construction, whatever their heights are. That is the property; the numbers
/// are a consequence of it, which is the direction that survives an edit.
///
/// # ⚠ Placed from the seat's CENTRE, not from its remaining space
///
/// The obvious spelling — `outer.y + (outer.h - h) / 2`, equal margins above
/// and below — **does not have the property this exists for**, and a
/// counterfactual is what said so: on an 18-pixel row it puts an 11-pixel band
/// at centre 8 and a 12-pixel one at centre 9, because two integer divisions
/// round independently. That one pixel is exactly the defect the pin legend was
/// reported for, so a derivation with it in would have been the bug with extra
/// steps, and the gate written against it had to allow a pixel and could then
/// no longer see the thing it was for.
///
/// Centring on `outer.y + outer.h / 2` rounds **once**, so every band of a seat
/// shares one centre exactly and the gates can demand equality.
///
/// ⚠ The cost, stated: the margins above and below can differ by one where the
/// parities differ. Equal margins and a shared centre are different properties
/// on an integer grid and only one of them is what alignment means.
///
/// The mature toolkits this project is judged against reach it with a
/// horizontal layout whose alignment is a per-child flag. What differs here is
/// that the answer is a **rectangle the caller receives**, so a painter that
/// draws by rectangle — which is what an introspectable scene needs — can hold
/// the property without a layout pass to consult, and a gate can read the
/// result out of the paint rather than out of the flag that asked for it.
#[must_use]
pub const fn band_in(outer: Rect, x: u32, w: u32, h: u32) -> Rect {
    Rect::new(x, centre_line(outer).saturating_sub(h / 2), w, h)
}

/// ★★★★★ R1956 — **the centre line of a box**, stated in the one place that
/// gets to say what that means.
///
/// [`band_in`] places a box ON this line; [`uncentred`] reads it back off the
/// paint. Spelling it once is what makes those two **one rule asked at two
/// moments** rather than two oracles free to disagree — and the disagreement is
/// not hypothetical here, because the naive `outer.y + (outer.h - h) / 2`
/// rounds a second time and lands a box one pixel off the line this returns.
///
/// # Why this being exact is what lets the gate be exact
///
/// Because [`band_in`] is written in terms of this, `centre_line(band_in(seat,
/// …))` is `centre_line(seat)` for every height — the `h / 2` subtracted at
/// placement is the same `h / 2` added back when the centre is read. So a gate
/// comparing two boxes of a seat can demand **equality**, with no allowance. A
/// second spelling would have forced a one-pixel tolerance, which is exactly
/// the size of the defect R1862 was built from: a tolerance as large as the
/// fault cannot see the fault.
#[must_use]
pub const fn centre_line(rect: Rect) -> u32 {
    rect.y + rect.h / 2
}

/// ★★★★★ R1956 — **a run's box centred ON a line**, where [`line_rect_in`]
/// centres it in a *seat*.
///
/// The case is a label against something that has a position rather than an
/// extent: an axis tick, a lane's middle, a marker's pixel. Written by hand it
/// is `line - px / 2 - 1`, and **that is not the same number** — the box is
/// [`line_box`] tall, not `px` tall, so the hand-spelled offset misses by
/// however much the shaped line box exceeds the face. Measured: a latency
/// chart's five y-tick labels each sat one pixel below the grid line they name,
/// with the axis and a bar reported alongside them because they are drawn from
/// the same tick.
///
/// Two sites spelled it — `chart::draw::y_tick_labels` and
/// `chart::timeline`'s lane names — which is why this is a derivation rather
/// than a repair at each.
///
/// ```
/// use pinion_core::containment::{centre_line, line_rect_on};
///
/// // The box straddles the line it was given, whatever the face.
/// assert_eq!(centre_line(line_rect_on(740, 432, 46, 10)), 740);
/// assert_eq!(centre_line(line_rect_on(740, 432, 46, 17)), 740);
/// ```
#[must_use]
pub const fn line_rect_on(line: u32, x: u32, w: u32, px: u32) -> Rect {
    band_on(line, x, w, line_box(px))
}

/// [`band_in`] against a **line** rather than a seat — a band of height `h`
/// straddling `line`.
///
/// [`line_rect_on`] is this with the height taken from the face, and splitting
/// them is what lets a caller that owns a different box-height rule keep it
/// while still sharing the centring. `pinion-chart` is that caller: its label
/// box is `size + 4`, which is not [`line_box`], and forcing the two together
/// would move every chart label in the tree for a reason that has nothing to do
/// with centring.
///
/// ⚠ That the two box-height rules differ at all is a separate finding and is
/// **not** repaired here.
#[must_use]
pub const fn band_on(line: u32, x: u32, w: u32, h: u32) -> Rect {
    // A band of no height has the line itself as its centre, so this is
    // `band_in`'s rule with the seat collapsed onto the line — one spelling of
    // the arithmetic, not a second.
    band_in(Rect::new(x, line, w, 0), x, w, h)
}

/// One run whose own box cannot hold it, as [`short_boxes`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortBox {
    /// The run's tag, when it carries one.
    pub tag: Option<String>,
    /// The path to it, for a reader who has to find it.
    pub path: Vec<String>,
    /// What it says.
    pub content: String,
    /// The box as authored, in its scroll frame.
    pub rect: Rect,
    /// The face size the run is set in.
    pub px: u32,
    /// Visual lines as the scene records them, `0` when no shape pass has run.
    /// Reported verbatim rather than normalised, so a reader can tell "one
    /// line" from "nobody has measured yet".
    pub lines: u32,
    /// The height the box needed.
    pub needs: u32,
    /// `needs - rect.h`, always positive here.
    pub short_by: u32,
}

impl ShortBox {
    /// The address a reader follows to reach this run.
    ///
    /// ★ R1870 — the tag when it carries one, and otherwise the **whole path**
    /// rather than its last segment. Measured on a real boot, the last segment
    /// of an untagged run's path is the run's *position among its siblings*, so
    /// the first line of the warning this feeds named its subject `2`. A
    /// position is not an address: `2` is unreachable, `packets/1/2` resolves.
    ///
    /// The empty string is possible in principle and only at a scene whose root
    /// is itself a text run — the walk gives the root an empty path, and a
    /// caller with nowhere to send a reader is told so in words.
    #[must_use]
    pub fn address(&self) -> String {
        match self.tag.as_deref() {
            Some(tag) => tag.to_owned(),
            None if self.path.is_empty() => "<an untagged box>".to_owned(),
            None => self.path.join("/"),
        }
    }

    /// The [repeating site](repeating_site) this run sits at.
    #[must_use]
    pub fn site(&self) -> String {
        repeating_site(&self.address())
    }
}

/// One address with its **positions** folded away, so the runs that are one
/// authoring mistake repeated read as one site.
///
/// ★★★★★ R1870 — this exists because a bound on lines is only half of what a
/// reader needs. Measured R1870 on the analysis-tool shell's dashboard: **eight
/// of the warning's ten lines went to one table's cells**, so the lines a reader
/// can act on restated a single authoring mistake and every other kind of short
/// box on that frame reached them only inside a count. Folding positions is what
/// lets the budget be spent on *kinds*.
///
/// ⚠ **Ask for the quantities; do not read them here.** They differ per
/// destination and the queued repair campaign exists to change them:
///
/// ```text
/// cargo test -p hello-analyzer-shell r1870_the_short_box_census -- --nocapture
/// ```
///
/// That census is here because the rule caught this very paragraph: R1870's
/// first draft wrote figures read off a log by hand, and re-measuring them in
/// the same round found **every quantity wrong** while the shape held.
///
/// ★ It also records something the hand reading could not see: **the pile-up is
/// not proportional to size.** The largest single site in the whole application
/// is a packet list's cells, at more than a hundred runs — and on that
/// destination the replaced ordering's worst concentration was three lines,
/// falling on a *different* site entirely. Its pile-up was an artefact of walk
/// order, which is precisely what grouping removes.
///
/// # What counts as a position
///
/// A segment made of nothing but digits and the punctuation an index is spelled
/// with (`_`, `-`) is a position, and so is a `#<digits>` suffix inside a named
/// segment. Both are *which one*, never *what*. Everything else survives
/// verbatim — `ipv4` keeps its `4`, because the digit there is part of a name
/// and folding it would put `ipv4` and `ipv6` at one site.
///
/// Both separators this tree spells an address with (`.` in a tag, `/` in a
/// path) are segment boundaries, and the separator itself is preserved, so a
/// site reads back as the address family it stands for:
/// `card.packet#0.cell.0_2` → `card.packet#*.cell.*`.
///
/// # ⚠ Which way it errs
///
/// Folding too eagerly merges two genuinely different sites and hides one of
/// them; folding too little is exactly the defect above. The rule above is the
/// narrow one on purpose: it folds only what is syntactically a position, so a
/// name that merely contains a digit is never merged with another name.
#[must_use]
pub fn repeating_site(address: &str) -> String {
    let mut out = String::with_capacity(address.len());
    let mut start = 0usize;
    for (i, ch) in address.char_indices() {
        if ch == '.' || ch == '/' {
            push_site_segment(&mut out, &address[start..i]);
            out.push(ch);
            start = i + ch.len_utf8();
        }
    }
    push_site_segment(&mut out, &address[start..]);
    out
}

/// One segment of [`repeating_site`], with a position folded to `*`.
///
/// # ★★★★★ R1879 — a segment has THREE shapes, and this used to know two
///
/// 1. **a pure position** (`3_2`) — all digits and separators. Folds whole.
/// 2. **an index glued to a name** (`0_direction`) — folds its INDEX only, to
///    `*_direction`. A table's rows collapse and its columns stay apart, which
///    is *more* information than shape 1 yields, not less.
/// 3. **a digit inside a name** (`ipv4`, `l0`, `v0x09`) — folds NOTHING.
///
/// Shape 2 had no arm here, so `kp.list.cell.0_direction` was its own site and
/// two whole tables — 107 runs, measured at R1878 — reached the census as 107
/// separate one-run sites, unspellable by a warning that ranks sites by how
/// many runs they hold.
///
/// ⚠ **The rule is not "fold anything containing a digit."** That merges
/// `proto.ipv4` with `proto.ipv6`, which is this defect's mirror image: an
/// over-fold hides two real sites inside one line, and a reader cannot tell
/// that from a correct fold. The index therefore has to be *delimited* — a run
/// of leading digits followed by `_` and then a name — so a name that merely
/// starts with a digit (`0direction`) stays a name.
fn push_site_segment(out: &mut String, seg: &str) {
    let is_position = !seg.is_empty()
        && seg
            .chars()
            .all(|c| c.is_ascii_digit() || c == '_' || c == '-')
        && seg.chars().any(|c| c.is_ascii_digit());
    if is_position {
        out.push('*');
        return;
    }
    // Shape 2. The leading digits are the index; everything from the `_` on is
    // the name and is kept, so the column survives the fold that retires the
    // row. Only ONE run of digits is folded: `1_2_name` becomes `*_2_name`,
    // which collapses the row without guessing that the next number is a
    // position too.
    let index = seg.len() - seg.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if index > 0 {
        let name = &seg[index..];
        if name.len() > 1 && name.starts_with('_') {
            out.push('*');
            out.push_str(name);
            return;
        }
    }
    if let Some(hash) = seg.rfind('#') {
        let digits = &seg[hash + 1..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            out.push_str(&seg[..hash]);
            out.push_str("#*");
            return;
        }
    }
    out.push_str(seg);
}

/// Every run in the scene whose own box is too short for the face it is set in.
///
/// Reports the *amount* per run rather than a count, for the reason
/// [`Overhang`] carries four numbers instead of a boolean: R1656 measured the
/// nearest available flag — `scene/text_painted`'s `overflows` — as true for
/// 124 of 157 runs on the first screen it was aimed at, and abandoned the axis
/// because a signal that fires on four fifths of a screen cannot discriminate.
/// That measurement was of *ink against the box*, where a one-pixel overshoot
/// is the shaper being one pixel more generous than the author reserved. This
/// asks the authoring question instead, so a run is short only when its box
/// could not have held the line under any shaping.
///
/// No clip is folded in, deliberately: a box authored too short is authored too
/// short whether or not something downstream then hides the evidence.
#[must_use]
pub fn short_boxes(scene: &Scene) -> Vec<ShortBox> {
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Scene::Text(t) = visit.node else {
            return;
        };
        // A run with no box makes no promise about holding anything.
        if t.rect.h == 0 || t.rect.w == 0 {
            return;
        }
        let short = short_by(t);
        if short == 0 {
            return;
        }
        let lines = if t.line_count == 0 { 1 } else { t.line_count };
        found.push(ShortBox {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            content: t.content.clone(),
            rect: t.rect,
            px: t.style.font_size_px,
            lines: t.line_count,
            needs: line_box(t.style.font_size_px).saturating_mul(lines),
            short_by: short,
        });
    });
    found
}

/// Whether a cut at the bottom of this text's box would **show**.
///
/// ★★★★★ R1863 — the difference between *a box is short* and *a reader can see
/// that it is*, which R1798 measured on one screen as **270 runs against 51**.
/// A box one pixel short of a line of capitals looks exactly like a box that
/// fits; the same box under a `g` loses the tail, and that is what three
/// separate reports from the same person were about.
///
/// # ⚠ An approximation, and it is named as one
///
/// Without the font there is no exact answer — a face can put a descender on a
/// glyph this list does not know, and a face can have none at all. So this
/// errs toward **saying yes**: the letters below are the ones that descend in
/// every Latin face this tree ships, the punctuation is the marks that hang,
/// and anything non-ASCII is unknown and therefore counted. What it must never
/// do is answer *no* for a run whose cut a reader would see, because that is
/// the direction that loses a defect silently.
///
/// # This is a PRIORITY, never a permission
///
/// A warning path may say these first. It may not use this to fall silent about
/// the rest: a run whose box is short is short whatever letters are in it, and
/// the count of the others belongs in the same breath. Ordering what a reader
/// hears is not the same act as deciding what they are told.
#[must_use]
pub fn cut_would_show(content: &str) -> bool {
    /// Latin letters whose ink goes below the baseline, and the marks that hang
    /// below it. `Q` is here for its tail.
    const BELOW_THE_BASELINE: &str = "gjpqyQ,;()[]{}/\\@$_";
    content
        .chars()
        // Anything non-ASCII is a script this list does not speak for, and the
        // safe answer there is "a reader might see it".
        .any(|c| !c.is_ascii() || BELOW_THE_BASELINE.contains(c))
}

/// How far a mark reached past the box that owns it, per edge, in pixels.
///
/// Four numbers rather than one boolean because the boolean was measured
/// useless: on the screen this module was written for, "the ink is bigger than
/// the box" is true of 79% of runs. *How much* and *which edge* is what tells a
/// three-pixel line-box rounding from a row painted over its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overhang {
    /// Pixels past the owner's left edge.
    pub left: u32,
    /// Pixels past the owner's top edge.
    pub top: u32,
    /// Pixels past the owner's right edge.
    pub right: u32,
    /// Pixels past the owner's bottom edge.
    pub bottom: u32,
}

impl Overhang {
    /// Nothing escaped.
    pub const NONE: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    /// How far `inner` reaches past `outer` on each edge.
    #[must_use]
    pub fn of(inner: Rect, outer: Rect) -> Self {
        Self {
            left: outer.x.saturating_sub(inner.x),
            top: outer.y.saturating_sub(inner.y),
            right: (inner.x + inner.w).saturating_sub(outer.x + outer.w),
            bottom: (inner.y + inner.h).saturating_sub(outer.y + outer.h),
        }
    }

    /// True when the mark stayed inside.
    #[must_use]
    pub const fn is_contained(&self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }

    /// The largest single-edge overhang — what a budget counts and what a
    /// tolerance compares against.
    #[must_use]
    pub const fn worst(&self) -> u32 {
        let a = if self.left > self.top {
            self.left
        } else {
            self.top
        };
        let b = if self.right > self.bottom {
            self.right
        } else {
            self.bottom
        };
        if a > b { a } else { b }
    }
}

/// What happened to the part that did not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// Nothing cut it: it is painted on top of whatever is beside the owner.
    Smeared,
    /// An enclosing clip removed it. The reader loses the content with no mark
    /// that anything was removed — which is why this is reported rather than
    /// forgiven.
    Clipped,
}

impl Fate {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Smeared => "smeared",
            Self::Clipped => "clipped",
        }
    }
}

/// One painted mark that did not stay inside the box that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    /// The mark's own tag, when it has one. A text run usually does not, which
    /// is exactly why this class stayed invisible: every other gate here is
    /// tag-keyed.
    pub tag: Option<String>,
    /// The mark's address, as `scene/locate` reports it.
    pub path: Vec<String>,
    /// The tag of the box it escaped, or `<untagged>` when that box has no tag.
    pub owner: String,
    /// What the mark holds, for a text run — the string a reader is losing.
    pub content: Option<String>,
    /// Window-absolute box the scene promised the mark.
    pub promised: Rect,
    /// Window-absolute extent actually painted: the shaped ink for a text run,
    /// the promised box for anything else.
    pub painted: Rect,
    /// Window-absolute box of the owner.
    pub owner_rect: Rect,
    /// How far past each edge of the owner the paint reached.
    pub over: Overhang,
    /// Whether the overhang is cut by a clip or drawn over the neighbour.
    pub fate: Fate,
    /// R1674 — **which parts of the owner** the mark landed on: its outer edge,
    /// its border, or a named chrome band. Never empty for an escape, because
    /// leaving the content rectangle means landing on at least one of them.
    pub trespass: Vec<Trespass>,
}

/// How wide and tall a text run's glyphs actually are, in the caller's unit.
///
/// Handed in rather than computed here for the reason the module header gives:
/// the GPU path measures shaped pixels and the §2 #6 terminal measures cells,
/// and no single answer is right for both.
pub type InkOf<'a> = &'a mut dyn FnMut(&TextNode) -> (u32, u32);

/// Where a text run's glyphs sit **inside the rectangle the run was given**,
/// and how large they are.
///
/// ★★★★★ R1904 — [`InkOf`] answers *how big*, and until this round nothing
/// answered *where*. Every derivation that needed a position took the ink to
/// begin at the run's own `rect.x`, which is true for a run laid out flush and
/// false for one that declared an alignment with room to move — the case
/// `r1780_an_alignment_moves_a_run_within_the_width_it_was_given` measures. A
/// person reading the running window reported exactly that gap: a byte not
/// centred in its box, on a screen whose every rectangle said it was.
///
/// ⚠ **This is not [`escapes`]'s question, and that is why `escapes` keeps
/// [`InkOf`].** An aligned run cannot leave a box a flush one would have stayed
/// inside — but *not* for the reason it first reads: "the alignment width is
/// the box" is only half of it, since a run WIDER than its box has negative
/// room and centring half of a negative number would push the glyphs out the
/// near side too. The half that actually settles it is that an overflowing line
/// is not aligned at all, and that is a property of the shaper rather than an
/// argument, so it is performed rather than asserted here:
/// `pinion_text::cache::r1904_an_overflowing_run_is_not_moved_by_its_alignment`.
/// Overflow is the shaper's business and the overflow policy's; *position
/// within the box* is this one's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkSpan {
    /// How far right of the run's own rectangle the first glyph inks — what an
    /// alignment moved it by, and `0` for a run laid out flush.
    pub dx: u32,
    /// Shaped width of the glyphs, in the caller's unit.
    pub w: u32,
    /// Shaped height of the glyphs, in the caller's unit.
    pub h: u32,
}

/// Where a text run's glyphs sit inside the rectangle it was given, in the
/// caller's unit.
///
/// Handed in for [`InkOf`]'s reason: the GPU path measures shaped pixels and
/// the §2 #6 terminal measures cells, and no single answer is right for both.
pub type InkSpanOf<'a> = &'a mut dyn FnMut(&TextNode) -> InkSpan;

/// Where what a box holds sits on one axis, relative to that box's centre.
///
/// ★★★★★ R1904 — **two arms, because a box smaller than its content has no
/// centre to be off.** The first draft answered a bare `i64`, and the margins
/// are computed with `saturating_sub`, so an overflowing box reported both
/// margins as `0` and therefore `0` — *perfectly centred*, the best answer
/// there is, for the one case where the question does not apply.
///
/// That is an escape hatch disabling its own gate: whatever a caller does with
/// the number, the unanswerable case reads as the ideal. The round's own screen
/// gate happened to close it by asserting room first — but that was a caller
/// remembering, not the type refusing, and the next caller does not remember.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centring {
    /// The content is inside the box, this far from its centre: the near
    /// margin less the far one. `0` is centred; negative means the content
    /// sits **left of** / **above** centre — the sign a reader uses saying "it
    /// is too far left".
    Within(i64),
    /// The content is larger than the box on this axis, so it has no centre to
    /// be measured against. Whether that is a defect is [`escapes`]'s question.
    Overflows,
}

/// How far what a box holds sits from that box's centre, per axis.
///
/// ★ Each axis is derived from the two margins rather than from one of them,
/// because "centred" is a claim about the PAIR: a left margin alone says
/// nothing without the right one to compare against, and a check written on one
/// margin passes for any box wide enough.
///
/// ⚠⚠ **The two axes are not the same question, and only [`x`](Self::x) is
/// answerable from ink.** R1874's rule — *width is measured by ink, height by
/// the line box* — is why: a string with no descender inks two or three pixels
/// short of the bottom of the line it sits on, so [`y`](Self::y) reports it
/// high of centre when it is exactly where the shaper puts it. Vertical
/// centring is [`line_rect_in`]'s to state, not this one's to check; `y` is
/// published because a caller comparing two runs on the same line has a use for
/// it, not as a floor anything should be held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffCentre {
    /// Horizontal, measured by ink.
    pub x: Centring,
    /// Vertical. ⚠ See the type's own warning: this is ink against a line box,
    /// and a run with no descender is not off centre for reporting a value.
    pub y: Centring,
}

/// A node's **content** rectangle: its box, less any border it draws inside it.
///
/// The distinction CSS calls border box versus content box. A child is judged
/// against this rather than against the box, because a border is ink the owner
/// owns and a child that covers it has left the region it was given even though
/// it is inside the outer rectangle.
///
/// ★ R1672 — **public, because the placement side needs the same answer.** A
/// pane that hands its scrolling body its own rectangle puts the body over its
/// outline, and a caller computing the inset itself would be a second copy of
/// this rule free to disagree with the check that reports it. Two screens and a
/// widget were doing exactly that, measured at 13 escapes the moment this
/// channel learned the distinction.
///
/// # Against the floor, measured by running it
///
/// The reference toolkit at 6.11 **has** this concept and derives it the same
/// way: a framed widget with a 2px line reports content margins of `2,2,2,2`
/// and a content rect of `(2, 2, 96, 36)` inside a `100x40` box, and a titled
/// group box reports `3,23,3,3` — its caption band included. So this is parity
/// on the *rectangle*, and one limit of ours is worth stating: only the border
/// is subtracted here, because a caption band is a widget's own layout decision
/// and the scene has no vocabulary for it.
///
/// What the floor has no answer for is the question this module exists to ask.
/// Probed there: nothing reports whether a mark actually **left** the content
/// rect. A painter is free to draw outside it and no API says so; there is no
/// per-edge overhang, no owner attribution, and `childrenRect` answers about
/// child *widgets* rather than about painted ink. Here the content rectangle
/// and the report that a mark crossed it are the same function's two halves.
#[must_use]
pub fn content_rect(node: &Scene, box_rect: Rect) -> Rect {
    let (border, chrome) = box_chrome(node);
    content_of(box_rect, border, chrome)
}

/// The border and the declared chrome bands of a node that has a
/// [`BoxStyle`](crate::style::BoxStyle),
/// or `(None, &[])` for one that does not.
///
/// One place reads the style, so [`content_rect`] and the trespass attribution
/// cannot disagree about which nodes have chrome.
fn box_chrome(node: &Scene) -> (Option<&Border>, &[Chrome]) {
    match node {
        Scene::Box(n) => (n.style.border.as_ref(), &n.style.chrome),
        Scene::Container(n) => (n.style.border.as_ref(), &n.style.chrome),
        _ => (None, &[]),
    }
}

/// The content rectangle of a box that strokes `border` inside itself and keeps
/// `chrome` bands of itself for itself.
///
/// The arithmetic half of [`content_rect`], for the side that is **placing**
/// children and so has the style before it has the node. Pass `None` and `&[]`
/// for a box that draws no frame and reserves nothing, and the rectangle comes
/// back unchanged.
///
/// ★★ R1673 — lifted at the tenth consumer, and the count is the argument. Three
/// screens had written the same `const fn panel_content(rect) -> Rect` by hand
/// with the inset spelled `1`, and a full re-measurement then found seven more
/// surfaces owing the same repair. A rule with ten independent implementations
/// is ten chances for one of them to disagree with the check that reports it —
/// and this one is *already* the check, which is the strongest case for a lift
/// there is: the placement and the judgement are now the same arithmetic.
///
/// ★★ R1674 — `chrome` joined it as a **required** argument rather than a second
/// entry point. A `content_of_with_chrome` beside this would let a caller
/// holding a style with bands ask the question that ignores them and get an
/// answer that looks right, which is the two-copies failure the paragraph above
/// records, re-created by an API shape. Every caller now states its chrome, and
/// `&[]` is a statement.
///
/// The bands are subtracted **after** the border, because that is where a
/// painter draws them: a titled frame strokes its outline on the box and then
/// lays its caption inside it. Two bands on one edge sum.
#[must_use]
pub fn content_of(box_rect: Rect, border: Option<&Border>, chrome: &[Chrome]) -> Rect {
    let inset = border.map_or(0, border_inset);
    let mut rect = Rect::new(
        box_rect.x + inset,
        box_rect.y + inset,
        box_rect.w.saturating_sub(inset * 2),
        box_rect.h.saturating_sub(inset * 2),
    );
    for band in chrome {
        rect = split_band(rect, *band).1;
    }
    rect
}

/// A band's own rectangle inside `rect`, and what is left for the content.
///
/// The single implementation of "where does this band sit": [`content_of`]
/// takes the remainder and [`trespasses`] takes the band, so the rectangle a
/// trespass is attributed to is by construction the rectangle the content was
/// denied. A band wider than what is left takes all of it and leaves an empty
/// rectangle **on the far side**, which is where a caller placing children next
/// would want the origin.
const fn split_band(rect: Rect, band: Chrome) -> (Rect, Rect) {
    let taken_h = if band.extent < rect.h {
        band.extent
    } else {
        rect.h
    };
    let taken_w = if band.extent < rect.w {
        band.extent
    } else {
        rect.w
    };
    match band.edge {
        ChromeEdge::Top => (
            Rect::new(rect.x, rect.y, rect.w, taken_h),
            Rect::new(rect.x, rect.y + taken_h, rect.w, rect.h - taken_h),
        ),
        ChromeEdge::Bottom => (
            Rect::new(rect.x, rect.y + rect.h - taken_h, rect.w, taken_h),
            Rect::new(rect.x, rect.y, rect.w, rect.h - taken_h),
        ),
        ChromeEdge::Left => (
            Rect::new(rect.x, rect.y, taken_w, rect.h),
            Rect::new(rect.x + taken_w, rect.y, rect.w - taken_w, rect.h),
        ),
        ChromeEdge::Right => (
            Rect::new(rect.x + rect.w - taken_w, rect.y, taken_w, rect.h),
            Rect::new(rect.x, rect.y, rect.w - taken_w, rect.h),
        ),
    }
}

/// The [`ChromeRole`] a node claims to be, or `None` for ordinary content.
///
/// Read from [`LayoutStyle::chrome_slot`](crate::style::LayoutStyle::chrome_slot)
/// through the node's layout sidecar, which every kind carries — a caption can
/// be a bare [`Scene::Text`] as easily as a container of one.
fn chrome_slot_of(node: &Scene) -> Option<ChromeRole> {
    node.layout_style().and_then(|layout| layout.chrome_slot)
}

/// The band `node` was given, when it claims one its parent actually reserved.
///
/// `None` for a node that claims nothing — the ordinary case — and also for one
/// whose claimed role the parent never declared. The second is deliberate: a
/// band that was not reserved was not taken from the content, so the content
/// rectangle is still the honest thing to judge that node against, and silently
/// exempting it instead would make a typo'd role into an exemption.
fn chrome_band_of(
    node: &Scene,
    owner_box: Rect,
    border: Option<&Border>,
    chrome: &[Chrome],
) -> Option<Rect> {
    let role = chrome_slot_of(node)?;
    let mut rect = content_of(owner_box, border, &[]);
    for band in chrome {
        let (band_rect, remainder) = split_band(rect, *band);
        if band.role == role {
            return Some(band_rect);
        }
        rect = remainder;
    }
    None
}

/// Whether two rectangles share at least one pixel. Zero-extent rectangles
/// cover no pixels, so they intersect nothing — a zero-height band was never
/// taken from the content and cannot be trespassed on.
const fn overlaps(a: Rect, b: Rect) -> bool {
    a.w > 0
        && a.h > 0
        && b.w > 0
        && b.h > 0
        && a.x < b.x + b.w
        && b.x < a.x + a.w
        && a.y < b.y + b.h
        && b.y < a.y + a.h
}

/// What a mark that left the content rectangle actually landed on.
///
/// The floor conflates all of these into "outside the content rect": probed at
/// 6.11, a widget publishes its reservation as four integers and reading them
/// back cannot say which pixels were frame and which were caption. Naming the
/// part is what turns *"this label is out of bounds"* into *"this label is over
/// the title"*, and the two have different repairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trespass {
    /// Past the owner's outer edge entirely — the mark is not in the box at all.
    Outside,
    /// On the border the owner strokes inside its own box.
    Border,
    /// In a band the owner reserved for its own chrome, named by its role.
    Chrome(ChromeRole),
}

impl Trespass {
    /// The word that rides on the wire. A chrome band is `chrome:<role>`, so
    /// one string sorts the three cases and a reader never has to join two
    /// fields to know what was hit.
    #[must_use]
    pub fn wire_word(self) -> String {
        match self {
            Self::Outside => "outside".to_owned(),
            Self::Border => "border".to_owned(),
            Self::Chrome(role) => format!("chrome:{}", role.wire_word()),
        }
    }
}

/// Every part of `owner_box` this mark landed on that was not content, in the
/// order a painter laid them: the outer edge first, then the border, then the
/// chrome bands in declaration order.
///
/// A mark can hit more than one — a header label drawn full-bleed sits on the
/// caption band *and* on the border beside it — and the list says so rather
/// than picking a winner, because the repairs are independent.
fn trespasses(
    painted: Rect,
    owner_box: Rect,
    border: Option<&Border>,
    chrome: &[Chrome],
) -> Vec<Trespass> {
    let mut found = Vec::new();
    if !Overhang::of(painted, owner_box).is_contained() {
        found.push(Trespass::Outside);
    }
    let inside_border = content_of(owner_box, border, &[]);
    // The border ring is what the box has and the inside does not. Testing the
    // four strips separately rather than "in the box but not in the ring"
    // because a mark can sit entirely within one strip.
    if inside_border != owner_box {
        let ring = [
            Rect::new(
                owner_box.x,
                owner_box.y,
                owner_box.w,
                inside_border.y - owner_box.y,
            ),
            Rect::new(
                owner_box.x,
                inside_border.y + inside_border.h,
                owner_box.w,
                (owner_box.y + owner_box.h).saturating_sub(inside_border.y + inside_border.h),
            ),
            Rect::new(
                owner_box.x,
                owner_box.y,
                inside_border.x - owner_box.x,
                owner_box.h,
            ),
            Rect::new(
                inside_border.x + inside_border.w,
                owner_box.y,
                (owner_box.x + owner_box.w).saturating_sub(inside_border.x + inside_border.w),
                owner_box.h,
            ),
        ];
        if ring.iter().any(|strip| overlaps(painted, *strip)) {
            found.push(Trespass::Border);
        }
    }
    let mut rect = inside_border;
    for band in chrome {
        let (band_rect, remainder) = split_band(rect, *band);
        if overlaps(painted, band_rect) {
            found.push(Trespass::Chrome(band.role));
        }
        rect = remainder;
    }
    found
}

/// How many pixels of the box a border's own stroke covers, per edge.
///
/// A `match` over every placement rather than a test for one of them, so a
/// placement added to [`BorderPlacement`] lands here as a compile error instead
/// of silently taking the "nothing" branch. R1672's first draft *was* that test
/// (`if placement != Inside { return box_rect }`) and it got
/// [`BorderPlacement::Center`] wrong: a centred stroke straddles the edge, so
/// half of it is inside the box and a child laid at the box covers that half.
const fn border_inset(border: &Border) -> u32 {
    match border.placement {
        // The whole stroke is inside the box.
        BorderPlacement::Inside => border.width,
        // Half in, half out — and a partially covered pixel is covered, so the
        // half that is inside rounds UP.
        BorderPlacement::Center => border.width.div_ceil(2),
        // Drawn beyond the box; it takes nothing from the content.
        BorderPlacement::Outside => 0,
    }
}

/// Every painted mark that left the box that owns it, in paint order.
///
/// The walk is [`Scene::for_each_node`], so the geometry fold — enclosing
/// scroll offsets and clips — is the same one the tag resolver and the hit test
/// use. A caller cannot get a different answer by doing the arithmetic itself,
/// which is the failure R1653 recorded when three descents each folded their
/// own.
///
/// # Precondition
///
/// The scene has been through `pinion_runtime::compute_layout`: every node's
/// `rect` is in its enclosing scroll frame, not relative to its parent. That is
/// what the renderer and the hit test read, so it is the only frame in which
/// "inside" is the question a reader is asking. Handed an un-laid-out tree this
/// reports whatever the author happened to write down, which is why the
/// consumer-side tests here drive `view()` through the real layout pass rather
/// than asserting against hand-written rectangles.
///
/// `ink_of` is asked only about [`Scene::Text`] nodes. Every other kind paints
/// its own rectangle, so its promise and its paint are the same value and it
/// can only escape by being placed outside its owner — which is still worth
/// reporting, and is how a badge drawn past its card is caught.
#[must_use]
pub fn escapes(scene: &Scene, ink_of: InkOf<'_>) -> Vec<Escape> {
    // Pass 1 — every node's window-absolute rectangle, keyed by identity.
    //
    // A parent is always visited before its children, so a one-pass version
    // would work for the LOOKUP; it is two passes because reading a parent's
    // rectangle out of the child's own fold is what the first draft did and it
    // was wrong. `NodeVisit::offset` accumulates only what enclosing
    // [`Scene::Scroll`] nodes contribute — post-layout rectangles are already
    // in their scroll frame, not their parent's — so subtracting the parent's
    // origin back out (which is what "the parent's rect is in its parent's
    // frame" would require) double-counted it and reported overhangs in the
    // thousands of pixels for a glyph inside a button. Measured, on the first
    // run against a real screen.
    let mut absolute: HashMap<*const Scene, Rect> = HashMap::new();
    scene.for_each_node(&mut |visit| {
        absolute.insert(
            std::ptr::from_ref(visit.node),
            translate(visit.node.rect(), visit.offset),
        );
    });

    // Pass 2 — the judgment.
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(parent) = visit.ancestors.last() else {
            return; // the root answers to nothing
        };
        let Some(&owner_rect) = absolute.get(&std::ptr::from_ref(*parent)) else {
            return;
        };
        if owner_rect.w == 0 || owner_rect.h == 0 {
            // A parent with no extent is a grouping node, not a box: it makes
            // no promise about where its children go, so it cannot be broken.
            return;
        }
        // ★★ R1672 — against the owner's CONTENT rectangle: its box less the
        // border it draws inside that box. A border is ink the owner owns, so a
        // child painted at the owner's full width covers the outline and leaves
        // a gap in it — and until this round that was reported as contained,
        // because "inside the box" was the whole of the question.
        //
        // Found by a person looking at a window twice in one session, on two
        // different bands of the same card, and neither `scene/containment` nor
        // any screen's own gate could see either: CSS has had border box versus
        // content box from the beginning and nothing here expressed the second.
        //
        // `Outside` placement draws the border beyond the box, so it takes
        // nothing from the content; only an inside border does.
        let owner_box = owner_rect;
        let (border, chrome) = box_chrome(parent);
        // ★★ R1674 — WHICH rectangle this child was promised depends on what it
        // says it is. A node carrying a `chrome_slot` is the band itself, so it
        // is judged against the band; every other child is judged against what
        // is left once the bands are taken out. Two questions, because a single
        // one has to be wrong for one of the two populations: judging the title
        // against the content rectangle reports every titled frame in the tree
        // as broken, and exempting whatever is drawn in the band lets a label
        // that really did land on the caption through.
        let owner_rect = chrome_band_of(visit.node, owner_box, border, chrome)
            .unwrap_or_else(|| content_of(owner_box, border, chrome));
        if matches!(parent, Scene::Scroll(_)) {
            // A scroll's content is SUPPOSED to be bigger than the viewport —
            // that is what makes it scrollable. Judging it here reported a
            // world surface as a 4,476-pixel escape on the first real run, and
            // a check that fires on the normal case is a check nobody keeps.
            // Marks INSIDE that content are still judged against their own
            // boxes, which is where the question is meaningful.
            //
            // ★★ R1685 — and a box that clips because it declares
            // `Overflow::Hidden` gets NO such exemption, deliberately. The two
            // look alike (a child taller than its parent, by design in both
            // cases) and they differ in the only thing this module is about:
            // under a scroll the reader can still get to it, and under a
            // hidden box the content is GONE. So the scroll case is normal and
            // the hidden case is a loss, even when it is an intended one — the
            // module's own rule is that a clip must not silently swallow the
            // report, because "this label ends here" and "this label is too
            // long" look identical on screen. `reach` then says which marks
            // actually went, which is the actionable half.
            return;
        }
        // Where this mark sits with no clip folded in: `absolute_rect` answers
        // where it can be SEEN, and a mark whose overhang is entirely clipped
        // away has still been mis-placed. The clip is read separately below,
        // to decide the fate rather than to hide the escape.
        let promised = translate(visit.node.rect(), visit.offset);
        let (painted, content) = match visit.node {
            Scene::Text(t) => {
                let (w, h) = ink_of(t);
                (
                    Rect::new(promised.x, promised.y, w, h),
                    Some(t.content.clone()),
                )
            }
            _ => (promised, None),
        };
        if painted.w == 0 || painted.h == 0 {
            return; // nothing was drawn, so nothing left anything
        }
        let over = Overhang::of(painted, owner_rect);
        if over.is_contained() {
            return;
        }
        let fate = match visit.clip {
            Some(clip) if !Overhang::of(painted, clip).is_contained() => Fate::Clipped,
            _ => Fate::Smeared,
        };
        // ★ R1674 — attributed against the owner's BOX, because that is the
        // rectangle the parts divide up: the border ring and every chrome band
        // are inside it, and the content rectangle is what is left after both.
        //
        // A chrome node's own band is not a trespass by it — it was given that
        // band — so the band it fills is dropped from its own list. What
        // remains is what it reached beyond its band: the border it covered, or
        // a neighbouring band, or the outside of the box.
        let mut trespass = trespasses(painted, owner_box, border, chrome);
        if let Some(role) = chrome_slot_of(visit.node) {
            trespass.retain(|t| *t != Trespass::Chrome(role));
        }
        found.push(Escape {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            owner: parent
                .tag()
                .map_or_else(|| UNTAGGED.to_owned(), str::to_owned),
            content,
            promised,
            painted,
            owner_rect,
            over,
            fate,
            trespass,
        });
    });
    found
}

/// Two boxes standing side by side in one seat whose centre lines are **one
/// pixel apart** — the signature [`band_in`]'s rule leaves when it is spelled
/// by hand instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncentred {
    /// Tag of the seat holding both, or [`UNTAGGED`].
    pub seat: String,
    /// Address of the leading box, as
    /// [`lookup_path_ref`](Scene::lookup_path_ref) accepts it.
    pub first: Vec<String>,
    /// Its window-absolute rectangle.
    pub first_rect: Rect,
    /// Address of the box beside it.
    pub second: Vec<String>,
    /// Its window-absolute rectangle.
    pub second_rect: Rect,
}

impl Uncentred {
    /// The two centre lines, leading box first — what a failure message has to
    /// print for the reader to see the pixel.
    #[must_use]
    pub const fn centres(&self) -> (u32, u32) {
        (centre_line(self.first_rect), centre_line(self.second_rect))
    }
}

/// ★★★★★ R1956 — **two things placed side by side in one seat that do not
/// share its centre line**, which is the axis every other check in this module
/// leaves open.
///
/// [`escapes`] asks whether ink left its box, [`short_boxes`] whether the box
/// is too small for the face, [`slack`] whether it is too big. All three are
/// about a box's SIZE. **Nothing asked where a box sits inside the seat it was
/// placed in**, and a person reading the running application does: R1862 was
/// opened because a legend's 11px pin sample and 12px label — each given a
/// plausible `+3` in an 18px row — did not line up, and R1882 because a card's
/// title and badge did not.
///
/// # ⚠ Why this reads BOXES, when [`Slack::off_centre`] already reads ink
///
/// [`OffCentre`] carries the warning in its own doc: only its horizontal axis
/// is answerable from ink, because a string with no descender inks short of the
/// bottom of its line box and would be reported high of centre while sitting
/// exactly where the shaper puts it. R1874's rule — *width is measured by ink,
/// height by the line box* — is why. So the vertical question cannot be asked
/// on that channel at all, and this one asks it where it IS answerable: of the
/// rectangles the scene declares.
///
/// # The population, and what it deliberately excludes
///
/// Two children of one parent, both with extent, that are **beside** each other
/// — horizontally disjoint and vertically overlapping — **at least one of them
/// a text run**. A pair stacked one above the other is not in it, because a
/// stack is not making the claim this checks; [`stacked_line_rects`] is that
/// shape's own answer and centres the block.
///
/// ⚠ **The text clause is not a let-off, it is what the question is about.**
/// Measured on the assembled application before it was there: the largest group
/// this reported was the three dots of a rail icon drawn one pixel apart on
/// purpose — a picture's own strokes, not two things placed side by side, and a
/// gate calling an icon's shape a defect is a gate nobody can hold at zero. The
/// defects this exists for are both *a run against the thing beside it*: R1862
/// a pin sample and its label, R1882 a title and its badge. This module's whole
/// vocabulary — [`line_box`], [`line_rect_in`] — is built around a line for the
/// same reason: **a centre line is what a run is placed on**, and two marks in
/// one drawing share nothing of the kind.
///
/// # ⚠ Exactly one pixel, which is a claim about the CAUSE
///
/// One pixel is the whole range a second rounding can produce, so the report is
/// narrow on purpose: it names the pairs whose separation is *the arithmetic*,
/// not every pair that is not centred. Two boxes 6 pixels apart are top-aligned
/// or baseline-aligned or wrong for some other reason, and lumping them in
/// would make this a check nobody can hold at zero — which is how a gate ends
/// up with a pin instead of a floor.
///
/// This is therefore a **weak claim made exactly**, in the direction this
/// project has repeatedly found to be the right one: it can be demanded at zero
/// on a real screen, and it goes red on the defect it was built from.
#[must_use]
pub fn uncentred(scene: &Scene) -> Vec<Uncentred> {
    // The children of each seat, in paint order, window-absolute. One pass is
    // enough where [`escapes`] needs two: that one has to read the PARENT's
    // rectangle, and this one only needs the parent's identity and tag.
    let mut seats: HashMap<*const Scene, Vec<Placed>> = HashMap::new();
    let mut tags: HashMap<*const Scene, String> = HashMap::new();
    // Insertion order, so the report is stable across runs — a `HashMap`'s is
    // not, and a gate whose failure text reorders itself cannot be diffed.
    let mut order: Vec<*const Scene> = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(parent) = visit.ancestors.last() else {
            return; // the root sits in no seat
        };
        let rect = translate(visit.node.rect(), visit.offset);
        if rect.w == 0 || rect.h == 0 {
            // No extent: a grouping node, which is not placed anywhere.
            return;
        }
        let seat = std::ptr::from_ref(*parent);
        if !seats.contains_key(&seat) {
            order.push(seat);
            tags.insert(
                seat,
                parent
                    .tag()
                    .map_or_else(|| UNTAGGED.to_owned(), str::to_owned),
            );
        }
        seats.entry(seat).or_default().push(Placed {
            path: visit.path.to_vec(),
            rect,
            run: matches!(visit.node, Scene::Text(_)),
        });
    });

    let mut found = Vec::new();
    for seat in order {
        let kids = &seats[&seat];
        for (i, first) in kids.iter().enumerate() {
            for second in &kids[i + 1..] {
                if !first.run && !second.run {
                    continue; // two marks of one drawing share no centre line
                }
                if !beside(first.rect, second.rect) {
                    continue;
                }
                if centre_line(first.rect).abs_diff(centre_line(second.rect)) != 1 {
                    continue;
                }
                found.push(Uncentred {
                    seat: tags[&seat].clone(),
                    first: first.path.clone(),
                    first_rect: first.rect,
                    second: second.path.clone(),
                    second_rect: second.rect,
                });
            }
        }
    }
    found
}

/// One child of a seat, as [`uncentred`] reads it off the paint.
struct Placed {
    /// Its address, as [`lookup_path_ref`](Scene::lookup_path_ref) accepts it.
    path: Vec<String>,
    /// Its window-absolute rectangle.
    rect: Rect,
    /// Whether it is a text run — the clause that keeps a drawing's own strokes
    /// out of the population. See [`uncentred`]'s own warning for why that is
    /// what the question is about rather than a let-off.
    run: bool,
}

/// Whether two rectangles stand **beside** each other: horizontally disjoint
/// and vertically overlapping. Saturating, for R1653's reason — a coordinate
/// arithmetic overflow here is a panic in debug and an absurd answer in
/// release.
const fn beside(a: Rect, b: Rect) -> bool {
    let disjoint = a.x.saturating_add(a.w) <= b.x || b.x.saturating_add(b.w) <= a.x;
    let overlapping = a.y < b.y.saturating_add(b.h) && b.y < a.y.saturating_add(a.h);
    disjoint && overlapping
}

/// ★★★★★ R1811 — **a box far larger than the one thing it holds**, which is
/// the question this module's other two do not ask.
///
/// [`escapes`] asks whether the ink left its box and [`short_boxes`] whether
/// the box is too small for the face. Both are "is the box big enough?" from
/// opposite sides. **Nothing asked whether it is too big**, and a reader looking
/// at the running application did: a status message reading *"Node Lab section"*
/// sat in a box 560 pixels wide because the width was a constant, and the
/// complaint was that the box was strangely wide — not that anything was lost.
///
/// # What this does NOT do, and the three measurements that settled it
///
/// It does not decide **which** boxes should be snug. A box larger than its
/// content is usually correct — a panel, a card, a canvas are all bigger than
/// what is in them — so the interesting population is "boxes whose size is a
/// claim about their content", and R1811 tried three times to derive that from
/// the scene and failed each time:
///
/// 1. *a box whose whole content is one text run.* Measured on the assembled
///    analysis tool: it reported a tree row 203px wider than its label and hex
///    cells 10px wider than their bytes — all correct, because a cell in a
///    column is sized by the column.
/// 2. *…and absolutely positioned, so the width was authored.* Measured: it
///    narrowed nothing. This tree's screens paint almost everything at absolute
///    rectangles by convention, so that flag does not separate an authored
///    width from a laid-out one **here**.
/// 3. *the box a reader actually complained about* — a status toast — is not a
///    one-run box at all. It holds a tone bullet and a label, so rule 1 never
///    reached the case it was invented for.
///
/// ⇒ **intent is not recoverable from geometry**, which is this repository's
/// recurring finding in its own shape: what a box's size MEANS is a thing an
/// author knows and the scene does not record. So this answers the measurable
/// half — *how much of this box does its content not use* — for every box, and
/// leaves choosing to the caller, who has the intent. A caller asks about the
/// boxes it means, and the reason it means them lives at that call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slack {
    /// The box's tag, when it has one.
    pub tag: Option<String>,
    /// Window-absolute box.
    pub box_rect: Rect,
    /// Window-absolute content rectangle — [`box_rect`](Self::box_rect) less
    /// the border the box draws inside it, which is the region what it holds
    /// was actually given.
    pub inner: Rect,
    /// What the box holds, as text, when any of it is a run — the words a
    /// reader is looking at when they say the box is the wrong size.
    pub content: String,
    /// Window-absolute union of everything the box holds: the shaped ink for a
    /// text run, **where the shaper put it**, and the painted rectangle for
    /// anything else.
    ///
    /// ★ R1904 — *where* is what this gained. Before that round the position
    /// came from the run's own rectangle, so a run an alignment had moved was
    /// reported at the place it would have inked without one.
    pub ink: Rect,
    /// Width the box holds beyond its content rectangle's ink.
    pub spare_w: u32,
    /// Height the box holds beyond its content rectangle's ink.
    pub spare_h: u32,
}

impl Slack {
    /// R1904 — how far what this box holds sits from the box's centre.
    ///
    /// Measured against [`inner`](Self::inner), for [`spare_w`](Self::spare_w)'s
    /// reason: a border is not room the content failed to use, and a rule that
    /// counted it would call a bordered box off-centre for drawing its own
    /// outline.
    ///
    /// ⚠ **Answered from the INK, not from the rectangle the run was promised**,
    /// which is the whole reason this exists. The two agree exactly when
    /// nothing moved the run inside its box — and the case a person reported is
    /// the case where they do not.
    #[must_use]
    pub fn off_centre(&self) -> OffCentre {
        // Before less after: the content is off centre towards the side whose
        // margin is the larger, and the sign says which. The overflow arm comes
        // FIRST, because the margins below saturate and would answer `0 - 0`
        // for it — see `Centring`.
        let axis = |lo: u32, len: u32, ink_lo: u32, ink_len: u32| -> Centring {
            if ink_lo < lo || ink_lo.saturating_add(ink_len) > lo.saturating_add(len) {
                return Centring::Overflows;
            }
            let before = i64::from(ink_lo - lo);
            let after = i64::from((lo + len) - (ink_lo + ink_len));
            Centring::Within(before - after)
        };
        OffCentre {
            x: axis(self.inner.x, self.inner.w, self.ink.x, self.ink.w),
            y: axis(self.inner.y, self.inner.h, self.ink.y, self.ink.h),
        }
    }
}

/// Every box that holds something, with how much of it that something leaves
/// unused.
///
/// # Precondition
///
/// [`escapes`]'s: the scene has been through `compute_layout`, so a rectangle
/// is where the renderer will put it. Handed an un-laid-out tree this reports
/// what somebody wrote down rather than what a reader will see.
///
/// The spare is measured against the **content** rectangle
/// ([`content_rect`]), not the box, so a border and a declared chrome band are
/// not counted as room the run failed to fill — they were never its to use.
///
/// A run wider than its box reports `0` spare rather than an underflow; that
/// direction is [`escapes`]'s question and is already answered there.
///
/// ★★★★★ R1904 — the metric is an [`InkSpanOf`] rather than an [`InkOf`],
/// because [`Slack::off_centre`] asks *where* and the size alone cannot answer
/// it. See [`InkSpan`] for why `escapes` is not changed with it.
#[must_use]
pub fn slack(scene: &Scene, ink_of: InkSpanOf<'_>) -> Vec<Slack> {
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Scene::Container(container) = visit.node else {
            return;
        };
        if container.children.is_empty() {
            return; // a spacer holds nothing, so it leaves nothing unused
        }
        // The same fold [`escapes`] uses — `NodeVisit::offset` carries only what
        // enclosing scroll nodes contribute, because a post-layout rectangle is
        // already in its scroll frame.
        let box_rect = translate(visit.node.rect(), visit.offset);
        let content = content_rect(visit.node, box_rect);
        let mut held: Option<Rect> = None;
        let mut said = String::new();
        for child in &container.children {
            // A child's rectangle is already in this box's frame; its INK is
            // the shaped extent for a run and the rectangle itself otherwise —
            // the same distinction `escapes` draws, for the same reason.
            let InkSpan { dx, w, h } = match child {
                Scene::Text(text) => {
                    if !said.is_empty() {
                        said.push(' ');
                    }
                    said.push_str(&text.content);
                    ink_of(text)
                }
                other => InkSpan {
                    dx: 0,
                    w: other.rect().w,
                    h: other.rect().h,
                },
            };
            if w == 0 || h == 0 {
                continue;
            }
            // ★★★★★ R1904 — the child's own rectangle is ALREADY window
            // absolute, folded into its scroll frame exactly as `box_rect` is,
            // so it takes the same `translate` and NOT the box's origin on top
            // of it. This line read `box_rect.x + at.x` until this round, which
            // is the double count `escapes` records having made in its own
            // first draft and repaired. What it costs is performed rather than
            // recounted here: `r1904_slack_reports_the_ink_where_the_shaper_put_it`
            // pins the answer for a cell at x 100 holding a run at x 102, and
            // restoring the old line moves that ink clean outside the box —
            // far enough that `off_centre` stops being able to answer at all.
            //
            // Nothing had reported it because nothing had read `ink`'s
            // POSITION: the only consumer wanted `spare_w`, and a spare is a
            // difference of extents that a wrong origin does not disturb. A
            // field can be wrong for as long as it goes unasked.
            let at = translate(child.rect(), visit.offset);
            // `dx` is what moved the glyphs inside the rectangle the run was
            // promised — zero for everything laid out flush, and for every mark
            // that is not a run.
            let here = Rect::new(at.x + dx, at.y, w, h);
            held = Some(match held {
                Some(so_far) => so_far.union(here),
                None => here,
            });
        }
        let Some(ink) = held else {
            return; // nothing was drawn, so the box holds no claim to check
        };
        found.push(Slack {
            tag: container.tag.as_ref().map(ToString::to_string),
            box_rect,
            inner: content,
            content: said,
            ink,
            spare_w: content.w.saturating_sub(ink.w),
            spare_h: content.h.saturating_sub(ink.h),
        });
    });
    found
}

/// What an escape names as its owner when the box that broke its promise
/// carries no tag. Spelled once so a caller filtering on it and this module
/// producing it cannot drift.
pub const UNTAGGED: &str = "<untagged>";

/// Fold a container-local rectangle into the walk root's frame.
///
/// Saturating rather than wrapping: R1653 measured what an underflow here costs
/// — a pan to the left turned a `u32` subtraction into a coordinate near four
/// billion, which is a panic in a debug build and a silently absurd rectangle
/// in a release one.
fn translate(rect: Rect, offset: (i64, i64)) -> Rect {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "clamped into u32's range on the line above the cast"
    )]
    let fold =
        |v: u32, by: i64| -> u32 { (i64::from(v) + by).clamp(0, i64::from(u32::MAX)) as u32 };
    Rect::new(
        fold(rect.x, offset.0),
        fold(rect.y, offset.1),
        rect.w,
        rect.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
    use crate::style::{BoxStyle, TextStyle};

    /// ★★★★★ R1870 — a position is *which one*, and folding it is what lets a
    /// table's cells read as the one mistake they are.
    #[test]
    fn r1870_a_repeating_site_folds_the_positions_and_keeps_the_names() {
        // The address the real boot printed, and the eight cells that shared it.
        assert_eq!(
            repeating_site("card.packet#0.cell.0_2"),
            "card.packet#*.cell.*"
        );
        assert_eq!(
            repeating_site("card.packet#0.cell.1_1"),
            repeating_site("card.packet#0.cell.4_3"),
            "two cells of one table are one site"
        );
        assert_eq!(
            repeating_site("card.packet#0.cell.1_1"),
            repeating_site("card.packet#7.cell.1_1"),
            "and so are the same cell of two cards — the card number is a position too"
        );
        // A path, whose separator differs and whose segments are positions.
        assert_eq!(repeating_site("packets/1/2"), "packets/*/*");
        // ⚠ The direction this must NOT fold: a digit inside a name.
        assert_eq!(repeating_site("proto.ipv4.head"), "proto.ipv4.head");
        assert_ne!(
            repeating_site("proto.ipv4"),
            repeating_site("proto.ipv6"),
            "folding a digit that is part of a name would merge two real sites"
        );
        // A `#` with no digits after it is punctuation in a name, not a position.
        assert_eq!(repeating_site("card.a#b"), "card.a#b");
        // Nothing to fold, and nothing lost.
        assert_eq!(repeating_site(""), "");
        assert_eq!(repeating_site("shell.status"), "shell.status");
    }

    /// ★★★★★ R1879 — **an index GLUED TO A NAME folds its index and keeps the
    /// name**, which is the third shape a segment can have and the one this
    /// function could not see.
    ///
    /// # The three shapes, tested together on purpose
    ///
    /// R1870 built the fold for two of them and the third had no case here, so
    /// nothing said what should happen to it:
    ///
    /// 1. **a pure position** — `3_2`, all digits and separators. Folds whole.
    /// 2. **an index glued to a name** — `0_direction`. Folds its INDEX only,
    ///    to `*_direction`, so a table's ROWS collapse and its COLUMNS stay
    ///    apart. That is more information than shape 1 gets, not less.
    /// 3. **a digit inside a name** — `ipv4`, `l0`, `v0x09`. Folds NOTHING.
    ///
    /// ⚠ Shape 3 is why the rule is not "fold anything containing a digit":
    /// that would merge `proto.ipv4` with `proto.ipv6`, which is this defect's
    /// mirror image — an over-fold that hides two real sites inside one line.
    ///
    /// # What it cost to leave shape 2 out
    ///
    /// Measured at R1878 through the census's second axis: `kp.list.cell.*` is
    /// **57 runs the census reported as 57 separate one-run sites**, and
    /// `lv.list.cell.*` is **50 more** — two tables of the same shape as the
    /// one R1872 repaired, 107 runs between them, invisible to every line the
    /// warning spells because none of their sites ever held more than one run.
    #[test]
    fn r1879_a_position_glued_to_a_name_folds_only_its_index() {
        // 1. A pure position still folds whole — R1870's case, kept.
        assert_eq!(repeating_site("pv.list.cell.3_2"), "pv.list.cell.*");

        // 2. The shape that had no case. The index goes, the column stays.
        assert_eq!(
            repeating_site("kp.list.cell.0_direction"),
            "kp.list.cell.*_direction",
        );
        assert_eq!(
            repeating_site("kp.list.cell.0_direction"),
            repeating_site("kp.list.cell.9_direction"),
            "two rows of one column are one site",
        );
        assert_ne!(
            repeating_site("kp.list.cell.0_direction"),
            repeating_site("kp.list.cell.0_pattern"),
            "two COLUMNS are not — folding them together would lose which \
             column a repair belongs to",
        );
        assert_eq!(
            repeating_site("lv.list.cell.12_message"),
            "lv.list.cell.*_message",
            "the index is a run of digits, not a single one",
        );

        // 3. A digit inside a name folds nothing — the over-fold this must not
        //    become.
        assert_eq!(repeating_site("proto.ipv4.head"), "proto.ipv4.head");
        assert_eq!(repeating_site("lab.gate.l0"), "lab.gate.l0");
        assert_eq!(repeating_site("pv.tree.v0x09"), "pv.tree.v0x09");
        assert_ne!(
            repeating_site("proto.ipv4"),
            repeating_site("proto.ipv6"),
            "still two sites",
        );

        // ⚠ The boundary between 2 and 3: an index needs its `_`. A name that
        // merely STARTS with a digit is a name.
        assert_eq!(repeating_site("kp.0direction"), "kp.0direction");
        // And a card's `#index` is unchanged by all of this.
        assert_eq!(
            repeating_site("card.decode#1.tree.0"),
            "card.decode#*.tree.*"
        );
    }

    /// ★ R1870 — an untagged run's address is the whole path, because its last
    /// segment is a POSITION: the real boot's first warning line named its
    /// subject `2`, which no reader can follow.
    #[test]
    fn r1870_an_untagged_run_is_addressed_by_its_whole_path() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Container(
            ContainerNode::new(vec![text(
                "Message Stream",
                // The fixture's face is the default one, so the box is authored
                // four pixels short of the line that face needs.
                Rect::new(0, 0, 200, line_box(TextStyle::default().font_size_px) - 4),
                None,
            )])
            .with_tag("shell"),
        )]));
        let short = short_boxes(&scene);
        assert_eq!(short.len(), 1, "the fixture holds exactly one short run");
        let row = &short[0];
        assert!(row.tag.is_none(), "the fixture's run carries no tag");
        assert!(
            row.path.len() > 1,
            "a path with one segment could not tell the two addressings apart: {:?}",
            row.path
        );
        assert_eq!(row.address(), row.path.join("/"));
        assert_ne!(
            row.address(),
            row.path.last().expect("a non-empty path").clone(),
            "the last segment is where the run sits among its siblings, not where it is"
        );
    }

    /// A text run whose ink the fixture decides, so these tests are about the
    /// POLICY and never about a shaper.
    fn text(content: &str, rect: Rect, tag: Option<&'static str>) -> Scene {
        let node = TextNode::new(content, rect);
        Scene::Text(match tag {
            Some(t) => node.with_tag(t),
            None => node,
        })
    }

    fn boxed(rect: Rect, tag: &str, children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = rect;
        c.tag = Some(tag.to_owned().into());
        Scene::Container(c)
    }

    /// Ink measured as `len * 8` wide by `12` tall — a stand-in with no font in
    /// it, so the assertions are about which box was compared against.
    fn stub_ink(t: &TextNode) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "fixture strings are a handful of characters"
        )]
        let w = (t.content.chars().count() as u32) * 8;
        (w, 12)
    }

    /// R1672 — ★★ a child that covers its owner's BORDER is an ESCAPE.
    ///
    /// R1671 pinned the opposite answer here, with a doc saying the test should
    /// be rewritten on the round that gave this channel a content box. This is
    /// that round, and the test is the rewrite.
    ///
    /// The distinction is CSS's border box versus content box, and it is not
    /// decorative: a person looking at a window reported the same defect twice
    /// in one session, on two bands of one card, and neither this channel nor
    /// any screen's own gate could see either — because "inside the box" was
    /// the whole of the question and a border lives inside the box.
    ///
    /// The overhang is reported per edge, so the repair (inset by the border)
    /// is legible from the report alone.
    #[test]
    fn r1672_a_mark_over_its_owners_border_is_an_escape() {
        use crate::style::{Border, Color};

        let strip = |x: u32, w: u32| {
            Scene::Box(
                BoxNode::new(
                    Rect::new(x, 10, w, 12),
                    BoxStyle::filled(Color::rgb(0x30, 0x30, 0x30)),
                )
                .with_tag("strip"),
            )
        };
        let framed = |child: Scene| {
            let mut frame = ContainerNode::new(vec![child]);
            frame.rect = Rect::new(0, 0, 100, 40);
            frame.tag = Some("frame".to_owned().into());
            frame.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10))
                .with_border(Border::new(Color::rgb(0xEC, 0x5A, 0xA0), 1));
            Scene::Container(frame)
        };

        // Exactly the owner's width: the fill covers both border columns.
        let found = escapes(&framed(strip(0, 100)), &mut stub_ink);
        assert_eq!(found.len(), 1, "the strip covers the frame: {found:?}");
        assert_eq!(found[0].owner, "frame");
        assert_eq!(
            found[0].owner_rect,
            Rect::new(1, 1, 98, 38),
            "the CONTENT box"
        );
        assert_eq!(
            found[0].over.left, 1,
            "one column each side, named per edge"
        );
        assert_eq!(found[0].over.right, 1);
        assert_eq!(found[0].over.top, 0);
        assert_eq!(found[0].over.bottom, 0);

        // Inset by the border: nothing to report. The rule is not "anything
        // touching the edge" — it is the border's own pixels.
        assert!(
            escapes(&framed(strip(1, 98)), &mut stub_ink).is_empty(),
            "a band inset by the frame is contained",
        );

        // And an owner with no border is judged against its box, unchanged:
        // this rule takes nothing away from a surface that draws no frame.
        let mut plain = ContainerNode::new(vec![strip(0, 100)]);
        plain.rect = Rect::new(0, 0, 100, 40);
        plain.tag = Some("plain".to_owned().into());
        assert!(
            escapes(&Scene::Container(plain), &mut stub_ink).is_empty(),
            "no border, no content inset",
        );
    }

    /// R1673 — the placement half and the judging half are one arithmetic.
    ///
    /// [`content_of`] is what a painter calls before it has a node, and
    /// [`content_rect`] is what the check calls after. If they could disagree,
    /// a screen could be laid out correctly by its own rule and reported wrong
    /// by ours — which is the failure three screens' hand-written
    /// `panel_content` was one edit away from at all times.
    #[test]
    fn r1673_placing_and_judging_read_the_same_content_box() {
        use crate::style::{Border, Color};

        let border = Border::new(Color::rgb(0xEC, 0x5A, 0xA0), 3);
        let mut framed = ContainerNode::new(Vec::new());
        framed.rect = Rect::new(10, 20, 100, 40);
        framed.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10)).with_border(border);
        let node = Scene::Container(framed);

        let box_rect = Rect::new(10, 20, 100, 40);
        assert_eq!(
            content_rect(&node, box_rect),
            content_of(box_rect, Some(&border), &[]),
            "the judging half and the placing half are one answer",
        );
        assert_eq!(
            content_of(box_rect, None, &[]),
            box_rect,
            "no border, no chrome, no inset"
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[]),
            Rect::new(13, 23, 94, 34)
        );

        // ★ R1674 — and the same identity holds once the box declares chrome,
        // which is the half that could have been added to one side only. A
        // titled frame is the case: the caption band is subtracted by the
        // painter placing children AND by the check judging them, or the two
        // disagree about the same twenty pixels.
        let caption = Chrome::caption(20);
        let mut titled = ContainerNode::new(Vec::new());
        titled.rect = box_rect;
        titled.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10))
            .with_border(border)
            .with_chrome(caption);
        assert_eq!(
            content_rect(&Scene::Container(titled), box_rect),
            content_of(box_rect, Some(&border), &[caption]),
            "chrome reaches the judging half too",
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[caption]),
            Rect::new(13, 43, 94, 14),
            "3px of border on every edge, then 20px of caption off the top",
        );
    }

    /// R1672 — how much of the box a border takes, for **every** placement.
    ///
    /// Written because the first draft of [`content_rect`] asked whether the
    /// placement was [`BorderPlacement::Inside`] and returned the box for
    /// anything else, which is right for [`BorderPlacement::Outside`] and wrong
    /// for [`BorderPlacement::Center`] — a centred stroke straddles the edge, so
    /// half its width is inside the box and a child laid at the box covers it.
    ///
    /// The population is a `match` over the enum, so a fourth placement is a
    /// compile error here rather than a silent fourth answer.
    #[test]
    fn r1672_each_border_placement_takes_its_own_share_of_the_box() {
        use crate::style::{Border, Color};

        let framed = |placement: BorderPlacement, width: u32| {
            let mut frame = ContainerNode::new(Vec::new());
            frame.rect = Rect::new(0, 0, 100, 40);
            frame.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10)).with_border(
                Border::new(Color::rgb(0xEC, 0x5A, 0xA0), width).with_placement(placement),
            );
            Scene::Container(frame)
        };
        let inset_of = |placement, width| {
            let node = framed(placement, width);
            content_rect(&node, Rect::new(0, 0, 100, 40)).x
        };

        let mut covered = 0;
        for placement in [
            BorderPlacement::Inside,
            BorderPlacement::Center,
            BorderPlacement::Outside,
        ] {
            covered += 1;
            let want = match placement {
                // The whole 4px stroke is in the box.
                BorderPlacement::Inside => 4,
                // Half of it is, and an odd width rounds up: a partly covered
                // pixel is covered.
                BorderPlacement::Center => 2,
                // None of it is.
                BorderPlacement::Outside => 0,
            };
            assert_eq!(inset_of(placement, 4), want, "{placement:?} at width 4");
        }
        assert_eq!(covered, 3, "the census covers every placement");
        assert_eq!(
            inset_of(BorderPlacement::Center, 3),
            2,
            "an odd centred stroke rounds its inside half UP",
        );
    }

    /// ★ The defect this module was written for, as a property: a card whose
    /// last row is painted below its own border.
    ///
    /// Stated against the ink rather than the box because that is the half that
    /// was missing — the row's promised rectangle can sit inside the card while
    /// the glyphs it holds do not.
    #[test]
    fn r1656_a_row_painted_past_the_card_border_is_reported() {
        let card = boxed(
            Rect::new(10, 10, 100, 40),
            "card",
            vec![text("row", Rect::new(14, 40, 40, 11), None)],
        );
        let found = escapes(&card, &mut stub_ink);
        assert_eq!(found.len(), 1, "one escape, not two: {found:?}");
        let escape = &found[0];
        assert_eq!(escape.owner, "card");
        assert_eq!(escape.content.as_deref(), Some("row"));
        // Row top at 40, ink 12 tall -> 52; the card's bottom edge is 10+40=50.
        assert_eq!(escape.over.bottom, 2, "{escape:?}");
        assert_eq!(escape.over.right, 0);
        assert_eq!(escape.fate, Fate::Smeared, "nothing clipped it");
    }

    /// ★ The stated limit, as a test: a painter that flattens its containment
    /// gets a CLEAN report, and that is not this module being wrong.
    ///
    /// This is the shape the analysis-tool canvas actually painted — the card
    /// and its parts as siblings — and it is why every check in the tree
    /// answered "contained" while a person could see text outside the border.
    /// The repair is to put the relation back in the scene (§2 #7 says the
    /// scene IS the description of the screen), not to guess it from a naming
    /// habit: a rule that judged a mark against its longest tag-prefix was
    /// written, measured against the real screen, and produced two findings
    /// that were both false — `lab.toolbar.zoom.out` is the button BESIDE the
    /// readout `lab.toolbar.zoom`, not content inside it.
    ///
    /// Asserting the blindness is what keeps it from being rediscovered as a
    /// surprise, and what makes the nesting repair in the consumer load-bearing
    /// rather than cosmetic.
    #[test]
    fn r1656_a_flat_painter_gets_a_clean_report_and_that_is_the_limit() {
        let flat = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::new(Rect::new(0, 0, 100, 40), BoxStyle::default()).with_tag("card"),
            ),
            // Painted well below the card, as a SIBLING of it.
            text("id", Rect::new(4, 44, 40, 11), Some("card.id")),
        ]));
        let root_rect = Rect::new(0, 0, 1000, 1000);
        let mut rooted = ContainerNode::new(match flat {
            Scene::Container(c) => c.children,
            other => vec![other],
        });
        rooted.rect = root_rect;
        let found = escapes(&Scene::Container(rooted), &mut stub_ink);
        assert!(
            found.is_empty(),
            "the scene says both are children of the root, and they are: {found:?}"
        );

        // The same two marks, with the relation the painter meant present in
        // the tree: now the escape is visible, and it is the only difference.
        let nested = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("id", Rect::new(4, 44, 40, 11), None)],
        );
        let found = escapes(&nested, &mut stub_ink);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].owner, "card");
        assert_eq!(found[0].over.bottom, 16, "{:?}", found[0]);
    }

    /// ★ A mark inside its box is not reported, however tight the fit — the
    /// meter this replaces failed exactly here, reporting 79% of a screen.
    #[test]
    fn r1656_a_mark_that_fits_exactly_is_not_an_escape() {
        let card = boxed(
            Rect::new(0, 0, 24, 12),
            "card",
            vec![text("abc", Rect::new(0, 0, 24, 12), None)],
        );
        assert!(
            escapes(&card, &mut stub_ink).is_empty(),
            "3 chars * 8 = 24 wide, 12 tall, in a 24x12 box"
        );
    }

    /// ★ A clip does not make an escape acceptable, it changes what the reader
    /// loses: the content is gone with nothing saying so.
    #[test]
    fn r1656_a_clipped_overhang_is_reported_as_clipped() {
        let inner = boxed(
            Rect::new(0, 0, 40, 20),
            "card",
            vec![text("a much longer string", Rect::new(0, 0, 40, 12), None)],
        );
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 40, 20),
            inner,
        ));
        let found = escapes(&scroll, &mut stub_ink);
        let cut: Vec<_> = found.iter().filter(|e| e.fate == Fate::Clipped).collect();
        assert!(
            !cut.is_empty(),
            "the scroll's clip cuts the overhang, and that is still a loss: {found:?}"
        );
    }

    /// ★★ R1685 — a clip a *container* declares cuts the same way, and is
    /// reported the same way.
    ///
    /// The fate is read off [`NodeVisit::clip`], which since R1685 is folded
    /// from the node's own declaration rather than from its kind — so this
    /// arrives with no arm here mentioning containers at all. It is asserted
    /// because "it falls out" is a claim about code, and this is the behaviour.
    ///
    /// ★ And the same escape under a container that declares NOTHING is
    /// `Smeared`, not `Clipped`: the two rows differ only in the declaration,
    /// which is what makes this a test of the declaration.
    #[test]
    fn r1685_an_overflow_container_cuts_the_overhang_and_says_so() {
        let long = || text("a much longer string", Rect::new(0, 0, 40, 12), None);
        let cutting = {
            let mut node =
                ContainerNode::new(vec![boxed(Rect::new(0, 0, 40, 20), "card", vec![long()])]);
            node.rect = Rect::new(0, 0, 40, 20);
            node.layout =
                crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);
            Scene::Container(node)
        };
        let smearing = {
            let mut node =
                ContainerNode::new(vec![boxed(Rect::new(0, 0, 40, 20), "card", vec![long()])]);
            node.rect = Rect::new(0, 0, 40, 20);
            Scene::Container(node)
        };

        let cut = escapes(&cutting, &mut stub_ink);
        assert!(
            cut.iter().any(|e| e.fate == Fate::Clipped),
            "the container declares the clip, so the overhang is cut and the \
             reader loses it silently: {cut:?}"
        );
        let smeared = escapes(&smearing, &mut stub_ink);
        assert!(
            smeared.iter().all(|e| e.fate == Fate::Smeared),
            "the same overhang under a container that declares nothing is \
             painted over its neighbours, not cut: {smeared:?}"
        );
        assert_eq!(
            cut.len(),
            smeared.len(),
            "the declaration changes the FATE of the escape, never whether it \
             is reported — a clip that hid the report would be the silence \
             this module exists to end"
        );
    }

    /// ★ A scroll's content is allowed to exceed its viewport, because that is
    /// what makes it scrollable — and the marks inside that content are still
    /// judged.
    #[test]
    fn r1656_scroll_content_is_not_an_escape_but_its_marks_still_are() {
        let card = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("row", Rect::new(0, 44, 40, 11), None)],
        );
        let mut world = ContainerNode::new(vec![card]);
        world.rect = Rect::new(0, 0, 4000, 4000);
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 200, 200),
            Scene::Container(world),
        ));
        let found = escapes(&scroll, &mut stub_ink);
        assert_eq!(
            found.len(),
            1,
            "the 4000px world is not an escape; the row below its card is: {found:?}"
        );
        assert_eq!(found[0].owner, "card");
    }

    /// ★ The overhang is per edge, because which edge it is decides the repair.
    #[test]
    fn r1656_the_overhang_names_the_edge() {
        let over = Overhang::of(Rect::new(5, 5, 100, 100), Rect::new(10, 10, 50, 50));
        assert_eq!(over.left, 5);
        assert_eq!(over.top, 5);
        assert_eq!(over.right, 45);
        assert_eq!(over.bottom, 45);
        assert_eq!(over.worst(), 45);
        assert!(!over.is_contained());
        assert!(Overhang::of(Rect::new(10, 10, 5, 5), Rect::new(10, 10, 50, 50)).is_contained());
    }

    /// ★★ R1674 — a caption band is subtracted from the content, and the parity
    /// case is the floor's own numbers.
    ///
    /// Probed by building and running it: a titled group box at 6.11 reports
    /// content margins of `3,23,3,3` inside a `100x40` box while a plain framed
    /// widget with a 2px line reports `2,2,2,2`. A 3px border plus a 20px
    /// caption is that first answer exactly, which is the point — the RECTANGLE
    /// is parity, and what the floor cannot carry is which twenty of those
    /// twenty-three pixels are the title.
    #[test]
    fn r1674_a_caption_band_comes_out_of_the_content_rectangle() {
        let box_rect = Rect::new(0, 0, 100, 40);
        let border = Border::new(crate::style::Color::rgb(0x33, 0x33, 0x33), 3);
        let caption = Chrome::caption(20);
        assert_eq!(
            content_of(box_rect, Some(&border), &[caption]),
            Rect::new(3, 23, 94, 14),
            "3px of border on every edge, then 20 more off the top",
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[]),
            Rect::new(3, 23 - 20, 94, 14 + 20),
            "the same box with no caption keeps those twenty pixels",
        );
    }

    /// ★ Every edge, and two bands on one edge sum.
    ///
    /// A `match` over [`ChromeEdge`] decides where a band is taken from, and
    /// the arm that moves the ORIGIN (top, left) is a different arm from the
    /// one that only shortens (bottom, right) — the asymmetry R1672 got wrong
    /// once already on `BorderPlacement`, in a `match` that looked complete.
    #[test]
    fn r1674_a_band_is_taken_from_the_edge_it_names() {
        let r = Rect::new(10, 20, 100, 60);
        let cases = [
            (ChromeEdge::Top, Rect::new(10, 30, 100, 50)),
            (ChromeEdge::Bottom, Rect::new(10, 20, 100, 50)),
            (ChromeEdge::Left, Rect::new(20, 20, 90, 60)),
            (ChromeEdge::Right, Rect::new(10, 20, 90, 60)),
        ];
        for (edge, want) in cases {
            let band = Chrome::new(edge, 10, ChromeRole::Header);
            assert_eq!(content_of(r, None, &[band]), want, "{edge:?}");
        }
        // A panel carrying a tab strip above a toolbar spends both.
        assert_eq!(
            content_of(
                r,
                None,
                &[
                    Chrome::new(ChromeEdge::Top, 10, ChromeRole::TabStrip),
                    Chrome::new(ChromeEdge::Top, 6, ChromeRole::Toolbar),
                ],
            ),
            Rect::new(10, 36, 100, 44),
            "two bands on one edge sum",
        );
    }

    /// ★ A band bigger than what is left takes all of it and leaves the origin
    /// on the far side, rather than underflowing.
    ///
    /// The `.max(0)` shape R1668 measured as a four-billion-pixel underflow, in
    /// the arithmetic that decides where a child goes.
    #[test]
    fn r1674_an_oversized_band_empties_the_content_without_underflowing() {
        let r = Rect::new(0, 0, 40, 30);
        let got = content_of(r, None, &[Chrome::caption(500)]);
        assert_eq!(got, Rect::new(0, 30, 40, 0), "empty, at the bottom edge");
        let got = content_of(
            r,
            None,
            &[Chrome::new(ChromeEdge::Left, 500, ChromeRole::Gutter)],
        );
        assert_eq!(got, Rect::new(40, 0, 0, 30), "empty, at the right edge");
    }

    /// ★★★ R1674 — what the mark landed on, named. The field the floor has no
    /// form for.
    ///
    /// Probed at 6.11: a custom-painted widget publishes its reservation with
    /// its four-integer content-margin setter with `3, 23, 3, 3`, and reading
    /// it back yields four integers indistinguishable from a 3px border with
    /// 20 more on top. So
    /// "this label is over the title" and "this label is out of bounds" arrive
    /// there as the same answer, and the repairs are not the same repair.
    #[test]
    fn r1674_an_escape_says_which_part_of_the_owner_it_landed_on() {
        let border = Border::new(crate::style::Color::rgb(0x33, 0x33, 0x33), 2);
        let owner_style = BoxStyle::filled(crate::style::Color::TRANSPARENT)
            .with_border(border)
            .with_chrome(Chrome::caption(20));
        // A label dropped at the owner's origin: over the border AND the caption.
        let intruder = text("Endpoint", Rect::new(0, 0, 60, 12), Some("intruder"));
        let mut owner = ContainerNode::new(vec![intruder]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = owner_style.clone();
        let found = escapes(&Scene::Container(owner), &mut |t| (t.rect.w, t.rect.h));
        assert_eq!(found.len(), 1, "one intruder, one report");
        assert_eq!(
            found[0].trespass,
            vec![Trespass::Border, Trespass::Chrome(ChromeRole::Caption)],
            "it covered the outline and it landed on the title, and both are said",
        );

        // The same label placed in the CONTENT is not an escape at all.
        let good = text("Endpoint", Rect::new(2, 22, 60, 12), Some("good"));
        let mut owner = ContainerNode::new(vec![good]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = owner_style;
        assert!(
            escapes(&Scene::Container(owner), &mut |t| (t.rect.w, t.rect.h)).is_empty(),
            "the content rectangle starts below the caption",
        );
    }

    /// ★★★ R1674 — a node that IS the chrome is judged against its band, and a
    /// node that merely sits in the band is not.
    ///
    /// The two halves are useless apart. Without the claim, declaring a caption
    /// makes every titled frame in the tree report its own title as an escape;
    /// without the declaration, whatever is drawn up there is exempt and a
    /// label that really did land on the caption goes unreported. This asserts
    /// both directions against one owner, so a change that collapses them into
    /// one answer fails here.
    #[test]
    fn r1674_a_chrome_node_answers_to_its_band_and_content_does_not() {
        let style =
            BoxStyle::filled(crate::style::Color::TRANSPARENT).with_chrome(Chrome::caption(20));
        let ink = &mut |t: &TextNode| (t.rect.w, t.rect.h);

        // Claims the caption, fits the caption: contained.
        let title = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 14))
                .with_tag("title")
                .with_layout(
                    crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Caption),
                ),
        );
        let mut owner = ContainerNode::new(vec![title]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style.clone();
        assert!(
            escapes(&Scene::Container(owner), &mut *ink).is_empty(),
            "the title is judged against the band it was given",
        );

        // Claims the caption, OUTGROWS the caption: reported. This is the
        // defect R1673 found in a group box's legend by accident, and the
        // reason the claim is not simply an exemption.
        let tall = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 30))
                .with_tag("tall")
                .with_layout(
                    crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Caption),
                ),
        );
        let mut owner = ContainerNode::new(vec![tall]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style.clone();
        let found = escapes(&Scene::Container(owner), &mut *ink);
        assert_eq!(
            found.len(),
            1,
            "a caption too tall for its band is reported"
        );
        assert_eq!(found[0].over.bottom, 12, "2 + 30 past a 20px band");

        // Claims a role the owner never reserved: judged as ordinary content,
        // NOT exempted. A band that was never taken from the content was never
        // taken from the content, and a typo in a role must not become an
        // exemption.
        let liar = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 14))
                .with_tag("liar")
                .with_layout(crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Footer)),
        );
        let mut owner = ContainerNode::new(vec![liar]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style;
        let found = escapes(&Scene::Container(owner), &mut *ink);
        assert_eq!(
            found.len(),
            1,
            "an unreserved role is not an exemption — it is content in the caption",
        );
        assert_eq!(
            found[0].trespass,
            vec![Trespass::Chrome(ChromeRole::Caption)]
        );
    }

    /// ★ The wire words are identity: two reads spell a role, and a rename
    /// would silently move an AI client's key on both.
    #[test]
    fn r1674_the_chrome_vocabulary_is_pinned_on_the_wire() {
        let roles = [
            (ChromeRole::Caption, "caption"),
            (ChromeRole::Header, "header"),
            (ChromeRole::TabStrip, "tab_strip"),
            (ChromeRole::Toolbar, "toolbar"),
            (ChromeRole::Gutter, "gutter"),
            (ChromeRole::Footer, "footer"),
        ];
        for (role, word) in roles {
            assert_eq!(role.wire_word(), word);
            assert_eq!(
                Trespass::Chrome(role).wire_word(),
                format!("chrome:{word}"),
                "one string carries the case and the role",
            );
        }
        for (edge, word) in [
            (ChromeEdge::Top, "top"),
            (ChromeEdge::Bottom, "bottom"),
            (ChromeEdge::Left, "left"),
            (ChromeEdge::Right, "right"),
        ] {
            assert_eq!(edge.wire_word(), word);
        }
        assert_eq!(Trespass::Outside.wire_word(), "outside");
        assert_eq!(Trespass::Border.wire_word(), "border");
    }

    /// ★★★★★ R1862 — **two elements of one row share a centre by construction,
    /// whatever their heights are.**
    ///
    /// The property, not the numbers: the pin legend that prompted this placed
    /// an 11-pixel sample and a 12-pixel label with the same hand-picked `+3`
    /// in an 18-pixel row, and the centres came out one apart.
    #[test]
    fn r1862_two_bands_of_a_row_share_a_centre_whatever_their_heights() {
        // ★★★★★ EQUALITY, not "within a pixel". A counterfactual moved the
        // legend's sample by one and this gate — written with a one-pixel
        // tolerance because the obvious derivation needed one — did not see it.
        // The defect being repaired was itself one pixel, so a tolerance the
        // size of the defect is a gate that cannot report it.
        let row = Rect::new(67, 522, 210, 18);
        for outer_h in 1..40u32 {
            let row = Rect::new(row.x, row.y, row.w, outer_h);
            for a in 1..=outer_h {
                for b in 1..=outer_h {
                    let one = super::band_in(row, row.x, 11, a);
                    let two = super::band_in(row, row.x + 20, 190, b);
                    let mid = |r: Rect| r.y + r.h / 2;
                    assert_eq!(
                        mid(one),
                        mid(two),
                        "in a {outer_h}-tall row, heights {a} and {b} do not share a centre"
                    );
                }
            }
        }
    }

    /// And the derivation the face-sized one is: same seat, same centre.
    #[test]
    fn r1862_a_line_rect_is_a_band_of_the_faces_height() {
        let row = Rect::new(67, 522, 210, 18);
        for px in 8..=14 {
            assert_eq!(
                super::line_rect_in(row, row.x, 190, px),
                super::band_in(row, row.x, 190, super::line_box(px)),
                "the face-sized band and the explicit one disagree at {px}px"
            );
        }
    }

    /// ⚠ A band taller than its seat still starts at the seat, rather than
    /// wrapping around: the caller is told nothing here, and the screen gates
    /// are what report a box that does not fit the row centring it.
    /// ⚠ A band taller than its seat is still centred on the seat's middle and
    /// simply overhangs it, except where that would put it above the window,
    /// which `saturating_sub` clamps. The caller is told nothing here; the
    /// screen gates are what report a box that does not fit the row centring it.
    #[test]
    fn r1862_a_band_taller_than_its_seat_still_shares_the_seats_centre() {
        let row = Rect::new(0, 100, 50, 10);
        let over = super::band_in(row, 0, 50, 30);
        assert_eq!(over.y + over.h / 2, row.y + row.h / 2);
        assert_eq!(over, Rect::new(0, 90, 50, 30));
    }

    /// ★★★★★ R1874 — **a one-line stack IS `line_rect_in`**, over every seat
    /// and face this tree plausibly uses.
    ///
    /// The generalisation has to agree with the element it generalises or it is
    /// a second answer to the same question, which is R1873's lesson about a
    /// gate that re-spells a rule. Asserted across a grid rather than at one
    /// point, because the two expressions round — and two roundings that agree
    /// on one sample are exactly the situation R1862 measured going wrong.
    #[test]
    fn r1874_a_stack_of_one_line_is_the_single_line_band() {
        for (h, y) in [(18_u32, 522_u32), (20, 100), (40, 0), (17, 3), (7, 41)] {
            let seat = Rect::new(67, y, 210, h);
            for px in 8..=24 {
                let [only] = super::stacked_line_rects(seat, seat.x, 190, [px]);
                assert_eq!(
                    only,
                    super::line_rect_in(seat, seat.x, 190, px),
                    "a one-line stack and the single-line band disagree at \
                     {px}px in a {h}px seat at y={y}"
                );
            }
        }
    }

    /// ★★★★★ R1874 — every line of a stack holds its own face, the lines abut
    /// exactly, and the block shares the seat's centre.
    ///
    /// The three properties the palette's role rows needed and did not have:
    /// the `+6` and `+20` they were authored with related the two lines to
    /// nothing, so any one of the three could break without the other two
    /// noticing.
    #[test]
    fn r1874_a_stack_holds_each_face_abuts_and_is_centred_as_a_block() {
        let row = Rect::new(10, 100, 160, 40);
        let faces = [12_u32, 10];
        let [name, gist] = super::stacked_line_rects(row, row.x + 20, 140, faces);

        // 1. each line is its own face's line box, so `short_by` is 0 for a run
        //    placed in it — asserted through the predicate, not a number here.
        for (rect, px) in [(name, faces[0]), (gist, faces[1])] {
            let mut node = TextNode::new("gjpqy", rect);
            node.style.font_size_px = px;
            assert_eq!(
                super::short_by(&node),
                0,
                "a {px}px line of the stack is {}px tall",
                rect.h,
            );
        }
        // 2. they abut: no gap a reader sees as a stray band, no overlap.
        assert_eq!(gist.y, name.y + name.h, "the two lines do not abut");
        // 3. the BLOCK shares the seat's centre, by `band_in`'s rule.
        let total = name.h + gist.h;
        assert_eq!(name.y, (row.y + row.h / 2) - total / 2);

        // A stack taller than its seat overhangs rather than being squeezed:
        // the lines keep their faces and the screen gates report the seat.
        let tight = Rect::new(0, 100, 50, 10);
        let [a, b] = super::stacked_line_rects(tight, 0, 50, [16, 16]);
        assert_eq!((a.h, b.h), (super::line_box(16), super::line_box(16)));
        assert_eq!(b.y, a.y + a.h);
    }

    /// A span fixture: ink `w` wide and 12 tall, sitting `dx` right of the run's
    /// own rectangle. No font in it, so these assertions are about the POLICY
    /// and never about a shaper — [`stub_ink`]'s reason, one round later and one
    /// question wider.
    fn stub_span(dx: u32, w: u32) -> impl FnMut(&TextNode) -> InkSpan {
        move |_| InkSpan { dx, w, h: 12 }
    }

    /// ★★★★★ R1904 — **the ink is reported where the shaper put it**, which is
    /// neither the run's own rectangle nor that rectangle added to its owner's
    /// origin.
    ///
    /// Two separate wrongs met in this one line, and both were invisible for
    /// the same reason: [`Slack::ink`]'s only consumer read
    /// [`Slack::spare_w`], and a spare is a difference of extents that neither
    /// a wrong origin nor a missing offset disturbs. ⇒ **a field can be wrong
    /// for as long as it goes unasked.**
    ///
    /// * the origin was counted twice (`box_rect.x + child.rect().x`) even
    ///   though a laid-out child's rectangle is already window absolute — on
    ///   the assembled screen that put a cell's ink far outside the cell;
    /// * and the alignment offset was not counted at all, so a run an
    ///   alignment had moved was reported at the place it would have inked
    ///   without one. That is the half a person reading the running window
    ///   reported: a byte flush left inside a chain of boxes each of which was
    ///   exactly centred.
    ///
    /// The fixture is the reported shape — a 22-wide cell, a band inset two
    /// inside it, a 10-wide byte in the band — so a regression reads as the
    /// screen it came from.
    #[test]
    fn r1904_slack_reports_the_ink_where_the_shaper_put_it() {
        let cell = |child: Scene| boxed(Rect::new(100, 50, 22, 18), "cell", vec![child]);
        let band = || text("0d", Rect::new(102, 50, 18, 18), None);

        // ★ Flush. The `102` is the assertion: `100 + 102` is the double count,
        // and `100` alone would be the offset being dropped.
        let found = slack(&cell(band()), &mut stub_span(0, 10));
        assert_eq!(found.len(), 1, "one box holds something: {found:?}");
        assert_eq!(
            found[0].ink,
            Rect::new(102, 50, 10, 12),
            "the ink sits at the run's own place in the window",
        );
        assert_eq!(
            found[0].off_centre().x,
            Centring::Within(-8),
            "and it is off centre towards the near side, which is what a person \
             saw: two before it and ten after",
        );

        // ★★ The SAME box and the SAME run, moved by an alignment. Nothing in
        // the scene changed — only what the shaper answered — so a channel that
        // reads rectangles cannot tell these two apart and this one must.
        let centred = slack(&cell(band()), &mut stub_span(4, 10));
        assert_eq!(
            centred[0].ink,
            Rect::new(106, 50, 10, 12),
            "an alignment moved the glyphs inside the box, and the ink says so",
        );
        assert_eq!(
            centred[0].off_centre().x,
            Centring::Within(0),
            "six each side of a 10-wide run in a 22-wide cell",
        );

        // ★★★ A border is not room the content failed to use: `inner` is the
        // content rectangle, and `off_centre` is measured against it. Without
        // this a bordered box would be called off centre for drawing its own
        // outline.
        let mut framed = ContainerNode::new(vec![text("0d", Rect::new(106, 51, 10, 16), None)]);
        framed.rect = Rect::new(100, 50, 22, 18);
        framed.tag = Some("framed".to_owned().into());
        framed.style = BoxStyle::filled(crate::style::Color::rgb(0x10, 0x10, 0x10)).with_border(
            crate::style::Border::new(crate::style::Color::rgb(0xEC, 0x5A, 0xA0), 1),
        );
        let bordered = slack(&Scene::Container(framed), &mut stub_span(0, 10));
        assert_eq!(
            bordered[0].inner,
            Rect::new(101, 51, 20, 16),
            "the CONTENT rectangle, the border's own pixels excluded",
        );
        assert_eq!(
            bordered[0].off_centre().x,
            Centring::Within(0),
            "and a 10-wide run centred in that content rectangle is centred",
        );
    }

    /// ★★★★★ R1904 — **a box smaller than what it holds has no centre**, and
    /// [`Centring`] says so rather than answering the best number there is.
    ///
    /// This is the round's own escape hatch, found by its closing audit and
    /// closed by it. The margins are computed with `saturating_sub`, so the
    /// first draft — a bare `i64` — answered `0 - 0` for an overflowing box:
    /// *perfectly centred*, for the one case where the question does not apply.
    /// Whatever a caller did with that number, the unanswerable case read as
    /// the ideal.
    ///
    /// The screen gate that consumes this happened to be safe, because it
    /// asserts the room before it asserts the centring — but that is a caller
    /// remembering rather than the type refusing, and the next caller does not
    /// remember. ⇒ the standing rule that **an unclassified case is a RED and
    /// not a pass**, applied to a return type.
    ///
    /// Overflow itself is not this channel's verdict to give: whether ink
    /// leaving its box is a defect is [`escapes`]'s question, and it is
    /// answered there with a per-edge overhang. This one only refuses to
    /// pretend it has an answer.
    #[test]
    fn r1904_a_box_smaller_than_what_it_holds_has_no_centre() {
        let cell = |child: Scene| boxed(Rect::new(100, 50, 22, 18), "cell", vec![child]);
        let run = || text("0d", Rect::new(102, 50, 18, 18), None);

        // Wider than the box it sits in: no centre on that axis.
        let wide = slack(&cell(run()), &mut stub_span(0, 30));
        assert_eq!(wide[0].off_centre().x, Centring::Overflows);
        assert_eq!(
            wide[0].spare_w, 0,
            "the premise: a spare saturates, which is why a difference of \
             margins would have answered zero here",
        );
        assert_eq!(
            wide[0].off_centre().y,
            Centring::Within(-6),
            "and the axes are answered independently — a run too wide is still \
             somewhere in particular vertically",
        );

        // Ink starting LEFT of the box is the same refusal: it is not within,
        // so there is no pair of margins to compare.
        let before = slack(
            &boxed(
                Rect::new(100, 50, 22, 18),
                "cell",
                vec![text("0d", Rect::new(90, 50, 18, 18), None)],
            ),
            &mut stub_span(0, 10),
        );
        assert_eq!(before[0].off_centre().x, Centring::Overflows);

        // Taller than the box: the vertical axis refuses on its own.
        let tall = slack(&cell(run()), &mut |_| InkSpan {
            dx: 0,
            w: 10,
            h: 30,
        });
        assert_eq!(tall[0].off_centre().y, Centring::Overflows);
        assert_eq!(
            tall[0].off_centre().x,
            Centring::Within(-8),
            "while the horizontal one still answers",
        );
    }

    /// ★★★★★ R1956 — **[`uncentred`] reports the hand-spelled centring and
    /// not [`band_in`]'s**, which is the whole of its value: a check that
    /// cannot be held at zero on correct code is a check that gets a pin
    /// instead of a floor.
    ///
    /// The counterfactual is inside the test rather than beside it, because
    /// what is being asserted is a DIFFERENCE between two placements — running
    /// only the correct one would pass against a function that reports nothing
    /// at all, and running only the naive one would pass against a function
    /// that reports everything.
    ///
    /// The four cases are the four answers the population has: placed by the
    /// framework (silent), placed by hand (named), stacked rather than beside
    /// (out of it whatever its centres are), and two marks of one drawing with
    /// no run between them (out of it too — an icon's strokes are not two
    /// things placed side by side, which is what the first draft of this called
    /// a defect on a real screen).
    #[test]
    fn r1956_the_hand_spelled_centring_is_the_one_reported() {
        use crate::style::{BoxStyle, Color};

        let seat = Rect::new(0, 0, 200, 36);
        let ink = Color::rgb(0x40, 0x40, 0x40);
        // A run, because a centre line is what a run is placed on: the pair
        // this reports is always *a run against the thing beside it*.
        let run = |rect: Rect, tag: &'static str| text("x", rect, Some(tag));
        let boxed = |rect: Rect, tag: &'static str| {
            Scene::Box(BoxNode::new(rect, BoxStyle::filled(ink)).with_tag(tag))
        };
        // Two heights of different parity in one 36px band — the pair R1862 was
        // opened for. Both are pressed against the seat, side by side.
        let (short_h, tall_h) = (17, 20);
        let in_seat = |children: Vec<Scene>| {
            let mut row = ContainerNode::new(children);
            row.rect = seat;
            row.tag = Some("row".to_owned().into());
            Scene::Container(row)
        };

        // 1. Placed by `band_in`: nothing to report, and the two centres are
        //    equal rather than merely close.
        let a = band_in(seat, 10, 30, short_h);
        let b = band_in(seat, 50, 30, tall_h);
        assert_eq!(
            centre_line(a),
            centre_line(b),
            "band_in rounds once, so a seat's bands share one centre exactly"
        );
        assert_eq!(
            uncentred(&in_seat(vec![boxed(a, "pin"), run(b, "label")])),
            Vec::new(),
            "the framework's own placement must be reportable at ZERO"
        );

        // 2. Spelled by hand — `outer.y + (outer.h - h) / 2`, the form every
        //    site in this tree had — rounds a second time and separates them.
        let naive = |h: u32, x: u32| Rect::new(x, seat.y + (seat.h - h) / 2, 30, h);
        let (na, nb) = (naive(short_h, 10), naive(tall_h, 50));
        let found = uncentred(&in_seat(vec![boxed(na, "pin"), run(nb, "label")]));
        assert_eq!(found.len(), 1, "expected the one pair, got {found:?}");
        assert_eq!(found[0].seat, "row", "the report names the seat");
        assert_eq!(
            found[0].centres(),
            (centre_line(na), centre_line(nb)),
            "and prints both centre lines, so the pixel is legible from the report"
        );
        assert_eq!(
            found[0].centres().0.abs_diff(found[0].centres().1),
            1,
            "the separation this reports is the second rounding's whole range"
        );

        // 3. Stacked rather than beside: out of the population. `naive` puts
        //    these at centres one apart too — the pair is excluded by WHERE the
        //    boxes stand, not by how far apart their centres are.
        let stacked = uncentred(&in_seat(vec![
            run(Rect::new(10, na.y, 30, short_h), "over"),
            run(Rect::new(10, nb.y + tall_h, 30, tall_h), "under"),
        ]));
        assert_eq!(
            stacked,
            Vec::new(),
            "a stack makes no claim to share a centre line: {stacked:?}"
        );

        // 4. The same hand-spelled separation between two marks of one
        //    drawing. An icon's dots are one picture, and a picture's strokes
        //    stand where its author put them — measured on the assembled
        //    application, this was the biggest group the first draft reported
        //    and every one of them was a rail icon, on six destinations.
        let drawing = uncentred(&in_seat(vec![boxed(na, "dot"), boxed(nb, "dot")]));
        assert_eq!(
            drawing,
            Vec::new(),
            "two marks of one drawing are not two things placed side by \
             side: {drawing:?}"
        );
    }
}
