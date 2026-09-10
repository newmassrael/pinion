//! ★★★★★ R2123 §5.2 §5.11 — **where a painted mark's address comes from, for
//! the key-patterns section.**
//!
//! Public because the ASSEMBLED shell needs it, and because THE WALK needs it —
//! which is what makes this section's shape different from the two before it.
//!
//! ⚠ The header lives HERE rather than on the `pub mod` line: a module carrying
//! both an outer doc at its declaration and an inner `//!` header has the two
//! MERGED and resolved in the DECLARING file's scope, so every link to this
//! module's own items stops resolving. Measured at R2121's push gate.
//!
//! # ★★★★★ THE SHAPE QUESTION, ASKED BEFORE ANY GATE WAS WRITTEN
//!
//! `sv.*` (R2121) and `tv.*` (R2122) each went whole in one round because their
//! populations were ONE: the crate spelled every literal, the shell spelled
//! none, the walks spelled none. So each could assert **zero literals outside
//! the declaration** instead of ratcheting toward it.
//!
//! **This section is not that shape, and it was counted rather than assumed.**
//! Measured at entry: the crate spells **44**, the shell spells **0**, and the
//! walks spell **6**. Two populations, not one.
//!
//! Of those six walk sites, **four are charged** by the paint-address census
//! and two are not: `tag.startswith("kp.")` is a FAMILY-WIDE QUESTION, which
//! R2115's host rule deliberately allows and R2122 re-derived in another crate —
//! the census needle wants three segments, or two with the separator, so a bare
//! family prefix is not an address. Charging them would bill a walk for a
//! question it is entitled to ask.
//!
//! ⇒ so the gate is NOT copied from the two rounds before it. The crate half
//! asserts zero; the walk half is repaid the way this debt's own entry
//! instruction says a walk must be repaid — **a walk cannot name a Rust const,
//! so it reads the address off the WIRE**. [`published`] is what the screen puts
//! on the wire for it.
//!
//! # ⚠ One stem, FOUR vocabularies — and R2118's classifier is not the answer
//!
//! `kp.list.` names a PART (`header`, `body`, `open`) and three INDEXED
//! families (`row.<n>`, `cell.<n>_<col>`, `dot.<n>`). That is more heads than
//! the node lab's wire family had, and yet it needs less machinery, which is
//! worth stating rather than leaving for the next reader to re-derive:
//!
//! R2118 needed a classifier because `lab.link.7` and `lab.link.act` BOTH
//! survive `strip_prefix` — one head is numeric and open, the other a fixed
//! word, and nothing but a classifier can say which. Here every head is named
//! by a WORD in the segment after the stem, so the heads are told apart by
//! taking the seat one segment DEEPER: `kp.list.row.` is its own seat, and
//! [`list_part`] — which refuses anything with a further separator — cannot
//! collide with it. **A deeper seat, not a classifier.**
//!
//! # 🟥 The join is an UNDERSCORE, and that is this section's own shape
//!
//! A cell is `kp.list.cell.<row>_<column>`. Every other joined key in this tree
//! joins on the separator (`sv.row.<session>.<column>`, R2121), so a reader that
//! knows this tree's convention parses this one wrong — and a dotted parse
//! cannot recover it at all. Three sites did the join by hand: the painter, the
//! accessibility roster, and the hit test's inverse, which called
//! `split_once('_')` itself. [`cell`] and [`cell_of`] are now the only places
//! that know, and [`CELL_JOIN`] is published so the walk does not spell it
//! either.
//!
//! # ⚠⚠ An entry is NOT under the part it belongs to, and that is the paint
//!
//! `spec::DETAIL` names a part `endpoints`; the marks under it are painted at
//! `kp.detail.endpoint.<n>` — SINGULAR, and therefore not beneath
//! `kp.detail.endpoints`. R2121's sessions section put its entries under their
//! part (`sv.detail.timeline.3`); this one does not. Declaring the divergence is
//! what stops the next reader composing the address they expected and finding
//! nothing, which is exactly what this campaign exists to prevent.

use crate::spec;

/// The stem this screen's marks hang under, without a separator.
///
/// ★ Wanted bare by `WidgetPaint::paint_stems`. The namespace gate's needle is
/// this rather than [`NAMESPACE`] precisely so that spelling is in the
/// population — R2122 measured that a namespace needle misses it.
pub const STEM: &str = "kp";

/// The namespace every mark this screen paints begins with.
///
/// Carried WITH its separator, which is what a membership test wants.
pub const NAMESPACE: &str = "kp.";

/// The root group every mark of this screen hangs under.
pub const ROOT: &str = "kp.root";

/// Where a resting description is painted and announced.
pub const TIP: &str = "kp.tip";

/// The header band's own group.
pub const HEADER: &str = "kp.header";

/// The list pane's own group.
pub const LIST: &str = "kp.list";

/// The detail pane's own group.
pub const DETAIL: &str = "kp.detail";

/// The filter field a reader types a query into.
///
/// ⚠ The one member its family has. Declared as a whole address rather than
/// composed, because `kp.filter.` with one member is a family in name only and
/// a composer for it would invite a second.
pub const QUERY: &str = "kp.filter.query";

/// [`HEADER`] with the separator its parts hang off.
pub const HEADER_SEAT: &str = "kp.header.";

/// [`LIST`] with the separator its parts hang off.
pub const LIST_SEAT: &str = "kp.list.";

/// [`DETAIL`] with the separator its parts hang off.
pub const DETAIL_SEAT: &str = "kp.detail.";

/// Where a column of the list is addressed.
pub const COLUMN_SEAT: &str = "kp.column.";

/// Where a row of the list is addressed — one segment deeper than
/// [`LIST_SEAT`], which is what tells a row from a part of the list.
pub const ROW_SEAT: &str = "kp.list.row.";

/// Where one cell of a row is addressed.
pub const CELL_SEAT: &str = "kp.list.cell.";

/// Where a row's standing dot is addressed.
pub const DOT_SEAT: &str = "kp.list.dot.";

/// Where one observed endpoint is addressed.
///
/// ⚠ SINGULAR, and not beneath the `endpoints` part it belongs to. See this
/// module's header.
pub const ENDPOINT_SEAT: &str = "kp.detail.endpoint.";

/// What joins a cell's two keys.
///
/// 🟥 An UNDERSCORE, where every other joined key in this tree joins on the
/// separator. Published so the walk does not spell it, and so a reader who
/// knows this tree's convention cannot apply it here by reflex.
pub const CELL_JOIN: char = '_';

/// The list's own header strip.
pub const LIST_HEADER: &str = "kp.list.header";

/// The list's scrollable body — an accessibility focus token compared by
/// identity, and the key a scroll state is resolved under, so it is a
/// `&'static str` held against [`list`] by this crate's gate.
pub const LIST_BODY: &str = "kp.list.body";

/// The mark drawn behind the row the list has open.
pub const LIST_OPEN: &str = "kp.list.open";

/// The header band's live count.
pub const HEADER_SUMMARY: &str = "kp.header.summary";

/// The detail pane's declaring-peer row, which a press resolves.
pub const DECLARER: &str = "kp.detail.declarer";

/// The words the `standing` part puts inside itself.
///
/// ★ A CLOSED set of two. [`child`] refuses a third, which is what keeps
/// [`detail_part`] able to say where a part's address ends.
pub const CHILD_WORDS: &[&str] = &["kind", "health"];

/// The address the header band paints `part` under.
#[must_use]
pub fn header(part: &str) -> String {
    format!("{HEADER_SEAT}{part}")
}

/// The address the list pane paints `part` under.
#[must_use]
pub fn list(part: &str) -> String {
    format!("{LIST_SEAT}{part}")
}

/// The address the detail pane paints `part` under.
#[must_use]
pub fn detail(part: &str) -> String {
    format!("{DETAIL_SEAT}{part}")
}

/// The address a column of the list is painted under.
#[must_use]
pub fn column(key: &str) -> String {
    format!("{COLUMN_SEAT}{key}")
}

/// The address the `n`th row is painted under.
#[must_use]
pub fn row(n: usize) -> String {
    format!("{ROW_SEAT}{n}")
}

/// The address one CELL of a row is painted under.
///
/// ★★★★★ The key is a JOIN of two vocabularies and it is composed here rather
/// than at each reader — R2111's measured case, with this section's own twist:
/// the join is an underscore, so a reader applying this tree's dotted
/// convention gets a mark that is never painted.
#[must_use]
pub fn cell(n: usize, column: &str) -> String {
    format!("{CELL_SEAT}{n}{CELL_JOIN}{column}")
}

/// The address the `n`th row's standing dot is painted under.
#[must_use]
pub fn dot(n: usize) -> String {
    format!("{DOT_SEAT}{n}")
}

/// The address the `n`th observed endpoint is painted under.
#[must_use]
pub fn endpoint(n: usize) -> String {
    format!("{ENDPOINT_SEAT}{n}")
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

/// The detail part `tag` names, or `None`.
#[must_use]
pub fn detail_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(DETAIL_SEAT)?)
}

/// The column `tag` names, or `None`.
#[must_use]
pub fn column_key(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(COLUMN_SEAT)?)
}

/// The row `tag` names, or `None`.
#[must_use]
pub fn row_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(ROW_SEAT)?)?.parse().ok()
}

/// The row and column `tag`'s cell is about, or `None`.
///
/// The inverse of [`cell`], and the only place besides it that knows the join
/// is an underscore.
#[must_use]
pub fn cell_of(tag: &str) -> Option<(usize, &str)> {
    let (row, column) = bare(tag.strip_prefix(CELL_SEAT)?)?.split_once(CELL_JOIN)?;
    let row: usize = row.parse().ok()?;
    (!column.is_empty()).then_some((row, column))
}

/// The row `tag`'s standing dot is about, or `None`.
#[must_use]
pub fn dot_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(DOT_SEAT)?)?.parse().ok()
}

/// The endpoint `tag` names, or `None`.
#[must_use]
pub fn endpoint_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(ENDPOINT_SEAT)?)?.parse().ok()
}

/// A key with nothing further hanging off it, or `None`.
///
/// The one rule the inverses share, written once: a remainder that still holds
/// a separator is not a key of that family, and an EMPTY remainder is the
/// family's own stem rather than a member of it. R2112 paid for the second half
/// on the capture viewer's decode tree.
///
/// ⚠ A cell's remainder legitimately holds the JOIN, which is not a separator —
/// that is why [`cell_of`] can call this and then split.
fn bare(rest: &str) -> Option<&str> {
    (!rest.is_empty() && !rest.contains('.')).then_some(rest)
}

/// Every part the three surfaces declare, beside the address it is painted at.
///
/// Derived from `spec::HEADER`, `spec::COLUMNS` and `spec::DETAIL` rather than
/// listed again.
///
/// ⚠ Each row carries the part's own KEY and TITLE rather than the key alone,
/// for the reason R2121 and R2122 both measured: two surfaces of one screen can
/// name a part the same word, and a caller handed the bare key cannot say which
/// surface it belongs to.
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
/// ★★★★★ THIS IS THE HALF THAT MAKES THIS SECTION DIFFERENT FROM THE TWO
/// BEFORE IT. `sv.*` and `tv.*` had no walk spelling anything, so a declaring
/// module was enough. Here four walk sites compose an address by hand, and a
/// walk is Python: it cannot name a const, so the only way it stops spelling is
/// if the screen HANDS it the address.
///
/// The specification already went over the wire — the walk reads
/// `spec["columns"]["canon"]` today — and what it never published was the
/// ADDRESS each key names. That is R2116's finding, which R2121 and R2122 each
/// met inside a crate; here the reader is in another LANGUAGE, which is the
/// sharpest form of it: *a table that publishes a key and not the address that
/// key names makes every reader assemble it*.
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
        "declarer": DECLARER,
        "panes": { "header": HEADER, "list": LIST, "detail": DETAIL },
        "seats": {
            "header": HEADER_SEAT,
            "list": LIST_SEAT,
            "detail": DETAIL_SEAT,
            "column": COLUMN_SEAT,
            "row": ROW_SEAT,
            "cell": CELL_SEAT,
            "dot": DOT_SEAT,
            "endpoint": ENDPOINT_SEAT,
        },
        "cell_join": CELL_JOIN.to_string(),
    })
}
