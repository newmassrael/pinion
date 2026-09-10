//! ★★★★★ R2124 §5.2 §5.11 — **where a painted mark's address comes from, for
//! the log section.**
//!
//! Public because the ASSEMBLED shell needs it, and because THE WALK needs it.
//!
//! ⚠ The header lives HERE rather than on the `pub mod` line: a module carrying
//! both an outer doc at its declaration and an inner `//!` header has the two
//! MERGED and resolved in the DECLARING file's scope, so every link to this
//! module's own items stops resolving. Measured at R2121's push gate.
//!
//! # ★★★★★ THE SHAPE QUESTION, ASKED BEFORE ANY GATE WAS WRITTEN
//!
//! R2123 wrote that question down as the first act of an instalment, and this
//! section is the first one where the answer is **all three populations**:
//!
//! | | crate | host | walk |
//! |---|--:|--:|--:|
//! | `sv` (R2121) · `tv` (R2122) | 71 · 94 | 0 | 0 |
//! | `kp` (R2123) | 44 | 0 | 6 |
//! | **`lv` (this round)** | **51** | **2** | **8** |
//!
//! So this instalment needs all three repairs at once, and each population has
//! its own gate because no one gate can see another's:
//!
//! * the **crate** — every literal below this line, asserted at zero by
//!   `r2124_no_module_but_the_declaration_spells_this_screens_namespace`;
//! * the **host** — `hello-analyzer-shell` mounts this section as a page and
//!   spelled two of its addresses. A gate in THIS crate cannot see that file,
//!   which is the whole of why those two survived twenty-five instalments: the
//!   crate gate reads `include_str!` of its own modules and the Python ratchet
//!   reads walks. The host's own gate is what holds them, and this module's
//!   [`NAMESPACE`] is the needle that gate takes;
//! * the **walk** — `tools/demos/r1731_*.py` is Python and cannot name a Rust
//!   const, so the only repair is that the screen HANDS it the address.
//!   [`published`] is what goes on the wire for that.
//!
//! ⚠ Of the eight walk sites, **six are charged** by the paint-address census
//! and two are not: `tag.startswith("lv.")` is a FAMILY-WIDE QUESTION, which
//! R2115's host rule deliberately allows and R2122 re-derived from the census
//! needle. They are left, and for R2123's second reason as well — *did this
//! screen paint anything at all* has to stay answerable when the screen has
//! stopped answering, and an address received from that screen's wire is no use
//! then.
//!
//! # ⚠ One stem, FOUR vocabularies — a deeper seat, not a classifier
//!
//! `lv.list.` names a PART (`header`, `body`, `open`) and three INDEXED
//! families (`row.<n>`, `cell.<n>_<col>`, `dot.<n>`). R2118 needed a classifier
//! for the node lab's wire family because `lab.link.7` and `lab.link.act` BOTH
//! survive `strip_prefix`; here every head is a WORD in the segment after the
//! stem, so taking the seat one segment DEEPER tells them apart and
//! [`list_part`] — which refuses anything with a further separator — cannot
//! collide with [`row`], [`cell`] or [`dot`]. **The same answer as `kp.list.`,
//! and it is the same shape rather than a copied habit: the question was asked
//! again here before any gate was written.**
//!
//! # 🟥 The join is an UNDERSCORE, and this section had the same three sites
//!
//! A cell is `lv.list.cell.<row>_<column>`. Most joined keys in this tree join
//! on the separator (`sv.row.<session>.<column>`, R2121), so a reader who knows
//! the convention parses this one wrong, and a dotted parse cannot recover it at
//! all. Three sites did the join by hand — the painter, the accessibility
//! roster, and the hit test's inverse, which called `split_once('_')` itself.
//! [`cell`] and [`cell_of`] are now the only places that know, and [`CELL_JOIN`]
//! is published so the walk does not spell it either.
//!
//! # ⚠⚠ The filter FIELD is not under the header part it sits in
//!
//! `spec::HEADER` names a part `filter`, painted at `lv.header.filter`; the text
//! field inside it is [`QUERY`] — `lv.filter.query`, a family of one outside the
//! header's seat. That is deliberate and predates this module: a surface's parts
//! are read back by walking the header seat and taking the names with no further
//! separator, so a field tagged inside its part would have to be excluded by
//! name. Declaring the divergence is what stops the next reader composing
//! `lv.header.filter.query` and finding nothing — the same service R2123 did for
//! that section's endpoints.

use crate::spec;

/// The stem this screen's marks hang under, without a separator.
///
/// ★ Wanted bare by `WidgetCore::paint_stems`, and the namespace gate's needle
/// is this rather than [`NAMESPACE`] precisely so that spelling is in the
/// population — R2122 measured that a namespace needle misses it, and this
/// screen had exactly one such site.
pub const STEM: &str = "lv";

/// The namespace every mark this screen paints begins with.
///
/// Carried WITH its separator, which is what a membership test wants: a bare
/// `lv` would also claim a mark called `lvalue`.
pub const NAMESPACE: &str = "lv.";

/// The root group every mark of this screen hangs under.
pub const ROOT: &str = "lv.root";

/// Where a resting description is painted and announced.
pub const TIP: &str = "lv.tip";

/// The section header's own band.
pub const HEADER: &str = "lv.header";

/// The event list's own grid.
pub const LIST: &str = "lv.list";

/// The decode pane.
pub const DETAIL: &str = "lv.detail";

/// The filter field a reader types a query into.
///
/// ⚠ Deliberately outside [`HEADER_SEAT`] — see this module's header. Declared
/// as a whole address rather than composed, because `lv.filter.` with one member
/// is a family in name only and a composer for it would invite a second.
pub const QUERY: &str = "lv.filter.query";

/// [`HEADER`] with the separator its parts hang off.
pub const HEADER_SEAT: &str = "lv.header.";

/// [`LIST`] with the separator its parts hang off.
pub const LIST_SEAT: &str = "lv.list.";

/// [`DETAIL`] with the separator its parts hang off.
pub const DETAIL_SEAT: &str = "lv.detail.";

/// Where a column of the list is addressed.
pub const COLUMN_SEAT: &str = "lv.column.";

/// Where a severity choice is addressed.
pub const SEVERITY_SEAT: &str = "lv.severity.";

/// Where an event row is addressed — one segment deeper than [`LIST_SEAT`],
/// which is what tells a row from a part of the list.
pub const ROW_SEAT: &str = "lv.list.row.";

/// Where one cell of an event row is addressed.
pub const CELL_SEAT: &str = "lv.list.cell.";

/// Where a row's severity dot is addressed.
pub const DOT_SEAT: &str = "lv.list.dot.";

/// What joins a cell's two keys.
///
/// 🟥 An UNDERSCORE, where most joined keys in this tree join on the separator.
/// Published so the walk does not spell it, and so a reader who knows this
/// tree's convention cannot apply it here by reflex.
pub const CELL_JOIN: char = '_';

/// The list's own heading row — one Tab stop, painted and announced under the
/// same tag since R2064.
///
/// ★ A `&'static str` because it is compared by identity as a focus stop and
/// looked up by name by two paint gates; a `const` cannot call [`list`], so it
/// is held against the derivation by this crate's gate.
pub const LIST_HEADER: &str = "lv.list.header";

/// The list's scrollable body — the key its scroll state is resolved under.
pub const LIST_BODY: &str = "lv.list.body";

/// The band drawn behind the event the list has open.
pub const LIST_OPEN: &str = "lv.list.open";

/// The header's live-capture reading, which announces itself when it changes.
pub const HEADER_LIVE: &str = "lv.header.live";

/// The severity choice's group — a radio group, and a Tab stop with a cursor.
pub const SEVERITY_GROUP: &str = "lv.header.severity";

/// The words a part of the decode pane puts inside itself.
///
/// ★ A CLOSED set of one. [`child`] refuses a second, which is what keeps
/// [`detail_part`] able to say where a part's address ends. An open suffix here
/// would make the part inverse unable to tell a part from its contents — the
/// ambiguity this module exists to remove rather than one to introduce.
pub const CHILD_WORDS: &[&str] = &["pill"];

/// The address the section header paints `part` under.
///
/// `part` is one of `spec::HEADER`'s keys — the painter names the part, this
/// composes the address, and nobody does both.
#[must_use]
pub fn header(part: &str) -> String {
    format!("{HEADER_SEAT}{part}")
}

/// The address the list pane paints `part` under.
#[must_use]
pub fn list(part: &str) -> String {
    format!("{LIST_SEAT}{part}")
}

/// The address the decode pane paints `part` under.
#[must_use]
pub fn detail(part: &str) -> String {
    format!("{DETAIL_SEAT}{part}")
}

/// The address a column heading is painted under.
#[must_use]
pub fn column(key: &str) -> String {
    format!("{COLUMN_SEAT}{key}")
}

/// The address a severity choice is painted under.
#[must_use]
pub fn severity(key: &str) -> String {
    format!("{SEVERITY_SEAT}{key}")
}

/// The address event `n` is painted under.
#[must_use]
pub fn row(n: usize) -> String {
    format!("{ROW_SEAT}{n}")
}

/// The address one CELL of an event row is painted under.
///
/// ★★★★★ The key is a JOIN of two vocabularies and it is composed here rather
/// than at each reader — R2111's measured case, with this section's own twist:
/// the join is an underscore, so a reader applying this tree's dotted convention
/// asks for a mark that is never painted.
#[must_use]
pub fn cell(n: usize, column: &str) -> String {
    format!("{CELL_SEAT}{n}{CELL_JOIN}{column}")
}

/// The address event `n`'s severity dot is painted under.
#[must_use]
pub fn dot(n: usize) -> String {
    format!("{DOT_SEAT}{n}")
}

/// The address a part's own child is painted under.
///
/// # Panics
///
/// If `word` is not one of [`CHILD_WORDS`] — a defect in the caller rather than
/// a state the screen reaches.
#[must_use]
pub fn child(seat: &str, word: &str) -> String {
    assert!(
        CHILD_WORDS.contains(&word),
        "`{word}` is not one of this screen's child words {CHILD_WORDS:?} — a \
         new one has to be declared before it can be addressed, or the part \
         inverses stop being able to tell a part from what is inside it"
    );
    format!("{seat}.{word}")
}

/// Whether `tag` is a mark of this screen at all.
#[must_use]
pub fn ours(tag: &str) -> bool {
    tag.starts_with(NAMESPACE)
}

/// The header part `tag` names, or `None`.
#[must_use]
pub fn header_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(HEADER_SEAT)?)
}

/// The list part `tag` names, or `None`.
///
/// ⚠ Refuses a row, a cell and a dot: those hang one segment deeper, and a
/// reader handed `row` as a part key would look it up in the list's parts and
/// find nothing — quietly.
#[must_use]
pub fn list_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(LIST_SEAT)?)
}

/// The decode-pane part `tag` names, or `None`.
#[must_use]
pub fn detail_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(DETAIL_SEAT)?)
}

/// The column `tag` names, or `None`.
#[must_use]
pub fn column_key(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(COLUMN_SEAT)?)
}

/// The severity choice `tag` names, or `None`.
#[must_use]
pub fn severity_key(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(SEVERITY_SEAT)?)
}

/// The event `tag`'s row is about, or `None`.
#[must_use]
pub fn row_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(ROW_SEAT)?)?.parse().ok()
}

/// The event and column `tag`'s cell is about, or `None`.
///
/// The inverse of [`cell`], and the only place besides it that knows the join is
/// an underscore.
#[must_use]
pub fn cell_of(tag: &str) -> Option<(usize, &str)> {
    let (row, column) = bare(tag.strip_prefix(CELL_SEAT)?)?.split_once(CELL_JOIN)?;
    let row: usize = row.parse().ok()?;
    (!column.is_empty()).then_some((row, column))
}

/// The event `tag`'s severity dot is about, or `None`.
#[must_use]
pub fn dot_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(DOT_SEAT)?)?.parse().ok()
}

/// A key with nothing further hanging off it, or `None`.
///
/// The one rule the inverses share, written once: a remainder that still holds a
/// separator is not a key of that family, and an EMPTY remainder is the family's
/// own stem rather than a member of it. R2112 paid for the second half on the
/// capture viewer's decode tree — `Some("")` there let a gate count a stem as an
/// unnamed member and report the family fully claimed.
///
/// ⚠ A cell's remainder legitimately holds the JOIN, which is not a separator —
/// that is why [`cell_of`] can call this and then split.
fn bare(rest: &str) -> Option<&str> {
    (!rest.is_empty() && !rest.contains('.')).then_some(rest)
}

/// Every part the three surfaces declare, beside the address it is painted at.
///
/// Derived from `spec::HEADER`, `spec::COLUMNS` and `spec::DETAIL` rather than
/// listed again: the specification is what says which parts exist, and a second
/// roster here would be exactly the drift this module removes.
///
/// ⚠ Each row carries the part's own KEY and TITLE rather than the key alone,
/// for the reason R2121 and R2122 both measured: two surfaces of one screen can
/// name a part with the same word, and a caller handed the bare key cannot say
/// which surface it belongs to.
#[must_use]
pub fn parts() -> Vec<(&'static str, &'static str, String)> {
    spec::HEADER
        .iter()
        .map(|p| (p.key, p.title, header(p.key)))
        .chain(spec::DETAIL.iter().map(|p| (p.key, p.title, detail(p.key))))
        .chain(
            spec::COLUMNS
                .iter()
                .map(|c| (c.key, c.title, column(c.key))),
        )
        .collect()
}

/// What this screen puts on the WIRE so a walk does not spell an address.
///
/// ★★★★★ A walk is Python: it cannot name a const, so the only way it stops
/// spelling an address is if the screen HANDS it one. The specification already
/// went over the wire — the walk reads `spec["columns"]` today — and what it
/// never published was the ADDRESS each key names. That is R2116's finding with
/// the reader in another LANGUAGE, which is its sharpest form: the second copy
/// is somewhere no compiler and no Rust gate will ever look.
///
/// ⚠ It publishes the SEATS and the join, not the per-key addresses: a column's
/// own address rides on that column's row of the specification wire
/// (`spec_json`), beside its key and title, so a walk reading a column reads its
/// address in the same breath. Publishing both here and there would be the
/// second copy this module exists to remove.
#[must_use]
pub fn published() -> serde_json::Value {
    serde_json::json!({
        "namespace": NAMESPACE,
        "root": ROOT,
        "tip": TIP,
        "query": QUERY,
        "panes": { "header": HEADER, "list": LIST, "detail": DETAIL },
        "seats": {
            "header": HEADER_SEAT,
            "list": LIST_SEAT,
            "detail": DETAIL_SEAT,
            "column": COLUMN_SEAT,
            "severity": SEVERITY_SEAT,
            "row": ROW_SEAT,
            "cell": CELL_SEAT,
            "dot": DOT_SEAT,
        },
        "cell_join": CELL_JOIN.to_string(),
    })
}
