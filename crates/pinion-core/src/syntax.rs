//! R904 §5.22 §5.36 — a minimal **syntax highlighter** for the text editor.
//!
//! The depth slice that turns a [`TextField`](crate::widgets::text_field) into a
//! code editor: derive the styled runs from the buffer *content* instead of from
//! manual formatting. [`highlight_code`] is a pure scanner over a C-family
//! grammar — line comments (`// …`), double-quoted strings (with `\` escapes),
//! numeric literals, and a caller-supplied keyword set — emitting one
//! [`StyleRun`] per coloured token (uncoloured text — identifiers, punctuation,
//! whitespace — carries no run and paints in the field's base style).
//!
//! ## Seam, not a framework `Language` trait
//!
//! There is no `Language` trait or registry: the configurable `keywords` slice
//! covers every C-family dialect by swapping the set, and the pluggability seam
//! the editor actually binds is
//! [`TextEditState::attach_highlighter`](crate::widgets::text_edit::TextEditState::attach_highlighter),
//! which takes *any* `Fn(&str) -> Vec<StyleRun>`. A second grammar (a non-C
//! tokeniser) is a sibling free fn behind that same closure seam, not a trait to
//! abstract over one consumer ([[abstraction-needs-second-consumer]]).
//!
//! ## Why runs carry the font size
//!
//! A [`StyleRun`]'s [`TextStyle`] is *fully resolved* — the paint adapter shapes
//! the run's glyphs at the run's own size, and [`TextStyle::default`] is size 0
//! (invisible). So [`highlight_code`] stamps the field's `font_size_px` onto
//! every run, overriding only the colour (the Qt `setCharFormat` shape the
//! manual styling path already uses): the base font flows through, the token
//! colour overlays.
//!
//! ## Scope (honest boundaries)
//!
//! - **Re-scan, not incremental.** Each call re-tokenises the whole buffer; the
//!   derived-`style_runs` accessor calls it per paint (the `find_matches`
//!   re-derive shape). Incremental / memoised highlighting is a later additive
//!   axis (no measured large-file consumer yet).
//! - **Fixed scheme.** [`SyntaxPalette`] is a dedicated *syntax* colour scheme,
//!   intentionally separate from the UI [`ColorRole`](crate::theme::ColorRole)
//!   palette (editors theme code independently). A theme-reactive syntax palette
//!   is a later axis.
//! - **One grammar family.** Block comments, char literals, raw strings, and
//!   number suffixes are out of scope for the first consumer.

use crate::scene::StyleRun;
use crate::style::{Color, TextStyle};

/// R904 §5.36 — a syntax colour scheme: one [`Color`] per token class. A
/// *syntax* theme, deliberately separate from the UI `ColorRole` palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxPalette {
    /// Language keywords (`fn`, `let`, `if`, …).
    pub keyword: Color,
    /// Double-quoted string literals.
    pub string: Color,
    /// `// …` line comments.
    pub comment: Color,
    /// Numeric literals.
    pub number: Color,
}

impl SyntaxPalette {
    /// A classic light-background scheme (VS Code "Light+" inspired): blue
    /// keywords, dark-red strings, green comments, teal numbers. Distinct hues
    /// that read on a *light* surface.
    #[must_use]
    pub const fn classic() -> Self {
        Self {
            keyword: Color::rgb(0x00, 0x00, 0xC0),
            string: Color::rgb(0xA3, 0x15, 0x15),
            comment: Color::rgb(0x00, 0x80, 0x00),
            number: Color::rgb(0x09, 0x86, 0x58),
        }
    }

    /// R906 §5.36 — a dark-background scheme (VS Code "Dark+" inspired): the
    /// light hues are illegible on a dark surface, so the dark variant lifts
    /// them — sky-blue keywords, salmon strings, muted-green comments,
    /// pale-green numbers. A binding picks [`classic`](Self::classic) /
    /// [`dark`](Self::dark) by the effective theme scheme
    /// ([`ThemeProvider::is_dark`](crate::theme::ThemeProvider::is_dark)) so
    /// the code reads against whichever surface the field paints.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            keyword: Color::rgb(0x56, 0x9C, 0xD6),
            string: Color::rgb(0xCE, 0x91, 0x78),
            comment: Color::rgb(0x6A, 0x99, 0x55),
            number: Color::rgb(0xB5, 0xCE, 0xA8),
        }
    }

    /// R906 §5.36 — the scheme for the effective theme: [`dark`](Self::dark)
    /// when `is_dark`, else [`classic`](Self::classic).
    #[must_use]
    pub const fn for_dark(is_dark: bool) -> Self {
        if is_dark {
            Self::dark()
        } else {
            Self::classic()
        }
    }
}

impl Default for SyntaxPalette {
    fn default() -> Self {
        Self::classic()
    }
}

/// R904 §5.22 §5.36 — tokenise `text` against the C-family grammar and the
/// caller's `keywords`, returning one [`StyleRun`] per coloured token (byte
/// ranges, in order, non-overlapping). Every run carries `font_size_px` so the
/// painted glyphs keep the field's size and only take the token colour.
/// Uncoloured spans (identifiers, punctuation, whitespace) get no run. The pure
/// scanner the [`attach_highlighter`](crate::widgets::text_edit::TextEditState::attach_highlighter)
/// closure wraps; unit-testable without a reactive scope (the `find_matches_in`
/// sibling shape).
#[must_use]
pub fn highlight_code(
    text: &str,
    keywords: &[&str],
    palette: SyntaxPalette,
    font_size_px: u32,
) -> Vec<StyleRun> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        // `i` is always a `char` boundary (it advances by whole tokens / chars),
        // so a char always remains; the `else` is unreachable but panic-free.
        let Some(c) = rest.chars().next() else { break };
        if rest.starts_with("//") {
            let end = line_comment_end(text, i);
            push_token(&mut runs, i, end, palette.comment, font_size_px);
            i = end;
        } else if c == '"' {
            let end = string_end(text, i);
            push_token(&mut runs, i, end, palette.string, font_size_px);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(text, i);
            push_token(&mut runs, i, end, palette.number, font_size_px);
            i = end;
        } else if c == '_' || c.is_alphabetic() {
            let end = word_end(text, i);
            let word = &text[i..end];
            if keywords.contains(&word) {
                push_token(&mut runs, i, end, palette.keyword, font_size_px);
            }
            i = end;
        } else {
            i += c.len_utf8();
        }
    }
    runs
}

/// Append a coloured run over `[start, end)` (no-op for an empty range). The
/// run's style is the field's base size with the token colour overlaid.
fn push_token(runs: &mut Vec<StyleRun>, start: usize, end: usize, color: Color, font_size_px: u32) {
    if end <= start {
        return;
    }
    if let (Ok(s), Ok(e)) = (u32::try_from(start), u32::try_from(end)) {
        runs.push(StyleRun::new(
            s,
            e,
            TextStyle::new().with_size_px(font_size_px).with_fg(color),
        ));
    }
}

/// Byte index one past a `//` line comment opened at `start` (the `\n`, or the
/// buffer end). The comment does not include the newline.
fn line_comment_end(text: &str, start: usize) -> usize {
    text[start..].find('\n').map_or(text.len(), |n| start + n)
}

/// Byte index one past the closing `"` of a string opened at `start` (the
/// opening quote). A backslash escapes the next char; an unterminated string
/// runs to the buffer end. `char`-aware so the returned index lands on a
/// boundary even with multi-byte content.
fn string_end(text: &str, start: usize) -> usize {
    let mut chars = text[start + 1..].char_indices();
    while let Some((off, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next(); // skip the escaped char
            }
            '"' => return start + 1 + off + 1, // past the closing quote
            _ => {}
        }
    }
    text.len()
}

/// Byte index one past a numeric literal at `start`: a digit run with an
/// optional single `.`-fraction (`12`, `3.14`). All bytes are ASCII so the
/// index is a boundary.
fn number_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

/// Byte index one past an identifier/keyword word at `start` (`[A-Za-z0-9_]`,
/// Unicode-alphanumeric included). `char`-aware.
fn word_end(text: &str, start: usize) -> usize {
    let mut end = start;
    for (off, c) in text[start..].char_indices() {
        if c.is_alphanumeric() || c == '_' {
            end = start + off + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::{SyntaxPalette, highlight_code};

    const KW: &[&str] = &["fn", "let", "if", "return", "true"];
    const SIZE: u32 = 16;

    /// `(start, end)` spans of the runs, for terse assertions.
    fn spans(text: &str) -> Vec<(u32, u32)> {
        highlight_code(text, KW, SyntaxPalette::classic(), SIZE)
            .iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn r904_keywords_only_match_whole_words() {
        // "fn" is a keyword; "fnord" is not (whole-word via the word scan).
        assert_eq!(spans("fn fnord"), vec![(0, 2)]);
    }

    #[test]
    fn r904_runs_carry_the_font_size_and_colour() {
        let runs = highlight_code("let", KW, SyntaxPalette::classic(), SIZE);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style.font_size_px, SIZE, "run keeps the field size");
        assert_eq!(
            runs[0].style.fg_color,
            SyntaxPalette::classic().keyword,
            "keyword takes the palette keyword colour",
        );
    }

    #[test]
    fn r904_line_comment_runs_to_eol_not_past() {
        // "let // c\nlet" — comment is bytes 4..8 (excludes the newline);
        // both "let" keywords colour.
        assert_eq!(spans("let // c\nlet"), vec![(0, 3), (4, 8), (9, 12)]);
    }

    #[test]
    fn r904_string_with_escape_spans_to_closing_quote() {
        // "a = \"x\\\"y\"" — the escaped quote does not close the string.
        let text = "a = \"x\\\"y\"";
        assert_eq!(spans(text), vec![(4, 10)], "string spans the escaped quote");
    }

    #[test]
    fn r904_unterminated_string_runs_to_end() {
        assert_eq!(spans("\"oops"), vec![(0, 5)]);
    }

    #[test]
    fn r904_numbers_integer_and_fraction() {
        // "12" + "3.14" + the "4" of "4." (the trailing "." is punctuation, no
        // fractional digit follows).
        assert_eq!(spans("12 3.14 4."), vec![(0, 2), (3, 7), (8, 9)]);
        let r = highlight_code("4.", KW, SyntaxPalette::classic(), SIZE);
        assert_eq!(
            (r[0].start, r[0].end),
            (0, 1),
            "no fraction digit → just '4'"
        );
    }

    #[test]
    fn r904_multibyte_before_token_keeps_offsets_exact() {
        // "é let" — 'é' is 2 bytes, so "let" colours at byte 3..6.
        assert_eq!(spans("\u{00e9} let"), vec![(3, 6)]);
    }

    #[test]
    fn r904_plain_identifiers_and_punctuation_get_no_run() {
        assert_eq!(spans("foo + bar;"), Vec::new());
    }

    #[test]
    fn r904_mixed_line_orders_runs_by_position() {
        // `let x = 42 // n` -> keyword, number, comment in order.
        assert_eq!(spans("let x = 42 // n"), vec![(0, 3), (8, 10), (11, 15)]);
    }

    #[test]
    fn r906_dark_scheme_differs_from_light_and_for_dark_selects() {
        // The dark variant must actually differ (the R904 light-on-dark fix).
        assert_ne!(
            SyntaxPalette::dark().keyword,
            SyntaxPalette::classic().keyword,
            "dark keyword colour lifts off the light scheme",
        );
        assert_eq!(SyntaxPalette::for_dark(true), SyntaxPalette::dark());
        assert_eq!(SyntaxPalette::for_dark(false), SyntaxPalette::classic());
    }
}
