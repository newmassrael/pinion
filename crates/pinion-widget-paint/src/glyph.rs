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

/// Collapsed-state disclosure twisty — `U+25B6` BLACK RIGHT-POINTING TRIANGLE.
pub const DISCLOSURE_COLLAPSED: &str = "\u{25B6}";

/// Expanded-state disclosure twisty — `U+25BC` BLACK DOWN-POINTING TRIANGLE.
pub const DISCLOSURE_EXPANDED: &str = "\u{25BC}";

/// Ascending column-sort arrow — `U+25B2` BLACK UP-POINTING TRIANGLE.
pub const SORT_ASCENDING: &str = "\u{25B2}";

/// Descending column-sort arrow — `U+25BC` BLACK DOWN-POINTING TRIANGLE.
pub const SORT_DESCENDING: &str = "\u{25BC}";

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

/// Minimize control — `U+2212` MINUS SIGN (a centred bar reads as minimize).
pub const WINDOW_MINIMIZE: &str = "\u{2212}";

/// Maximize / restore control — `U+25A1` WHITE SQUARE.
pub const WINDOW_MAXIMIZE: &str = "\u{25A1}";

/// Close control — `U+00D7` MULTIPLICATION SIGN.
pub const WINDOW_CLOSE: &str = "\u{00D7}";

// (R1562 §5.27 §5.40) The grid corner's tri-state select-all marks. Both glyphs
// are already painted elsewhere in this crate — the R668 checkbox's check and
// the window-control minus above — so the corner adds no font obligation, and
// the two controls that mean "checked" cannot come out looking like different
// ideas. The EMPTY extent draws no glyph at all: an unchecked box is the
// absence of a mark, which is how `crate::checkbox` paints it too.

/// Select-all in its **indeterminate** leg — `U+2212` MINUS SIGN, the dash an
/// HTML `<input type=checkbox>.indeterminate` draws.
pub const SELECT_ALL_PARTIAL: &str = "\u{2212}";

/// Select-all with **everything** selected — `U+2713` CHECK MARK.
///
/// 🟥 R1952 — this used to say *"the same glyph `crate::checkbox` paints when
/// checked"*, and it stopped being true at **R1674**, which moved the
/// checkbox's tick from a character to a stroked polyline and wrote down why:
/// *the commonest glyph in the catalog stops depending on the host's fonts.*
/// Nothing performed the sentence, so the corner kept the character and the
/// two controls that mean "checked" came out as different ideas — the exact
/// outcome the comment beside it says must not happen. Measured: no face this
/// tree ships has `U+2713` at all. See [`FACELESS`].
pub const SELECT_ALL_COMPLETE: &str = "\u{2713}";

/// R886.1 §5.50 — the sort-direction → glyph mapping every column header
/// paints: `Some(true)` → [`SORT_ASCENDING`], `Some(false)` →
/// [`SORT_DESCENDING`], `None` (not the active sort column) → `None` so
/// each consumer renders its own unsorted representation. Pairs with
/// `pinion_core::widgets::grid_sort::col_sort_dir` (the "is THIS column
/// active" decision) on the input side.
///
/// ⚠ R1952 — the substrate's own header painters no longer call this. A column
/// header carries the DIRECTION and
/// [`crate::indicator::Indicator::of_sort`] draws it, because the face this
/// tree ships has no glyph for either arrow. This stays for the five example
/// screens that hand-roll their own header rows; each of them paints a box
/// today, which is the debt [`FACELESS`] counts.
#[must_use]
pub const fn sort_glyph(dir: Option<bool>) -> Option<&'static str> {
    match dir {
        Some(true) => Some(SORT_ASCENDING),
        Some(false) => Some(SORT_DESCENDING),
        None => None,
    }
}

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
pub const FACELESS: usize = 6;

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
        let mut out = Vec::new();
        for line in include_str!("glyph.rs").lines() {
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

        // ★ The parse must actually find the constants, or every line below
        // passes over an empty population. Cross-checked against the COMPILED
        // values, so a parser that quietly stopped matching is red here rather
        // than green everywhere.
        for known in [
            super::WINDOW_CLOSE,
            super::SELECT_ALL_COMPLETE,
            super::DISCLOSURE_COLLAPSED,
        ] {
            assert!(
                declared.iter().any(|(_, text)| text == known),
                "the source parse missed {known:?}, so this gate is asking \
                 about a population that is not this module's",
            );
        }

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
        assert!(
            declared.len() > FACELESS,
            "every mark this module declares is faceless, which means the \
             comparison is answering about the parse rather than about the face",
        );
    }
}
