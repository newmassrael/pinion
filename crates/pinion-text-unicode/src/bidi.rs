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
// R51.20 §5.37.4 — BD16 paired bracket substrate (UCD `BidiBrackets.txt`)
// ======================================================================
//
// The N0 rule (UAX #9 §3.3.4) operates on *paired brackets* — pairs
// of opening / closing brackets whose enclosed strong-type evidence
// determines the bracket pair's resolved direction. The matching
// pair table is published by Unicode as `BidiBrackets.txt` (UCD
// 16.0.0, 64 pairs / 128 entries). This slice lands the codepoint →
// matching codepoint + Open/Close kind lookup; the N0 rule itself
// follows in R51.21.

/// `Bidi_Paired_Bracket_Type` (UAX #9 BD16). `Open` corresponds to
/// the UCD `o` value (an opening bracket whose `Bidi_Paired_Bracket`
/// points to the matching close); `Close` is the inverse. Codepoints
/// outside the published bracket pair list have neither type and are
/// reported as `None` by [`paired_bracket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracketType {
    /// `BPT_o` — opening bracket. The matching codepoint reported by
    /// [`paired_bracket`] is the corresponding closing bracket.
    Open,
    /// `BPT_c` — closing bracket. The matching codepoint reported by
    /// [`paired_bracket`] is the corresponding opening bracket.
    Close,
}

/// UAX #9 BD16 lookup — given a codepoint, return its matching
/// bracket and the kind (Open or Close) when the codepoint is a
/// paired bracket. Codepoints not present in `BidiBrackets.txt`
/// return `None`.
///
/// The matching codepoint is the other half of the bracket pair (so
/// `paired_bracket('(') = Some((')', Open))` and conversely
/// `paired_bracket(')') = Some(('(', Close))`). For non-BMP brackets
/// the matching value is decoded via [`char::from_u32`]; UCD entries
/// are guaranteed valid scalar values.
///
/// # Panics
///
/// Panics if the codegen layer emits a bracket pair with an invalid
/// matching codepoint or an unknown `Bidi_Paired_Bracket_Type`
/// discriminant — both are build-time invariants asserted by
/// [`crate::bidi::tests::bd16_round_trip_invariant`] and the
/// `parse_bidi_brackets` codegen, so a panic here indicates a
/// generator / UCD version skew.
#[must_use]
pub fn paired_bracket(cp: char) -> Option<(char, BracketType)> {
    let key = cp as u32;
    let pairs = tables::BIDI_BRACKET_PAIRS;
    let idx = pairs.binary_search_by_key(&key, |&(k, _, _)| k).ok()?;
    let (_, matching, kind) = pairs[idx];
    let matching_char = char::from_u32(matching)
        .expect("BidiBrackets.txt entries are valid scalar values");
    let bt = match kind {
        0 => BracketType::Open,
        1 => BracketType::Close,
        other => unreachable!(
            "unexpected Bidi_Paired_Bracket_Type discriminant {other} \
             — build.rs invariant violated"
        ),
    };
    Some((matching_char, bt))
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

// ======================================================================
// R51.19 §5.37.4 — W-rules (UAX #9 §3.3.3 weak type resolution)
// ======================================================================
//
// Pipeline order: X-rules (resolve_explicit_levels) → W-rules
// (resolve_weak_types) → N → I → L. The W-rules operate per
// "isolating run sequence" (UAX #9 BD13) — a chain of level runs
// connected by matched isolate initiator / PDI pairs that span the
// inner (higher) embedding levels they enclose. Within each
// sequence, W1-W7 rewrite weak `BidiClass` values to strong
// equivalents (L / R / EN / AN / ON) so the downstream N rules see
// a sequence of strong + neutral types only.

/// One UAX #9 level run — a maximal sub-sequence of consecutive
/// non-removed codepoints sharing the same embedding level. Removed
/// (X9) codes — RLE / LRE / RLO / LRO / PDF / original BN — are
/// excluded from `members`; their level is preserved in
/// [`ExplicitLevels::levels`] for downstream layout but they do not
/// participate in W/N rule resolution.
#[derive(Debug, Clone)]
struct LevelRun {
    level: u8,
    members: Vec<usize>,
}

/// UAX #9 BD13 isolating run sequence — a chain of [`LevelRun`]s
/// connected by matched isolate initiator (LRI / RLI / FSI) → PDI
/// pairs. All runs in a sequence share the same embedding level
/// (the X-rules ensure isolate initiators emit their *outer* level,
/// so the matching PDI also reports the outer level — both sit on
/// the run that owns the surrounding context).
#[derive(Debug, Clone)]
struct IsolatingRunSequence {
    level: u8,
    /// Indices into the parent `Vec<LevelRun>`, in textual order.
    run_indices: Vec<usize>,
}

/// Group the non-removed codepoints of an X-rules output into level
/// runs. The result preserves textual order; each run's `members`
/// slice is monotonically increasing.
fn build_level_runs(levels: &[u8], classes: &[BidiClass]) -> Vec<LevelRun> {
    let mut runs: Vec<LevelRun> = Vec::new();
    for (i, &cls) in classes.iter().enumerate() {
        if cls == BidiClass::BN {
            continue;
        }
        let level = levels[i];
        if let Some(last) = runs.last_mut()
            && last.level == level
        {
            last.members.push(i);
            continue;
        }
        runs.push(LevelRun {
            level,
            members: vec![i],
        });
    }
    runs
}

/// Match every isolate initiator (LRI / RLI / FSI) to its
/// corresponding PDI using the same depth-tracked walk the X-rules
/// already implement. Removed (BN) codes are skipped — by this
/// point they no longer participate in the algorithm. Unmatched
/// initiators (no PDI found before EOP) and stray PDIs (no prior
/// initiator) are silently dropped from the result.
fn match_isolate_initiators(classes: &[BidiClass]) -> Vec<(usize, usize)> {
    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (i, &cls) in classes.iter().enumerate() {
        // BN (X9 "removed") chars fall into the wildcard arm — by
        // this stage they no longer participate in matching, same
        // as every other non-isolate character.
        match cls {
            BidiClass::LRI | BidiClass::RLI | BidiClass::FSI => stack.push(i),
            BidiClass::PDI => {
                if let Some(initiator) = stack.pop() {
                    matches.push((initiator, i));
                }
            }
            _ => {}
        }
    }
    matches
}

/// Walk the level runs and group them into isolating run sequences
/// (UAX #9 BD13). A pair `(initiator_pos, pdi_pos)` connects
/// `run_a → run_b` when `initiator_pos` is the *last* member of
/// `run_a` and `pdi_pos` is the *first* member of `run_b` *and* the
/// two runs are distinct (the latter excludes the "overflowed
/// isolate" case where the initiator and PDI are in the same run at
/// the outer level).
fn build_isolating_run_sequences(
    runs: &[LevelRun],
    classes: &[BidiClass],
) -> Vec<IsolatingRunSequence> {
    let n_runs = runs.len();
    if n_runs == 0 {
        return Vec::new();
    }
    let n_chars = classes.len();

    // Position → owning run index. `None` for removed codes (X9).
    let mut run_at_pos: Vec<Option<usize>> = vec![None; n_chars];
    for (ri, run) in runs.iter().enumerate() {
        for &p in &run.members {
            run_at_pos[p] = Some(ri);
        }
    }

    // next_in_seq[ri] = Some(rj) when run ri's last member is an
    // isolate initiator matched to a PDI that begins run rj (and
    // rj != ri).
    let mut next_in_seq: Vec<Option<usize>> = vec![None; n_runs];
    let mut has_prev: Vec<bool> = vec![false; n_runs];
    for (initiator_pos, pdi_pos) in match_isolate_initiators(classes) {
        let Some(ri) = run_at_pos[initiator_pos] else {
            continue;
        };
        let Some(rj) = run_at_pos[pdi_pos] else {
            continue;
        };
        if ri == rj {
            continue;
        }
        if *runs[ri].members.last().unwrap_or(&usize::MAX) != initiator_pos {
            continue;
        }
        if *runs[rj].members.first().unwrap_or(&usize::MAX) != pdi_pos {
            continue;
        }
        next_in_seq[ri] = Some(rj);
        has_prev[rj] = true;
    }

    // Walk: every run that has no predecessor starts a new sequence;
    // follow next_in_seq until a chain end.
    let mut sequences = Vec::new();
    let mut visited = vec![false; n_runs];
    for ri in 0..n_runs {
        if visited[ri] || has_prev[ri] {
            continue;
        }
        let mut run_indices = vec![ri];
        visited[ri] = true;
        let mut cur = ri;
        while let Some(next) = next_in_seq[cur] {
            if visited[next] {
                break;
            }
            run_indices.push(next);
            visited[next] = true;
            cur = next;
        }
        sequences.push(IsolatingRunSequence {
            level: runs[ri].level,
            run_indices,
        });
    }
    sequences
}

/// UAX #9 X10 sos/eos — determine the start-of-sequence and
/// end-of-sequence types for an isolating run sequence. The types
/// are L or R, computed from the higher of (`sequence_level`,
/// `neighbor_level`) parity. The neighbor is the closest non-removed
/// character outside the sequence; if no neighbor exists,
/// `paragraph_level` substitutes.
fn compute_sos_eos(
    seq: &IsolatingRunSequence,
    runs: &[LevelRun],
    levels: &[u8],
    classes: &[BidiClass],
    paragraph_level: u8,
) -> (BidiClass, BidiClass) {
    let first_run = &runs[seq.run_indices[0]];
    let last_run = &runs[*seq.run_indices.last().expect("sequence has at least one run")];
    let first_pos = first_run.members[0];
    let last_pos = *last_run
        .members
        .last()
        .expect("level run has at least one member");

    let prev_level = (0..first_pos)
        .rev()
        .find(|&i| classes[i] != BidiClass::BN)
        .map_or(paragraph_level, |i| levels[i]);

    let next_level = ((last_pos + 1)..classes.len())
        .find(|&i| classes[i] != BidiClass::BN)
        .map_or(paragraph_level, |i| levels[i]);

    let sos_max = core::cmp::max(prev_level, seq.level);
    let eos_max = core::cmp::max(seq.level, next_level);
    let sos = if sos_max % 2 == 0 {
        BidiClass::L
    } else {
        BidiClass::R
    };
    let eos = if eos_max % 2 == 0 {
        BidiClass::L
    } else {
        BidiClass::R
    };
    (sos, eos)
}

/// W1 — NSM resolution. NSM at the start of the sequence or
/// immediately after an isolate initiator / PDI takes type `ON`;
/// otherwise it inherits the type of the preceding character (which
/// may itself have been rewritten by an earlier W1 application).
fn apply_w1(view: &mut [BidiClass]) {
    if view.is_empty() {
        return;
    }
    if view[0] == BidiClass::NSM {
        view[0] = BidiClass::ON;
    }
    for i in 1..view.len() {
        if view[i] != BidiClass::NSM {
            continue;
        }
        view[i] = match view[i - 1] {
            BidiClass::LRI | BidiClass::RLI | BidiClass::FSI | BidiClass::PDI => BidiClass::ON,
            other => other,
        };
    }
}

/// W2 — EN preceded (in the sequence, skipping non-strong) by AL
/// becomes AN. `sos` is treated as the last strong before the
/// sequence start; since sos is always L or R, it never triggers
/// the AL conversion.
fn apply_w2(view: &mut [BidiClass], sos: BidiClass) {
    let mut last_strong = sos;
    for cls in view.iter_mut() {
        match *cls {
            BidiClass::L | BidiClass::R | BidiClass::AL => last_strong = *cls,
            BidiClass::EN => {
                if last_strong == BidiClass::AL {
                    *cls = BidiClass::AN;
                }
            }
            _ => {}
        }
    }
}

/// W3 — every remaining AL becomes R.
fn apply_w3(view: &mut [BidiClass]) {
    for cls in view.iter_mut() {
        if *cls == BidiClass::AL {
            *cls = BidiClass::R;
        }
    }
}

/// W4 — a *single* ES or CS between two ENs becomes EN; a *single*
/// CS between two ANs becomes AN. The neighbors are the immediate
/// previous and next character in the flattened sequence.
fn apply_w4(view: &mut [BidiClass]) {
    if view.len() < 3 {
        return;
    }
    for i in 1..view.len() - 1 {
        let prev = view[i - 1];
        let next = view[i + 1];
        match view[i] {
            BidiClass::ES | BidiClass::CS
                if prev == BidiClass::EN && next == BidiClass::EN =>
            {
                view[i] = BidiClass::EN;
            }
            BidiClass::CS if prev == BidiClass::AN && next == BidiClass::AN => {
                view[i] = BidiClass::AN;
            }
            _ => {}
        }
    }
}

/// W5 — a maximal sequence of consecutive ETs that touches (on
/// either side) at least one EN turns into all ENs.
fn apply_w5(view: &mut [BidiClass]) {
    let n = view.len();
    let mut i = 0;
    while i < n {
        if view[i] != BidiClass::ET {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && view[i] == BidiClass::ET {
            i += 1;
        }
        let end = i; // exclusive
        let before_en = start > 0 && view[start - 1] == BidiClass::EN;
        let after_en = end < n && view[end] == BidiClass::EN;
        if before_en || after_en {
            for cls in &mut view[start..end] {
                *cls = BidiClass::EN;
            }
        }
    }
}

/// W6 — any ES, ET, or CS that did not change in W4/W5 becomes ON.
fn apply_w6(view: &mut [BidiClass]) {
    for cls in view.iter_mut() {
        match *cls {
            BidiClass::ES | BidiClass::ET | BidiClass::CS => *cls = BidiClass::ON,
            _ => {}
        }
    }
}

/// W7 — EN preceded (in the sequence, scanning backward over
/// non-strong) by L becomes L. `sos` participates: at sequence
/// start, a sos of L converts the leading ENs to L (this is what
/// makes ASCII digits in an LTR paragraph render as L-tagged
/// characters for the L rules).
fn apply_w7(view: &mut [BidiClass], sos: BidiClass) {
    let mut last_strong = sos;
    for cls in view.iter_mut() {
        match *cls {
            BidiClass::L | BidiClass::R => last_strong = *cls,
            BidiClass::EN => {
                if last_strong == BidiClass::L {
                    *cls = BidiClass::L;
                }
            }
            _ => {}
        }
    }
}

/// Flatten an isolating run sequence into a sorted vector of
/// codepoint positions. Each run's `members` is already sorted; the
/// runs themselves are concatenated in textual order.
fn collect_sequence_positions(seq: &IsolatingRunSequence, runs: &[LevelRun]) -> Vec<usize> {
    let mut positions = Vec::new();
    for &ri in &seq.run_indices {
        positions.extend_from_slice(&runs[ri].members);
    }
    positions
}

// ======================================================================
// R51.21 §5.37.4 — N-rules (UAX #9 §3.3.4 neutral type resolution)
// ======================================================================
//
// Pipeline order: W-rules (resolve_weak_types) → N-rules
// (resolve_neutral_types) → I → L. The N-rules close out the
// "logical-order" half of UAX #9 by resolving everything that is
// still a Neutral (B, S, WS, ON) or Isolate initiator/terminator
// (LRI, RLI, FSI, PDI) into L or R per the surrounding strong
// context. Three sub-rules apply per isolating run sequence:
//
//   N0 — paired bracket pairs (BD16 stack matching, UCD
//        BidiBrackets.txt). Each pair takes the embedding direction
//        when enclosed strong matches; otherwise the
//        opposite-direction-with-context fallback.
//   N1 — runs of Neutrals/Isolates between matching strong
//        neighbours (EN and AN act as R for this purpose) → that
//        strong direction.
//   N2 — every remaining Neutral/Isolate → embedding direction
//        (L if `sequence_level` is even, R if odd).
//
// Known limitation: UAX #9 N0 step 5 — NSMs that *originally* had
// `Bidi_Class = NSM` and follow a bracket whose direction changed
// in N0 should adopt that direction. Implementing it requires the
// pre-W1 class array; for now this slice carries the limitation
// (rare in practice and not exercised by the typical-text BidiTest
// subset). Tracked for a follow-up alongside BidiTest.txt
// conformance harness.

const N0_BRACKET_STACK_MAX: usize = 63;

/// `true` if `cls` is a Neutral or Isolate (NI in UAX #9
/// terminology) — the classes N1/N2 mutate. After W-rules + N0,
/// these are the only classes left other than L, R, EN, AN, and the
/// X9-removed BN.
const fn is_neutral_or_isolate(cls: BidiClass) -> bool {
    matches!(
        cls,
        BidiClass::B
            | BidiClass::S
            | BidiClass::WS
            | BidiClass::ON
            | BidiClass::LRI
            | BidiClass::RLI
            | BidiClass::FSI
            | BidiClass::PDI
    )
}

/// Project a (W-rules-resolved) class to its strong direction for
/// N-rule purposes: L → L, R / EN / AN → R, everything else
/// returns `cls` unchanged (callers only invoke this on non-NI
/// chars, where the result is always L or R).
const fn n_strong_direction(cls: BidiClass) -> BidiClass {
    match cls {
        BidiClass::L => BidiClass::L,
        BidiClass::R | BidiClass::EN | BidiClass::AN => BidiClass::R,
        _ => cls,
    }
}

/// BD16 — match paired brackets within an isolating run sequence
/// view using a stack of size [`N0_BRACKET_STACK_MAX`]. Each entry
/// stores `(view_position, matching_codepoint)` so a close bracket
/// can search down the stack for an entry whose recorded match
/// equals the close bracket's own codepoint. Stack overflow aborts
/// the matching for this sequence (returns an empty pair list per
/// UAX #9).
fn find_bracket_pairs(
    view: &[BidiClass],
    positions: &[usize],
    chars: &[char],
) -> Vec<(usize, usize)> {
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, &cls) in view.iter().enumerate() {
        // After W-rules, only ON remains as a candidate neutral the
        // bracket could be classified as. (W6 leaves residual ON;
        // brackets in UCD have Bidi_Class = ON.)
        if cls != BidiClass::ON {
            continue;
        }
        let ch = chars[positions[i]];
        let Some((matching, kind)) = paired_bracket(ch) else {
            continue;
        };
        match kind {
            BracketType::Open => {
                if stack.len() >= N0_BRACKET_STACK_MAX {
                    return Vec::new();
                }
                stack.push((i, matching));
            }
            BracketType::Close => {
                if let Some(found) =
                    stack.iter().rposition(|&(_, m)| m == ch)
                {
                    let (open_pos, _) = stack[found];
                    stack.truncate(found);
                    pairs.push((open_pos, i));
                }
            }
        }
    }
    pairs.sort_by_key(|(open, _)| *open);
    pairs
}

/// N0 — for each matched bracket pair in `pairs`, set both
/// brackets in `view` to the resolved direction per UAX #9 N0.
/// `embed_dir` is `L` (sequence level even) or `R` (odd). `sos` is
/// the start-of-sequence strong direction.
fn apply_n0(
    view: &mut [BidiClass],
    pairs: &[(usize, usize)],
    embed_dir: BidiClass,
    sos: BidiClass,
) {
    let opposite_dir = match embed_dir {
        BidiClass::L => BidiClass::R,
        _ => BidiClass::L,
    };
    for &(open_pos, close_pos) in pairs {
        let mut found_embed = false;
        let mut found_opposite = false;
        for cls in &view[open_pos + 1..close_pos] {
            let strong = match *cls {
                BidiClass::L => Some(BidiClass::L),
                BidiClass::R | BidiClass::EN | BidiClass::AN => Some(BidiClass::R),
                _ => None,
            };
            if let Some(dir) = strong {
                if dir == embed_dir {
                    found_embed = true;
                    break;
                }
                found_opposite = true;
            }
        }
        let pair_dir = if found_embed {
            embed_dir
        } else if found_opposite {
            // Establish preceding context by scanning backward for
            // the closest strong (or fall through to sos).
            let mut context = sos;
            for cls in view[..open_pos].iter().rev() {
                let strong = match *cls {
                    BidiClass::L => Some(BidiClass::L),
                    BidiClass::R | BidiClass::EN | BidiClass::AN => Some(BidiClass::R),
                    _ => None,
                };
                if let Some(dir) = strong {
                    context = dir;
                    break;
                }
            }
            if context == opposite_dir {
                opposite_dir
            } else {
                embed_dir
            }
        } else {
            // No strong inside — brackets retain Other_Neutral. They
            // will be resolved by N1/N2 like any other neutral.
            continue;
        };
        view[open_pos] = pair_dir;
        view[close_pos] = pair_dir;
    }
}

/// N1 — resolve runs of Neutrals/Isolates between two strong
/// characters (or sequence boundary sos/eos) when both sides agree.
/// EN and AN are treated as R for influence purposes.
fn apply_n1(view: &mut [BidiClass], sos: BidiClass, eos: BidiClass) {
    let n = view.len();
    let mut i = 0;
    while i < n {
        if !is_neutral_or_isolate(view[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && is_neutral_or_isolate(view[i]) {
            i += 1;
        }
        let end = i; // exclusive
        let before = if start == 0 {
            sos
        } else {
            n_strong_direction(view[start - 1])
        };
        let after = if end == n {
            eos
        } else {
            n_strong_direction(view[end])
        };
        if before == after && matches!(before, BidiClass::L | BidiClass::R) {
            for cls in &mut view[start..end] {
                *cls = before;
            }
        }
    }
}

/// N2 — any remaining Neutral/Isolate takes the embedding
/// direction. Always fires last so N1 can resolve matched-strong
/// runs first.
fn apply_n2(view: &mut [BidiClass], embed_dir: BidiClass) {
    for cls in view.iter_mut() {
        if is_neutral_or_isolate(*cls) {
            *cls = embed_dir;
        }
    }
}

/// UAX #9 §3.3.3 — apply W1-W7 to each isolating run sequence built
/// over the X-rules output. The returned struct shares its `levels`
/// vector with the input verbatim (W rules do not touch levels);
/// only `classes` may have been rewritten.
///
/// `paragraph_level` is the same value passed to
/// [`resolve_explicit_levels`] — needed by [`compute_sos_eos`] to
/// determine sos/eos at paragraph boundaries.
#[must_use]
pub fn resolve_weak_types(explicit: ExplicitLevels, paragraph_level: u8) -> ExplicitLevels {
    let ExplicitLevels {
        levels,
        mut classes,
    } = explicit;
    let runs = build_level_runs(&levels, &classes);
    let sequences = build_isolating_run_sequences(&runs, &classes);

    for seq in &sequences {
        let positions = collect_sequence_positions(seq, &runs);
        let (sos, _eos) = compute_sos_eos(seq, &runs, &levels, &classes, paragraph_level);
        let mut view: Vec<BidiClass> = positions.iter().map(|&i| classes[i]).collect();
        apply_w1(&mut view);
        apply_w2(&mut view, sos);
        apply_w3(&mut view);
        apply_w4(&mut view);
        apply_w5(&mut view);
        apply_w6(&mut view);
        apply_w7(&mut view, sos);
        for (k, &p) in positions.iter().enumerate() {
            classes[p] = view[k];
        }
    }

    ExplicitLevels { levels, classes }
}

/// UAX #9 §3.3.4 — apply N0/N1/N2 to each isolating run sequence
/// built over the W-rules output. `paragraph` is the original text
/// passed to [`resolve_explicit_levels`]; it is re-collected into a
/// `Vec<char>` so N0 can resolve paired brackets via
/// [`paired_bracket`]. Returns a new `ExplicitLevels` with the
/// neutral types resolved; `levels` are unchanged.
#[must_use]
pub fn resolve_neutral_types(
    weak: ExplicitLevels,
    paragraph: &str,
    paragraph_level: u8,
) -> ExplicitLevels {
    let chars: Vec<char> = paragraph.chars().collect();
    let ExplicitLevels {
        levels,
        mut classes,
    } = weak;
    let runs = build_level_runs(&levels, &classes);
    let sequences = build_isolating_run_sequences(&runs, &classes);

    for seq in &sequences {
        let positions = collect_sequence_positions(seq, &runs);
        let (sos, eos) =
            compute_sos_eos(seq, &runs, &levels, &classes, paragraph_level);
        let mut view: Vec<BidiClass> =
            positions.iter().map(|&i| classes[i]).collect();

        let embed_dir = if seq.level % 2 == 0 {
            BidiClass::L
        } else {
            BidiClass::R
        };

        // N0: bracket pairs first (must run before N1/N2 so brackets
        // either become strong or stay ON for N1 to resolve).
        let pairs = find_bracket_pairs(&view, &positions, &chars);
        apply_n0(&mut view, &pairs, embed_dir, sos);
        // N1: matched-strong neutral runs.
        apply_n1(&mut view, sos, eos);
        // N2: residual neutrals.
        apply_n2(&mut view, embed_dir);

        for (k, &p) in positions.iter().enumerate() {
            classes[p] = view[k];
        }
    }

    ExplicitLevels { levels, classes }
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

    // ---- R51.19 §5.37.4 — W-rules (resolve_weak_types) ----

    fn resolve_pipeline(text: &str, paragraph_level: u8) -> ExplicitLevels {
        let explicit = resolve_explicit_levels(text, paragraph_level);
        resolve_weak_types(explicit, paragraph_level)
    }

    #[test]
    fn w_rules_empty_paragraph() {
        let out = resolve_pipeline("", 0);
        assert!(out.classes.is_empty());
        assert!(out.levels.is_empty());
    }

    #[test]
    fn w_rules_pure_ltr_unchanged() {
        // No weak types, no rule fires. L stays L.
        let out = resolve_pipeline("abc", 0);
        assert_eq!(out.classes, vec![BidiClass::L; 3]);
        assert_eq!(out.levels, vec![0; 3]);
    }

    #[test]
    fn w_rules_pure_rtl_unchanged() {
        let out = resolve_pipeline("אבג", 1);
        assert_eq!(out.classes, vec![BidiClass::R; 3]);
        assert_eq!(out.levels, vec![1; 3]);
    }

    // ---- W1 ----

    #[test]
    fn w1_nsm_at_sequence_start_becomes_on() {
        // NSM at the very start of the paragraph (= sequence start) → ON.
        // U+0301 COMBINING ACUTE ACCENT is class NSM.
        let nsm = '\u{0301}';
        let out = resolve_pipeline(&nsm.to_string(), 0);
        assert_eq!(out.classes, vec![BidiClass::ON]);
    }

    #[test]
    fn w1_nsm_after_l_becomes_l() {
        // ASCII 'a' + combining accent: NSM inherits L.
        let text = "a\u{0301}";
        let out = resolve_pipeline(text, 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn w1_nsm_after_r_becomes_r() {
        // Hebrew letter + combining mark: NSM inherits R.
        let text = "א\u{0301}";
        let out = resolve_pipeline(text, 1);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::R]);
    }

    #[test]
    fn w1_nsm_after_lri_becomes_on() {
        // LRI followed by NSM: W1 maps NSM to ON because the
        // preceding char is an isolate initiator.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("a{lri}\u{0301}b{pdi}");
        let out = resolve_pipeline(&text, 0);
        // Positions: 'a' (L), LRI (LRI), NSM→ON, 'b' (L), PDI (PDI).
        assert_eq!(out.classes[2], BidiClass::ON);
    }

    #[test]
    fn w1_nsm_after_pdi_becomes_on() {
        // PDI followed by NSM (in the outer sequence): NSM → ON.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("a{lri}b{pdi}\u{0301}");
        let out = resolve_pipeline(&text, 0);
        // Positions: a, LRI, b, PDI, NSM. NSM follows PDI → ON.
        assert_eq!(out.classes[4], BidiClass::ON);
    }

    #[test]
    fn w1_consecutive_nsm_propagate() {
        // Two NSMs after 'a': both become L (the second propagates
        // through the already-W1'd first).
        let text = "a\u{0301}\u{0302}";
        let out = resolve_pipeline(text, 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L, BidiClass::L]);
    }

    // ---- W2 ----

    #[test]
    fn w2_en_after_al_becomes_an() {
        // Arabic letter followed by ASCII digit: W2 turns EN → AN.
        // Then W3 turns AL → R.
        let out = resolve_pipeline("ا5", 1);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::AN]);
    }

    #[test]
    fn w2_en_after_l_stays_en_then_w7_to_l() {
        // ASCII letter + ASCII digit in LTR paragraph: W2 keeps EN
        // (preceding strong is L, not AL). W7 then converts EN → L.
        let out = resolve_pipeline("a5", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn w2_en_at_sequence_start_stays_en_in_rtl_paragraph() {
        // RTL paragraph: sos = R. No AL precedes the EN, so W2 does
        // not fire. W7 also does not fire (sos is R, not L). EN stays.
        let out = resolve_pipeline("5", 1);
        assert_eq!(out.classes, vec![BidiClass::EN]);
    }

    // ---- W3 ----

    #[test]
    fn w3_al_becomes_r() {
        // Even an isolated AL becomes R.
        let out = resolve_pipeline("ا", 1);
        assert_eq!(out.classes, vec![BidiClass::R]);
    }

    // ---- W4 ----

    #[test]
    fn w4_single_es_between_en_en_becomes_en() {
        // ASCII '+' is ES. "1+2" in RTL paragraph (so W7 doesn't
        // override the EN to L afterward — keeps the W4 effect
        // observable).
        let out = resolve_pipeline("1+2", 1);
        assert_eq!(out.classes, vec![BidiClass::EN, BidiClass::EN, BidiClass::EN]);
    }

    #[test]
    fn w4_double_es_between_en_stays_on_after_w6() {
        // "1++2" — two ESes. Neither matches W4's "single between
        // ENs" condition. They fall through to W6 → ON.
        let out = resolve_pipeline("1++2", 1);
        assert_eq!(
            out.classes,
            vec![BidiClass::EN, BidiClass::ON, BidiClass::ON, BidiClass::EN],
        );
    }

    #[test]
    fn w4_single_cs_between_an_an_becomes_an() {
        // Arabic-Indic digit + comma + Arabic-Indic digit.
        // U+0660 = ARABIC-INDIC DIGIT ZERO (class AN), ',' is CS.
        // RTL paragraph keeps the AN→AN visible (W7 doesn't touch AN).
        let text = "\u{0660},\u{0661}";
        let out = resolve_pipeline(text, 1);
        assert_eq!(out.classes, vec![BidiClass::AN, BidiClass::AN, BidiClass::AN]);
    }

    #[test]
    fn w4_cs_between_mixed_en_an_stays_separator() {
        // CS between EN and AN does NOT trigger W4 (mismatched
        // neighbor types). W6 reclassifies it to ON.
        let text = "5,\u{0660}";
        let out = resolve_pipeline(text, 1);
        assert_eq!(out.classes, vec![BidiClass::EN, BidiClass::ON, BidiClass::AN]);
    }

    // ---- W5 ----

    #[test]
    fn w5_et_before_en_becomes_en() {
        // '$' is ET, '5' is EN. ET adjacent to EN → EN. RTL
        // paragraph keeps W7 from overriding.
        let out = resolve_pipeline("$5", 1);
        assert_eq!(out.classes, vec![BidiClass::EN, BidiClass::EN]);
    }

    #[test]
    fn w5_et_after_en_becomes_en() {
        let out = resolve_pipeline("5$", 1);
        assert_eq!(out.classes, vec![BidiClass::EN, BidiClass::EN]);
    }

    #[test]
    fn w5_et_sequence_adjacent_to_en_all_become_en() {
        // Multiple ETs in a row adjacent to one EN.
        let out = resolve_pipeline("$$5", 1);
        assert_eq!(
            out.classes,
            vec![BidiClass::EN, BidiClass::EN, BidiClass::EN],
        );
    }

    #[test]
    fn w5_isolated_et_falls_to_on_via_w6() {
        // ET with no EN neighbor — W5 does not fire, W6 does.
        let out = resolve_pipeline("$", 1);
        assert_eq!(out.classes, vec![BidiClass::ON]);
    }

    // ---- W6 ----

    #[test]
    fn w6_residual_es_becomes_on() {
        // "a+b" — '+' is ES with non-EN neighbors. W4 doesn't fire,
        // W6 maps ES → ON.
        let out = resolve_pipeline("a+b", 1);
        assert_eq!(out.classes[1], BidiClass::ON);
    }

    #[test]
    fn w6_residual_cs_becomes_on() {
        // "a,b" — CS with non-EN/AN neighbors.
        let out = resolve_pipeline("a,b", 1);
        assert_eq!(out.classes[1], BidiClass::ON);
    }

    // ---- W7 ----

    #[test]
    fn w7_en_after_sos_l_becomes_l() {
        // LTR paragraph (sos = L), bare EN → L.
        let out = resolve_pipeline("5", 0);
        assert_eq!(out.classes, vec![BidiClass::L]);
    }

    #[test]
    fn w7_en_after_l_in_text_becomes_l() {
        // Mid-text L then EN: EN → L.
        let out = resolve_pipeline("a5", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn w7_en_after_r_stays_en() {
        // Hebrew letter (R) then EN: W7 doesn't fire (no L in
        // backward scan), EN stays.
        let out = resolve_pipeline("א5", 1);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::EN]);
    }

    #[test]
    fn w7_an_unaffected() {
        // W7 operates on EN, not AN. Arabic number stays AN.
        let out = resolve_pipeline("ا5", 0);
        // 'ا' is AL → R via W3. '5' after AL → AN via W2. W7 only
        // affects EN, so AN survives.
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::AN]);
    }

    // ---- Cross-sequence behaviour ----

    #[test]
    fn w2_does_not_cross_isolating_run_sequence_boundary() {
        // Outer sequence sees AL, but inside the isolate's own
        // sequence the EN must not see that AL (different sequence).
        // Outer: ...AL [LRI inner-content PDI] EN.
        // Inner: inner-content (own sequence).
        // After AL→R via W3, and W2 on outer: EN preceded by AL
        // (skipping isolate-treated-as-non-strong) → EN→AN.
        // But this test is about INSIDE the isolate not seeing the
        // outer AL. Place EN inside the isolate, AL outside.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        // Layout: AL LRI EN PDI. Outer sequence: [AL, LRI, PDI].
        // Inner sequence: [EN].
        let text = format!("ا{lri}5{pdi}");
        let out = resolve_pipeline(&text, 1);
        // Inner sequence's sos = max(outer_level=1, inner_level=2)
        // is 2 → even → sos = L. So in the inner sequence the EN
        // gets converted by W7 (sos=L), not by W2 (no AL in inner
        // sequence). Both paths end at L.
        // Crucially, the EN was NOT converted to AN — that would
        // happen if W2 crossed sequence boundaries.
        assert_eq!(out.classes[2], BidiClass::L);
        assert_ne!(out.classes[2], BidiClass::AN);
    }

    #[test]
    fn w2_crosses_runs_inside_one_isolating_run_sequence() {
        // Outer sequence spans across a matched LRI...PDI (BD13
        // connects the outer runs through the isolate). An AL
        // before the LRI must influence an EN after the PDI in
        // the OUTER sequence.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        // Outer sequence chars: AL, LRI, PDI, EN. Inner: 'a'.
        let text = format!("ا{lri}a{pdi}5");
        let out = resolve_pipeline(&text, 1);
        // After W3: AL → R at idx 0.
        // After W2 on outer: when walking [R, LRI, PDI, EN], the
        // last strong before EN is R, not AL → EN stays EN. Then
        // W7 on outer: last strong before EN is R → EN stays EN.
        // So EN at idx 4 = EN.
        // But the point is W rules are sequence-local: had we
        // mistakenly processed AL and EN as belonging to separate
        // sequences, the result would still be EN at the end. So
        // this test verifies the *converse*: if AL were preserved
        // (not converted by W3 before W2 in outer pass), W2 inside
        // outer sequence WOULD convert EN to AN. Since W3 runs
        // after W2 in our impl per UAX #9 order, the AL is still
        // AL when W2 fires and EN→AN.
        assert_eq!(out.classes[4], BidiClass::AN);
    }

    #[test]
    fn w_pipeline_dollar_then_digits_becomes_l_in_ltr() {
        // "$50" in LTR paragraph:
        //  - W5: ETs adjacent to EN → EN. classes: [EN, EN, EN].
        //  - W7: sos=L, EN→L. classes: [L, L, L].
        let out = resolve_pipeline("$50", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn w_pipeline_arabic_number_in_arabic_text() {
        // "ا5" in RTL: AL precedes EN.
        //  - W2: EN→AN.
        //  - W3: AL→R.
        //  - Final: [R, AN].
        let out = resolve_pipeline("ا5", 1);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::AN]);
    }

    // ---- R51.20 §5.37.4 — BD16 paired bracket lookup ----

    #[test]
    fn bd16_ascii_parenthesis_pair() {
        assert_eq!(paired_bracket('('), Some((')', BracketType::Open)));
        assert_eq!(paired_bracket(')'), Some(('(', BracketType::Close)));
    }

    #[test]
    fn bd16_ascii_square_bracket_pair() {
        assert_eq!(paired_bracket('['), Some((']', BracketType::Open)));
        assert_eq!(paired_bracket(']'), Some(('[', BracketType::Close)));
    }

    #[test]
    fn bd16_ascii_curly_brace_pair() {
        assert_eq!(paired_bracket('{'), Some(('}', BracketType::Open)));
        assert_eq!(paired_bracket('}'), Some(('{', BracketType::Close)));
    }

    #[test]
    fn bd16_letter_is_not_a_paired_bracket() {
        assert_eq!(paired_bracket('a'), None);
        assert_eq!(paired_bracket('Z'), None);
        assert_eq!(paired_bracket('5'), None);
        assert_eq!(paired_bracket('א'), None);
    }

    #[test]
    fn bd16_mathematical_bracket_pair() {
        // U+27E6 MATHEMATICAL LEFT WHITE SQUARE BRACKET / U+27E7
        // matching close. Confirms non-Latin BMP brackets are
        // looked up correctly.
        assert_eq!(
            paired_bracket('\u{27E6}'),
            Some(('\u{27E7}', BracketType::Open)),
        );
        assert_eq!(
            paired_bracket('\u{27E7}'),
            Some(('\u{27E6}', BracketType::Close)),
        );
        // U+2030 PER MILLE SIGN — in the U+2000 punctuation range
        // but not a paired bracket. Verifies the binary search
        // correctly reports None for near-neighbors.
        assert_eq!(paired_bracket('\u{2030}'), None);
    }

    #[test]
    fn bd16_tibetan_bracket_pair() {
        // U+0F3A TIBETAN MARK GUG RTAGS GYON / U+0F3B GYAS — first
        // non-ASCII pair in the BidiBrackets table; confirms BMP
        // lookup beyond the basic Latin range.
        assert_eq!(
            paired_bracket('\u{0F3A}'),
            Some(('\u{0F3B}', BracketType::Open)),
        );
        assert_eq!(
            paired_bracket('\u{0F3B}'),
            Some(('\u{0F3A}', BracketType::Close)),
        );
    }

    #[test]
    fn bd16_round_trip_invariant() {
        // For every paired bracket, paired_bracket(matching) must
        // report the original character back, with the inverse kind.
        // Sample several pairs across the table to catch any
        // sort/parse regression in the codegen layer.
        for ch in ['(', '[', '{', '\u{0F3A}', '\u{27E6}', '\u{2983}'] {
            let (matching, kind) = paired_bracket(ch).expect("paired bracket");
            assert_eq!(kind, BracketType::Open);
            let (back, back_kind) =
                paired_bracket(matching).expect("inverse paired bracket");
            assert_eq!(back, ch);
            assert_eq!(back_kind, BracketType::Close);
        }
    }

    // ---- R51.21 §5.37.4 — N-rules (resolve_neutral_types) ----

    fn resolve_full(text: &str, paragraph_level: u8) -> ExplicitLevels {
        let post_x = resolve_explicit_levels(text, paragraph_level);
        let post_w = resolve_weak_types(post_x, paragraph_level);
        resolve_neutral_types(post_w, text, paragraph_level)
    }

    #[test]
    fn n_rules_empty_paragraph() {
        let out = resolve_full("", 0);
        assert!(out.classes.is_empty());
    }

    #[test]
    fn n_rules_pure_ltr_unchanged() {
        // No neutrals, no bracket pairs — N rules are no-ops.
        let out = resolve_full("abc", 0);
        assert_eq!(out.classes, vec![BidiClass::L; 3]);
    }

    // ---- N0: bracket pair direction ----

    #[test]
    fn n0_bracket_embedding_direction_match_lt_ltr() {
        // "(a)" in LTR paragraph. Embedding = L, inner 'a' is L (matches
        // embed) → brackets become L.
        let out = resolve_full("(a)", 0);
        assert_eq!(
            out.classes,
            vec![BidiClass::L, BidiClass::L, BidiClass::L],
        );
    }

    #[test]
    fn n0_bracket_opposite_strong_with_ltr_context() {
        // "a(אb)c" in LTR — inside the parens, first strong is R
        // (the Hebrew). Embed = L, opposite = R. Preceding context
        // (scan back from open paren) is 'a' = L = embed direction.
        // Per N0: context not opposite → fall back to embed → brackets = L.
        let out = resolve_full("a(אb)c", 0);
        // 'a' L, '(' L (N0 embed fallback), 'א' R, 'b' L, ')' L, 'c' L.
        assert_eq!(out.classes[0], BidiClass::L);
        assert_eq!(out.classes[1], BidiClass::L);
        assert_eq!(out.classes[4], BidiClass::L);
        assert_eq!(out.classes[5], BidiClass::L);
    }

    #[test]
    fn n0_bracket_opposite_strong_with_opposite_context() {
        // "א(אb)" in RTL: embed = R, inside has 'א' (R) AND 'b' (L).
        // First strong inside is 'א' = R = embed → brackets become R.
        let out = resolve_full("א(אb)", 1);
        // Indices: 'א'(0), '('(1), 'א'(2), 'b'(3), ')'(4).
        assert_eq!(out.classes[1], BidiClass::R);
        assert_eq!(out.classes[4], BidiClass::R);
    }

    #[test]
    fn n0_bracket_opposite_first_strong_with_opposite_preceding_context() {
        // "א(b)" in LTR paragraph. Embed=L, opposite=R.
        // Inside parens: 'b' (L) — matches embed direction immediately.
        // So brackets → L (embed match).
        let out = resolve_full("א(b)", 0);
        assert_eq!(out.classes[1], BidiClass::L); // '('
        assert_eq!(out.classes[3], BidiClass::L); // ')'
    }

    #[test]
    fn n0_empty_bracket_pair_stays_neutral_for_n1_n2() {
        // "(\u{0020})" in LTR (space inside, no strong).
        // N0: no strong inside → brackets stay ON.
        // N1: ON between sos=L and eos=L → L.
        // Final brackets and inner space → L.
        let out = resolve_full("( )", 0);
        assert_eq!(out.classes, vec![BidiClass::L; 3]);
    }

    // ---- N1: matched-strong neutral runs ----

    #[test]
    fn n1_neutral_between_l_neighbors_becomes_l() {
        // "a b" (space class WS, an NI). Both sides L → space becomes L.
        let out = resolve_full("a b", 0);
        assert_eq!(out.classes, vec![BidiClass::L; 3]);
    }

    #[test]
    fn n1_neutral_between_r_neighbors_becomes_r() {
        // "א ב" (RTL). Both sides R → space becomes R.
        let out = resolve_full("א ב", 1);
        assert_eq!(out.classes, vec![BidiClass::R; 3]);
    }

    #[test]
    fn n1_en_treated_as_r_for_influence_in_rtl() {
        // "1 ב" in RTL paragraph. sos=R.
        //   Post-W: 1 stays EN (sos=R so W7 doesn't fire).
        //   N1 looks at space: before = n_strong_direction(EN) = R,
        //   after = n_strong_direction(R) = R → space becomes R.
        let out = resolve_full("1 ב", 1);
        assert_eq!(out.classes, vec![BidiClass::EN, BidiClass::R, BidiClass::R]);
    }

    #[test]
    fn n1_mismatched_neighbors_fall_to_n2() {
        // "a ב" in LTR: L on left, R on right (mismatched).
        // N1 doesn't fire on space. N2: space → embed_dir = L (level 0).
        let out = resolve_full("a ב", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L, BidiClass::R]);
    }

    // ---- N2: residual neutrals → embedding direction ----

    #[test]
    fn n2_leading_neutral_becomes_embedding_direction() {
        // " a" in LTR: sos = L, eos = ... but the run before 'a' is
        // sos=L, after 'a' is L. Wait, the NI is only at start.
        // Actually for " a": NI run is [' ']. before = sos = L, after = L.
        // N1 fires → ' ' becomes L.
        let out = resolve_full(" a", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn n2_trailing_neutral_in_rtl_becomes_r() {
        // "א " in RTL: NI is [' ']. before = R, after = eos = R. N1
        // fires → space becomes R.
        let out = resolve_full("א ", 1);
        assert_eq!(out.classes, vec![BidiClass::R, BidiClass::R]);
    }

    #[test]
    fn n2_neutral_only_paragraph_becomes_embed() {
        // " " alone in LTR: NI is [' ']. sos = eos = L. N1 fires → L.
        let out = resolve_full(" ", 0);
        assert_eq!(out.classes, vec![BidiClass::L]);
    }

    #[test]
    fn n2_neutral_only_paragraph_in_rtl() {
        let out = resolve_full(" ", 1);
        assert_eq!(out.classes, vec![BidiClass::R]);
    }

    // ---- Pipeline composition ----

    #[test]
    fn pipeline_bracket_inside_hebrew_text() {
        // "א(a)ב" in RTL. Embed=R. Inside (): 'a' is L (opposite of embed).
        // Preceding context (scan back from '('): 'א' = R = embed.
        // Per N0: opposite-strong-found AND preceding-context=embed → embed.
        // So brackets → R. Then N1/N2 don't fire on the brackets again.
        let out = resolve_full("א(a)ב", 1);
        assert_eq!(out.classes[1], BidiClass::R); // '('
        assert_eq!(out.classes[3], BidiClass::R); // ')'
    }

    #[test]
    fn pipeline_nested_brackets_inner_pair_resolves_first() {
        // "([a])" in LTR: nested brackets. Inner [a] has L inside,
        // matches embed → [a] brackets become L. Outer (...) has L
        // chars inside (the inner [a] resolved to LLL) → outer also
        // becomes L.
        let out = resolve_full("([a])", 0);
        // '(' [ 'a' ] ')'
        assert_eq!(out.classes, vec![BidiClass::L; 5]);
    }

    #[test]
    fn pipeline_bracket_with_only_whitespace_inside() {
        // "א(  )ב" in RTL. Inside (...): only spaces (NI), no strong.
        // N0 leaves brackets as ON. N1 then sees the brackets and
        // spaces collectively as a single NI run between two R chars
        // ('א' and 'ב') → all become R.
        let out = resolve_full("א(  )ב", 1);
        assert_eq!(out.classes, vec![BidiClass::R; 6]);
    }

    #[test]
    fn pipeline_unmatched_brackets_stay_neutral_then_n2() {
        // ")(": no matching pairs (close before open). Both ')' and
        // '(' stay ON via N0 (no pair recorded). N1: no strong
        // neighbours (sos=eos=L if LTR paragraph). N1 fires with both
        // L → both become L.
        let out = resolve_full(")(", 0);
        assert_eq!(out.classes, vec![BidiClass::L, BidiClass::L]);
    }

    #[test]
    fn pipeline_isolate_initiator_resolves_to_strong() {
        // LRI by itself in LTR paragraph. LRI is treated as NI by
        // N1/N2.
        let lri = '\u{2066}';
        let pdi = '\u{2069}';
        let text = format!("a{lri}b{pdi}c");
        let out = resolve_full(&text, 0);
        // Outer sequence: [a, LRI, PDI, c]. All NIs (LRI/PDI) between
        // L's → become L via N1.
        assert_eq!(out.classes[1], BidiClass::L); // LRI
        assert_eq!(out.classes[3], BidiClass::L); // PDI
    }
}
