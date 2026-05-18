//! `pinion-text-unicode` UAX #15 NFC normalization micro-benchmarks.
//!
//! Five scenarios cover the canonical performance envelopes of the
//! [`normalize`] entry point so R50.2.7's debt-repayment claims
//! (Quick-check fast path, `O(n)` compose-write, `Cow<str>` zero-copy
//! return) become quantitatively measurable against any future
//! optimisation slice (e.g. R50.2.9 two-stage table acceleration).
//!
//! ## Scenarios
//!
//! 1. `ascii_fast_path` — pure ASCII text. UAX #15 §5 Quick-check
//!    returns `Yes` for every codepoint, so [`normalize`] returns
//!    `Cow::Borrowed` with no allocation. Establishes the lower
//!    bound for the Quick-check pass itself.
//! 2. `precomposed_nfc` — Latin-1 Supplement / Latin Extended
//!    multilingual text already in NFC form. Exercises Quick-check
//!    over non-ASCII codepoints (each lookup is a `binary_search`
//!    into the `NFC_QC` table); no decomposition runs.
//! 3. `decomposed_recompose` — sequences of base + combining mark
//!    (e.g. `A` + `U+0300`) where Quick-check returns `No`/`Maybe`,
//!    forcing the full pipeline: decompose → canonical-order →
//!    compose. Measures the recomposition hot path.
//! 4. `hangul_jamo_compose` — Hangul L+V jamo pairs that compose
//!    algorithmically per UAX #15 §16 (no UCD table involvement).
//!    Isolates the algorithmic Hangul branch from binary-search cost.
//! 5. `normalization_test_sample` — first 1024 rows of the vendored
//!    UCD `NormalizationTest.txt`. Mixed real-world input drawn from
//!    the conformance fixture itself; tracks aggregate throughput
//!    on the same corpus used for correctness validation.
//!
//! The bench binary owns its own `NormalizationTest.txt` parser
//! (~30 LOC) instead of importing `crate::test_fixture` (which is
//! `#[cfg(test)]`-gated). Keeping measurement and verification in
//! separate compilation units honours the criterion convention of
//! bench targets as standalone binaries; the duplicated parse cost
//! is one-shot at criterion startup.
//!
//! [`normalize`]: pinion_text_unicode::normalize

use std::path::PathBuf;

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion,
    Throughput,
};
use pinion_text_unicode::{normalize, NormForm};

fn ascii_fast_path(c: &mut Criterion) {
    // ~990 bytes of pangram ASCII — well above L1 cache line yet small
    // enough that criterion's iter loop stays in single-digit ns/byte.
    let input: String =
        "The quick brown fox jumps over the lazy dog. ".repeat(22);
    let mut group = c.benchmark_group("ascii_fast_path");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(BenchmarkId::from_parameter("nfc"), |b| {
        b.iter(|| normalize(black_box(&input), NormForm::Nfc));
    });
    group.finish();
}

fn precomposed_nfc(c: &mut Criterion) {
    // Latin-1 + Extended already in NFC. Each non-ASCII codepoint
    // costs one NFC_QC binary_search; nothing allocates.
    let input: String = concat!(
        "caf\u{00E9} r\u{00E9}sum\u{00E9} na\u{00EF}ve fa\u{00E7}ade ",
        "jalape\u{00F1}o co\u{00F6}perate \u{00E0} la mode ",
    )
    .repeat(22);
    let mut group = c.benchmark_group("precomposed_nfc");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(BenchmarkId::from_parameter("nfc"), |b| {
        b.iter(|| normalize(black_box(&input), NormForm::Nfc));
    });
    group.finish();
}

fn decomposed_recompose(c: &mut Criterion) {
    // Base + combining mark sequences. Quick-check yields No/Maybe
    // (combining marks classified as "Maybe" composers); the full
    // pipeline runs: decompose → canonical_ordering → composition.
    let input: String = concat!(
        "A\u{0300}E\u{0301}I\u{0302}O\u{0303}U\u{0308}",
        "a\u{0300}e\u{0301}i\u{0302}o\u{0303}u\u{0308} ",
    )
    .repeat(40);
    let mut group = c.benchmark_group("decomposed_recompose");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(BenchmarkId::from_parameter("nfc"), |b| {
        b.iter(|| normalize(black_box(&input), NormForm::Nfc));
    });
    group.finish();
}

fn hangul_jamo_compose(c: &mut Criterion) {
    // L+V jamo pairs (U+1100..U+1112 + U+1161..U+1175). UAX #15 §16
    // algorithmic composition path — no UCD binary_search per char,
    // only the §16 arithmetic. Quick-check returns No for conjoining
    // jamo, so the compose path runs.
    let input: String = concat!(
        "\u{1100}\u{1161}\u{1102}\u{1163}\u{1103}\u{1165}",
        "\u{1106}\u{1167}\u{1107}\u{1169}\u{1109}\u{116B}",
    )
    .repeat(40);
    let mut group = c.benchmark_group("hangul_jamo_compose");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(BenchmarkId::from_parameter("nfc"), |b| {
        b.iter(|| normalize(black_box(&input), NormForm::Nfc));
    });
    group.finish();
}

fn normalization_test_sample(c: &mut Criterion) {
    let cases = load_sample(1024);
    let total_bytes: usize = cases.iter().map(String::len).sum();
    let mut group = c.benchmark_group("normalization_test_sample");
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.bench_function(BenchmarkId::from_parameter("nfc"), |b| {
        b.iter(|| {
            for src in &cases {
                let _ = normalize(black_box(src), NormForm::Nfc);
            }
        });
    });
    group.finish();
}

/// Load up to `limit` source-column strings from the vendored
/// `NormalizationTest.txt`, mirroring the parser in
/// `crate::test_fixture` but standalone for the bench binary.
fn load_sample(limit: usize) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ucd")
        .join("NormalizationTest.txt");
    let text = std::fs::read_to_string(&path)
        .expect("NormalizationTest.txt must be vendored at ucd/");
    let mut out = Vec::with_capacity(limit);
    for raw in text.lines() {
        if out.len() >= limit {
            break;
        }
        if raw.starts_with('@') || raw.starts_with('#') || raw.is_empty() {
            continue;
        }
        let data = raw.split_once('#').map_or(raw, |(d, _)| d);
        let Some(col) = data.split(';').next() else {
            continue;
        };
        let decoded: String = col
            .split_whitespace()
            .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
            .filter_map(char::from_u32)
            .collect();
        if !decoded.is_empty() {
            out.push(decoded);
        }
    }
    out
}

criterion_group!(
    benches,
    ascii_fast_path,
    precomposed_nfc,
    decomposed_recompose,
    hangul_jamo_compose,
    normalization_test_sample,
);
criterion_main!(benches);
