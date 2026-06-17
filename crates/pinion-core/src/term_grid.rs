//! R973 §5.41 — the cell-native `TextGrid` **content data model** (the
//! first S5 data-model slice: terminal colour model + palette + cell
//! projection).
//!
//! [`Scene::TextGrid`](crate::scene::Scene::TextGrid) is the *retained
//! projection* of a producer terminal buffer (R969): a PTY + `vte`
//! producer (owned by the consumer crate, e.g. `sprag`) holds the
//! authoritative grid and hands pinion a [`GridBuffer`] snapshot each
//! frame. pinion holds that projection and renders / introspects it; it
//! is **not** an independent mutable terminal emulator (R969 "dual-grid
//! 금지"). The node therefore replaces its whole [`GridBuffer`] wholesale
//! and exposes no per-cell mutation — the producer assembles the buffer,
//! the node projects it.
//!
//! This slice models the cell's **colour** faithfully to a real
//! terminal:
//!
//! - [`TermColor`] is the *circular* cell colour (R969 "원형"): a cell
//!   stores [`TermColor::Default`] (the terminal's default fg/bg),
//!   [`TermColor::Indexed`] (an entry in the 256-colour palette), or
//!   [`TermColor::Rgb`] (a direct 24-bit truecolor). The variant is
//!   preserved in storage — *resolution to a concrete [`Color`] happens
//!   only at paint / introspection time*, through the [`Palette`], so a
//!   palette swap (theme change) restains every indexed/default cell
//!   coherently without rewriting the buffer (R969 "resolve=paint시점만").
//! - [`Palette`] is the single source of truth for `indexed → rgb`: the
//!   16 themeable ANSI base colours plus the standard xterm 6×6×6 colour
//!   cube and 24-step grayscale ramp (computed by formula), and the
//!   terminal's default foreground / background.
//!
//! The cell's **attributes** ([`CellAttrs`]: bold / dim / italic /
//! underline / blink / reverse / hidden / strikethrough) land in R974,
//! the grid **cursor** ([`GridCursor`]: position / shape / visibility) in
//! R975, the **wide-char trailer** ([`CellWidth`]: a wide cluster occupies
//! two columns — a head cell plus a continuation trailer) in R976, the
//! **alternate-screen** discriminator ([`ScreenKind`]: whether the
//! projection is the main shell surface or a fullscreen app's screen) in
//! R977, and **damage** tracking (per-row
//! [generation](GridBuffer::row_generation) counters for incremental,
//! streaming reads) in R978 — completing the S5 data model. These slices
//! prove the model with their cell + projection consumer (the
//! [`crate::scene::TextGridNode`] cells + the `scene/snapshot` readback),
//! not pixel paint (glyph rendering stays deferred — the grid is still
//! paint-opaque this round).

use crate::style::Color;
use std::borrow::Cow;

/// Which side of a cell a [`TermColor::Default`] resolves against — a
/// terminal's default colour differs for foreground glyphs vs the cell
/// background, so [`Palette::resolve`] needs to know which it is asked
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    /// The glyph (text) colour slot.
    Foreground,
    /// The cell background slot.
    Background,
}

/// A terminal cell colour, stored in its *original* form (R969 circular
/// colour): the producer's intent is preserved and the concrete [`Color`]
/// is computed only when painting or introspecting, through a
/// [`Palette`]. This is what lets a single palette swap restain every
/// indexed / default cell at once.
///
/// `#[non_exhaustive]` is intentionally **not** applied: the three forms
/// (default / indexed / direct) are the complete, closed terminal colour
/// model — there is no fourth kind of colour a cell can carry, so callers
/// may exhaustively match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    /// The terminal's configured default colour for this slot — resolved
    /// via [`Palette::default_fg`] / [`Palette::default_bg`] by
    /// [`ColorTarget`]. The default for a freshly-blanked cell.
    #[default]
    Default,
    /// An entry in the 256-colour palette (`0..=15` themeable ANSI,
    /// `16..=231` the 6×6×6 cube, `232..=255` the grayscale ramp).
    /// Resolved via [`Palette::indexed`].
    Indexed(u8),
    /// A direct 24-bit truecolor, used verbatim (palette-independent).
    Rgb(Color),
}

/// The `indexed → rgb` single source of truth (R969): the 16 themeable
/// ANSI base colours plus the terminal default foreground / background.
/// The 6×6×6 colour cube (`16..=231`) and 24-step grayscale ramp
/// (`232..=255`) are the fixed xterm formulas and are computed on
/// resolution rather than stored.
///
/// One [`Palette`] is held per [`crate::scene::TextGridNode`]: a grid
/// resolves its cells' [`TermColor`]s against its own palette, so two
/// grids (or a theme swap) can restain independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The 16 ANSI base colours (`0..=15`): the 8 normal + 8 bright
    /// colours a terminal theme configures. Indexed slots `16..=255` are
    /// the standard xterm cube / ramp and are not stored.
    ansi16: [Color; 16],
    /// The colour [`TermColor::Default`] resolves to in a
    /// [`ColorTarget::Foreground`] slot.
    default_fg: Color,
    /// The colour [`TermColor::Default`] resolves to in a
    /// [`ColorTarget::Background`] slot.
    default_bg: Color,
}

impl Palette {
    /// The standard xterm 16-colour ANSI base table (`0..=7` normal,
    /// `8..=15` bright) — the conventional default a terminal ships with.
    const XTERM_ANSI16: [Color; 16] = [
        Color::rgb(0x00, 0x00, 0x00), // 0  black
        Color::rgb(0xcd, 0x00, 0x00), // 1  red
        Color::rgb(0x00, 0xcd, 0x00), // 2  green
        Color::rgb(0xcd, 0xcd, 0x00), // 3  yellow
        Color::rgb(0x00, 0x00, 0xee), // 4  blue
        Color::rgb(0xcd, 0x00, 0xcd), // 5  magenta
        Color::rgb(0x00, 0xcd, 0xcd), // 6  cyan
        Color::rgb(0xe5, 0xe5, 0xe5), // 7  white (light grey)
        Color::rgb(0x7f, 0x7f, 0x7f), // 8  bright black (grey)
        Color::rgb(0xff, 0x00, 0x00), // 9  bright red
        Color::rgb(0x00, 0xff, 0x00), // 10 bright green
        Color::rgb(0xff, 0xff, 0x00), // 11 bright yellow
        Color::rgb(0x5c, 0x5c, 0xff), // 12 bright blue
        Color::rgb(0xff, 0x00, 0xff), // 13 bright magenta
        Color::rgb(0x00, 0xff, 0xff), // 14 bright cyan
        Color::rgb(0xff, 0xff, 0xff), // 15 bright white
    ];

    /// The conventional xterm default palette: the standard 16 ANSI
    /// colours, light-grey-on-black default foreground / background.
    #[must_use]
    pub const fn xterm_default() -> Self {
        Self {
            ansi16: Self::XTERM_ANSI16,
            // ANSI 7 (light grey) on ANSI 0 (black) — the conventional
            // terminal default surface.
            default_fg: Self::XTERM_ANSI16[7],
            default_bg: Self::XTERM_ANSI16[0],
        }
    }

    // Theme mutators (replace the ANSI base / default fg+bg) are
    // deliberately NOT exposed yet: this slice ships only the fixed xterm
    // palette, and the theme-swap consumer is a later S5 slice. Per the
    // R972 "no unconsumed surface" discipline they land with that
    // consumer (the `indexed → rgb` resolution they would exercise is
    // already proven against the default palette below).

    /// The default-foreground colour.
    #[must_use]
    pub const fn default_fg(&self) -> Color {
        self.default_fg
    }

    /// The default-background colour.
    #[must_use]
    pub const fn default_bg(&self) -> Color {
        self.default_bg
    }

    /// Resolve a 256-colour palette index to a concrete [`Color`].
    ///
    /// - `0..=15` — the ANSI base colours (themeable when theme support
    ///   lands; fixed to the xterm defaults this slice).
    /// - `16..=231` — the 6×6×6 colour cube: each axis steps through
    ///   `{0, 95, 135, 175, 215, 255}`.
    /// - `232..=255` — the 24-step grayscale ramp `8, 18, …, 238`.
    #[must_use]
    pub fn indexed(&self, index: u8) -> Color {
        match index {
            0..=15 => self.ansi16[usize::from(index)],
            16..=231 => {
                let n = index - 16; // 0..=215
                let r = Self::cube_channel(n / 36);
                let g = Self::cube_channel((n / 6) % 6);
                let b = Self::cube_channel(n % 6);
                Color::rgb(r, g, b)
            }
            232..=255 => {
                // 24 grays: level = 8 + step*10  ->  8, 18, …, 238.
                let level = 8 + (index - 232) * 10;
                Color::rgb(level, level, level)
            }
        }
    }

    /// Map a cube axis step (`0..=5`) to its 8-bit channel value: `0` for
    /// step 0, then `55 + step*40` (→ `95, 135, 175, 215, 255`). This is
    /// the standard xterm colour-cube channel formula.
    const fn cube_channel(step: u8) -> u8 {
        if step == 0 {
            0
        } else {
            55 + step * 40
        }
    }

    /// Resolve a stored [`TermColor`] to the concrete [`Color`] a painter
    /// would draw — the *only* place index / default resolution happens
    /// (R969 "resolve=paint시점만"). `Default` consults the per-target
    /// default, `Indexed` the palette, `Rgb` is used verbatim.
    #[must_use]
    pub fn resolve(&self, color: TermColor, target: ColorTarget) -> Color {
        match color {
            TermColor::Default => match target {
                ColorTarget::Foreground => self.default_fg,
                ColorTarget::Background => self.default_bg,
            },
            TermColor::Indexed(index) => self.indexed(index),
            TermColor::Rgb(rgb) => rgb,
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::xterm_default()
    }
}

/// The SGR display attributes a terminal cell carries (R974) — the
/// standard boolean set every terminal (`xterm` / `vte` / `alacritty`)
/// and Rust terminal library (`ratatui` `Modifier`, `crossterm`) speaks.
/// Each flag is an independent SGR toggle, so this is a struct of named
/// booleans (mirroring [`crate::input::Modifiers`] / [`TextDecoration`]),
/// not a bifurcating enum.
///
/// [`Self::reverse`] is the attribute that most directly drives colour
/// resolution: at *paint* time a reversed cell swaps its effective
/// foreground / background. (`dim` / `bold` may also shift the rendered
/// intensity depending on the renderer.) This slice stores and
/// introspects the flags; the transforms themselves land with glyph
/// paint (the grid is still paint-opaque), so a snapshot reports the
/// *stored* colours plus the flags and a renderer applies them.
///
/// `struct_excessive_bools` is suppressed for the same reason it is on
/// [`crate::input::Modifiers`]: the SGR attribute set is a fixed industry
/// vocabulary, and a bitflag would diverge from the names callers expect.
/// `#[non_exhaustive]` per the R974.1 forward-compat hedge: later S5
/// slices add SGR flags (e.g. an underline-style axis), and construction
/// already routes through [`Self::empty`] + the `with_*` builders, so the
/// hedge is free (matching the [`TextDecoration`] sibling).
///
/// [`TextDecoration`]: crate::style::TextDecoration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct CellAttrs {
    /// SGR 1 — increased intensity / bold weight.
    pub bold: bool,
    /// SGR 2 — decreased intensity / faint.
    pub dim: bool,
    /// SGR 3 — italic.
    pub italic: bool,
    /// SGR 4 — underline.
    pub underline: bool,
    /// SGR 5 — blink (slow / rapid are folded into one flag).
    pub blink: bool,
    /// SGR 7 — reverse video: swaps effective fg / bg at paint time.
    pub reverse: bool,
    /// SGR 8 — conceal / hidden (glyph not drawn).
    pub hidden: bool,
    /// SGR 9 — crossed-out / strikethrough.
    pub strikethrough: bool,
}

impl CellAttrs {
    /// All attributes off — the default a blank cell carries.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            blink: false,
            reverse: false,
            hidden: false,
            strikethrough: false,
        }
    }

    /// `true` iff no attribute is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !(self.bold
            || self.dim
            || self.italic
            || self.underline
            || self.blink
            || self.reverse
            || self.hidden
            || self.strikethrough)
    }

    /// Builder: set the bold flag.
    #[must_use]
    pub const fn with_bold(mut self, on: bool) -> Self {
        self.bold = on;
        self
    }

    /// Builder: set the dim flag.
    #[must_use]
    pub const fn with_dim(mut self, on: bool) -> Self {
        self.dim = on;
        self
    }

    /// Builder: set the italic flag.
    #[must_use]
    pub const fn with_italic(mut self, on: bool) -> Self {
        self.italic = on;
        self
    }

    /// Builder: set the underline flag.
    #[must_use]
    pub const fn with_underline(mut self, on: bool) -> Self {
        self.underline = on;
        self
    }

    /// Builder: set the blink flag.
    #[must_use]
    pub const fn with_blink(mut self, on: bool) -> Self {
        self.blink = on;
        self
    }

    /// Builder: set the reverse-video flag.
    #[must_use]
    pub const fn with_reverse(mut self, on: bool) -> Self {
        self.reverse = on;
        self
    }

    /// Builder: set the hidden flag.
    #[must_use]
    pub const fn with_hidden(mut self, on: bool) -> Self {
        self.hidden = on;
        self
    }

    /// Builder: set the strikethrough flag.
    #[must_use]
    pub const fn with_strikethrough(mut self, on: bool) -> Self {
        self.strikethrough = on;
        self
    }
}

/// A cell's display-width role (R976) — how many terminal columns its
/// grapheme cluster occupies, and whether it is the *continuation* column
/// of a wide cluster.
///
/// A CJK ideograph or a fullwidth Latin form occupies **two** columns: the
/// cluster is drawn in the [`Wide`](Self::Wide) head cell and the next
/// column is a [`Trailer`](Self::Trailer) — a continuation marker that
/// carries no independent glyph and is not separately addressable. This
/// mirrors a real emulator's two-cell representation (`alacritty`'s
/// `WIDE_CHAR` + `WIDE_CHAR_SPACER`). The display width is the
/// *producer's* determination (it owns the emulator and its width tables —
/// R969 "producer owns authoritative state"); pinion's model only
/// represents the result, so no width table lives in the core.
///
/// Like [`TermColor`] / [`CursorShape`] this is the *complete, closed* set
/// of width roles a cell in a settled projection can carry, so
/// `#[non_exhaustive]` is deliberately **not** applied and callers may
/// match exhaustively. (The line-wrap leading-spacer edge case an emulator
/// tracks during reflow is resolved producer-side into a blank
/// [`Narrow`](Self::Narrow) cell before projection — it is not a fourth
/// display state.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellWidth {
    /// A single-column cluster — the default for every cell.
    #[default]
    Narrow,
    /// A wide-cluster **head**: the cluster is drawn here and occupies
    /// this column plus the following [`Trailer`](Self::Trailer) column.
    Wide,
    /// The continuation column occupied by the preceding [`Wide`](Self::Wide)
    /// head: no independent glyph (an empty cluster), not separately
    /// addressable. It carries the head's colours so the background paints
    /// across both columns.
    Trailer,
}

/// One terminal grid cell: the displayed grapheme cluster plus its
/// foreground / background [`TermColor`]s, its [`CellAttrs`] (R974), and
/// its [`CellWidth`] display-width role (R976). The cursor is a grid-level
/// concern carried on the [`GridBuffer`] (R975), not on the cell.
///
/// `cluster` is a grapheme cluster string (not a single `char`) so a
/// base char plus combining marks / a ZWJ emoji sequence occupy one cell
/// — `Cow<'static, str>` lets a blank cell borrow the static `" "` while
/// produced cells own their string. A [`CellWidth::Trailer`] cell carries
/// the empty cluster `""`, so a wide cluster contributes its glyph exactly
/// once to an assembled row even though it occupies two columns.
///
/// `#[non_exhaustive]` per the R974.1 forward-compat hedge: later
/// refinements add fields (e.g. an OSC-8 hyperlink target), and
/// construction routes through [`Self::new`] / [`Self::blank`] + the
/// builders, so the hedge is free (matching [`crate::scene::TextGridNode`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TermCell {
    /// The grapheme cluster painted in this cell. `" "` for a blank cell,
    /// `""` for a [`CellWidth::Trailer`].
    pub cluster: Cow<'static, str>,
    /// Foreground (glyph) colour, resolved via the grid's [`Palette`].
    pub fg: TermColor,
    /// Background colour, resolved via the grid's [`Palette`].
    pub bg: TermColor,
    /// SGR display attributes (R974). Empty for a blank cell.
    pub attrs: CellAttrs,
    /// Display-width role (R976). [`CellWidth::Narrow`] for a blank cell.
    pub width: CellWidth,
}

impl TermCell {
    /// A blank cell: a single narrow space with default foreground /
    /// background and no attributes. The fill value a fresh [`GridBuffer`]
    /// uses.
    #[must_use]
    pub const fn blank() -> Self {
        Self {
            cluster: Cow::Borrowed(" "),
            fg: TermColor::Default,
            bg: TermColor::Default,
            attrs: CellAttrs::empty(),
            width: CellWidth::Narrow,
        }
    }

    /// A narrow cell carrying `cluster` with the given foreground /
    /// background and no attributes. Chain [`Self::with_attrs`] for SGR
    /// styling or [`Self::wide`] to mark it a wide-cluster head.
    #[must_use]
    pub fn new(cluster: impl Into<Cow<'static, str>>, fg: TermColor, bg: TermColor) -> Self {
        Self {
            cluster: cluster.into(),
            fg,
            bg,
            attrs: CellAttrs::empty(),
            width: CellWidth::Narrow,
        }
    }

    /// Attach SGR display [`CellAttrs`] (builder form).
    #[must_use]
    pub fn with_attrs(mut self, attrs: CellAttrs) -> Self {
        self.attrs = attrs;
        self
    }

    /// Mark this cell a wide-cluster **head** ([`CellWidth::Wide`], builder
    /// form): its cluster occupies this column and the next, which the
    /// producer fills with [`Self::trailer`]. The producer marks the cells
    /// it knows are wide — pinion stores the determination, it does not
    /// compute width.
    #[must_use]
    pub fn wide(mut self) -> Self {
        self.width = CellWidth::Wide;
        self
    }

    /// The [`CellWidth::Trailer`] cell that occupies the column after this
    /// wide head: the empty cluster `""`, this head's colours and
    /// attributes (so the background paints across both columns). The
    /// producer places it in the column immediately after the head.
    #[must_use]
    pub fn trailer(&self) -> Self {
        Self {
            cluster: Cow::Borrowed(""),
            fg: self.fg,
            bg: self.bg,
            attrs: self.attrs,
            width: CellWidth::Trailer,
        }
    }
}

impl Default for TermCell {
    fn default() -> Self {
        Self::blank()
    }
}

/// The shape a terminal cursor is drawn as — the closed DECSCUSR set
/// (`CSI Ps SP q`): a full-cell **block**, a thin vertical **bar** (the
/// text-insertion beam), or a baseline **underline**.
///
/// Like [`TermColor`], this is the *complete, closed* terminal vocabulary
/// — DECSCUSR defines no fourth shape — so `#[non_exhaustive]` is
/// deliberately **not** applied and callers may match exhaustively. (The
/// DECSCUSR blink-vs-steady axis is a separate concern and is not folded
/// into the shape.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// A filled full-cell block — the conventional terminal default.
    #[default]
    Block,
    /// A thin vertical bar at the cell's leading edge (the insertion beam).
    Bar,
    /// A thin underline along the cell's baseline.
    Underline,
}

/// The terminal cursor (R975): its cell position plus how it is drawn.
///
/// This is the cursor as it should currently *appear* — the **effective**
/// cursor a producer reports each frame, not an emulator's internal mode
/// state. A real emulator (the producer, e.g. `sprag`'s `vte`) owns the
/// DECTCEM (show / hide) and DECSCUSR (shape) mode logic and the cursor
/// save / restore the alternate screen performs; it folds all of that
/// into the one effective cursor it hands pinion, keeping the projection
/// at the right altitude (R969 "producer owns authoritative state"). The
/// cursor therefore lives on the [`GridBuffer`] — like
/// `alacritty_terminal`'s `Grid::cursor` — so when the alternate-screen
/// buffer slice lands each buffer simply carries its own.
///
/// The position is the producer's reported `(col, row)` and may briefly
/// fall outside the buffer's own `(cols, rows)` during an in-flight resize
/// (the producer clamps it); a client detects that by comparing this
/// position against *the buffer's* dimensions — not the layout-derived
/// winsize, which diverges from the buffer during the resize — both being
/// first-class facts (R974.1 dual-fact discipline), so no separate
/// in-bounds flag is stored.
///
/// `#[non_exhaustive]` per the R974.1 forward-compat hedge (matching
/// [`TermCell`] / [`CellAttrs`]): later refinements add fields (e.g. a
/// blink flag or an explicit cursor colour), and construction routes
/// through [`Self::new`] / [`Self::default`], so the hedge is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct GridCursor {
    /// Cursor column (cell coordinate).
    pub col: u16,
    /// Cursor row (cell coordinate).
    pub row: u16,
    /// How the cursor is drawn ([`CursorShape::Block`] by default).
    pub shape: CursorShape,
    /// Whether the cursor is shown (DECTCEM). `false` is also the default
    /// a freshly-constructed [`GridBuffer`] carries: a projection reports
    /// no visible cursor until its producer asks for one.
    pub visible: bool,
}

impl GridCursor {
    /// A cursor at `(col, row)` drawn as `shape`, shown iff `visible` —
    /// the producer assembles this from its emulator's effective cursor
    /// state each frame.
    #[must_use]
    pub const fn new(col: u16, row: u16, shape: CursorShape, visible: bool) -> Self {
        Self {
            col,
            row,
            shape,
            visible,
        }
    }
}

/// Which of a terminal's two screen buffers a projection represents
/// (R977 alt-screen): the **main** screen (the scrollback shell surface) or
/// the **alternate** screen (the cleared, no-scrollback surface a
/// fullscreen app — `vim`, `htop`, a pager — switches to via DECSET 1049).
///
/// A projection holds exactly the screen the producer is currently
/// *displaying*: pinion does not hold the inactive buffer (the producer
/// owns both and projects the active one — R969 "producer owns
/// authoritative state"; this is not the alacritty-style hold-both model).
/// The discriminator tells an AI client which mode the terminal is in
/// (fullscreen app vs. shell) without inferring it from content.
///
/// Like [`CursorShape`] / [`CellWidth`] this is the *complete, closed* set
/// (a terminal has exactly these two screens), so `#[non_exhaustive]` is
/// deliberately **not** applied and callers may match exhaustively.
///
/// This is distinct from the OS terminal's alternate screen that the
/// `pinion-tui` backend itself enters to render — that is pinion's own
/// output surface; this is the *projected* terminal's screen mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenKind {
    /// The main screen — the conventional shell surface (with scrollback),
    /// and the default a fresh projection represents.
    #[default]
    Main,
    /// The alternate screen — the cleared, scrollback-free surface a
    /// fullscreen application takes over (DECSET 1049).
    Alternate,
}

/// A row-major rectangular buffer of [`TermCell`]s — the retained
/// projection of the producer's terminal buffer (R969). Its own
/// `(cols, rows)` describe the *snapshot the producer last sent*; the
/// authoritative winsize the producer is *told* to size to is the
/// layout-derived [`TextGridNode::cols`](crate::scene::TextGridNode::cols)
/// / [`rows`](crate::scene::TextGridNode::rows) (R969 one-directional
/// SSOT). These two converge at steady state and are distinct facts (a
/// requested size vs. a received snapshot), not a dual mutable grid.
///
/// The buffer is assembled by the producer and projected wholesale; it
/// exposes construction ([`Self::new`] / [`Self::with_row`] /
/// [`Self::with_cursor`] / [`Self::with_screen`] /
/// [`Self::with_row_generation`]) and reads ([`Self::cell`] /
/// [`Self::cursor`] / [`Self::screen`] / [`Self::row_generation`]) but the
/// *node* that holds it never mutates it per-cell — it swaps the whole
/// projection. The [`GridCursor`] (R975), the [`ScreenKind`] (R977), and
/// the per-row damage [generations](Self::row_generation) (R978) ride
/// along with the cells as part of the same self-describing screen
/// projection.
///
/// **Damage (R978).** Each row carries a monotonic `u64` *generation* the
/// producer bumps whenever it rewrites that row (a real emulator tracks
/// dirty lines as it processes PTY output — R969 "producer owns
/// authoritative state"). A streaming client (an AI agent watching a CLI
/// emit output) remembers the highest generation it has read and, on its
/// next `scene/snapshot`, re-reads only the rows whose generation exceeds
/// that high-water mark — an incremental update that survives an arbitrary
/// polling cadence (unlike a frame-reset dirty flag). pinion stores and
/// reports the producer's stamps; it does not diff buffers itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GridBuffer {
    cols: u16,
    rows: u16,
    cells: Vec<TermCell>,
    cursor: GridCursor,
    screen: ScreenKind,
    /// One monotonic damage generation per row (length == `rows`), R978.
    row_generations: Vec<u64>,
}

impl GridBuffer {
    /// A `cols × rows` buffer filled with [`TermCell::blank`], on the
    /// [`ScreenKind::Main`] screen, carrying the default
    /// ([`GridCursor::default`]: hidden, home, block) cursor and every
    /// row at damage generation `0` until the producer sets them via the
    /// builders.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let count = usize::from(cols) * usize::from(rows);
        Self {
            cols,
            rows,
            cells: vec![TermCell::blank(); count],
            cursor: GridCursor::default(),
            screen: ScreenKind::Main,
            row_generations: vec![0; usize::from(rows)],
        }
    }

    /// Columns in this projection.
    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols
    }

    /// Rows in this projection.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows
    }

    /// `true` when the projection holds no cells (the `0×0` default a
    /// fresh geometry-only grid carries).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Row-major flat index of cell `(col, row)`, or `None` if either
    /// coordinate is out of bounds.
    fn index(&self, col: u16, row: u16) -> Option<usize> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    /// The cell at `(col, row)`, or `None` if out of bounds.
    #[must_use]
    pub fn cell(&self, col: u16, row: u16) -> Option<&TermCell> {
        self.index(col, row).map(|i| &self.cells[i])
    }

    /// The [`GridCursor`] this projection carries (R975) — its position,
    /// shape, and visibility. A freshly-[`new`](Self::new)'d buffer
    /// carries the default (hidden, home, block) cursor until the producer
    /// sets one.
    #[must_use]
    pub const fn cursor(&self) -> GridCursor {
        self.cursor
    }

    /// Set the grid cursor (builder form). The producer reports its
    /// emulator's effective cursor here alongside the cell projection.
    #[must_use]
    pub const fn with_cursor(mut self, cursor: GridCursor) -> Self {
        self.cursor = cursor;
        self
    }

    /// Which screen ([`ScreenKind`]) this projection represents (R977).
    /// [`ScreenKind::Main`] for a fresh buffer until the producer reports
    /// the alternate screen.
    #[must_use]
    pub const fn screen(&self) -> ScreenKind {
        self.screen
    }

    /// Set the screen kind (builder form). The producer reports
    /// [`ScreenKind::Alternate`] when projecting a fullscreen app's screen.
    #[must_use]
    pub const fn with_screen(mut self, screen: ScreenKind) -> Self {
        self.screen = screen;
        self
    }

    /// The damage generation of `row`, or `None` if the row is out of
    /// bounds (R978). The monotonic stamp the producer last gave the row; a
    /// client re-reads rows whose generation exceeds the highest it has
    /// already seen. `0` for an untouched row. See the type-level docs for
    /// the streaming-read model.
    #[must_use]
    pub fn row_generation(&self, row: u16) -> Option<u64> {
        self.row_generations.get(usize::from(row)).copied()
    }

    /// Stamp `row` with damage `generation` (builder form). The producer
    /// bumps this each time it rewrites the row. An out-of-bounds row is
    /// ignored (mirroring [`Self::with_row`]).
    #[must_use]
    pub fn with_row_generation(mut self, row: u16, generation: u64) -> Self {
        if let Some(slot) = self.row_generations.get_mut(usize::from(row)) {
            *slot = generation;
        }
        self
    }

    /// Write a whole row of cells starting at column 0 (builder form).
    /// Cells beyond the buffer width are ignored; a short row leaves the
    /// trailing cells blank.
    #[must_use]
    pub fn with_row(mut self, row: u16, cells: impl IntoIterator<Item = TermCell>) -> Self {
        for (col, cell) in (0..self.cols).zip(cells) {
            if let Some(i) = self.index(col, row) {
                self.cells[i] = cell;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CellAttrs, CellWidth, ColorTarget, CursorShape, GridBuffer, GridCursor, Palette,
        ScreenKind, TermCell, TermColor,
    };
    use crate::style::Color;

    #[test]
    fn default_term_color_is_default_variant() {
        assert_eq!(TermColor::default(), TermColor::Default);
    }

    #[test]
    fn palette_resolves_default_per_target() {
        let p = Palette::xterm_default();
        assert_eq!(p.resolve(TermColor::Default, ColorTarget::Foreground), p.default_fg());
        assert_eq!(p.resolve(TermColor::Default, ColorTarget::Background), p.default_bg());
        // The xterm convention: light grey on black.
        assert_eq!(p.default_fg(), Color::rgb(0xe5, 0xe5, 0xe5));
        assert_eq!(p.default_bg(), Color::rgb(0x00, 0x00, 0x00));
    }

    #[test]
    fn palette_resolves_rgb_verbatim() {
        let p = Palette::xterm_default();
        let c = Color::rgb(0x12, 0x34, 0x56);
        // Truecolor is palette-independent: same colour for either target.
        assert_eq!(p.resolve(TermColor::Rgb(c), ColorTarget::Foreground), c);
        assert_eq!(p.resolve(TermColor::Rgb(c), ColorTarget::Background), c);
    }

    #[test]
    fn palette_indexed_ansi16_resolves_xterm_defaults() {
        // The `0..=15` slots resolve through the stored ANSI base (the
        // `indexed → rgb` SSOT — not a hardcoded table). Theme-swap
        // restaining lands with its consumer slice (the mutator builders
        // were not shipped speculatively).
        let p = Palette::xterm_default();
        assert_eq!(p.indexed(0), Color::rgb(0x00, 0x00, 0x00)); // black
        assert_eq!(p.indexed(1), Color::rgb(0xcd, 0x00, 0x00)); // red
        assert_eq!(p.indexed(7), Color::rgb(0xe5, 0xe5, 0xe5)); // white
        assert_eq!(p.indexed(9), Color::rgb(0xff, 0x00, 0x00)); // bright red
        assert_eq!(p.indexed(15), Color::rgb(0xff, 0xff, 0xff)); // bright white
    }

    #[test]
    fn palette_indexed_color_cube_uses_xterm_formula() {
        let p = Palette::xterm_default();
        // 16 = cube origin (0,0,0).
        assert_eq!(p.indexed(16), Color::rgb(0, 0, 0));
        // 21 = (0,0,5) -> pure blue at full cube intensity.
        assert_eq!(p.indexed(21), Color::rgb(0, 0, 255));
        // 196 = (5,0,0) -> pure red.
        assert_eq!(p.indexed(196), Color::rgb(255, 0, 0));
        // 231 = (5,5,5) -> white.
        assert_eq!(p.indexed(231), Color::rgb(255, 255, 255));
        // A mid-cube step uses 55 + step*40 (here step 1 = 95).
        assert_eq!(p.indexed(59), Color::rgb(95, 95, 95));
    }

    #[test]
    fn palette_indexed_grayscale_ramp() {
        let p = Palette::xterm_default();
        assert_eq!(p.indexed(232), Color::rgb(8, 8, 8)); // darkest gray
        assert_eq!(p.indexed(255), Color::rgb(238, 238, 238)); // lightest gray
        assert_eq!(p.indexed(243), Color::rgb(118, 118, 118)); // 8 + 11*10
    }

    #[test]
    fn grid_buffer_new_is_blank_and_bounded() {
        let b = GridBuffer::new(3, 2);
        assert_eq!((b.cols(), b.rows()), (3, 2));
        assert!(!b.is_empty());
        assert_eq!(b.cell(0, 0), Some(&TermCell::blank()));
        assert_eq!(b.cell(2, 1), Some(&TermCell::blank()));
        // Out of bounds on either axis.
        assert_eq!(b.cell(3, 0), None);
        assert_eq!(b.cell(0, 2), None);
    }

    #[test]
    fn grid_buffer_default_is_empty_zero_by_zero() {
        let b = GridBuffer::default();
        assert_eq!((b.cols(), b.rows()), (0, 0));
        assert!(b.is_empty());
        assert_eq!(b.cell(0, 0), None);
    }

    #[test]
    fn grid_buffer_with_row_places_row_major() {
        let red = TermCell::new("X", TermColor::Indexed(1), TermColor::Default);
        // Place `red` at (col 1, row 1) by writing row 1 as [blank, red].
        let b = GridBuffer::new(2, 2).with_row(1, [TermCell::blank(), red.clone()]);
        assert_eq!(b.cell(1, 1), Some(&red));
        // Neighbours stay blank — confirms row-major `row*cols + col`
        // (red landed at row 1 / col 1, not row 0 or column 0).
        assert_eq!(b.cell(0, 1), Some(&TermCell::blank()));
        assert_eq!(b.cell(1, 0), Some(&TermCell::blank()));
        assert_eq!(b.cell(0, 0), Some(&TermCell::blank()));
        // Writing an out-of-bounds row is ignored, not a panic.
        let same = b.clone().with_row(9, [red]);
        assert_eq!(same, b);
    }

    #[test]
    fn grid_buffer_with_row_fills_left_to_right() {
        let cells = [
            TermCell::new("a", TermColor::Indexed(1), TermColor::Default),
            TermCell::new("b", TermColor::Indexed(2), TermColor::Default),
        ];
        let b = GridBuffer::new(3, 1).with_row(0, cells);
        assert_eq!(b.cell(0, 0).unwrap().cluster, "a");
        assert_eq!(b.cell(1, 0).unwrap().cluster, "b");
        // The short row leaves the trailing cell blank.
        assert_eq!(b.cell(2, 0), Some(&TermCell::blank()));
    }

    #[test]
    fn cell_attrs_default_and_empty_are_all_off() {
        assert_eq!(CellAttrs::default(), CellAttrs::empty());
        assert!(CellAttrs::empty().is_empty());
        assert!(!CellAttrs::empty().with_bold(true).is_empty());
        // Builders set independent, non-interfering flags.
        let a = CellAttrs::empty().with_bold(true).with_reverse(true);
        assert!(a.bold && a.reverse);
        assert!(!a.italic && !a.underline && !a.dim);
        assert!(!a.is_empty());
        // A flag can be cleared back off.
        assert!(a.with_bold(false).with_reverse(false).is_empty());
    }

    #[test]
    fn grid_cell_carries_attrs_and_blank_is_empty() {
        assert!(TermCell::blank().attrs.is_empty());
        assert!(TermCell::new("x", TermColor::Default, TermColor::Default).attrs.is_empty());
        let styled = TermCell::new("x", TermColor::Default, TermColor::Default)
            .with_attrs(CellAttrs::empty().with_italic(true).with_underline(true));
        assert!(styled.attrs.italic && styled.attrs.underline);
        assert!(!styled.attrs.bold);
    }

    #[test]
    fn cursor_shape_default_is_block() {
        // Block is the conventional terminal default. The enum is the
        // complete closed DECSCUSR set (exhaustively matchable — the
        // `grid_cursor_snapshot` wire map matches all three by name).
        assert_eq!(CursorShape::default(), CursorShape::Block);
        assert_ne!(CursorShape::Bar, CursorShape::Underline);
    }

    #[test]
    fn grid_cursor_default_is_hidden_home_block() {
        // The default is the "no cursor reported yet" state: a projection
        // shows no cursor until its producer asks for one.
        let c = GridCursor::default();
        assert_eq!((c.col, c.row), (0, 0));
        assert_eq!(c.shape, CursorShape::Block);
        assert!(!c.visible);
    }

    #[test]
    fn grid_cursor_new_carries_every_field() {
        let c = GridCursor::new(7, 3, CursorShape::Bar, true);
        assert_eq!((c.col, c.row), (7, 3));
        assert_eq!(c.shape, CursorShape::Bar);
        assert!(c.visible);
        // A hidden underline cursor round-trips its fields too.
        let h = GridCursor::new(1, 2, CursorShape::Underline, false);
        assert_eq!(h.shape, CursorShape::Underline);
        assert!(!h.visible);
    }

    #[test]
    fn grid_buffer_default_cursor_is_hidden_then_settable() {
        // A fresh buffer carries the hidden default cursor...
        let b = GridBuffer::new(4, 2);
        assert_eq!(b.cursor(), GridCursor::default());
        assert!(!b.cursor().visible);
        // ...and the producer sets the effective cursor wholesale.
        let cur = GridCursor::new(2, 1, CursorShape::Bar, true);
        let b = b.with_cursor(cur);
        assert_eq!(b.cursor(), cur);
        // The cursor is independent of the cells (setting it leaves the
        // blank projection intact).
        assert_eq!(b.cell(0, 0), Some(&TermCell::blank()));
        // The geometry-only default buffer also carries the hidden cursor.
        assert_eq!(GridBuffer::default().cursor(), GridCursor::default());
    }

    /// `\u{4e16}` is the CJK ideograph 世 (East Asian Wide) — kept as an
    /// escape per the non-ASCII-literal rule (raw glyphs in docs only).
    const WIDE_CLUSTER: &str = "\u{4e16}";

    #[test]
    fn cell_width_default_is_narrow() {
        // Narrow is the default role; the enum is the complete closed set
        // (exhaustively matchable, like CursorShape / TermColor).
        assert_eq!(CellWidth::default(), CellWidth::Narrow);
        assert_eq!(TermCell::blank().width, CellWidth::Narrow);
        assert_eq!(
            TermCell::new("a", TermColor::Default, TermColor::Default).width,
            CellWidth::Narrow,
        );
    }

    #[test]
    fn wide_head_and_trailer_pair_up() {
        let head = TermCell::new(WIDE_CLUSTER, TermColor::Indexed(1), TermColor::Indexed(4))
            .with_attrs(CellAttrs::empty().with_bold(true))
            .wide();
        assert_eq!(head.width, CellWidth::Wide);
        assert_eq!(head.cluster, WIDE_CLUSTER);

        // The trailer derives from the head: empty cluster, the head's
        // colours + attrs (so the background paints across), Trailer role.
        let trail = head.trailer();
        assert_eq!(trail.width, CellWidth::Trailer);
        assert_eq!(trail.cluster, ""); // no independent glyph
        assert_eq!(trail.fg, head.fg);
        assert_eq!(trail.bg, head.bg);
        assert_eq!(trail.attrs, head.attrs);
    }

    #[test]
    fn wide_cluster_contributes_its_glyph_once_across_two_columns() {
        // A 3-column row: a wide head + its trailer + one narrow cell. The
        // assembled text has two glyphs (世 once, then x) for three columns
        // — the trailer's empty cluster contributes nothing.
        let head = TermCell::new(WIDE_CLUSTER, TermColor::Default, TermColor::Default).wide();
        let b = GridBuffer::new(3, 1).with_row(
            0,
            [
                head.clone(),
                head.trailer(),
                TermCell::new("x", TermColor::Default, TermColor::Default),
            ],
        );
        assert_eq!(b.cell(0, 0).unwrap().width, CellWidth::Wide);
        assert_eq!(b.cell(1, 0).unwrap().width, CellWidth::Trailer);
        assert_eq!(b.cell(2, 0).unwrap().width, CellWidth::Narrow);
        let row: String = (0..3).map(|c| b.cell(c, 0).unwrap().cluster.clone()).collect();
        assert_eq!(row, format!("{WIDE_CLUSTER}x")); // two glyphs, three columns
    }

    #[test]
    fn screen_kind_default_is_main() {
        // Main is the conventional initial screen; the enum is the complete
        // closed set (exhaustively matchable, like CursorShape / CellWidth).
        assert_eq!(ScreenKind::default(), ScreenKind::Main);
        assert_eq!(GridBuffer::new(2, 1).screen(), ScreenKind::Main);
        assert_eq!(GridBuffer::default().screen(), ScreenKind::Main);
    }

    #[test]
    fn grid_buffer_screen_is_settable_and_independent_of_cells_and_cursor() {
        let cur = GridCursor::new(0, 0, CursorShape::Block, true);
        let b = GridBuffer::new(4, 2)
            .with_screen(ScreenKind::Alternate)
            .with_cursor(cur);
        // The screen kind, the cursor, and the cells are independent facts
        // the producer reports together for one screen projection.
        assert_eq!(b.screen(), ScreenKind::Alternate);
        assert_eq!(b.cursor(), cur);
        assert_eq!(b.cell(0, 0), Some(&TermCell::blank()));
        // Switching the screen kind leaves the rest intact.
        assert_eq!(b.with_screen(ScreenKind::Main).screen(), ScreenKind::Main);
    }

    #[test]
    fn row_generations_default_to_zero_and_are_bounded() {
        let b = GridBuffer::new(4, 3);
        // Every row starts at generation 0 (untouched baseline).
        assert_eq!(b.row_generation(0), Some(0));
        assert_eq!(b.row_generation(2), Some(0));
        // Out of bounds is None, not a panic.
        assert_eq!(b.row_generation(3), None);
        // A geometry-only buffer has no row generations.
        assert_eq!(GridBuffer::default().row_generation(0), None);
    }

    #[test]
    fn with_row_generation_stamps_one_row_and_enables_a_baseline_diff() {
        // A streaming terminal: three rows stamped with increasing
        // generations (row 2 changed most recently).
        let b = GridBuffer::new(8, 3)
            .with_row_generation(0, 10)
            .with_row_generation(1, 20)
            .with_row_generation(2, 30);
        assert_eq!(b.row_generation(0), Some(10));
        assert_eq!(b.row_generation(1), Some(20));
        assert_eq!(b.row_generation(2), Some(30));

        // A client at baseline 15 re-reads only the rows newer than that.
        let changed: Vec<u16> = (0..b.rows())
            .filter(|&r| b.row_generation(r).unwrap_or(0) > 15)
            .collect();
        assert_eq!(changed, vec![1, 2]);
        // At baseline 30 nothing is newer.
        let none_newer: Vec<u16> = (0..b.rows())
            .filter(|&r| b.row_generation(r).unwrap_or(0) > 30)
            .collect();
        assert!(none_newer.is_empty());

        // An out-of-bounds stamp is ignored, not a panic.
        let same = b.clone().with_row_generation(9, 99);
        assert_eq!(same, b);
    }
}
