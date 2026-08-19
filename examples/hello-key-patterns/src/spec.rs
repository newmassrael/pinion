//! R1730 — **the reference's key-pattern section, written down as a value.**
//!
//! The same discipline `hello-node-lab`'s and `hello-packet-view`'s `spec`
//! modules hold for screens A and B, with the correction R1728 made to it: what
//! the reference *is* lives in `docs/analyzer-keys-spec.json`, a tracked
//! artifact reviewed as a claim, and this module holds what a **running**
//! screen needs — paint tags, widths, row data, the wording of a refusal.
//!
//! A specification written in the same file, in the same edit, by the same hand
//! as the thing it judges is a gate asking the subject for the answer. That was
//! measured on the rail: seven seats, three of them keys the reference does not
//! have, unremarked for several hundred rounds because nothing compared the two.
//!
//! **Vocabulary is neutral by construction.** The reference's protocol words,
//! configuration paths and resource names are replaced with the words the tool
//! class uses generally. The structure and the behaviour are what is being
//! reproduced, and those are what these tables hold.

use pinion_core::conformance::{Ledger, Part, SurfaceSpec};

// ── Geometry ────────────────────────────────────────────────────────────────

/// The window the standalone binary opens at.
pub const WIN_W: u32 = 1180;
/// The window height.
pub const WIN_H: u32 = 760;

/// The section header's height, as the reference draws it.
pub const HEADER_H: u32 = 46;
/// The column header row's height.
pub const COLHEAD_H: u32 = 32;
/// One declaration row's height.
pub const ROW_H: u32 = 40;
/// The record pane's width, as the reference fixes it.
pub const DETAIL_W: u32 = 320;
/// The horizontal padding the reference gives a row and the pane.
pub const PAD: u32 = 16;
/// The gap between two columns.
pub const GAP: u32 = 8;

/// The narrowest the pattern column may become.
///
/// The reference's own number: it lays its list out with six fixed columns and
/// `minmax(150px, 1fr)` for the pattern, so 150 is what the reference states
/// rather than what this build chose. It is what the screen's layout floor is
/// derived from.
pub const PATTERN_MIN: u32 = 150;

/// The filter box's width, inside the header.
pub const FILTER_W: u32 = 210;
/// Its height.
pub const FILTER_H: u32 = 32;

// ── The list's columns ──────────────────────────────────────────────────────

/// One column of the declaration list.
pub struct ColumnSpec {
    /// What the surface addresses it by — the suffix of its paint tag, the name
    /// a query may use, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it, in the header row.
    pub title: &'static str,
    /// Its width in logical pixels, or 0 for the column that takes the rest.
    ///
    /// The reference fixes six and lets the pattern take what is left, because
    /// that is the column a reader is actually reading.
    pub width: u32,
}

/// The seven columns, left to right.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        key: "id",
        title: "ID",
        width: 52,
    },
    ColumnSpec {
        key: "pattern",
        title: "Pattern",
        width: 0,
    },
    ColumnSpec {
        key: "by",
        title: "By",
        width: 74,
    },
    ColumnSpec {
        key: "direction",
        title: "Direction",
        width: 120,
    },
    ColumnSpec {
        key: "matches",
        title: "Match",
        width: 60,
    },
    ColumnSpec {
        key: "rate",
        title: "Msg/s",
        width: 64,
    },
    ColumnSpec {
        key: "status",
        title: "Status",
        width: 112,
    },
];

/// The names a query may address.
///
/// The column keys and nothing else, derived rather than written twice: a
/// second list is how a filter comes to accept a name no column has.
#[must_use]
pub fn query_columns() -> Vec<&'static str> {
    COLUMNS.iter().map(|c| c.key).collect()
}

// ── The section header ──────────────────────────────────────────────────────

/// One part of the section header.
pub struct HeaderPart {
    /// The suffix of its paint tag, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it — for the two parts the reference draws without a
    /// label, this file's word for what is there.
    pub title: &'static str,
}

/// The header, left to right: the section's name, how much is in it, and the
/// box that narrows it.
pub const HEADER: &[HeaderPart] = &[
    HeaderPart {
        key: "title",
        title: "Key Patterns",
    },
    HeaderPart {
        key: "summary",
        title: "Summary",
    },
    HeaderPart {
        key: "filter",
        title: "Filter",
    },
];

// ── The record pane ─────────────────────────────────────────────────────────

/// One part of the record pane, top to bottom.
pub struct DetailPart {
    /// The suffix of its paint tag, and the key the specification compares.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// How tall the part is.
    pub height: u32,
    /// Whether it shares its row with the part after it — the reference draws
    /// the four single facts as a two-by-two grid rather than a column.
    pub pairs: bool,
}

/// The eleven parts of the record pane.
///
/// The order is the reference's and the heights are this screen's, which is the
/// split the specification makes: a specification fixes what is there and in
/// what order, and a build decides how tall it is.
pub const DETAIL: &[DetailPart] = &[
    DetailPart {
        key: "subject",
        title: "Key Pattern",
        height: 20,
        pairs: false,
    },
    DetailPart {
        key: "ordinal",
        title: "Number",
        height: 20,
        pairs: false,
    },
    DetailPart {
        key: "pattern",
        title: "Pattern",
        height: 24,
        pairs: false,
    },
    DetailPart {
        key: "standing",
        title: "Standing",
        height: 24,
        pairs: false,
    },
    DetailPart {
        key: "declared_by",
        title: "Declared by",
        height: 50,
        pairs: true,
    },
    DetailPart {
        key: "direction",
        title: "Direction",
        height: 50,
        pairs: false,
    },
    DetailPart {
        key: "matches",
        title: "Matches",
        height: 50,
        pairs: true,
    },
    DetailPart {
        key: "rate",
        title: "Msg rate",
        height: 50,
        pairs: false,
    },
    DetailPart {
        key: "endpoints",
        title: "Matched endpoints",
        height: 46,
        pairs: false,
    },
    DetailPart {
        key: "first_seen",
        title: "First seen",
        height: 18,
        pairs: false,
    },
    DetailPart {
        key: "declarer",
        title: "Show declarer in Topology",
        height: 34,
        pairs: false,
    },
];

/// The rail seat the record pane's action leads to.
///
/// Named rather than spelled at the call site because the standing of that seat
/// is `docs/analyzer-rail-spec.json`'s fact and this screen only points at it.
pub const DECLARER_SECTION: &str = "topology";

// ── The declarations ────────────────────────────────────────────────────────

/// What a declaration's resolution amounts to.
///
/// Two arms, which is what the reference distinguishes: a declaration whose
/// pattern the session resolved, and one seen only as a numeric identifier
/// because its declaration was not captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// The pattern is known.
    Resolved,
    /// Only the number is known, so the pattern shown is inferred.
    NumericOnly,
}

impl Health {
    /// What a reader is told.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Health::Resolved => "Resolved",
            Health::NumericOnly => "Numeric-only",
        }
    }

    /// The wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Health::Resolved => "resolved",
            Health::NumericOnly => "numeric_only",
        }
    }
}

/// One declaration.
pub struct RowSpec {
    /// Its number, which is what the list and the pane both show.
    pub id: &'static str,
    /// The pattern declared.
    pub pattern: &'static str,
    /// The endpoint that declared it.
    pub by: &'static str,
    /// What it declared — publishing, subscribing or answering queries.
    pub direction: &'static str,
    /// How many other declarations it matches.
    pub matches: u32,
    /// Its message rate, or an em dash when none has been observed.
    pub rate: &'static str,
    /// Whether the pattern is known or only its number.
    pub health: Health,
    /// How long ago it was first seen.
    pub first_seen: &'static str,
    /// The endpoints whose declarations it matches.
    pub endpoints: &'static [&'static str],
}

impl RowSpec {
    /// The row's attributes in [`COLUMNS`] order, which is what a query reads.
    ///
    /// Derived from [`COLUMNS`] rather than written out a second time, so a
    /// column added without a cell panics in [`cell`](Self::cell) rather than
    /// silently comparing against the empty string — which reads as "nothing
    /// matches" and looks exactly like a correct empty result.
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
            "id" => self.id.to_owned(),
            "pattern" => self.pattern.to_owned(),
            "by" => self.by.to_owned(),
            "direction" => self.direction.to_owned(),
            "matches" => self.matches.to_string(),
            "rate" => self.rate.to_owned(),
            "status" => self.health.label().to_owned(),
            other => panic!("no column named {other}"),
        }
    }
}

/// The eight declarations the reference's section opens on.
///
/// The endpoint identifiers are the node lab's own — the sibling screen already
/// draws `R-01`, `S-01`, `T-01` and the peers — so a reader who follows a
/// declaration to the endpoint that made it lands on a node this tool knows
/// about. The reference's own sections do not share a roster; this build's do,
/// and that is an improvement on it rather than a divergence from it, because
/// nothing in the reference's structure says they must differ.
pub const ROWS: &[RowSpec] = &[
    RowSpec {
        id: "1",
        pattern: "admin/router/**",
        by: "R-01",
        direction: "declare queryable",
        matches: 1,
        rate: "12",
        health: Health::Resolved,
        first_seen: "-6h33m",
        endpoints: &["S-01"],
    },
    RowSpec {
        id: "2",
        pattern: "sensors/unit-1/pose",
        by: "T-01",
        direction: "declare publish",
        matches: 3,
        rate: "1.6k",
        health: Health::Resolved,
        first_seen: "-4h12m",
        endpoints: &["P-01", "P-02"],
    },
    RowSpec {
        id: "3",
        pattern: "sensors/unit-1/vel",
        by: "T-01",
        direction: "declare publish",
        matches: 2,
        rate: "1.5k",
        health: Health::Resolved,
        first_seen: "-4h12m",
        endpoints: &["P-01"],
    },
    RowSpec {
        id: "4",
        pattern: "sensors/unit-*/pose",
        by: "T-02",
        direction: "declare subscribe",
        matches: 2,
        rate: "864",
        health: Health::Resolved,
        first_seen: "-3h48m",
        endpoints: &["T-01"],
    },
    RowSpec {
        id: "5",
        pattern: "sensors/telemetry/**",
        by: "P-03",
        direction: "declare subscribe",
        matches: 5,
        rate: "-",
        health: Health::NumericOnly,
        first_seen: "-0h02m",
        endpoints: &["P-01", "P-02"],
    },
    RowSpec {
        id: "6",
        pattern: "store/**",
        by: "S-01",
        direction: "declare queryable",
        matches: 1,
        rate: "212",
        health: Health::Resolved,
        first_seen: "-1h05m",
        endpoints: &["R-01"],
    },
    RowSpec {
        id: "7",
        pattern: "mesh/telemetry",
        by: "P-02",
        direction: "declare publish",
        matches: 1,
        rate: "1.4k",
        health: Health::Resolved,
        first_seen: "-6h33m",
        endpoints: &["P-03"],
    },
    RowSpec {
        id: "8",
        pattern: "mesh/ctrl",
        by: "P-02",
        direction: "declare subscribe",
        matches: 1,
        rate: "40",
        health: Health::Resolved,
        first_seen: "-6h33m",
        endpoints: &["P-01"],
    },
];

/// Which declaration the section opens on.
///
/// The reference opens on its second row rather than its first, and the choice
/// is not decorative: the first row is the administrative declaration every
/// session has, and the second is one of the session's own.
pub const OPENING_ROW: usize = 1;

/// What this screen tells a person the pointer and the keyboard do.
///
/// Taken from the behaviour reference's own section rather than invented. It
/// binds a click on a declaration and typing in the filter, and this build adds
/// the keyboard walk the reference has no binding for at all.
pub const GESTURES: &[(&str, &str)] = &[
    ("click a declaration", "show its record"),
    ("type in the filter", "narrow the list"),
    ("up and down", "walk the declarations"),
];

/// What the filter box shows when no query is running.
///
/// The reference shows a pattern here rather than an instruction, and this
/// keeps that posture while making the hint a clause the grammar actually
/// accepts — the box in the reference is decorative and this one filters, so a
/// hint that could not be typed would be worse than no hint.
///
/// ★ It is also short **on purpose**, and the measurement is recorded because
/// the reason is not obvious: a single-line field does not clip, so a hint
/// wider than the 210-pixel box the reference draws is painted over the column
/// header beside it. The first draft of this string was 44 characters and this
/// screen's own containment gate reported it.
pub const FILTER_PLACEHOLDER: &str = "by in (T-01)";

// ── The specification, and where this build does not reproduce it ───────────

/// The specification, as text, compiled in.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging. It is the same posture the rail's pin and the
/// census pins take.
const KEYS_SPEC_JSON: &str = include_str!("../../../docs/analyzer-keys-spec.json");

/// The specification document, parsed.
///
/// # Panics
///
/// If the pin is not readable JSON — a defect in the pin rather than a state
/// the running screen can reach.
#[must_use]
fn document() -> serde_json::Value {
    serde_json::from_str(KEYS_SPEC_JSON).expect("the section specification is readable JSON")
}

/// The three surfaces the specification fixes, by the name it gives them.
pub const SURFACES: &[&str] = &["header", "columns", "detail"];

/// One surface of the specification, as the framework's comparable form.
///
/// # Panics
///
/// If the pin does not declare `surface` as a roster of named parts — a defect
/// in the pin. It must stop the build rather than quietly weaken the
/// comparison, which is what an empty or unparsed specification would do.
#[must_use]
pub fn canon(surface: &str) -> SurfaceSpec {
    let doc = document();
    let parts = doc[surface]["canon"]
        .as_array()
        .unwrap_or_else(|| panic!("the specification declares a `{surface}` canon array"))
        .iter()
        .map(|part| {
            Part::new(
                part["key"]
                    .as_str()
                    .expect("a specified part has a key")
                    .to_owned(),
                part["title"]
                    .as_str()
                    .expect("a specified part has a title")
                    .to_owned(),
            )
        })
        .collect();
    SurfaceSpec::new(parts).expect("the specification is a roster of named parts")
}

/// The declared, reviewed remainder for one surface.
///
/// # Panics
///
/// If the pin's `owed` entries are malformed, name no round, state no reason or
/// do not name their own part — all defects in the pin, all refused by the
/// framework's own loader rather than by a check written here.
#[must_use]
pub fn owed(surface: &str) -> Ledger {
    Ledger::from_json(&document()[surface])
        .unwrap_or_else(|e| panic!("the `{surface}` remainder is a readable ledger: {e:?}"))
}
