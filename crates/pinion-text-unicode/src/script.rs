//! R50.5 §5.37.5 — UAX #24 Script property + script-run segmentation.
//!
//! Intended as the shaping pipeline's run-itemisation input: which
//! writing system a codepoint belongs to ([`script`]), and how a string
//! splits into maximal same-script runs ([`script_runs`]) so the shaper
//! can pick the right per-script behaviour for each run.
//!
//! # Property
//!
//! [`script`] is a binary search over the code-generated `SCRIPT_RANGES`
//! table (the alphabetically-indexed `Script` discriminants emitted by
//! `build.rs` from `ucd/Scripts.txt`, UCD 16.0.0), sharing the crate-
//! internal `range_class_index` helper with `bidi_class` (§5.37.4) and
//! `line_break_class` (§5.37.7). A codepoint listed nowhere resolves to
//! [`Script::Unknown`] per the `@missing: 0000..10FFFF; Unknown`
//! directive.
//!
//! Unlike the small, closed `BidiClass` / `LineBreak` enums (hand-written
//! with a parallel index map), the ~170-value `Script` enum is
//! code-generated wholesale from the parsed name list — one source for
//! the variants, `from_index`, `ucd_name`, and the range indices, so the
//! N-arm skew the R1019 line-break lesson warns about is structurally
//! impossible.
//!
//! # Segmentation scope
//!
//! [`script_runs`] applies the UAX #24 *simplified* resolution:
//! `Common` and `Inherited` codepoints extend the current run by
//! adjacency, and a run breaks only at a different concrete script.
//! Deliberately NOT yet handled (honest deferral, not a silent gap):
//! `Script_Extensions` (`scx`) membership; the UAX #24 paired-bracket
//! `Common` resolution (matching brackets take the run's script); and
//! the interaction with BIDI run direction (§5.37.4) — segmentation here
//! is over logical order only. Production paint is still §5.36 swash;
//! [`script`] / [`script_runs`] are test forcing-consumers until the
//! paint pipeline wires the self-hosted shaper.

#[allow(
    clippy::unreadable_literal,
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::enum_variant_names,
    reason = "Code-generated Script enum + UCD range table; consumed by script()."
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/script_tables.rs"));
}

pub use generated::Script;

use crate::range::range_class_index;

/// The UAX #24 `Script` property of `c` (UCD 16.0.0). A codepoint with
/// no explicit assignment resolves to [`Script::Unknown`].
#[must_use]
pub fn script(c: char) -> Script {
    range_class_index(generated::SCRIPT_RANGES, c as u32)
        .map_or(Script::Unknown, Script::from_index)
}

/// A maximal run of text resolved to a single [`Script`], delimited by
/// byte offsets into the source `&str` (`start..end`, `end` exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptRun {
    /// The resolved script of every codepoint in the run.
    pub script: Script,
    /// Byte offset of the run's first codepoint.
    pub start: usize,
    /// Byte offset just past the run's last codepoint.
    pub end: usize,
}

/// Split `text` into maximal same-script runs in logical order (UAX #24
/// simplified resolution — see the module scope). `Common` and
/// `Inherited` codepoints extend the current run; a leading
/// `Common`/`Inherited` prefix is absorbed by the first concrete script,
/// and an all-`Common`/`Inherited` string yields one [`Script::Common`]
/// run. An empty string yields no runs.
#[must_use]
pub fn script_runs(text: &str) -> Vec<ScriptRun> {
    let mut runs: Vec<ScriptRun> = Vec::new();
    let mut run_start = 0usize;
    // `Common` doubles as the "unresolved" sentinel: it is the natural
    // default and the first concrete script overwrites it.
    let mut run_script = Script::Common;
    for (i, ch) in text.char_indices() {
        let s = script(ch);
        if s == Script::Common || s == Script::Inherited {
            // Always extends the current run; never a break.
            continue;
        }
        if run_script == Script::Common {
            // First concrete script resolves the run, absorbing any
            // leading Common/Inherited codepoints.
            run_script = s;
        } else if s != run_script {
            runs.push(ScriptRun {
                script: run_script,
                start: run_start,
                end: i,
            });
            run_start = i;
            run_script = s;
        }
    }
    if !text.is_empty() {
        runs.push(ScriptRun {
            script: run_script,
            start: run_start,
            end: text.len(),
        });
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{Script, ScriptRun, generated, script, script_runs};
    use std::collections::BTreeSet;

    /// Re-derive the alphabetically-sorted distinct script-name list
    /// (with `Unknown` always present) directly from the vendored UCD
    /// file — an independent reproduction of the `build.rs` contract so a
    /// skew between the generator and the compiled enum is caught.
    fn vendored_script_names() -> Vec<String> {
        let text = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/ucd/Scripts.txt"));
        let mut names: BTreeSet<String> = BTreeSet::new();
        names.insert("Unknown".to_string());
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split(';');
            let _range = parts.next();
            if let Some(name) = parts.next() {
                names.insert(name.trim().to_string());
            }
        }
        names.into_iter().collect()
    }

    #[test]
    fn from_index_and_ucd_name_cover_every_value_without_skew() {
        // Pins all ~170 generated arms: the discriminant order, the
        // `from_index` map, and the `ucd_name` map must all agree with
        // the independently-parsed alphabetical name list.
        let names = vendored_script_names();
        assert!(
            names.len() > 150,
            "unexpectedly few scripts: {}",
            names.len()
        );
        for (i, name) in names.iter().enumerate() {
            let idx = u8::try_from(i).expect("discriminant fits u8");
            let value = Script::from_index(idx);
            assert_eq!(value.ucd_name(), name, "ucd_name skew at index {i}");
        }
        // Distinctness of names (no two variants share a UCD name).
        let distinct: BTreeSet<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(distinct.len(), names.len(), "duplicate script name");
    }

    #[test]
    fn every_range_start_maps_to_its_script() {
        // Each `Scripts.txt` data row's start codepoint must look up to
        // the row's script — pins `SCRIPT_RANGES` against the source.
        let text = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/ucd/Scripts.txt"));
        let mut checked = 0usize;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split(';');
            let range = parts.next().unwrap_or("").trim();
            let Some(name) = parts.next().map(str::trim) else {
                continue;
            };
            let start_hex = range.split("..").next().unwrap_or(range);
            let cp = u32::from_str_radix(start_hex, 16).expect("hex start");
            let Some(ch) = char::from_u32(cp) else {
                continue; // surrogate gap — not a valid `char`
            };
            assert_eq!(script(ch).ucd_name(), name, "script({cp:#06X}) mismatch");
            checked += 1;
        }
        assert!(checked > 2000, "too few range rows checked: {checked}");
    }

    #[test]
    fn known_codepoint_assignments() {
        // Ground-truth anchors independent of the sort contract.
        assert_eq!(script('A').ucd_name(), "Latin"); // U+0041
        assert_eq!(script('0').ucd_name(), "Common"); // U+0030
        assert_eq!(script('\u{0300}').ucd_name(), "Inherited"); // combining grave
        assert_eq!(script('\u{05D0}').ucd_name(), "Hebrew"); // alef
        assert_eq!(script('\u{0E01}').ucd_name(), "Thai"); // ko kai
        assert_eq!(script('\u{AC00}').ucd_name(), "Hangul"); // syllable ga
        assert_eq!(script('\u{4E00}').ucd_name(), "Han"); // CJK ideograph
        assert_eq!(script('A'), Script::Latin);
        assert_eq!(script('0'), Script::Common);
    }

    #[test]
    fn unassigned_codepoint_defaults_to_unknown() {
        // U+10FFFF is a permanent noncharacter — never given a script.
        assert_eq!(script('\u{10FFFF}'), Script::Unknown);
    }

    #[test]
    fn segments_mixed_script_string() {
        // "abc" (Latin) + "אב" (Hebrew, 2 bytes/char) + "12" (Common,
        // absorbed into the trailing Hebrew run).
        let runs = script_runs("abc\u{05D0}\u{05D1}12");
        assert_eq!(
            runs,
            vec![
                ScriptRun {
                    script: Script::Latin,
                    start: 0,
                    end: 3
                },
                ScriptRun {
                    script: Script::Hebrew,
                    start: 3,
                    end: 9
                },
            ]
        );
    }

    #[test]
    fn all_common_string_is_one_common_run() {
        let runs = script_runs("12 .!");
        assert_eq!(
            runs,
            vec![ScriptRun {
                script: Script::Common,
                start: 0,
                end: 5
            }]
        );
    }

    #[test]
    fn leading_inherited_is_absorbed_by_following_script() {
        // U+0301 (Inherited combining acute) then 'a' (Latin): the whole
        // string is one Latin run starting at byte 0.
        let runs = script_runs("\u{0301}a");
        assert_eq!(
            runs,
            vec![ScriptRun {
                script: Script::Latin,
                start: 0,
                end: 3
            }]
        );
    }

    #[test]
    fn all_inherited_string_is_one_common_run() {
        // Inherited codepoints with no concrete base resolve through the
        // `Common` sentinel to a single Common run (two 2-byte combining
        // marks -> bytes 0..4).
        let runs = script_runs("\u{0301}\u{0300}");
        assert_eq!(
            runs,
            vec![ScriptRun {
                script: Script::Common,
                start: 0,
                end: 4
            }]
        );
    }

    #[test]
    fn empty_string_yields_no_runs() {
        assert!(script_runs("").is_empty());
    }

    #[test]
    fn script_ranges_sorted_non_overlapping_and_in_bounds() {
        let ranges = generated::SCRIPT_RANGES;
        let mut prev_end: Option<u32> = None;
        for &(start, end, idx) in ranges {
            assert!(start <= end, "inverted range {start:#X}..{end:#X}");
            assert!(end <= 0x10_FFFF, "range past Unicode max: {end:#X}");
            if let Some(pe) = prev_end {
                assert!(
                    start > pe,
                    "overlap/disorder at {start:#X} (prev end {pe:#X})"
                );
            }
            prev_end = Some(end);
            // `from_index` must accept every emitted discriminant.
            let _ = Script::from_index(idx);
        }
    }
}
