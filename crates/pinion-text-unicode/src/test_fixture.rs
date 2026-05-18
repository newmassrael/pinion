//! Shared `#[cfg(test)]` fixture loaders for UCD conformance suites:
//!
//! * `load_normalization_test` — `NormalizationTest.txt` (UAX #15,
//!   consumed by per-form NFD/NFC/NFKD/NFKC sweeps).
//! * `load_bidi_character_test` — `BidiCharacterTest.txt` (UAX #9,
//!   consumed by the BIDI conformance harness in `bidi.rs`).

#[derive(Debug)]
pub(crate) struct NormalizationCase {
    pub(crate) source: String,
    pub(crate) nfc: String,
    pub(crate) nfd: String,
    pub(crate) nfkc: String,
    pub(crate) nfkd: String,
    pub(crate) label: String,
}

/// Load every test row from the vendored UCD
/// `NormalizationTest.txt`. Comment lines (`#`), section markers
/// (`@`), and trailing `#` annotations are stripped; each remaining
/// row is parsed into a [`NormalizationCase`] with five decoded
/// columns.
pub(crate) fn load_normalization_test() -> Vec<NormalizationCase> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/ucd/NormalizationTest.txt"
    );
    let text = std::fs::read_to_string(path)
        .expect("NormalizationTest.txt must be vendored");
    parse(&text)
}

fn parse(text: &str) -> Vec<NormalizationCase> {
    let mut cases = Vec::new();
    for raw in text.lines() {
        if raw.starts_with('@') || raw.starts_with('#') || raw.is_empty() {
            continue;
        }
        let (data, label) = match raw.find('#') {
            Some(pos) => (&raw[..pos], raw[pos + 1..].trim().to_owned()),
            None => (raw, String::new()),
        };
        let cols: Vec<&str> = data.split(';').collect();
        if cols.len() < 5 {
            continue;
        }
        cases.push(NormalizationCase {
            source: decode_column(cols[0]),
            nfc: decode_column(cols[1]),
            nfd: decode_column(cols[2]),
            nfkc: decode_column(cols[3]),
            nfkd: decode_column(cols[4]),
            label,
        });
    }
    cases
}

fn decode_column(col: &str) -> String {
    col.split_whitespace()
        .map(|hex| {
            let cp = u32::from_str_radix(hex, 16).expect("hex codepoint");
            char::from_u32(cp).expect("valid Unicode scalar")
        })
        .collect()
}

// ---- UAX #9 `BidiCharacterTest.txt` (R51.24.1) ----

/// Paragraph direction selector from Field 1 of `BidiCharacterTest.txt`.
///
/// UCD encodes the three modes as `0` / `1` / `2`. The enum carries
/// the same discriminants so a caller can `as u8` cast back when
/// debugging a mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BidiParagraphDirectionInput {
    /// `0` — force LTR (skip P2/P3 first-strong scan).
    Ltr = 0,
    /// `1` — force RTL.
    Rtl = 1,
    /// `2` — auto, equivalent to UAX #9 P2/P3 first-strong.
    Auto = 2,
}

/// One row of `BidiCharacterTest.txt`. Comments are stripped; the
/// `line_number` field is preserved so a conformance-harness mismatch
/// can be quoted back at the UCD coordinate that produced it.
#[derive(Debug)]
pub(crate) struct BidiCharacterCase {
    /// 1-based line number in the vendored UCD file (for diagnostics).
    pub(crate) line_number: usize,
    /// Field 0 decoded — the paragraph as a `Vec<char>` (codepoints
    /// kept in storage order, including format-control and BN).
    pub(crate) codepoints: Vec<char>,
    /// Field 1 — paragraph direction input (LTR / RTL / Auto).
    pub(crate) paragraph_direction_input: BidiParagraphDirectionInput,
    /// Field 2 — resolved paragraph embedding level (0 or 1).
    pub(crate) resolved_paragraph_level: u8,
    /// Field 3 — per-codepoint resolved level; `None` marks an `'x'`
    /// position (X9-removed: RLE / LRE / RLO / LRO / PDF / BN).
    pub(crate) resolved_levels: Vec<Option<u8>>,
    /// Field 4 — visual-order indices, X9-removed positions skipped.
    pub(crate) visual_indices: Vec<usize>,
}

/// Load every test row from the vendored UCD `BidiCharacterTest.txt`.
///
/// Comment lines (`#`) and blank lines are skipped. The full UCD 16.0
/// vector set (~96 K rows) is decoded into memory in one pass — the
/// conformance harness then filters the subset it wants to exercise.
pub(crate) fn load_bidi_character_test() -> Vec<BidiCharacterCase> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/ucd/BidiCharacterTest.txt"
    );
    let text = std::fs::read_to_string(path)
        .expect("BidiCharacterTest.txt must be vendored");
    parse_bidi_character_test(&text)
}

fn parse_bidi_character_test(text: &str) -> Vec<BidiCharacterCase> {
    let mut cases = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        if raw.starts_with('#') || raw.is_empty() {
            continue;
        }
        let cols: Vec<&str> = raw.split(';').collect();
        assert!(
            cols.len() >= 5,
            "BidiCharacterTest.txt:{}: expected 5 ';'-separated fields, \
             got {}: {raw:?}",
            idx + 1,
            cols.len(),
        );
        let codepoints: Vec<char> = cols[0]
            .split_whitespace()
            .map(|hex| {
                let cp = u32::from_str_radix(hex, 16)
                    .expect("BidiCharacterTest field 0: hex codepoint");
                char::from_u32(cp)
                    .expect("BidiCharacterTest field 0: valid Unicode scalar")
            })
            .collect();
        let dir_input = match cols[1].trim() {
            "0" => BidiParagraphDirectionInput::Ltr,
            "1" => BidiParagraphDirectionInput::Rtl,
            "2" => BidiParagraphDirectionInput::Auto,
            other => panic!(
                "BidiCharacterTest.txt:{}: paragraph direction must be \
                 0/1/2, got {other:?}",
                idx + 1,
            ),
        };
        let resolved_paragraph_level: u8 = cols[2]
            .trim()
            .parse()
            .expect("BidiCharacterTest field 2: resolved paragraph level u8");
        let resolved_levels: Vec<Option<u8>> = cols[3]
            .split_whitespace()
            .map(|tok| {
                if tok == "x" {
                    None
                } else {
                    Some(
                        tok.parse::<u8>().expect(
                            "BidiCharacterTest field 3: level u8 or 'x'",
                        ),
                    )
                }
            })
            .collect();
        let visual_indices: Vec<usize> = cols[4]
            .split_whitespace()
            .map(|tok| {
                tok.parse::<usize>()
                    .expect("BidiCharacterTest field 4: visual index usize")
            })
            .collect();
        assert_eq!(
            codepoints.len(),
            resolved_levels.len(),
            "BidiCharacterTest.txt:{}: field 0 / field 3 length mismatch",
            idx + 1,
        );
        cases.push(BidiCharacterCase {
            line_number: idx + 1,
            codepoints,
            paragraph_direction_input: dir_input,
            resolved_paragraph_level,
            resolved_levels,
            visual_indices,
        });
    }
    cases
}
