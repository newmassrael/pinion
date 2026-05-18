//! R51.11 §5.37.4 — BIDI directional resolution (UAX #9) scaffold.
//!
//! This first slice lands the `BidiClass` enum (UAX #9 Table 4, 23
//! values) and the codepoint → class lookup via a build.rs codegen'd
//! range table (`BIDI_CLASS_RANGES`, parsed from UCD 16.0
//! `DerivedBidiClass.txt`). The 6-stage resolution algorithm
//! (P / X / W / N / I / L rules) is a follow-up slice — this layer
//! is the substrate every rule reads.
//!
//! External dependencies: zero. `std` + `core::cmp` only — mirrors
//! the §5.37.3 NFC engine policy ([[uax-semantic-spec-lock]]).

/// UAX #9 Table 4 — the 23 `Bidi_Class` values. Discriminant order
/// matches the `u8` indices emitted by `build.rs`
/// (`BIDI_L = 0`, …, `BIDI_PDI = 22`) so `BidiClass::from_index`
/// is a direct enum cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BidiClass {
    /// L — Left-to-Right (default for most Latin / CJK characters).
    L = 0,
    /// R — Right-to-Left (Hebrew, Thaana).
    R = 1,
    /// AL — Arabic Letter.
    AL = 2,
    /// EN — European Number (ASCII digits).
    EN = 3,
    /// ES — European Separator (+, -).
    ES = 4,
    /// ET — European Terminator (%, $, ¢).
    ET = 5,
    /// AN — Arabic Number.
    AN = 6,
    /// CS — Common Separator (., :, comma, NBSP).
    CS = 7,
    /// NSM — Nonspacing Mark (combining accents).
    NSM = 8,
    /// BN — Boundary Neutral (zero-width formatting controls).
    BN = 9,
    /// B — Paragraph Separator.
    B = 10,
    /// S — Segment Separator (tab).
    S = 11,
    /// WS — Whitespace.
    WS = 12,
    /// ON — Other Neutral (most punctuation, symbols).
    ON = 13,
    /// LRE — Left-to-Right Embedding.
    LRE = 14,
    /// LRO — Left-to-Right Override.
    LRO = 15,
    /// RLE — Right-to-Left Embedding.
    RLE = 16,
    /// RLO — Right-to-Left Override.
    RLO = 17,
    /// PDF — Pop Directional Format.
    PDF = 18,
    /// LRI — Left-to-Right Isolate.
    LRI = 19,
    /// RLI — Right-to-Left Isolate.
    RLI = 20,
    /// FSI — First Strong Isolate.
    FSI = 21,
    /// PDI — Pop Directional Isolate.
    PDI = 22,
}

impl BidiClass {
    /// Decode the `u8` index emitted by `build.rs` back into the
    /// enum. Panics on unknown values — the table is closed-set per
    /// UAX #9 and `parse_bidi_class` already rejects unknown class
    /// names at codegen time, so an unknown index here indicates a
    /// generator / runtime version skew.
    #[must_use]
    pub const fn from_index(idx: u8) -> Self {
        match idx {
            0 => BidiClass::L,
            1 => BidiClass::R,
            2 => BidiClass::AL,
            3 => BidiClass::EN,
            4 => BidiClass::ES,
            5 => BidiClass::ET,
            6 => BidiClass::AN,
            7 => BidiClass::CS,
            8 => BidiClass::NSM,
            9 => BidiClass::BN,
            10 => BidiClass::B,
            11 => BidiClass::S,
            12 => BidiClass::WS,
            13 => BidiClass::ON,
            14 => BidiClass::LRE,
            15 => BidiClass::LRO,
            16 => BidiClass::RLE,
            17 => BidiClass::RLO,
            18 => BidiClass::PDF,
            19 => BidiClass::LRI,
            20 => BidiClass::RLI,
            21 => BidiClass::FSI,
            22 => BidiClass::PDI,
            _ => panic!("BidiClass::from_index: out-of-range bidi class index"),
        }
    }

    /// UCD source name (the literal in `DerivedBidiClass.txt`).
    #[must_use]
    pub const fn ucd_name(self) -> &'static str {
        match self {
            BidiClass::L => "L",
            BidiClass::R => "R",
            BidiClass::AL => "AL",
            BidiClass::EN => "EN",
            BidiClass::ES => "ES",
            BidiClass::ET => "ET",
            BidiClass::AN => "AN",
            BidiClass::CS => "CS",
            BidiClass::NSM => "NSM",
            BidiClass::BN => "BN",
            BidiClass::B => "B",
            BidiClass::S => "S",
            BidiClass::WS => "WS",
            BidiClass::ON => "ON",
            BidiClass::LRE => "LRE",
            BidiClass::LRO => "LRO",
            BidiClass::RLE => "RLE",
            BidiClass::RLO => "RLO",
            BidiClass::PDF => "PDF",
            BidiClass::LRI => "LRI",
            BidiClass::RLI => "RLI",
            BidiClass::FSI => "FSI",
            BidiClass::PDI => "PDI",
        }
    }
}

#[allow(
    dead_code,
    clippy::unreadable_literal,
    clippy::doc_markdown,
    reason = "Generated table; consumed by bidi_class()."
)]
mod tables {
    include!(concat!(env!("OUT_DIR"), "/bidi_tables.rs"));
    pub use BIDI_CLASS_RANGES as RANGES;
}

/// Look up the UAX #9 `Bidi_Class` for `cp`. Uses binary search over
/// the codegen'd `(start, end, class_idx)` range table.
///
/// Codepoints outside every published range return
/// [`BidiClass::L`] — the UAX #9 default for unassigned codepoints
/// in the BMP / SMP planes. (The UCD `@missing` directives at the
/// top of `DerivedBidiClass.txt` assign L, R, AL, or ET to
/// reserved-but-unassigned ranges; the parsed table already
/// folds those `@missing` ranges in as explicit entries so the
/// fallback is only hit by genuine gaps.)
#[must_use]
pub fn bidi_class(cp: char) -> BidiClass {
    let cp = cp as u32;
    let ranges = tables::RANGES;
    // Binary search for the largest range whose `start <= cp`,
    // then check `cp <= end`. Mirrors the §5.37.3 NFC CCC lookup
    // shape ([[uax-semantic-spec-lock]]).
    let mut lo = 0usize;
    let mut hi = ranges.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (start, end, _) = ranges[mid];
        if cp < start {
            hi = mid;
        } else if cp > end {
            lo = mid + 1;
        } else {
            return BidiClass::from_index(ranges[mid].2);
        }
    }
    BidiClass::L
}

// ======================================================================
// R51.17 §5.37.4 — P-rules (UAX #9 §3.3.1 paragraph level resolution)
// ======================================================================
//
// The P-rules sit at the top of the UAX #9 algorithm: every later
// stage (X / W / N / I / L) needs a paragraph embedding level
// (paragraph_level) and a paragraph boundary set (iter_paragraphs).
// This slice lands P1 + P2 + P3 as pure functions of `bidi_class`,
// independent of the higher stages.

/// UAX #9 §3.3.1 P2 + P3 — resolve the paragraph embedding level
/// for a single paragraph. Returns `0` for LTR (the default when no
/// strong character is found), `1` for RTL.
///
/// Scans `paragraph` once looking for the first character of class
/// L, R, or AL while skipping over any character inside an isolate
/// (LRI / RLI / FSI ... matching PDI). Per UAX #9 P2, an isolate
/// initiator's matching PDI may be absent; in that case the inner
/// span stays skipped through the end of the paragraph. A stray
/// PDI without a matching initiator is treated as a depth-0 PDI
/// (no-op for level resolution).
///
/// This function does not split paragraphs — pass a single
/// paragraph (typically a slice from [`iter_paragraphs`]). For a
/// caller that needs every paragraph's level in one pass, iterate
/// [`iter_paragraphs`] and call this on each item.
#[must_use]
pub fn paragraph_level(paragraph: &str) -> u8 {
    let mut isolate_depth: u32 = 0;
    for ch in paragraph.chars() {
        match bidi_class(ch) {
            BidiClass::LRI | BidiClass::RLI | BidiClass::FSI => {
                isolate_depth = isolate_depth.saturating_add(1);
            }
            BidiClass::PDI => {
                isolate_depth = isolate_depth.saturating_sub(1);
            }
            BidiClass::L if isolate_depth == 0 => return 0,
            BidiClass::R | BidiClass::AL if isolate_depth == 0 => return 1,
            _ => {}
        }
    }
    // P3 default: no strong character at depth 0 → LTR.
    0
}

/// UAX #9 §3.3.1 P1 — iterate the paragraphs of `text`. A paragraph
/// extends from the previous boundary up to and **including** its
/// terminating [`BidiClass::B`] character (per UAX #9 the paragraph
/// separator belongs to the paragraph that ends with it); the final
/// paragraph in the input may have no trailing B.
///
/// `Bidi_Class = B` covers LF (U+000A), VT (U+000B), FF (U+000C),
/// CR (U+000D), NEL (U+0085), LSEP (U+2028), PSEP (U+2029) and a
/// few other code points per the UCD. The iterator is lazy — it
/// scans only as far as needed for each `next()` call and does not
/// allocate.
#[must_use]
pub fn iter_paragraphs(text: &str) -> ParagraphIter<'_> {
    ParagraphIter { text, pos: 0 }
}

/// Lazy paragraph iterator returned by [`iter_paragraphs`]. See the
/// function docs for the boundary semantics.
#[derive(Debug, Clone)]
pub struct ParagraphIter<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Iterator for ParagraphIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.pos >= self.text.len() {
            return None;
        }
        let rest = &self.text[self.pos..];
        let mut end_rel = rest.len();
        for (idx, ch) in rest.char_indices() {
            if bidi_class(ch) == BidiClass::B {
                end_rel = idx + ch.len_utf8();
                break;
            }
        }
        let start = self.pos;
        self.pos += end_rel;
        Some(&self.text[start..self.pos])
    }
}

// ======================================================================
// R51.18 §5.37.4 — X-rules (UAX #9 §3.3.2 explicit embedding/override)
// ======================================================================
//
// Pipeline order: P-rules (paragraph_level / iter_paragraphs) → X-rules
// (resolve_explicit_levels) → W / N / I / L rules. Each slice consumes
// the previous slice's output and emits the substrate the next slice
// reads. The X-rules turn a paragraph + paragraph_level into a per-
// codepoint `(level, class)` pair after honoring the UAX #9
// directional status stack semantics.

/// UAX #9 §3.3.2 maximum embedding depth. Pushes beyond this depth are
/// routed through the overflow counters instead so the algorithm
/// tolerates pathological nesting without unbounded recursion.
pub const MAX_DEPTH: u8 = 125;

/// Per-entry override mode in the UAX #9 directional status stack.
/// Drives the X6 class override — when an override is in effect every
/// character processed at that stack level is re-classified to L
/// (LTR override) or R (RTL override).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectionalOverride {
    Neutral,
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, Copy)]
struct DirectionalStatusEntry {
    embedding_level: u8,
    directional_override: DirectionalOverride,
    directional_isolate: bool,
}

/// UAX #9 §3.3.2 directional status stack — the runtime state every
/// X-rule mutates.
///
/// The stack is bounded by [`MAX_DEPTH`]. Entries that would exceed
/// that bound are routed through `overflow_isolate` /
/// `overflow_embedding` counters so the algorithm tolerates
/// pathological inputs without unbounded growth. `valid_isolate`
/// tracks the number of isolate entries currently on the stack so
/// X6a can decide between popping a matching isolate, decrementing
/// the overflow counter, or treating a stray PDI as a no-op.
struct DirectionalStatusStack {
    entries: Vec<DirectionalStatusEntry>,
    overflow_isolate: u32,
    overflow_embedding: u32,
    valid_isolate: u32,
}

impl DirectionalStatusStack {
    fn new(paragraph_level: u8) -> Self {
        Self {
            entries: vec![DirectionalStatusEntry {
                embedding_level: paragraph_level,
                directional_override: DirectionalOverride::Neutral,
                directional_isolate: false,
            }],
            overflow_isolate: 0,
            overflow_embedding: 0,
            valid_isolate: 0,
        }
    }

    fn top(&self) -> DirectionalStatusEntry {
        *self
            .entries
            .last()
            .expect("X1 base entry guarantees the stack is never empty")
    }

    fn push(&mut self, entry: DirectionalStatusEntry) {
        self.entries.push(entry);
    }

    /// Pop unless we are already at the X1 base entry — the base
    /// entry must remain to satisfy [`Self::top`]'s invariant.
    fn pop_one(&mut self) {
        if self.entries.len() > 1 {
            self.entries.pop();
        }
    }
}

/// "Least odd embedding level greater than `level`" per UAX #9 X2/X4
/// /X5a. Saturates at `u8::MAX | 1` if `level` is near the top of
/// `u8`; callers gate the result against [`MAX_DEPTH`] before
/// pushing.
const fn next_odd(level: u8) -> u8 {
    level.saturating_add(1) | 1
}

/// "Least even embedding level greater than `level`" per UAX #9 X3/X5
/// /X5b. Same saturation contract as [`next_odd`].
const fn next_even(level: u8) -> u8 {
    level.saturating_add(2) & !1u8
}

/// Per-codepoint output of [`resolve_explicit_levels`].
///
/// `levels[i]` is the resolved embedding level for the `i`-th
/// codepoint of the input paragraph (always between `0` and
/// [`MAX_DEPTH`] inclusive). `classes[i]` is the `Bidi_Class` after
/// X6 override application; the X9 "removed" codes
/// (RLE/LRE/RLO/LRO/PDF/BN) are reported as [`BidiClass::BN`] —
/// preserving their level for layout while neutralizing them for
/// the downstream W/N rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitLevels {
    /// Embedding level per codepoint.
    pub levels: Vec<u8>,
    /// Possibly-overridden `Bidi_Class` per codepoint.
    pub classes: Vec<BidiClass>,
}

/// UAX #9 §3.3.2 X-rules — turn a paragraph + `paragraph_level` into
/// a per-codepoint `(level, class)` pair after applying explicit
/// embeddings (X2-X5), overrides (X4/X5/X6), isolates (X5a/X5b/X5c
/// /X6a), and the X7/X8/X9 cleanup. The output is the substrate the
/// W-rules consume.
///
/// `paragraph_level` is typically obtained from [`paragraph_level`]
/// applied to the same input; per UAX #9 P2/P3 it is either 0 (LTR)
/// or 1 (RTL).
///
/// This function expects a single paragraph (typically a slice from
/// [`iter_paragraphs`]). A `BidiClass::B` codepoint inside the input
/// is assigned `paragraph_level` per X8 but the stack is not reset —
/// the caller is responsible for splitting at paragraph boundaries.
#[must_use]
pub fn resolve_explicit_levels(paragraph: &str, paragraph_level: u8) -> ExplicitLevels {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut stack = DirectionalStatusStack::new(paragraph_level);
    let len = chars.len();
    let mut levels = Vec::with_capacity(len);
    let mut classes = Vec::with_capacity(len);

    for (idx, &ch) in chars.iter().enumerate() {
        let cls = bidi_class(ch);
        let (level, out_cls) = match cls {
            // X2 (RLE) / X3 (LRE) / X4 (RLO) / X5 (LRO) — embedding push.
            BidiClass::RLE => push_embedding(&mut stack, true, DirectionalOverride::Neutral),
            BidiClass::LRE => push_embedding(&mut stack, false, DirectionalOverride::Neutral),
            BidiClass::RLO => push_embedding(&mut stack, true, DirectionalOverride::RightToLeft),
            BidiClass::LRO => push_embedding(&mut stack, false, DirectionalOverride::LeftToRight),
            // X5a (RLI) / X5b (LRI) — isolate push.
            BidiClass::RLI => push_isolate(&mut stack, true, cls),
            BidiClass::LRI => push_isolate(&mut stack, false, cls),
            // X5c (FSI) — lookahead for first strong then push as RLI/LRI.
            BidiClass::FSI => {
                let is_rli = fsi_resolves_to_rli(&chars, idx);
                push_isolate(&mut stack, is_rli, cls)
            }
            // X6a (PDI) / X7 (PDF).
            BidiClass::PDI => pop_isolate(&mut stack),
            BidiClass::PDF => pop_embedding(&mut stack),
            // X8 — paragraph separator assigned paragraph_level.
            BidiClass::B => (paragraph_level, BidiClass::B),
            // X9 — boundary neutral keeps current top's level.
            BidiClass::BN => (stack.top().embedding_level, BidiClass::BN),
            // X6 — every remaining class (L / R / AL / EN / ES / ET /
            // AN / CS / NSM / ON / S / WS).
            _ => apply_x6(&stack, cls),
        };
        levels.push(level);
        classes.push(out_cls);
    }

    ExplicitLevels { levels, classes }
}

/// X2-X5 — push an embedding entry for RLE/LRE/RLO/LRO. The
/// formatting code itself becomes BN at the current top's level per
/// X9 ("Remove all RLE, LRE, RLO, LRO, PDF, and BN codes").
fn push_embedding(
    stack: &mut DirectionalStatusStack,
    is_rtl: bool,
    override_mode: DirectionalOverride,
) -> (u8, BidiClass) {
    let top = stack.top();
    let new_level = if is_rtl {
        next_odd(top.embedding_level)
    } else {
        next_even(top.embedding_level)
    };
    if new_level <= MAX_DEPTH && stack.overflow_isolate == 0 && stack.overflow_embedding == 0 {
        stack.push(DirectionalStatusEntry {
            embedding_level: new_level,
            directional_override: override_mode,
            directional_isolate: false,
        });
    } else if stack.overflow_isolate == 0 {
        stack.overflow_embedding = stack.overflow_embedding.saturating_add(1);
    }
    (top.embedding_level, BidiClass::BN)
}

/// X5a (RLI) / X5b (LRI) / X5c (FSI after resolution) — assign the
/// initiator's level + override to the surrounding context's status
/// entry, then push a fresh isolate entry. `initiator_cls` is the
/// character's original class (RLI/LRI/FSI), preserved in the
/// `classes` output for downstream introspection.
fn push_isolate(
    stack: &mut DirectionalStatusStack,
    is_rtl: bool,
    initiator_cls: BidiClass,
) -> (u8, BidiClass) {
    let top = stack.top();
    let assigned = apply_override(top.directional_override, initiator_cls);
    let outer_level = top.embedding_level;

    let new_level = if is_rtl {
        next_odd(outer_level)
    } else {
        next_even(outer_level)
    };
    if new_level <= MAX_DEPTH && stack.overflow_isolate == 0 && stack.overflow_embedding == 0 {
        stack.push(DirectionalStatusEntry {
            embedding_level: new_level,
            directional_override: DirectionalOverride::Neutral,
            directional_isolate: true,
        });
        stack.valid_isolate = stack.valid_isolate.saturating_add(1);
    } else {
        stack.overflow_isolate = stack.overflow_isolate.saturating_add(1);
    }
    (outer_level, assigned)
}

/// X6a — PDI pops the directional status stack back through (and
/// including) the matching isolate entry, then assigns the PDI's
/// level/class from the surrounding (post-pop) context. Stray PDIs
/// (no matching initiator) are no-ops per UAX #9.
fn pop_isolate(stack: &mut DirectionalStatusStack) -> (u8, BidiClass) {
    if stack.overflow_isolate > 0 {
        stack.overflow_isolate -= 1;
    } else if stack.valid_isolate > 0 {
        while stack.entries.len() > 1 && !stack.top().directional_isolate {
            stack.pop_one();
        }
        if stack.entries.len() > 1 {
            stack.pop_one();
        }
        stack.valid_isolate -= 1;
        // Per UAX #9: "overflow_embedding count is reset to zero"
        // because any pending embedding overflow lived inside the
        // matching isolate's scope, which has just been closed.
        stack.overflow_embedding = 0;
    }
    let top = stack.top();
    let assigned = apply_override(top.directional_override, BidiClass::PDI);
    (top.embedding_level, assigned)
}

/// X7 — PDF pops the top non-isolate embedding entry. PDF inside an
/// embedding overflow only decrements the counter; PDF inside an
/// isolate overflow is a no-op (the isolate's PDI will reset the
/// embedding overflow). PDF that would pop an isolate entry is a
/// no-op — only PDI can close an isolate.
///
/// The PDF's own level is the *pre-pop* top — i.e. the level of the
/// embedding being closed. (UAX #9 X7 does not specify the PDF's
/// level explicitly; this convention keeps the PDF aligned with its
/// embedded run, matching `icu4c` / `unicode-bidi`. PDI takes the
/// post-pop level instead, per the explicit X6a rule.)
fn pop_embedding(stack: &mut DirectionalStatusStack) -> (u8, BidiClass) {
    let pdf_level = stack.top().embedding_level;
    if stack.overflow_isolate > 0 {
        // ignore — only PDI clears overflow_isolate
    } else if stack.overflow_embedding > 0 {
        stack.overflow_embedding -= 1;
    } else if stack.entries.len() >= 2 && !stack.top().directional_isolate {
        stack.pop_one();
    }
    (pdf_level, BidiClass::BN)
}

/// X6 — apply the current stack top's level and override status to a
/// non-formatting character.
fn apply_x6(stack: &DirectionalStatusStack, cls: BidiClass) -> (u8, BidiClass) {
    let top = stack.top();
    (top.embedding_level, apply_override(top.directional_override, cls))
}

/// Override projection: if the current stack entry has an LTR or RTL
/// override active, every character on that level is re-classified
/// as the corresponding strong class. Neutral leaves the class
/// untouched.
const fn apply_override(mode: DirectionalOverride, cls: BidiClass) -> BidiClass {
    match mode {
        DirectionalOverride::Neutral => cls,
        DirectionalOverride::LeftToRight => BidiClass::L,
        DirectionalOverride::RightToLeft => BidiClass::R,
    }
}

/// X5c first-strong lookahead — scan forward from `fsi_idx + 1` at
/// depth 0 (skipping over nested LRI/RLI/FSI ... PDI pairs) looking
/// for the first L/R/AL character. Returns `true` when the first
/// strong is R or AL (so the FSI is treated as RLI), `false` for L
/// or for the no-strong-found case (so the FSI is treated as LRI).
fn fsi_resolves_to_rli(chars: &[char], fsi_idx: usize) -> bool {
    let start = fsi_idx.saturating_add(1);
    if start >= chars.len() {
        return false;
    }
    let mut depth: u32 = 0;
    for &ch in &chars[start..] {
        match bidi_class(ch) {
            BidiClass::LRI | BidiClass::RLI | BidiClass::FSI => {
                depth = depth.saturating_add(1);
            }
            BidiClass::PDI => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            BidiClass::L if depth == 0 => return false,
            BidiClass::R | BidiClass::AL if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
#[allow(
    clippy::similar_names,
    reason = "UAX #9 format-control codepoints share canonical 3-letter abbreviations \
              (LRE/LRI/LRO, PDF/PDI/PDF, RLE/RLI/RLO/RLM); the names are domain-mandated \
              and renaming them away from the spec hurts readability."
)]
mod tests {
    use super::*;

    #[test]
    fn ascii_letters_are_l() {
        assert_eq!(bidi_class('A'), BidiClass::L);
        assert_eq!(bidi_class('z'), BidiClass::L);
        assert_eq!(bidi_class('한'), BidiClass::L); // Hangul syllable
    }

    #[test]
    fn ascii_digits_are_en() {
        assert_eq!(bidi_class('0'), BidiClass::EN);
        assert_eq!(bidi_class('9'), BidiClass::EN);
    }

    #[test]
    fn hebrew_letters_are_r() {
        assert_eq!(bidi_class('א'), BidiClass::R); // U+05D0 HEBREW LETTER ALEF
        assert_eq!(bidi_class('ת'), BidiClass::R); // U+05EA HEBREW LETTER TAV
    }

    #[test]
    fn arabic_letters_are_al() {
        assert_eq!(bidi_class('ا'), BidiClass::AL); // U+0627 ARABIC LETTER ALEF
        assert_eq!(bidi_class('ي'), BidiClass::AL); // U+064A ARABIC LETTER YEH
    }

    #[test]
    fn arabic_indic_digits_are_an() {
        assert_eq!(bidi_class('\u{0660}'), BidiClass::AN); // ARABIC-INDIC DIGIT ZERO
        assert_eq!(bidi_class('\u{0669}'), BidiClass::AN); // ARABIC-INDIC DIGIT NINE
    }

    #[test]
    fn space_is_ws() {
        assert_eq!(bidi_class(' '), BidiClass::WS);
    }

    #[test]
    fn newline_is_b_or_s() {
        // LF (U+000A) is B per UAX #9 (paragraph separator).
        assert_eq!(bidi_class('\n'), BidiClass::B);
        // HT (U+0009) is S (segment separator).
        assert_eq!(bidi_class('\t'), BidiClass::S);
    }

    #[test]
    fn directional_isolate_markers() {
        assert_eq!(bidi_class('\u{2066}'), BidiClass::LRI);
        assert_eq!(bidi_class('\u{2067}'), BidiClass::RLI);
        assert_eq!(bidi_class('\u{2068}'), BidiClass::FSI);
        assert_eq!(bidi_class('\u{2069}'), BidiClass::PDI);
    }

    #[test]
    fn unassigned_codepoint_in_pua_is_l() {
        // Private Use Area defaults — the table folds the UCD
        // @missing directives in, so this exercises real entries
        // rather than the fallback.
        assert_eq!(bidi_class('\u{E000}'), BidiClass::L);
    }

    #[test]
    fn ucd_name_round_trips_index() {
        for idx in 0..=22u8 {
            let cls = BidiClass::from_index(idx);
            // Sanity: the discriminant cast equals the input index.
            assert_eq!(cls as u8, idx);
            // ucd_name must be non-empty for every variant.
            assert!(!cls.ucd_name().is_empty());
        }
    }

    // ---- R51.17 §5.37.4 — P-rules (paragraph_level / iter_paragraphs) ----

    #[test]
    fn paragraph_level_empty_text_is_ltr() {
        // P3 default: no strong character → 0.
        assert_eq!(paragraph_level(""), 0);
    }

    #[test]
    fn paragraph_level_ascii_text_is_ltr() {
        assert_eq!(paragraph_level("Hello, world"), 0);
    }

    #[test]
    fn paragraph_level_pure_hebrew_is_rtl() {
        // U+05D0..U+05D2 — Hebrew Alef, Bet, Gimel (all class R).
        assert_eq!(paragraph_level("אבג"), 1);
    }

    #[test]
    fn paragraph_level_pure_arabic_is_rtl() {
        // Arabic letters are class AL — still RTL per P3.
        assert_eq!(paragraph_level("ابج"), 1);
    }

    #[test]
    fn paragraph_level_ltr_strong_first_is_ltr() {
        // The first strong is L ("H"), so paragraph is LTR even
        // though Hebrew follows.
        assert_eq!(paragraph_level("Hello אבג"), 0);
    }

    #[test]
    fn paragraph_level_rtl_strong_first_is_rtl() {
        // The first strong is R (Hebrew Alef), so RTL even though
        // ASCII follows.
        assert_eq!(paragraph_level("אבג Hello"), 1);
    }

    #[test]
    fn paragraph_level_weak_chars_before_strong_do_not_count() {
        // Digits (EN), commas (CS), spaces (WS) are weak/neutral —
        // they must not determine the level. First strong is "H" (L).
        assert_eq!(paragraph_level("123, Hello אבג"), 0);
    }

    #[test]
    fn paragraph_level_isolate_skips_inner_strong() {
        // LRI ... PDI wraps the Hebrew so the outer "first strong"
        // is the trailing "L" (LTR). Without the isolate handling,
        // the algorithm would mistakenly return RTL.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("{lri}אבג{pdi}L");
        assert_eq!(paragraph_level(&text), 0);
    }

    #[test]
    fn paragraph_level_unmatched_isolate_skips_remainder() {
        // LRI without PDI hides everything through end-of-paragraph.
        // No strong char at depth 0 → P3 default LTR (0).
        let lri = '\u{2066}';
        let text = format!("{lri}אבג");
        assert_eq!(paragraph_level(&text), 0);
    }

    #[test]
    fn paragraph_level_stray_pdi_is_no_op() {
        // PDI at depth 0 is invalid per UAX #9; the implementation
        // saturates the counter at 0 so the rest of the paragraph
        // still resolves normally.
        let pdi = '\u{2069}';
        let text = format!("{pdi}אבג");
        assert_eq!(paragraph_level(&text), 1);
    }

    #[test]
    fn iter_paragraphs_single_paragraph_no_b() {
        // No B class character → one paragraph spanning the entire text.
        let paras: Vec<&str> = iter_paragraphs("Hello world").collect();
        assert_eq!(paras, &["Hello world"]);
    }

    #[test]
    fn iter_paragraphs_splits_at_lf() {
        // LF is class B. Each paragraph includes its trailing LF.
        let paras: Vec<&str> = iter_paragraphs("ab\ncd\nef").collect();
        assert_eq!(paras, &["ab\n", "cd\n", "ef"]);
    }

    #[test]
    fn iter_paragraphs_trailing_b_yields_no_empty_paragraph() {
        // If the text ends with B, the iterator stops after the
        // paragraph that contains the trailing B — no spurious empty
        // final paragraph.
        let paras: Vec<&str> = iter_paragraphs("ab\n").collect();
        assert_eq!(paras, &["ab\n"]);
    }

    #[test]
    fn iter_paragraphs_empty_text_yields_no_paragraphs() {
        let paras: Vec<&str> = iter_paragraphs("").collect();
        assert!(paras.is_empty());
    }

    #[test]
    fn iter_paragraphs_psep_is_paragraph_break() {
        // U+2029 PARAGRAPH SEPARATOR is class B per UCD. (U+2028
        // LINE SEPARATOR is class WS — line break inside paragraph,
        // not a paragraph boundary.)
        let psep = '\u{2029}';
        let text = format!("ab{psep}cd");
        let paras: Vec<&str> = iter_paragraphs(&text).collect();
        assert_eq!(paras.len(), 2);
        assert!(paras[0].ends_with(psep));
        assert_eq!(paras[1], "cd");
    }

    #[test]
    fn iter_paragraphs_then_paragraph_level_classifies_each() {
        // P1 composes with P2/P3: each split paragraph resolves
        // independently. Demonstrates the canonical pipeline.
        let text = "Hello\nאבג\n123";
        let levels: Vec<u8> = iter_paragraphs(text)
            .map(paragraph_level)
            .collect();
        // LTR / RTL / LTR (no strong → default 0).
        assert_eq!(levels, vec![0, 1, 0]);
    }

    // ---- R51.18 §5.37.4 — X-rules (resolve_explicit_levels) ----

    // UCD sanity: the format-control codepoints used below must map
    // to the expected `Bidi_Class` in our codegen'd table. A failure
    // here means the rest of the X-rule suite is testing the wrong
    // characters, not the algorithm.
    #[test]
    fn ucd_format_control_classes() {
        assert_eq!(bidi_class('\u{202A}'), BidiClass::LRE);
        assert_eq!(bidi_class('\u{202B}'), BidiClass::RLE);
        assert_eq!(bidi_class('\u{202C}'), BidiClass::PDF);
        assert_eq!(bidi_class('\u{202D}'), BidiClass::LRO);
        assert_eq!(bidi_class('\u{202E}'), BidiClass::RLO);
        assert_eq!(bidi_class('\u{2066}'), BidiClass::LRI);
        assert_eq!(bidi_class('\u{2067}'), BidiClass::RLI);
        assert_eq!(bidi_class('\u{2068}'), BidiClass::FSI);
        assert_eq!(bidi_class('\u{2069}'), BidiClass::PDI);
        assert_eq!(bidi_class('\u{200C}'), BidiClass::BN);
    }

    #[test]
    fn x_rules_empty_paragraph_yields_empty_output() {
        let out = resolve_explicit_levels("", 0);
        assert!(out.levels.is_empty());
        assert!(out.classes.is_empty());
    }

    #[test]
    fn x_rules_pure_ltr_keeps_level_zero() {
        // X6: every char gets the base entry's level (0); no override.
        let out = resolve_explicit_levels("abc", 0);
        assert_eq!(out.levels, vec![0, 0, 0]);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn x_rules_pure_rtl_paragraph_keeps_level_one() {
        // X6 with paragraph_level=1: every char gets level 1.
        let out = resolve_explicit_levels("אבג", 1);
        assert_eq!(out.levels, vec![1, 1, 1]);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::R, BidiClass::R]);
    }

    #[test]
    fn x_rules_rle_pushes_level_one_at_zero() {
        // RLE at paragraph_level=0 → next odd = 1. PDF pops back.
        let rle = '\u{202B}';
        let pdf = '\u{202C}';
        let text = format!("a{rle}b{pdf}c");
        let out = resolve_explicit_levels(&text, 0);
        // 'a' (X6 lvl 0), RLE (X9 BN at lvl 0), 'b' (X6 lvl 1 inside
        // embedding), PDF (X9 BN at lvl 1 — top before pop), 'c'
        // (X6 lvl 0 after pop).
        assert_eq!(out.levels, vec![0, 0, 1, 1, 0]);
        assert_eq!(
            out.classes,
            vec![
                BidiClass::L,
                BidiClass::BN,
                BidiClass::L,
                BidiClass::BN,
                BidiClass::L,
            ],
        );
    }

    #[test]
    fn x_rules_lre_pushes_level_two_at_zero() {
        // LRE at paragraph_level=0 → next even = 2.
        let lre = '\u{202A}';
        let pdf = '\u{202C}';
        let text = format!("{lre}a{pdf}");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.levels, vec![0, 2, 2]);
    }

    #[test]
    fn x_rules_lre_at_rtl_paragraph_pushes_level_two() {
        // LRE at paragraph_level=1 → next even = 2 (not 0).
        let lre = '\u{202A}';
        let pdf = '\u{202C}';
        let text = format!("{lre}a{pdf}");
        let out = resolve_explicit_levels(&text, 1);
        // LRE BN at lvl 1, 'a' at lvl 2, PDF BN at lvl 2.
        assert_eq!(out.levels, vec![1, 2, 2]);
    }

    #[test]
    fn x_rules_rlo_overrides_l_to_r() {
        // RLO pushes next odd (1) with RTL override; characters on
        // the embedded level are re-classified as R.
        let rlo = '\u{202E}';
        let pdf = '\u{202C}';
        let text = format!("{rlo}abc{pdf}");
        let out = resolve_explicit_levels(&text, 0);
        // 'a','b','c' are L originally; override to R.
        assert_eq!(out.classes[1..=3], [BidiClass::R, BidiClass::R, BidiClass::R]);
        assert_eq!(out.levels[1..=3], [1, 1, 1]);
    }

    #[test]
    fn x_rules_lro_overrides_r_to_l() {
        // LRO at paragraph_level=0 → level 2 with LTR override.
        // Hebrew letters (R) get reclassified to L.
        let lro = '\u{202D}';
        let pdf = '\u{202C}';
        let text = format!("{lro}אבג{pdf}");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.classes[1..=3], [BidiClass::L, BidiClass::L, BidiClass::L]);
        assert_eq!(out.levels[1..=3], [2, 2, 2]);
    }

    #[test]
    fn x_rules_unmatched_pdf_is_no_op() {
        // Stray PDF without a matching push: stack stays at base
        // entry, PDF itself reports as BN at level 0.
        let pdf = '\u{202C}';
        let text = format!("a{pdf}b");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.levels, vec![0, 0, 0]);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::BN, BidiClass::L]);
    }

    #[test]
    fn x_rules_lri_isolates_content() {
        // LRI pushes an isolate entry at level 2; PDI pops it.
        // LRI/PDI themselves get the OUTER level per UAX #9 X5b/X6a.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("a{lri}b{pdi}c");
        let out = resolve_explicit_levels(&text, 0);
        // 'a' lvl 0, LRI lvl 0 (outer), 'b' lvl 2 (inside isolate),
        // PDI lvl 0 (outer after pop), 'c' lvl 0.
        assert_eq!(out.levels, vec![0, 0, 2, 0, 0]);
        // Isolate initiators keep their original class (no override
        // applied here).
        assert_eq!(out.classes[1], BidiClass::LRI);
        assert_eq!(out.classes[3], BidiClass::PDI);
    }

    #[test]
    fn x_rules_rli_isolates_at_odd_level() {
        // RLI at paragraph_level=0 → next odd = 1.
        let rli = '\u{2067}';
        let pdi = '\u{2069}';
        let text = format!("a{rli}b{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // 'a' lvl 0, RLI lvl 0 (outer), 'b' lvl 1 (inside), PDI lvl 0.
        assert_eq!(out.levels, vec![0, 0, 1, 0]);
    }

    #[test]
    fn x_rules_fsi_with_rtl_first_strong_acts_as_rli() {
        // FSI followed by Hebrew → first strong is R → RLI semantics
        // → inner level is odd (1 at paragraph_level=0).
        let fsi = '\u{2068}';
        let pdi = '\u{2069}';
        let text = format!("{fsi}אb{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // FSI lvl 0 (outer), 'א' lvl 1, 'b' lvl 1, PDI lvl 0.
        assert_eq!(out.levels, vec![0, 1, 1, 0]);
        assert_eq!(out.classes[0], BidiClass::FSI);
    }

    #[test]
    fn x_rules_fsi_with_ltr_first_strong_acts_as_lri() {
        // FSI followed by ASCII letter → first strong is L → LRI.
        let fsi = '\u{2068}';
        let pdi = '\u{2069}';
        let text = format!("{fsi}ab{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // FSI lvl 0, 'a' lvl 2, 'b' lvl 2, PDI lvl 0.
        assert_eq!(out.levels, vec![0, 2, 2, 0]);
    }

    #[test]
    fn x_rules_fsi_with_no_strong_defaults_to_lri() {
        // FSI followed only by neutral chars → default LRI (level 2).
        let fsi = '\u{2068}';
        let pdi = '\u{2069}';
        let text = format!("{fsi}123{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // Digits at level 2 (LRI), not 1 (RLI).
        assert_eq!(out.levels[1..=3], [2, 2, 2]);
    }

    #[test]
    fn x_rules_fsi_skips_nested_isolate_in_lookahead() {
        // The FSI's lookahead must skip over an inner LRI...PDI pair
        // so that the inner Hebrew does NOT determine the FSI's
        // direction. The first strong at depth 0 here is 'a' (L) →
        // LRI semantics.
        let fsi = '\u{2068}';
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("{fsi}{lri}א{pdi}a{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // FSI resolves to LRI, so the FSI entry is at level 2.
        // Inside the FSI: the LRI is itself an isolate initiator on
        // level 2 → its inner is at level 4 (next even of 2).
        // FSI(lvl0), LRI(lvl2), 'א'(lvl4), PDI(lvl2), 'a'(lvl2), PDI(lvl0).
        assert_eq!(out.levels, vec![0, 2, 4, 2, 2, 0]);
    }

    #[test]
    fn x_rules_unmatched_pdi_is_no_op() {
        // PDI with no matching isolate initiator: stack untouched,
        // PDI reports outer level.
        let pdi = '\u{2069}';
        let text = format!("a{pdi}b");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.levels, vec![0, 0, 0]);
    }

    #[test]
    fn x_rules_unmatched_lri_isolates_through_end() {
        // LRI without PDI: stack stays pushed through EOP.
        let lri = '\u{2066}';
        let text = format!("a{lri}b");
        let out = resolve_explicit_levels(&text, 0);
        // 'a' lvl 0, LRI lvl 0 (outer), 'b' lvl 2 (inside isolate).
        assert_eq!(out.levels, vec![0, 0, 2]);
    }

    #[test]
    fn x_rules_b_class_assigned_paragraph_level() {
        // LF (class B) always gets paragraph_level per X8, even when
        // it appears at the end of an embedded run.
        let rle = '\u{202B}';
        let text = format!("{rle}a\n");
        let out = resolve_explicit_levels(&text, 0);
        // RLE BN lvl 0, 'a' lvl 1, LF (B) lvl 0 (paragraph_level).
        assert_eq!(out.levels, vec![0, 1, 0]);
        assert_eq!(out.classes[2], BidiClass::B);
    }

    #[test]
    fn x_rules_bn_keeps_current_top_level() {
        // ZWNJ (U+200C, class BN) inside an embedded run takes the
        // current top's level; class is reported as BN per X9.
        let lre = '\u{202A}';
        let zwnj = '\u{200C}';
        let pdf = '\u{202C}';
        let text = format!("{lre}a{zwnj}b{pdf}");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.levels, vec![0, 2, 2, 2, 2]);
        assert_eq!(out.classes[2], BidiClass::BN);
    }

    #[test]
    fn x_rules_pdf_does_not_pop_isolate_entry() {
        // PDF must not pop an isolate entry — only PDI can. Here the
        // PDF between LRI and PDI is a no-op, so 'b' stays at the
        // isolate's level.
        let lri = '\u{2066}';
        let pdf = '\u{202C}';
        let pdi = '\u{2069}';
        let text = format!("{lri}{pdf}b{pdi}");
        let out = resolve_explicit_levels(&text, 0);
        // LRI lvl 0, PDF lvl 2 (top is isolate, no-op pop, current
        // top is the isolate entry), 'b' lvl 2, PDI lvl 0.
        assert_eq!(out.levels, vec![0, 2, 2, 0]);
    }

    #[test]
    fn x_rules_nested_embedding_then_isolate() {
        // RLE then LRI: stack grows by two; PDI pops the isolate,
        // then PDF pops the embedding.
        let rle = '\u{202B}';
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let pdf = '\u{202C}';
        let text = format!("a{rle}b{lri}c{pdi}d{pdf}e");
        let out = resolve_explicit_levels(&text, 0);
        // 'a' lvl 0, RLE BN lvl 0, 'b' lvl 1, LRI lvl 1 (outer of
        // its push), 'c' lvl 2 (next even of 1), PDI lvl 1, 'd'
        // lvl 1, PDF BN lvl 1, 'e' lvl 0.
        assert_eq!(out.levels, vec![0, 0, 1, 1, 2, 1, 1, 1, 0]);
    }

    #[test]
    fn x_rules_max_depth_triggers_overflow_embedding() {
        // 63 consecutive RLEs at paragraph_level=0 push levels
        // 1, 3, 5, ..., 125 (63 odd levels; the 63rd push lands at
        // exactly MAX_DEPTH). A 64th RLE would compute level 127
        // which exceeds MAX_DEPTH=125, so it must increment
        // overflow_embedding instead. A character after the 64th
        // RLE stays at the 63rd push's level (125), and the matching
        // PDFs pop in reverse.
        let rle = '\u{202B}';
        let mut text = rle.to_string().repeat(63);
        text.push(rle); // 64th RLE — overflow
        text.push('a');
        text.push_str(&'\u{202C}'.to_string().repeat(63)); // 63 PDFs
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.levels[63], 125); // 64th RLE itself: top is lvl 125
        assert_eq!(out.levels[64], 125); // 'a' at lvl 125 (no further push)
    }

    #[test]
    fn x_rules_pdf_clears_embedding_overflow_before_popping_stack() {
        // After embedding overflow, the first PDF must decrement the
        // counter (not pop the stack); the second PDF then pops the
        // last valid embedding push.
        let lre = '\u{202A}';
        let pdf = '\u{202C}';
        // Force one real push (level 2) then one overflow push
        // (using 62 more LREs to climb to level 124, then one more
        // overflow). Easier: directly construct the case where
        // overflow_embedding=1 is reached.
        let mut text = lre.to_string().repeat(62); // pushes 2,4,...,124
        text.push(lre); // 63rd push: level 126 > MAX_DEPTH → overflow
        text.push('a'); // at level 124
        text.push(pdf); // PDF: clears overflow (counter -=1)
        text.push('b'); // still at level 124
        text.push(pdf); // PDF: now pops; back to level 122
        text.push('c');
        let out = resolve_explicit_levels(&text, 0);
        // Index 62 = 'a' after 63 LREs; 'b' after first PDF still 124;
        // 'c' after second PDF drops to 122.
        assert_eq!(out.levels[63], 124); // 'a'
        assert_eq!(out.levels[65], 124); // 'b'
        assert_eq!(out.levels[67], 122); // 'c'
    }

    #[test]
    fn x_rules_override_does_not_leak_past_pdf() {
        // RLO active, then PDF pops, then another L char must NOT be
        // overridden to R. Catches a bug where the override mode
        // leaked from the popped entry to the surrounding context.
        let rlo = '\u{202E}';
        let pdf = '\u{202C}';
        let text = format!("{rlo}a{pdf}b");
        let out = resolve_explicit_levels(&text, 0);
        assert_eq!(out.classes[1], BidiClass::R); // 'a' overridden
        assert_eq!(out.classes[3], BidiClass::L); // 'b' NOT overridden
    }

    #[test]
    fn x_rules_isolate_isolates_outer_overrides() {
        // Outer LRO override re-classifies the LRI initiator to L
        // per X6 (LRI is a regular character at the outer level),
        // but does NOT propagate into the isolate's inner content
        // (Neutral override pushed for the isolate entry).
        let lro = '\u{202D}';
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let pdf = '\u{202C}';
        let text = format!("{lro}{lri}א{pdi}{pdf}");
        let out = resolve_explicit_levels(&text, 0);
        // LRO BN lvl 0, LRI overridden to L at lvl 2, 'א' at lvl 4
        // (NOT overridden — isolate's new entry has Neutral
        // override), PDI lvl 2, PDF lvl 2.
        assert_eq!(out.classes[1], BidiClass::L); // LRI under outer LTR override
        assert_eq!(out.classes[2], BidiClass::R); // 'א' inside isolate stays R
        assert_eq!(out.levels[2], 4);
    }
}
