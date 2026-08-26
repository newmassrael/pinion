//! R1663 — **the reference screen B, written down as a value.**
//!
//! The same discipline `hello-node-lab`'s `spec` module holds for screen A: the
//! reference states a screen, and a round can claim to have reproduced it or it
//! can put the screen in a table a machine reads and let a test compare the
//! painted scene against it in *both* directions — so an element the screen is
//! missing and an element the screen invented are both failures.
//!
//! Screen B is the capture viewer: a filter bar over a session-context strip
//! over a three-pane body (the message list, the layered decode tree, the
//! bytes), with a reassembly strip along the bottom. The whole table is
//! published on the wire as `spec`, so the demo reads it from the running
//! application rather than carrying a second copy.
//!
//! **Vocabulary is neutral by construction.** The reference's protocol words,
//! configuration paths and resource names are replaced with the words the tool
//! class uses generally. The structure and the behaviour are what is being
//! reproduced, and those are what this table holds.

use pinion_core::widgets::chip_group::Choice;
use pinion_node_graph::{SightedTopology, Sighting};

/// One pane of the shell, and the width the reference gives it.
pub struct PaneSpec {
    /// The paint tag.
    pub tag: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// Its width in logical pixels, or 0 for the pane that takes the rest.
    pub width: u32,
    /// The paint tag of its scrolling body, or `None` when the pane has none.
    ///
    /// ★ R1662 — a pane whose content can exceed it must scroll, or the
    /// overflow is painted outside the window where no gesture reaches it.
    /// Stated here, in the specification, because it is a property of the
    /// screen rather than of one painter.
    pub body: Option<&'static str>,
}

/// The three body panes, left to right. The list takes what the other two
/// leave, because it is the pane a reader spends the session in.
pub const PANES: &[PaneSpec] = &[
    PaneSpec {
        tag: "pv.list",
        title: "Messages",
        width: 0,
        body: Some("pv.list.body"),
    },
    PaneSpec {
        tag: "pv.tree",
        title: "Decode",
        width: 348,
        body: Some("pv.tree.body"),
    },
    PaneSpec {
        tag: "pv.bytes",
        title: "Bytes",
        width: 318,
        body: Some("pv.bytes.body"),
    },
];

/// The application bar's height.
pub const APP_BAR_H: u32 = 54;
/// The filter bar's height.
pub const FILTER_H: u32 = 46;
/// The session-context strip's height.
pub const CONTEXT_H: u32 = 38;
/// The reassembly strip's height.
pub const REASSEMBLY_H: u32 = 96;

/// The capture this screen opens on, as the application bar states it.
pub const INTERFACE: &str = "if0 · stream 3";
/// The rate readout beside it.
pub const RATE: &str = "1,284 msg/s";

// ── The filter bar (the reference's search-and-filter requirement) ──────────

/// ★★★ R1707 — **the names a query may address**, which is wider than the
/// columns on purpose.
///
/// [`COLUMNS`] is what the list DRAWS. This is what a reader may ask about, and
/// the two differ by `note` and `fragment` — facts a row carries and shows
/// inside itself rather than in a column of its own. The reference floor cannot
/// express the difference at all: its row-filtering proxy addresses a model
/// COLUMN by ordinal (`filterKeyColumn` is an `int`, measured on 6.11.1 — there
/// is no name-taking peer), so anything not laid out as a column is unfilterable
/// and a saved filter changes meaning the day a column moves.
///
/// The order is the roster's own and nothing derives an index from it.
///
/// ★★★★★ R1827 — `session` is the first name here that no column draws AND no
/// row stores: it is [`RowSpec::session`], the unordered endpoint pair, derived
/// from `hop`. That is what makes *follow one session* a filter rather than a
/// second list — the roster is what a reader may ASK about, and a conversation
/// is a thing a reader asks about even though no cell holds it.
pub const QUERY_COLUMNS: &[&str] = &[
    "time", "hop", "channel", "sn", "type", "name", "len", "note", "fragment", "session",
];

/// ★★★ R1787 — **what an export of this capture covers**, as the closed
/// vocabulary the `export` action's argument domain is drawn from.
///
/// Deliberately **not** `pinion_core::widgets::table_export::Scope`, whose
/// `selection` means a rectangle of cells. This screen has no cell rectangle;
/// what a person here is looking at is the set of rows the filter **kept**, and
/// calling that "selection" would put two different facts under one word — the
/// class of defect this screen's own history is full of. The derivation is
/// shared (`table_export::write` writes both); the vocabulary is not, because
/// the vocabularies genuinely differ.
///
/// The floor, measured by building and running the reference toolkit at 6.11:
/// asked for a rectangle of cells as data, its item-model layer answers two
/// **binary** payloads carrying no text and no header labels, and its tabular
/// widget with every cell selected and a real copy chord delivered leaves the
/// clipboard holding no format at all. There is nothing to be superior to on
/// coverage, so the bar here is set by the capture's own content: measured
/// through the wire, **one** cell of the exported message list holds a comma,
/// so a naive comma-separated export of this very screen splits a column
/// silently. (Seven is what a grep of this file answers, and six of those
/// literals belong to the decode tree rather than to a row — the population is
/// what a row EXPORTS, not what the fixture contains.)
pub const EXPORT_SCOPES: &[&str] = &["shown", "all"];

/// ★★★ R1707 — **what this screen tells a person the mouse does**, which until
/// now it never said at all.
///
/// The sibling screen prints a hint strip and R1703 built a gate over it, after
/// a gesture it advertised for its whole life turned out to be dead. That gate
/// found nothing here because this screen declared no gestures — and an empty
/// population passes every check, so "advertises nothing" and "keeps every
/// promise" were the same reading.
///
/// Taken from the behaviour prototype's capture section rather than invented:
/// it binds a click on a message row, a click on a decode field, and typing in
/// the filter box, and those are the three.
pub const GESTURES: &[(&str, &str)] = &[
    ("click a message", "decode it"),
    ("click a decode field", "light its bytes"),
    ("type in the filter", "narrow the list"),
    ("click a column header", "order the list by it"),
];

/// What the query bar says when no query is running.
///
/// It shows the shape rather than an instruction, because the grammar is small
/// enough to demonstrate and a reader who can see one clause can write another.
pub const QUERY_PLACEHOLDER: &str = "filter — e.g. type in (Data, Query)";

/// The reference's own opening query, kept verbatim in the vocabulary of this
/// screen's roster.
///
/// It is offered as a saved filter rather than applied at boot, and the reason
/// is written in [`ROWS`]: those sixteen are the *requirement set* — a fragment
/// run, a dropped piece, a declaration, an out-of-band payload, an unknown
/// extension — and a screen that opened with thirteen of them hidden could not
/// show what it exists to show. The behaviour prototype opens with its filter
/// empty for the same reason.
pub const EXAMPLE_QUERY: &str =
    "name ~= \"sensors/**\" and type in (Data, Query) and channel != bg/beff";

/// One saved filter: what the chip is called, and the query pressing it runs.
pub struct SavedFilter {
    /// The chip's label.
    pub name: &'static str,
    /// The query it applies, in the grammar of
    /// [`RowQuery`](pinion_core::widgets::row_query::RowQuery).
    pub query: &'static str,
}

/// The saved filters offered beside the query, in the order the bar shows them.
///
/// ★ R1707 — each carries the query it runs. Until this round these were three
/// labels and a boolean each: pressing one said "applied units only" in the
/// status line and the list did not move, which is this screen's own instance
/// of the defect the tool keeps reporting — an affordance that is announced,
/// named, and does nothing.
/// ★★★★★ R1721 — how many saved filters may be on at once, and therefore what
/// the bar **is**.
///
/// Measured 2026-08-19 by driving this screen: pressing a second chip cleared
/// the first and pressing the chosen one cleared it, so the behaviour was
/// already at-most-one — while the accessibility tree announced three
/// independent toggle buttons, the Tab ring cost three stops, and no arrow
/// walked the bar. One word, and the roles, the stop count, the arrows and the
/// `Enter` all follow from it.
///
/// It lives in the specification because it is a statement about what this
/// screen *is*, and because both the census in `tests.rs` and the widget in
/// `main.rs` must read one copy of it.
pub const SAVED_ROW: Choice = Choice::AtMostOne;

pub const SAVED_FILTERS: &[SavedFilter] = &[
    SavedFilter {
        name: "units only",
        query: "name ~= \"sensors/unit-*/**\"",
    },
    SavedFilter {
        name: "out-of-band only",
        query: "note = \"out of band\"",
    },
    SavedFilter {
        name: "reassembly failed",
        query: "fragment = Drop",
    },
];

/// How many messages match, and how many were captured.
pub const MATCHED: u32 = 12_418;
/// The whole capture's message count.
pub const CAPTURED: u32 = 184_392;

// ── The session-context strip ──────────────────────────────────────────────

/// The extensions this session agreed to, by their wire marker.
///
/// ★★★★★ R1845 — the referent the third violation kind had none of. [`CONTEXT`]
/// holds negotiated SETTINGS (an id width, a batch size, a compression flag);
/// nothing said which EXTENSIONS were agreed, so a row carrying one could not
/// be wrong about it. A capture that shows `extension 0x1f` and cannot say
/// whether 0x1f was negotiated is showing a fact with no premise.
///
/// ⚠ Kept beside `CONTEXT` and not inside it because the two answer different
/// questions: a context value is an input the decoder needs to read anything at
/// all, and an extension is a capability the peers agreed to use. Folding them
/// together would make the strip show a roster a reader has no use for on every
/// screen.
pub const NEGOTIATED_EXTENSIONS: &[&str] = &["0x02", "0x07", "0x11"];

/// One negotiated value the decoder was given.
pub struct ContextValue {
    /// The name the strip shows.
    pub key: &'static str,
    /// The value it was negotiated to.
    pub value: &'static str,
    /// The one-word consequence, or `""` when the value speaks for itself.
    pub note: &'static str,
}

/// The six negotiated values, always visible.
///
/// The reference keeps these on screen at all times and says why: they are the
/// decoder's *input*, so nothing below them is interpretable without them. A
/// screen that hid them behind a panel would be able to show a decode whose
/// premises the reader cannot see.
pub const CONTEXT: &[ContextValue] = &[
    ContextValue {
        key: "id width",
        value: "u16",
        note: "",
    },
    ContextValue {
        key: "batch size",
        value: "65,535",
        note: "",
    },
    ContextValue {
        key: "compression",
        value: "on",
        note: "",
    },
    ContextValue {
        key: "low latency",
        value: "off",
        note: "4 layers",
    },
    ContextValue {
        key: "revision",
        value: "1",
        note: "",
    },
    ContextValue {
        key: "delivery",
        value: "on",
        note: "8 channels",
    },
];

/// The session the context was negotiated for.
pub const SESSION: &str = "n1 <-> r1";

/// How the two endpoints of a session are written when neither direction is
/// meant — the reference's own spelling in [`SESSION`].
pub const SESSION_SEP: &str = " <-> ";

// ── R1852: the topology this capture SHOWS, built from its own hops ─────────

/// The topology this capture shows, built from its own hops and nothing else.
///
/// ★★★★★ THE MEASUREMENT THAT MADE THIS WORTH BUILDING, and it is about the
/// strip above rather than about any missing widget.
///
/// [`CONTEXT`] holds the values the decoder was **given**, and the reference
/// keeps them on screen at all times precisely because nothing below them is
/// interpretable without them. [`SESSION`] says which session they were
/// negotiated for. Measured over [`ROWS`] at R1852: this capture's hops mention
/// **eight** endpoints across **nine** directions and **seven** conversations,
/// and `SESSION` names exactly one of those seven. So the strip states a premise
/// and the table below it shows rows the premise does not cover — a fact with no
/// premise, which is the class R1845 closed for the extension marker and this
/// closes for the negotiated context.
///
/// Nothing was missing to notice it: every hop has said who sent and who
/// received since the screen was written, and `hop` was read only to be PAINTED
/// and to pair a reply with its request. The ingredients were there and nothing
/// asked.
///
/// ⚠ Built rather than stored, and rebuilt per call. The alternative is a cached
/// graph beside the rows, which is a second account of the same fact — and this
/// capture is sixteen rows, so the pass is not worth a cache that could drift.
#[must_use]
pub fn sighted_topology() -> SightedTopology {
    SightedTopology::from_sightings(ROWS.iter().map(|row| {
        let (from, to) = row.hop.split_once(HOP_SEP).unwrap_or((row.hop, row.hop));
        Sighting::new(from, to)
    }))
}

/// The two endpoints [`SESSION`] names, in the order it writes them.
///
/// A `Result`-free split with a stated fallback: a session written without the
/// separator is one endpoint talking to itself, which is a shape a capture can
/// have. The gate below holds `SESSION` to naming endpoints this capture
/// actually shows, which is the check that matters.
#[must_use]
pub fn session_endpoints() -> (&'static str, &'static str) {
    SESSION
        .split_once(SESSION_SEP)
        .unwrap_or((SESSION, SESSION))
}

/// How many of [`ROWS`] belong to the session [`CONTEXT`] was negotiated for.
///
/// ★ The premise's REACH. A row belongs to that session when its hop runs
/// between those two endpoints either way — a session is a conversation and not
/// a direction, which is why [`SESSION`] is written with `<->`.
#[must_use]
pub fn rows_in_session() -> usize {
    let (a, b) = session_endpoints();
    ROWS.iter()
        .filter(|row| {
            let (from, to) = row.hop.split_once(HOP_SEP).unwrap_or((row.hop, row.hop));
            (from == a && to == b) || (from == b && to == a)
        })
        .count()
}

/// Whether the row at `n` is one the negotiated context covers.
///
/// The per-row form of [`rows_in_session`], for the strip that says so about the
/// row a reader has selected.
#[must_use]
pub fn row_in_session(n: usize) -> bool {
    let (a, b) = session_endpoints();
    ROWS.get(n).is_some_and(|row| {
        let (from, to) = row.hop.split_once(HOP_SEP).unwrap_or((row.hop, row.hop));
        (from == a && to == b) || (from == b && to == a)
    })
}

// ── The message list ───────────────────────────────────────────────────────

/// One column of the message list, and the width the reference gives it.
pub struct ColumnSpec {
    /// The header text.
    pub title: &'static str,
    /// Its width in logical pixels, or 0 for the column that takes the rest.
    pub width: u32,
}

/// The columns of the message list, left to right.
///
/// ★ R1827 — deliberately not a number in this sentence. It said "the seven
/// columns", which is the shape a hand count in prose always eventually takes.
/// Everything that consumes this table derives its arity from `COLUMNS.len()`.
///
/// ★★★★★ **And an eighth column does not fit here — that is a measurement, not
/// a style preference.** R1827 set out to show request-response correlation as
/// a `linked` column, built it, and re-ran the whole suite against it. **Six**
/// gates refused it at the size this screen opens in, and they refused it for
/// four distinct reasons, which is what makes this a property of the screen
/// rather than one gate's opinion:
///
/// * `r1663_every_painted_mark_is_inside_the_pane_its_address_names` — the byte
///   pane was painted **outside the window** (`pv.bytes.body` at x=1204 w=316,
///   in a 1440-wide window);
/// * `r1672_every_painted_mark_is_inside_the_box_that_owns_it` — the screen
///   overhung its own root to the right;
/// * `r1747_the_capture_viewer_reproduces_its_specification_or_says_why_not` —
///   the surface carried a `part 7` that no specification declares;
/// * `r1663_every_declared_element_of_the_screen_is_painted` and
///   `r1693_the_screen_speaks_and_is_quiet_exactly_where_the_specification_says`
///   — the empty runs, on which see `lib::name_column_paint`;
/// * `r1800_no_run_sits_in_a_box_too_short_for_its_own_face` — the pin, moved by
///   the new runs.
///
/// The cause is in this table's own arithmetic — `LIST_FLOOR` is
/// `fixed_columns() + NAME_FLOOR + PAD * 2`, and the opening size clears that
/// floor by a margin narrower than any usable column.
///
/// ⚠ **The overhang is deliberately not given as a number here.** The first
/// draft of this comment recorded one (`47px`), and re-running the
/// counterfactual measured **81px** — because the overhang is not a fact about
/// the design at all, it is `the column's width minus the margin`, and the
/// width of a column that was never going to ship is a number somebody picked.
/// The gates above are what is reproducible; the pixel count is not.
///
/// So a new *fact* about a message goes where this screen already puts per-row
/// derived facts: an annotation run inside the name column, beside the note and
/// the fragment marker. See `lib::link_text`.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        title: "time",
        width: 96,
    },
    ColumnSpec {
        title: "from -> to",
        width: 96,
    },
    ColumnSpec {
        title: "channel",
        width: 84,
    },
    ColumnSpec {
        title: "sn",
        width: 54,
    },
    ColumnSpec {
        title: "type",
        width: 76,
    },
    ColumnSpec {
        title: "name",
        width: 0,
    },
    ColumnSpec {
        title: "len",
        width: 52,
    },
];

/// The message classes the reference gives their own colour.
///
/// Five and not one, because the colour is the reader's index into a list
/// scrolling past: a screen with one ink makes a reader parse the type column.
pub const KINDS: &[&str] = &["Data", "Query", "Response", "Declare", "Transport"];

/// How a message row relates to a reassembly, when it does.
pub struct FragmentSpec {
    /// `First`, `More` or `Drop` — the marker the row carries.
    pub marker: &'static str,
    /// Which piece of how many.
    pub piece: &'static str,
}

/// One row of the message list.
pub struct RowSpec {
    /// The capture timestamp.
    pub time: &'static str,
    /// Who sent it and who received it.
    pub hop: &'static str,
    /// Which priority-and-delivery channel it travelled on.
    pub channel: &'static str,
    /// Its sequence number in that channel.
    pub sn: u32,
    /// Which of [`KINDS`] it is.
    pub kind: &'static str,
    /// The resource name, the declaration it establishes, or what a
    /// transport-level message is.
    pub name: &'static str,
    /// Its length on the wire.
    pub len: u32,
    /// Its fragment marker, when it is one piece of a larger message.
    pub fragment: Option<FragmentSpec>,
    /// A one-word annotation the reference shows in the row rather than hiding:
    /// an out-of-band payload descriptor, an unknown extension, a reassembly
    /// result. Empty when the row has none.
    pub note: &'static str,
}

impl RowSpec {
    /// ★ R1707 — this row's attributes in [`QUERY_COLUMNS`] order, which is
    /// what a query reads.
    ///
    /// One function rather than a match at each call site, and it is aligned
    /// with the roster by a test rather than by care: a name added to
    /// `QUERY_COLUMNS` with no attribute here would otherwise make every query
    /// mentioning it silently compare against the empty string, which reads as
    /// "nothing matches" and looks exactly like a correct empty result.
    #[must_use]
    pub fn attributes(&self) -> Vec<String> {
        vec![
            self.time.to_owned(),
            self.hop.to_owned(),
            self.channel.to_owned(),
            self.sn.to_string(),
            self.kind.to_owned(),
            self.name.to_owned(),
            self.len.to_string(),
            self.note.to_owned(),
            self.fragment
                .as_ref()
                .map_or_else(String::new, |f| f.marker.to_owned()),
            self.session(),
        ]
    }

    /// ★★★★★ R1827 — **the conversation this message belongs to**: the two
    /// endpoints, unordered.
    ///
    /// `hop` is directed (`n4 -> r1`), and a session is not — a query and the
    /// reply to it are the same conversation travelling opposite ways. Sorting
    /// the two names is what makes both directions produce ONE string, so a
    /// filter written against it keeps both halves of an exchange; taking the
    /// text as written would keep half and look like a working filter.
    ///
    /// Spelled with the same separator [`SESSION`] uses, because a reader who
    /// sees the negotiated session in the context strip and then filters by
    /// session should be typing the thing they just read.
    #[must_use]
    pub fn session(&self) -> String {
        let (from, to) = self.hop.split_once(HOP_SEP).unwrap_or((self.hop, self.hop));
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        format!("{a} <-> {b}")
    }
}

// ── R1827: request-response correlation, derived from the capture ───────────

/// `a == b`, at compile time.
///
/// Written out because `str`'s `PartialEq` is not `const`, and the correlation
/// rule below has to run in a `const` context — see its own header for why that
/// is not a preference.
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `a < b` as text, at compile time.
///
/// ⚠ This is only *chronological* because every timestamp in a capture is the
/// same fixed-width shape, and that is a thing to assert rather than assume:
/// `r1827_a_timestamp_sorts_as_text_because_every_one_is_the_same_shape` is
/// where it is asserted, so a row added in another format fails a test instead
/// of silently pairing the wrong two messages.
const fn str_lt(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    a.len() < b.len()
}

/// The separator a [`RowSpec::hop`] is written with.
pub const HOP_SEP: &str = " -> ";

/// Where the separator sits in a hop, or `usize::MAX` when it has none — which
/// is the shape of "this hop is not a pair of endpoints", stated without an
/// `Option` so the callers below stay `const`-simple.
const fn arrow_at(hop: &str) -> usize {
    let (b, sep) = (hop.as_bytes(), HOP_SEP.as_bytes());
    let mut i = 0;
    while i + sep.len() <= b.len() {
        let mut k = 0;
        while k < sep.len() && b[i + k] == sep[k] {
            k += 1;
        }
        if k == sep.len() {
            return i;
        }
        i += 1;
    }
    usize::MAX
}

/// `a[a0..a1] == b[b0..b1]`, over bytes, at compile time.
const fn range_eq(a: &[u8], a0: usize, a1: usize, b: &[u8], b0: usize, b1: usize) -> bool {
    if a1 - a0 != b1 - b0 {
        return false;
    }
    let mut i = 0;
    while a0 + i < a1 {
        if a[a0 + i] != b[b0 + i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `q` runs between the same two endpoints as `r`, the other way round.
///
/// Compared half against half rather than by building the reversed string,
/// because `format!` does not exist in a `const` context — and because not
/// building it means there is no second spelling of the separator to drift.
const fn is_reverse_hop(q: &str, r: &str) -> bool {
    let (qa, ra) = (arrow_at(q), arrow_at(r));
    if qa == usize::MAX || ra == usize::MAX {
        return false;
    }
    let (qb, rb) = (q.as_bytes(), r.as_bytes());
    let sep = HOP_SEP.len();
    range_eq(qb, 0, qa, rb, ra + sep, rb.len()) && range_eq(qb, qa + sep, qb.len(), rb, 0, ra)
}

/// ★★★★★ R1827 — **the message row `n` is paired with**, or `None`.
///
/// # Why this is derived and not stored
///
/// Because on the wire it is. A reply in this protocol carries no request id;
/// what pairs the two is that they travel the same channel between the same two
/// endpoints in opposite directions, and the reply comes after. An analyser that
/// wanted a stored field would have to invent one the capture does not contain,
/// and would then be showing its own bookkeeping rather than the exchange. The
/// census row this answers (`capture.t1.9`) says the same in its own words — *a
/// derived column over the model* — so storing it would also be this screen
/// quietly disagreeing with the specification it is measured against.
///
/// # The rule, stated once because it is read from both ends
///
/// **A reply answers the most recent request before it on the same channel in
/// the reverse direction.** The request end is not a second rule: it is that
/// one, inverted — a request's counterpart is the **earliest** reply that picks
/// it. Two rules would be two opportunities to disagree.
///
/// ⚠ **It is not a symmetric relation, and the first draft of this paragraph
/// claimed it was.** Inverting a rule gives a relation that is many-to-one, not
/// one-to-one: a query answered by three replies is named by all three and names
/// only the first of them back. That is what the capture analysers this screen
/// is measured against do, and it is the honest shape — the alternative, linking
/// only the first reply so that the pointer always comes back, would leave the
/// second and third replies looking like messages in no exchange at all. What
/// IS guaranteed, and asserted by test:
///
/// * following from the **reply** end always returns — the request a reply
///   answers always names some reply back;
/// * every edge, from either end, joins a `Query` to a `Response` that share a
///   channel, run between the same two endpoints, and are in time order.
///
/// # Against the floor
///
/// Against the reference toolkit at 6.11: an item model addresses a cell by row
/// and column ordinal and has no vocabulary for a relation BETWEEN rows at all —
/// a correlated pair there lives in whatever the caller wrote down beside the
/// model. Here it is published (announced in the row, and whole on the wire), so
/// an agent that never saw the screen can follow the exchange.
///
/// # Why a slice and not the fixture
///
/// So the rule can be exercised on a capture other than this screen's. The
/// opening capture holds exactly ONE exchange, which leaves the interesting
/// states — a query with several replies, a reply whose request is off the front
/// of the capture, two exchanges on one channel — unreachable from it. Taking
/// `rows` as an argument is what lets the tests build those captures instead of
/// asserting that a single pair still pairs.
///
/// # Why `const`
///
/// Because the name column's floor has to include the link annotation, and that
/// floor is a `const` this screen **derives** rather than picks (`lib`'s
/// `NAME_FLOOR` records what it cost to learn that). A floor computed from an
/// upper bound instead — "the widest sequence number, on every row" — is wider
/// on the row that already carries the widest annotations by more headroom than
/// the opening size has. So the exact answer is load-bearing, and the only way
/// to have it exactly, in a `const`, with **one** rule rather than a `const`
/// copy of a runtime rule, is for the rule itself to be `const`.
#[must_use]
pub const fn correlation_in(rows: &[RowSpec], n: usize) -> Option<usize> {
    if n >= rows.len() {
        return None;
    }
    if str_eq(rows[n].kind, "Response") {
        return request_answered_by_in(rows, n);
    }
    if !str_eq(rows[n].kind, "Query") {
        return None;
    }
    // The mirror, and it calls the arm above rather than restating it: the
    // earliest reply whose own answer is this row.
    let mut best: Option<usize> = None;
    let mut reply = 0;
    while reply < rows.len() {
        let picks = match request_answered_by_in(rows, reply) {
            Some(q) => q == n,
            None => false,
        };
        if picks {
            let take = match best {
                Some(b) => str_lt(rows[reply].time, rows[b].time),
                None => true,
            };
            if take {
                best = Some(reply);
            }
        }
        reply += 1;
    }
    best
}

/// [`correlation_in`] over the capture this screen is showing.
#[must_use]
pub const fn correlation(n: usize) -> Option<usize> {
    correlation_in(ROWS, n)
}

/// The request a reply answers: the most recent one before it, same channel,
/// reverse direction. `None` for a row that is not a reply, or for a reply with
/// nothing before it to answer — which is a real state in a capture that began
/// mid-conversation, and is why this is an `Option` rather than a panic.
#[must_use]
pub const fn request_answered_by_in(rows: &[RowSpec], reply: usize) -> Option<usize> {
    if reply >= rows.len() {
        return None;
    }
    let r = &rows[reply];
    if !str_eq(r.kind, "Response") {
        return None;
    }
    let mut best: Option<usize> = None;
    let mut i = 0;
    while i < rows.len() {
        let q = &rows[i];
        if str_eq(q.kind, "Query")
            && str_eq(q.channel, r.channel)
            && is_reverse_hop(q.hop, r.hop)
            && str_lt(q.time, r.time)
        {
            let take = match best {
                Some(b) => str_lt(rows[b].time, q.time),
                None => true,
            };
            if take {
                best = Some(i);
            }
        }
        i += 1;
    }
    best
}

/// What a linked row's annotation opens with, said from that row's own end of
/// the pair.
///
/// Two words and not one heading, which is the freedom an annotation run has and
/// a column does not: a column that meant "answers" on one row and "answered by"
/// on another would be two facts wearing one title, so the `linked` column this
/// round abandoned could only ever have shown a bare number. Here each row says
/// which end it is.
pub const ANSWERS: &str = "answers ";
/// The other end's word. See [`ANSWERS`].
pub const ANSWERED_BY: &str = "answered by ";

/// The word row `n` opens its link annotation with, or `""` when it has none.
#[must_use]
pub const fn link_word(rows: &[RowSpec], n: usize) -> &'static str {
    match correlation_in(rows, n) {
        None => "",
        Some(_) => {
            if str_eq(rows[n].kind, "Response") {
                ANSWERS
            } else {
                ANSWERED_BY
            }
        }
    }
}

/// Row `n`'s link annotation as painted and announced, or `None`.
///
/// The ONLY place the text is spelled. Its width has a second, `const`
/// derivation (`lib`'s `link_width`) that cannot build this string, and
/// `r1827_the_link_annotations_reserved_width_is_the_width_it_paints` is what
/// keeps the two from drifting.
#[must_use]
pub fn link_text(n: usize) -> Option<String> {
    let other = correlation(n)?;
    Some(format!("{}{}", link_word(ROWS, n), ROWS[other].sn))
}

/// The opening capture — the rows the reference screen shows, in the order it
/// shows them (newest first).
///
/// The awkward ones are here on purpose, because they are the requirement: a
/// three-piece fragment run whose last piece carries the reassembled length, a
/// dropped piece on a best-effort channel, a declaration that later rows are
/// read through, an out-of-band payload, and an unknown extension. A capture of
/// only well-formed messages would let a screen pass while being unable to say
/// any of it.
///
/// ★★★★★ R1845 — **the sequence numbers agree with the clock, and the faults
/// they carry are SEPARATED on purpose.** They did not: measured before this
/// round, the table descended in time while its `sn` ASCENDED down it — 10 of
/// the 12 adjacent same-channel pairs — so a detector over them would have
/// reported ten sequence regressions that were facts about the TABLE and not
/// about a protocol. ⚠ Those two tens are different numbers, and this comment
/// said `10 of 10` until the closing audit re-derived both: ten of twelve pairs
/// ran the wrong way, and the watermark read over them answers ten regressions.
///
/// Renumbered, the capture now plants one of each fault the screen can name,
/// **on different channels**:
///
/// | channel | what it carries |
/// |---|---|
/// | `data/rel` | a contiguous sequence with one row that ARRIVED OUT OF ORDER, and nothing missing |
/// | `bg/beff` | numbers that never arrived, and an abandoned reassembly, and nothing out of order |
/// | `ihigh/rel` | neither — an unbroken lane, so the continuous arm is not vacuous |
///
/// ⚠ **The separation is the load-bearing part and it was found by a
/// counterfactual, not by design.** While both faults sat on one channel,
/// [`LaneSpec::continuous`] could drop its out-of-order term and **nothing
/// failed** — every lane with a regression also had a gap, so the second
/// conjunct never changed an answer. A fixture where two faults always travel
/// together cannot tell a reading that checks both from one that checks either.
/// `r1845_a_lane_derives_its_reading_from_the_rows_it_is_about` asserts the
/// separation, so a later edit cannot quietly collapse it again.
pub const ROWS: &[RowSpec] = &[
    RowSpec {
        time: "12:04:38.221",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3418,
        kind: "Data",
        name: "sensors/unit-1/pose",
        len: 48,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:38.198",
        hop: "n4 -> r1",
        channel: "ihigh/rel",
        sn: 1182,
        kind: "Query",
        name: "store/**",
        len: 64,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:38.140",
        hop: "r1 -> n4",
        channel: "ihigh/rel",
        sn: 1181,
        kind: "Response",
        name: "store/config",
        len: 212,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:37.960",
        hop: "n2 -> r1",
        channel: "data/rel",
        sn: 3416,
        kind: "Declare",
        name: "id 4 -> sensors/unit/*/pose",
        len: 32,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:37.902",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3417,
        kind: "Data",
        name: "piece 1 of 3",
        len: 1280,
        fragment: Some(FragmentSpec {
            marker: "First",
            piece: "1/3",
        }),
        note: "",
    },
    RowSpec {
        time: "12:04:37.901",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3415,
        kind: "Data",
        name: "piece 2 of 3",
        len: 1280,
        fragment: Some(FragmentSpec {
            marker: "More",
            piece: "2/3",
        }),
        note: "",
    },
    RowSpec {
        time: "12:04:37.900",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3414,
        kind: "Data",
        name: "sensors/unit-1/depth",
        len: 584,
        fragment: Some(FragmentSpec {
            marker: "Last",
            piece: "3/3",
        }),
        note: "reassembled 3,144 B",
    },
    RowSpec {
        time: "12:04:37.771",
        hop: "n5 -> r1",
        channel: "bg/beff",
        sn: 806,
        kind: "Transport",
        name: "keep-alive",
        len: 16,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:37.660",
        hop: "n3 -> r1",
        channel: "data/rel",
        sn: 3413,
        kind: "Data",
        name: "cameras/0/frame",
        len: 40,
        fragment: None,
        note: "out of band",
    },
    RowSpec {
        time: "12:04:37.540",
        hop: "n6 -> r1",
        channel: "data/rel",
        sn: 3412,
        kind: "Data",
        name: "id 7 -> sensors/unit-2/pose",
        len: 48,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:37.421",
        hop: "n4 -> r1",
        channel: "ihigh/rel",
        sn: 1180,
        kind: "Query",
        name: "sensors/unit/**?since=5s",
        len: 72,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:37.310",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3411,
        kind: "Data",
        name: "sensors/unit-1/pose",
        len: 60,
        fragment: None,
        note: "extension 0x1f",
    },
    RowSpec {
        time: "12:04:37.204",
        hop: "n7 -> r1",
        channel: "bg/beff",
        sn: 802,
        kind: "Data",
        name: "piece 2 of 4",
        len: 1280,
        fragment: Some(FragmentSpec {
            marker: "Drop",
            piece: "2/4",
        }),
        note: "",
    },
    RowSpec {
        time: "12:04:37.101",
        hop: "r1 -> n2",
        channel: "ctrl/rel",
        sn: 214,
        kind: "Transport",
        name: "open-ack",
        len: 20,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:36.998",
        hop: "n5 -> r1",
        channel: "data/rel",
        sn: 3410,
        kind: "Data",
        name: "mesh/telemetry",
        len: 96,
        fragment: None,
        note: "",
    },
    RowSpec {
        time: "12:04:36.904",
        hop: "n2 -> r1",
        channel: "data/rel",
        sn: 3409,
        kind: "Declare",
        name: "id 6 -> store/**",
        len: 28,
        fragment: None,
        note: "",
    },
];

/// The row the screen opens with selected — the reassembled one, because it is
/// the row whose decode exercises every layer and a second byte source.
pub const OPENING_ROW: usize = 6;

// ── The decode tree ────────────────────────────────────────────────────────

/// What a field's bytes **hold**, where this specification can say it.
///
/// ★★★★★ R1814 — the declaration this table was missing, and the reason it was
/// missing is worth stating because it is a class this tree keeps finding.
/// [`FieldSpecRow::value`] answers *what does the reader see*; the byte pane
/// needs *what do the bytes encode*. Those are different questions, and they
/// diverge exactly where it matters: `l3.encoding` shows
/// `application/octet-stream` — twenty-four characters — in a **two**-byte
/// extent, because what is on the wire is a registry id and what is printed is
/// its name. A single `value` field cannot answer both, so before this the
/// bytes under it were a hash of the row index and the hex pane was decorative
/// while every geometric check around it was green.
///
/// # Declaring one is a claim, so it is checked
///
/// A variant other than [`Wire::Undeclared`] means *this extent really encodes
/// that value*, and two tests hold it to that: the bytes are written from here,
/// and the value is then read back out of the painted frame **and matched
/// against the number the reference prints**. The second half is what stops a
/// declaration from being fitted to whatever the bytes already were — the
/// failure the hash version is an example of.
///
/// # Undeclared is not a gap to be filled in later by guessing
///
/// The reasons are of three kinds: an enumerated word whose code the reference
/// never prints (`reliable`, `drop`), a value the reference prints in a form
/// the extent cannot hold (`l0.link`, `l3.encoding`), and a field the reference
/// itself calls undecoded (`l3.extension`). Inventing a wire code for those
/// would put the screen back where it started — showing bytes that look like an
/// encoding and are not — only harder to notice, because the round-trip test
/// would pass.
///
/// **How many of each is not written here.** Run the gate:
///
/// ```text
/// cargo test -p hello-packet-view r1814 -- --nocapture
/// ```
///
/// ★★★★★ R1814's own closing audit is why. This paragraph said *nine of the
/// fifteen framed fields*, and the table answers **twelve of eighteen** — four
/// of those being layer headings, so eight are leaves. Both numbers were
/// written by hand beside the list that answers them, and neither was ever
/// asked of it. That is the same defect R1813 found in a ratchet constant one
/// round earlier, in a different crate, by the same audit question — so the
/// remedy is the one that round settled on: state the command, not the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// A big-endian unsigned integer filling the field's whole extent.
    Be(u64),
    /// Text filling the field's whole extent, one byte per character.
    Ascii(&'static str),
    /// A one-byte flag: `01` when set, `00` when clear.
    ///
    /// Separate from [`Wire::Be`] so the convention is stated by the type once
    /// rather than invented at each site: the reference prints `true`, not `1`,
    /// so a site declaring `Be(1)` would be reading a number the screen never
    /// shows and the check below could not tell it from a guess.
    Flag(bool),
    /// This specification does not state the field's encoding, **and why**.
    ///
    /// The bytes under it stay the deterministic capture filler, which is the
    /// honest answer: they are a frame, and this table cannot say what part of
    /// the value the reader sees is in them.
    Undeclared(&'static str),
}

impl Wire {
    /// The bytes this declaration puts in an extent of `len`, or `None` when it
    /// declares nothing — or cannot fit.
    ///
    /// A value too wide for its extent returns `None` rather than truncating:
    /// a silently narrowed number is the same defect one level down, and the
    /// census below counts what was written, so a drop shows up as a fall.
    #[must_use]
    pub fn encode(self, len: usize) -> Option<Vec<u8>> {
        match self {
            Self::Be(n) => {
                if len == 0 || len > 8 {
                    return None;
                }
                let full = n.to_be_bytes();
                let head = 8 - len;
                // Refuse rather than truncate: the high bytes must be zero.
                if full[..head].iter().any(|b| *b != 0) {
                    return None;
                }
                Some(full[head..].to_vec())
            }
            Self::Ascii(text) => (text.len() == len).then(|| text.as_bytes().to_vec()),
            Self::Flag(set) => (len == 1).then(|| vec![u8::from(set)]),
            Self::Undeclared(_) => None,
        }
    }

    /// Whether `bytes` read back as this declaration — the **third direction**,
    /// bytes to value, which is the one this screen could not answer.
    ///
    /// Deliberately not written as `self.encode(bytes.len()) == Some(bytes)`:
    /// that would ask the writer to check the writer. This decodes.
    #[must_use]
    pub fn reads(self, bytes: &[u8]) -> bool {
        match self {
            Self::Be(n) => {
                bytes.len() <= 8
                    && !bytes.is_empty()
                    && bytes.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)) == n
            }
            Self::Ascii(text) => std::str::from_utf8(bytes) == Ok(text),
            Self::Flag(set) => bytes == [u8::from(set)],
            Self::Undeclared(_) => false,
        }
    }

    /// Whether this declaration is one — `false` for [`Wire::Undeclared`].
    #[must_use]
    pub const fn is_declared(self) -> bool {
        !matches!(self, Self::Undeclared(_))
    }
}

/// One field of the decode tree.
pub struct FieldSpecRow {
    /// Its stable path, which is also the key the byte map joins on.
    pub path: &'static str,
    /// What the row is called.
    pub name: &'static str,
    /// The value the row shows.
    pub value: &'static str,
    /// Which byte source it was read from — `0` the captured frame, `1` the
    /// reassembled payload — or `None` when the decoder derived it.
    pub source: Option<u16>,
    /// Its offset in that source.
    pub at: usize,
    /// Its length in bytes.
    pub len: usize,
    /// What those bytes encode — see [`Wire`].
    ///
    /// ★ Deliberately not `Option<Wire>` with a default: adding this field to
    /// the struct made the compiler demand a decision at all twenty-one rows,
    /// which is how this tree proves an absence rather than assuming one.
    pub wire: Wire,
}

/// The four layers the reference names, outermost first.
///
/// The count is a fact about the *session*, not about the screen: the context
/// strip's `low latency` value decides whether the transport layer is present
/// at all, so a screen that hard-coded four would be wrong for half the
/// captures it is meant to read.
pub const LAYERS: &[(&str, &str)] = &[
    ("l0", "L0 · capture and framing"),
    ("l1", "L1 · transport"),
    ("l2", "L2 · network"),
    ("l3", "L3 · message"),
];

/// The decode of [`OPENING_ROW`], as the reference shows it — every row, and
/// the bytes it came from.
///
/// This table is the whole point of the round: it is one declaration, and both
/// the tree the reader clicks and the highlight the byte pane paints are
/// derived from it.
///
/// ★★★★★ R1814 — and since this round the **bytes** are derived from it too.
/// Every `wire` below is a decision the compiler demanded; see [`Wire`] for why
/// nine of them are [`Wire::Undeclared`] rather than a guess.
pub const FIELDS: &[FieldSpecRow] = &[
    FieldSpecRow {
        path: "l0",
        name: "L0 · capture and framing",
        value: "frame 18,442",
        source: Some(0),
        at: 0x00,
        len: 0x0c,
        // A layer heading summarises the fields inside it. Its extent is
        // theirs, so writing anything here would overwrite them — the
        // containment check below refuses a declaration on a spanning row.
        wire: Wire::Undeclared("a layer heading; its bytes belong to the fields inside it"),
    },
    FieldSpecRow {
        path: "l0.link",
        name: "link",
        value: "00:1b:21:c4 -> 00:1b:21:9a",
        source: Some(0),
        at: 0x00,
        len: 0x06,
        // ★ Found while declaring this table: the reference prints a PAIR of
        // four-byte addresses — eight bytes — against a six-byte extent. One
        // of the two is wrong and this specification cannot say which, so it
        // says that instead of picking.
        wire: Wire::Undeclared("the reference prints eight bytes of address in a six-byte extent"),
    },
    FieldSpecRow {
        path: "l0.stream",
        name: "stream offset",
        value: "41,118 (+1,164)",
        source: Some(0),
        at: 0x06,
        len: 0x04,
        // The offset is on the wire; the parenthesised delta is against the
        // previous message, which is a fact about the LIST and not the frame.
        wire: Wire::Be(41_118),
    },
    FieldSpecRow {
        path: "l0.batch",
        name: "batch",
        value: "1,164 -> 2,880 B",
        source: Some(0),
        at: 0x0a,
        len: 0x02,
        wire: Wire::Undeclared(
            "the reference prints a before and an after; which is on the wire is not stated",
        ),
    },
    FieldSpecRow {
        path: "l1",
        name: "L1 · transport",
        value: "v0x09",
        source: Some(0),
        at: 0x0c,
        len: 0x08,
        wire: Wire::Undeclared("a layer heading; its bytes belong to the fields inside it"),
    },
    FieldSpecRow {
        path: "l1.delivery",
        name: "delivery",
        value: "reliable",
        source: Some(0),
        at: 0x0c,
        len: 0x01,
        wire: Wire::Undeclared("an enumerated word whose code the reference never prints"),
    },
    FieldSpecRow {
        path: "l1.priority",
        name: "priority",
        value: "data (4)",
        source: Some(0),
        at: 0x0d,
        len: 0x01,
        // The reference prints the code beside the name, which is what makes
        // this declarable where `delivery` two rows up is not.
        wire: Wire::Be(4),
    },
    FieldSpecRow {
        path: "l1.sn",
        name: "sn",
        value: "3419",
        source: Some(0),
        at: 0x0e,
        len: 0x02,
        // ★★★★★ The reference's own illustration of what field-to-byte MEANS,
        // and the row this debt was written against: these two bytes are now
        // `0d 5b`, which is 3419, where they used to be `18 1c`, which is
        // 6172 and is nothing.
        wire: Wire::Be(3419),
    },
    FieldSpecRow {
        path: "l1.fragment",
        name: "fragment",
        value: "3/3 · more=0",
        source: Some(0),
        at: 0x10,
        len: 0x02,
        wire: Wire::Undeclared("three facts in two bytes and the reference states no packing"),
    },
    FieldSpecRow {
        path: "l1.assembled",
        name: "reassembled",
        value: "3 pieces · 3,144 B",
        source: None,
        at: 0,
        len: 0,
        wire: Wire::Undeclared("derived by the decoder; it has no bytes of its own"),
    },
    FieldSpecRow {
        path: "l2",
        name: "L2 · network",
        value: "push",
        source: Some(0),
        at: 0x14,
        len: 0x04,
        wire: Wire::Undeclared("a layer heading; its bytes belong to the fields inside it"),
    },
    FieldSpecRow {
        path: "l2.congestion",
        name: "congestion",
        value: "drop",
        source: Some(0),
        at: 0x14,
        len: 0x01,
        wire: Wire::Undeclared("an enumerated word whose code the reference never prints"),
    },
    FieldSpecRow {
        path: "l2.express",
        name: "express",
        value: "true",
        source: Some(0),
        at: 0x15,
        len: 0x01,
        wire: Wire::Flag(true),
    },
    FieldSpecRow {
        path: "l3",
        name: "L3 · message",
        value: "put",
        source: Some(0),
        at: 0x18,
        len: 0x18,
        wire: Wire::Undeclared("a layer heading; its bytes belong to the fields inside it"),
    },
    FieldSpecRow {
        path: "l3.name_id",
        name: "name id",
        value: "4",
        source: Some(0),
        at: 0x18,
        len: 0x02,
        wire: Wire::Be(4),
    },
    FieldSpecRow {
        path: "l3.suffix",
        name: "suffix",
        value: "/1/depth",
        source: Some(0),
        at: 0x1a,
        len: 0x08,
        // Eight characters in an eight-byte extent, so the byte pane's own
        // text column reads it back — the one field on this screen where a
        // person can check the decode without decoding anything.
        wire: Wire::Ascii("/1/depth"),
    },
    FieldSpecRow {
        path: "l3.resolved",
        name: "resolved name",
        value: "sensors/unit-1/depth",
        source: None,
        at: 0,
        len: 0,
        wire: Wire::Undeclared("derived by the decoder; it has no bytes of its own"),
    },
    FieldSpecRow {
        path: "l3.encoding",
        name: "encoding",
        value: "application/octet-stream",
        source: Some(0),
        at: 0x22,
        len: 0x02,
        // The clearest case in the table of `value` and `wire` being different
        // questions: a registry id on the wire, its name on the screen.
        wire: Wire::Undeclared(
            "a registry id the reference prints by name, in a fifth of the room",
        ),
    },
    FieldSpecRow {
        path: "l3.stamp",
        name: "timestamp",
        value: "17:04:37.900 · n1",
        source: Some(0),
        at: 0x24,
        len: 0x08,
        wire: Wire::Undeclared("a wall clock the reference prints without an epoch"),
    },
    FieldSpecRow {
        path: "l3.extension",
        name: "extension 0x1f",
        value: "unknown · 12 B · shown, not decoded",
        source: Some(0),
        at: 0x2c,
        len: 0x04,
        // The reference declares this one undecoded itself, in the value the
        // reader sees. Declaring an encoding here would contradict the screen.
        wire: Wire::Undeclared("the reference itself prints `shown, not decoded`"),
    },
    FieldSpecRow {
        path: "l3.payload",
        name: "payload",
        value: "3,144 B",
        source: Some(1),
        at: 0,
        len: 3144,
        wire: Wire::Undeclared(
            "the row states a length rather than a value, and its source is the reassembly",
        ),
    },
];

/// The field the screen opens with selected. The reference's own screen shows
/// `sn` picked and its two bytes lit, which is the illustration of the whole
/// requirement.
pub const OPENING_FIELD: &str = "l1.sn";

/// The two byte sources the decode addresses, and how long each is.
pub const SOURCES: &[(&str, usize)] = &[("frame", 0x48), ("reassembled payload", 3144)];

/// How many bytes of the frame the byte pane shows at once.
pub const BYTES_PER_ROW: usize = 8;

// ── The reassembly strip ───────────────────────────────────────────────────

/// One channel lane of the reassembly strip.
///
/// ★★★★★ R1845 — **a lane declares WHICH channel it is about and reads
/// everything else off [`ROWS`].**
///
/// It used to carry `sn`, `continuous` and `dropped` as written-down values with
/// no link to the capture at all — not even a channel code, so nothing *could*
/// have compared them. What that permitted was measured before this was written:
/// the strip said `data/rel` was unbroken while the rows on that channel skipped
/// a number and one of them went backwards, and every gate was green. It is the
/// defect R1797 took out of the latency card and R1842 out of the option
/// surface, in its third setting.
///
/// ⚠ What stays declared is what a capture cannot answer: the **roster** — the
/// reference's own strip draws three lanes beside a header that counts four
/// carrying channels, so the two are separate facts — and the reader-facing
/// **name**, which is prose the wire code `data/rel` does not contain.
pub struct LaneSpec {
    /// The channel's name, as a reader sees it.
    pub name: &'static str,
    /// The wire code [`RowSpec::channel`] carries for it — the join between this
    /// lane and the messages it is about, and the field whose absence made every
    /// number above underivable.
    pub channel: &'static str,
}

/// The channels the strip draws a lane for, of the eight the session negotiated.
///
/// ⚠ Not "the channels carrying traffic" — that is [`channels`], which the
/// capture answers and which this roster is deliberately smaller than.
pub const LANES: &[LaneSpec] = &[
    LaneSpec {
        name: "data · reliable",
        channel: "data/rel",
    },
    LaneSpec {
        name: "interactive-high · reliable",
        channel: "ihigh/rel",
    },
    LaneSpec {
        name: "background · best-effort",
        channel: "bg/beff",
    },
];

/// Every channel the capture carries, in the order it first shows one.
///
/// ★ The strip's *carrying* count is this and not [`LANES`]`.len()`. The two are
/// different facts — the roster is what the strip DRAWS, this is what the
/// capture HOLDS — and writing one where the other was meant is how the header
/// came to announce three while the rows carried four.
#[must_use]
pub fn channels() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for row in ROWS {
        if !out.contains(&row.channel) {
            out.push(row.channel);
        }
    }
    out
}

/// One channel's rows and their sequence numbers, **oldest first**.
///
/// ⚠ [`ROWS`] is newest first — R1829 built the ordering feature on that fact
/// and R1845 re-measured it — so this walks the table backwards. A sequence is a
/// claim about time and the table is ordered for a reader, so every reading of
/// one has to turn it round first.
#[must_use]
pub fn series(channel: &str) -> Vec<(usize, u32)> {
    ROWS.iter()
        .enumerate()
        .rev()
        .filter(|(_, row)| row.channel == channel)
        .map(|(n, row)| (n, row.sn))
        .collect()
}

/// The rows on one channel that failed to beat the number the channel had
/// already reached, each with the watermark it failed to beat.
///
/// ★★★★★ **A WATERMARK, NOT AN ADJACENT DIFFERENCE, and R1845 got that wrong
/// first.** Differencing neighbouring pairs reports a break on BOTH SIDES of a
/// row that goes backwards — a series holding one gap and one regression
/// answered "three gaps" — because a backwards row makes each of its neighbours
/// look non-consecutive. The highest number the channel has reached is the only
/// reading that separates the two: a regression is a row that does not beat it,
/// and a gap is a number the channel never showed at all.
#[must_use]
pub fn regressions(channel: &str) -> Vec<(usize, u32)> {
    let mut highest: Option<u32> = None;
    let mut out = Vec::new();
    for (n, sn) in series(channel) {
        if let Some(hi) = highest {
            if sn <= hi {
                out.push((n, hi));
            }
        }
        highest = Some(highest.map_or(sn, |hi| hi.max(sn)));
    }
    out
}

impl LaneSpec {
    /// The sequence number this channel has reached — the highest it showed.
    #[must_use]
    pub fn sn(&self) -> u32 {
        series(self.channel)
            .into_iter()
            .map(|(_, sn)| sn)
            .max()
            .unwrap_or(0)
    }

    /// The numbers between this channel's lowest and its highest that never
    /// arrived.
    #[must_use]
    pub fn skipped(&self) -> Vec<u32> {
        let seen: Vec<u32> = series(self.channel).into_iter().map(|(_, sn)| sn).collect();
        let (Some(lo), Some(hi)) = (seen.iter().min().copied(), seen.iter().max().copied()) else {
            return Vec::new();
        };
        (lo..hi).filter(|n| !seen.contains(n)).collect()
    }

    /// How many of this channel's rows arrived out of order — the same reading
    /// [`violations`] publishes as a sequence regression, asked per lane.
    #[must_use]
    pub fn out_of_order(&self) -> usize {
        regressions(self.channel).len()
    }

    /// How many reassemblies this channel abandoned — its rows marked `Drop`.
    #[must_use]
    pub fn dropped(&self) -> usize {
        ROWS.iter()
            .filter(|row| row.channel == self.channel)
            .filter(|row| {
                row.fragment
                    .as_ref()
                    .is_some_and(|piece| piece.marker == "Drop")
            })
            .count()
    }

    /// Whether this channel's sequence is unbroken: nothing skipped, nothing out
    /// of order.
    #[must_use]
    pub fn continuous(&self) -> bool {
        self.skipped().is_empty() && self.out_of_order() == 0
    }

    /// What is wrong with this lane, in report order. Empty when nothing is.
    ///
    /// ★ Three separate faults rather than one boolean, because the reading this
    /// replaces said `{dropped} abandoned` for **every** kind of break — so a
    /// channel that had skipped a number and abandoned nothing would have been
    /// shown the sentence "0 abandoned" as its explanation.
    #[must_use]
    pub fn faults(&self) -> Vec<String> {
        let mut out = Vec::new();
        let skipped = self.skipped().len();
        if skipped > 0 {
            out.push(format!("{skipped} missing"));
        }
        let out_of_order = self.out_of_order();
        if out_of_order > 0 {
            out.push(format!("{out_of_order} out of order"));
        }
        let dropped = self.dropped();
        if dropped > 0 {
            out.push(format!("{dropped} abandoned"));
        }
        out
    }
}

/// How many channels the session negotiated, of which [`channels`] carry
/// traffic and [`LANES`] draw a lane.
pub const CHANNELS: u32 = 8;
/// Reassemblies completed, in progress and abandoned.
pub const REASSEMBLY: (u32, u32, u32) = (1_204, 2, 1);

// ── What a reader is told this screen has (R1693) ──────────────────────────

/// ★★★★★ R1693 — **what a reader is told this screen has**, which nothing in
/// this table said until now.
///
/// The tables above describe what is painted. None of them says what reaches
/// somebody who never sees the drawing, and the gap that opened under that
/// silence was measured the day this was written: the screen painted **186**
/// addressable regions and announced **three** nodes — a `table` with no row, a
/// `tree` with no item, and a `group`. Sixteen messages of seven columns each,
/// twenty-one decoded fields, seventy-two bytes, the query, the negotiated
/// context and the reassembly lanes were not in the accessibility tree at all,
/// and every check in this example was green.
///
/// [`pinion_core::voice`] classifies every addressable region, but a *total*
/// census is satisfied by declaring everything silent — so this table pins the
/// **split**: these regions owe a voice and these owe a silence.
///
/// # Why a population and not a list of tags
///
/// Because most of this screen is one shape repeated per item, and a list of the
/// expanded tags would be a second copy of the tables above. [`Population`]
/// names where the family's members come from, so a family that grows a member
/// cannot be satisfied by the members that were there when this was written.
///
/// ★★★★ **Screen B is what settled whether that has to be a closed enum.** The
/// question was registered when screen A's eight arms were all "index into a
/// table this specification has", and a family with no backing table would have
/// had to be written out member by member — the shape the structure exists to
/// avoid. This screen has two families of neither kind and needed no new
/// machinery for either: [`Population::Cells`] is a **product** of two tables,
/// and [`Population::Bytes`] is a **range** derived from a scalar. What has to
/// generalise is the *expander*, which was already a function; the arm list is
/// per-screen and closed on purpose, because a screen naming a family it does
/// not have should not compile.
pub struct VoiceSpec {
    /// The tag, verbatim for [`Population::One`] and with `{}` where the family
    /// substitutes its member's name otherwise.
    pub tag: &'static str,
    /// The role a reader is told this region is — the WAI-ARIA word
    /// `scene/access` publishes, so the two surfaces join on it.
    pub role: &'static str,
    /// Which family the members come from.
    pub population: Population,
}

/// Where a [`VoiceSpec`]'s members come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Population {
    /// Exactly one region, at [`VoiceSpec::tag`] verbatim.
    One,
    /// One per [`COLUMNS`] entry, keyed by index.
    Columns,
    /// One per [`ROWS`] message, keyed by index.
    Rows,
    /// ★ One per (message, column) **pair** — the product of [`ROWS`] and
    /// [`COLUMNS`], substituted as `{row}_{column}`. Screen A had no family of
    /// this shape, and it is the first evidence that a population is a rule
    /// rather than an index.
    Cells,
    /// One per [`FIELDS`] row of the decode, keyed by its path.
    Fields,
    /// One per [`LAYERS`] entry, keyed by its identifier.
    Layers,
    /// ★ One per **byte of the captured frame** — a range whose length comes
    /// from [`SOURCES`] rather than from any table of rows. The second shape
    /// screen A's families do not have.
    Bytes,
    /// One per row of the byte grid: [`SOURCES`] divided by [`BYTES_PER_ROW`].
    ByteRows,
    /// One per [`CONTEXT`] value, keyed by its name with spaces underscored —
    /// the same slug the painter builds the tag from.
    Context,
    /// One per [`LANES`] entry, keyed by index.
    Lanes,
    /// One per [`SAVED_FILTERS`] entry, keyed by index.
    SavedFilters,
    /// ★ One per message that **carries an annotation**, keyed by index.
    ///
    /// Four of the families below are *conditional*: only some members of a
    /// table paint the region. Declaring them over the whole table would make
    /// the gate demand thirteen notes this capture does not have, and declaring
    /// them as "whichever happen to be painted" would let the region stop being
    /// painted with nothing failing. A predicate over the same table is exact in
    /// both directions, which is the property the population shape exists for.
    Annotated,
    /// One per message that is one piece of a larger one, keyed by index.
    Fragmented,
    /// ★★★★★ R1827 — one per message that is **half of an exchange**, keyed by
    /// index.
    ///
    /// The first conditional family whose predicate is not a field of the row:
    /// [`correlation`] is a relation between rows, so this population cannot be
    /// answered by looking at one `RowSpec` the way `Annotated` and `Fragmented`
    /// are. It is the reason the correlation rule lives in this file rather than
    /// beside the painter — a population is a rule about the model, and the gate
    /// that expands it has to be able to ask the model.
    Correlated,
    /// One per [`FIELDS`] row the decoder **computed** rather than read from
    /// bytes, keyed by its path.
    Derived,
    /// One per byte of the frame the field the screen opens on was read from.
    LitBytes,
}

impl Population {
    /// The members this family expands to, as the strings a tag substitutes.
    ///
    /// The expander lives beside the arms rather than in the gate, because the
    /// two families this screen added are **computed** — a product and a range —
    /// and a gate holding the rule would be the place a screen's own shape got
    /// re-derived by whoever wrote the gate.
    #[must_use]
    pub fn members(self) -> Vec<String> {
        let indexes = |n: usize| (0..n).map(|i| i.to_string()).collect();
        match self {
            Population::One => vec![String::new()],
            Population::Columns => indexes(COLUMNS.len()),
            Population::Rows => indexes(ROWS.len()),
            Population::Cells => (0..ROWS.len())
                .flat_map(|row| (0..COLUMNS.len()).map(move |col| format!("{row}_{col}")))
                .collect(),
            Population::Fields => FIELDS.iter().map(|f| f.path.to_owned()).collect(),
            Population::Layers => LAYERS.iter().map(|(id, _)| (*id).to_owned()).collect(),
            Population::Bytes => indexes(SOURCES[0].1),
            Population::ByteRows => indexes(SOURCES[0].1.div_ceil(BYTES_PER_ROW)),
            Population::Context => CONTEXT
                .iter()
                .map(|value| value.key.replace(' ', "_"))
                .collect(),
            Population::Lanes => indexes(LANES.len()),
            Population::SavedFilters => indexes(SAVED_FILTERS.len()),
            Population::Annotated => picked(|row| !row.note.is_empty()),
            Population::Fragmented => picked(|row| row.fragment.is_some()),
            // Not `picked`, and that is the point: its predicate takes a row,
            // and whether a message is half of an exchange is not a fact the row
            // holds.
            Population::Correlated => (0..ROWS.len())
                .filter(|&n| correlation(n).is_some())
                .map(|n| n.to_string())
                .collect(),
            Population::Derived => FIELDS
                .iter()
                .filter(|f| f.source.is_none())
                .map(|f| f.path.to_owned())
                .collect(),
            Population::LitBytes => FIELDS
                .iter()
                .find(|f| f.path == OPENING_FIELD)
                .filter(|f| f.source == Some(0))
                .map_or_else(Vec::new, |f| {
                    (f.at..f.at + f.len).map(|b| b.to_string()).collect()
                }),
        }
    }
}

/// The indexes of the messages a predicate picks.
fn picked(keep: impl Fn(&RowSpec) -> bool) -> Vec<String> {
    ROWS.iter()
        .enumerate()
        .filter(|(_, row)| keep(row))
        .map(|(n, _)| n.to_string())
        .collect()
}

/// Every region of the opening screen that owes a reader a voice, and what it
/// announces as.
pub const VOICES: &[VoiceSpec] = &[
    // The application bar.
    VoiceSpec {
        tag: "pv.appbar",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.appbar.interface",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.appbar.rate",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.appbar.said",
        role: "status",
        population: Population::One,
    },
    // The filter bar. The saved filters are TOGGLES, not links: each one is on
    // or off, and a reader who is told "button" and not which way it is set
    // cannot know what the count below is counting.
    VoiceSpec {
        tag: "pv.filter",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.filter.query",
        role: "textbox",
        population: Population::One,
    },
    // ★★★★★ R1721 — the bar and its chips announce what the RULE says they are.
    // These two roles were the word `button`, typed here, over a set that can
    // never have two on; now they are `pinion_a11y::group_role(SAVED_ROW)` and
    // `member_role(SAVED_ROW)`, which is what the tree actually builds. The
    // census and the screen read one declaration, so this table cannot check a
    // role somebody typed against a role somebody else typed.
    VoiceSpec {
        tag: "pv.filter.saved",
        role: pinion_a11y::group_role(SAVED_ROW).aria_name(),
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.filter.saved.{}",
        role: pinion_a11y::member_role(SAVED_ROW).aria_name(),
        population: Population::SavedFilters,
    },
    VoiceSpec {
        tag: "pv.filter.count",
        role: "status",
        population: Population::One,
    },
    // The negotiated context. Always on screen because the decode is not
    // interpretable without it — which makes an inaudible context strip a
    // reader looking at a decode whose premises they cannot reach.
    VoiceSpec {
        tag: "pv.context",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.context.session",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.context.{}",
        role: "status",
        population: Population::Context,
    },
    // ★★ The message list, as a GRID. Not a `table`: rows are selected, the
    // arrow keys move the selection, and what a reader is told they can do has
    // to be what they can do.
    VoiceSpec {
        tag: "pv.list",
        role: "grid",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.list.head.{}",
        role: "columnheader",
        population: Population::Columns,
    },
    VoiceSpec {
        tag: "pv.list.row.{}",
        role: "row",
        population: Population::Rows,
    },
    VoiceSpec {
        tag: "pv.list.cell.{}",
        role: "gridcell",
        population: Population::Cells,
    },
    // The decode tree.
    VoiceSpec {
        tag: "pv.tree",
        role: "tree",
        population: Population::One,
    },
    // ★ Every FIELD, not every visible row: a folded layer's children are not
    // painted, so the gate expands this against what the screen currently
    // shows. The opening screen shows them all.
    VoiceSpec {
        tag: "pv.tree.field.{}",
        role: "treeitem",
        population: Population::Fields,
    },
    // The byte grid.
    VoiceSpec {
        tag: "pv.bytes",
        role: "grid",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.bytes.span",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.bytes.offset.{}",
        role: "rowheader",
        population: Population::ByteRows,
    },
    VoiceSpec {
        tag: "pv.bytes.cell.{}",
        role: "gridcell",
        population: Population::Bytes,
    },
    // The reassembly strip.
    VoiceSpec {
        tag: "pv.reassembly",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.reassembly.counts",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "pv.reassembly.lane.{}",
        role: "status",
        population: Population::Lanes,
    },
];

/// Every region of the opening screen that deliberately has **no** voice, and
/// the class of silence it declares.
///
/// The other half of the split, and the half that makes the first half mean
/// something: a census with nothing in this column would be satisfiable by
/// naming every rectangle, and one with everything in it by naming none.
///
/// The `kind` words are [`pinion_core::voice::SilenceKind`]'s own wire
/// spellings, so what this table says is what `scene/voice` publishes.
pub const SILENCES: &[(&str, Population, &str)] = &[
    // The two roots: an address for the sweep, and the receiver a press falls
    // through to. Neither is a place a reader travels.
    ("packet_view", Population::One, "layout"),
    ("pv.root", Population::One, "layout"),
    // The scrolling bodies. What a reader lands on is what is inside them.
    ("pv.list.body", Population::One, "layout"),
    ("pv.tree.body", Population::One, "layout"),
    ("pv.bytes.body", Population::One, "layout"),
    // ★ R1707 — the query text inside the query box. The framework's text-field
    // painter tags the run separately from the box so a caret can be placed in
    // it; a reader is told the box's value once, by the box, and a second stop
    // reading the same characters back would be the field announced twice.
    ("pv.filter.query-text", Population::One, "part_of"),
    // Titles painted inside the pane they name.
    ("pv.tree.title", Population::One, "name_of"),
    ("pv.bytes.title", Population::One, "name_of"),
    ("pv.reassembly.title", Population::One, "name_of"),
    // Selection bands. The row and the item announce that they are selected;
    // the ink behind them is how a sighted reader is told the same thing.
    ("pv.list.selected", Population::One, "decorative"),
    ("pv.tree.selected", Population::One, "decorative"),
    // Annotations painted inside the name column, announced with that cell —
    // "out of band" as its own stop names nothing.
    ("pv.list.row.{}.note", Population::Annotated, "part_of"),
    ("pv.list.row.{}.fragment", Population::Fragmented, "part_of"),
    // ★★★★★ R1827 — and the exchange the message is half of. `part_of` for the
    // same reason as its two neighbours: "answers 1182" read as its own stop
    // names nothing, and the cell it belongs to announces it.
    ("pv.list.row.{}.linked", Population::Correlated, "part_of"),
    // The fold chevron and the derived badge, announced with the item they
    // belong to: `aria-expanded` is the state the chevron draws, and "derived"
    // is a fact about the field beside it.
    ("pv.tree.layer.{}", Population::Layers, "part_of"),
    ("pv.tree.derived.{}", Population::Derived, "part_of"),
    // The highlight behind a byte the open field was read from.
    ("pv.bytes.lit.{}", Population::LitBytes, "decorative"),
];

// ── The reference screen, as a pin written by another hand (R1747) ──────────

/// ★★★★★ R1747 — what the byte pane's readout says when it is showing none of
/// the open row's bytes.
///
/// A constant rather than a literal in the painter because two things read this
/// line now: the painter writes it, and `crate::judge` takes it as the screen's
/// own statement that this is a state rather than a defect. Two spellings of
/// one sentence would make the conformance verdict silently stop noticing the
/// state it is about.
pub const NO_BYTES: &str = "no bytes here";

/// The pinned specification of the capture viewer's surfaces.
///
/// ★★★★★ Separate from everything above it, and that is the point rather than
/// an accident of file layout. This module is **the screen's own table** — it
/// was written in the same edit as the painter it feeds, so a check against it
/// says this build is self-consistent. `docs/analyzer-packets-spec.json` is the
/// other hand: extracted from the behaviour reference in neutral vocabulary,
/// reviewable on its own, and what makes a verdict a claim about the REFERENCE.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging. The same decision every sibling pin carries.
const PACKETS_SPEC_JSON: &str = include_str!("../../../docs/analyzer-packets-spec.json");

/// The capture viewer's specification, as the framework's own document.
///
/// # Panics
///
/// If the pin is not a specification — unreadable JSON, no surfaces, a
/// duplicate part key, a remainder entry naming no round. All are defects in
/// the pin rather than states the running screen can reach.
#[must_use]
pub fn packets_document() -> pinion_core::conformance::SpecDocument {
    pinion_core::conformance::SpecDocument::pinned(
        PACKETS_SPEC_JSON,
        "docs/analyzer-packets-spec.json",
    )
}

/// ★★★★★ R1772 — the third screen to get one, and the reason is the second
/// screen's bill.
///
/// The two sibling screens carry this table; this one did not, and R1707
/// measured what that cost: the filter bar drew a three-clause query, three
/// saved chips and a `kept / total` readout while `row_count` answered 16
/// whatever was typed, there was no `filter` verb, and there was no text input
/// anywhere on the screen — with every check on this example green. A screen
/// census counts what IS drawn, so an operation the screen cannot perform
/// paints nothing and is invisible by construction. This table is the list that
/// can hold an absence.
///
/// ⚠ Those particular defects are FIXED — re-measured at the head of R1772, the
/// `filter` verb routes and the query is real. The debt's evidence had gone
/// stale while its headline claim (no table here) stayed true, which is why the
/// entry point for this round was a re-measurement and not the file's numbers.
pub use pinion_core::operation::Operation as OperationSpec;

/// The capture viewer's operations, in the behaviour reference's own order.
///
/// # Where the population comes from
///
/// Extracted from the reference's capture section, which is markedly smaller
/// than the node lab's: measured, its whole capture view binds ONE named
/// handler (the filter box) and two per-row click closures (a message row, a
/// decode-tree row), over three scrollable regions. So this table is seven rows
/// and not thirty, and that is the reference's shape rather than a shortfall.
///
/// Every row is MEASURED against this screen as it stands. The `verb` column
/// holds an action the wire actually routes today and `gesture` says whether a
/// pointer path actually reaches it; the gate drives both and fails on an
/// optimistic entry.
///
/// ★★★★★ **What writing it found, and could not have been found another way.**
/// `witness` is mandatory — a row cannot be written for an operation whose
/// effect nothing publishes — and the three scroll rows had no slot to name.
/// This screen held all three pane offsets, hit-tested with them, and published
/// none of them, so a client could not ask whether a scroll had happened.
/// `scroll` is published now, which is what lets those rows be in the table at
/// all.
///
/// ⚠ And what `verb: None` claims, exactly. It says **this screen declares no
/// action for it** — not that the pane cannot be scrolled by a client. The
/// framework's own `scene/scroll` reaches all three, measured in this round's
/// demo, and stating the narrower claim is the difference between a column a
/// reader can act on and one they would have to distrust.
pub const OPERATIONS: &[OperationSpec] = &[
    // ── the filter bar ───────────────────────────────────────────
    // ★★ The witness is `kept_rows` and NOT `row_count`, and finding that out
    // is the column earning its place. `row_count` answers how many messages
    // the capture holds — a constant — so a filter row witnessed on it would
    // have been an entry that could never fail. R1707 measured a build where
    // that number stood still whatever was typed; the number is right and the
    // filter is real, and it is `kept_rows` that moves.
    OperationSpec {
        name: "filter the capture",
        verb: Some(("filter", "type=data")),
        gesture: true,
        witness: "kept_rows",
        needs: None,
    },
    // ★ Emptying the box is its own operation and not the absence of the one
    // above: the reference's box can be cleared back to the whole capture, and
    // a table that recorded only the narrowing would describe a tool you cannot
    // get out of.
    OperationSpec {
        name: "clear the filter",
        verb: Some(("filter", "")),
        gesture: true,
        witness: "kept_rows",
        needs: Some("filter the capture"),
    },
    // ★★★★★ R1829 — ordering, which is half of `follow one session, in time
    // order`: a filter alone leaves the reply above the request it answers,
    // because this capture is written newest-first.
    //
    // ⚠ The witness is `sort` and not `kept_rows`, and the first draft of this
    // comment gave the wrong reason for that — it said a sort would leave
    // `kept_rows` standing still, which is FALSE: the witness is the whole
    // published array and ordering permutes it, so it moves. The real reason is
    // that it moves only INCIDENTALLY. A filter that kept one row, or none,
    // would order to the same array and the entry would pass while asserting
    // nothing. `sort` names the fact the operation is about, so it moves
    // whenever the operation happens and only then.
    //
    // ★ The verb is `order` and the witness is `sort` — the act and the fact,
    // which is the same split this screen already runs on one axis over
    // (`filter` is what you do, `query` is what it left). The two carry ONE
    // value vocabulary, so an order read off `sort` can be handed straight back
    // to `order`.
    OperationSpec {
        name: "order the list",
        verb: Some(("order", "0:ascending")),
        gesture: true,
        witness: "sort",
        needs: None,
    },
    // Returning to the capture's own order is its own operation, for the reason
    // clearing the filter is: an analyser you cannot get back out of is a
    // different tool from the one the reference describes.
    OperationSpec {
        name: "return to capture order",
        verb: Some(("order", "none")),
        gesture: true,
        witness: "sort",
        needs: Some("order the list"),
    },
    // ── the message list ─────────────────────────────────────────
    OperationSpec {
        name: "select a message",
        verb: Some(("select_message", "3")),
        gesture: true,
        witness: "selected_row",
        needs: None,
    },
    // ★★★★★ The first of the three the reference offers and an agent cannot
    // cause. A person scrolls the list with the pointer; no action on this
    // screen moves it, and until this round nothing published that it had
    // moved at all. The row is here rather than omitted precisely so the
    // absence is counted.
    OperationSpec {
        name: "scroll the message list",
        verb: None,
        gesture: true,
        witness: "scroll",
        needs: None,
    },
    // ── the decode tree ──────────────────────────────────────────
    OperationSpec {
        name: "select a decoded field",
        verb: Some(("select_field", "l0.link")),
        gesture: true,
        witness: "selected_field",
        needs: Some("select a message"),
    },
    OperationSpec {
        name: "scroll the decode tree",
        verb: None,
        gesture: true,
        witness: "scroll",
        needs: None,
    },
    // ── the byte pane ────────────────────────────────────────────
    OperationSpec {
        name: "scroll the byte pane",
        verb: None,
        gesture: true,
        witness: "scroll",
        needs: None,
    },
];

// ── R1845 — protocol violation rows, DERIVED ────────────────────────────────

/// What a violation row says, and which capture row it is about.
///
/// ★★★★★ R1845 — the census's `capture.t2.18`, and the point is that every
/// ingredient was already here. `RowSpec` carries a per-channel `sn`, a `name`
/// that doubles as the declaration a row establishes, and a `note` that carries
/// an extension marker. Nothing asked. This asks.
///
/// ⚠ **A derived row kind, not a stored one.** The capture is the source and
/// this is a reading of it, so a violation cannot drift from the rows the way a
/// second table would — the defect R1797 removed from the latency card and
/// R1842 from the option surface, in its third setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Which of [`VIOLATION_KINDS`] this is.
    pub kind: &'static str,
    /// The index into [`ROWS`] the reader should look at.
    pub row: usize,
    /// The sentence a reader is shown.
    pub why: String,
}

/// The three kinds the capability names, in report order.
pub const VIOLATION_KINDS: &[&str] = &[
    "sequence regression",
    "undeclared reference",
    "unnegotiated extension",
];

/// The id a row's name establishes or uses, when it names one.
///
/// A declaration and a reference are spelled the same way — `id N -> resource`
/// — and which one it is comes from the row's KIND, not from the text. That is
/// why this answers only the number and leaves the judgement to the caller.
fn named_id(name: &str) -> Option<u32> {
    name.strip_prefix("id ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|digits| digits.parse().ok())
}

/// The extension marker a row's note carries, when it carries one.
fn noted_extension(note: &str) -> Option<&str> {
    note.strip_prefix("extension ").map(str::trim)
}

/// Every violation the capture contains, in row order.
///
/// ★★★★★ THE TWO SEQUENCE KINDS ARE DERIVED AGAINST A WATERMARK, NOT AGAINST
/// THE PREVIOUS ROW, and R1845 got that wrong first. Differencing adjacent
/// pairs reports a gap on BOTH SIDES of a regression — a series with one gap
/// and one regression answered "three gaps" — because a row that goes backwards
/// makes its neighbours look non-consecutive. The watermark is the highest `sn`
/// the channel has reached, so a regression is a row that does not beat it, and
/// a gap is a number the channel never showed at all.
///
/// ⚠ And the capture is NEWEST FIRST (R1829 established it and this round
/// re-measured it: `time` descends down [`ROWS`]). So the series is read in
/// reverse, because a sequence is a fact about time and the table is ordered
/// for a reader.
#[must_use]
pub fn violations() -> Vec<Violation> {
    let mut out: Vec<Violation> = Vec::new();

    // (1) sequence regression — per channel, read against the watermark.
    //
    // ★ The same [`regressions`] the reassembly strip's lanes are derived from,
    // asked once per channel here and once per lane there. One reading of the
    // rows with two readers, which is the whole point of the round: a violation
    // row and a lane cannot disagree about whether a channel went backwards.
    for channel in channels() {
        for (n, highest) in regressions(channel) {
            out.push(Violation {
                kind: VIOLATION_KINDS[0],
                row: n,
                why: format!(
                    "{channel} reached {highest} on an earlier message and this row is {}",
                    ROWS[n].sn
                ),
            });
        }
    }

    // (2) undeclared reference — a row that USES an id no row DECLARES.
    //
    // ★ Over the whole capture rather than "declared before it was used": a
    // declaration this capture never contains is a different fact from one that
    // arrives late, and the second needs an ordering claim the reference does
    // not make. Reporting the stronger one only is the honest reading.
    let declared: Vec<u32> = ROWS
        .iter()
        .filter(|row| row.kind == "Declare")
        .filter_map(|row| named_id(row.name))
        .collect();
    for (n, row) in ROWS.iter().enumerate() {
        if row.kind == "Declare" {
            continue;
        }
        if let Some(id) = named_id(row.name) {
            if !declared.contains(&id) {
                out.push(Violation {
                    kind: VIOLATION_KINDS[1],
                    row: n,
                    why: format!("id {id} is used here and no row in this capture declares it"),
                });
            }
        }
    }

    // (3) unnegotiated extension — a marker outside what the session agreed.
    for (n, row) in ROWS.iter().enumerate() {
        if let Some(marker) = noted_extension(row.note) {
            if !NEGOTIATED_EXTENSIONS.contains(&marker) {
                out.push(Violation {
                    kind: VIOLATION_KINDS[2],
                    row: n,
                    why: format!(
                        "extension {marker} is carried here and the session negotiated {}",
                        NEGOTIATED_EXTENSIONS.join(", ")
                    ),
                });
            }
        }
    }

    out.sort_by_key(|v| v.row);
    out
}
