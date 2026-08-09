//! R1606 §5.41 — **the geometry of a byte dump**, as a value.
//!
//! A hex dump is the other half of a protocol inspector's window: the detail
//! tree ([`row_dissect`](crate::widgets::row_dissect)) says what a field
//! *means*, and this says where its bytes *are*. Both are needed, and only one
//! of them was in a crate — the dump lived entirely inside
//! `examples/hello-hex-dump` (1,477 lines), so an application that wanted one
//! had to copy it. Copying a renderer is a nuisance; copying a **geometry** is
//! a fork, because the invariants go with it and then drift.
//!
//! ## What is here, and what is not
//!
//! Here: the map between a byte offset and the cells that show it, in **both
//! directions**, plus the classification of every other cell. Not here: colour,
//! fonts, the buffer, what a byte *means*, and any notion of a renderer. This
//! is integer arithmetic over a declaration, so a dump's rules are testable
//! without a window.
//!
//! ## The one thing that makes it worth a type
//!
//! A byte appears in **two disjoint places** — its two hex digits and its one
//! ascii glyph — so a dump needs `byte -> cell` for painting and
//! `cell -> byte` for hit-testing, and **the two must not be able to
//! disagree**. Before this module they were three hand-written functions
//! (`hex_col`, `ascii_col`, `byte_at_cell`) that agreed by inspection, beside
//! **five hand-computed column constants** (`ASCII_BAR_L = 60`,
//! `ASCII_START = 61`, `ASCII_BAR_R = 77`, `TOTAL_COLS = 78`, `HEX_START = 10`)
//! that a person had derived once, on paper, from a 16-byte row split into two
//! groups of 8. Change the row width and every one of those numbers is wrong
//! with nothing to say so.
//!
//! Here the declaration is [`HexLayout`] — bytes per row, bytes per group,
//! offset digits, gutter width — and **every column is derived from it**,
//! including the inverse: [`HexLayout::region_at`] is the only place that reads
//! a cell, and [`HexLayout::hex_cell`] / [`HexLayout::ascii_cell`] are checked
//! against it by a round-trip property over a range of layouts, so a widened
//! row cannot silently break the hit-test.
//!
//! ## Past the references
//!
//! The toolkit has no hex view at all — no widget, no geometry, nothing to be
//! a floor — so the *existence* is not parity with anything and the *shape* is
//! chosen. Two choices are worth naming against what a hex editor normally
//! does:
//!
//! * **A cell that is not a byte says WHAT it is.** [`Region`] names the offset
//!   field, the gutter bars and the padding apart from each other, where the
//!   example this replaces answered `Option<usize>` — so "you clicked the
//!   offset column" and "you clicked the gap between the two groups" were the
//!   same value, and an editor wanting to select a *row* by its offset had
//!   nothing to read.
//! * **A hex cell names its nibble.** A click lands on the high or the low
//!   digit of a byte, and that is the difference between overtyping one nibble
//!   and overtyping the byte — the thing a hex *editor* is for. The mapping
//!   answers it rather than rounding both digits to the byte.

use std::fmt;

/// One cell of the dump grid.
///
/// Column-major-free: a cell is a pair, and which pair belongs to which byte is
/// [`HexLayout`]'s business rather than the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    /// Zero-based column.
    pub col: usize,
    /// Zero-based row.
    pub row: usize,
}

impl Cell {
    /// A cell at `(col, row)`.
    #[must_use]
    pub const fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.col, self.row)
    }
}

/// Which half of a byte a hex cell shows.
///
/// Named rather than folded into the byte index, because overtyping one nibble
/// and overtyping a byte are different edits and only the cell says which the
/// pointer asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nibble {
    /// The left digit — bits 4..8.
    High,
    /// The right digit — bits 0..4.
    Low,
}

impl Nibble {
    /// The nibble's value in `byte`.
    #[must_use]
    pub const fn of(self, byte: u8) -> u8 {
        match self {
            Self::High => byte >> 4,
            Self::Low => byte & 0x0f,
        }
    }
}

/// What a cell of the dump is.
///
/// Total over the grid: every cell inside `0..total_cols` x `0..rows` is one of
/// these, so a hit-test has no unclassified answer to fall through to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Region {
    /// A digit of the row's offset field.
    Offset {
        /// The byte offset the row begins at.
        at: usize,
        /// Which digit, most significant first.
        digit: usize,
    },
    /// A hex digit of a byte.
    Hex {
        /// The byte's index in the buffer.
        byte: usize,
        /// Which of its two digits.
        nibble: Nibble,
    },
    /// A byte's ascii glyph.
    Ascii {
        /// The byte's index in the buffer.
        byte: usize,
    },
    /// One of the two `|` bars fencing the ascii gutter.
    Bar,
    /// A cell inside the grid that shows nothing — the gaps after a hex pair,
    /// the extra space between groups, and the tail of a short final row.
    Padding,
    /// Outside the grid entirely.
    Outside,
}

/// What a painter has to decide about a cell.
///
/// [`Region`] answers *what a cell is*, which is the hit-test's question and
/// grows over time — it is `#[non_exhaustive]` for exactly that reason. A
/// painter's question is narrower and closed: which of a fixed set of roles
/// does this cell take its ink from.
///
/// **Splitting them is what keeps a new region from going unpainted.** A
/// painter matching `Region` directly would need a `_` arm, and a `_` arm
/// silently absorbs the next variant — the failure recorded in
/// `debt-node-body-arm-is-weaker-than-a-type`. Here [`Region::role`] is an
/// exhaustive match *inside this module*, so a new region cannot compile
/// without saying which role it paints as, and this enum is deliberately NOT
/// `#[non_exhaustive]`, so adding a role breaks every painter loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellRole {
    /// A digit of the row's offset field.
    Offset,
    /// A hex digit of a byte.
    Hex,
    /// A byte's ascii glyph.
    Ascii,
    /// A gutter bar.
    Bar,
    /// A cell that shows nothing.
    Blank,
}

impl Region {
    /// Which ink role this cell takes.
    ///
    /// The one place [`Region`] is collapsed for painting; see [`CellRole`]
    /// for why the collapse is a named step rather than a `_` arm in every
    /// painter.
    #[must_use]
    pub const fn role(&self) -> CellRole {
        match self {
            Self::Offset { .. } => CellRole::Offset,
            Self::Hex { .. } => CellRole::Hex,
            Self::Ascii { .. } => CellRole::Ascii,
            Self::Bar => CellRole::Bar,
            Self::Padding | Self::Outside => CellRole::Blank,
        }
    }

    /// The byte this cell shows, if it shows one.
    ///
    /// The convenience the old hand-written hit-test answered *instead* of the
    /// region; keeping both means a caller that only wants the byte does not
    /// have to match, and one that wants to know it hit the offset column can.
    #[must_use]
    pub const fn byte(&self) -> Option<usize> {
        match self {
            Self::Hex { byte, .. } | Self::Ascii { byte } => Some(*byte),
            _ => None,
        }
    }
}

/// How a byte buffer is laid out in cells.
///
/// The declaration, from which every column is derived. Construct with
/// [`HexLayout::new`] and adjust with the builders; the defaults are the
/// classic `xxd` dump — 16 bytes a row in two groups of 8, an 8-digit offset,
/// two spaces between regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexLayout {
    len: usize,
    bytes_per_row: usize,
    group: usize,
    offset_digits: usize,
    gutter: usize,
}

impl HexLayout {
    /// A dump of `len` bytes in the classic shape.
    #[must_use]
    pub const fn new(len: usize) -> Self {
        Self {
            len,
            bytes_per_row: 16,
            group: 8,
            offset_digits: 8,
            gutter: 2,
        }
    }

    /// How many bytes a row shows. Clamped to at least one, because a row of
    /// zero bytes has no rows and no inverse.
    #[must_use]
    pub const fn with_bytes_per_row(mut self, bytes: usize) -> Self {
        self.bytes_per_row = if bytes == 0 { 1 } else { bytes };
        self
    }

    /// How many bytes share a group. An extra space falls between groups, which
    /// is what makes a 16-wide row readable. A group at or above the row width
    /// means one group and no extra spaces.
    #[must_use]
    pub const fn with_group(mut self, bytes: usize) -> Self {
        self.group = if bytes == 0 { 1 } else { bytes };
        self
    }

    /// How many hex digits the offset field shows.
    #[must_use]
    pub const fn with_offset_digits(mut self, digits: usize) -> Self {
        self.offset_digits = digits;
        self
    }

    /// How many blank columns fence each region.
    #[must_use]
    pub const fn with_gutter(mut self, columns: usize) -> Self {
        self.gutter = columns;
        self
    }

    /// The buffer length this layout was built for.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty — then there are no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes per row.
    #[must_use]
    pub const fn bytes_per_row(&self) -> usize {
        self.bytes_per_row
    }

    /// How many hex digits the offset field shows.
    #[must_use]
    pub const fn offset_digits(&self) -> usize {
        self.offset_digits
    }

    /// How many blank columns fence each region.
    #[must_use]
    pub const fn gutter(&self) -> usize {
        self.gutter
    }

    /// How many rows the buffer needs.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.len.div_ceil(self.bytes_per_row)
    }

    /// How many groups a full row is split into.
    #[must_use]
    pub const fn groups(&self) -> usize {
        self.bytes_per_row.div_ceil(self.group)
    }

    /// The first hex column — past the offset field and its gutter.
    #[must_use]
    pub const fn hex_start(&self) -> usize {
        self.offset_digits + self.gutter
    }

    /// One past the last column any hex pair can occupy.
    ///
    /// Each byte takes two digits and a trailing space, and each group boundary
    /// after the first takes one more space.
    #[must_use]
    pub const fn hex_end(&self) -> usize {
        self.hex_start() + self.bytes_per_row * 3 + (self.groups() - 1)
    }

    /// The left `|` of the ascii gutter.
    #[must_use]
    pub const fn ascii_bar_left(&self) -> usize {
        // `- 1` because the last hex pair's trailing space is already a blank
        // column, so the gutter is measured from the digits rather than from it.
        self.hex_end() - 1 + self.gutter
    }

    /// The first ascii column.
    #[must_use]
    pub const fn ascii_start(&self) -> usize {
        self.ascii_bar_left() + 1
    }

    /// The right `|` of the ascii gutter.
    #[must_use]
    pub const fn ascii_bar_right(&self) -> usize {
        self.ascii_start() + self.bytes_per_row
    }

    /// How many columns the grid is wide.
    #[must_use]
    pub const fn total_cols(&self) -> usize {
        self.ascii_bar_right() + 1
    }

    /// Where in a row the `j`-th byte's hex digits begin.
    #[must_use]
    pub const fn hex_col(&self, index_in_row: usize) -> usize {
        self.hex_start() + index_in_row * 3 + index_in_row / self.group
    }

    /// Where in a row the `j`-th byte's ascii glyph is.
    #[must_use]
    pub const fn ascii_col(&self, index_in_row: usize) -> usize {
        self.ascii_start() + index_in_row
    }

    /// The cell holding `byte`'s high hex digit, or `None` past the buffer.
    #[must_use]
    pub const fn hex_cell(&self, byte: usize) -> Option<Cell> {
        if byte >= self.len {
            return None;
        }
        Some(Cell::new(
            self.hex_col(byte % self.bytes_per_row),
            byte / self.bytes_per_row,
        ))
    }

    /// The cell holding `byte`'s ascii glyph, or `None` past the buffer.
    #[must_use]
    pub const fn ascii_cell(&self, byte: usize) -> Option<Cell> {
        if byte >= self.len {
            return None;
        }
        Some(Cell::new(
            self.ascii_col(byte % self.bytes_per_row),
            byte / self.bytes_per_row,
        ))
    }

    /// How many columns one whole group of hex digits occupies.
    ///
    /// Three per byte — two digits and the space after them — plus the one
    /// extra space that separates this group from the next. It is what makes
    /// the inverse a division instead of a search.
    #[must_use]
    const fn group_stride(&self) -> usize {
        self.group * 3 + 1
    }

    /// What `cell` is.
    ///
    /// **The only place a cell is read**, which is what keeps the forward and
    /// the reverse map from drifting: `hex_cell` and `ascii_cell` are checked
    /// against this by a round-trip property rather than by inspection.
    ///
    /// ## Constant time, because paint calls it once per cell
    ///
    /// This started as a scan of the row's bytes, which is fine for a
    /// hit-test — a pointer asks about one cell — and quadratic for the thing
    /// that came next. A painter that renders the dump by *asking what each
    /// cell is* walks `total_cols * rows` cells, so a scan makes the whole
    /// render `cells * bytes_per_row`, and a dump wide enough to be worth
    /// paging is exactly where that bites.
    ///
    /// So the inverse is closed form. Every group of hex digits occupies a
    /// fixed stride of columns, so the group is a division and the byte
    /// inside it is another; the remainder mod three says high digit, low
    /// digit, or the space after the pair. The
    /// `the_closed_form_inverse_agrees_with_a_scan` property holds it to the
    /// scan it replaced, cell for cell, over every layout shape.
    #[must_use]
    pub const fn region_at(&self, cell: Cell) -> Region {
        if cell.row >= self.rows() || cell.col >= self.total_cols() {
            return Region::Outside;
        }
        let row_start = cell.row * self.bytes_per_row;
        if cell.col < self.offset_digits {
            return Region::Offset {
                at: row_start,
                digit: cell.col,
            };
        }
        if cell.col == self.ascii_bar_left() || cell.col == self.ascii_bar_right() {
            return Region::Bar;
        }
        // How many bytes this row actually has — a short final row's tail is
        // padding rather than a byte nobody can click.
        let in_row = if self.len - row_start < self.bytes_per_row {
            self.len - row_start
        } else {
            self.bytes_per_row
        };
        if cell.col >= self.ascii_start() && cell.col < self.ascii_bar_right() {
            let index = cell.col - self.ascii_start();
            if index < in_row {
                return Region::Ascii {
                    byte: row_start + index,
                };
            }
            return Region::Padding;
        }
        if cell.col >= self.hex_start() && cell.col < self.hex_end() {
            let rel = cell.col - self.hex_start();
            let group = rel / self.group_stride();
            let within = rel % self.group_stride();
            let index = group * self.group + within / 3;
            // `within / 3 == self.group` is the extra space between groups, and
            // `within % 3 == 2` is the space after a pair; both are padding.
            if within / 3 < self.group && within % 3 < 2 && index < in_row {
                return Region::Hex {
                    byte: row_start + index,
                    nibble: if within % 3 == 0 {
                        Nibble::High
                    } else {
                        Nibble::Low
                    },
                };
            }
        }
        Region::Padding
    }

    /// The byte `cell` shows, if any — the hit-test a pointer resolves.
    #[must_use]
    pub const fn byte_at(&self, cell: Cell) -> Option<usize> {
        self.region_at(cell).byte()
    }

    /// The character `cell` shows.
    ///
    /// **The dump's glyphs, derived from the same [`Self::region_at`] the
    /// hit-test uses** — so what is drawn and what answers a pointer cannot be
    /// two facts. A painter is then a walk over the grid asking this, and the
    /// only thing left for the application is colour.
    ///
    /// A cell showing nothing — padding, or a byte past the end of `bytes` —
    /// is a space. `Outside` is a space too, which is what lets a caller paint
    /// a grid larger than the layout without a bounds test of its own.
    ///
    /// ## The offset field is derived, not truncated
    ///
    /// The offset digits are computed from the row's own start, most
    /// significant first, at whatever width [`Self::offset_digits`] declares.
    /// The example this replaced formatted the offset to a **fixed eight**
    /// digits and then took the first `offset_digits` of that string, so any
    /// width below eight printed the leading zeros of an eight-digit number —
    /// a four-digit offset column read `0000` on every row. An offset wider
    /// than the field shows its low digits, as a fixed-width field must.
    #[must_use]
    pub fn glyph_at(&self, bytes: &[u8], cell: Cell) -> char {
        match self.region_at(cell) {
            Region::Offset { at, digit } => self.offset_digit(at, digit),
            Region::Hex { byte, nibble } => bytes
                .get(byte)
                .map_or(' ', |value| hex_digit(nibble.of(*value))),
            Region::Ascii { byte } => bytes.get(byte).map_or(' ', |value| ascii_glyph(*value)),
            Region::Bar => '|',
            Region::Padding | Region::Outside => ' ',
        }
    }

    /// Digit `digit` of the offset `at`, most significant first, in a field of
    /// [`Self::offset_digits`] digits.
    ///
    /// A digit past the field's width is a space rather than a panic, so a
    /// [`Region::Offset`] a caller built by hand cannot crash a paint.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the mask bounds the value to 0..=15 one line above the cast, \
                  so the u8 is exact; `u8::try_from` is not const"
    )]
    pub const fn offset_digit(&self, at: usize, digit: usize) -> char {
        if digit >= self.offset_digits {
            return ' ';
        }
        let shift = 4 * (self.offset_digits - 1 - digit);
        if shift >= usize::BITS as usize {
            return '0';
        }
        hex_digit(((at >> shift) & 0x0f) as u8)
    }

    /// The byte range a `[low, high]` pair of buffer fractions covers.
    ///
    /// What an overview brush hands down. Ordered and clamped, so a drag in
    /// either direction is the same window and neither end can leave the
    /// buffer.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a buffer offset round-trips through f32 exactly far past any \
                  dump a person reads; the result is clamped either way"
    )]
    pub fn window(&self, low: f32, high: f32) -> (usize, usize) {
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        let scale = self.len as f32;
        let lo = (low.max(0.0) * scale).floor() as usize;
        let hi = (high.max(0.0) * scale).ceil() as usize;
        (lo.min(self.len), hi.min(self.len))
    }
}

/// The hex digit of a nibble value, lowercase.
///
/// Takes the low four bits, so a caller cannot pass a whole byte and get two
/// characters or a panic.
#[must_use]
pub const fn hex_digit(nibble: u8) -> char {
    match nibble & 0x0f {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        _ => 'f',
    }
}

/// What a byte shows in the ascii gutter — itself if it is printable ASCII,
/// otherwise `.`.
///
/// The `0x20..=0x7e` rule every dump uses, stated once. Deliberately not
/// locale- or encoding-aware: the gutter's job is to make ASCII runs visible
/// inside binary, and a decoder that knows better belongs above this.
#[must_use]
pub const fn ascii_glyph(byte: u8) -> char {
    if byte.is_ascii_graphic() || byte == b' ' {
        byte as char
    } else {
        '.'
    }
}

/// A contiguous run of selected bytes, `[start, end)`.
///
/// Canonical by construction: `start <= end`, both inside the buffer. A drag
/// latches an anchor and moves a focus, either of which may be the lower end,
/// so the type stores the *run* and remembers the focus separately — which is
/// what lets a caller ring the byte under the cursor without also having to
/// know which way the drag went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSelection {
    start: usize,
    end: usize,
    focus: usize,
}

impl ByteSelection {
    /// The collapsed selection at `byte` — a plain click.
    #[must_use]
    pub const fn at(byte: usize) -> Self {
        Self {
            start: byte,
            end: byte + 1,
            focus: byte,
        }
    }

    /// The selection a drag from `anchor` to `focus` makes, both inclusive.
    #[must_use]
    pub const fn drag(anchor: usize, focus: usize) -> Self {
        let (start, end) = if anchor <= focus {
            (anchor, focus + 1)
        } else {
            (focus, anchor + 1)
        };
        Self { start, end, focus }
    }

    /// First selected byte.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// One past the last selected byte.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// The byte the cursor is on — inside the run, and not necessarily an end
    /// of it after a drag reverses.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// How many bytes are selected. Never zero: the collapsed case is one byte.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the run holds no bytes. Only reachable by clamping a selection
    /// off the end of a buffer.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Whether `byte` is in the run.
    #[must_use]
    pub const fn contains(&self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }

    /// The same selection trimmed to a buffer of `len` bytes.
    ///
    /// A document can shrink under a selection — a capture is re-read, a
    /// smaller frame is chosen — and the honest answer is a shorter run rather
    /// than a panic or a run naming bytes that are gone.
    #[must_use]
    pub const fn clamped(self, len: usize) -> Self {
        let end = if self.end < len { self.end } else { len };
        let start = if self.start < end { self.start } else { end };
        let focus = if self.focus < end {
            self.focus
        } else if end == 0 {
            0
        } else {
            end - 1
        };
        Self { start, end, focus }
    }

    /// The selected bytes of `buffer` as hex pairs joined by `separator`.
    ///
    /// **The separator is the caller's**, and that is not indecision: the two
    /// real uses want different ones. A clipboard payload meant to be pasted
    /// into code wants `""` or `", 0x"`; a status readout wants `" "`. A crate
    /// that picked one would be making a presentation decision on behalf of
    /// every consumer — and it would have silently changed one, which is how
    /// this signature was chosen (the first draft defaulted to a space and the
    /// existing consumer's wire contract was `""`).
    ///
    /// Empty for an empty run, and it takes whatever of the run the buffer
    /// actually has, so a stale selection cannot index past it.
    #[must_use]
    pub fn hex(&self, buffer: &[u8], separator: &str) -> String {
        let end = self.end.min(buffer.len());
        if self.start >= end {
            return String::new();
        }
        let mut out = String::with_capacity((end - self.start) * (2 + separator.len()));
        for (index, byte) in buffer[self.start..end].iter().enumerate() {
            if index > 0 {
                out.push_str(separator);
            }
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(*byte));
        }
        out
    }
}

impl fmt::Display for ByteSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A named run of bytes, `[start, end)`.
///
/// The thing an inspector actually has: not "these bytes are blue" but "these
/// bytes are the length field". The name is the point — it is what a caller
/// keys colour off, what an assistant reads back over the wire, and what
/// [`MarkSet::names_at`] answers with when someone asks *why* a byte is lit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    name: String,
    start: usize,
    end: usize,
}

impl Mark {
    /// A mark called `name` over `[start, end)`. An inverted or empty range is
    /// stored as it was ordered, then reports [`Mark::is_empty`].
    #[must_use]
    pub fn new(name: impl Into<String>, start: usize, end: usize) -> Self {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            name: name.into(),
            start,
            end,
        }
    }

    /// The mark over the bytes a [`ByteSelection`] holds.
    #[must_use]
    pub fn of_selection(name: impl Into<String>, selection: ByteSelection) -> Self {
        Self::new(name, selection.start(), selection.end())
    }

    /// What this run is called.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// First marked byte.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// One past the last marked byte.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// How many bytes the run covers.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the run covers no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Whether `byte` is in the run.
    #[must_use]
    pub const fn contains(&self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }
}

/// The marks over one buffer, in declaration order.
///
/// ## Overlap resolves in ONE direction, and the direction is queryable
///
/// Marks overlap constantly — a packet's length field is inside its header is
/// inside the frame — so a byte can carry several, and something has to decide
/// which one a painter obeys. Two decisions are made here and both are
/// deliberate.
///
/// **Declaration order, later wins, for every visual channel alike.** A caller
/// that declares the frame, then the header, then the field gets the field on
/// top, which is the order it wrote them in. The reference for this is a
/// mature toolkit's list of range decorations over a text layout, and it is
/// worth naming what is different: there the ordering is *split by channel* —
/// a later range's background overpaints an earlier one, while a later range's
/// foreground is suppressed wherever an earlier one already drew text, so
/// background is last-wins and foreground is first-wins **in the same loop**.
/// Two directions in one list is a rule nobody can hold, and it is not written
/// down anywhere in that interface. One direction, stated, is the choice here.
///
/// **A mark carries a name and no colour.** The same reference's range
/// decoration is `(start, length, format)` — the format *is* the colour, so
/// the run has no identity apart from how it looks, and once the list is built
/// there is no way to ask which entries cover a position: the list is stored
/// and re-scanned by whoever wants to know. [`MarkSet::names_at`] is that
/// question, answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkSet {
    marks: Vec<Mark>,
}

impl MarkSet {
    /// No marks.
    #[must_use]
    pub const fn new() -> Self {
        Self { marks: Vec::new() }
    }

    /// The same set with `mark` declared after everything already in it — so
    /// it wins wherever it overlaps.
    #[must_use]
    pub fn with(mut self, mark: Mark) -> Self {
        self.marks.push(mark);
        self
    }

    /// The same set with a mark called `name` over `[start, end)` declared
    /// last. The builder for the common case.
    #[must_use]
    pub fn marking(self, name: impl Into<String>, start: usize, end: usize) -> Self {
        self.with(Mark::new(name, start, end))
    }

    /// Declare `mark` last, in place.
    pub fn push(&mut self, mark: Mark) {
        self.marks.push(mark);
    }

    /// The marks, in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &Mark> {
        self.marks.iter()
    }

    /// How many marks are declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether nothing is marked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The mark by that name, if one is declared. The last, if a name is
    /// declared twice — the same rule overlap follows.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Mark> {
        self.marks.iter().rev().find(|mark| mark.name == name)
    }

    /// Every mark covering `byte`, in declaration order.
    ///
    /// **Why a byte looks the way it does.** A painter obeys the last of
    /// these; a reader — a person, or an assistant over the wire — gets the
    /// whole stack, so "the length field, inside the header, inside the frame"
    /// is answerable rather than inferred from a colour.
    #[must_use]
    pub fn at(&self, byte: usize) -> impl DoubleEndedIterator<Item = &Mark> {
        self.marks.iter().filter(move |mark| mark.contains(byte))
    }

    /// The names covering `byte`, in declaration order.
    #[must_use]
    pub fn names_at(&self, byte: usize) -> Vec<&str> {
        self.at(byte).map(Mark::name).collect()
    }

    /// The mark a painter obeys at `byte` — the last declared one covering it.
    #[must_use]
    pub fn top_at(&self, byte: usize) -> Option<&Mark> {
        self.at(byte).next_back()
    }
}

impl FromIterator<Mark> for MarkSet {
    fn from_iter<I: IntoIterator<Item = Mark>>(iter: I) -> Self {
        Self {
            marks: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layouts every property below is checked over — the classic dump plus
    /// the shapes that break a hand-computed column: one group, a group that
    /// does not divide the row, a row narrower than a group, and a short final
    /// row.
    fn layouts() -> Vec<HexLayout> {
        vec![
            HexLayout::new(128),
            HexLayout::new(130),
            HexLayout::new(1),
            HexLayout::new(64).with_group(16),
            HexLayout::new(64).with_bytes_per_row(8).with_group(4),
            HexLayout::new(37).with_bytes_per_row(6).with_group(4),
            HexLayout::new(200).with_offset_digits(4).with_gutter(1),
            HexLayout::new(9).with_bytes_per_row(4).with_group(8),
        ]
    }

    #[test]
    fn the_classic_dump_reproduces_the_columns_a_person_computed_by_hand() {
        // The five constants `hello-hex-dump` carried as literals before this
        // module existed. They are derivations now, and this is the assertion
        // that the derivation agrees with the paper arithmetic.
        let layout = HexLayout::new(128);
        assert_eq!(layout.hex_start(), 10);
        assert_eq!(layout.ascii_bar_left(), 60);
        assert_eq!(layout.ascii_start(), 61);
        assert_eq!(layout.ascii_bar_right(), 77);
        assert_eq!(layout.total_cols(), 78);
        assert_eq!(layout.rows(), 8);
        assert_eq!(layout.hex_col(0), 10);
        assert_eq!(layout.hex_col(8), 35, "the group split shifts the second 8");
        assert_eq!(layout.ascii_col(0), 61);
    }

    #[test]
    fn every_byte_round_trips_through_both_of_its_cells() {
        // ★ The property the whole type exists for: the forward maps and the
        // inverse cannot disagree, over every layout rather than the one the
        // example happened to use.
        for layout in layouts() {
            for byte in 0..layout.len() {
                let hex = layout.hex_cell(byte).expect("inside the buffer");
                assert_eq!(
                    layout.region_at(hex),
                    Region::Hex {
                        byte,
                        nibble: Nibble::High
                    },
                    "{layout:?} byte {byte} hex cell {hex}"
                );
                assert_eq!(
                    layout.region_at(Cell::new(hex.col + 1, hex.row)),
                    Region::Hex {
                        byte,
                        nibble: Nibble::Low
                    },
                    "{layout:?} byte {byte} low nibble"
                );
                let ascii = layout.ascii_cell(byte).expect("inside the buffer");
                assert_eq!(
                    layout.region_at(ascii),
                    Region::Ascii { byte },
                    "{layout:?} byte {byte} ascii cell {ascii}"
                );
                assert_eq!(layout.byte_at(hex), Some(byte));
                assert_eq!(layout.byte_at(ascii), Some(byte));
            }
        }
    }

    #[test]
    fn every_cell_in_the_grid_is_classified() {
        // Totality: a hit-test has no unclassified answer to fall through to,
        // and no two bytes claim one cell.
        for layout in layouts() {
            let mut claimed: Vec<Option<usize>> = Vec::new();
            for row in 0..layout.rows() {
                for col in 0..layout.total_cols() {
                    let region = layout.region_at(Cell::new(col, row));
                    assert_ne!(
                        region,
                        Region::Outside,
                        "{layout:?} ({col},{row}) is inside the grid"
                    );
                    if let Some(byte) = region.byte() {
                        assert!(byte < layout.len());
                        claimed.push(Some(byte));
                    }
                }
            }
            // Each byte is claimed exactly three times: two hex digits and one
            // ascii glyph.
            for byte in 0..layout.len() {
                let times = claimed.iter().filter(|c| **c == Some(byte)).count();
                assert_eq!(times, 3, "{layout:?} byte {byte}");
            }
        }
    }

    #[test]
    fn a_cell_that_is_not_a_byte_says_what_it_is() {
        let layout = HexLayout::new(128);
        assert_eq!(
            layout.region_at(Cell::new(0, 2)),
            Region::Offset { at: 32, digit: 0 },
            "the offset field names the row's own first byte"
        );
        assert_eq!(
            layout.region_at(Cell::new(7, 0)),
            Region::Offset { at: 0, digit: 7 }
        );
        assert_eq!(layout.region_at(Cell::new(60, 0)), Region::Bar);
        assert_eq!(layout.region_at(Cell::new(77, 0)), Region::Bar);
        assert_eq!(
            layout.region_at(Cell::new(12, 0)),
            Region::Padding,
            "the space after byte 0's pair"
        );
        assert_eq!(layout.region_at(Cell::new(78, 0)), Region::Outside);
        assert_eq!(layout.region_at(Cell::new(0, 8)), Region::Outside);
    }

    #[test]
    fn a_short_final_rows_tail_is_padding_rather_than_a_byte() {
        let layout = HexLayout::new(18);
        assert_eq!(layout.rows(), 2);
        // Byte 17 is the last one; row 1 index 2 onward has no byte.
        assert_eq!(layout.byte_at(Cell::new(layout.hex_col(1), 1)), Some(17));
        assert_eq!(
            layout.region_at(Cell::new(layout.hex_col(2), 1)),
            Region::Padding
        );
        assert_eq!(
            layout.region_at(Cell::new(layout.ascii_col(9), 1)),
            Region::Padding
        );
        assert_eq!(layout.hex_cell(18), None);
        assert_eq!(layout.ascii_cell(18), None);
    }

    #[test]
    fn a_nibble_is_the_half_a_click_landed_on() {
        let layout = HexLayout::new(4);
        let hex = layout.hex_cell(2).unwrap();
        let Region::Hex { nibble: high, .. } = layout.region_at(hex) else {
            panic!("a hex cell")
        };
        let Region::Hex { nibble: low, .. } = layout.region_at(Cell::new(hex.col + 1, hex.row))
        else {
            panic!("a hex cell")
        };
        assert_eq!(high, Nibble::High);
        assert_eq!(low, Nibble::Low);
        assert_eq!(high.of(0xab), 0xa);
        assert_eq!(low.of(0xab), 0xb);
    }

    #[test]
    fn a_degenerate_declaration_still_has_an_inverse() {
        // The builders clamp rather than allowing a layout with no rows and no
        // hit-test — a zero row width is a caller's arithmetic slip, and the
        // honest repair is the narrowest legal dump.
        let layout = HexLayout::new(3).with_bytes_per_row(0).with_group(0);
        assert_eq!(layout.bytes_per_row(), 1);
        assert_eq!(layout.rows(), 3);
        for byte in 0..3 {
            let hex = layout.hex_cell(byte).unwrap();
            assert_eq!(layout.byte_at(hex), Some(byte));
        }
    }

    #[test]
    fn an_empty_buffer_has_no_rows_and_no_cells() {
        let layout = HexLayout::new(0);
        assert!(layout.is_empty());
        assert_eq!(layout.rows(), 0);
        assert_eq!(layout.hex_cell(0), None);
        assert_eq!(layout.region_at(Cell::new(0, 0)), Region::Outside);
    }

    #[test]
    fn a_window_is_ordered_and_clamped_either_way_it_was_dragged() {
        let layout = HexLayout::new(128);
        assert_eq!(layout.window(0.0625, 0.1875), (8, 24));
        assert_eq!(
            layout.window(0.1875, 0.0625),
            (8, 24),
            "a drag in either direction is the same window"
        );
        assert_eq!(
            layout.window(-1.0, 9.0),
            (0, 128),
            "and neither end leaves it"
        );
        assert_eq!(layout.window(0.5, 0.5), (64, 64));
    }

    #[test]
    fn a_selection_is_canonical_whichever_way_the_drag_went() {
        let up = ByteSelection::drag(3, 7);
        let down = ByteSelection::drag(7, 3);
        assert_ne!(up, down, "the RUN is the same and the focus is not");
        assert_eq!((up.start(), up.end()), (3, 8));
        assert_eq!((down.start(), down.end()), (3, 8));
        assert_eq!(up.focus(), 7, "the focus is where the cursor is");
        assert_eq!(down.focus(), 3, "and it survives the reversal");
        assert_eq!(up.len(), 5);
        assert!(up.contains(3) && up.contains(7) && !up.contains(8));

        let click = ByteSelection::at(4);
        assert_eq!((click.start(), click.end(), click.len()), (4, 5, 1));
    }

    #[test]
    fn a_selection_survives_the_buffer_shrinking_under_it() {
        let selection = ByteSelection::drag(2, 9).clamped(5);
        assert_eq!((selection.start(), selection.end()), (2, 5));
        assert_eq!(selection.focus(), 4, "the focus came back inside the run");

        let gone = ByteSelection::drag(8, 9).clamped(2);
        assert!(gone.is_empty(), "a run entirely past the end holds nothing");
        assert_eq!(gone.hex(&[1, 2], " "), "");
    }

    #[test]
    fn the_hex_readout_is_the_selected_bytes_and_nothing_else() {
        let buffer: Vec<u8> = (0u8..=0x0f).collect();
        assert_eq!(ByteSelection::drag(0, 3).hex(&buffer, " "), "00 01 02 03");
        assert_eq!(
            ByteSelection::drag(0, 3).hex(&buffer, ""),
            "00010203",
            "the separator is the caller's — a clipboard payload and a readout \
             want different ones, and neither is this crate's to pick"
        );
        assert_eq!(ByteSelection::at(15).hex(&buffer, " "), "0f");
        assert_eq!(
            ByteSelection::drag(14, 40).hex(&buffer, " "),
            "0e 0f",
            "a stale selection takes what the buffer has rather than panicking"
        );
    }

    #[test]
    fn the_gutter_shows_printable_ascii_and_a_dot_for_everything_else() {
        assert_eq!(ascii_glyph(b'A'), 'A');
        assert_eq!(ascii_glyph(b' '), ' ', "a space is printable and stays one");
        assert_eq!(ascii_glyph(b'~'), '~');
        assert_eq!(ascii_glyph(0x1f), '.');
        assert_eq!(ascii_glyph(0x7f), '.', "DEL is not graphic");
        assert_eq!(ascii_glyph(0xff), '.');
    }

    #[test]
    fn a_hex_digit_takes_the_low_four_bits_of_whatever_it_is_given() {
        assert_eq!(hex_digit(0), '0');
        assert_eq!(hex_digit(9), '9');
        assert_eq!(hex_digit(10), 'a');
        assert_eq!(hex_digit(15), 'f');
        assert_eq!(
            hex_digit(0xab),
            'b',
            "a whole byte gives one digit rather than two characters or a panic"
        );
    }

    /// The implementation `region_at` replaced: a scan of the row's bytes,
    /// asking each in turn whether it owns the column. It is the reference the
    /// closed form is held to, kept here rather than deleted because a
    /// rewritten inverse with no witness is a rewritten inverse nobody checked.
    fn region_at_by_scan(layout: &HexLayout, cell: Cell) -> Region {
        if cell.row >= layout.rows() || cell.col >= layout.total_cols() {
            return Region::Outside;
        }
        let row_start = cell.row * layout.bytes_per_row();
        if cell.col < layout.offset_digits() {
            return Region::Offset {
                at: row_start,
                digit: cell.col,
            };
        }
        if cell.col == layout.ascii_bar_left() || cell.col == layout.ascii_bar_right() {
            return Region::Bar;
        }
        let in_row = (layout.len() - row_start).min(layout.bytes_per_row());
        for index in 0..in_row {
            let hex = layout.hex_col(index);
            if cell.col == hex {
                return Region::Hex {
                    byte: row_start + index,
                    nibble: Nibble::High,
                };
            }
            if cell.col == hex + 1 {
                return Region::Hex {
                    byte: row_start + index,
                    nibble: Nibble::Low,
                };
            }
            if cell.col == layout.ascii_col(index) {
                return Region::Ascii {
                    byte: row_start + index,
                };
            }
        }
        Region::Padding
    }

    #[test]
    fn the_closed_form_inverse_agrees_with_a_scan() {
        // Every cell of every layout shape, plus a margin past both edges so
        // the `Outside` boundary is compared too.
        for layout in layouts() {
            for row in 0..layout.rows() + 2 {
                for col in 0..layout.total_cols() + 2 {
                    let cell = Cell::new(col, row);
                    assert_eq!(
                        layout.region_at(cell),
                        region_at_by_scan(&layout, cell),
                        "{layout:?} at {cell}"
                    );
                }
            }
        }
    }

    #[test]
    fn classifying_a_cell_is_constant_time_in_the_row_width() {
        // ★ The reason the inverse was rewritten. A painter asks what EVERY
        // cell is, so an inverse that scans the row makes a render cost
        // `cells * bytes_per_row`.
        //
        // The assertion is a wall clock, which is only defensible because the
        // two costs are not close: at 2^24 bytes a row, a scan reaching the
        // last byte of the row does ~1.7e7 comparisons, and 64 of those probes
        // is ~1.1e9 -- seconds even in release, tens of seconds unoptimised.
        // The closed form does 64 divisions. The ceiling below is ~4 orders of
        // magnitude above what the closed form costs and at least an order
        // BELOW what the scan costs, so neither a loaded machine nor a fast one
        // moves the verdict.
        let wide = HexLayout::new(1 << 25)
            .with_bytes_per_row(1 << 24)
            .with_group(1 << 24);
        let last = wide.hex_col(wide.bytes_per_row() - 1);
        let start = std::time::Instant::now();
        let mut bytes_seen = 0usize;
        for probe in 0..64 {
            let cell = Cell::new(last - probe * 3, 1);
            let region = wide.region_at(cell);
            assert_eq!(
                region,
                Region::Hex {
                    byte: wide.bytes_per_row() * 2 - 1 - probe,
                    nibble: Nibble::High,
                },
                "the far end of a very wide row still classifies exactly"
            );
            bytes_seen += 1;
        }
        assert_eq!(bytes_seen, 64);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "64 classifications at 2^24 bytes a row took {elapsed:?}; a scanning \
             inverse is what that means"
        );
    }

    #[test]
    fn the_offset_field_is_derived_at_its_declared_width() {
        // ★ The defect this closes. `hello-hex-dump` formatted the row offset
        // to a FIXED eight hex digits and then took the first `offset_digits`
        // characters of that string, so a four-digit offset column printed the
        // leading zeros of an eight-digit number -- `0000` on every row,
        // including the rows that are not at offset zero.
        let narrow = HexLayout::new(0x2000)
            .with_bytes_per_row(16)
            .with_offset_digits(4);
        let bytes = vec![0u8; 0x2000];
        let row = 0x123; // byte offset 0x1230
        let digits: String = (0..4)
            .map(|d| narrow.glyph_at(&bytes, Cell::new(d, row)))
            .collect();
        assert_eq!(
            digits, "1230",
            "the row's own offset, at the declared width"
        );

        // What the string-truncation it replaces would have produced.
        let truncated: String = format!("{:08x}", row * 16).chars().take(4).collect();
        assert_eq!(truncated, "0000");
        assert_ne!(digits, truncated, "the derivation is not the truncation");

        // The classic width is unchanged.
        let classic = HexLayout::new(128);
        let wide: String = (0..8)
            .map(|d| classic.glyph_at(&bytes, Cell::new(d, 2)))
            .collect();
        assert_eq!(wide, "00000020");

        // A field narrower than the offset shows its low digits, as a
        // fixed-width field must -- not a panic and not a truncation to zero.
        let tiny = HexLayout::new(0x2000)
            .with_bytes_per_row(16)
            .with_offset_digits(2);
        let low: String = (0..2)
            .map(|d| tiny.glyph_at(&bytes, Cell::new(d, row)))
            .collect();
        assert_eq!(low, "30");
    }

    #[test]
    fn every_cell_shows_the_glyph_its_region_names() {
        // ★ Paint and hit-test read ONE fact: the character a cell shows is
        // derived from the same `region_at` that answers a pointer, so a cell
        // that draws a byte is a cell that selects that byte.
        let bytes: Vec<u8> = (0..=255u8).collect();
        for layout in layouts() {
            let bytes = &bytes[..layout.len().min(bytes.len())];
            for row in 0..layout.rows() {
                for col in 0..layout.total_cols() {
                    let cell = Cell::new(col, row);
                    let glyph = layout.glyph_at(bytes, cell);
                    match layout.region_at(cell) {
                        Region::Hex { byte, nibble } => {
                            let value = bytes.get(byte).copied();
                            assert_eq!(
                                glyph,
                                value.map_or(' ', |v| hex_digit(nibble.of(v))),
                                "{layout:?} {cell} draws its own byte's nibble"
                            );
                            assert_eq!(layout.byte_at(cell), Some(byte));
                        }
                        Region::Ascii { byte } => {
                            let value = bytes.get(byte).copied();
                            assert_eq!(glyph, value.map_or(' ', ascii_glyph));
                            assert_eq!(layout.byte_at(cell), Some(byte));
                        }
                        Region::Bar => {
                            assert_eq!(glyph, '|');
                            assert_eq!(layout.byte_at(cell), None);
                        }
                        Region::Offset { at, digit } => {
                            assert_eq!(glyph, layout.offset_digit(at, digit));
                            assert_eq!(layout.byte_at(cell), None);
                        }
                        Region::Padding | Region::Outside => {
                            assert_eq!(glyph, ' ');
                            assert_eq!(layout.byte_at(cell), None);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_glyph_outside_the_grid_is_a_space_rather_than_a_panic() {
        let layout = HexLayout::new(32);
        let bytes = [0u8; 32];
        assert_eq!(layout.glyph_at(&bytes, Cell::new(0, 99)), ' ');
        assert_eq!(layout.glyph_at(&bytes, Cell::new(9_999, 0)), ' ');
        // A buffer shorter than the layout claims: the tail is blank, not a
        // stale byte and not an index panic.
        assert_eq!(
            layout.glyph_at(&[1, 2, 3], Cell::new(layout.ascii_col(9), 0)),
            ' '
        );
    }

    #[test]
    fn overlapping_marks_resolve_in_declaration_order_last_wins() {
        // ★ ONE direction, unlike the split the reference interface has.
        let marks = MarkSet::new()
            .marking("frame", 0, 64)
            .marking("header", 0, 16)
            .marking("length", 4, 8);
        assert_eq!(marks.len(), 3);
        assert_eq!(marks.names_at(5), vec!["frame", "header", "length"]);
        assert_eq!(marks.top_at(5).map(Mark::name), Some("length"));
        assert_eq!(marks.names_at(12), vec!["frame", "header"]);
        assert_eq!(marks.top_at(12).map(Mark::name), Some("header"));
        assert_eq!(marks.names_at(40), vec!["frame"]);
        assert_eq!(marks.top_at(40).map(Mark::name), Some("frame"));
        assert!(marks.names_at(100).is_empty());
        assert_eq!(marks.top_at(100), None);
    }

    #[test]
    fn a_byte_says_why_it_is_lit() {
        // ★ The query the reference's range-decoration list has no form of:
        // the whole stack covering a byte, in the order that decided it.
        // A colour cannot answer this -- two marks that resolve to the same
        // ink are indistinguishable once drawn.
        let marks = MarkSet::new()
            .marking("frame", 0, 64)
            .marking("header", 0, 16)
            .marking("length", 4, 8);
        let why = marks.names_at(6);
        assert_eq!(why, vec!["frame", "header", "length"]);
        assert_eq!(
            why.last().copied(),
            Some("length"),
            "the last of the stack is the one paint obeys"
        );

        // Model of the split-direction rule: background takes the LAST range
        // covering a byte, foreground the FIRST. Two marks, two answers, and
        // no way to ask which run a byte belongs to.
        let ranges = [("frame", 0usize, 64usize), ("length", 4, 8)];
        let covering = |byte: usize| -> Vec<&str> {
            ranges
                .iter()
                .filter(|(_, s, e)| (*s..*e).contains(&byte))
                .map(|(n, _, _)| *n)
                .collect()
        };
        let split = covering(6);
        assert_eq!(split.first().copied(), Some("frame"), "foreground: first");
        assert_eq!(split.last().copied(), Some("length"), "background: last");
        assert_ne!(
            split.first(),
            split.last(),
            "the two channels of one byte come from DIFFERENT runs -- which is \
             the rule this module refuses"
        );
        let ours = MarkSet::new()
            .marking("frame", 0, 64)
            .marking("length", 4, 8);
        assert_eq!(
            ours.top_at(6).map(Mark::name),
            Some("length"),
            "here both channels come from the same run, and it is nameable"
        );
    }

    #[test]
    fn a_mark_is_a_run_and_says_so() {
        let mark = Mark::new("payload", 8, 24);
        assert_eq!(mark.name(), "payload");
        assert_eq!((mark.start(), mark.end(), mark.len()), (8, 24, 16));
        assert!(mark.contains(8));
        assert!(mark.contains(23));
        assert!(!mark.contains(24));
        assert!(!mark.is_empty());
        // An inverted range is ordered rather than rejected -- a drag runs
        // either way and the run is the same run.
        assert_eq!(Mark::new("x", 9, 3), Mark::new("x", 3, 9));
        assert!(Mark::new("empty", 5, 5).is_empty());
        // A selection is a run with a focus; a mark is the run.
        let selection = ByteSelection::drag(11, 4);
        let of_sel = Mark::of_selection("selection", selection);
        assert_eq!((of_sel.start(), of_sel.end()), (4, 12));
    }

    #[test]
    fn a_mark_set_is_queryable_by_name_and_collectable() {
        let marks: MarkSet = [Mark::new("a", 0, 4), Mark::new("b", 4, 8)]
            .into_iter()
            .collect();
        assert_eq!(marks.get("b").map(Mark::start), Some(4));
        assert_eq!(marks.get("missing"), None);
        assert!(MarkSet::new().is_empty());
        // A name declared twice resolves the way overlap does: the last one.
        let shadowed = MarkSet::new().marking("f", 0, 4).marking("f", 8, 12);
        assert_eq!(shadowed.get("f").map(Mark::start), Some(8));
        assert_eq!(shadowed.iter().count(), 2);
    }
}
