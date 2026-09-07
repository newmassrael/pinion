//! R873 §5.50 — shared paint-glyph SSOT.
//!
//! The **disclosure twisty** — the collapsed (`U+25B6` BLACK RIGHT-POINTING
//! TRIANGLE) / expanded (`U+25BC` BLACK DOWN-POINTING TRIANGLE) pair — is one
//! affordance used by every collapsible surface in the catalog: a
//! [`crate::disclosure`] section, a [`crate::tree_view`] branch, and a
//! [`crate::group_header`] category row all show the *same* twisty so they read
//! as the same gesture. Before R873 each module re-declared the pair privately
//! (three byte-identical copies, each doc-cross-referencing the others as "the
//! same glyph") — the Rule-of-Three SSOT miss the R758 self-grep mandate names
//! ([[self-grep-count-all-sites-not-just-new-pair]]). They lift here.
//!
//! The **column-sort direction** pair (`U+25B2` ascending / `U+25BC`
//! descending) lifts here too (R886.1) — by then FIVE same-semantic copies
//! existed (this crate's table header + four grid examples), the same
//! Rule-of-Three class as the twisty. It stays a *separate* affordance
//! from the disclosure pair: `U+25BC` recurring in both is a glyph
//! coincidence, not a shared gesture, so the two pairs are distinct
//! constants (merging them would be the R735.1 wrong abstraction). The
//! datepicker month-nav arrows (`U+25C0` / `U+25B6`) remain deliberately
//! un-lifted for the same semantics reason. A consumer's *unsorted*
//! representation (`""`, `"\u{2195}"`, a fixed-width blank) is a style
//! choice, not a shared decision — it stays per-consumer (R758).

// ★★★★★ R2057 — the two disclosure twisties are GONE from this module.
//
// They were `U+25B6` and `U+25BC`, and the one face this tree renders through
// carries neither, so every widget that drew one — the disclosure section, the
// tree view, the group header, and an example's tree rows — showed a `.notdef`
// box where the twisty belongs. R1952 had already reached this conclusion for
// the four marks the analysis shell paints, and named the reason: this is a
// Latin/Greek/Cyrillic text face, so every triangle, chevron and arrow asked of
// it is outside it BY CONSTRUCTION, and the behaviour reference draws its marks
// as paths for exactly that reason — measured, it uses none of these
// codepoints anywhere at all.
//
// The mark is now `Indicator::Disclosure`, drawn as a path. Removing the
// constants rather than leaving them unused is what takes them out of
// [`FACELESS`]: a declared mark this tree cannot draw is a promise it cannot
// keep, whether or not anybody is currently calling it.

// ★★★★★ R2058 — the two column-sort arrows are GONE, and so is the mapping
// that handed them out.
//
// They were `U+25B2` / `U+25BC`. R1952 moved this crate's own header painters
// onto [`crate::indicator::Indicator::of_sort`] and left the pair here for the
// example screens that build their own header rows — which meant those screens
// went on painting a box. R2058 moved all four of them, so nothing asks for a
// sort character any more.
//
// ⚠ Their unsorted representation went with them. That was documented as a
// per-consumer style choice and it was entitled to be one; what it was not
// entitled to be is `U+2195`, which this tree cannot draw either. An unsorted
// column shows no mark now — which is what `of_sort` answers for it, and what
// "no direction yet" honestly looks like.

// (R1171 §5.16) Window-control glyphs for a floating dock panel's HEADER controls
// (minimize / maximize / close). Text glyphs — the widget-layer convention (like
// the disclosure twisty above) — so they lay out with the header font + flex and
// auto-size to the header height, NOT a fixed-pixel shell overlay the binding has
// to dimension-match (the R1170 smell the controls-in-header redesign cleared).
//
// 🟥 R1952 — this comment used to end *"Chosen from blocks the bundled fonts
// cover"*, and that sentence was **false**, in the direction that matters:
// measured with `Font::glyph_id_for` against the face
// `pinion_text::test_font` calls *one face across the tree*, `U+25A1` is not in
// it. `U+2212` and `U+00D7` are. See [`FACELESS`] for the whole census and for
// why the sentence could stand for 780 rounds: nothing performed it.

// ★★★★★ R2059 — all three window-control characters are GONE, and the trio
// moved together rather than only the broken one.
//
// `U+25A1` was the faceless one, so a torn-off panel's maximise button was a
// box between two controls that happened to render. The other two are drawable
// today — which is not a reason for a control to stay a character: R1952 moved
// `U+00D7` out of the config form on exactly that argument, and a trio drawn
// half in text and half in paths is a trio free to stop looking like each
// other.
//
// ⚠ WHAT THAT COST, stated because the test asserting it said so first: a
// character lays out with the header font and flex, where a path needs its
// dimensions given. The dock's controls now give them. That convenience was
// real, and it was being paid for with a mark a third of a reader's window
// could not draw.
//
// They are `ControlMark::{Minimize, Maximize, Close}` now — and `Minimize` is
// new this round, added so the third of the trio had a face to move to.

// ★★★★★ R2060 — the grid corner's tri-state marks are GONE, and this module
// now declares no mark at all.
//
// The comment that stood here justified two characters by saying both were
// "already painted elsewhere in this crate — the checkbox's check and the
// window-control minus". BOTH HALVES HAD STOPPED BEING TRUE: R1674 made the
// checkbox's tick a stroked polyline, and R2059 removed the minus when the
// window controls became marks. So the justification pointed at two things that
// no longer existed, while `U+2713` — absent from BOTH faces this tree ships —
// left the checked corner a `.notdef` box: a control whose whole job is to say
// *everything here is selected*, saying it illegibly.
//
// ⇒ the corner draws `crate::checkbox`'s own mark now, which is what that
// comment always MEANT by "cannot come out looking like different ideas": one
// shape with two consumers, not two chances to draw a check. The dash went with
// it — its character IS in the face and was not a defect, but a tri-state
// control drawn half in text and half in paths is the shape R2059 removed from
// the window controls, and for the same reason.
//
// The EMPTY extent still draws nothing: an unchecked box is the ABSENCE of a
// mark, which is how `crate::checkbox` paints it too.

/// ★★★★★ R1952 — **how many marks this module declares that the face this tree
/// ships cannot draw.**
///
/// A PIN, not a ceiling. Each one is a `.notdef` box wherever it is painted, so
/// this number going up is a new defect and it going down is the repair;
/// `r1952_this_modules_marks_are_counted_against_the_face_this_tree_ships`
/// refuses either without somebody moving the number here on purpose.
///
/// # Why the number is not zero
///
/// R1952 repaired every mark the analysis shell — the screen this project is
/// judged on — actually paints. What is left are marks **no destination of that
/// shell draws**: the disclosure twisty (`U+25B6` / `U+25BC`), the maximise
/// control (`U+25A1`), and the select-all tick (`U+2713`), plus the sort pair
/// which the substrate's own painters no longer use. Each is a box on any
/// screen that does paint it. They are a registered debt rather than this
/// round's work, because the population that decided this round's scope was
/// *what a person sees on the shell*, not *what a module declares*.
///
/// ⚠ The declarations that are NOT counted here are the ones the face has —
/// `U+2212` and `U+00D7`. They are not exempt: they are simply not faceless. A
/// mark being drawable by today's face is not a reason for it to stay a
/// character, and R1952 moved `U+00D7` out of [`crate::config_form`] for
/// exactly that reason. It is a reason it is not a DEFECT.
/// ★★★★★ R2057/R2058 — **six became two**, in two instalments.
///
/// R2057 took the disclosure twisties, drawn by four painters in this crate and
/// an example's tree rows. R2058 took the column-sort arrows, whose remaining
/// callers were four example screens building their own header rows — and with
/// them the `U+2195` those screens used for an unsorted column.
///
/// ★★★★★ R2060 — **ZERO**, and this module declares no mark at all any more.
///
/// Six were counted when this pin was written. R2057 took the disclosure
/// twisties, R2058 the column-sort arrows and the unsorted marker beside them,
/// R2059 the three window controls, and R2060 the grid corner's tri-state pair.
/// Every one of them is a drawn path now.
///
/// ⚠ THE PIN STAYS, and staying at zero is the point. This module can still
/// declare a mark — nothing stops the next round adding a constant — and the
/// gate below asks the face about every one it declares. Zero is a state to be
/// held, not a reason to delete the question: the sentence that stood here for
/// 780 rounds claimed the characters were "chosen from blocks the bundled fonts
/// cover", and it was false the whole time because nothing performed it.
///
/// ★ What this does NOT cover, said rather than left to be found: the count
/// reads what THIS MODULE declares. A screen spelling its own characters is
/// invisible to it, and R2058 measured that more than one example does. The
/// instrument that can see that asks a RUNNING screen what it paints and holds
/// every character against the face; it exists, and it runs on one screen.
pub const FACELESS: usize = 0;

#[cfg(test)]
mod tests {
    use super::FACELESS;
    use pinion_text_font::Font;

    /// The one face this tree renders through — the same `NotoSans-Regular.ttf`
    /// `pinion_text::test_font` calls *one face across the tree* and
    /// `pinion-shell` installs for its pixel guards.
    fn tree_face() -> Font {
        const NOTO: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
        Font::from_bytes(NOTO.to_vec()).expect("the face this tree ships parses")
    }

    /// Every mark this module declares, as `(name, the string)`, read out of
    /// the module's own source.
    ///
    /// ★★★★★ Parsed rather than listed. A list written in this test is a
    /// population that silently misses the next constant — and a gate whose
    /// population can lose members reports afterwards as though it had covered
    /// them (R1651.1). `include_str!` of the file being compiled is the only
    /// population that grows when the module does.
    fn declared() -> Vec<(String, String)> {
        declared_in(include_str!("glyph.rs"))
    }

    /// The same parse, PURE in its text.
    ///
    /// ★★★★★ R2060 — split out when this module reached ZERO declarations. The
    /// cross-check below used to prove the parser worked by naming constants it
    /// must find; with none left, that check had nothing to assert and would
    /// have passed just as well with a parser that matched nothing — the
    /// vacuous pass this project keeps paying for. A fixture the parse must
    /// find restores the question, and it keeps working however few marks this
    /// module declares.
    fn declared_in(source: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for line in source.lines() {
            let Some(rest) = line.strip_prefix("pub const ") else {
                continue;
            };
            let Some((name, tail)) = rest.split_once(": &str = ") else {
                continue;
            };
            let mut text = String::new();
            let mut chars = tail.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '\\' || chars.peek() != Some(&'u') {
                    continue;
                }
                chars.next();
                let hex: String = chars
                    .by_ref()
                    .skip_while(|c| *c == '{')
                    .take_while(|c| *c != '}')
                    .collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    text.push(ch);
                }
            }
            if !text.is_empty() {
                out.push((name.to_owned(), text));
            }
        }
        out
    }

    /// ★★★★★ R1952 — **the module's claim about its own glyphs, performed.**
    ///
    /// The comment on the window controls said they were *"chosen from blocks
    /// the bundled fonts cover"*. Nothing ever asked a font, and the sentence
    /// was false for one of the three it was written about — and for three more
    /// declared above it. This asks, every run, and pins the answer.
    #[test]
    fn r1952_this_modules_marks_are_counted_against_the_face_this_tree_ships() {
        let face = tree_face();
        let declared = declared();

        // ★★★★★ The parse must be capable of finding something, or every line
        // below passes over an empty population.
        //
        // ⚠ R2060 — this used to prove that by naming constants the module
        // declares, which worked while it declared any. It declares NONE now,
        // so that form would have become an empty loop asserting nothing — and
        // the whole gate would then have been a parser that matched nothing
        // agreeing with a module that has nothing, forever green. A FIXTURE
        // keeps the question alive at zero.
        let fixture = "pub const EXAMPLE: &str = \"\\u{2713}\";";
        assert_eq!(
            declared_in(fixture),
            vec![("EXAMPLE".to_owned(), "\u{2713}".to_owned())],
            "the parse cannot read a declaration it is handed, so what it \
             reports about this module means nothing",
        );

        let faceless: Vec<&str> = declared
            .iter()
            .filter(|(_, text)| {
                text.chars()
                    .any(|c| !matches!(face.glyph_id_for(c as u32), Some(g) if g != 0))
            })
            .map(|(name, _)| name.as_str())
            .collect();

        assert_eq!(
            faceless.len(),
            FACELESS,
            "the budget is a PIN, not a ceiling: the face this tree ships \
             cannot draw {faceless:?}. If that list grew, a mark added here \
             paints a box — draw it with `crate::indicator` instead. If it \
             shrank, lower `FACELESS`.",
        );
        // ★★★★★ R2060 — this said `declared.len() > FACELESS`, guarding against
        // a comparison that was really about the parse: if EVERY declared mark
        // were faceless, the count could be right for the wrong reason.
        //
        // At zero declarations that guard asks the wrong question and fails —
        // there is nothing to compare, which is the goal rather than a defect.
        // What it MEANT is kept: whenever this module declares anything, at
        // least one of them must be drawable, or the face is not what is being
        // measured. The parser fixture above is what holds the gate honest when
        // the module declares nothing at all.
        assert!(
            declared.is_empty() || declared.len() > faceless.len(),
            "every mark this module declares is faceless, which means the \
             comparison is answering about the parse rather than about the face",
        );
    }
}
