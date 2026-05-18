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

#[cfg(test)]
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
}
