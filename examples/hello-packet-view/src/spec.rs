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

/// The query the bar opens with, as its three clauses.
///
/// Held as clauses rather than as one string because that is what the screen
/// paints — each clause is a separate run — and because a test that only knew
/// the joined string could not tell a wrapped clause from a missing one.
pub const QUERY_CLAUSES: &[&str] = &[
    "name ~= \"sensors/unit/**\"",
    "and type in (Data, Declare)",
    "and node != n3",
];

/// The saved filters offered beside the query, in the order the bar shows them.
pub const SAVED_FILTERS: &[&str] = &["units only", "out-of-band only", "reassembly failed"];

/// How many messages match, and how many were captured.
pub const MATCHED: u32 = 12_418;
/// The whole capture's message count.
pub const CAPTURED: u32 = 184_392;

// ── The session-context strip ──────────────────────────────────────────────

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

// ── The message list ───────────────────────────────────────────────────────

/// One column of the message list, and the width the reference gives it.
pub struct ColumnSpec {
    /// The header text.
    pub title: &'static str,
    /// Its width in logical pixels, or 0 for the column that takes the rest.
    pub width: u32,
}

/// The seven columns, left to right.
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

/// The opening capture — the rows the reference screen shows, in the order it
/// shows them (newest first).
///
/// The awkward ones are here on purpose, because they are the requirement: a
/// three-piece fragment run whose last piece carries the reassembled length, a
/// dropped piece on a best-effort channel, a declaration that later rows are
/// read through, an out-of-band payload, and an unknown extension. A capture of
/// only well-formed messages would let a screen pass while being unable to say
/// any of it.
pub const ROWS: &[RowSpec] = &[
    RowSpec {
        time: "12:04:38.221",
        hop: "n1 -> r1",
        channel: "data/rel",
        sn: 3414,
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
        sn: 1180,
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
        sn: 3418,
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
        sn: 3419,
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
        sn: 802,
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
        sn: 3420,
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
        sn: 3421,
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
        sn: 1182,
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
        sn: 3422,
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
        sn: 3410,
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
        sn: 3409,
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
        sn: 3408,
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
pub const FIELDS: &[FieldSpecRow] = &[
    FieldSpecRow {
        path: "l0",
        name: "L0 · capture and framing",
        value: "frame 18,442",
        source: Some(0),
        at: 0x00,
        len: 0x0c,
    },
    FieldSpecRow {
        path: "l0.link",
        name: "link",
        value: "00:1b:21:c4 -> 00:1b:21:9a",
        source: Some(0),
        at: 0x00,
        len: 0x06,
    },
    FieldSpecRow {
        path: "l0.stream",
        name: "stream offset",
        value: "41,118 (+1,164)",
        source: Some(0),
        at: 0x06,
        len: 0x04,
    },
    FieldSpecRow {
        path: "l0.batch",
        name: "batch",
        value: "1,164 -> 2,880 B",
        source: Some(0),
        at: 0x0a,
        len: 0x02,
    },
    FieldSpecRow {
        path: "l1",
        name: "L1 · transport",
        value: "v0x09",
        source: Some(0),
        at: 0x0c,
        len: 0x08,
    },
    FieldSpecRow {
        path: "l1.delivery",
        name: "delivery",
        value: "reliable",
        source: Some(0),
        at: 0x0c,
        len: 0x01,
    },
    FieldSpecRow {
        path: "l1.priority",
        name: "priority",
        value: "data (4)",
        source: Some(0),
        at: 0x0d,
        len: 0x01,
    },
    FieldSpecRow {
        path: "l1.sn",
        name: "sn",
        value: "3419",
        source: Some(0),
        at: 0x0e,
        len: 0x02,
    },
    FieldSpecRow {
        path: "l1.fragment",
        name: "fragment",
        value: "3/3 · more=0",
        source: Some(0),
        at: 0x10,
        len: 0x02,
    },
    FieldSpecRow {
        path: "l1.assembled",
        name: "reassembled",
        value: "3 pieces · 3,144 B",
        source: None,
        at: 0,
        len: 0,
    },
    FieldSpecRow {
        path: "l2",
        name: "L2 · network",
        value: "push",
        source: Some(0),
        at: 0x14,
        len: 0x04,
    },
    FieldSpecRow {
        path: "l2.congestion",
        name: "congestion",
        value: "drop",
        source: Some(0),
        at: 0x14,
        len: 0x01,
    },
    FieldSpecRow {
        path: "l2.express",
        name: "express",
        value: "true",
        source: Some(0),
        at: 0x15,
        len: 0x01,
    },
    FieldSpecRow {
        path: "l3",
        name: "L3 · message",
        value: "put",
        source: Some(0),
        at: 0x18,
        len: 0x18,
    },
    FieldSpecRow {
        path: "l3.name_id",
        name: "name id",
        value: "4",
        source: Some(0),
        at: 0x18,
        len: 0x02,
    },
    FieldSpecRow {
        path: "l3.suffix",
        name: "suffix",
        value: "/1/depth",
        source: Some(0),
        at: 0x1a,
        len: 0x08,
    },
    FieldSpecRow {
        path: "l3.resolved",
        name: "resolved name",
        value: "sensors/unit-1/depth",
        source: None,
        at: 0,
        len: 0,
    },
    FieldSpecRow {
        path: "l3.encoding",
        name: "encoding",
        value: "application/octet-stream",
        source: Some(0),
        at: 0x22,
        len: 0x02,
    },
    FieldSpecRow {
        path: "l3.stamp",
        name: "timestamp",
        value: "17:04:37.900 · n1",
        source: Some(0),
        at: 0x24,
        len: 0x08,
    },
    FieldSpecRow {
        path: "l3.extension",
        name: "extension 0x1f",
        value: "unknown · 12 B · shown, not decoded",
        source: Some(0),
        at: 0x2c,
        len: 0x04,
    },
    FieldSpecRow {
        path: "l3.payload",
        name: "payload",
        value: "3,144 B",
        source: Some(1),
        at: 0,
        len: 3144,
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
pub struct LaneSpec {
    /// The channel's name.
    pub name: &'static str,
    /// The sequence number it has reached.
    pub sn: u32,
    /// Whether its sequence is unbroken.
    pub continuous: bool,
    /// How many pieces it has abandoned.
    pub dropped: u32,
}

/// The channels carrying traffic, of the eight the session negotiated.
pub const LANES: &[LaneSpec] = &[
    LaneSpec {
        name: "data · reliable",
        sn: 3419,
        continuous: true,
        dropped: 0,
    },
    LaneSpec {
        name: "interactive-high · reliable",
        sn: 1182,
        continuous: true,
        dropped: 0,
    },
    LaneSpec {
        name: "background · best-effort",
        sn: 3410,
        continuous: false,
        dropped: 1,
    },
];

/// How many channels the session negotiated, of which [`LANES`] carry traffic.
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
    /// One per [`QUERY_CLAUSES`] clause, keyed by index.
    Clauses,
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
            Population::Clauses => indexes(QUERY_CLAUSES.len()),
            Population::SavedFilters => indexes(SAVED_FILTERS.len()),
            Population::Annotated => picked(|row| !row.note.is_empty()),
            Population::Fragmented => picked(|row| row.fragment.is_some()),
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
        tag: "pv.filter.clause.{}",
        role: "status",
        population: Population::Clauses,
    },
    VoiceSpec {
        tag: "pv.filter.saved.{}",
        role: "button",
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
    // The fold chevron and the derived badge, announced with the item they
    // belong to: `aria-expanded` is the state the chevron draws, and "derived"
    // is a fact about the field beside it.
    ("pv.tree.layer.{}", Population::Layers, "part_of"),
    ("pv.tree.derived.{}", Population::Derived, "part_of"),
    // The highlight behind a byte the open field was read from.
    ("pv.bytes.lit.{}", Population::LitBytes, "decorative"),
];
