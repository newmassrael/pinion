//! R50.6.4 §5.37.6 — GDEF `GlyphClassDef` (glyph classification).
//!
//! Microsoft OpenType 1.9.x spec, "GDEF — Glyph Definition Table". GDEF carries
//! per-glyph metadata that GSUB/GPOS lean on; this slice parses its
//! **`GlyphClassDef`** — a `ClassDef` mapping each glyph to one of Base,
//! Ligature, Mark, or Component. The shaper uses it to recognise combining marks
//! independently of whether a `mark`/`mkmk` lookup happened to attach them
//! ([`crate::shape::shape_run`]), so a mark that declares no anchor is still
//! treated as a mark rather than mistaken for a base.
//!
//! Only `GlyphClassDef` is read here. The other GDEF subtables — `AttachList`,
//! `LigCaretList`, `MarkAttachClassDef`, and the v1.2+ mark-glyph-set / v1.3+
//! item-variation extensions — refine attachment / caret / `lookupFlag`
//! filtering and are R50.6.x. Offsets are from the GDEF table start.

use crate::error::ParseError;
use crate::reader::Reader;
use crate::tables::layout::{ClassDef, slice_at};

const GDEF_TAG: [u8; 4] = *b"GDEF";

/// A glyph's GDEF `GlyphClassDef` category. The numeric values are the OpenType
/// class ids; an unlisted glyph is [`GlyphClass::Unclassified`] (class 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphClass {
    /// Class 0 — not listed in `GlyphClassDef`.
    Unclassified,
    /// Class 1 — a base glyph (a single, spacing character).
    Base,
    /// Class 2 — a ligature (represents multiple characters).
    Ligature,
    /// Class 3 — a combining mark.
    Mark,
    /// Class 4 — a ligature component (not directly rendered).
    Component,
}

impl GlyphClass {
    /// Map a raw `GlyphClassDef` class id; anything outside 1..=4 is unclassified.
    fn from_class_id(class: u16) -> Self {
        match class {
            1 => Self::Base,
            2 => Self::Ligature,
            3 => Self::Mark,
            4 => Self::Component,
            _ => Self::Unclassified,
        }
    }
}

/// A parsed GDEF table — currently just its optional `GlyphClassDef`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Gdef {
    /// The `GlyphClassDef` class table, or `None` when GDEF omits it (offset 0).
    glyph_class_def: Option<ClassDef>,
}

impl Gdef {
    /// Parse the GDEF table, retaining the `GlyphClassDef`.
    ///
    /// # Errors
    ///
    /// * [`ParseError::UnsupportedTableVersion`] — majorVersion != 1.
    /// * [`ParseError::TableTooShort`] — header / offset past the table.
    /// * [`ParseError::InvalidTableField`] — malformed `ClassDef`.
    pub fn parse(table: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(table, GDEF_TAG);
        let major = r.read_u16()?;
        let minor = r.read_u16()?;
        if major != 1 {
            return Err(ParseError::UnsupportedTableVersion {
                tag: GDEF_TAG,
                major,
                minor,
            });
        }
        // glyphClassDefOffset is the first of four offsets; the rest (attachList /
        // ligCaretList / markAttachClassDef) are not read by this slice (R50.6.x).
        let glyph_class_def_off = usize::from(r.read_u16()?);
        let glyph_class_def = if glyph_class_def_off == 0 {
            None
        } else {
            Some(ClassDef::parse(
                slice_at(table, glyph_class_def_off, GDEF_TAG)?,
                GDEF_TAG,
            )?)
        };
        Ok(Self { glyph_class_def })
    }

    /// `glyph`'s GDEF class, or [`GlyphClass::Unclassified`] when GDEF declares no
    /// `GlyphClassDef` or does not list the glyph.
    #[must_use]
    pub fn glyph_class(&self, glyph: u16) -> GlyphClass {
        self.glyph_class_def
            .as_ref()
            .map_or(GlyphClass::Unclassified, |cd| {
                GlyphClass::from_class_id(cd.class_of(glyph))
            })
    }

    /// Whether `glyph` is a combining mark (`GlyphClassDef` class 3).
    #[must_use]
    pub fn is_mark(&self, glyph: u16) -> bool {
        self.glyph_class(glyph) == GlyphClass::Mark
    }

    /// Whether GDEF carries a `GlyphClassDef` at all (test / diagnostic). When
    /// `false`, [`Self::glyph_class`] is always [`GlyphClass::Unclassified`].
    #[must_use]
    pub fn has_glyph_classes(&self) -> bool {
        self.glyph_class_def.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{Gdef, GlyphClass};
    use crate::error::ParseError;

    /// A Format-2 `ClassDef` over three ranges: 10..=10 Base, 20..=21 Ligature,
    /// 30..=32 Mark — 6 bytes header + 3 × 6-byte range records.
    fn class_def_bytes() -> Vec<u8> {
        let mut b = 2u16.to_be_bytes().to_vec(); // classFormat 2
        b.extend_from_slice(&3u16.to_be_bytes()); // classRangeCount
        for (start, end, class) in [(10u16, 10u16, 1u16), (20, 21, 2), (30, 32, 3)] {
            b.extend_from_slice(&start.to_be_bytes());
            b.extend_from_slice(&end.to_be_bytes());
            b.extend_from_slice(&class.to_be_bytes());
        }
        b
    }

    /// A GDEF table (v1.0 header, 12 bytes) whose `glyphClassDefOffset` points at
    /// `class_def_bytes()`; the other three offsets are null.
    fn build_gdef(class_def_off: u16) -> Vec<u8> {
        let mut t = 1u16.to_be_bytes().to_vec(); // majorVersion
        t.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        t.extend_from_slice(&class_def_off.to_be_bytes()); // glyphClassDefOffset
        t.extend_from_slice(&0u16.to_be_bytes()); // attachListOffset
        t.extend_from_slice(&0u16.to_be_bytes()); // ligCaretListOffset
        t.extend_from_slice(&0u16.to_be_bytes()); // markAttachClassDefOffset
        assert_eq!(t.len(), 12);
        if class_def_off != 0 {
            assert_eq!(
                usize::from(class_def_off),
                t.len(),
                "class def appended after header"
            );
            t.extend_from_slice(&class_def_bytes());
        }
        t
    }

    #[test]
    fn classifies_each_glyph_class() {
        let gdef = Gdef::parse(&build_gdef(12)).unwrap();
        assert!(gdef.has_glyph_classes());
        assert_eq!(gdef.glyph_class(10), GlyphClass::Base);
        assert_eq!(gdef.glyph_class(20), GlyphClass::Ligature);
        assert_eq!(gdef.glyph_class(21), GlyphClass::Ligature);
        assert_eq!(gdef.glyph_class(30), GlyphClass::Mark);
        assert_eq!(gdef.glyph_class(32), GlyphClass::Mark);
        assert!(gdef.is_mark(31), "in the mark range");
        assert!(!gdef.is_mark(10), "a base is not a mark");
        // Unlisted glyphs fall through to class 0.
        assert_eq!(gdef.glyph_class(99), GlyphClass::Unclassified);
        assert!(!gdef.is_mark(99));
    }

    #[test]
    fn absent_glyph_class_def_is_unclassified() {
        // glyphClassDefOffset 0 => no class table; every glyph is unclassified and
        // nothing is a mark (so the shaper falls back to its attach heuristic).
        let gdef = Gdef::parse(&build_gdef(0)).unwrap();
        assert!(!gdef.has_glyph_classes());
        assert_eq!(gdef.glyph_class(30), GlyphClass::Unclassified);
        assert!(!gdef.is_mark(30));
    }

    #[test]
    fn reject_major_version_other_than_one() {
        let mut t = build_gdef(12);
        t[0..2].copy_from_slice(&2u16.to_be_bytes());
        assert!(matches!(
            Gdef::parse(&t),
            Err(ParseError::UnsupportedTableVersion { major: 2, .. })
        ));
    }
}
