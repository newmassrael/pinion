//! R50.7 §5.37.7 — line breaking (UAX #14).
//!
//! Two layers, landed in order (the `BidiClass` table → UAX #9 rules
//! precedent of §5.37.4):
//!
//! 1. **Property substrate** (R50.7): the `LineBreak` enum (UAX #14
//!    Table 1, 48 `Line_Break` values) and the codepoint → class lookup
//!    [`line_break_class`] over a build.rs codegen'd range table parsed
//!    from UCD 16.0 `LineBreak.txt`. Every LB rule is phrased in terms
//!    of the `Line_Break` class of adjacent characters, so the class
//!    lookup is the irreducible foundation.
//!
//! 2. **Break algorithm** (R50.7.x): [`line_break_opportunities`]
//!    applies the resolution + pair rules `LB1`–`LB31` to a string and
//!    returns the byte offsets at which a line may (or must) break. It
//!    is validated against the full vendored `LineBreakTest.txt`
//!    (UCD 16.0.0) conformance suite.
//!
//! Why a property substrate first: line breaking is the layer above
//! shaping (§5.37.6) that decides *where* a run may wrap; it is
//! meaningless to encode `LB2`…`LB31` before the classes they pair on
//! exist. Landing each standalone keeps every round a complete,
//! UCD-conformant artifact rather than a half-built algorithm.
//!
//! Deferred (honest gaps, not silent): the East Asian Width carve-outs
//! of `LB19a`/`LB30` (this engine has no `East_Asian_Width` substrate
//! yet — `$EastAsian` is treated as empty, which is exact for the
//! all-non-East-Asian conformance suite and over-applies the QU /
//! parenthesis non-break rules to real East Asian text); the `LB30b`
//! `[\p{Extended_Pictographic}&\p{Cn}] × EM` clause (needs the
//! `Extended_Pictographic` property); and the `LB1` `SA → CM`
//! resolution for `Mn`/`Mc` South-East Asian marks (resolved to `AL`;
//! the suite exercises no `SA` combining mark). Each follows the
//! `East_Asian_Width` / `Script` substrate (§5.37.5).
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

/// `true` if `c` has `General_Category=Initial_Punctuation` (Pi) — the
/// UAX #14 `[\p{Pi}&QU]` opening-quotation sub-class (`LB15a`, `LB19`).
fn is_initial_punctuation(c: char) -> bool {
    tables::INITIAL_PUNCTUATION
        .binary_search(&(c as u32))
        .is_ok()
}

/// `true` if `c` has `General_Category=Final_Punctuation` (Pf) — the
/// UAX #14 `[\p{Pf}&QU]` closing-quotation sub-class (`LB15b`, `LB19`).
fn is_final_punctuation(c: char) -> bool {
    tables::FINAL_PUNCTUATION.binary_search(&(c as u32)).is_ok()
}

/// `true` if `c` is an East Asian character per the UAX #14 `$EastAsian`
/// macro — `East_Asian_Width` `F`, `W`, or `H` — tested by the `LB19a`,
/// `LB21a` and `LB30` carve-outs. A codepoint outside every range
/// defaults to `N` (not East Asian) per `EastAsianWidth.txt`'s single
/// `@missing: 0000..10FFFF; N`; the block-level `W` default for some
/// unassigned CJK ranges is not applied (no such codepoint reaches the
/// rules in practice, and the conformance suite samples are assigned).
fn is_east_asian_wide(c: char) -> bool {
    let cp = c as u32;
    tables::EAST_ASIAN_WIDE_RANGES
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A position in a string at which UAX #14 permits — or requires — a line
/// break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakOpportunity {
    /// Byte offset of the break in the source string: where the next line
    /// begins. `text[..offset]` ends a line and `text[offset..]` starts
    /// the next. Never `0` (`LB2`); the final opportunity is always
    /// `text.len()` (`LB3`).
    pub offset: usize,
    /// `true` for a mandatory break — the `LB4`/`LB5` hard breaks and the
    /// end-of-text break (`LB3`); `false` for an optional opportunity.
    pub mandatory: bool,
}

/// The break action UAX #14 assigns to the boundary after a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// `×` — no break permitted here.
    Prohibited,
    /// `÷` — an optional break opportunity.
    Allowed,
    /// `!` — a mandatory (forced) break.
    Mandatory,
}

/// Stage in a `NU (SY | IS)* (CL | CP)?` numeric run, tracked for `LB25`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumState {
    /// Not in a numeric run.
    None,
    /// Inside `NU (SY | IS)*`.
    InNumber,
    /// `NU (SY | IS)*` followed by one closing `CL`/`CP`.
    Closed,
}

/// `LB1` — resolve the tailorable classes to concrete ones using the
/// default (non-tailored) assignment. `AI`/`SG`/`XX`/`SA` → `AL`,
/// `CJ` → `NS`. `CB` is left for `LB20`. `SA` marks of
/// `General_Category` `Mn`/`Mc` should resolve to `CM`, but this engine
/// has no `Mn`/`Mc` substrate yet (see module docs); the conformance
/// suite exercises no such mark.
fn resolve_lb1(raw: LineBreak) -> LineBreak {
    use LineBreak as L;
    match raw {
        L::AI | L::SG | L::XX | L::SA => L::AL,
        L::CJ => L::NS,
        other => other,
    }
}

/// `LB25` numeric-run transition on consuming a character of class `c`.
fn next_num_state(state: NumState, c: LineBreak) -> NumState {
    use LineBreak as L;
    match (state, c) {
        (_, L::NU) | (NumState::InNumber, L::SY | L::IS) => NumState::InNumber,
        (NumState::InNumber, L::CL | L::CP) => NumState::Closed,
        _ => NumState::None,
    }
}

/// Compute the UAX #14 line-break opportunities of `text`.
///
/// Returns, in source order, every byte offset at which a line may or
/// must break: the optional opportunities together with the mandatory
/// hard breaks (`LB4`/`LB5`) and the end-of-text break (`LB3`). The
/// start of text is never a break (`LB2`); empty input yields none.
///
/// The full resolution + pair rules `LB1`–`LB31` are applied. See the
/// module documentation for the deliberately deferred East Asian Width
/// and emoji carve-outs.
#[must_use]
pub fn line_break_opportunities(text: &str) -> Vec<BreakOpportunity> {
    let chars: Vec<char> = text.chars().collect();
    let actions = break_actions(&chars);
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        offset += c.len_utf8();
        match actions[i] {
            Action::Prohibited => {}
            Action::Allowed => out.push(BreakOpportunity {
                offset,
                mandatory: false,
            }),
            Action::Mandatory => out.push(BreakOpportunity {
                offset,
                mandatory: true,
            }),
        }
    }
    out
}

/// One UAX #14 unit: a base character together with the combining marks
/// `LB9` folds into it. The pair rules `LB2`–`LB31` run over units, so
/// `LB9`/`LB10` need no per-rule special-casing — a break inside a
/// cluster is simply not a unit boundary.
struct Unit {
    /// Resolved (`LB1`) line-break class of the base, used by the rules.
    cls: LineBreak,
    /// The base character (for `is_*_punctuation`, `is_east_asian_wide`,
    /// and the literal U+25CC / U+2010 checks in `LB28a` / `LB20a`).
    base_char: char,
    /// Raw class of the *last* char in the cluster — `LB8a` (`× after
    /// ZWJ`) fires when a cluster ends in a `ZWJ`.
    last_raw: LineBreak,
    /// Index of the last char of the cluster (where the post-unit break
    /// action is recorded).
    last: usize,
}

/// Per-character break action: `actions[i]` is the action at the boundary
/// *after* `chars[i]` (between `chars[i]` and `chars[i+1]`). The last
/// entry is the end-of-text break (`LB3`, always [`Action::Mandatory`]).
///
/// A direct, in-order transcription of UAX #14 `LB1`–`LB31`: the first
/// matching rule decides each boundary. `LB1` resolution and `LB9`/`LB10`
/// combining-mark folding run as a pre-pass that collapses the string
/// into [`Unit`]s; the remaining rules are evaluated unit-to-unit against
/// the resolved class, the class of the last non-`SP` unit (the
/// `… SP* ×` rules), the consecutive regional-indicator count (`LB30a`)
/// and the numeric-run state (`LB25`).
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "Flat 1:1 transcription of UAX #14 LB1-LB31; splitting the \
              rule chain would obscure its correspondence to the spec. \
              `l`/`r` are the left/right break class, `u` the unit index, \
              `n`/`m` the char/unit counts — conventional for the scan."
)]
fn break_actions(chars: &[char]) -> Vec<Action> {
    use LineBreak as L;
    let n = chars.len();
    if n == 0 {
        return Vec::new();
    }

    // LB1 resolution + LB9/LB10 folding: collapse the string into units,
    // each a base char plus any trailing CM/ZWJ. A CM/ZWJ extends the
    // current unit unless that unit's class is BK/CR/LF/NL/SP/ZW; with no
    // such base it stands alone as AL (LB10).
    let mut units: Vec<Unit> = Vec::new();
    for (i, &ch) in chars.iter().enumerate() {
        let raw = line_break_class(ch);
        if matches!(raw, L::CM | L::ZWJ) {
            if let Some(u) = units.last_mut() {
                if !matches!(u.cls, L::BK | L::CR | L::LF | L::NL | L::SP | L::ZW) {
                    u.last_raw = raw;
                    u.last = i;
                    continue;
                }
            }
            units.push(Unit {
                cls: L::AL,
                base_char: ch,
                last_raw: raw,
                last: i,
            });
        } else {
            units.push(Unit {
                cls: resolve_lb1(raw),
                base_char: ch,
                last_raw: raw,
                last: i,
            });
        }
    }

    let m = units.len();
    let ea = |k: usize| is_east_asian_wide(units[k].base_char);
    let is_ak_group =
        |k: usize| matches!(units[k].cls, L::AK | L::AS) || units[k].base_char == '\u{25CC}';
    let is_ak_or_dotted = |k: usize| units[k].cls == L::AK || units[k].base_char == '\u{25CC}';

    let mut unit_action = vec![Action::Prohibited; m];
    let mut last_nonsp: Option<L> = None;
    let mut last_nonsp_idx = 0usize;
    let mut ri_run = 0usize;
    let mut num_state = NumState::None;

    for u in 0..m {
        let l = units[u].cls;
        // Advance running state to include unit u.
        if l != L::SP {
            last_nonsp = Some(l);
            last_nonsp_idx = u;
        }
        if l == L::RI {
            ri_run += 1;
        } else {
            ri_run = 0;
        }
        num_state = next_num_state(num_state, l);

        if u == m - 1 {
            unit_action[u] = Action::Mandatory; // LB3
            break;
        }

        let r = units[u + 1].cls;
        unit_action[u] = 'decide: {
            // LB4 / LB5 — mandatory breaks.
            if l == L::BK {
                break 'decide Action::Mandatory;
            }
            if l == L::CR && r == L::LF {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::CR | L::LF | L::NL) {
                break 'decide Action::Mandatory;
            }
            // LB6 — × before a hard break.
            if matches!(r, L::BK | L::CR | L::LF | L::NL) {
                break 'decide Action::Prohibited;
            }
            // LB7 — × before SP / ZW.
            if matches!(r, L::SP | L::ZW) {
                break 'decide Action::Prohibited;
            }
            // LB8 — ZW SP* ÷.
            if last_nonsp == Some(L::ZW) {
                break 'decide Action::Allowed;
            }
            // LB8a — × after ZWJ (a cluster may end in one).
            if units[u].last_raw == L::ZWJ {
                break 'decide Action::Prohibited;
            }
            // LB11 — × around WJ.
            if l == L::WJ || r == L::WJ {
                break 'decide Action::Prohibited;
            }
            // LB12 — GL ×.
            if l == L::GL {
                break 'decide Action::Prohibited;
            }
            // LB12a — [^SP BA HY] × GL.
            if !matches!(l, L::SP | L::BA | L::HY) && r == L::GL {
                break 'decide Action::Prohibited;
            }
            // LB13 — × CL / CP / EX / SY.
            if matches!(r, L::CL | L::CP | L::EX | L::SY) {
                break 'decide Action::Prohibited;
            }
            // LB14 — OP SP* ×.
            if last_nonsp == Some(L::OP) {
                break 'decide Action::Prohibited;
            }
            // LB15a — (sot|BK|CR|LF|NL|OP|QU|GL|SP|ZW) [Pi&QU] SP* ×.
            if last_nonsp == Some(L::QU) {
                let j = last_nonsp_idx;
                if is_initial_punctuation(units[j].base_char)
                    && (j == 0
                        || matches!(
                            units[j - 1].cls,
                            L::BK | L::CR | L::LF | L::NL | L::OP | L::QU | L::GL | L::SP | L::ZW
                        ))
                {
                    break 'decide Action::Prohibited;
                }
            }
            // LB15b — × [Pf&QU] (SP|GL|WJ|CL|QU|CP|EX|IS|SY|BK|CR|LF|NL|ZW|eot).
            if r == L::QU && is_final_punctuation(units[u + 1].base_char) {
                let follower_ok = u + 2 >= m
                    || matches!(
                        units[u + 2].cls,
                        L::SP
                            | L::GL
                            | L::WJ
                            | L::CL
                            | L::QU
                            | L::CP
                            | L::EX
                            | L::IS
                            | L::SY
                            | L::BK
                            | L::CR
                            | L::LF
                            | L::NL
                            | L::ZW
                    );
                if follower_ok {
                    break 'decide Action::Prohibited;
                }
            }
            // LB15c — SP ÷ IS NU.
            if l == L::SP && r == L::IS && u + 2 < m && units[u + 2].cls == L::NU {
                break 'decide Action::Allowed;
            }
            // LB15d — × IS.
            if r == L::IS {
                break 'decide Action::Prohibited;
            }
            // LB16 — (CL|CP) SP* × NS.
            if matches!(last_nonsp, Some(L::CL | L::CP)) && r == L::NS {
                break 'decide Action::Prohibited;
            }
            // LB17 — B2 SP* × B2.
            if last_nonsp == Some(L::B2) && r == L::B2 {
                break 'decide Action::Prohibited;
            }
            // LB18 — SP ÷.
            if l == L::SP {
                break 'decide Action::Allowed;
            }
            // LB19 — × [QU-Pi] ; [QU-Pf] ×.
            if r == L::QU && !is_initial_punctuation(units[u + 1].base_char) {
                break 'decide Action::Prohibited;
            }
            if l == L::QU && !is_final_punctuation(units[u].base_char) {
                break 'decide Action::Prohibited;
            }
            // LB19a — do not break either side of a QU unless surrounded
            // by East Asian characters.
            if r == L::QU && (!ea(u) || u + 2 >= m || !ea(u + 2)) {
                break 'decide Action::Prohibited;
            }
            if l == L::QU && (!ea(u + 1) || u == 0 || !ea(u - 1)) {
                break 'decide Action::Prohibited;
            }
            // LB20 — ÷ around CB.
            if l == L::CB || r == L::CB {
                break 'decide Action::Allowed;
            }
            // LB20a — (sot|BK|CR|LF|NL|SP|ZW|CB|GL) (HY | U+2010) × AL.
            if (l == L::HY || units[u].base_char == '\u{2010}')
                && r == L::AL
                && (u == 0
                    || matches!(
                        units[u - 1].cls,
                        L::BK | L::CR | L::LF | L::NL | L::SP | L::ZW | L::CB | L::GL
                    ))
            {
                break 'decide Action::Prohibited;
            }
            // LB21 — × BA / HY / NS ; BB ×.
            if matches!(r, L::BA | L::HY | L::NS) {
                break 'decide Action::Prohibited;
            }
            if l == L::BB {
                break 'decide Action::Prohibited;
            }
            // LB21a — HL (HY | [BA - EastAsian]) × [^HL].
            if u > 0
                && units[u - 1].cls == L::HL
                && (l == L::HY || (l == L::BA && !ea(u)))
                && r != L::HL
            {
                break 'decide Action::Prohibited;
            }
            // LB21b — SY × HL.
            if l == L::SY && r == L::HL {
                break 'decide Action::Prohibited;
            }
            // LB22 — × IN.
            if r == L::IN {
                break 'decide Action::Prohibited;
            }
            // LB23 — (AL|HL) × NU ; NU × (AL|HL).
            if matches!(l, L::AL | L::HL) && r == L::NU {
                break 'decide Action::Prohibited;
            }
            if l == L::NU && matches!(r, L::AL | L::HL) {
                break 'decide Action::Prohibited;
            }
            // LB23a — PR × (ID|EB|EM) ; (ID|EB|EM) × PO.
            if l == L::PR && matches!(r, L::ID | L::EB | L::EM) {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::ID | L::EB | L::EM) && r == L::PO {
                break 'decide Action::Prohibited;
            }
            // LB24 — (PR|PO) × (AL|HL) ; (AL|HL) × (PR|PO).
            if matches!(l, L::PR | L::PO) && matches!(r, L::AL | L::HL) {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::AL | L::HL) && matches!(r, L::PR | L::PO) {
                break 'decide Action::Prohibited;
            }
            // LB25 — numbers.
            if matches!(num_state, NumState::InNumber | NumState::Closed)
                && matches!(r, L::PO | L::PR)
            {
                break 'decide Action::Prohibited;
            }
            if num_state == NumState::InNumber && r == L::NU {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::PO | L::PR) && r == L::NU {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::PO | L::PR) && r == L::OP {
                let glued = (u + 2 < m && units[u + 2].cls == L::NU)
                    || (u + 3 < m && units[u + 2].cls == L::IS && units[u + 3].cls == L::NU);
                if glued {
                    break 'decide Action::Prohibited;
                }
            }
            if matches!(l, L::HY | L::IS) && r == L::NU {
                break 'decide Action::Prohibited;
            }
            // LB26 — Korean syllable blocks.
            if l == L::JL && matches!(r, L::JL | L::JV | L::H2 | L::H3) {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::JV | L::H2) && matches!(r, L::JV | L::JT) {
                break 'decide Action::Prohibited;
            }
            if matches!(l, L::JT | L::H3) && r == L::JT {
                break 'decide Action::Prohibited;
            }
            // LB27 — Korean syllable block treated as ID for affixes.
            if matches!(l, L::JL | L::JV | L::JT | L::H2 | L::H3) && r == L::PO {
                break 'decide Action::Prohibited;
            }
            if l == L::PR && matches!(r, L::JL | L::JV | L::JT | L::H2 | L::H3) {
                break 'decide Action::Prohibited;
            }
            // LB28 — (AL|HL) × (AL|HL).
            if matches!(l, L::AL | L::HL) && matches!(r, L::AL | L::HL) {
                break 'decide Action::Prohibited;
            }
            // LB28a — Brahmic orthographic syllables (◌ = U+25CC).
            if l == L::AP && is_ak_group(u + 1) {
                break 'decide Action::Prohibited;
            }
            if is_ak_group(u) && matches!(r, L::VF | L::VI) {
                break 'decide Action::Prohibited;
            }
            if u > 0 && is_ak_group(u - 1) && l == L::VI && is_ak_or_dotted(u + 1) {
                break 'decide Action::Prohibited;
            }
            if is_ak_group(u) && is_ak_group(u + 1) && u + 2 < m && units[u + 2].cls == L::VF {
                break 'decide Action::Prohibited;
            }
            // LB29 — IS × (AL|HL).
            if l == L::IS && matches!(r, L::AL | L::HL) {
                break 'decide Action::Prohibited;
            }
            // LB30 — (AL|HL|NU) × [OP-EastAsian] ; [CP-EastAsian] × (AL|HL|NU).
            if matches!(l, L::AL | L::HL | L::NU) && r == L::OP && !ea(u + 1) {
                break 'decide Action::Prohibited;
            }
            if l == L::CP && !ea(u) && matches!(r, L::AL | L::HL | L::NU) {
                break 'decide Action::Prohibited;
            }
            // LB30a — RI RI ÷ RI (break only between regional-indicator pairs).
            if l == L::RI && r == L::RI {
                break 'decide if ri_run % 2 == 0 {
                    Action::Allowed
                } else {
                    Action::Prohibited
                };
            }
            // LB30b — EB × EM (the [Extended_Pictographic & Cn] × EM clause is deferred).
            if l == L::EB && r == L::EM {
                break 'decide Action::Prohibited;
            }
            // LB31 — break everywhere else.
            Action::Allowed
        };
    }

    // Map unit boundaries back to char positions; boundaries inside a
    // cluster are never a unit's `last`, so they keep the LB9 default `×`.
    let mut actions = vec![Action::Prohibited; n];
    for (u, unit) in units.iter().enumerate() {
        actions[unit.last] = unit_action[u];
    }
    actions
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
            ('\u{00A7}', LineBreak::AI),
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
            // Private-use area carries an explicit `; XX` row, so this
            // exercises XX through a table *hit* — distinct from the
            // search-*miss* default path covered by the next test.
            ('\u{E000}', LineBreak::XX),
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
            assert!(
                idx < 48,
                "class index {idx} out of enum range at 0x{start:04X}"
            );
            // from_index must accept every emitted index (no skew).
            let _ = LineBreak::from_index(idx);
            if let Some(pe) = prev_end {
                assert!(pe < start, "ranges overlap / unsorted near 0x{start:04X}");
            }
            prev_end = Some(end);
        }
    }

    // ---- LB1-LB31 break algorithm ----

    use super::{BreakOpportunity, line_break_opportunities};

    /// Resolve the break opportunities of `text` into one `÷`/`×` marker
    /// per boundary (`out[0]` = sot, `out[n]` = eot), mirroring the
    /// `LineBreakTest.txt` row form so a test can assert against it.
    fn boundary_breaks(text: &str) -> Vec<bool> {
        let n = text.chars().count();
        let mut offsets = Vec::with_capacity(n + 1);
        let mut off = 0usize;
        offsets.push(0usize);
        for c in text.chars() {
            off += c.len_utf8();
            offsets.push(off);
        }
        let mut breaks = vec![false; n + 1];
        for BreakOpportunity { offset, .. } in line_break_opportunities(text) {
            let b = offsets
                .binary_search(&offset)
                .expect("opportunity at a char boundary");
            breaks[b] = true;
        }
        breaks
    }

    #[test]
    fn empty_and_single_char_degenerate_cases() {
        assert!(
            line_break_opportunities("").is_empty(),
            "LB: empty text → no breaks"
        );
        // LB3 — a single character always has the trailing eot break.
        let opps = line_break_opportunities("a");
        assert_eq!(opps.len(), 1);
        assert_eq!(
            opps[0],
            BreakOpportunity {
                offset: 1,
                mandatory: true
            }
        );
    }

    #[test]
    fn spaces_words_and_mandatory_breaks() {
        // LB28 keeps a word whole; LB7/LB18 break after the space; LB3 eot.
        assert_eq!(
            boundary_breaks("ab cd"),
            [false, false, false, true, false, true]
        );
        // LB5 — LF is a mandatory hard break (offset after it), eot too.
        let opps = line_break_opportunities("a\nb");
        assert_eq!(
            opps[0],
            BreakOpportunity {
                offset: 2,
                mandatory: true
            }
        );
        // LB5 — CR×LF stays glued, the break falls after the pair.
        let crlf = line_break_opportunities("a\r\nb");
        assert!(crlf.iter().any(|o| o.offset == 3 && o.mandatory));
        assert!(!crlf.iter().any(|o| o.offset == 2), "no break inside CR LF");
    }

    /// Exhaustive UAX #14 conformance: every row of the vendored
    /// `LineBreakTest.txt` (UCD 16.0.0) must match
    /// [`line_break_opportunities`], save the single documented `LB30b`
    /// `[Extended_Pictographic & Cn] × EM` deferral (rule `[30.22]`,
    /// which needs the `Extended_Pictographic` property this engine does
    /// not yet vendor — see module docs).
    #[test]
    fn uax14_linebreaktest_conformance() {
        let cases = crate::test_fixture::load_line_break_test();
        assert!(
            cases.len() > 9000,
            "LineBreakTest.txt under-loaded: {} rows",
            cases.len()
        );
        let mut deferred = Vec::new();
        let mut unexpected = Vec::new();
        for case in &cases {
            let text: String = case.codepoints.iter().collect();
            if boundary_breaks(&text) == case.breaks {
                continue;
            }
            if case.comment.contains("[30.22]") {
                deferred.push(case.line_number);
            } else {
                unexpected.push((case.line_number, case.comment.clone()));
            }
        }
        assert!(
            unexpected.is_empty(),
            "{} unexpected LineBreakTest.txt mismatches (first few): {:?}",
            unexpected.len(),
            &unexpected[..unexpected.len().min(8)]
        );
        assert_eq!(
            deferred.len(),
            1,
            "exactly one [30.22] Extended_Pictographic&Cn line is deferred; \
             got lines {deferred:?}"
        );
    }
}
