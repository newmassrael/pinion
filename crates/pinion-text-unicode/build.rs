//! R50.2.2 §5.37.3 — UCD 16.0.0 table codegen.
//!
//! Parses the three vendored UCD source files
//! (`ucd/UnicodeData.txt`, `ucd/DerivedNormalizationProps.txt`,
//! `ucd/CompositionExclusions.txt`) and emits the five
//! normalization tables to `$OUT_DIR/tables.rs`:
//!
//! 1. `CANONICAL_DECOMPOSITION` — sorted `&[(u32, &[u32])]` of
//!    canonical decomposition mappings (no `<tag>` prefix).
//! 2. `COMPATIBILITY_DECOMPOSITION` — sorted `&[(u32, &[u32])]` of
//!    all decomposition mappings (canonical + compatibility, tag
//!    stripped).
//! 3. `CANONICAL_COMBINING_CLASS` — sorted `&[(u32, u8)]` of
//!    non-zero CCC entries only (CCC == 0 is the default).
//! 4. `FULL_COMPOSITION_EXCLUSION` — sorted `&[u32]` from the
//!    `Full_Composition_Exclusion` derived property (already
//!    includes singleton + non-starter + script-specific
//!    exclusions per UAX #44).
//! 5. `PRIMARY_COMPOSITES` — derived from canonical decompositions
//!    of length 2 whose composed codepoint is not in
//!    `FULL_COMPOSITION_EXCLUSION`; sorted by `(a, b)` key for
//!    binary-search lookup during NFC composition.
//!
//! External dependencies: zero. `std` only — no parser combinators,
//! no `phf`, no `lazy_static`.

use std::collections::HashSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Parsed UCD tables produced from `UnicodeData.txt`. Carries three
/// sorted vectors that share a single parse pass.
struct UnicodeDataTables {
    canonical_decomposition: Vec<(u32, Vec<u32>)>,
    compatibility_decomposition: Vec<(u32, Vec<u32>)>,
    canonical_combining_class: Vec<(u32, u8)>,
}

#[allow(
    clippy::similar_names,
    reason = "nfc_qc / nfd_qc_no / nfkc_qc / nfkd_qc_no mirror the UCD property names by design"
)]
fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo");
    let ucd_dir = Path::new(&manifest_dir).join("ucd");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ucd/UnicodeData.txt");
    println!("cargo:rerun-if-changed=ucd/DerivedNormalizationProps.txt");

    let unicode_data =
        fs::read_to_string(ucd_dir.join("UnicodeData.txt"))
            .expect("ucd/UnicodeData.txt must be vendored");
    let derived = fs::read_to_string(
        ucd_dir.join("DerivedNormalizationProps.txt"),
    )
    .expect("ucd/DerivedNormalizationProps.txt must be vendored");

    let parsed = parse_unicode_data(&unicode_data);
    let exclusions = parse_full_composition_exclusion(&derived);
    let primary_composites =
        derive_primary_composites(&parsed.canonical_decomposition, &exclusions);

    // Quick-check tables (UAX #15 §5). NFD/NFKD only carry "N"
    // entries (decomposed forms are definite); NFC/NFKC also carry
    // "M" (Maybe) for marks that may compose depending on context.
    let nfc_qc = parse_quick_check(&derived, "NFC_QC");
    let nfd_qc_no = parse_quick_check_no_only(&derived, "NFD_QC");
    let nfkc_qc = parse_quick_check(&derived, "NFKC_QC");
    let nfkd_qc_no = parse_quick_check_no_only(&derived, "NFKD_QC");

    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR must be set by cargo");
    let out_path = Path::new(&out_dir).join("tables.rs");
    emit_tables(
        &out_path,
        &parsed,
        &exclusions,
        &primary_composites,
        &nfc_qc,
        &nfd_qc_no,
        &nfkc_qc,
        &nfkd_qc_no,
    );
}

/// Quick-check ternary value packed into a `u8` for the emitted
/// table. `1 = No`, `2 = Maybe`. `Yes` is the default and absent.
const QC_NO: u8 = 1;
const QC_MAYBE: u8 = 2;

/// Parse `Property; N` or `Property; M` lines from
/// `DerivedNormalizationProps.txt` (for `NFC_QC` / `NFKC_QC`).
/// Returns a sorted vec of `(codepoint, qc_value)` pairs.
fn parse_quick_check(text: &str, prop_name: &str) -> Vec<(u32, u8)> {
    let mut out: Vec<(u32, u8)> = Vec::new();
    for raw in text.lines() {
        let line = match raw.find('#') {
            Some(pos) => &raw[..pos],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() < 3 {
            continue;
        }
        if parts[1].trim() != prop_name {
            continue;
        }
        let qc = match parts[2].trim() {
            "N" => QC_NO,
            "M" => QC_MAYBE,
            _ => continue,
        };
        let range = parts[0].trim();
        if let Some(dot) = range.find("..") {
            let start =
                u32::from_str_radix(&range[..dot], 16).expect("range start");
            let end =
                u32::from_str_radix(&range[dot + 2..], 16).expect("range end");
            for cp in start..=end {
                out.push((cp, qc));
            }
        } else {
            out.push((
                u32::from_str_radix(range, 16).expect("single codepoint"),
                qc,
            ));
        }
    }
    out.sort_by_key(|(cp, _)| *cp);
    out
}

/// Same as [`parse_quick_check`] but for `NFD_QC` / `NFKD_QC`
/// which have only `N` entries (decomposed forms are definite).
/// Returns a sorted vec of codepoints.
fn parse_quick_check_no_only(text: &str, prop_name: &str) -> Vec<u32> {
    parse_quick_check(text, prop_name)
        .into_iter()
        .map(|(cp, _)| cp)
        .collect()
}

/// Parse `UnicodeData.txt` into three sorted vectors: canonical
/// decompositions, all decompositions (compatibility-broad), and
/// non-zero canonical combining classes.
fn parse_unicode_data(text: &str) -> UnicodeDataTables {
    let mut canonical: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut compatibility: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut ccc: Vec<(u32, u8)> = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 6 {
            continue;
        }
        let cp = u32::from_str_radix(fields[0], 16).unwrap_or_else(|_| {
            panic!("invalid hex codepoint: {}", fields[0])
        });
        let ccc_value: u8 = fields[3]
            .parse()
            .unwrap_or_else(|_| panic!("invalid CCC: {}", fields[3]));
        if ccc_value != 0 {
            ccc.push((cp, ccc_value));
        }
        let decomp_raw = fields[5].trim();
        if decomp_raw.is_empty() {
            continue;
        }
        let (is_canonical, hex_seq) = strip_decomposition_tag(decomp_raw);
        let decomp: Vec<u32> = hex_seq
            .split_whitespace()
            .map(|h| {
                u32::from_str_radix(h, 16).unwrap_or_else(|_| {
                    panic!("invalid decomp codepoint: {h}")
                })
            })
            .collect();
        if is_canonical {
            canonical.push((cp, decomp.clone()));
        }
        compatibility.push((cp, decomp));
    }

    canonical.sort_by_key(|(cp, _)| *cp);
    compatibility.sort_by_key(|(cp, _)| *cp);
    ccc.sort_by_key(|(cp, _)| *cp);

    UnicodeDataTables {
        canonical_decomposition: canonical,
        compatibility_decomposition: compatibility,
        canonical_combining_class: ccc,
    }
}

/// Strip a leading `<tag>` from a decomposition mapping value. Returns
/// `(is_canonical, hex_sequence)` where `is_canonical` is `true` iff no
/// tag was present (UAX #44 §5.7.3).
fn strip_decomposition_tag(value: &str) -> (bool, &str) {
    if let Some(rest) = value.strip_prefix('<') {
        if let Some(close) = rest.find('>') {
            let after = rest[close + 1..].trim_start();
            return (false, after);
        }
    }
    (true, value)
}

/// Parse `DerivedNormalizationProps.txt` and return the sorted set of
/// codepoints with `Full_Composition_Exclusion`. Range syntax
/// (`xxxx..yyyy`) is expanded.
fn parse_full_composition_exclusion(text: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for raw in text.lines() {
        let line = match raw.find('#') {
            Some(pos) => &raw[..pos],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() < 2 {
            continue;
        }
        if parts[1].trim() != "Full_Composition_Exclusion" {
            continue;
        }
        let range = parts[0].trim();
        if let Some(dot) = range.find("..") {
            let start = u32::from_str_radix(&range[..dot], 16)
                .expect("invalid range start");
            let end = u32::from_str_radix(&range[dot + 2..], 16)
                .expect("invalid range end");
            for cp in start..=end {
                out.push(cp);
            }
        } else {
            out.push(
                u32::from_str_radix(range, 16)
                    .expect("invalid single codepoint"),
            );
        }
    }
    out.sort_unstable();
    out
}

/// Derive `(a, b) → c` primary composites from canonical decompositions
/// of length 2 whose composed codepoint is not excluded. The
/// `Full_Composition_Exclusion` property already covers singleton and
/// non-starter decompositions per UAX #44, so length and exclusion
/// checks are sufficient.
fn derive_primary_composites(
    canonical: &[(u32, Vec<u32>)],
    exclusions: &[u32],
) -> Vec<((u32, u32), u32)> {
    let excluded: HashSet<u32> = exclusions.iter().copied().collect();
    let mut out: Vec<((u32, u32), u32)> = canonical
        .iter()
        .filter_map(|(c, decomp)| {
            if decomp.len() == 2 && !excluded.contains(c) {
                Some(((decomp[0], decomp[1]), *c))
            } else {
                None
            }
        })
        .collect();
    out.sort_by_key(|(key, _)| *key);
    out
}

#[allow(
    clippy::too_many_arguments,
    clippy::similar_names,
    reason = "single-pass UCD-derived table emit — parallel UCD property names by design; struct wrap adds boilerplate without information value"
)]
fn emit_tables(
    out_path: &Path,
    parsed: &UnicodeDataTables,
    exclusions: &[u32],
    primary_composites: &[((u32, u32), u32)],
    nfc_qc: &[(u32, u8)],
    nfd_qc_no: &[u32],
    nfkc_qc: &[(u32, u8)],
    nfkd_qc_no: &[u32],
) {
    let mut s = String::new();
    s.push_str("// Auto-generated by build.rs from UCD 16.0.0.\n");
    s.push_str("// DO NOT EDIT — regenerated on every build.\n\n");

    s.push_str(
        "/// UCD version pin (UAX #44 — Hyrum's Law deterministic).\n",
    );
    s.push_str("pub static UCD_VERSION: &str = \"16.0.0\";\n\n");

    emit_fast_path_anchors(
        &mut s,
        parsed,
        exclusions,
        primary_composites,
        nfc_qc,
        nfd_qc_no,
        nfkc_qc,
        nfkd_qc_no,
    );

    emit_decomp_table(
        &mut s,
        "CANONICAL_DECOMPOSITION",
        "Canonical decompositions (UAX #44 §5.7.3, no `<tag>` prefix).",
        &parsed.canonical_decomposition,
    );
    emit_decomp_table(
        &mut s,
        "COMPATIBILITY_DECOMPOSITION",
        "All decompositions (canonical + compatibility, tag stripped).",
        &parsed.compatibility_decomposition,
    );
    emit_ccc_table(&mut s, &parsed.canonical_combining_class);
    emit_exclusion_table(&mut s, exclusions);
    emit_primary_composites_table(&mut s, primary_composites);
    emit_qc_ynm_table(
        &mut s,
        "NFC_QC_NON_YES",
        "NFC `Quick_Check` non-Yes entries (`1 = No`, `2 = Maybe`). \
         Default `Yes` is absent; consult \
         `crate::quick_check::nfc_quick_check`.",
        nfc_qc,
    );
    emit_qc_no_table(
        &mut s,
        "NFD_QC_NO",
        "NFD `Quick_Check` `No` entries (NFD admits no `Maybe`). \
         Default `Yes` is absent.",
        nfd_qc_no,
    );
    emit_qc_ynm_table(
        &mut s,
        "NFKC_QC_NON_YES",
        "NFKC `Quick_Check` non-Yes entries (`1 = No`, `2 = Maybe`).",
        nfkc_qc,
    );
    emit_qc_no_table(
        &mut s,
        "NFKD_QC_NO",
        "NFKD `Quick_Check` `No` entries.",
        nfkd_qc_no,
    );

    fs::write(out_path, s).expect("failed to write tables.rs");
}

/// Emit the R50.2.9 short-circuit codepoint anchors. Each constant
/// is the smallest (or for `PRIMARY_COMPOSITES_LAST_A_CP`, largest)
/// codepoint with a non-trivial entry in the corresponding table,
/// derived automatically from the already-sorted vectors. Codepoints
/// outside the anchor range are guaranteed by UAX #44 §3 stability
/// policy to remain absent from these tables across Unicode
/// versions, so the short-circuit branches added in `quick_check.rs`,
/// `ordering.rs`, `decompose.rs`, and `composition.rs` skip the
/// `binary_search` entirely on ASCII / Latin-1 input.
#[allow(
    clippy::too_many_arguments,
    clippy::similar_names,
    reason = "anchor const block parallels the UCD-derived table inputs by design"
)]
fn emit_fast_path_anchors(
    s: &mut String,
    parsed: &UnicodeDataTables,
    exclusions: &[u32],
    primary_composites: &[((u32, u32), u32)],
    nfc_qc: &[(u32, u8)],
    nfd_qc_no: &[u32],
    nfkc_qc: &[(u32, u8)],
    nfkd_qc_no: &[u32],
) {
    let canonical_first = parsed.canonical_decomposition[0].0;
    let compatibility_first = parsed.compatibility_decomposition[0].0;
    let ccc_first = parsed.canonical_combining_class[0].0;
    let exclusion_first = exclusions[0];
    let primary_first_a = primary_composites[0].0 .0;
    let primary_last_a = primary_composites
        .iter()
        .map(|((a, _), _)| *a)
        .max()
        .expect("primary composite table is non-empty");
    let nfc_qc_first = nfc_qc[0].0;
    let nfd_qc_first = nfd_qc_no[0];
    let nfkc_qc_first = nfkc_qc[0].0;
    let nfkd_qc_first = nfkd_qc_no[0];

    s.push_str(
        "// R50.2.9 §5.37.3 fast-path codepoint anchors — UCD-derived\n\
         // smallest codepoint with a non-trivial table entry. UAX #44\n\
         // §3 stability policy forbids back-porting entries below an\n\
         // existing anchor, so these constants are forward-stable.\n\n",
    );
    writeln!(
        s,
        "/// Smallest codepoint in `CANONICAL_DECOMPOSITION` \
         (UCD-derived).\n\
         pub(crate) const CANONICAL_DECOMPOSITION_FIRST_CP: u32 = \
         0x{canonical_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `COMPATIBILITY_DECOMPOSITION` \
         (UCD-derived).\n\
         pub(crate) const COMPATIBILITY_DECOMPOSITION_FIRST_CP: u32 = \
         0x{compatibility_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint with non-zero \
         `Canonical_Combining_Class`.\n\
         pub(crate) const CANONICAL_COMBINING_CLASS_FIRST_CP: u32 = \
         0x{ccc_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `FULL_COMPOSITION_EXCLUSION`.\n\
         pub(crate) const FULL_COMPOSITION_EXCLUSION_FIRST_CP: u32 = \
         0x{exclusion_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest `a` codepoint appearing as the first half of a\n\
         /// `PRIMARY_COMPOSITES` `(a, b)` key.\n\
         pub(crate) const PRIMARY_COMPOSITES_FIRST_A_CP: u32 = \
         0x{primary_first_a:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Largest `a` codepoint appearing as the first half of a\n\
         /// `PRIMARY_COMPOSITES` `(a, b)` key. Composition probes\n\
         /// with `a > LAST_A_CP` short-circuit to the algorithmic\n\
         /// Hangul branch without a table search.\n\
         pub(crate) const PRIMARY_COMPOSITES_LAST_A_CP: u32 = \
         0x{primary_last_a:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `NFC_QC_NON_YES`.\n\
         pub(crate) const NFC_QC_FIRST_NON_YES_CP: u32 = \
         0x{nfc_qc_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `NFD_QC_NO`.\n\
         pub(crate) const NFD_QC_FIRST_NO_CP: u32 = \
         0x{nfd_qc_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `NFKC_QC_NON_YES`.\n\
         pub(crate) const NFKC_QC_FIRST_NON_YES_CP: u32 = \
         0x{nfkc_qc_first:04X};"
    )
    .expect("String write infallible");
    writeln!(
        s,
        "/// Smallest codepoint in `NFKD_QC_NO`.\n\
         pub(crate) const NFKD_QC_FIRST_NO_CP: u32 = \
         0x{nfkd_qc_first:04X};\n"
    )
    .expect("String write infallible");
}

fn emit_decomp_table(
    s: &mut String,
    name: &str,
    doc: &str,
    table: &[(u32, Vec<u32>)],
) {
    s.push_str("/// ");
    s.push_str(doc);
    s.push_str(
        "\n///\n/// Sorted by codepoint; use `binary_search_by_key`.\n",
    );
    writeln!(s, "pub(crate) static {name}: &[(u32, &[u32])] = &[")
        .expect("String write infallible");
    for (cp, decomp) in table {
        write!(s, "    (0x{cp:04X}, &[").expect("String write infallible");
        for (i, dc) in decomp.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            write!(s, "0x{dc:04X}").expect("String write infallible");
        }
        s.push_str("]),\n");
    }
    s.push_str("];\n\n");
}

fn emit_ccc_table(s: &mut String, ccc: &[(u32, u8)]) {
    emit_u8_bmp_trie_table(
        s,
        "CANONICAL_COMBINING_CLASS",
        "Non-zero Canonical Combining Class values (UAX #44).",
        ccc,
    );
}

/// Emit a 2-stage BMP trie (Stage 1 `u16[256]` + deduplicated
/// Stage 2 packed `u8` blocks) plus a sorted supplementary fallback
/// for codepoints at or above `U+10000`. Shared by CCC (R50.2.10)
/// and the four Quick-check tables (R50.2.11).
fn emit_u8_bmp_trie_table(
    s: &mut String,
    name: &str,
    purpose: &str,
    entries: &[(u32, u8)],
) {
    let (bmp_index, bmp_data) = build_u8_bmp_trie(entries);
    let supplementary: Vec<(u32, u8)> = entries
        .iter()
        .copied()
        .filter(|(cp, _)| *cp >= 0x10000)
        .collect();

    writeln!(
        s,
        "/// {purpose}\n///\n/// R50.2.10/R50.2.11 \u{2014} BMP lookup via 2-stage trie\n/// \
         (`Stage1[c >> 8]` selects a Stage-2 block;\n/// \
         `Stage2[block * 256 + (c & 0xFF)]` returns the packed value).\n/// \
         Identical 256-byte blocks share a single Stage-2 entry, so the\n/// \
         data table is dramatically smaller than the 64 KiB worst case.\n/// \
         Supplementary-plane codepoints (`cp >= 0x10000`) use the sorted\n/// \
         `_SUPPLEMENTARY` sibling array.",
    )
    .expect("String write infallible");
    writeln!(s, "pub(crate) static {name}_BMP_INDEX: &[u16; 256] = &[")
        .expect("String write infallible");
    emit_packed_row(s, &bmp_index);
    s.push_str("];\n\n");

    writeln!(
        s,
        "/// Packed value blocks indexed by `{name}_BMP_INDEX`.",
    )
    .expect("String write infallible");
    writeln!(s, "pub(crate) static {name}_BMP_DATA: &[u8] = &[")
        .expect("String write infallible");
    emit_packed_row(s, &bmp_data);
    s.push_str("];\n\n");

    writeln!(
        s,
        "/// Supplementary-plane (`cp >= 0x10000`) entries for \
         `{name}`\n/// \
         (sparse, sorted by codepoint; served by `binary_search`).",
    )
    .expect("String write infallible");
    writeln!(
        s,
        "pub(crate) static {name}_SUPPLEMENTARY: &[(u32, u8)] = &[",
    )
    .expect("String write infallible");
    for (cp, value) in &supplementary {
        writeln!(s, "    (0x{cp:04X}, {value}),")
            .expect("String write infallible");
    }
    s.push_str("];\n\n");
}

fn emit_packed_row<T: std::fmt::Display>(s: &mut String, items: &[T]) {
    for (i, value) in items.iter().enumerate() {
        if i % 16 == 0 {
            s.push_str("    ");
        }
        write!(s, "{value}, ").expect("String write infallible");
        if i % 16 == 15 {
            s.push('\n');
        }
    }
    if !items.is_empty() && items.len() % 16 != 0 {
        s.push('\n');
    }
}

/// R50.2.10/R50.2.11 — build the BMP 2-stage trie shared by CCC and
/// the four Quick-check tables. Returns `(stage1, stage2)` where
/// `stage1` has exactly 256 entries (one per high byte) and `stage2`
/// is the deduplicated 256-byte block table. Block 0 of Stage 2 is
/// the all-zero "null" block, shared by every high byte whose 256
/// codepoint slice carries only default values. Inputs with codepoint
/// `>= 0x10000` are filtered out — supplementary entries land in a
/// separate sorted array.
fn build_u8_bmp_trie(entries: &[(u32, u8)]) -> (Vec<u16>, Vec<u8>) {
    const BLOCK_SIZE: usize = 256;
    const NUM_BLOCKS: usize = 0x10000 / BLOCK_SIZE; // 256

    let mut blocks: Vec<[u8; BLOCK_SIZE]> = vec![[0; BLOCK_SIZE]; NUM_BLOCKS];
    for &(cp, value) in entries {
        if cp >= 0x10000 {
            continue;
        }
        let block = (cp as usize) / BLOCK_SIZE;
        let offset = (cp as usize) % BLOCK_SIZE;
        blocks[block][offset] = value;
    }

    let null_block = [0_u8; BLOCK_SIZE];
    let mut stage1: Vec<u16> = vec![0; NUM_BLOCKS];
    let mut stage2: Vec<u8> = null_block.to_vec();
    let mut block_map: std::collections::HashMap<[u8; BLOCK_SIZE], u16> =
        std::collections::HashMap::new();
    block_map.insert(null_block, 0);

    for (i, blk) in blocks.iter().enumerate() {
        if let Some(&existing) = block_map.get(blk) {
            stage1[i] = existing;
        } else {
            let idx = u16::try_from(stage2.len() / BLOCK_SIZE)
                .expect("BMP u8 trie block index must fit in u16");
            stage2.extend_from_slice(blk);
            block_map.insert(*blk, idx);
            stage1[i] = idx;
        }
    }
    (stage1, stage2)
}

fn emit_exclusion_table(s: &mut String, exclusions: &[u32]) {
    s.push_str(
        "/// `Full_Composition_Exclusion` from \
         `DerivedNormalizationProps.txt` (UAX #44).\n///\n/// \
         Sorted ascending; `binary_search` for membership.\n",
    );
    s.push_str("pub(crate) static FULL_COMPOSITION_EXCLUSION: &[u32] = &[\n");
    for cp in exclusions {
        writeln!(s, "    0x{cp:04X},").expect("String write infallible");
    }
    s.push_str("];\n\n");
}

fn emit_primary_composites_table(
    s: &mut String,
    primary_composites: &[((u32, u32), u32)],
) {
    s.push_str(
        "/// Primary composite map for NFC: `(a, b) → c` where `c` has \
         canonical decomposition `[a, b]` and is not in \
         `FULL_COMPOSITION_EXCLUSION` (UAX #15 §1.3, D5).\n///\n/// \
         Sorted by `(a, b)`; `binary_search` for adjacent-pair lookup \
         during canonical composition.\n",
    );
    s.push_str(
        "pub(crate) static PRIMARY_COMPOSITES: &[((u32, u32), u32)] = &[\n",
    );
    for ((a, b), c) in primary_composites {
        writeln!(s, "    ((0x{a:04X}, 0x{b:04X}), 0x{c:04X}),")
            .expect("String write infallible");
    }
    s.push_str("];\n\n");
}

fn emit_qc_ynm_table(
    s: &mut String,
    name: &str,
    doc: &str,
    table: &[(u32, u8)],
) {
    emit_u8_bmp_trie_table(s, name, doc, table);
}

/// Promote a `Yes`/`No`-only QC table (NFD/NFKD) to the shared
/// `(u32, u8)` shape used by [`emit_u8_bmp_trie_table`], with value
/// `1` standing in for "member" (the lookup just tests non-zero).
fn emit_qc_no_table(s: &mut String, name: &str, doc: &str, table: &[u32]) {
    let promoted: Vec<(u32, u8)> =
        table.iter().copied().map(|cp| (cp, 1)).collect();
    emit_u8_bmp_trie_table(s, name, doc, &promoted);
}
