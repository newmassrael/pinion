//! ★★★★★ R1948 — **the reference's sessions section, written down as a value.**
//!
//! The sibling of `hello-topology-view::spec`, and written for the same reason:
//! a round can *claim* to have reproduced a screen or it can put the screen in a
//! table a machine reads and let a test compare the painted scene against it in
//! **both** directions.
//!
//! # One capture, two sections
//!
//! Every session here names a PEER, and those are the six
//! `hello-topology-view` plots. The reference's detail pane has a control that
//! jumps to the graph with that peer selected, so the two sections are views of
//! one capture rather than two screens sharing a rail — and that is asserted
//! (`crate::tests`) against `hello_topology_view::peers()` rather than left to
//! two tables staying in step by hand.

use pinion_core::conformance::SpecDocument;
use pinion_core::scene::Rect;
use pinion_core::widgets::severity::SeverityScale;

// ── The window ──────────────────────────────────────────────────────────────

/// The width the section is specified at, matching the pin's `$at`.
pub const WIN_W: u32 = 1220;
/// The height the section is specified at.
pub const WIN_H: u32 = 760;

/// The detail pane's width.
pub const DETAIL_W: u32 = 320;
/// The header strip's height, over both panes' own content.
pub const HEADER_H: u32 = 46;
/// The column-heading strip's height.
pub const COLHEAD_H: u32 = 34;
/// One session row's height.
pub const ROW_H: u32 = 38;

/// The padding inside a pane.
pub const PAD: u32 = 16;
/// The gap between two grid columns.
pub const GAP: u32 = 8;

// ── Type ────────────────────────────────────────────────────────────────────

/// The smallest text — a caption under a control, a column heading.
pub const FONT_TINY: u32 = 10;
/// A monospace cell, a chip, a secondary run.
pub const FONT_SMALL: u32 = 11;
/// Body text: a cell, a control's label.
pub const FONT_BODY: u32 = 13;
/// A pane's own title.
pub const FONT_TITLE: u32 = 14;
/// The detail's headline.
pub const FONT_HEADLINE: u32 = 20;

// ── The list's columns ──────────────────────────────────────────────────────

/// One column of the session list.
pub struct ColumnSpec {
    /// The suffix of the column's paint tag.
    pub key: &'static str,
    /// The heading a reader reads.
    pub title: &'static str,
    /// How wide the column is drawn, in pixels.
    ///
    /// ★ The reference states its grid as
    /// `84px minmax(110px,1fr) 70px 70px 84px 80px 70px 118px` — seven fixed
    /// tracks and one that takes the slack. `width` is the fixed part and
    /// [`ColumnSpec::stretches`] marks the one that grows, so the same table
    /// answers both what the reference declared and what this build draws at
    /// any width.
    pub width: u32,
    /// Whether this column absorbs the width the fixed ones leave.
    pub stretches: bool,
}

/// The eight columns, in the reference's own order.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        key: "session",
        title: "Session",
        width: 84,
        stretches: false,
    },
    ColumnSpec {
        key: "peer",
        title: "Peer",
        width: 110,
        stretches: true,
    },
    ColumnSpec {
        key: "link",
        title: "Link",
        width: 70,
        stretches: false,
    },
    ColumnSpec {
        key: "role",
        title: "Role",
        width: 70,
        stretches: false,
    },
    ColumnSpec {
        key: "encryption",
        title: "Enc",
        width: 84,
        stretches: false,
    },
    ColumnSpec {
        key: "uptime",
        title: "Uptime",
        width: 80,
        stretches: false,
    },
    ColumnSpec {
        key: "rate",
        title: "Msg/s",
        width: 70,
        stretches: false,
    },
    ColumnSpec {
        key: "status",
        title: "Status",
        width: 118,
        stretches: false,
    },
];

// ── The population ──────────────────────────────────────────────────────────

/// How a session is doing.
///
/// ★ Three states, and the reference has three. Unlike the topology section's
/// standings these line up one for one, because a session either completed its
/// handshake, is trying again, or is over — there is no fourth thing a session
/// can be, which is why this is an enum rather than a string the rows carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The handshake completed and the link is carrying traffic.
    Established,
    /// The link dropped and the peer is trying again.
    Reconnecting,
    /// The session is over.
    Closed,
}

impl Standing {
    /// What a reader calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Established => "Established",
            Self::Reconnecting => "Reconnecting",
            Self::Closed => "Closed",
        }
    }

    /// How bad it is — a word of [`SEVERITY`], which `crate::tests` checks
    /// rather than assumes.
    #[must_use]
    pub const fn severity(self) -> &'static str {
        match self {
            Self::Established => "info",
            Self::Reconnecting => "warn",
            Self::Closed => "error",
        }
    }

    /// Whether a session in this state is still counted as running.
    ///
    /// ★ What the header's count is derived THROUGH. The reference writes
    /// `5 active / 1 closed` into its markup; deriving both halves from this
    /// predicate is what keeps the sentence true when the capture changes,
    /// and what makes "active" a property of a state rather than a number
    /// somebody typed.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Established | Self::Reconnecting)
    }
}

/// The graded words this section's standings are drawn from — the same scale
/// the shell's alarm feed and the topology section grade by.
pub const SEVERITY: SeverityScale = SeverityScale::new(&["info", "warn", "error"]);

/// One observed session.
pub struct SessionSpec {
    /// What the capture calls it, and the suffix of its paint tag.
    pub id: &'static str,
    /// The peer at the other end — one of `hello_topology_view::peers()`.
    pub peer: &'static str,
    /// The peer's session identifier.
    pub zid: &'static str,
    /// What the peer is acting as.
    pub role: &'static str,
    /// The transport carrying it.
    pub link: &'static str,
    /// How it is doing.
    pub standing: Standing,
    /// How the link is secured.
    pub encryption: &'static str,
    /// How long it has been up.
    pub uptime: &'static str,
    /// What it is moving, per second.
    pub rate: &'static str,
    /// The protocol version the two ends agreed on.
    pub version: &'static str,
    /// The batch size they agreed on.
    pub batch: &'static str,
    /// The sequence-number resolution they agreed on.
    pub resolution: &'static str,
}

/// The sessions the capture observed.
///
/// ★ Six, over the six peers the topology section plots — one session per peer,
/// which is what makes `peer` a JOIN rather than a label. The closed one is
/// deliberate: a list where every row is healthy never draws the failed state,
/// and a reader cannot recognise what they have never seen.
pub const SESSIONS: &[SessionSpec] = &[
    SessionSpec {
        id: "S-01",
        peer: "P-01",
        zid: "a3f1c8d2",
        role: "peer",
        link: "tcp",
        standing: Standing::Established,
        encryption: "TLS 1.3",
        uptime: "4h 12m",
        rate: "3.1k",
        version: "0x09",
        batch: "65535",
        resolution: "u16",
    },
    SessionSpec {
        id: "S-02",
        peer: "P-02",
        zid: "7c02ff10",
        role: "peer",
        link: "tcp",
        standing: Standing::Established,
        encryption: "TLS 1.3",
        uptime: "3h 48m",
        rate: "864",
        version: "0x09",
        batch: "65535",
        resolution: "u16",
    },
    SessionSpec {
        id: "S-03",
        peer: "P-03",
        zid: "b5519a3e",
        role: "peer",
        link: "quic",
        standing: Standing::Reconnecting,
        encryption: "TLS 1.3",
        uptime: "0h 02m",
        rate: "-",
        version: "0x09",
        batch: "32768",
        resolution: "u16",
    },
    SessionSpec {
        id: "S-04",
        peer: "P-04",
        zid: "d9b8e720",
        role: "client",
        link: "tcp",
        standing: Standing::Established,
        encryption: "none",
        uptime: "1h 05m",
        rate: "212",
        version: "0x09",
        batch: "65535",
        resolution: "u16",
    },
    SessionSpec {
        id: "S-05",
        peer: "P-05",
        zid: "5e10c4aa",
        role: "peer",
        link: "quic",
        standing: Standing::Established,
        encryption: "QUIC",
        uptime: "6h 33m",
        rate: "1.4k",
        version: "0x09",
        batch: "65535",
        resolution: "u32",
    },
    SessionSpec {
        id: "S-06",
        peer: "P-06",
        zid: "0f22ab91",
        role: "peer",
        link: "tcp",
        standing: Standing::Closed,
        encryption: "-",
        uptime: "-",
        rate: "0",
        version: "-",
        batch: "-",
        resolution: "-",
    },
];

impl SessionSpec {
    /// What this session's cell in `column` reads.
    ///
    /// ★ One derivation, so the grid and the wire cannot disagree about what a
    /// row says. The population is the column table's, so a column added there
    /// has to be answered here or the compiler's match is not exhaustive —
    /// which is the point: a new column that silently drew blank is exactly the
    /// defect a `_ => ""` arm would allow.
    ///
    /// # Panics
    ///
    /// If asked for a column the table does not declare, which is a defect in
    /// the caller rather than a state the screen can reach.
    #[must_use]
    pub fn cell(&self, column: &str) -> &'static str {
        match column {
            "session" => self.id,
            "peer" => self.zid,
            "link" => self.link,
            "role" => self.role,
            "encryption" => self.encryption,
            "uptime" => self.uptime,
            "rate" => self.rate,
            "status" => self.standing.label(),
            other => panic!("no session column named {other}"),
        }
    }

    /// The handshake this session went through, as the detail pane draws it.
    ///
    /// ★★★★★ DERIVED from the standing rather than stored per session. The
    /// reference builds the same list the same way — four steps every session
    /// takes, then a fifth that depends on how it is doing — and storing it
    /// would let a row claim a handshake its own status contradicts.
    #[must_use]
    pub fn timeline(&self) -> Vec<TimelineStep> {
        let at = self.uptime;
        let mut steps = vec![
            TimelineStep::new("Init request", at, "info"),
            TimelineStep::new("Init reply", at, "info"),
            TimelineStep::new("Open request", at, "info"),
            TimelineStep::new("Open reply", at, "info"),
        ];
        match self.standing {
            Standing::Established => {
                steps.push(TimelineStep::new("Keepalive", "now", "info"));
            }
            Standing::Reconnecting => {
                steps.push(TimelineStep::new("Keepalive lost", "-8s", "warn"));
                steps.push(TimelineStep::new("Reconnecting", "now", "warn"));
            }
            Standing::Closed => {
                steps.push(TimelineStep::new("Closed (link error)", "-2m", "error"));
            }
        }
        steps
    }

    /// What each channel's last sequence number reads.
    ///
    /// A closed session has none — stated as a dash rather than as an empty
    /// cell, because a blank is indistinguishable from a value that failed to
    /// draw.
    #[must_use]
    pub fn sequence(&self, channel: &ChannelSpec) -> &'static str {
        if self.standing == Standing::Closed {
            "-"
        } else {
            channel.sequence
        }
    }
}

/// One step of the handshake timeline.
pub struct TimelineStep {
    /// What happened.
    pub label: &'static str,
    /// When, relative to now.
    pub at: &'static str,
    /// How it is graded — a word of [`SEVERITY`].
    pub severity: &'static str,
}

impl TimelineStep {
    const fn new(label: &'static str, at: &'static str, severity: &'static str) -> Self {
        Self {
            label,
            at,
            severity,
        }
    }
}

/// One quality-of-service channel a session carries.
pub struct ChannelSpec {
    /// What a reader calls it.
    pub name: &'static str,
    /// What the channel promises about delivery.
    pub reliability: &'static str,
    /// The last sequence number seen on it, while the session is up.
    pub sequence: &'static str,
}

/// The channels every session carries, in the reference's order.
pub const CHANNELS: &[ChannelSpec] = &[
    ChannelSpec {
        name: "RealTime",
        reliability: "best-effort",
        sequence: "#48211",
    },
    ChannelSpec {
        name: "Interactive",
        reliability: "reliable",
        sequence: "#9032",
    },
    ChannelSpec {
        name: "Data",
        reliability: "reliable",
        sequence: "#155120",
    },
];

// ── The header's controls ───────────────────────────────────────────────────

/// One status chip in the list header.
pub struct ChipSpec {
    /// The suffix of its paint tag.
    pub key: &'static str,
    /// What it says.
    pub title: &'static str,
    /// The standing it keeps, or `None` for the chip that keeps everything.
    ///
    /// ★ An `Option` and not a fourth `Standing`: "all" is not a state a
    /// session can be in, and giving it one would put it in the severity scale
    /// and in the count. R1928's rule the other way round — two answers where
    /// the empty one is meaningful.
    pub keeps: Option<Standing>,
}

/// The chips, in the reference's own order.
pub const CHIPS: &[ChipSpec] = &[
    ChipSpec {
        key: "all",
        title: "All",
        keeps: None,
    },
    ChipSpec {
        key: "established",
        title: "Established",
        keeps: Some(Standing::Established),
    },
    ChipSpec {
        key: "reconnecting",
        title: "Reconnecting",
        keeps: Some(Standing::Reconnecting),
    },
];

/// What the filter field says when it is empty.
pub const FILTER_HINT: &str = "filter session, peer, link\u{2026}";

// ── The detail's actions ────────────────────────────────────────────────────

/// The session this section opens with picked.
pub const OPENS_ON: &str = "S-01";

/// How tall one negotiated tile is.
pub const TILE_H: u32 = 52;

/// The most steps any session's handshake can have.
///
/// ★ A constant because the geometry below it must be a `const fn` — and it is
/// held to the tables by `tests::r1948_the_handshake_fits_the_space_reserved_for_it`,
/// which walks every session and fails if one is longer. A number nothing
/// checks is the shape this file spends its comments warning about; this one is
/// checked.
pub const MAX_HANDSHAKE_STEPS: u32 = 6;

// ── The pin ─────────────────────────────────────────────────────────────────

/// The written specification, compiled in.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file.
const SESSIONS_SPEC_JSON: &str = include_str!("../../../docs/analyzer-sessions-spec.json");

/// This section's two surfaces, as `docs/analyzer-sessions-spec.json` states
/// them.
///
/// # Panics
///
/// If the document is not a specification — all such defects must stop the
/// build rather than weaken the gate.
#[must_use]
pub fn document() -> SpecDocument {
    SpecDocument::pinned(SESSIONS_SPEC_JSON, "docs/analyzer-sessions-spec.json")
}

/// One part of a pane, as this build draws it.
pub struct PartSpec {
    /// The suffix of the part's paint tag.
    pub key: &'static str,
    /// What a reader calls it — held against the pin's own title.
    pub title: &'static str,
}

/// The list pane's parts, in the order they are drawn.
pub const LIST: &[PartSpec] = &[
    PartSpec {
        key: "title",
        title: "Sessions",
    },
    PartSpec {
        key: "count",
        title: "Observed sessions",
    },
    PartSpec {
        key: "filter",
        title: "Filter",
    },
    PartSpec {
        key: "chips",
        title: "Status filter",
    },
    PartSpec {
        key: "columns",
        title: "Column headings",
    },
    PartSpec {
        key: "rows",
        title: "The session list",
    },
];

/// The detail pane's parts.
pub const DETAIL: &[PartSpec] = &[
    PartSpec {
        key: "title",
        title: "Session detail",
    },
    PartSpec {
        key: "badge",
        title: "Selected session",
    },
    PartSpec {
        key: "id",
        title: "Session identifier",
    },
    PartSpec {
        key: "status",
        title: "Status",
    },
    PartSpec {
        key: "peer",
        title: "Peer reached",
    },
    PartSpec {
        key: "version",
        title: "Negotiated version",
    },
    PartSpec {
        key: "batch",
        title: "Negotiated batch",
    },
    PartSpec {
        key: "resolution",
        title: "Negotiated resolution",
    },
    PartSpec {
        key: "encryption",
        title: "Encryption",
    },
    PartSpec {
        key: "timeline",
        title: "Handshake timeline",
    },
    PartSpec {
        key: "channels",
        title: "Channels and last sequence",
    },
    PartSpec {
        key: "topology",
        title: "Show in topology",
    },
    PartSpec {
        key: "close",
        title: "Close session",
    },
];

/// The requirement that books the destructive action this section draws and
/// refuses.
pub const CLOSE_RESERVED_FOR: &str = "requirement 24";

/// The session this table names, or `None`.
#[must_use]
pub fn session(id: &str) -> Option<&'static SessionSpec> {
    SESSIONS.iter().find(|s| s.id == id)
}

/// How many sessions are running, and how many are over.
///
/// ★ Both halves derived, which is what the header's sentence is built from.
#[must_use]
pub fn tally() -> (usize, usize) {
    let active = SESSIONS.iter().filter(|s| s.standing.is_active()).count();
    (active, SESSIONS.len() - active)
}

/// Where a column's cells are drawn, given the width the list has.
///
/// ★ The stretch is resolved HERE rather than at each reader: the reference's
/// grid gives one track the slack, and a screen where the heading and the cells
/// each computed that separately is a screen whose columns can disagree.
#[must_use]
pub fn column_rect(nth: usize, list: Rect) -> Rect {
    let fixed: u32 = COLUMNS
        .iter()
        .filter(|c| !c.stretches)
        .map(|c| c.width)
        .sum();
    let gaps = GAP * u32::try_from(COLUMNS.len().saturating_sub(1)).unwrap_or(0);
    let stretch_count = u32::try_from(COLUMNS.iter().filter(|c| c.stretches).count()).unwrap_or(1);
    let slack = list
        .w
        .saturating_sub(PAD * 2)
        .saturating_sub(fixed)
        .saturating_sub(gaps);
    let mut x = list.x + PAD;
    for (n, column) in COLUMNS.iter().enumerate() {
        let wide = if column.stretches {
            column.width.max(slack / stretch_count.max(1))
        } else {
            column.width
        };
        if n == nth {
            return Rect::new(x, list.y, wide, ROW_H);
        }
        x += wide + GAP;
    }
    Rect::new(x, list.y, 0, ROW_H)
}
