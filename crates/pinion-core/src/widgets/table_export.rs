//! R1787 §5.38 — a tabular export that **names every cell its dialect could not
//! carry**.
//!
//! # What was here before, and what was wrong with it
//!
//! [`rows_to_tsv`](super::table::rows_to_tsv) has been this tree's only
//! tabular serialisation since R1372. It is *structure-preserving over
//! content-faithful*: a cell holding a tab or a newline is rewritten with a
//! space so the block's row/column shape always matches the rectangle it came
//! from. That trade is the right one — a raw join silently splits a row, which
//! is worse — but it was made **silently**. A consumer handed the string could
//! not ask whether anything had been altered, and its own doc deferred the
//! question ("full spreadsheet-style quoting is a later enhancement").
//!
//! The analysis-tool census recorded the state of this as *"no tabular export
//! derivation exists in any crate"* (`capture.t1.12`). Measured at R1787 that
//! was wrong in both directions: a derivation existed, and what it lacked was
//! not existence but **faithfulness and a report**.
//!
//! # The floor this is measured against
//!
//! Built and run at 6.11 rather than read: asked for a rectangle of four cells
//! *as data*, the reference toolkit's item-model layer answers with two
//! **binary** payloads, `hasText` false, a text length of `0`, and neither the
//! header labels nor the cell text recoverable from the bytes. Its tabular
//! widget, with every cell selected and a real copy chord delivered, leaves the
//! clipboard holding **no format at all**. So the floor for "export this range
//! as text" is nothing, and the bar this module has to clear is set by what a
//! person actually needs rather than by a competitor's surface.
//!
//! # The shape
//!
//! A [`Dialect`] says how cells are separated and **whether that separation can
//! be escaped**. [`write()`] returns an [`Export`]: the text, plus one [`Loss`]
//! per cell the dialect could not carry faithfully, each naming *what was in
//! it*. A dialect that can quote never produces a loss — proven by
//! [`read`], which recovers the original rows exactly, so "faithful" is a law
//! and not a claim. A dialect that cannot quote (the clipboard TSV form every
//! grid here already writes) still preserves the block's shape, and now says
//! which cells it paid for that with.

/// How a block of rows ends its lines.
///
/// Two arms rather than a free string: a terminator that is not one of these is
/// not a line ending any reader agrees on, and a value nobody can spell wrongly
/// is one fewer thing for [`read`] to have to forgive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineEnd {
    /// A single line feed — the form every tool on this platform reads.
    Lf,
    /// A carriage return followed by a line feed — what RFC 4180 specifies and
    /// what a spreadsheet on another platform expects.
    CrLf,
}

impl LineEnd {
    /// The literal characters this terminator writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }

    /// The wire name of this terminator.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::CrLf => "crlf",
        }
    }
}

/// Whether an export carries the column names as its first line.
///
/// A separate word rather than `Option<&[String]>` at every call site, because
/// "this export has no header row" and "this table has no column names" are
/// different facts and only the first one is a choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Headers {
    /// Write the column names as the first line.
    Include,
    /// Write body rows only.
    Omit,
}

impl Headers {
    /// Every arm, in wire order.
    pub const ALL: &'static [Self] = &[Self::Include, Self::Omit];

    /// Every arm's wire name, in [`ALL`](Self::ALL) order — the closed
    /// vocabulary an argument declaration draws its domain from, so the surface
    /// that accepts these words and the surface that publishes them are one
    /// list. (R1630's ratchet: never a literal spelled at the call site.)
    pub const WIRE_NAMES: &'static [&'static str] = &["include", "omit"];

    /// The wire name of this choice.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::Omit => "omit",
        }
    }

    /// The arm `name` names, or `None`.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.as_wire_name() == name)
    }
}

/// R1787 §5.38 — which cells an export covers.
///
/// A word rather than two methods, because a wire caller has to be able to
/// *name* the choice, and a surface that offers "export" without saying what it
/// exports is the shape this module is replacing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// The selected cell rectangle only. `None` when nothing is selected.
    Selection,
    /// Every row and column.
    All,
}

impl Scope {
    /// Every arm, in wire order.
    pub const ALL: &'static [Self] = &[Self::Selection, Self::All];

    /// Every arm's wire name, in [`ALL`](Self::ALL) order.
    pub const WIRE_NAMES: &'static [&'static str] = &["selection", "all"];

    /// The wire name of this scope.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::All => "all",
        }
    }

    /// The arm `name` names, or `None`.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.as_wire_name() == name)
    }
}

/// A canonical [`Dialect`] and the name it answers to on the wire.
///
/// A named struct rather than a tuple of a name and a constructor: the tuple
/// form was refused by `clippy::type_complexity`, and the refusal was right —
/// `.0` and `.1` at four call sites read as nothing, and a roster entry is the
/// kind of thing that grows a field (a description, a file extension) rather
/// than staying a pair forever.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NamedDialect {
    /// The wire name a caller passes as the `dialect` argument.
    pub name: &'static str,
    /// The dialect that name selects.
    pub dialect: Dialect,
}

/// What a dialect found in a cell that it could not carry through unchanged.
///
/// Named for **what was in the cell**, not for what the writer did about it: a
/// consumer deciding whether to offer a different dialect needs to know the
/// content was delimiter-bearing, and "we replaced it with a space" is this
/// module's answer rather than the question.
///
/// # Why there is no `Quote` arm
///
/// The first draft had one, and writing the test that counted losses is what
/// established it could never occur: a dialect **with** a quote escapes an
/// embedded quote by doubling it, so nothing is lost, and a dialect **without**
/// one gives the character no meaning to be confused by, so nothing is lost
/// there either. The one case where a quote could be ambiguous — the quote
/// character *being* the delimiter — is refused at construction by
/// [`DialectDefect::QuoteIsDelimiter`]. An arm no code path can produce forces
/// every consumer to write a branch that never runs, so it is not here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Carried {
    /// The cell contained the dialect's own delimiter.
    Delimiter,
    /// The cell contained a line break.
    LineBreak,
}

impl Carried {
    /// Every arm, in wire order. The roster the totality tests count against,
    /// so a new arm cannot be added without the counts moving.
    pub const ALL: &'static [Self] = &[Self::Delimiter, Self::LineBreak];

    /// The wire name of this content kind.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Delimiter => "delimiter",
            Self::LineBreak => "line_break",
        }
    }

    /// This arm's bit in a [`CarriedSet`].
    const fn bit(self) -> u8 {
        match self {
            Self::Delimiter => 1,
            Self::LineBreak => 2,
        }
    }
}

/// The set of [`Carried`] kinds found in **one** cell.
///
/// A set rather than one [`Loss`] per kind, because a cell can hold a tab *and*
/// a newline and reporting it twice makes `losses().len()` answer a question
/// nobody asked. With a set that length is the number of **cells** altered,
/// which is the number a person reading a warning wants. (R1742 hit the
/// mirror-image of this bug — a part that diverged twice was subtracted
/// twice — which is why the shape is chosen here rather than discovered later.)
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct CarriedSet(u8);

impl CarriedSet {
    /// The empty set — a cell the dialect carried unchanged.
    #[must_use]
    pub const fn none() -> Self {
        Self(0)
    }

    /// This set with `kind` added.
    #[must_use]
    pub const fn with(self, kind: Carried) -> Self {
        Self(self.0 | kind.bit())
    }

    /// Whether `kind` is in this set.
    #[must_use]
    pub const fn contains(self, kind: Carried) -> bool {
        self.0 & kind.bit() != 0
    }

    /// Whether this set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// How many kinds are in this set.
    #[must_use]
    pub fn len(self) -> usize {
        Carried::ALL.iter().filter(|k| self.contains(**k)).count()
    }

    /// The kinds in this set, in [`Carried::ALL`] order.
    #[must_use]
    pub fn kinds(self) -> Vec<Carried> {
        Carried::ALL
            .iter()
            .copied()
            .filter(|k| self.contains(*k))
            .collect()
    }
}

/// Which line of the export a cell sits on.
///
/// The header row is addressable because a column *name* can contain a
/// delimiter too, and an export that reported losses only for body cells would
/// be silent about the one line every reader parses first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    /// The column-name line, written only under [`Headers::Include`].
    Header,
    /// A body row, by its index in the rows handed to [`write()`].
    Body(usize),
}

impl Line {
    /// The wire form of this address: `"header"` or the row index as text.
    #[must_use]
    pub fn as_wire_name(self) -> String {
        match self {
            Self::Header => "header".to_string(),
            Self::Body(row) => row.to_string(),
        }
    }
}

/// One cell a dialect could not carry unchanged, and what was in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loss {
    /// The line the cell sits on.
    pub line: Line,
    /// The cell's column index.
    pub col: usize,
    /// Every kind of content the dialect could not carry through.
    pub carried: CarriedSet,
}

/// Why a [`Dialect`] could not be constructed.
///
/// Fail-fast rather than a silently-normalised dialect: every one of these
/// produces a block that a conforming reader parses into something other than
/// what was written, which is the exact failure this module exists to make
/// impossible to have unknowingly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialectDefect {
    /// The quote character is the delimiter, so a quoted cell cannot be told
    /// from an empty one.
    QuoteIsDelimiter,
    /// The delimiter is a line break, so a row cannot be told from a column.
    DelimiterIsLineBreak,
    /// The quote character is a line break, so a quoted run cannot be closed.
    QuoteIsLineBreak,
}

impl DialectDefect {
    /// The wire name of this defect.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::QuoteIsDelimiter => "quote_is_delimiter",
            Self::DelimiterIsLineBreak => "delimiter_is_line_break",
            Self::QuoteIsLineBreak => "quote_is_line_break",
        }
    }
}

/// How a block of cells is separated, and whether that separation can be
/// escaped.
///
/// The quote is an `Option` and that is the whole point: a dialect that has one
/// carries any content faithfully, and a dialect that has none is *declared*
/// lossy rather than discovered to be. The clipboard form every grid in this
/// tree already writes is the second kind, and [`clipboard`](Self::clipboard)
/// is it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dialect {
    delimiter: char,
    quote: Option<char>,
    terminator: LineEnd,
}

impl Dialect {
    /// RFC 4180: comma-separated, double-quoted, CRLF-terminated. What a
    /// spreadsheet on any platform reads back unchanged.
    #[must_use]
    pub const fn comma() -> Self {
        Self {
            delimiter: ',',
            quote: Some('"'),
            terminator: LineEnd::CrLf,
        }
    }

    /// Tab-separated with RFC 4180 quoting and LF line endings — a faithful
    /// tab form, which is what [`clipboard`](Self::clipboard) is not.
    #[must_use]
    pub const fn tab() -> Self {
        Self {
            delimiter: '\t',
            quote: Some('"'),
            terminator: LineEnd::Lf,
        }
    }

    /// The **lossy** clipboard form: tab-separated, LF-terminated, *no quote*.
    ///
    /// Byte-for-byte what [`rows_to_tsv`](super::table::rows_to_tsv) has always
    /// written — a cell holding a delimiter or a line break is rewritten with a
    /// space so the block's shape survives — with the one difference that the
    /// cells it did that to are now named. Kept as its own constructor because
    /// pasting into a spreadsheet is a real requirement and quoting breaks it:
    /// a spreadsheet paste treats `"` literally.
    #[must_use]
    pub const fn clipboard() -> Self {
        Self {
            delimiter: '\t',
            quote: None,
            terminator: LineEnd::Lf,
        }
    }

    /// A dialect over an arbitrary delimiter, quote and terminator.
    ///
    /// # Errors
    /// [`DialectDefect`] when the three characters cannot be told apart well
    /// enough for a reader to recover what a writer wrote.
    pub const fn try_new(
        delimiter: char,
        quote: Option<char>,
        terminator: LineEnd,
    ) -> Result<Self, DialectDefect> {
        if delimiter == '\n' || delimiter == '\r' {
            return Err(DialectDefect::DelimiterIsLineBreak);
        }
        if let Some(q) = quote {
            if q == delimiter {
                return Err(DialectDefect::QuoteIsDelimiter);
            }
            if q == '\n' || q == '\r' {
                return Err(DialectDefect::QuoteIsLineBreak);
            }
        }
        Ok(Self {
            delimiter,
            quote,
            terminator,
        })
    }

    /// The character between cells on a line.
    #[must_use]
    pub const fn delimiter(self) -> char {
        self.delimiter
    }

    /// The character that protects a cell, or `None` in a dialect that has
    /// none — the single fact that decides whether this dialect can be
    /// faithful.
    #[must_use]
    pub const fn quote(self) -> Option<char> {
        self.quote
    }

    /// The characters between lines.
    #[must_use]
    pub const fn terminator(self) -> LineEnd {
        self.terminator
    }

    /// Whether this dialect can carry **any** cell content unchanged.
    ///
    /// Equivalently: whether [`write()`] can ever report a [`Loss`] for it. A
    /// consumer offering an export menu reads this to say so up front rather
    /// than after the fact.
    #[must_use]
    pub const fn is_faithful(self) -> bool {
        self.quote.is_some()
    }

    /// The canonical dialects, by wire name.
    ///
    /// The roster the wire surface publishes and the argument domain a client
    /// picks from, so the two cannot drift: there is one list.
    pub const NAMED: &'static [NamedDialect] = &[
        NamedDialect {
            name: "comma",
            dialect: Self::comma(),
        },
        NamedDialect {
            name: "tab",
            dialect: Self::tab(),
        },
        NamedDialect {
            name: "clipboard",
            dialect: Self::clipboard(),
        },
    ];

    /// The dialect `name` names, or `None`.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::NAMED
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.dialect)
    }

    /// What this cell holds that this dialect cannot carry through unchanged.
    ///
    /// Empty for every cell of a [`is_faithful`](Self::is_faithful) dialect,
    /// which is why the quoting writer can skip the check entirely.
    fn losses_in(self, cell: &str) -> CarriedSet {
        if self.is_faithful() {
            return CarriedSet::none();
        }
        let mut set = CarriedSet::none();
        if cell.contains(self.delimiter) {
            set = set.with(Carried::Delimiter);
        }
        if cell.contains('\n') || cell.contains('\r') {
            set = set.with(Carried::LineBreak);
        }
        set
    }

    /// `cell` rendered for this dialect: quoted when it needs protecting, or
    /// flattened when this dialect has no quote to protect it with.
    fn render(self, cell: &str) -> String {
        let Some(q) = self.quote else {
            // The R1372 trade, unchanged: shape over content. What is new is
            // that `losses_in` said so.
            return cell
                .chars()
                .map(|c| {
                    if c == self.delimiter || c == '\n' || c == '\r' {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
        };
        let needs = cell.contains(self.delimiter)
            || cell.contains('\n')
            || cell.contains('\r')
            || cell.contains(q);
        if !needs {
            return cell.to_string();
        }
        let mut out = String::with_capacity(cell.len() + 2);
        out.push(q);
        for c in cell.chars() {
            if c == q {
                out.push(q);
            }
            out.push(c);
        }
        out.push(q);
        out
    }
}

/// The text a dialect produced, and every cell it could not carry faithfully.
///
/// The pair is the deliverable. A bare `String` cannot be interrogated, and the
/// caller that most needs to interrogate it — a person about to hand the block
/// to something else — is exactly the one who has already thrown the rows away.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Export {
    text: String,
    losses: Vec<Loss>,
}

impl Export {
    /// The serialised block.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The serialised block, taken.
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }

    /// One entry per **cell** the dialect could not carry unchanged, in
    /// row-major order.
    #[must_use]
    pub fn losses(&self) -> &[Loss] {
        &self.losses
    }

    /// Whether every cell survived unchanged.
    ///
    /// Always `true` for a [`Dialect::is_faithful`] dialect, and that is a law
    /// rather than a comment: `read(d, write(d, rows).text())` recovers `rows`.
    #[must_use]
    pub fn is_faithful(&self) -> bool {
        self.losses.is_empty()
    }

    /// The `scene/invoke` form of this export: the text, whether every cell
    /// survived, and one entry per cell that did not.
    ///
    /// Rendered here rather than by the transport, for R1642's reason — the
    /// types render themselves, so a field added to [`Loss`] cannot go missing
    /// in a crate that does not own it.
    #[must_use]
    pub fn to_wire(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "text".to_owned(),
            serde_json::Value::from(self.text.clone()),
        );
        obj.insert(
            "faithful".to_owned(),
            serde_json::Value::from(self.is_faithful()),
        );
        obj.insert(
            "losses".to_owned(),
            serde_json::Value::Array(self.losses.iter().map(Loss::to_wire).collect()),
        );
        serde_json::Value::Object(obj)
    }
}

impl Loss {
    /// The wire form of this loss: where the cell was, and what was in it.
    #[must_use]
    pub fn to_wire(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "line".to_owned(),
            serde_json::Value::from(self.line.as_wire_name()),
        );
        obj.insert("col".to_owned(), serde_json::Value::from(self.col));
        obj.insert(
            "carried".to_owned(),
            serde_json::Value::Array(
                self.carried
                    .kinds()
                    .into_iter()
                    .map(|k| serde_json::Value::from(k.as_wire_name()))
                    .collect(),
            ),
        );
        serde_json::Value::Object(obj)
    }
}

/// The `export_dialects` wire form: every canonical dialect, with the one fact
/// a caller needs before picking — whether it can carry any cell unchanged.
///
/// Derived from [`Dialect::NAMED`] rather than written out, so a dialect added
/// there appears here without anyone remembering to.
#[must_use]
pub fn dialects_to_wire() -> serde_json::Value {
    serde_json::Value::Array(
        Dialect::NAMED
            .iter()
            .map(|entry| {
                let mut obj = serde_json::Map::new();
                obj.insert("name".to_owned(), serde_json::Value::from(entry.name));
                obj.insert(
                    "faithful".to_owned(),
                    serde_json::Value::from(entry.dialect.is_faithful()),
                );
                obj.insert(
                    "terminator".to_owned(),
                    serde_json::Value::from(entry.dialect.terminator().as_wire_name()),
                );
                serde_json::Value::Object(obj)
            })
            .collect(),
    )
}

/// Serialise `rows` in `dialect`, optionally preceded by `headers`.
///
/// `headers` is `None` when the caller has no column names *or* chose
/// [`Headers::Omit`]; the choice is the caller's to make and this takes the
/// result of it. Ragged input is written as given — a short row writes fewer
/// cells — because clipping it would be a second silent alteration of exactly
/// the kind this module exists to end.
#[must_use]
pub fn write(dialect: Dialect, headers: Option<&[String]>, rows: &[Vec<String>]) -> Export {
    let mut text = String::new();
    let mut losses = Vec::new();
    let mut first = true;
    if let Some(head) = headers {
        push_line(dialect, head, Line::Header, &mut text, &mut losses);
        first = false;
    }
    for (r, row) in rows.iter().enumerate() {
        if !first {
            text.push_str(dialect.terminator().as_str());
        }
        first = false;
        push_line(dialect, row, Line::Body(r), &mut text, &mut losses);
    }
    Export { text, losses }
}

/// Render one line into `text`, recording a [`Loss`] per altered cell.
fn push_line(
    dialect: Dialect,
    cells: &[String],
    line: Line,
    text: &mut String,
    losses: &mut Vec<Loss>,
) {
    for (c, cell) in cells.iter().enumerate() {
        if c > 0 {
            text.push(dialect.delimiter());
        }
        let carried = dialect.losses_in(cell);
        if !carried.is_empty() {
            losses.push(Loss {
                line,
                col: c,
                carried,
            });
        }
        text.push_str(&dialect.render(cell));
    }
}

/// Why a block could not be read back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadDefect {
    /// A quoted cell was never closed.
    UnterminatedQuote {
        /// The line the cell started on, 0-based.
        line: usize,
        /// The cell's column index.
        col: usize,
    },
    /// A closing quote was followed by something other than a delimiter or a
    /// line end.
    TextAfterClosingQuote {
        /// The line the cell is on, 0-based.
        line: usize,
        /// The cell's column index.
        col: usize,
    },
    /// The dialect has no quote, so no block written in it can be read back
    /// without guessing which spaces used to be delimiters.
    ///
    /// A refusal rather than a best effort: a lossy dialect's output is not a
    /// container, and pretending otherwise is how a round trip comes to look
    /// like it works on the cells that happened to be plain.
    NotReadable,
}

impl ReadDefect {
    /// The wire name of this defect kind.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::UnterminatedQuote { .. } => "unterminated_quote",
            Self::TextAfterClosingQuote { .. } => "text_after_closing_quote",
            Self::NotReadable => "not_readable",
        }
    }
}

/// Recover the rows a [`write()`] in this `dialect` produced.
///
/// The inverse half of the pair, and the reason [`Export::is_faithful`] is a
/// law: for any faithful dialect and any rows, reading back what was written
/// yields the rows unchanged, including cells holding delimiters, line breaks
/// and quotes. A trailing terminator is consumed rather than yielding an empty
/// final row.
///
/// # Errors
/// [`ReadDefect`] for a dialect that has no quote, or for input whose quoting
/// no reader can resolve.
pub fn read(dialect: Dialect, text: &str) -> Result<Vec<Vec<String>>, ReadDefect> {
    let quote = dialect.quote().ok_or(ReadDefect::NotReadable)?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut line = 0usize;
    while let Some(c) = chars.next() {
        if c == quote && cell.is_empty() {
            read_quoted(&mut chars, quote, &mut cell, line, row.len())?;
            match chars.peek() {
                None => {}
                Some(&n) if n == dialect.delimiter() || n == '\n' || n == '\r' => {}
                Some(_) => {
                    return Err(ReadDefect::TextAfterClosingQuote {
                        line,
                        col: row.len(),
                    });
                }
            }
            continue;
        }
        if c == dialect.delimiter() {
            row.push(core::mem::take(&mut cell));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(core::mem::take(&mut cell));
            rows.push(core::mem::take(&mut row));
            line += 1;
        } else {
            cell.push(c);
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}

/// Consume a quoted run, un-doubling escaped quotes, leaving the iterator on
/// the character after the closing quote.
fn read_quoted(
    chars: &mut core::iter::Peekable<core::str::Chars<'_>>,
    quote: char,
    cell: &mut String,
    line: usize,
    col: usize,
) -> Result<(), ReadDefect> {
    loop {
        let Some(c) = chars.next() else {
            return Err(ReadDefect::UnterminatedQuote { line, col });
        };
        if c != quote {
            cell.push(c);
            continue;
        }
        if chars.peek() == Some(&quote) {
            chars.next();
            cell.push(quote);
            continue;
        }
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Vec<String>> {
        vec![
            vec!["plain".to_string(), "has\ttab".to_string()],
            vec!["has\nbreak".to_string(), "has\"quote".to_string()],
        ]
    }

    #[test]
    fn r1787_a_faithful_dialect_round_trips_every_awkward_cell() {
        for dialect in [Dialect::comma(), Dialect::tab()] {
            let export = write(dialect, None, &rows());
            assert!(
                export.is_faithful(),
                "a quoting dialect must never report a loss"
            );
            assert_eq!(
                read(dialect, export.text()),
                Ok(rows()),
                "reading back what was written must yield the rows unchanged"
            );
        }
    }

    #[test]
    fn r1787_a_faithful_dialect_round_trips_its_header_line_too() {
        let head = vec!["a,b".to_string(), "c\"d".to_string()];
        let export = write(Dialect::comma(), Some(&head), &rows());
        assert!(export.is_faithful());
        let back = read(Dialect::comma(), export.text()).expect("readable");
        assert_eq!(back[0], head);
        assert_eq!(back[1..], rows()[..]);
    }

    #[test]
    fn r1787_the_clipboard_dialect_names_every_cell_it_flattened() {
        let export = write(Dialect::clipboard(), None, &rows());
        assert!(!export.is_faithful());
        // TWO of the four cells, not three. `has"quote` survives a dialect that
        // has no quote character, because nothing there gives `"` a meaning —
        // which is the measurement that removed the `Carried::Quote` arm.
        assert_eq!(export.losses().len(), 2);
        assert_eq!(export.losses()[0].line, Line::Body(0));
        assert_eq!(export.losses()[0].col, 1);
        assert!(export.losses()[0].carried.contains(Carried::Delimiter));
        assert_eq!(export.losses()[1].line, Line::Body(1));
        assert_eq!(export.losses()[1].col, 0);
        assert!(export.losses()[1].carried.contains(Carried::LineBreak));
        assert!(
            !export
                .losses()
                .iter()
                .any(|l| l.line == Line::Body(1) && l.col == 1),
            "a quote character is not a loss in a dialect that has no quote"
        );
    }

    #[test]
    fn r1787_a_cell_holding_two_awkward_things_is_one_loss_naming_both() {
        let both = vec![vec!["a\tb\nc".to_string()]];
        let export = write(Dialect::clipboard(), None, &both);
        assert_eq!(
            export.losses().len(),
            1,
            "one entry per CELL, so the length is the number of cells altered"
        );
        assert_eq!(export.losses()[0].carried.len(), 2);
        assert_eq!(
            export.losses()[0].carried.kinds(),
            vec![Carried::Delimiter, Carried::LineBreak]
        );
    }

    #[test]
    fn r1787_the_clipboard_dialect_keeps_the_blocks_shape() {
        let export = write(Dialect::clipboard(), None, &rows());
        let lines: Vec<&str> = export.text().split('\n').collect();
        assert_eq!(lines.len(), 2, "two rows in, two lines out");
        for line in lines {
            assert_eq!(line.split('\t').count(), 2, "two columns on every line");
        }
    }

    #[test]
    fn r1787_a_lossy_dialect_refuses_to_be_read_rather_than_guessing() {
        let export = write(Dialect::clipboard(), None, &rows());
        assert_eq!(
            read(Dialect::clipboard(), export.text()),
            Err(ReadDefect::NotReadable)
        );
    }

    #[test]
    fn r1787_a_dialect_that_cannot_be_told_apart_is_refused() {
        assert_eq!(
            Dialect::try_new(',', Some(','), LineEnd::Lf),
            Err(DialectDefect::QuoteIsDelimiter)
        );
        assert_eq!(
            Dialect::try_new('\n', Some('"'), LineEnd::Lf),
            Err(DialectDefect::DelimiterIsLineBreak)
        );
        assert_eq!(
            Dialect::try_new(';', Some('\r'), LineEnd::Lf),
            Err(DialectDefect::QuoteIsLineBreak)
        );
        assert!(Dialect::try_new(';', Some('\''), LineEnd::CrLf).is_ok());
    }

    #[test]
    fn r1787_unclosed_and_trailing_quotes_are_named_rather_than_guessed() {
        assert_eq!(
            read(Dialect::comma(), "\"never closed"),
            Err(ReadDefect::UnterminatedQuote { line: 0, col: 0 })
        );
        assert_eq!(
            read(Dialect::comma(), "a,\"b\"c"),
            Err(ReadDefect::TextAfterClosingQuote { line: 0, col: 1 })
        );
    }

    #[test]
    fn r1787_a_trailing_terminator_does_not_invent_a_row() {
        let export = write(Dialect::comma(), None, &rows());
        let with_trailing = format!("{}{}", export.text(), LineEnd::CrLf.as_str());
        assert_eq!(read(Dialect::comma(), &with_trailing), Ok(rows()));
    }

    #[test]
    fn r1787_the_named_roster_is_what_by_name_answers() {
        for entry in Dialect::NAMED {
            assert_eq!(Dialect::by_name(entry.name), Some(entry.dialect));
        }
        assert_eq!(Dialect::by_name("no-such-dialect"), None);
        assert_eq!(Dialect::NAMED.len(), 3);
    }

    #[test]
    fn r1787_faithfulness_is_readable_off_the_dialect_before_any_rows_exist() {
        assert!(Dialect::comma().is_faithful());
        assert!(Dialect::tab().is_faithful());
        assert!(!Dialect::clipboard().is_faithful());
        // The claim the constructor makes and the behaviour must agree, on
        // content chosen to break a dialect that lies about it.
        for entry in Dialect::NAMED {
            let d = entry.dialect;
            assert_eq!(d.is_faithful(), write(d, None, &rows()).is_faithful());
        }
    }

    #[test]
    fn r1787_every_carried_kind_has_a_distinct_wire_name() {
        let mut names: Vec<&str> = Carried::ALL.iter().map(|k| k.as_wire_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Carried::ALL.len());
    }

    #[test]
    fn r1787_an_empty_table_writes_an_empty_block_and_reads_back_empty() {
        let export = write(Dialect::comma(), None, &[]);
        assert_eq!(export.text(), "");
        assert!(export.is_faithful());
        assert_eq!(read(Dialect::comma(), ""), Ok(Vec::new()));
    }

    #[test]
    fn r1787_a_ragged_row_is_written_as_given_rather_than_padded() {
        let ragged = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ];
        let export = write(Dialect::comma(), None, &ragged);
        assert_eq!(read(Dialect::comma(), export.text()), Ok(ragged));
    }

    #[test]
    fn r1787_a_line_names_itself_on_the_wire() {
        assert_eq!(Line::Header.as_wire_name(), "header");
        assert_eq!(Line::Body(7).as_wire_name(), "7");
    }

    #[test]
    fn r1787_each_closed_vocabulary_publishes_exactly_its_own_arms() {
        // The declared domain and the parser must be one list. Spelled twice
        // because a `const fn` cannot map over the arms, so this is the check
        // that keeps the two spellings from drifting.
        let head: Vec<&str> = Headers::ALL.iter().map(|a| a.as_wire_name()).collect();
        assert_eq!(head, Headers::WIRE_NAMES);
        let scope: Vec<&str> = Scope::ALL.iter().map(|a| a.as_wire_name()).collect();
        assert_eq!(scope, Scope::WIRE_NAMES);
        for name in Headers::WIRE_NAMES {
            assert_eq!(
                Headers::from_wire_name(name).map(Headers::as_wire_name),
                Some(*name)
            );
        }
        for name in Scope::WIRE_NAMES {
            assert_eq!(
                Scope::from_wire_name(name).map(Scope::as_wire_name),
                Some(*name)
            );
        }
        assert_eq!(Headers::from_wire_name("include-please"), None);
        assert_eq!(Scope::from_wire_name(""), None);
    }

    #[test]
    fn r1787_the_published_dialect_roster_is_the_one_by_name_answers() {
        let wire = dialects_to_wire();
        let arr = wire.as_array().expect("an array");
        assert_eq!(arr.len(), Dialect::NAMED.len());
        for entry in arr {
            let name = entry["name"].as_str().expect("a name");
            let d = Dialect::by_name(name).expect("the roster's own name resolves");
            assert_eq!(entry["faithful"], serde_json::Value::from(d.is_faithful()));
            assert_eq!(
                entry["terminator"],
                serde_json::Value::from(d.terminator().as_wire_name())
            );
        }
    }

    #[test]
    fn r1787_the_wire_form_carries_the_losses_and_not_only_the_text() {
        let wire = write(Dialect::clipboard(), None, &rows()).to_wire();
        assert_eq!(wire["faithful"], serde_json::Value::Bool(false));
        let losses = wire["losses"].as_array().expect("an array");
        assert_eq!(losses.len(), 2);
        assert_eq!(losses[0]["line"], serde_json::Value::from("0"));
        assert_eq!(losses[0]["col"], serde_json::Value::from(1));
        assert_eq!(
            losses[0]["carried"],
            serde_json::json!([Carried::Delimiter.as_wire_name()])
        );
        assert!(wire["text"].as_str().expect("text").contains('\t'));

        let faithful = write(Dialect::comma(), None, &rows()).to_wire();
        assert_eq!(faithful["faithful"], serde_json::Value::Bool(true));
        assert_eq!(faithful["losses"], serde_json::json!([]));
    }
}
