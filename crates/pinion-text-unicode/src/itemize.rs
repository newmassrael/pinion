//! R1038 §5.37.4 + §5.37.5 — bidi + script itemisation (run splitting).
//!
//! The bridge between the analysis layers (BIDI directional resolution
//! §5.37.4, Script property §5.37.5) and the shaper (§5.37.6): splits a
//! paragraph into maximal runs uniform in BOTH resolved embedding level
//! AND script. That run is the unit a shaper consumes — one shaping pass
//! at one direction per run — which is exactly the "character order"
//! §5.37.4 names as a shape-engine prerequisite and the "run splitting"
//! §5.37.5 names as the shape-engine input.
//!
//! [`itemize`] is logical-order only. Visual (display) order is a later
//! step belonging to the shaper: reorder the runs by their levels
//! (UAX #9 L2) and reverse glyphs within RTL runs. Production paint is
//! still §5.36 swash, so [`itemize`] is a test forcing-consumer until the
//! self-hosted shaper wires in.

use crate::bidi::resolved_levels;
use crate::script::{Script, script_runs};

/// One itemised run: a maximal byte range uniform in both resolved BIDI
/// embedding level and script — the unit a shaper consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRun {
    /// Resolved UAX #9 embedding level (even = LTR, odd = RTL).
    pub level: u8,
    /// Resolved UAX #24 script of every codepoint in the run.
    pub script: Script,
    /// Byte offset of the run's first codepoint.
    pub start: usize,
    /// Byte offset just past the run's last codepoint.
    pub end: usize,
}

impl ItemRun {
    /// `true` if the run is right-to-left (odd embedding level).
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        self.level % 2 == 1
    }
}

/// Split `paragraph` into maximal runs uniform in both resolved BIDI
/// embedding level (§5.37.4) and script (§5.37.5), in logical order — the
/// shaper's input. A new run begins wherever either the level or the
/// script changes; runs are contiguous and cover the whole paragraph, and
/// an empty string yields no runs. `Common`/`Inherited` codepoints inherit
/// the surrounding script (via [`script_runs`]) rather than splitting a
/// run. Expects a single paragraph (slice from
/// [`crate::bidi::iter_paragraphs`]); levels are paragraph-local.
#[must_use]
pub fn itemize(paragraph: &str) -> Vec<ItemRun> {
    let levels = resolved_levels(paragraph);
    let sruns = script_runs(paragraph);
    let mut out: Vec<ItemRun> = Vec::new();
    let mut srun = 0usize;
    let mut cur: Option<ItemRun> = None;
    for (char_idx, (byte, ch)) in paragraph.char_indices().enumerate() {
        // Advance to the script run containing this byte. `script_runs`
        // is contiguous and sorted, so a forward cursor suffices.
        while srun + 1 < sruns.len() && byte >= sruns[srun].end {
            srun += 1;
        }
        let script = sruns[srun].script;
        let level = levels[char_idx];
        let end = byte + ch.len_utf8();
        match cur {
            Some(ref mut run) if run.level == level && run.script == script => {
                run.end = end;
            }
            _ => {
                if let Some(run) = cur.take() {
                    out.push(run);
                }
                cur = Some(ItemRun {
                    level,
                    script,
                    start: byte,
                    end,
                });
            }
        }
    }
    if let Some(run) = cur {
        out.push(run);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ItemRun, itemize};
    use crate::script::Script;

    #[test]
    fn ltr_single_script_is_one_run() {
        let runs = itemize("abc");
        assert_eq!(
            runs,
            vec![ItemRun {
                level: 0,
                script: Script::Latin,
                start: 0,
                end: 3
            }]
        );
        assert!(!runs[0].is_rtl());
    }

    #[test]
    fn rtl_single_script_is_one_rtl_run() {
        // Hebrew alef-bet-gimel: base level resolves RTL (level 1).
        let runs = itemize("\u{05D0}\u{05D1}\u{05D2}");
        assert_eq!(
            runs,
            vec![ItemRun {
                level: 1,
                script: Script::Hebrew,
                start: 0,
                end: 6
            }]
        );
        assert!(runs[0].is_rtl());
    }

    #[test]
    fn mixed_direction_splits_by_level_and_script() {
        // "abc" (Latin, LTR level 0) then Hebrew (RTL level 1).
        let runs = itemize("abc\u{05D0}\u{05D1}\u{05D2}");
        assert_eq!(
            runs,
            vec![
                ItemRun {
                    level: 0,
                    script: Script::Latin,
                    start: 0,
                    end: 3
                },
                ItemRun {
                    level: 1,
                    script: Script::Hebrew,
                    start: 3,
                    end: 9
                },
            ]
        );
    }

    #[test]
    fn same_direction_splits_by_script() {
        // Latin then Han, both LTR (level 0): split on script alone.
        let runs = itemize("abc\u{4E16}\u{754C}");
        assert_eq!(
            runs,
            vec![
                ItemRun {
                    level: 0,
                    script: Script::Latin,
                    start: 0,
                    end: 3
                },
                ItemRun {
                    level: 0,
                    script: Script::Han,
                    start: 3,
                    end: 9
                },
            ]
        );
    }

    #[test]
    fn trailing_common_is_absorbed_into_preceding_run() {
        // The space (Common) joins the Latin run; L1 keeps trailing
        // whitespace at the paragraph level (0), so level matches too.
        let runs = itemize("abc ");
        assert_eq!(
            runs,
            vec![ItemRun {
                level: 0,
                script: Script::Latin,
                start: 0,
                end: 4
            }]
        );
    }

    #[test]
    fn empty_paragraph_yields_no_runs() {
        assert!(itemize("").is_empty());
    }

    #[test]
    fn same_script_splits_by_level() {
        // LRI...PDI embeds an inner left-to-right isolate inside Latin
        // text: inner 'b' resolves to a deeper even level (>= 2), the
        // outer 'a'/'c' stay at level 0, every codepoint is Script::Latin
        // (the isolate format chars are Common, absorbed). So itemize must
        // split on level alone — a script-only merge would yield 1 run.
        let runs = itemize("a\u{2066}b\u{2069}c");
        assert!(
            runs.len() >= 3,
            "embedding must split by level, got {runs:?}"
        );
        assert!(runs.iter().all(|r| r.script == Script::Latin), "all Latin");
        assert!(
            runs.iter().any(|r| r.level >= 2),
            "inner isolate run level >= 2"
        );
        assert!(runs.iter().any(|r| r.level == 0), "outer runs at level 0");
    }

    #[test]
    fn runs_are_contiguous_and_cover_the_paragraph() {
        let text = "ab\u{05D0}\u{05D1}\u{4E16} cd";
        let runs = itemize(text);
        assert!(!runs.is_empty());
        assert_eq!(runs[0].start, 0, "first run starts at 0");
        assert_eq!(runs[runs.len() - 1].end, text.len(), "last run ends at len");
        for pair in runs.windows(2) {
            assert_eq!(
                pair[0].end, pair[1].start,
                "runs must be contiguous, no gap/overlap"
            );
            assert!(pair[0].start < pair[0].end, "run must be non-empty");
        }
        // Every boundary lands on a char boundary.
        for run in &runs {
            assert!(text.is_char_boundary(run.start) && text.is_char_boundary(run.end));
        }
    }
}
