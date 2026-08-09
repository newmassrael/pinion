//! R1613 §5.16 §5.41 — **painting a byte dump**, from the geometry that
//! hit-tests it.
//!
//! R1606 lifted the *geometry* of a hex dump into
//! [`pinion_core::widgets::hex_dump`] and left the paint in the example, which
//! made the pair of a protocol inspector's two panes asymmetric: an
//! application that wanted the detail tree depended on
//! [`view_virtual_tree`](crate::tree_view::view_virtual_tree) and got a picture,
//! and an application that wanted the dump got a map and had to write the
//! cell assembly itself. Fifty-five lines is not a lot to copy, but what was
//! being copied was **a second statement of a layout the crate already
//! knows** — and a copied invariant drifts.
//!
//! ## Paint is a walk over the hit-test
//!
//! [`view_hex_dump`] asks [`HexLayout::region_at`] what each cell is and
//! [`HexLayout::glyph_at`] what it shows, which is the same pair of questions
//! a pointer asks. So a cell that draws a byte is a cell that selects that
//! byte, by construction rather than by two functions agreeing — the failure
//! this closes is the one where what is drawn and what responds are derived
//! from different facts.
//!
//! That is also why `region_at` had to become constant time: a painter calls
//! it once per cell, and it used to scan the row's bytes.
//!
//! ## What the application still owns: colour, and only colour
//!
//! The crate does not pick colours, the same way R1606 declined to pick the
//! separator between hex groups. The application declares
//! [`pinion_core::widgets::hex_dump::MarkSet`] — *named* byte runs,
//! no colour — and hands down a [`HexPalette`] for the cells that are not
//! marked plus a function from a mark's **name** to its [`MarkInk`].
//!
//! Keeping the name in the model and the colour in the application is what
//! makes "why is this byte lit" answerable: `names_at` returns the stack of
//! runs covering a byte in the order that decided it, where a list of
//! `(start, length, format)` decorations — what a mature toolkit offers over a
//! text layout — has thrown the name away by the time anyone can ask.

use pinion_core::widgets::hex_dump::{Cell, CellRole, HexLayout, Mark, MarkSet};
use pinion_core::{CellAttrs, GridBuffer, TermCell, TermColor};

/// The colours of a dump's unmarked cells.
///
/// Every field is a decision the application makes; the crate has no default
/// for any of them, because a dump inside a dark editor and a dump inside a
/// printed report disagree on all six.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexPalette {
    /// The offset column's digits.
    pub offset: TermColor,
    /// The two `|` bars fencing the ascii gutter.
    pub bar: TermColor,
    /// A hex digit.
    pub hex: TermColor,
    /// An ascii glyph the byte actually has.
    pub printable: TermColor,
    /// The `.` standing in for a byte with no printable glyph.
    pub nonprintable: TermColor,
    /// Behind everything not covered by a mark.
    pub background: TermColor,
}

impl HexPalette {
    /// Every role in the terminal's own default colour — the palette a caller
    /// starts from before deciding what to tint.
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            offset: TermColor::Default,
            bar: TermColor::Default,
            hex: TermColor::Default,
            printable: TermColor::Default,
            nonprintable: TermColor::Default,
            background: TermColor::Default,
        }
    }

    /// The same palette with the dim roles — the offset column, the gutter
    /// bars and the `.` — set to `muted`.
    ///
    /// The one grouping worth a helper: those three are the "chrome" of a dump
    /// and every caller so far tints them together.
    #[must_use]
    pub const fn with_muted(mut self, muted: TermColor) -> Self {
        self.offset = muted;
        self.bar = muted;
        self.nonprintable = muted;
        self
    }

    /// The colour a cell of this role takes before any mark is applied.
    ///
    /// `substituted` is the ascii gutter's one question: is this cell drawing
    /// a stand-in `.` rather than the byte's own glyph? It is the single place
    /// a cell's colour depends on the byte's *value*, because a `.` is chrome
    /// and the byte itself is content.
    ///
    /// **Derive it from the glyph, not from the byte's range.** The rule for
    /// which bytes have a glyph belongs to
    /// [`ascii_glyph`](pinion_core::widgets::hex_dump::ascii_glyph), and
    /// restating it here would be a second copy that can disagree — a
    /// counterfactual found exactly that: this crate's own `is printable`
    /// predicate could be narrowed to exclude the space, and nothing failed,
    /// because a space would then draw its own glyph in the colour that means
    /// "substituted". [`view_hex_dump`] asks `glyph != byte`, which cannot
    /// drift because there is nothing to drift from.
    ///
    /// **The match is exhaustive over [`CellRole`] on purpose.** A new region
    /// in the geometry becomes a new role, and a new role fails to compile
    /// here rather than falling into a wildcard.
    #[must_use]
    pub const fn role_ink(&self, role: CellRole, substituted: bool) -> TermColor {
        match role {
            CellRole::Offset => self.offset,
            CellRole::Hex => self.hex,
            CellRole::Ascii => {
                if substituted {
                    self.nonprintable
                } else {
                    self.printable
                }
            }
            CellRole::Bar => self.bar,
            CellRole::Blank => self.background,
        }
    }
}

/// How one mark's bytes are drawn.
///
/// Each channel is optional and **absent means "say nothing"**, which is what
/// lets marks stack: a run that only sets a background leaves the foreground
/// to whatever is underneath it, so "the header is tinted" and "the selection
/// is reversed" compose instead of one erasing the other.
///
/// Where two marks both speak to a channel, the later-declared one wins — one
/// direction, for every channel alike. See
/// [`pinion_core::widgets::hex_dump::MarkSet`] for why that is worth
/// stating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkInk {
    /// The glyph's colour, if this mark decides it.
    pub fg: Option<TermColor>,
    /// The cell's background, if this mark decides it.
    pub bg: Option<TermColor>,
    /// Whether the cell is reverse-video, if this mark decides it.
    pub reverse: Option<bool>,
}

impl MarkInk {
    /// A mark that changes nothing — what a name the application does not
    /// style resolves to.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            fg: None,
            bg: None,
            reverse: None,
        }
    }

    /// A mark that fills its bytes with `bg` and writes them in `fg`.
    #[must_use]
    pub const fn filled(fg: TermColor, bg: TermColor) -> Self {
        Self {
            fg: Some(fg),
            bg: Some(bg),
            reverse: None,
        }
    }

    /// A mark that reverse-videos its bytes and leaves the colours alone.
    #[must_use]
    pub const fn reversed() -> Self {
        Self {
            fg: None,
            bg: None,
            reverse: Some(true),
        }
    }

    /// This ink laid over `self` — every channel `other` decides replaces
    /// this one's, and the channels it is silent about are kept.
    #[must_use]
    pub const fn under(self, other: Self) -> Self {
        Self {
            fg: if other.fg.is_some() {
                other.fg
            } else {
                self.fg
            },
            bg: if other.bg.is_some() {
                other.bg
            } else {
                self.bg
            },
            reverse: if other.reverse.is_some() {
                other.reverse
            } else {
                self.reverse
            },
        }
    }
}

/// The ink every mark covering `byte` resolves to, folded in declaration
/// order.
///
/// Exposed because it is the assertion a caller wants: "this byte is drawn
/// like this, and [`MarkSet::names_at`] says which runs decided it".
pub fn ink_at<F>(marks: &MarkSet, byte: usize, ink_for: F) -> MarkInk
where
    F: Fn(&str) -> MarkInk,
{
    marks
        .at(byte)
        .map(Mark::name)
        .fold(MarkInk::none(), |acc, name| acc.under(ink_for(name)))
}

/// The dump as a [`GridBuffer`]: one cell per column per row, every one of
/// them classified by `layout` and filled from `bytes`.
///
/// `ink_for` maps a mark's name to how its bytes are drawn; a name it does not
/// recognise should answer [`MarkInk::none`], which draws that run in the
/// palette like anything else. Marks over bytes the buffer does not have are
/// simply never reached.
///
/// The grid is exactly [`HexLayout::total_cols`] by [`HexLayout::rows`], so a
/// caller sizing a `Scene::TextGrid` reads both off the same declaration it
/// hit-tests with.
pub fn view_hex_dump<F>(
    layout: &HexLayout,
    bytes: &[u8],
    marks: &MarkSet,
    palette: &HexPalette,
    ink_for: F,
) -> GridBuffer
where
    F: Fn(&str) -> MarkInk,
{
    let cols = u16::try_from(layout.total_cols()).unwrap_or(u16::MAX);
    let rows = u16::try_from(layout.rows()).unwrap_or(u16::MAX);
    let mut buffer = GridBuffer::new(cols, rows);
    for row in 0..usize::from(rows) {
        let mut cells = Vec::with_capacity(usize::from(cols));
        for col in 0..usize::from(cols) {
            let cell = Cell::new(col, row);
            let region = layout.region_at(cell);
            let glyph = layout.glyph_at(bytes, cell);
            // The gutter draws a `.` for a byte with no glyph of its own, and
            // that substitution is what the `nonprintable` role means. Asking
            // whether the glyph IS the byte reads the geometry's own rule
            // rather than restating it.
            let substituted = match region.role() {
                CellRole::Ascii => region
                    .byte()
                    .and_then(|byte| bytes.get(byte))
                    .is_none_or(|value| char::from(*value) != glyph),
                _ => false,
            };
            let base_fg = palette.role_ink(region.role(), substituted);
            let ink = match region.byte() {
                Some(byte) => ink_at(marks, byte, &ink_for),
                None => MarkInk::none(),
            };
            let attrs = CellAttrs::empty().with_reverse(ink.reverse.unwrap_or(false));
            cells.push(
                TermCell::new(
                    glyph.to_string(),
                    ink.fg.unwrap_or(base_fg),
                    ink.bg.unwrap_or(palette.background),
                )
                .with_attrs(attrs),
            );
        }
        buffer = buffer.with_row(u16::try_from(row).unwrap_or(u16::MAX), cells);
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::hex_dump::{Nibble, Region};

    /// The palette every test below reads assertions off — six distinguishable
    /// roles, so a cell drawn with the wrong one is a failing assertion rather
    /// than a coincidence.
    fn palette() -> HexPalette {
        HexPalette {
            offset: TermColor::Indexed(1),
            bar: TermColor::Indexed(2),
            hex: TermColor::Indexed(3),
            printable: TermColor::Indexed(4),
            nonprintable: TermColor::Indexed(5),
            background: TermColor::Indexed(6),
        }
    }

    fn sample() -> Vec<u8> {
        // A space is deliberately in here: it is printable and it is the byte
        // a narrower "is printable" rule drops first, which is what a
        // counterfactual caught this fixture failing to separate.
        let mut bytes = b"HEAD".to_vec();
        bytes.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        bytes.extend_from_slice(b"pay load!");
        bytes.extend_from_slice(&[0xff; 15]);
        bytes
    }

    fn cell_at(buffer: &GridBuffer, cell: Cell) -> TermCell {
        buffer
            .cell(
                u16::try_from(cell.col).expect("col fits"),
                u16::try_from(cell.row).expect("row fits"),
            )
            .expect("inside the grid")
            .clone()
    }

    #[test]
    fn the_grid_is_exactly_the_layouts_extent() {
        let bytes = sample();
        let layout = HexLayout::new(bytes.len());
        let buffer = view_hex_dump(&layout, &bytes, &MarkSet::new(), &palette(), |_| {
            MarkInk::none()
        });
        assert_eq!(usize::from(buffer.cols()), layout.total_cols());
        assert_eq!(usize::from(buffer.rows()), layout.rows());
    }

    #[test]
    fn every_cell_draws_what_the_hit_test_says_it_is() {
        // ★ The property the module exists for: paint and hit-test read ONE
        // fact. Every cell of the painted grid is checked against the region
        // the pointer would resolve at that same cell.
        let bytes = sample();
        for layout in [
            HexLayout::new(bytes.len()),
            HexLayout::new(bytes.len())
                .with_bytes_per_row(8)
                .with_group(4),
            HexLayout::new(bytes.len())
                .with_offset_digits(4)
                .with_gutter(1),
        ] {
            let buffer = view_hex_dump(&layout, &bytes, &MarkSet::new(), &palette(), |_| {
                MarkInk::none()
            });
            for row in 0..layout.rows() {
                for col in 0..layout.total_cols() {
                    let at = Cell::new(col, row);
                    let painted = cell_at(&buffer, at);
                    let glyph = layout.glyph_at(&bytes, at);
                    assert_eq!(painted.cluster, glyph.to_string(), "{layout:?} {at} glyph");
                    let region = layout.region_at(at);
                    let expected_fg = match region.role() {
                        CellRole::Offset => palette().offset,
                        CellRole::Bar => palette().bar,
                        CellRole::Hex => palette().hex,
                        CellRole::Ascii => {
                            // ★ The expectation is derived from what the
                            // GLYPH is, not from this crate's own `printable`
                            // -- otherwise the assertion and the code under it
                            // would be the same fact and a divergence between
                            // `printable` and the geometry's `ascii_glyph`
                            // would paint a substituted `.` in the colour that
                            // says "this is the byte itself".
                            let byte = region.byte().expect("an ascii cell names its byte");
                            if glyph == bytes[byte] as char {
                                palette().printable
                            } else {
                                palette().nonprintable
                            }
                        }
                        CellRole::Blank => palette().background,
                    };
                    assert_eq!(painted.fg, expected_fg, "{layout:?} {at} fg");
                    assert_eq!(painted.bg, palette().background, "{layout:?} {at} bg");
                }
            }
        }
    }

    #[test]
    fn a_marks_ink_lands_on_both_of_a_bytes_regions() {
        // ★ The linked highlight: one byte, two disjoint places in the grid,
        // and the mark reaches both. This is the assertion the example's own
        // paint could only make about itself.
        let bytes = sample();
        let layout = HexLayout::new(bytes.len());
        let marks = MarkSet::new().marking("field", 4, 8);
        let buffer = view_hex_dump(&layout, &bytes, &marks, &palette(), |name| {
            assert_eq!(name, "field");
            MarkInk::filled(TermColor::Indexed(20), TermColor::Indexed(21))
        });
        for byte in 0..bytes.len() {
            let lit = (4..8).contains(&byte);
            let hex = layout.hex_cell(byte).expect("inside");
            let ascii = layout.ascii_cell(byte).expect("inside");
            for at in [hex, Cell::new(hex.col + 1, hex.row), ascii] {
                let painted = cell_at(&buffer, at);
                if lit {
                    assert_eq!(painted.bg, TermColor::Indexed(21), "byte {byte} at {at}");
                    assert_eq!(painted.fg, TermColor::Indexed(20), "byte {byte} at {at}");
                } else {
                    assert_eq!(painted.bg, palette().background, "byte {byte} at {at}");
                }
            }
        }
    }

    #[test]
    fn overlapping_marks_paint_in_declaration_order_and_say_why() {
        // ★ One direction for every channel, and the reason is queryable.
        let bytes = sample();
        let layout = HexLayout::new(bytes.len());
        let marks = MarkSet::new()
            .marking("frame", 0, 24)
            .marking("header", 0, 8)
            .marking("length", 4, 8);
        let ink_for = |name: &str| match name {
            "frame" => MarkInk {
                bg: Some(TermColor::Indexed(30)),
                ..MarkInk::none()
            },
            "header" => MarkInk::filled(TermColor::Indexed(31), TermColor::Indexed(32)),
            "length" => MarkInk {
                fg: Some(TermColor::Indexed(33)),
                ..MarkInk::none()
            },
            _ => MarkInk::none(),
        };
        let buffer = view_hex_dump(&layout, &bytes, &marks, &palette(), ink_for);

        // Byte 5 is in all three. The last mark to speak to a channel wins it,
        // so the foreground is `length`'s and the background is `header`'s --
        // `length` is silent about background and does NOT fall back past it.
        let at = layout.hex_cell(5).expect("inside");
        let painted = cell_at(&buffer, at);
        assert_eq!(painted.fg, TermColor::Indexed(33), "length decides the ink");
        assert_eq!(
            painted.bg,
            TermColor::Indexed(32),
            "header decides the fill; length said nothing about it"
        );
        assert_eq!(marks.names_at(5), vec!["frame", "header", "length"]);

        // Byte 2: frame and header, no length -- header decides both channels.
        let at = layout.hex_cell(2).expect("inside");
        let painted = cell_at(&buffer, at);
        assert_eq!(painted.fg, TermColor::Indexed(31));
        assert_eq!(painted.bg, TermColor::Indexed(32));
        assert_eq!(marks.names_at(2), vec!["frame", "header"]);

        // Byte 20: frame alone -- which sets only a background, so the
        // foreground stays the palette's.
        let at = layout.hex_cell(20).expect("inside");
        let painted = cell_at(&buffer, at);
        assert_eq!(painted.fg, palette().hex);
        assert_eq!(painted.bg, TermColor::Indexed(30));
    }

    #[test]
    fn ink_at_is_the_fold_the_painter_uses() {
        let marks = MarkSet::new()
            .marking("a", 0, 8)
            .marking("b", 4, 12)
            .marking("quiet", 0, 16);
        let ink_for = |name: &str| match name {
            "a" => MarkInk::filled(TermColor::Indexed(1), TermColor::Indexed(2)),
            "b" => MarkInk {
                bg: Some(TermColor::Indexed(3)),
                ..MarkInk::none()
            },
            _ => MarkInk::none(),
        };
        // Overlap: `b` overrides the background, `a` keeps the foreground, and
        // a mark with no ink at all changes nothing even though it is last.
        assert_eq!(
            ink_at(&marks, 5, ink_for),
            MarkInk {
                fg: Some(TermColor::Indexed(1)),
                bg: Some(TermColor::Indexed(3)),
                reverse: None,
            }
        );
        assert_eq!(
            ink_at(&marks, 2, ink_for),
            MarkInk::filled(TermColor::Indexed(1), TermColor::Indexed(2))
        );
        assert_eq!(ink_at(&marks, 14, ink_for), MarkInk::none());
        assert_eq!(ink_at(&marks, 99, ink_for), MarkInk::none());
    }

    #[test]
    fn reverse_video_is_a_channel_like_the_others() {
        let bytes = sample();
        let layout = HexLayout::new(bytes.len());
        let marks = MarkSet::new()
            .marking("field", 0, 12)
            .marking("selection", 8, 16);
        let buffer = view_hex_dump(&layout, &bytes, &marks, &palette(), |name| match name {
            "field" => MarkInk::filled(TermColor::Indexed(40), TermColor::Indexed(41)),
            "selection" => MarkInk::reversed(),
            _ => MarkInk::none(),
        });
        // A byte in both: the fill stays (selection says nothing about colour)
        // and the reverse rides on top -- the two gestures compose.
        let both = cell_at(&buffer, layout.hex_cell(9).expect("inside"));
        assert_eq!(both.bg, TermColor::Indexed(41));
        assert!(both.attrs.reverse);
        let brushed_only = cell_at(&buffer, layout.hex_cell(2).expect("inside"));
        assert_eq!(brushed_only.bg, TermColor::Indexed(41));
        assert!(!brushed_only.attrs.reverse);
        let selected_only = cell_at(&buffer, layout.hex_cell(13).expect("inside"));
        assert_eq!(selected_only.bg, palette().background);
        assert!(selected_only.attrs.reverse);
    }

    #[test]
    fn a_narrow_offset_column_prints_the_rows_own_offset() {
        // ★ The defect the lift closes at its source: the example formatted
        // the offset to a fixed eight digits and took the first
        // `offset_digits` of the string, so every row of a four-digit column
        // read `0000`.
        let bytes = vec![0u8; 512];
        let layout = HexLayout::new(bytes.len()).with_offset_digits(4);
        let buffer = view_hex_dump(&layout, &bytes, &MarkSet::new(), &palette(), |_| {
            MarkInk::none()
        });
        let row = 3;
        let digits: String = (0..4)
            .map(|col| cell_at(&buffer, Cell::new(col, row)).cluster.into_owned())
            .collect();
        assert_eq!(digits, "0030", "row 3 begins at byte 0x30");
        assert_ne!(digits, "0000");
    }

    #[test]
    fn a_buffer_shorter_than_the_layout_paints_blanks_rather_than_panicking() {
        let layout = HexLayout::new(64);
        let bytes = [0xab_u8; 3];
        let buffer = view_hex_dump(
            &layout,
            &bytes,
            &MarkSet::new().marking("gone", 40, 60),
            &palette(),
            |_| MarkInk::filled(TermColor::Indexed(9), TermColor::Indexed(9)),
        );
        let present = cell_at(&buffer, layout.hex_cell(2).expect("inside"));
        assert_eq!(present.cluster, "a");
        let missing = cell_at(&buffer, layout.hex_cell(40).expect("inside"));
        assert_eq!(missing.cluster, " ", "no byte, no glyph");
        assert_eq!(
            layout.region_at(layout.hex_cell(40).expect("inside")),
            Region::Hex {
                byte: 40,
                nibble: Nibble::High
            },
            "the CELL still belongs to that byte -- only the buffer is short"
        );
    }
}
