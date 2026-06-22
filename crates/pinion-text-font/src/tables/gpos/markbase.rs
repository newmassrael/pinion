//! R50.6.2 §5.37.6 — GPOS `MarkBasePos` subtable (Lookup Type 4, format 1).
//!
//! Microsoft OpenType 1.9.x spec, "Mark-to-Base Attachment Positioning
//! Subtable". This is how a combining mark (an accent / diacritic) is placed
//! against the base glyph it attaches to: each mark carries an *anchor* point
//! and each base carries one anchor per *mark class*; the shaper translates the
//! mark so its anchor coincides with the base's anchor for the mark's class.
//!
//! Reached through the `mark` feature (the GPOS analogue of `kern` for
//! positioning). Mark-to-ligature (Lookup Type 5) and mark-to-mark stacking
//! (Lookup Type 6), plus `lookupFlag` mark filtering, are R50.6.x — this slice
//! covers the universal base+single-mark case. Anchor formats 2 (contour point)
//! and 3 (device tables) are accepted but their hinting refinements are ignored;
//! only the `(x, y)` design-unit coordinates are used.
//!
//! Offsets inside the subtable are relative to the subtable start, so the parser
//! is handed the whole GPOS table plus the subtable's absolute offset.

use super::GPOS_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;
use crate::tables::layout::{Coverage, slice_at};

/// An anchor point in font design units (y is up, the OpenType convention).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Anchor {
    x: i16,
    y: i16,
}

/// One mark's `(class, anchor)` — parallel to the mark Coverage by index.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct MarkRecord {
    class: u16,
    anchor: Anchor,
}

/// A parsed `MarkBasePos` subtable (format 1).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct MarkBasePos {
    mark_coverage: Coverage,
    base_coverage: Coverage,
    /// `marks[markCoverageIndex]` = that mark's class + anchor.
    marks: Vec<MarkRecord>,
    /// `base_anchors[baseCoverageIndex][markClass]` = the base's anchor for that
    /// mark class, or `None` when the base declares no anchor for it.
    base_anchors: Vec<Vec<Option<Anchor>>>,
}

impl MarkBasePos {
    /// Parse a `MarkBasePos` subtable. `table` = full GPOS table bytes;
    /// `subtable_off` = the subtable's offset from the GPOS table start.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — an offset / record runs past the table.
    /// * [`ParseError::InvalidTableField`] — unknown posFormat / anchorFormat or
    ///   a malformed Coverage.
    pub fn parse(table: &[u8], subtable_off: usize) -> Result<Self, ParseError> {
        let local = slice_at(table, subtable_off, GPOS_TAG)?;
        let mut r = Reader::new(local, GPOS_TAG);
        let pos_format = r.read_u16()?;
        if pos_format != 1 {
            return Err(ParseError::InvalidTableField {
                tag: GPOS_TAG,
                field: "markbasepos/posFormat",
                value: FieldValue::from_u16(pos_format),
            });
        }
        let mark_coverage_off = usize::from(r.read_u16()?);
        let base_coverage_off = usize::from(r.read_u16()?);
        let mark_class_count = r.read_u16()?;
        let mark_array_off = usize::from(r.read_u16()?);
        let base_array_off = usize::from(r.read_u16()?);

        let mark_coverage =
            Coverage::parse(slice_at(local, mark_coverage_off, GPOS_TAG)?, GPOS_TAG)?;
        let base_coverage =
            Coverage::parse(slice_at(local, base_coverage_off, GPOS_TAG)?, GPOS_TAG)?;
        let marks = parse_mark_array(local, mark_array_off)?;
        let base_anchors = parse_base_array(local, base_array_off, mark_class_count)?;
        Ok(Self {
            mark_coverage,
            base_coverage,
            marks,
            base_anchors,
        })
    }

    /// The design-unit translation `(dx, dy)` that places `mark`'s anchor onto
    /// `base`'s anchor for `mark`'s class — i.e. `baseAnchor - markAnchor`, y up.
    ///
    /// `None` when `mark` is not in the mark Coverage, `base` is not in the base
    /// Coverage, or the base declares no anchor for the mark's class (so the
    /// caller should try the next subtable / leave the mark on the pen).
    #[must_use]
    pub fn mark_offset(&self, base: u16, mark: u16) -> Option<(i16, i16)> {
        let mi = usize::from(self.mark_coverage.index(mark)?);
        let m = self.marks.get(mi)?;
        let bi = usize::from(self.base_coverage.index(base)?);
        let base_anchor = self
            .base_anchors
            .get(bi)?
            .get(usize::from(m.class))?
            .as_ref()?;
        // saturating: a malformed font could give anchors whose difference
        // overflows i16; saturate rather than debug-panic (the value is scaled
        // to f32 by the caller regardless).
        Some((
            base_anchor.x.saturating_sub(m.anchor.x),
            base_anchor.y.saturating_sub(m.anchor.y),
        ))
    }
}

/// Parse a `MarkArray`: `markCount` × `(markClass, markAnchorOffset)`, anchors
/// resolved against the `MarkArray` table start.
fn parse_mark_array(table: &[u8], off: usize) -> Result<Vec<MarkRecord>, ParseError> {
    let local = slice_at(table, off, GPOS_TAG)?;
    let mut r = Reader::new(local, GPOS_TAG);
    let mark_count = r.read_u16()?;
    let mut marks = Vec::with_capacity(usize::from(mark_count));
    for _ in 0..mark_count {
        let class = r.read_u16()?;
        let anchor_off = usize::from(r.read_u16()?);
        let anchor = parse_anchor(local, anchor_off)?;
        marks.push(MarkRecord { class, anchor });
    }
    Ok(marks)
}

/// Parse a `BaseArray`: `baseCount` × (`markClassCount` baseAnchorOffsets). An
/// offset of 0 means the base declares no anchor for that mark class.
fn parse_base_array(
    table: &[u8],
    off: usize,
    mark_class_count: u16,
) -> Result<Vec<Vec<Option<Anchor>>>, ParseError> {
    let local = slice_at(table, off, GPOS_TAG)?;
    let mut r = Reader::new(local, GPOS_TAG);
    let base_count = r.read_u16()?;
    let mut rows = Vec::with_capacity(usize::from(base_count));
    for _ in 0..base_count {
        // Plain push (not `with_capacity(mark_class_count)`): the Reader's
        // bounds check fails fast if a font claims more offsets than it stores.
        let mut row = Vec::new();
        for _ in 0..mark_class_count {
            let anchor_off = usize::from(r.read_u16()?);
            row.push(if anchor_off == 0 {
                None
            } else {
                Some(parse_anchor(local, anchor_off)?)
            });
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Parse an Anchor table. Formats 1/2/3 all begin with `(format, x, y)`; format
/// 2's contour `anchorPoint` and format 3's device-table offsets refine grid
/// fitting only and are ignored (R50.6.x) — the design-unit `(x, y)` is used.
fn parse_anchor(table: &[u8], off: usize) -> Result<Anchor, ParseError> {
    let local = slice_at(table, off, GPOS_TAG)?;
    let mut r = Reader::new(local, GPOS_TAG);
    let format = r.read_u16()?;
    if !(1..=3).contains(&format) {
        return Err(ParseError::InvalidTableField {
            tag: GPOS_TAG,
            field: "markbasepos/anchorFormat",
            value: FieldValue::from_u16(format),
        });
    }
    let x = r.read_i16()?;
    let y = r.read_i16()?;
    Ok(Anchor { x, y })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage1(glyph: u16) -> Vec<u8> {
        let mut b = 1u16.to_be_bytes().to_vec(); // format 1
        b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
        b.extend_from_slice(&glyph.to_be_bytes());
        b
    }

    fn anchor_bytes(format: u16, x: i16, y: i16) -> Vec<u8> {
        let mut b = format.to_be_bytes().to_vec();
        b.extend_from_slice(&x.to_be_bytes());
        b.extend_from_slice(&y.to_be_bytes());
        match format {
            2 => b.extend_from_slice(&0u16.to_be_bytes()), // anchorPoint (ignored)
            3 => b.extend_from_slice(&[0, 0, 0, 0]),       // x/y device offsets (null)
            _ => {}
        }
        b
    }

    // One mark (glyph 30, class 0, anchor (100,0)) + one base (glyph 10). The
    // base's class-0 anchor is `base_anchor` (None => no anchor offset). Offsets
    // are computed from section lengths so any anchor format places correctly.
    fn build_markbase(base_anchor: Option<(i16, i16)>, anchor_format: u16) -> Vec<u8> {
        const MARK_GLYPH: u16 = 30;
        const BASE_GLYPH: u16 = 10;
        let mark_class_count: u16 = 1;

        let mark_cov = coverage1(MARK_GLYPH);
        let base_cov = coverage1(BASE_GLYPH);

        // markArray: markCount(2) + record(class 2 + anchorOff 2) = 6, anchor at +6.
        let mut mark_array = 1u16.to_be_bytes().to_vec(); // markCount
        mark_array.extend_from_slice(&0u16.to_be_bytes()); // markClass = 0
        mark_array.extend_from_slice(&6u16.to_be_bytes()); // markAnchorOffset
        mark_array.extend_from_slice(&anchor_bytes(anchor_format, 100, 0));

        // baseArray: baseCount(2) + record(markClassCount * 2) = 4, anchor at +4.
        let mut base_array = 1u16.to_be_bytes().to_vec(); // baseCount
        let base_anchor_off: u16 = if base_anchor.is_some() { 4 } else { 0 };
        base_array.extend_from_slice(&base_anchor_off.to_be_bytes());
        if let Some((bx, by)) = base_anchor {
            base_array.extend_from_slice(&anchor_bytes(anchor_format, bx, by));
        }

        let header_len = 12;
        let mark_cov_off = header_len;
        let base_cov_off = mark_cov_off + mark_cov.len();
        let mark_array_off = base_cov_off + base_cov.len();
        let base_array_off = mark_array_off + mark_array.len();

        let mut sub = 1u16.to_be_bytes().to_vec(); // posFormat
        sub.extend_from_slice(&u16::try_from(mark_cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&u16::try_from(base_cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&mark_class_count.to_be_bytes());
        sub.extend_from_slice(&u16::try_from(mark_array_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&u16::try_from(base_array_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&mark_cov);
        sub.extend_from_slice(&base_cov);
        sub.extend_from_slice(&mark_array);
        sub.extend_from_slice(&base_array);
        sub
    }

    #[test]
    fn mark_to_base_anchor_delta() {
        // base anchor (250,700), mark anchor (100,0) => delta (150,700).
        let sub = build_markbase(Some((250, 700)), 1);
        let mb = MarkBasePos::parse(&sub, 0).unwrap();
        assert_eq!(
            mb.mark_offset(10, 30),
            Some((150, 700)),
            "baseAnchor - markAnchor"
        );
        assert_eq!(mb.mark_offset(99, 30), None, "base not covered");
        assert_eq!(mb.mark_offset(10, 99), None, "mark not covered");
    }

    #[test]
    fn nonzero_subtable_offset() {
        let sub = build_markbase(Some((250, 700)), 1);
        let mut table = vec![0u8; 8];
        table.extend_from_slice(&sub);
        let mb = MarkBasePos::parse(&table, 8).unwrap();
        assert_eq!(mb.mark_offset(10, 30), Some((150, 700)));
    }

    #[test]
    fn anchor_format3_ignores_device_tables() {
        // Format 3 adds (ignored) device offsets; the (x,y) delta is unchanged.
        let sub = build_markbase(Some((250, 700)), 3);
        let mb = MarkBasePos::parse(&sub, 0).unwrap();
        assert_eq!(mb.mark_offset(10, 30), Some((150, 700)));
    }

    #[test]
    fn missing_base_anchor_is_none() {
        // baseAnchorOffset 0 => the base declares no anchor for this mark class.
        let sub = build_markbase(None, 1);
        let mb = MarkBasePos::parse(&sub, 0).unwrap();
        assert_eq!(mb.mark_offset(10, 30), None, "no base anchor for the class");
    }

    #[test]
    fn reject_unknown_posformat() {
        let mut sub = build_markbase(Some((250, 700)), 1);
        sub[0..2].copy_from_slice(&7u16.to_be_bytes());
        assert!(matches!(
            MarkBasePos::parse(&sub, 0),
            Err(ParseError::InvalidTableField {
                field: "markbasepos/posFormat",
                ..
            })
        ));
    }

    #[test]
    fn reject_unknown_anchorformat() {
        // Corrupt the mark anchor's format (it sits last in the subtable: the
        // final 6 bytes are format(2)+x(2)+y(2) for the base anchor; the mark
        // anchor precedes it). Rebuild with a bad format via direct edit of the
        // mark anchor format word.
        let mut sub = build_markbase(Some((250, 700)), 1);
        // mark anchor format word is at mark_array_off + 6; recompute it.
        let mark_cov = coverage1(30);
        let base_cov = coverage1(10);
        let mark_array_off = 12 + mark_cov.len() + base_cov.len();
        let fmt_pos = mark_array_off + 6;
        sub[fmt_pos..fmt_pos + 2].copy_from_slice(&9u16.to_be_bytes());
        assert!(matches!(
            MarkBasePos::parse(&sub, 0),
            Err(ParseError::InvalidTableField {
                field: "markbasepos/anchorFormat",
                ..
            })
        ));
    }

    #[test]
    fn multi_class_uses_the_marks_class_anchor() {
        // markClassCount = 2, the mark in class 1: mark_offset must read the
        // base's CLASS-1 anchor. A bug indexing class 0 would give (100, 550)
        // instead of (200, 750) — the only mutation the single-class tests miss.
        let mark_cov = coverage1(30);
        let base_cov = coverage1(10);

        // markArray: count(2) + record(class 2 + anchorOff 2) = 6, anchor at +6.
        let mut mark_array = 1u16.to_be_bytes().to_vec(); // markCount
        mark_array.extend_from_slice(&1u16.to_be_bytes()); // markClass = 1
        mark_array.extend_from_slice(&6u16.to_be_bytes()); // markAnchorOffset
        mark_array.extend_from_slice(&anchor_bytes(1, 100, 50));

        // baseArray: count(2) + record(2 class offsets) = 6; anchors at +6, +12.
        let mut base_array = 1u16.to_be_bytes().to_vec(); // baseCount
        base_array.extend_from_slice(&6u16.to_be_bytes()); // class-0 anchorOffset
        base_array.extend_from_slice(&12u16.to_be_bytes()); // class-1 anchorOffset
        base_array.extend_from_slice(&anchor_bytes(1, 200, 600)); // class 0
        base_array.extend_from_slice(&anchor_bytes(1, 300, 800)); // class 1

        let header_len = 12;
        let mark_cov_off = header_len;
        let base_cov_off = mark_cov_off + mark_cov.len();
        let mark_array_off = base_cov_off + base_cov.len();
        let base_array_off = mark_array_off + mark_array.len();

        let mut sub = 1u16.to_be_bytes().to_vec(); // posFormat
        sub.extend_from_slice(&u16::try_from(mark_cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&u16::try_from(base_cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&2u16.to_be_bytes()); // markClassCount = 2
        sub.extend_from_slice(&u16::try_from(mark_array_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&u16::try_from(base_array_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&mark_cov);
        sub.extend_from_slice(&base_cov);
        sub.extend_from_slice(&mark_array);
        sub.extend_from_slice(&base_array);

        let mb = MarkBasePos::parse(&sub, 0).unwrap();
        // class-1 base anchor (300,800) - mark anchor (100,50) = (200, 750).
        assert_eq!(
            mb.mark_offset(10, 30),
            Some((200, 750)),
            "mark_offset reads the mark's own class anchor, not class 0"
        );
    }
}
