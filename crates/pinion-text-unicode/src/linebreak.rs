//! R50.7 §5.37.7 — line-break property substrate (UAX #14).
//!
//! This first slice lands the `LineBreak` enum (UAX #14 Table 1, 48
//! `Line_Break` property values) and the codepoint → class lookup via
//! a build.rs codegen'd range table (`LINE_BREAK_CLASS_RANGES`, parsed
//! from UCD 16.0 `LineBreak.txt`). The pair-table break algorithm
//! (rules `LB1`–`LB31`, validated against `LineBreakTest.txt`) is the
//! follow-up slice (R50.7.x) — this layer is the substrate every rule
//! reads, exactly as the `BidiClass` table (§5.37.4) precedes the
//! UAX #9 resolution rules.
//!
//! Why a property substrate first: line breaking is the layer above
//! shaping (§5.37.6) that decides *where* a run may wrap. Every LB
//! rule is phrased in terms of the `Line_Break` class of adjacent
//! characters, so the class lookup is the irreducible foundation; it
//! is meaningless to encode `LB2`…`LB31` before the classes they pair
//! on exist. Landing it standalone keeps each round a complete,
//! UCD-conformant artifact (the table is validated against the
//! published property assignments) rather than a half-built algorithm.
//!
//! External dependencies: zero. `std` only — mirrors the §5.37.3 NFC
//! engine and §5.37.4 BIDI policy.

/// UAX #14 Table 1 — the 48 `Line_Break` property values. Discriminant
/// order is alphabetical by the UCD short name (the
/// `PropertyValueAliases` order, and the order the values first appear
/// in a sorted `LineBreak.txt`), matching the `u8` indices emitted by
/// `build.rs` (`AI = 0`, …, `ZWJ = 47`) so [`LineBreak::from_index`]
/// is a direct enum cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LineBreak {
    /// AI — Ambiguous (alphabetic or ideograph; context-dependent).
    AI = 0,
    /// AK — Aksara (Brahmic orthographic syllable base).
    AK = 1,
    /// AL — Alphabetic (the catch-all for ordinary letters).
    AL = 2,
    /// AP — Aksara Pre-Base.
    AP = 3,
    /// AS — Aksara Start.
    AS = 4,
    /// B2 — Break Opportunity Before and After (em dash).
    B2 = 5,
    /// BA — Break After (spaces, hyphens, some punctuation).
    BA = 6,
    /// BB — Break Before.
    BB = 7,
    /// BK — Mandatory Break (line/paragraph separators).
    BK = 8,
    /// CB — Contingent Break Opportunity (object replacement / inline
    /// objects whose breaking is decided out of band).
    CB = 9,
    /// CJ — Conditional Japanese Starter (small kana; tailorable NS).
    CJ = 10,
    /// CL — Close Punctuation.
    CL = 11,
    /// CM — Combining Mark (attaches to the preceding base, `LB9`).
    CM = 12,
    /// CP — Close Parenthesis.
    CP = 13,
    /// CR — Carriage Return.
    CR = 14,
    /// EB — Emoji Base (takes an emoji modifier).
    EB = 15,
    /// EM — Emoji Modifier (skin-tone, etc.).
    EM = 16,
    /// EX — Exclamation / Interrogation.
    EX = 17,
    /// GL — Non-breaking ("Glue") — NBSP, word joiner-like glue.
    GL = 18,
    /// H2 — Hangul LV syllable.
    H2 = 19,
    /// H3 — Hangul LVT syllable.
    H3 = 20,
    /// HL — Hebrew Letter.
    HL = 21,
    /// HY — Hyphen (U+002D HYPHEN-MINUS).
    HY = 22,
    /// ID — Ideographic (CJK; breaks freely on both sides).
    ID = 23,
    /// IN — Inseparable (leaders, ellipsis).
    IN = 24,
    /// IS — Infix Numeric Separator (comma, period in numbers).
    IS = 25,
    /// JL — Hangul L Jamo (leading consonant).
    JL = 26,
    /// JT — Hangul T Jamo (trailing consonant).
    JT = 27,
    /// JV — Hangul V Jamo (vowel).
    JV = 28,
    /// LF — Line Feed.
    LF = 29,
    /// NL — Next Line.
    NL = 30,
    /// NS — Nonstarter (small kana, some marks; no break before).
    NS = 31,
    /// NU — Numeric (digits).
    NU = 32,
    /// OP — Open Punctuation.
    OP = 33,
    /// PO — Postfix Numeric (percent, degree).
    PO = 34,
    /// PR — Prefix Numeric (currency, sign).
    PR = 35,
    /// QU — Quotation.
    QU = 36,
    /// RI — Regional Indicator (flag sequence component).
    RI = 37,
    /// SA — Complex-Context Dependent (South-East Asian; needs a
    /// dictionary for true breaking — resolved to AL by default).
    SA = 38,
    /// SG — Surrogate. Unreachable through [`line_break_class`]
    /// (a Rust `char` can never be a surrogate); present only so the
    /// property value set is complete.
    SG = 39,
    /// SP — Space (U+0020).
    SP = 40,
    /// SY — Symbols Allowing Break After (solidus).
    SY = 41,
    /// VF — Virama Final.
    VF = 42,
    /// VI — Virama.
    VI = 43,
    /// WJ — Word Joiner (no break on either side).
    WJ = 44,
    /// XX — Unknown (the `@missing` default for unassigned codepoints;
    /// resolved to AL by the algorithm, `LB1`).
    XX = 45,
    /// ZW — Zero Width Space (break opportunity after).
    ZW = 46,
    /// ZWJ — Zero Width Joiner (emoji / grapheme glue, `LB8a`).
    ZWJ = 47,
}

impl LineBreak {
    /// Decode the `u8` index emitted by `build.rs` back into the enum.
    /// Panics on unknown values — the table is closed-set per UAX #14
    /// and `lb_class_index` (codegen) already rejects unknown class
    /// names, so an unknown index here indicates a generator / runtime
    /// version skew.
    #[must_use]
    pub const fn from_index(idx: u8) -> Self {
        match idx {
            0 => LineBreak::AI,
            1 => LineBreak::AK,
            2 => LineBreak::AL,
            3 => LineBreak::AP,
            4 => LineBreak::AS,
            5 => LineBreak::B2,
            6 => LineBreak::BA,
            7 => LineBreak::BB,
            8 => LineBreak::BK,
            9 => LineBreak::CB,
            10 => LineBreak::CJ,
            11 => LineBreak::CL,
            12 => LineBreak::CM,
            13 => LineBreak::CP,
            14 => LineBreak::CR,
            15 => LineBreak::EB,
            16 => LineBreak::EM,
            17 => LineBreak::EX,
            18 => LineBreak::GL,
            19 => LineBreak::H2,
            20 => LineBreak::H3,
            21 => LineBreak::HL,
            22 => LineBreak::HY,
            23 => LineBreak::ID,
            24 => LineBreak::IN,
            25 => LineBreak::IS,
            26 => LineBreak::JL,
            27 => LineBreak::JT,
            28 => LineBreak::JV,
            29 => LineBreak::LF,
            30 => LineBreak::NL,
            31 => LineBreak::NS,
            32 => LineBreak::NU,
            33 => LineBreak::OP,
            34 => LineBreak::PO,
            35 => LineBreak::PR,
            36 => LineBreak::QU,
            37 => LineBreak::RI,
            38 => LineBreak::SA,
            39 => LineBreak::SG,
            40 => LineBreak::SP,
            41 => LineBreak::SY,
            42 => LineBreak::VF,
            43 => LineBreak::VI,
            44 => LineBreak::WJ,
            45 => LineBreak::XX,
            46 => LineBreak::ZW,
            47 => LineBreak::ZWJ,
            _ => panic!("LineBreak::from_index: out-of-range line-break class index"),
        }
    }

    /// UCD source name (the literal short name in `LineBreak.txt`).
    #[must_use]
    pub const fn ucd_name(self) -> &'static str {
        match self {
            LineBreak::AI => "AI",
            LineBreak::AK => "AK",
            LineBreak::AL => "AL",
            LineBreak::AP => "AP",
            LineBreak::AS => "AS",
            LineBreak::B2 => "B2",
            LineBreak::BA => "BA",
            LineBreak::BB => "BB",
            LineBreak::BK => "BK",
            LineBreak::CB => "CB",
            LineBreak::CJ => "CJ",
            LineBreak::CL => "CL",
            LineBreak::CM => "CM",
            LineBreak::CP => "CP",
            LineBreak::CR => "CR",
            LineBreak::EB => "EB",
            LineBreak::EM => "EM",
            LineBreak::EX => "EX",
            LineBreak::GL => "GL",
            LineBreak::H2 => "H2",
            LineBreak::H3 => "H3",
            LineBreak::HL => "HL",
            LineBreak::HY => "HY",
            LineBreak::ID => "ID",
            LineBreak::IN => "IN",
            LineBreak::IS => "IS",
            LineBreak::JL => "JL",
            LineBreak::JT => "JT",
            LineBreak::JV => "JV",
            LineBreak::LF => "LF",
            LineBreak::NL => "NL",
            LineBreak::NS => "NS",
            LineBreak::NU => "NU",
            LineBreak::OP => "OP",
            LineBreak::PO => "PO",
            LineBreak::PR => "PR",
            LineBreak::QU => "QU",
            LineBreak::RI => "RI",
            LineBreak::SA => "SA",
            LineBreak::SG => "SG",
            LineBreak::SP => "SP",
            LineBreak::SY => "SY",
            LineBreak::VF => "VF",
            LineBreak::VI => "VI",
            LineBreak::WJ => "WJ",
            LineBreak::XX => "XX",
            LineBreak::ZW => "ZW",
            LineBreak::ZWJ => "ZWJ",
        }
    }
}

#[allow(
    dead_code,
    clippy::unreadable_literal,
    clippy::doc_markdown,
    reason = "Generated table; consumed by line_break_class()."
)]
mod tables {
    include!(concat!(env!("OUT_DIR"), "/linebreak_tables.rs"));
    pub use LINE_BREAK_CLASS_RANGES as RANGES;
}

/// Look up the UAX #14 `Line_Break` class for `cp`. Binary-searches the
/// codegen'd `(start, end, class_idx)` range table.
///
/// A codepoint outside every published range returns [`LineBreak::XX`]
/// — the UCD `@missing: 0000..10FFFF; XX` default. (Unlike
/// `DerivedBidiClass.txt`, `LineBreak.txt` carries only that single
/// universal `@missing` directive, so no `@missing` ranges are folded
/// into the table and the fallback is the genuine default, not a gap
/// sentinel.) The algorithm's `LB1` step later resolves XX (and AI,
/// SG, SA, CJ) to concrete classes; this layer reports the raw
/// property.
///
/// `SG` (surrogate) values can never be reached: a Rust `char` excludes
/// the surrogate range by construction.
#[must_use]
pub fn line_break_class(cp: char) -> LineBreak {
    let cp = cp as u32;
    let ranges = tables::RANGES;
    // Largest range whose `start <= cp`, then confirm `cp <= end`.
    // Mirrors `bidi::bidi_class` (§5.37.4) — the second range-property
    // lookup of this exact `(start, end, idx)` shape. A third such
    // property (e.g. §5.37.5 Script / East_Asian_Width) is the
    // Rule-of-Three trigger to lift a shared range-search helper; at
    // two sites the parallel is kept explicit rather than abstracted.
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
            return LineBreak::from_index(ranges[mid].2);
        }
    }
    LineBreak::XX
}

#[cfg(test)]
mod tests {
    use super::{LineBreak, line_break_class, tables};

    /// Every `LineBreak` variant in discriminant order. Pins the enum
    /// against accidental reorder and against the `build.rs`
    /// `lb_class_index` map (the spot-checks below cross-validate the
    /// two through real codepoints).
    const ALL: [LineBreak; 48] = [
        LineBreak::AI,
        LineBreak::AK,
        LineBreak::AL,
        LineBreak::AP,
        LineBreak::AS,
        LineBreak::B2,
        LineBreak::BA,
        LineBreak::BB,
        LineBreak::BK,
        LineBreak::CB,
        LineBreak::CJ,
        LineBreak::CL,
        LineBreak::CM,
        LineBreak::CP,
        LineBreak::CR,
        LineBreak::EB,
        LineBreak::EM,
        LineBreak::EX,
        LineBreak::GL,
        LineBreak::H2,
        LineBreak::H3,
        LineBreak::HL,
        LineBreak::HY,
        LineBreak::ID,
        LineBreak::IN,
        LineBreak::IS,
        LineBreak::JL,
        LineBreak::JT,
        LineBreak::JV,
        LineBreak::LF,
        LineBreak::NL,
        LineBreak::NS,
        LineBreak::NU,
        LineBreak::OP,
        LineBreak::PO,
        LineBreak::PR,
        LineBreak::QU,
        LineBreak::RI,
        LineBreak::SA,
        LineBreak::SG,
        LineBreak::SP,
        LineBreak::SY,
        LineBreak::VF,
        LineBreak::VI,
        LineBreak::WJ,
        LineBreak::XX,
        LineBreak::ZW,
        LineBreak::ZWJ,
    ];

    #[test]
    fn enum_index_roundtrip_is_closed_and_distinct() {
        for (i, &lb) in ALL.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let idx = i as u8;
            assert_eq!(lb as u8, idx, "discriminant must equal array position");
            assert_eq!(LineBreak::from_index(idx), lb, "from_index round-trip");
        }
        // Names are unique (no two variants share a UCD short name).
        for (i, &a) in ALL.iter().enumerate() {
            for &b in &ALL[i + 1..] {
                assert_ne!(a.ucd_name(), b.ucd_name(), "duplicate ucd_name");
            }
        }
    }

    #[test]
    fn spot_checks_against_vendored_ucd() {
        // (codepoint, expected class) pairs read directly from
        // ucd/LineBreak.txt (UCD 16.0.0). Chosen to exercise a wide
        // spread of discriminants, including the Brahmic (AK/AP/AS/
        // VF/VI), emoji (EB/EM/RI/ZWJ) and Hangul (H2/H3/JL/JV/JT)
        // values added in recent Unicode versions, so a build/runtime
        // index skew on any class surfaces here.
        let cases: &[(char, LineBreak)] = &[
            (' ', LineBreak::SP),
            ('A', LineBreak::AL),
            ('0', LineBreak::NU),
            ('\r', LineBreak::CR),
            ('\n', LineBreak::LF),
            ('\u{000B}', LineBreak::BK),
            ('\u{0085}', LineBreak::NL),
            ('\t', LineBreak::BA),
            ('!', LineBreak::EX),
            ('(', LineBreak::OP),
            (')', LineBreak::CP),
            ('}', LineBreak::CL),
            (']', LineBreak::CP),
            (',', LineBreak::IS),
            ('/', LineBreak::SY),
            ('$', LineBreak::PR),
            ('%', LineBreak::PO),
            ('"', LineBreak::QU),
            ('-', LineBreak::HY),
            ('\u{2010}', LineBreak::BA),
            ('\u{2014}', LineBreak::B2),
            ('\u{2024}', LineBreak::IN),
            ('\u{00A0}', LineBreak::GL),
            ('\u{00B4}', LineBreak::BB),
            ('\u{0300}', LineBreak::CM),
            ('\u{200B}', LineBreak::ZW),
            ('\u{200D}', LineBreak::ZWJ),
            ('\u{2060}', LineBreak::WJ),
            ('\u{FFFC}', LineBreak::CB),
            ('\u{4E00}', LineBreak::ID),
            ('\u{3041}', LineBreak::CJ),
            ('\u{17D6}', LineBreak::NS),
            ('\u{0E01}', LineBreak::SA),
            ('\u{05D0}', LineBreak::HL),
            ('\u{AC00}', LineBreak::H2),
            ('\u{AC01}', LineBreak::H3),
            ('\u{1100}', LineBreak::JL),
            ('\u{1160}', LineBreak::JV),
            ('\u{11A8}', LineBreak::JT),
            ('\u{1B05}', LineBreak::AK),
            ('\u{11003}', LineBreak::AP),
            ('\u{1B50}', LineBreak::AS),
            ('\u{1BF2}', LineBreak::VF),
            ('\u{1B44}', LineBreak::VI),
            ('\u{261D}', LineBreak::EB),
            ('\u{1F3FB}', LineBreak::EM),
            ('\u{1F1E6}', LineBreak::RI),
        ];
        for &(cp, expected) in cases {
            assert_eq!(
                line_break_class(cp),
                expected,
                "U+{:04X} expected {}",
                cp as u32,
                expected.ucd_name()
            );
        }
    }

    #[test]
    fn unassigned_codepoints_default_to_xx() {
        // Reserved gaps absent from LineBreak.txt fall to the
        // @missing XX default, exercising the binary-search miss path.
        for &cp in &['\u{0378}', '\u{05EB}', '\u{1FFFE}'] {
            assert_eq!(line_break_class(cp), LineBreak::XX, "U+{:04X}", cp as u32);
        }
    }

    #[test]
    fn table_is_sorted_non_overlapping_and_in_bounds() {
        let ranges = tables::RANGES;
        assert!(!ranges.is_empty(), "table must not be empty");
        let mut prev_end: Option<u32> = None;
        for &(start, end, idx) in ranges {
            assert!(start <= end, "range start..end inverted at 0x{start:04X}");
            assert!(idx < 48, "class index {idx} out of enum range at 0x{start:04X}");
            // from_index must accept every emitted index (no skew).
            let _ = LineBreak::from_index(idx);
            if let Some(pe) = prev_end {
                assert!(pe < start, "ranges overlap / unsorted near 0x{start:04X}");
            }
            prev_end = Some(end);
        }
    }
}
