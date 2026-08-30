//! R1731 — **the reference's log section, written down as a value.**
//!
//! What the reference *is* lives in `docs/analyzer-logs-spec.json`, a tracked
//! artifact reviewed as a claim; this module holds what a **running** screen
//! needs — paint tags, widths, event data, the vocabulary of a severity.
//!
//! **Vocabulary is neutral by construction.** The reference's protocol words,
//! configuration paths and resource names are replaced with the words the tool
//! class uses generally. The structure and the behaviour are what is being
//! reproduced, and those are what these tables hold.

use pinion_core::conformance::SpecDocument;

// ── Geometry ────────────────────────────────────────────────────────────────

/// The window the standalone binary opens at.
pub const WIN_W: u32 = 1220;
/// The window height.
pub const WIN_H: u32 = 760;

/// The section header's height, as the reference draws it.
pub const HEADER_H: u32 = 46;
/// The column header row's height.
pub const COLHEAD_H: u32 = 32;
/// One event row's height. The reference gives this list tighter rows than its
/// key-pattern section — a log is read by scanning and a declaration by reading.
pub const ROW_H: u32 = 34;
/// The decode pane's width, as the reference fixes it.
pub const DETAIL_W: u32 = 340;
/// The horizontal padding the reference gives a row and the pane.
pub const PAD: u32 = 16;
/// The gap between two columns.
pub const GAP: u32 = 10;

/// The narrowest the message column may become. The reference's own number:
/// it lays the list out with four fixed columns and `minmax(110px, 1fr)`.
pub const MESSAGE_MIN: u32 = 110;

/// The filter box's width, inside the header.
pub const FILTER_W: u32 = 190;
/// Its height.
pub const FILTER_H: u32 = 32;
/// The severity choice's width.
pub const SEVERITY_W: u32 = 168;

// ── The list's columns ──────────────────────────────────────────────────────

/// One column of the event list.
pub struct ColumnSpec {
    /// What the surface addresses it by — the suffix of its paint tag, the name
    /// a query may use, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it, in the header row.
    pub title: &'static str,
    /// ★★★★★ R1918 — **what the column holds**, in a sentence.
    ///
    /// A required field and not an `Option`: three of these five titles are
    /// abbreviations a header band has no room to expand (`Sev`, `Src`), which
    /// is the canon's own rule for which controls get a `title` attribute —
    /// *the ones with no room to print what they do*. Making it required means a
    /// column added later **cannot compile** without saying what it holds, which
    /// is a stronger gate than any test over the table.
    pub description: &'static str,
    /// Its width in logical pixels, or 0 for the column that takes the rest.
    pub width: u32,
}

/// The five columns, left to right.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        key: "time",
        title: "Time",
        description: "When the event was seen, to the millisecond",
        width: 112,
    },
    ColumnSpec {
        key: "severity",
        title: "Sev",
        description: "How bad the event is - info, warn or error",
        width: 58,
    },
    ColumnSpec {
        key: "source",
        title: "Src",
        description: "The endpoint the event was reported by",
        width: 54,
    },
    ColumnSpec {
        key: "type",
        title: "Type",
        description: "What kind of message the event is about",
        width: 104,
    },
    ColumnSpec {
        key: "message",
        title: "Message",
        description: "The one-line reading of the event",
        width: 0,
    },
];

/// The names a query may address — the column keys and nothing else, derived
/// rather than written twice.
#[must_use]
pub fn query_columns() -> Vec<&'static str> {
    COLUMNS.iter().map(|c| c.key).collect()
}

// ── The section header ──────────────────────────────────────────────────────

/// One part of the section header.
pub struct HeaderPart {
    /// The suffix of its paint tag, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it — for the parts the reference draws without a
    /// label, this build's word for what is there.
    pub title: &'static str,
}

/// The header, left to right.
pub const HEADER: &[HeaderPart] = &[
    HeaderPart {
        key: "title",
        title: "Logs",
    },
    HeaderPart {
        key: "live",
        title: "Capture state",
    },
    HeaderPart {
        key: "filter",
        title: "Filter",
    },
    HeaderPart {
        key: "severity",
        title: "Severity",
    },
];

// ── The decode pane ─────────────────────────────────────────────────────────

/// The faces this screen sets its text in.
///
/// ★★★★★ R1877 — **here rather than in the painter, because the parts' heights
/// are now DERIVED from them** and a face spelled in two files is a face that
/// can disagree with the geometry built on it. R1834 moved a face into the
/// specification for the same reason one screen over.
pub const FONT_TINY: u32 = 10;
/// The face a list's own label is set in.
pub const FONT_SMALL: u32 = 11;
/// The face a record's message is set in.
pub const FONT_BODY: u32 = 12;
/// The face the pane's title is set in.
pub const FONT_TITLE: u32 = 14;

/// The clearance a fixed part keeps above and below the single run it holds.
pub const DETAIL_PART_PAD: u32 = 1;

/// One part of the decode pane, top to bottom.
pub struct DetailPart {
    /// The suffix of its paint tag, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// The face the part's own run is set in, or `0` when the part is a LIST
    /// whose height comes from its content — the decoded fields and the bytes.
    ///
    /// ★★★★★ R1877 — this replaces a hand-written `height`, and the swap is the
    /// round. The field it replaces was documented *"the part is measured from
    /// what it holds"*, and **four of the six did not do that**: `subject` was
    /// 20 for a 14px face whose line box is 23, and `meta` was 16 for an 11px
    /// face wanting 18 — both too short to hold the word they exist to show.
    /// A part now declares WHAT IT HOLDS and [`DetailPart::height`] derives the
    /// rest, so the sentence and the number cannot part company again.
    pub face: u32,
}

impl DetailPart {
    /// How tall this part is: the line box of the face it holds, plus its
    /// clearance — or `0` for a list, whose height is its content's.
    #[must_use]
    pub const fn height(&self) -> u32 {
        if self.face == 0 {
            0
        } else {
            pinion_core::containment::line_box(self.face) + DETAIL_PART_PAD * 2
        }
    }
}

/// The six parts of the decode pane.
pub const DETAIL: &[DetailPart] = &[
    DetailPart {
        key: "subject",
        title: "Decode Inspector",
        face: FONT_TITLE,
    },
    DetailPart {
        key: "kind",
        title: "Type",
        face: FONT_SMALL,
    },
    DetailPart {
        key: "message",
        title: "Message",
        face: FONT_BODY,
    },
    DetailPart {
        key: "meta",
        title: "When and where",
        face: FONT_SMALL,
    },
    DetailPart {
        key: "layers",
        title: "Decoded layers",
        face: 0,
    },
    DetailPart {
        key: "bytes",
        title: "Wire bytes",
        face: 0,
    },
];

/// One decoded field row's height inside the `layers` part.
///
/// ★ R1877 — derived. It was `22`, which happens to be what an 11px face wants
/// plus four; now it says so, and a face change moves it.
pub const FIELD_H: u32 = pinion_core::containment::line_box(FONT_SMALL) + 4;
/// The label above a list part.
///
/// ★★ R1877 — derived, and it was **too short**: `16` for a 10px face whose
/// line box is 17, so both list labels this screen paints were one pixel
/// shorter than the words they hold.
pub const LIST_LABEL_H: u32 = pinion_core::containment::line_box(FONT_TINY);
/// One line of the byte dump.
///
/// ★ R1877 — derived from the face it shows, where the painter spelled `18`
/// twice: once as the row pitch and once as the run's own box.
pub const BYTE_LINE_H: u32 = pinion_core::containment::line_box(FONT_SMALL);

// ── Severity ────────────────────────────────────────────────────────────────

/// How bad an event is.
///
/// Three arms, which is what the reference distinguishes and what its header's
/// severity choice filters on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something happened.
    Info,
    /// Something is not right and the session continues.
    Warn,
    /// Something failed.
    Error,
}

impl Severity {
    /// What a reader is told.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }

    /// Every arm, which is what the choice is built from.
    pub const ALL: &'static [Severity] = &[Severity::Info, Severity::Warn, Severity::Error];
}

/// One choice on the header's severity control.
pub struct ChoiceSpec {
    /// The suffix of its paint tag and the key the wire addresses it by.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// The least severity it keeps, or `None` for the choice that keeps all.
    pub floor: Option<Severity>,
}

impl ChoiceSpec {
    /// ★★★★★ R1918 — what choosing this does, **derived from the floor**.
    ///
    /// The label cannot carry it: a choice reading `Warn` keeps warnings *and*
    /// errors, because a floor is an ordering rather than a set — the very
    /// thing the table's own comment says three independent toggles could not
    /// express. So the control's one word is exactly the fact a reader needs a
    /// sentence for.
    ///
    /// Derived rather than declared, so a choice added to [`CHOICES`] arrives
    /// described and cannot describe itself as keeping a different floor from
    /// the one it filters by.
    #[must_use]
    pub fn description(&self) -> String {
        match self.floor {
            None => "Keep every event, whatever its severity".to_owned(),
            Some(floor) => format!("Keep only events at {} or worse", floor.label()),
        }
    }
}

/// The three choices, in the reference's order.
///
/// Exclusive rather than independent: the reference draws exactly one of them
/// filled, and a severity floor is an ordering rather than a set — *warnings*
/// means warnings **and** errors, which three independent toggles could not say.
pub const CHOICES: &[ChoiceSpec] = &[
    ChoiceSpec {
        key: "all",
        title: "All",
        floor: None,
    },
    ChoiceSpec {
        key: "warn",
        title: "Warn",
        floor: Some(Severity::Warn),
    },
    ChoiceSpec {
        key: "error",
        title: "Error",
        floor: Some(Severity::Error),
    },
];

/// Which choice the section opens on.
pub const OPENING_CHOICE: usize = 0;

// ── The events ──────────────────────────────────────────────────────────────

/// One captured event.
pub struct RowSpec {
    /// When it was seen.
    pub time: &'static str,
    /// How bad it is.
    pub severity: Severity,
    /// The endpoint it came from.
    pub source: &'static str,
    /// What kind of message it was.
    pub kind: &'static str,
    /// The one-line reading.
    pub message: &'static str,
    /// The decoded fields, in the order the reference lists them.
    pub fields: &'static [(&'static str, &'static str)],
    /// The frame's bytes, or empty when no frame arrived — which is a fact the
    /// reference draws rather than an absence it hides.
    pub bytes: &'static [u8],
}

impl RowSpec {
    /// The row's attributes in [`COLUMNS`] order, which is what a query reads.
    #[must_use]
    pub fn attributes(&self) -> Vec<String> {
        COLUMNS.iter().map(|column| self.cell(column.key)).collect()
    }

    /// One cell of the row, by column key.
    ///
    /// # Panics
    ///
    /// If asked for a column [`COLUMNS`] does not declare, which is a defect in
    /// this file rather than a state the screen can reach.
    #[must_use]
    pub fn cell(&self, column: &str) -> String {
        match column {
            "time" => self.time.to_owned(),
            "severity" => self.severity.label().to_owned(),
            "source" => self.source.to_owned(),
            "type" => self.kind.to_owned(),
            "message" => self.message.to_owned(),
            other => panic!("no column named {other}"),
        }
    }
}

/// The ten events the reference's section opens on.
///
/// The endpoint identifiers are the sibling sections' own, so a reader who
/// follows an event to the endpoint that produced it lands on a node this tool
/// knows about. The awkward ones are here on purpose, because they are the
/// requirement: a warning whose frame never arrived (so the byte pane has
/// nothing to show and must say so rather than look broken), and an error that
/// ends a session.
pub const ROWS: &[RowSpec] = &[
    RowSpec {
        time: "12:04:38.221",
        severity: Severity::Info,
        source: "T-01",
        kind: "Data",
        message: "Push sensors/unit-1/pose",
        fields: &[
            ("Message", "Push (Data)"),
            ("Pattern", "sensors/unit-1/pose"),
            ("Payload", "48 B"),
            ("Encoding", "app/record"),
            ("Priority", "RealTime"),
            ("Sequence", "48211"),
        ],
        bytes: &[
            0x0b, 0x51, 0x12, 0x03, 0xa4, 0x7f, 0x00, 0x12, 0x5f, 0x6f, 0x3a, 0x21, 0x88, 0xc0,
            0x12, 0x4d,
        ],
    },
    RowSpec {
        time: "12:04:38.198",
        severity: Severity::Info,
        source: "S-01",
        kind: "Query",
        message: "Get store/**",
        fields: &[
            ("Message", "Request (Query)"),
            ("Pattern", "store/**"),
            ("Target", "All complete"),
            ("Request id", "9032"),
            ("Sequence", "9031"),
        ],
        bytes: &[
            0x0d, 0x41, 0x08, 0x64, 0x62, 0x2f, 0x2a, 0x2a, 0x00, 0x02, 0x33, 0x07, 0x90, 0x32,
            0x00, 0x00,
        ],
    },
    RowSpec {
        time: "12:04:38.140",
        severity: Severity::Info,
        source: "R-01",
        kind: "Response",
        message: "Reply store/config",
        fields: &[
            ("Message", "Reply (Response)"),
            ("Pattern", "store/config"),
            ("Payload", "212 B"),
            ("Request id", "9032"),
        ],
        bytes: &[
            0x0e, 0x41, 0x63, 0x6f, 0x6e, 0x66, 0x69, 0x67, 0xd4, 0x00, 0x07, 0x90, 0x32, 0x11,
            0x02, 0xaa,
        ],
    },
    RowSpec {
        time: "12:04:37.902",
        severity: Severity::Info,
        source: "T-02",
        kind: "Declaration",
        message: "Declare subscribe sensors/unit-*/pose",
        fields: &[
            ("Message", "Declare Subscriber"),
            ("Pattern id", "4"),
            ("Pattern", "sensors/unit-*/pose"),
        ],
        bytes: &[
            0x0f, 0x04, 0x72, 0x6f, 0x62, 0x6f, 0x74, 0x73, 0x2f, 0x2a, 0x2f, 0x70, 0x6f, 0x73,
            0x65, 0x00,
        ],
    },
    RowSpec {
        time: "12:04:37.771",
        severity: Severity::Info,
        source: "P-02",
        kind: "Transport",
        message: "KeepAlive P-02",
        fields: &[
            ("Message", "KeepAlive"),
            ("Link", "datagram"),
            ("Endpoint id", "5e10c4aa"),
        ],
        bytes: &[
            0x20, 0x5e, 0x10, 0xc4, 0xaa, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
    },
    RowSpec {
        time: "12:04:36.559",
        severity: Severity::Warn,
        source: "P-03",
        kind: "Transport",
        message: "KeepAlive timeout P-03",
        fields: &[
            ("Message", "KeepAlive timeout"),
            ("Link", "datagram"),
            ("Retry", "1 / 3"),
        ],
        // ★ Nothing arrived. The reference draws this case rather than hiding
        // it, so the byte pane has to be able to say so.
        bytes: &[],
    },
    RowSpec {
        time: "12:04:35.120",
        severity: Severity::Info,
        source: "P-02",
        kind: "Data",
        message: "Push mesh/telemetry",
        fields: &[
            ("Message", "Push (Data)"),
            ("Pattern", "mesh/telemetry"),
            ("Payload", "96 B"),
            ("Priority", "Data"),
        ],
        bytes: &[
            0x0b, 0x42, 0x6d, 0x65, 0x73, 0x68, 0x2f, 0x74, 0x65, 0x6c, 0x65, 0x6d, 0x65, 0x74,
            0x72, 0x79,
        ],
    },
    RowSpec {
        time: "12:04:34.007",
        severity: Severity::Error,
        source: "P-01",
        kind: "Transport",
        message: "Session close P-01 (link error)",
        fields: &[
            ("Message", "Close"),
            ("Reason", "link error / io"),
            ("Link", "stream"),
            ("Endpoint id", "0f22ab91"),
        ],
        bytes: &[
            0x22, 0x0f, 0x22, 0xab, 0x91, 0x01, 0x03, 0x69, 0x6f, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
    },
    RowSpec {
        time: "12:04:33.884",
        severity: Severity::Info,
        source: "S-01",
        kind: "Declaration",
        message: "Declare queryable store/**",
        fields: &[
            ("Message", "Declare Queryable"),
            ("Pattern id", "6"),
            ("Pattern", "store/**"),
        ],
        bytes: &[
            0x0f, 0x06, 0x64, 0x62, 0x2f, 0x2a, 0x2a, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
    },
    RowSpec {
        time: "12:04:33.221",
        severity: Severity::Info,
        source: "T-01",
        kind: "Data",
        message: "Push sensors/unit-1/vel",
        fields: &[
            ("Message", "Push (Data)"),
            ("Pattern", "sensors/unit-1/vel"),
            ("Payload", "32 B"),
            ("Priority", "RealTime"),
        ],
        bytes: &[
            0x0b, 0x51, 0x12, 0x76, 0x65, 0x6c, 0x00, 0x20, 0xa1, 0x4f, 0x00, 0x12, 0x88, 0xc0,
            0x00, 0x00,
        ],
    },
];

/// Which event the section opens on — the newest, which is what a log opens on.
pub const OPENING_ROW: usize = 0;

/// What the section says when the selected event has no frame.
pub const NO_FRAME: &str = "no frame arrived";

/// What this screen tells a person the pointer and the keyboard do.
pub const GESTURES: &[(&str, &str)] = &[
    ("click an event", "decode it"),
    ("type in the filter", "narrow the list"),
    ("click a severity", "keep that severity and worse"),
    ("up and down", "walk the events"),
];

/// What the filter box shows when no query is running.
///
/// Short on purpose: a single-line field does not clip, so a hint wider than the
/// box the reference draws is painted over the column header beside it. See the
/// debt note the sibling section opened.
pub const FILTER_PLACEHOLDER: &str = "type in (Data)";

// ── The specification ───────────────────────────────────────────────────────

/// The specification, as text, compiled in.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging.
const LOGS_SPEC_JSON: &str = include_str!("../../../docs/analyzer-logs-spec.json");

/// The specification, as the framework's own document.
///
/// # Panics
///
/// If the pin is not a specification — unreadable JSON, no surfaces, a duplicate
/// part key, a remainder entry naming no round. All are defects in the pin
/// rather than states the running screen can reach.
#[must_use]
pub fn document() -> SpecDocument {
    SpecDocument::pinned(LOGS_SPEC_JSON, "docs/analyzer-logs-spec.json")
}
