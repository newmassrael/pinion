//! R50.6 §5.37.6 — real-font text-run shaping integration tests.
//!
//! Drives the baseline shaper end-to-end on the OFL fixtures (Noto Sans Latin +
//! Nanum Gothic 한글): `&str` → positioned glyph run → composited AA coverage.
//! Complements the pure `over()` unit tests in `shape.rs`.

use pinion_text_font::Font;
use std::fs;

fn load(path: &str) -> Font {
    let bytes = fs::read(path).expect("fixture present");
    Font::from_bytes(bytes).expect("valid Font")
}

const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";

/// Design-units → device-px scale for a font at `px_per_em`.
fn scale(font: &Font, px_per_em: f32) -> f32 {
    px_per_em / f32::from(font.units_per_em())
}

#[test]
fn shape_run_accumulates_advances() {
    // "AV": glyph 0 sits at the origin, glyph 1 sits at exactly glyph 0's
    // advance, and the run advance is the sum — proves hmtx advances accumulate.
    let font = load(NOTO);
    let px = 48.0;
    let s = scale(&font, px);
    let a = font.glyph_id_for(0x0041).expect("'A' mapped");
    let v = font.glyph_id_for(0x0056).expect("'V' mapped");
    let adv_a = f32::from(font.glyph_advance_width(a).expect("'A' advance")) * s;
    let adv_v = f32::from(font.glyph_advance_width(v).expect("'V' advance")) * s;

    let run = font.shape_run("AV", px);
    assert_eq!(run.glyphs.len(), 2, "two codepoints → two glyphs");
    assert_eq!(run.glyphs[0].glyph_id, a);
    assert_eq!(run.glyphs[1].glyph_id, v);
    assert!(
        (run.glyphs[0].x - 0.0).abs() < 1e-3,
        "first glyph at the pen origin"
    );
    assert!(
        (run.glyphs[1].x - adv_a).abs() < 1e-3,
        "second glyph at glyph 0's advance"
    );
    assert!(
        (run.advance - (adv_a + adv_v)).abs() < 1e-3,
        "run advance = sum of advances"
    );
    assert!(
        adv_a > 0.0,
        "advances are positive (non-degenerate fixture)"
    );
}

#[test]
fn shape_run_applies_gpos_pair_kerning() {
    // The forcing consumer for §5.37.6 GPOS pair kerning: NotoSans ships a real
    // GPOS `kern` feature; shape_run must reach it (Script→Feature→Lookup→
    // PairPos) and fold the adjustment into the pen.
    let font = load(NOTO);
    assert!(font.gpos.is_some(), "NotoSans ships a GPOS table");
    assert!(
        font.gpos.as_ref().unwrap().has_kerning(),
        "NotoSans GPOS exposes a kern feature reachable from the default script"
    );

    let px = 64.0_f32;
    let s = scale(&font, px);

    // Classic Latin kern pairs. Probe until one is actually kerned so the
    // assertion is non-vacuous — a parser that found no kerning would leave
    // every probe at 0 and fail the `expect` below.
    let candidates = [
        "AV", "VA", "AW", "WA", "AT", "TA", "AY", "YA", "AC", "PA", "Ta", "Te", "To", "Tr", "Tu",
        "Tw", "Ty", "Wa", "We", "Wo", "Ya", "Yo", "Ve", "Vo", "F.", "P.", "L'", "r.", "7.", "f.",
    ];
    let kerned = candidates.iter().find_map(|pair| {
        let mut it = pair.chars();
        let (a, b) = (it.next().unwrap(), it.next().unwrap());
        let ga = font.glyph_id_for(a as u32)?;
        let gb = font.glyph_id_for(b as u32)?;
        let k = font.kern_x_advance(ga, gb);
        (k != 0).then_some((*pair, ga, gb, k))
    });
    let (pair, ga, gb, kern) =
        kerned.expect("NotoSans kerns at least one classic Latin pair via GPOS");

    // Exact positioning oracle — mirror shape_run's accumulation term-by-term so
    // the f32 result is bit-identical (no tolerance fudge): glyph 1 sits at glyph
    // 0's hmtx advance PLUS the kern, and the run advance then adds glyph 1's.
    let adv_a = font.glyph_advance_width(ga).unwrap();
    let adv_b = font.glyph_advance_width(gb).unwrap();
    let expected_second_x = f32::from(adv_a) * s + f32::from(kern) * s;
    let expected_advance = expected_second_x + f32::from(adv_b) * s;

    let run = font.shape_run(pair, px);
    assert_eq!(run.glyphs.len(), 2, "{pair} → two glyphs");
    // Exact f32 equality via bit patterns: the oracle mirrors shape_run's
    // accumulation term-by-term, so the results are bit-identical — no proxy
    // tolerance that could mask a partial-kern bug.
    assert_eq!(
        run.glyphs[0].x.to_bits(),
        0.0f32.to_bits(),
        "first glyph at the origin"
    );
    assert_eq!(
        run.glyphs[1].x.to_bits(),
        expected_second_x.to_bits(),
        "{pair}: glyph 1 at adv(0) + kern ({kern} units)"
    );
    assert_eq!(
        run.advance.to_bits(),
        expected_advance.to_bits(),
        "{pair}: run advance folds kern"
    );

    // The kern must actually displace glyph 1 from its bare-advance position,
    // proving GPOS changed the layout rather than being a no-op.
    let bare_second_x = f32::from(adv_a) * s;
    assert_ne!(
        run.glyphs[1].x.to_bits(),
        bare_second_x.to_bits(),
        "{pair}: kern ({kern} units) moves glyph 1 off the bare hmtx advance"
    );
}

#[test]
fn shape_run_applies_gsub_ligature() {
    // The forcing consumer for §5.37.6 GSUB ligature substitution: NotoSans
    // ships a real `liga` feature; substitute_glyphs must reach it (Script→
    // Feature→Lookup→LigatureSubst) and collapse a component sequence, and
    // shape_run must surface the collapse (fewer glyphs, correct cluster).
    let font = load(NOTO);
    assert!(font.gsub.is_some(), "NotoSans ships a GSUB table");
    assert!(
        font.gsub.as_ref().unwrap().has_ligatures(),
        "NotoSans GSUB exposes a liga feature reachable from the default script"
    );

    // Probe classic Latin ligature clusters; find one NotoSans actually ligates
    // (output glyph count < input) so the assertion is non-vacuous.
    let candidates = ["ffi", "ffl", "fi", "fl", "ff"];
    let ligated = candidates.iter().find_map(|word| {
        let glyphs: Vec<u16> = word
            .chars()
            .map(|c| font.glyph_id_for(c as u32).expect("Latin glyph mapped"))
            .collect();
        let out = font.substitute_glyphs(&glyphs);
        (out.len() < glyphs.len()).then_some((*word, glyphs.len(), out))
    });
    let (word, n_in, out) =
        ligated.expect("NotoSans ligates at least one classic Latin cluster via GSUB");
    assert!(
        out.len() < n_in,
        "{word}: {n_in} glyphs collapsed to {}",
        out.len()
    );

    // shape_run surfaces the collapse: fewer positioned glyphs than codepoints,
    // the ligature glyph first, carrying its first component's byte cluster (0).
    let run = font.shape_run(word, 64.0);
    assert_eq!(
        run.glyphs.len(),
        out.len(),
        "{word}: shaped glyph count == substituted count"
    );
    assert!(
        run.glyphs.len() < word.chars().count(),
        "{word}: shaped fewer glyphs ({}) than chars ({})",
        run.glyphs.len(),
        word.chars().count()
    );
    assert_eq!(
        run.glyphs[0].glyph_id, out[0].0,
        "first glyph is the ligature"
    );
    assert_eq!(
        run.glyphs[0].cluster, 0,
        "ligature cluster = first component"
    );

    // Cluster mapping for a non-leading ligature: prepend ASCII 'a' (1 byte,
    // does not ligature with 'f') — the ligature now starts at byte 1.
    let prefixed = format!("a{word}");
    let run2 = font.shape_run(&prefixed, 64.0);
    assert_eq!(run2.glyphs[0].cluster, 0, "'a' at byte 0");
    assert_eq!(
        run2.glyphs[1].cluster, 1,
        "ligature carries its first component's cluster (byte 1)"
    );
    assert_eq!(
        run2.glyphs[1].glyph_id, out[0].0,
        "same ligature glyph after 'a'"
    );
}

#[test]
fn shape_run_single_glyph_has_no_kern() {
    // Kerning is a pair property: a lone glyph's advance is the bare hmtx value,
    // never adjusted by GPOS (there is no preceding glyph to kern against).
    let font = load(NOTO);
    let px = 64.0_f32;
    let s = scale(&font, px);
    let a = font.glyph_id_for(0x0041).expect("'A' mapped");
    let run = font.shape_run("A", px);
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(
        run.advance.to_bits(),
        (f32::from(font.glyph_advance_width(a).unwrap()) * s).to_bits(),
        "single glyph advance is the unmodified hmtx value"
    );
}

#[test]
fn shape_run_records_utf8_byte_clusters() {
    // "A한B": clusters are byte offsets, so the 3-byte 한 pushes 'B' to byte 4.
    // Proves the shaper iterates by codepoint and tracks source byte offsets.
    let font = load(NOTO);
    let run = font.shape_run("A한B", 32.0);
    let clusters: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(
        clusters,
        vec![0, 1, 4],
        "byte offsets: A@0, 한@1 (3 bytes), B@4"
    );
}

#[test]
fn shape_run_unmapped_codepoint_is_notdef() {
    // U+10FFFF (the last code point, unassigned) maps to no glyph → .notdef (0).
    let font = load(NOTO);
    let run = font.shape_run("\u{10FFFF}", 32.0);
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(
        run.glyphs[0].glyph_id, 0,
        "unmapped codepoint resolves to .notdef"
    );
}

#[test]
fn shape_run_empty_string_is_empty() {
    let font = load(NOTO);
    let run = font.shape_run("", 32.0);
    assert!(run.glyphs.is_empty(), "no glyphs");
    assert!((run.advance - 0.0).abs() < f32::EPSILON, "zero advance");
}

#[test]
fn render_run_positions_two_latin_glyphs_side_by_side() {
    // "HH": two non-overlapping stems. The composited bitmap is wider than one
    // 'H', carries ~2x the ink (alpha-over adds for non-overlapping glyphs), and
    // has ink in BOTH its left and right thirds — proving the second glyph was
    // placed at an advance, not stacked on the first.
    let font = load(NOTO);
    let px = 48.0;
    let one = font.render_run("H", px).expect("renders");
    let two = font.render_run("HH", px).expect("renders");
    assert!(one.ink_sum() > 0, "single 'H' inks");
    assert!(
        two.width > one.width,
        "two glyphs wider than one: {} vs {}",
        two.width,
        one.width
    );
    assert!(
        two.ink_sum() > one.ink_sum() * 3 / 2,
        "two glyphs ≈ 2x ink, got {} vs {}",
        two.ink_sum(),
        one.ink_sum(),
    );
    // Exact positioning oracle: "HH" is one 'H' bitmap plus a second copy shifted
    // right by exactly the integer-snapped advance, so the composite width is
    // round(advance) + one-'H' width. A partial-advance bug (glyph 2 at a
    // fraction of the advance) changes this width — the ink-thirds check below
    // alone would not catch it.
    let advance_px = font.shape_run("HH", px).glyphs[1].x;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let expected_w = advance_px.round() as usize + one.width;
    assert_eq!(
        two.width, expected_w,
        "second 'H' placed at exactly the advance"
    );
    let third = two.width / 3;
    let ink_in =
        |xs: std::ops::Range<usize>| (0..two.height).any(|y| xs.clone().any(|x| two.at(x, y) > 0));
    assert!(ink_in(0..third), "ink in the left third (first glyph)");
    assert!(
        ink_in(two.width - third..two.width),
        "ink in the right third (second glyph)"
    );
}

#[test]
fn render_run_aligns_mixed_height_glyphs_on_one_baseline() {
    // 'H' (cap height) is taller than 'x' (x-height): they ink above the baseline
    // with different `top`. The composite must place both on the shared baseline
    // (min_y = the topmost glyph's top, each blitted at top − min_y), so 'H' ink
    // reaches a higher row (smaller y) than 'x' ink. A min_y bug (max instead of
    // min, or per-glyph y offset dropped) would misalign them — same-height runs
    // like "HH" cannot detect that.
    let font = load(NOTO);
    let px = 48.0;
    let hx = font.render_run("Hx", px).expect("renders");
    let third = hx.width / 3;
    let top_row =
        |xs: std::ops::Range<usize>| (0..hx.height).find(|&y| xs.clone().any(|x| hx.at(x, y) > 0));
    let h_top = top_row(0..third).expect("'H' inks in the left third");
    let x_top = top_row(hx.width - third..hx.width).expect("'x' inks in the right third");
    assert!(
        h_top < x_top,
        "cap 'H' rises above x-height 'x' on the baseline: H@{h_top} x@{x_top}"
    );
}

#[test]
fn render_run_single_glyph_is_byte_identical_to_raster() {
    // render_run of one glyph == that glyph's coverage, byte-for-byte. Proves the
    // atlas round-trip inside render_run (rasterize → pack → blit sub-rect) is
    // lossless with no off-by-one in the packed-sub-rect read.
    let font = load(NOTO);
    let h = font.glyph_id_for(0x0048).expect("'H' mapped");
    let direct = font.rasterize_glyph(h, 48.0).expect("rasterizes");
    let rendered = font.render_run("H", 48.0).expect("renders");
    assert_eq!(
        (rendered.width, rendered.height),
        (direct.width, direct.height),
        "same size"
    );
    assert_eq!(
        (rendered.left, rendered.top),
        (direct.left, direct.top),
        "same pen offset"
    );
    assert_eq!(
        rendered.alpha, direct.alpha,
        "render_run('H') is byte-identical to the raster"
    );
}

#[test]
fn render_run_blank_run_is_empty() {
    // A space is an empty outline: it advances the pen but inks nothing, so a
    // run of only spaces (and the empty string) composites to an empty bitmap.
    let font = load(NOTO);
    assert!(
        font.render_run("", 32.0).expect("renders").is_empty(),
        "empty string"
    );
    assert!(
        font.glyph_id_for(0x0020).is_some(),
        "Noto maps U+0020 space"
    );
    assert!(
        font.render_run("   ", 32.0).expect("renders").is_empty(),
        "all spaces ink nothing"
    );
}

#[test]
fn render_run_space_widens_the_run() {
    // "H H" must be wider than "HH": the interior space advances the pen, opening
    // a gap between the two inked glyphs even though it leaves no ink itself.
    let font = load(NOTO);
    assert!(
        font.glyph_id_for(0x0020).is_some(),
        "Noto maps U+0020 space"
    );
    let px = 48.0;
    let tight = font.render_run("HH", px).expect("renders");
    let spaced = font.render_run("H H", px).expect("renders");
    assert!(
        spaced.width > tight.width,
        "the space widens the run: {} vs {}",
        spaced.width,
        tight.width,
    );
}

#[test]
fn render_run_wide_cjk_places_two_syllables() {
    // 가 (U+AC00) is a full-width 한글 syllable Nanum renders to ink. A run of two
    // copies is wider than one and carries ~2x the ink — the self-hosted engine
    // rendering a real CJK *string*, not just one glyph.
    let font = load(NANUM);
    let px = 48.0;
    // Guard: the fixture must map 가 and rasterize it (fail loud, never skip).
    let ga = font.glyph_id_for(0xAC00).expect("Nanum maps 가 (U+AC00)");
    let single_ink = font
        .rasterize_glyph(ga, px)
        .expect("가 rasterizes (simple or composite, no point-match)")
        .ink_sum();
    assert!(single_ink > 0, "가 inks");

    let one = font.render_run("가", px).expect("renders");
    let two = font.render_run("가가", px).expect("renders");
    assert!(
        two.width > one.width,
        "two 가 wider than one: {} vs {}",
        two.width,
        one.width
    );
    assert!(
        two.ink_sum() > one.ink_sum() * 3 / 2,
        "two 가 ≈ 2x ink, got {} vs {}",
        two.ink_sum(),
        one.ink_sum(),
    );
}

/// Find a `(base, mark)` pair `NotoSans` attaches via a GPOS mark-to-base
/// lookup with a non-zero vertical lift (`dy != 0`) — the R50.6.2 forcing case.
/// Returns `(base, mark, base_gid, mark_gid, dx, dy)` (anchor delta, design
/// units, y up).
fn positioned_mark(font: &Font) -> Option<(char, char, u16, u16, i16, i16)> {
    let bases = ['a', 'e', 'o', 'n', 'c', 'u', 'i', 'A', 'E', 'O'];
    let marks = [
        '\u{0301}', '\u{0300}', '\u{0302}', '\u{0303}', '\u{0308}', '\u{030C}', '\u{0306}',
        '\u{0327}',
    ];
    bases.iter().find_map(|&b| {
        let bg = font.glyph_id_for(b as u32)?;
        marks.iter().find_map(|&m| {
            let mg = font.glyph_id_for(m as u32)?;
            let (dx, dy) = font.mark_offset(bg, mg)?;
            (dy != 0).then_some((b, m, bg, mg, dx, dy))
        })
    })
}

#[test]
fn shape_run_applies_gpos_mark_to_base() {
    // Forcing consumer for §5.37.6 GPOS mark-to-base (R50.6.2): NotoSans ships
    // real `mark`-feature MarkBasePos lookups; shape_run must reach them
    // (Script→Feature→Lookup→MarkBasePos) and attach a combining mark to its
    // base at the anchor, lifting it off the baseline.
    let font = load(NOTO);
    assert!(font.gpos.is_some(), "NotoSans ships a GPOS table");
    assert!(
        font.gpos.as_ref().unwrap().has_marks(),
        "NotoSans GPOS exposes a mark feature reachable from the default script"
    );

    let px = 64.0_f32;
    let s = scale(&font, px);
    let (base, mark, bg, mg, dx, dy) =
        positioned_mark(&font).expect("NotoSans attaches a combining mark to a Latin base");

    let run = font.shape_run(&format!("{base}{mark}"), px);
    assert_eq!(
        run.glyphs.len(),
        2,
        "base + mark => two glyphs (no ccmp/liga)"
    );
    assert_eq!(run.glyphs[0].glyph_id, bg, "glyph 0 is the base");
    assert_eq!(run.glyphs[1].glyph_id, mg, "glyph 1 is the mark");

    // Base on the baseline at the origin; mark anchor-placed relative to it.
    // Exact f32 oracle mirrors shape_run's `base.x + dx*scale` / `-dy*scale`.
    assert_eq!(run.glyphs[0].x.to_bits(), 0.0f32.to_bits(), "base origin x");
    assert_eq!(
        run.glyphs[0].y.to_bits(),
        0.0f32.to_bits(),
        "base on baseline"
    );
    let expected_x = 0.0f32 + f32::from(dx) * s;
    let expected_y = -f32::from(dy) * s;
    assert_eq!(
        run.glyphs[1].x.to_bits(),
        expected_x.to_bits(),
        "{base}{mark}: mark x = base.x + anchor dx ({dx} units)"
    );
    assert_eq!(
        run.glyphs[1].y.to_bits(),
        expected_y.to_bits(),
        "{base}{mark}: mark y = -anchor dy ({dy} units)"
    );

    // Non-vacuous: the mark genuinely left the baseline — bare shaping never
    // sets a non-zero y, so this only passes when mark positioning ran.
    assert_ne!(
        run.glyphs[1].y.to_bits(),
        0.0f32.to_bits(),
        "mark is lifted off the baseline"
    );
}

#[test]
fn render_run_mark_inks_above_the_base() {
    // The composited coverage of base+mark reaches above the bare base: the
    // mark's ink sits over the baseline, so the union top is higher (smaller).
    let font = load(NOTO);
    let px = 64.0_f32;
    let (base, mark, ..) =
        positioned_mark(&font).expect("NotoSans attaches a combining mark to a Latin base");

    let plain = font
        .render_run(&base.to_string(), px)
        .expect("base renders");
    let accented = font
        .render_run(&format!("{base}{mark}"), px)
        .expect("base+mark renders");
    assert!(
        accented.top < plain.top,
        "{base}{mark}: accented top {} must rise above bare base top {}",
        accented.top,
        plain.top
    );
    assert!(
        accented.ink_sum() > plain.ink_sum(),
        "accented text inks more than the bare base"
    );
}

/// Find a `(base, mark1, mark2)` triple where `NotoSans` attaches `mark1` to the
/// base (mark-to-base) AND stacks `mark2` on `mark1` (mark-to-mark / `mkmk`),
/// returning the chars and the `mkmk` `(dx, dy)` of `mark2` on `mark1`. Searches
/// Latin bases × common combining marks (e.g. Vietnamese â + ́: circumflex on the
/// base, acute stacked on the circumflex). Discovery, not a hardcoded glyph pair,
/// so a fixture change can only skip — never silently mis-assert.
fn stacked_mark(font: &Font) -> Option<(char, char, char, i16, i16)> {
    let bases = ['a', 'e', 'o', 'u', 'i', 'A', 'E', 'O'];
    let marks = [
        '\u{0302}', '\u{0301}', '\u{0300}', '\u{0303}', '\u{0308}', '\u{0327}', '\u{0306}',
        '\u{030C}', '\u{0309}', '\u{0323}',
    ];
    for &b in &bases {
        let Some(bg) = font.glyph_id_for(b as u32) else {
            continue;
        };
        for &m1 in &marks {
            let Some(m1g) = font.glyph_id_for(m1 as u32) else {
                continue;
            };
            // mark1 must attach to the base, so the shaper makes it the stacking
            // reference (prev_mark) for the following mark.
            if font.mark_offset(bg, m1g).is_none() {
                continue;
            }
            for &m2 in &marks {
                let Some(m2g) = font.glyph_id_for(m2 as u32) else {
                    continue;
                };
                if let Some((dx, dy)) = font.mark_mark_offset(m1g, m2g) {
                    return Some((b, m1, m2, dx, dy));
                }
            }
        }
    }
    None
}

#[test]
fn shape_run_applies_gpos_mark_to_mark() {
    // Forcing consumer for §5.37.6 GPOS mark-to-mark (R50.6.3): NotoSans ships real
    // `mkmk` lookups; shape_run must reach them (Script→Feature→Lookup→MarkMarkPos)
    // and stack a second combining mark on the FIRST mark — not on the base — so
    // diacritics pile up correctly.
    let font = load(NOTO);
    assert!(
        font.gpos
            .as_ref()
            .is_some_and(pinion_text_font::Gpos::has_mark_marks),
        "NotoSans exposes a mkmk feature reachable from the default script"
    );

    let px = 64.0_f32;
    let s = scale(&font, px);
    let (base, m1, m2, dx, dy) =
        stacked_mark(&font).expect("NotoSans stacks one combining mark on another via mkmk");

    let run = font.shape_run(&format!("{base}{m1}{m2}"), px);
    assert_eq!(
        run.glyphs.len(),
        3,
        "base + two marks => three glyphs (no ccmp/liga collapse)"
    );

    // The stacking mark (glyph 2) is placed against glyph 1's RESOLVED position,
    // not the baseline or the base: exact f32 oracle mirrors shape_run's
    // `prev_mark.x + dx*scale` / `prev_mark.y - dy*scale`.
    let (ref_x, ref_y) = (run.glyphs[1].x, run.glyphs[1].y);
    let expected_x = ref_x + f32::from(dx) * s;
    let expected_y = ref_y - f32::from(dy) * s;
    assert_eq!(
        run.glyphs[2].x.to_bits(),
        expected_x.to_bits(),
        "{base}{m1}{m2}: mark2 x = mark1.x + mkmk dx ({dx} units)"
    );
    assert_eq!(
        run.glyphs[2].y.to_bits(),
        expected_y.to_bits(),
        "{base}{m1}{m2}: mark2 y = mark1.y - mkmk dy ({dy} units)"
    );

    // Non-vacuous: mark1 itself left the baseline (mark-to-base ran), so mark2's
    // y is referenced off a non-zero anchor — this only holds when the mkmk pass
    // used mark1 (not the base) as the reference.
    assert_ne!(
        run.glyphs[1].y.to_bits(),
        0.0f32.to_bits(),
        "mark1 is lifted off the baseline (mark-to-base ran first)"
    );
}

#[test]
fn gdef_classifies_letters_and_marks() {
    // NotoSans ships a GDEF GlyphClassDef: letters are Base glyphs, combining marks
    // are Mark glyphs. This is what lets the shaper recognise a mark structurally
    // (rather than only by whether a GPOS lookup happened to attach it).
    use pinion_text_font::GlyphClass;
    let font = load(NOTO);
    assert!(
        font.has_glyph_classes(),
        "NotoSans ships a GDEF GlyphClassDef"
    );

    for ch in ['a', 'A', 'o', 'M'] {
        let g = font.glyph_id_for(ch as u32).expect("Latin letter mapped");
        assert_eq!(font.glyph_class(g), GlyphClass::Base, "{ch:?} is a base");
        assert!(!font.is_mark(g), "{ch:?} is not a mark");
    }
    // Several standard combining marks must all classify as Mark.
    let mut seen_mark = 0;
    for m in ['\u{0301}', '\u{0302}', '\u{0308}', '\u{0323}'] {
        if let Some(g) = font.glyph_id_for(m as u32) {
            assert_eq!(
                font.glyph_class(g),
                GlyphClass::Mark,
                "U+{:04X} is a combining mark",
                m as u32
            );
            assert!(font.is_mark(g));
            seen_mark += 1;
        }
    }
    assert!(seen_mark >= 2, "NotoSans maps common combining marks");
}

/// Find a `(non_attaching_mark, attaching_mark, dx, dy)` for base `'a'`: a GDEF
/// mark that declares NO mark-to-base anchor on `'a'`, plus another mark that DOES
/// (with offset `(dx, dy)`) and does not `mkmk`-stack on the first — so the second
/// must reach the base past the first. The scenario GDEF mark-recognition fixes.
fn cross_mark_case(font: &Font) -> Option<(char, char, i16, i16)> {
    let bg = font.glyph_id_for('a' as u32)?;
    let mut non_attaching: Option<(char, u16)> = None;
    let mut attaching: Option<(char, u16, i16, i16)> = None;
    // Scan the Combining Diacritical Marks block (U+0300..=U+036F): NotoSans anchors
    // the common above/below accents to 'a' but not the rarer marks, so both an
    // attaching and a non-attaching GDEF mark exist in here.
    for cp in 0x0300u32..=0x036F {
        let (Some(m), Some(mg)) = (char::from_u32(cp), font.glyph_id_for(cp)) else {
            continue;
        };
        if !font.is_mark(mg) {
            continue; // must be a GDEF mark for the recognition to bite
        }
        match font.mark_offset(bg, mg) {
            Some((dx, dy)) if dy != 0 && attaching.is_none() => attaching = Some((m, mg, dx, dy)),
            None if non_attaching.is_none() => non_attaching = Some((m, mg)),
            _ => {}
        }
        if non_attaching.is_some() && attaching.is_some() {
            break;
        }
    }
    let (non_char, non_glyph) = non_attaching?;
    let (attach_char, attach_glyph, dx, dy) = attaching?;
    // The attaching mark must NOT mkmk-stack on the non-attaching one, so in the
    // shaper it falls through to mark-to-base on the real base (clean oracle).
    if font.mark_mark_offset(non_glyph, attach_glyph).is_some() {
        return None;
    }
    Some((non_char, attach_char, dx, dy))
}

#[test]
fn shape_run_recognises_marks_from_gdef() {
    // GDEF mark-recognition (R50.6.4): in "a + non_attaching_mark + attaching_mark",
    // the middle mark declares no anchor on 'a'. WITHOUT GDEF it would be mistaken
    // for a base and capture the third glyph; WITH GDEF it stays a mark, so the
    // third mark still attaches to the real base 'a'. Observable only because the
    // shaper now reads the GlyphClassDef.
    let font = load(NOTO);
    let px = 64.0_f32;
    let s = scale(&font, px);
    let Some((nonattach, attach, dx, dy)) = cross_mark_case(&font) else {
        panic!("NotoSans should expose a non-attaching GDEF mark alongside an attaching one");
    };

    let run = font.shape_run(&format!("a{nonattach}{attach}"), px);
    assert_eq!(run.glyphs.len(), 3, "base + two marks => three glyphs");

    // The third glyph (attaching mark) is placed against the BASE 'a' (glyph 0 at
    // the origin), not the intervening non-attaching mark — exact f32 bit-oracle.
    let expected_x = run.glyphs[0].x + f32::from(dx) * s;
    let expected_y = -f32::from(dy) * s;
    assert_eq!(
        run.glyphs[2].x.to_bits(),
        expected_x.to_bits(),
        "a{nonattach}{attach}: mark attaches to base.x + anchor dx ({dx} units), not the stray mark"
    );
    assert_eq!(
        run.glyphs[2].y.to_bits(),
        expected_y.to_bits(),
        "a{nonattach}{attach}: mark y = -anchor dy ({dy} units)"
    );
    // Non-vacuous: it genuinely left the baseline (attachment ran).
    assert_ne!(
        run.glyphs[2].y.to_bits(),
        0.0f32.to_bits(),
        "the attaching mark is lifted off the baseline"
    );
}

#[test]
fn gdef_absent_font_reports_no_marks() {
    // NanumGothic ships GPOS/GSUB but NO GDEF table. This pins the precondition of
    // the shaper's GDEF fallback (shape.rs Stage 4): with no GlyphClassDef,
    // `has_glyph_classes()` is false and `is_mark()` is false for every glyph, so
    // mark recognition collapses to the pre-GDEF attach-based path. A regression
    // that defaulted `is_mark` to true (or read a phantom GDEF) would fail here.
    let nanum = load(NANUM);
    assert!(nanum.gdef.is_none(), "NanumGothic has no GDEF table");
    assert!(
        !nanum.has_glyph_classes(),
        "no GlyphClassDef => no glyph classes"
    );
    // Sample real glyphs (a Latin letter + a Hangul syllable) — both must report
    // not-a-mark when GDEF is absent.
    for cp in ['A', '\u{AC00}'] {
        if let Some(g) = nanum.glyph_id_for(cp as u32) {
            assert!(
                !nanum.is_mark(g),
                "U+{:04X}: no GDEF => is_mark is false",
                cp as u32
            );
        }
    }
}
