//! Shared `#[cfg(test)]` fixture loader for the UCD
//! `NormalizationTest.txt` conformance suite. Used by per-form
//! conformance sweeps (NFD in `nfd.rs`, NFC in `nfc.rs`, etc.).

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
