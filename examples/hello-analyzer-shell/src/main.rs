// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-analyzer-shell` — R1648 §5.21 §5.51 §2 #7 — the analysis-tool
//! **dashboard shell**, assembled as one application.
//!
//! ## Why an assembly is the deliverable
//!
//! `tools/analyzer_census.py` classifies every capability of this tool class
//! with one of five verdicts, and the largest bin is `app`: *the substrate is
//! here, the domain logic is the application's*. Twenty-six rows say that, and
//! until this file existed **nobody had ever assembled one**. A `have` verdict
//! is proven by a test that exercises the capability through the public API
//! (R1602); an `app` verdict is a claim about composition, and the only thing
//! that proves composition is a composite. This is that composite, and the
//! census now names it.
//!
//! It is deliberately ONE binary rather than twelve. The dashboard's own
//! capability list is a *shell* — an app bar, a rail, a board of cards, named
//! layout presets — and a shell is exactly the part that cannot be demonstrated
//! by a page of separate examples each showing one widget. The existing tree
//! had `hello-tile-dashboard` (the board), `hello-dock-presets` (named
//! layouts), `hello-stat-tiles` (a KPI tile), `hello-row-dissect` and
//! `hello-hex-dump` (the decode panes) and no place where they meet.
//!
//! ## What it assembles, against the capability list
//!
//! * **The app bar** — source selection, a capture toggle, a global search box
//!   and a theme toggle. Four writable slots, all on the wire.
//! * **The navigation rail** — the thin left strip, one entry per board
//!   section.
//! * **The board** — twelve cards on
//!   [`TileGrid`], which is where
//!   placement, drag-snapping and reflow already live.
//! * **The card** — [`Card`], new in R1648: a title, the header affordances it
//!   offers, and what its body is showing. A card that offers **both** tear-off
//!   and maximise is one value here; the toolkit splits those across two class
//!   hierarchies that cannot be combined.
//! * **Named layout presets** — a preset is a stored
//!   [`TileGrid`], which is serde, so
//!   saving one is a clone and applying one is a `Signal::set` — the shape
//!   `hello-dock-presets` established for dock topologies.
//! * **The transport** — live / paused / replaying, on the existing
//!   [`TransportClock`], with a scrub. Not a fourth clock.
//!
//! ## The state vocabulary is exercised, not described
//!
//! The twelve cards are seeded so that **every arm of [`CardState`] is on the
//! board at once**: a loading series, an empty alarm feed, a failed latency
//! collector, a denied injection console, and a report card whose link is
//! encrypted. That is the assembly's real finding — the capability list asks
//! for loading, empty, error, no-permission and encrypted states, and a shell
//! that has to paint five of those per card kind is where twelve widget kinds
//! becomes sixty hand-written explanations that disagree with each other about
//! whether an encrypted link deserves a retry button.
//!
//! Here each card publishes `state`, `detail` and the **derived**
//! [`Remedy`], and the shell paints the remedy from the derivation rather than
//! from the card kind. So `console` (denied) offers an action and `report`
//! (opaque) does not, and neither card decided that.
//!
//! ## It is operated by hand, and by the wire, through one set of handlers
//!
//! ```text
//! cargo run -p hello-analyzer-shell --release
//! ```
//!
//! Click a card to select it and drag it to rearrange the board; press the
//! affordances in its header; press the app bar's chips; press an actionable
//! remedy. The keyboard is [`KEYMAP`] — arrows move the selection, `Shift` and
//! `Alt` move and resize the selected card, `Enter` maximises, `Escape`
//! restores, `/` types into the global search — and the window prints a
//! condensed version of it along the bottom, so it teaches itself.
//!
//! The part worth stating: **a real press and a scripted one reach the same
//! code**. The framework's router calls `pointer_move` and sends `PointerDown`
//! / `PointerUp` through `invoke("send", …)`; the wire's `point` moves the same
//! cursor and `key` takes the chord the platform spells. There is no parallel
//! automation surface that can drift from what a hand does.
//!
//! The geometry is likewise one thing. Every rectangle is computed by the
//! helpers above [`Hit::at`], and both the painter and the hit test read them,
//! because a surface that computes them twice ends up with a control drawn
//! where it cannot be pressed — the open
//! `debt-paint-and-gesture-read-two-facts` in this project. The demo sweeps the
//! window in both directions to keep it that way: every name the hit test can
//! produce must be a tag the scene painted, and every control the scene painted
//! must be pressable at the centre of the rectangle it was painted in. The
//! second direction is not decoration — it is what caught this file painting
//! every card's contents at twice their intended offset, because children of an
//! absolutely-positioned container are placed relative to *it*.
//!
//! ## The AI-first witness (§2 #7)
//!
//! Everything above is a read or a verb, so an agent drives the whole shell
//! with no pixel:
//!
//! ```text
//! scene/query  /external/cards                 -> the board, in order
//! scene/query  /external/state?...             -> per-card, via `state` (an action)
//! scene/invoke /external/act "console,settings"
//! scene/invoke /external/act "report,tear_off" -> REFUSED: not offered
//! scene/invoke /external/maximize "topology"
//! scene/query  /external/restore_to            -> the arrangement waiting
//! ```
//!
//! See `tools/demos/r1648_the_analyzer_shell_is_assembled.py`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Sparkline};
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, ThemeMode, ThemeProvider, use_theme};
use pinion_core::widgets::Widget;
use pinion_core::widgets::card::{Card, CardAffordance, CardChrome, CardState, Remedy};
use pinion_core::widgets::dock_panel::{DockPanelEvent, DockPanelPolicy, DockPanelState};
use pinion_core::widgets::tile_grid::{
    Maximized, Tile, TileDirection, TileGrid, TileId, TileNudge,
};
use pinion_core::widgets::transport::{TransportClock, TransportStatus, use_transport_clock};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloAnalyzerShellRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloAnalyzerShellRenderer, HelloAnalyzerShellRendererError);

const WIN_W: u32 = 1040;
const WIN_H: u32 = 566;

const VIEW_TAG: &str = "analyzer_shell";
const THEME_TAG: &str = "app";
const STATE_KEY: &str = "hello-analyzer-shell/state";
const TRANSPORT_KEY: &str = "hello-analyzer-shell/transport";

/// The replay window a scrub moves through, in seconds.
const REPLAY_SECS: f32 = 12.0;

const APP_BAR_H: u32 = 40;
const RAIL_W: u32 = 52;
const STATUS_H: u32 = 26;
const ROW_H: u32 = 96;
const COLUMNS: u32 = 12;
/// The gap between a card's cell and its painted box, on every side.
const CARD_PAD: u32 = 4;
const HEADER_H: u32 = 20;

const TITLE_FONT_PX: u32 = 13;
const BODY_FONT_PX: u32 = 11;

/// The sources the app bar offers. The first is the one a session opens on.
const SOURCES: [&str; 3] = ["live-capture", "session-2.pcapng", "lab-replay"];

/// The rail's entries — the sections the board is grouped into.
const RAIL: [&str; 4] = ["capture", "topology", "metrics", "operate"];

/// One card the shell assembles: its identity, its title, the header it
/// offers, where it sits on the board, and what its body starts out showing.
///
/// A `const` table rather than a builder chain, so the *whole* shell is one
/// value a reader can check against the capability list's twelve widget kinds —
/// and so that the seeded states below can be read off as a set.
struct Seed {
    id: &'static str,
    title: &'static str,
    /// Which rail section the card belongs to.
    section: &'static str,
    chrome: &'static [CardAffordance],
    cell: (u32, u32, u32, u32),
    state: CardState,
}

use CardAffordance::{Close, Maximize, Settings, TearOff};

/// The twelve widget kinds the capability list names, seeded so that every arm
/// of [`CardState`] is on the board at once.
///
/// The states are not decoration. A shell whose cards are all `Ready` never
/// exercises the half of this design that matters, and the failure it hides is
/// the one the capability list warns about: an encrypted link and a permission
/// denial look identical on screen and are opposite in what the person can do.
static SEEDS: &[Seed] = &[
    Seed {
        id: "stream",
        title: "Packet stream",
        section: "capture",
        chrome: &[Settings, TearOff, Maximize, Close],
        cell: (0, 0, 5, 2),
        state: CardState::Ready,
    },
    Seed {
        id: "inspector",
        title: "Decode inspector",
        section: "capture",
        chrome: &[Settings, TearOff, Maximize],
        cell: (5, 0, 4, 2),
        state: CardState::Ready,
    },
    Seed {
        id: "topology",
        title: "Topology map",
        section: "topology",
        chrome: &[Settings, TearOff, Maximize, Close],
        cell: (9, 0, 3, 2),
        state: CardState::Ready,
    },
    Seed {
        id: "throughput",
        title: "Throughput",
        section: "metrics",
        chrome: &[Settings, Maximize, Close],
        cell: (0, 2, 4, 1),
        state: CardState::Loading,
    },
    Seed {
        id: "share",
        title: "Share by endpoint",
        section: "metrics",
        chrome: &[Settings, Maximize, Close],
        cell: (4, 2, 4, 1),
        state: CardState::Ready,
    },
    Seed {
        id: "latency",
        title: "Latency distribution",
        section: "metrics",
        chrome: &[Settings, Maximize, Close],
        cell: (8, 2, 4, 1),
        state: CardState::Failed(std::borrow::Cow::Borrowed("collector unreachable")),
    },
    Seed {
        id: "loss",
        title: "Loss timeline",
        section: "metrics",
        chrome: &[Settings, Maximize, Close],
        cell: (0, 3, 4, 1),
        state: CardState::Empty,
    },
    Seed {
        id: "kpi",
        title: "KPI",
        section: "metrics",
        chrome: &[Settings, Close],
        cell: (4, 3, 4, 1),
        state: CardState::Ready,
    },
    Seed {
        id: "alarms",
        title: "Alarm feed",
        section: "operate",
        chrome: &[Settings, TearOff, Close],
        cell: (8, 3, 4, 1),
        state: CardState::Empty,
    },
    Seed {
        id: "search",
        title: "Search and filter",
        section: "capture",
        // No settings: this card IS the configuration surface, so a settings
        // affordance on it would open a panel for a panel. It is also the one
        // card on the board that withholds `Settings`, which is what gives the
        // wire's refusal path a case to demonstrate — a board where every card
        // offers an affordance can never show it being refused.
        chrome: &[Close],
        cell: (0, 4, 4, 1),
        state: CardState::Ready,
    },
    Seed {
        id: "console",
        title: "Replay and injection",
        section: "operate",
        chrome: &[Settings, Maximize, Close],
        cell: (4, 4, 4, 1),
        state: CardState::Denied(std::borrow::Cow::Borrowed("operator role")),
    },
    Seed {
        id: "report",
        title: "Report export",
        section: "operate",
        // No tear-off: an export card whose link cannot be read has nothing to
        // show in a window of its own.
        chrome: &[Settings, Close],
        cell: (8, 4, 4, 1),
        state: CardState::Opaque,
    },
];

/// The chords this shell claims, and what each does.
///
/// A table rather than prose, because it is **published** (`scene/query
/// /external/keymap`) as well as painted: a person reads the strip at the
/// bottom of the window and an agent reads the same list, so neither has to
/// open this file to find out what the keyboard does. Both then send the same
/// chord to the same handler.
const KEYMAP: [(&str, &str); 11] = [
    ("/", "type into the global search; Enter or Escape leaves"),
    ("Arrow", "move the selection to the neighbouring card"),
    ("Shift+Arrow", "move the selected card one cell"),
    ("Alt+Arrow", "grow that side of the card"),
    ("Alt+Shift+Arrow", "shrink that side"),
    ("Enter", "maximise the selection, or restore"),
    ("Escape", "restore a maximised board"),
    ("Delete", "close the selected card"),
    ("o", "tear the selected card off, or dock it back"),
    ("r", "act on the selected card's remedy"),
    ("c / t / s", "capture / theme / source"),
];

/// The keymap, condensed for the strip along the bottom of the window.
///
/// Short enough to fit and long enough to get someone started; the full table
/// is [`KEYMAP`], published on the wire. Written out rather than derived from
/// it because the two audiences want different densities, and a strip that
/// listed all eleven rows would be unreadable at 11px.
const HELP_STRIP: &str = "click a card, drag to move · arrows select · Shift/Alt+arrow move/resize · \
     Enter maximise · Esc restore · Del close · o tear off · r remedy · / search · c/t/s bar";

/// One hit, named, for the wire and for the status strip.
///
/// A word rather than a JSON object: what a caller needs before pressing is
/// *what is there*, and a name is what both a person and an agent compare.
///
/// ★ Every name here is the **scene tag** of the thing that was hit, not a
/// description of it. R1614's lesson — a name that has to survive is an address
/// and not a quotation — and the demo enforces it by sweeping the window and
/// requiring every name this returns to be a tag the paint actually emitted.
/// The first draft spelled the rail's hits `rail.<name>` while the painter
/// tagged them `shell.rail.<name>`, and the sweep found it on its first probe.
fn hit_word(hit: &Hit) -> String {
    match hit {
        Hit::Chip(chip) => chip.tag().to_string(),
        Hit::Rail(name) => format!("shell.rail.{name}"),
        Hit::Affordance(id, affordance) => format!("card.{id}.{}", affordance.wire()),
        Hit::Remedy(id) => format!("card.{id}.remedy"),
        Hit::Card(id) => format!("card.{id}"),
        Hit::Nothing => "nothing".to_string(),
    }
}

/// The sparkline the KPI card draws — the capability list's own example of a
/// widget that is a box, a label and a chart primitive rather than a widget the
/// framework ships.
const KPI_SERIES: [f64; 12] = [
    4.0, 6.0, 5.0, 9.0, 7.0, 12.0, 10.0, 14.0, 11.0, 15.0, 13.0, 17.0,
];

/// Which rail section a card belongs to.
///
/// One map, read by the rail (which counts its section's cards) and by the
/// wire (which publishes a card's section). Two lookups over one table rather
/// than a table each, because a card's section is one fact (R1631).
fn section_of(id: &str) -> &'static str {
    SEEDS.iter().find(|s| s.id == id).map_or("", |s| s.section)
}

/// How many of a section's cards are still on the board.
fn section_count(state: &ShellState, section: &str) -> usize {
    state
        .cards
        .get()
        .iter()
        .filter(|c| section_of(c.id().as_str()) == section)
        .count()
}

// --- State --------------------------------------------------------------------

/// Everything the shell holds. The board and the cards are separate signals
/// because they answer separate questions — *where* a card is and *what* it is
/// showing — and a maximise changes only the first.
struct ShellState {
    /// The replay clock, resolved once inside the owner scope and held.
    ///
    /// `use_transport_clock` panics outside an owner scope, and an External's
    /// `query` / `invoke` run outside one — so resolving it per call would put
    /// a panic on the wire. Holding the `Rc` is also what makes "live" a
    /// *derived* word rather than a fourth clock state (see `transport_word`).
    clock: Rc<TransportClock>,
    /// The theme provider, held for the same reason as the clock: the app
    /// bar's theme toggle is a wire slot, and `use_theme` panics off the owner
    /// scope an External's `query` runs on.
    theme: Rc<ThemeProvider>,
    board: Signal<TileGrid>,
    cards: Signal<Vec<Card>>,
    /// `Some` exactly while a card is maximised. The token IS the way home:
    /// there is no second copy of the arrangement anywhere in this file, which
    /// is the property [`Maximized`] exists to make checkable.
    maximized: Signal<Option<Maximized>>,
    /// The dock lifecycle of each card that has ever been torn off. The
    /// statechart is the authority on floating; `floating` below is a
    /// projection of it for the wire and for repaint.
    dock: RefCell<BTreeMap<String, Widget<DockPanelPolicy>>>,
    floating: Signal<String>,
    presets: RefCell<BTreeMap<String, TileGrid>>,
    preset: Signal<String>,
    /// The card the keyboard acts on. One roving selection rather than twelve
    /// tab stops — the pattern `hello-tile-dashboard` established in R1609,
    /// and the reason a pointer press also sets it: the two entry points must
    /// share one current card or they drift.
    selected: Signal<Option<String>>,
    /// Which rail section is highlighted. A view filter would be a second
    /// board, so the rail only *marks* a section; the cards stay put.
    rail_focus: Signal<Option<String>>,
    /// Where the cursor is, in view pixels, and what the press latched.
    cursor: Signal<(u32, u32)>,
    pressed: RefCell<Option<Hit>>,
    /// A press on a card body opens a board drag: the card, and where inside
    /// it the grab happened, so the card stays under the finger rather than
    /// teleporting its corner to the cursor.
    grab: RefCell<Option<(TileId, u32, u32)>>,
    source: Signal<String>,
    capturing: Signal<bool>,
    search: Signal<String>,
    /// Whether keystrokes are going into the global search box.
    ///
    /// A mode, and modes are usually a smell — this one earns its place
    /// because the alternative is giving up the single-letter shortcuts, and a
    /// dashboard that needs a modifier to toggle capture is worse than one that
    /// needs a slash to search. It is also the convention every tool with a
    /// filter bar already uses, so it costs a person nothing to learn.
    searching: Signal<bool>,
    last_event: Signal<String>,
}

impl ShellState {
    fn new(clock: Rc<TransportClock>, theme: Rc<ThemeProvider>) -> Self {
        let mut board = TileGrid::new(COLUMNS);
        for spec in SEEDS {
            let (col, row, w, h) = spec.cell;
            board
                .place(Tile::new(spec.id, col, row, w, h))
                .expect("the seeded board is a legal arrangement");
        }
        let cards = SEEDS
            .iter()
            .map(|spec| {
                Card::new(spec.id, spec.title)
                    .with_chrome(CardChrome::of(spec.chrome.iter().copied()))
                    .with_state(spec.state.clone())
            })
            .collect();
        let mut presets = BTreeMap::new();
        presets.insert("default".to_string(), board.clone());
        Self {
            clock,
            theme,
            board: Signal::new(board),
            cards: Signal::new(cards),
            maximized: Signal::new(None),
            dock: RefCell::new(BTreeMap::new()),
            floating: Signal::new(String::new()),
            presets: RefCell::new(presets),
            preset: Signal::new("default".to_string()),
            selected: Signal::new(None),
            rail_focus: Signal::new(None),
            cursor: Signal::new((0, 0)),
            pressed: RefCell::new(None),
            grab: RefCell::new(None),
            source: Signal::new(SOURCES[0].to_string()),
            capturing: Signal::new(true),
            search: Signal::new(String::new()),
            searching: Signal::new(false),
            last_event: Signal::new("assembled".to_string()),
        }
    }

    fn card(&self, id: &str) -> Option<Card> {
        self.cards.get().into_iter().find(|c| c.id().as_str() == id)
    }

    /// Replace one card, leaving the rest and the board alone.
    fn update_card(&self, id: &str, edit: impl FnOnce(&mut Card)) -> bool {
        let mut cards = self.cards.get();
        let Some(found) = cards.iter_mut().find(|c| c.id().as_str() == id) else {
            return false;
        };
        edit(found);
        self.cards.set(cards);
        true
    }

    fn card_ids(&self) -> String {
        self.cards
            .get()
            .iter()
            .map(|c| c.id().as_str().to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Whether a card is currently torn off, per its statechart.
    fn is_floating(&self, id: &str) -> bool {
        self.dock
            .borrow()
            .get(id)
            .is_some_and(|w| !matches!(w.state(), DockPanelState::Docked))
    }

    /// Send a dock event to one card's lifecycle and re-project `floating`.
    ///
    /// The projection is recomputed from the statecharts rather than edited
    /// alongside them: two writers on one fact is how a "floating" list and the
    /// panels that are actually floating drift apart.
    fn dock_send(&self, id: &str, event: DockPanelEvent) {
        {
            let mut dock = self.dock.borrow_mut();
            dock.entry(id.to_string()).or_default().send(event);
        }
        let mut floating: Vec<String> = self
            .dock
            .borrow()
            .iter()
            .filter(|(_, w)| !matches!(w.state(), DockPanelState::Docked))
            .map(|(id, _)| id.clone())
            .collect();
        floating.sort();
        self.floating.set(floating.join(","));
    }

    fn preset_names(&self) -> String {
        self.presets
            .borrow()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn use_shell_state() -> Rc<ShellState> {
    // Resolved BEFORE the cache call rather than inside the factory: the clock
    // registers itself with the §5.28 animation driver through the same owner,
    // and doing that while the cache is mid-insert is a re-entrancy this file
    // has no reason to rely on.
    let clock = use_transport_clock(TRANSPORT_KEY, REPLAY_SECS);
    let theme = use_theme(THEME_TAG);
    Owner::current()
        .expect("use_shell_state requires an active Owner scope")
        .cache(STATE_KEY, move || ShellState::new(clock, theme))
}

// --- The oracle (primary External) --------------------------------------------

/// Publishes the shell — the app bar's four slots, the rail, the board, and
/// every card's header and body state — and owns the verbs that change them.
struct ShellOracle {
    state: Option<Rc<ShellState>>,
}

impl core::fmt::Debug for ShellOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ShellOracle")
            .field("attached", &self.state.is_some())
            .finish()
    }
}

impl ShellOracle {
    /// R1564 §5.15 — the one sentence for "not wired to a model yet".
    const NO_STATE: &str = "this shell surface is not bound to a model yet";

    const fn new() -> Self {
        Self { state: None }
    }

    fn attach_state(&mut self, state: Rc<ShellState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<ShellState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.clone()),
            other => Err(InvokeError::rejected(format!(
                "expected a string argument, got {other:?}"
            ))),
        }
    }

    /// The card an argument names, or a refusal that says the id is unknown —
    /// distinct from a malformed argument, because "you spelled a card that is
    /// not on this board" is a different fact from "that is not a card id".
    fn card_of(state: &ShellState, id: &str) -> Result<Card, InvokeError> {
        state.card(id).ok_or_else(|| {
            InvokeError::rejected(format!(
                "no card {id:?} on this board; it holds {}",
                state.card_ids()
            ))
        })
    }

    /// The six reads that take a card id, answered together.
    ///
    /// `None` when `path` is not one of them, so the dispatcher below stays a
    /// list of verbs. They are grouped because they share one argument shape
    /// and one refusal — a card id that is not on this board — and a reader
    /// checking that refusal should find it once.
    fn card_read(
        state: &Rc<ShellState>,
        path: &str,
        args: &IntrospectValue,
    ) -> Option<Result<IntrospectValue, InvokeError>> {
        let wanted = matches!(
            path,
            "title" | "chrome" | "section" | "state" | "detail" | "remedy" | "actionable"
        );
        if !wanted {
            return None;
        }
        let card = match Self::text(args).and_then(|id| Self::card_of(state, id.trim())) {
            Ok(card) => card,
            Err(why) => return Some(Err(why)),
        };
        let answer = match path {
            "title" => card.title().to_string(),
            "chrome" => offered_words(&card),
            "section" => section_of(card.id().as_str()).to_string(),
            "state" => card.state().wire().to_string(),
            // `""` for the four arms that carry nothing — the absence is the
            // answer, and it differs from a carried reason that happens to be
            // empty only in that the arms which carry one always have one.
            "detail" => card.state().detail().unwrap_or("").to_string(),
            "remedy" => remedy_word(card.remedy()).to_string(),
            _ => if card.remedy().is_some_and(Remedy::is_actionable) {
                "yes"
            } else {
                "no"
            }
            .to_string(),
        };
        Some(Ok(IntrospectValue::Text(answer)))
    }

    /// `act` — perform one header affordance on one card.
    ///
    /// The whole point of the header being a *set* rather than four booleans a
    /// painter reads: an affordance the card does not offer is refused **by
    /// name**, before anything happens, and the refusal says what the card does
    /// offer. A shell that only hides the button leaves the wire able to do
    /// what the screen says is impossible.
    fn act(state: &Rc<ShellState>, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let (id, word) = raw
            .split_once(',')
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<affordance>")))?;
        let (id, word) = (id.trim(), word.trim());
        let card = Self::card_of(state, id)?;
        let affordance = CardAffordance::from_wire(word).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{word:?} is not an affordance; they are {}",
                CardAffordance::ALL.map(CardAffordance::wire).join(" / ")
            ))
        })?;
        if !card.chrome().offers(affordance) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} does not offer {word}; its header offers {}",
                offered_words(&card)
            )));
        }
        match affordance {
            CardAffordance::Settings => {
                state.last_event.set(format!("settings opened for {id}"));
            }
            CardAffordance::TearOff => {
                // Not a second float model: the existing dock lifecycle is what
                // says whether a panel is floating, and this sends it the event
                // it already understands.
                state.dock_send(id, DockPanelEvent::Escaped);
                state.last_event.set(format!("{id} torn off"));
            }
            CardAffordance::Maximize => return Self::maximize(state, id),
            CardAffordance::Close => {
                let mut board = state.board.get();
                board.remove(&TileId::new(id)).ok();
                state.board.set(board);
                let cards = state
                    .cards
                    .get()
                    .into_iter()
                    .filter(|c| c.id().as_str() != id)
                    .collect();
                state.cards.set(cards);
                state.last_event.set(format!("{id} closed"));
            }
        }
        Ok(IntrospectValue::Text(format!("{id} {word}")))
    }

    /// Fill the board with one card, keeping the way home in the one place it
    /// lives.
    fn maximize(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if state.maximized.get().is_some() {
            return Err(InvokeError::rejected(
                "a card is already maximised; restore first",
            ));
        }
        let mut board = state.board.get();
        let token = board
            .maximize(&TileId::new(id))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.maximized.set(Some(token));
        state.last_event.set(format!("{id} maximised"));
        Ok(IntrospectValue::Text(format!("{id} maximize")))
    }

    /// Put the board back the way it was before the maximise.
    fn restore(state: &Rc<ShellState>) -> Result<IntrospectValue, InvokeError> {
        let token = state
            .maximized
            .get()
            .ok_or_else(|| InvokeError::rejected("no card is maximised"))?;
        let id = token.id().as_str().to_string();
        state.board.set(token.restore());
        state.maximized.set(None);
        state.last_event.set(format!("{id} restored"));
        Ok(IntrospectValue::Text(id))
    }

    /// Drive one card's body state — the input a real collector would provide.
    ///
    /// `<card>,<state>[,<detail>]`. The detail segment is required by exactly
    /// the two arms that carry one and refused on the four that do not: an
    /// explanation attached to `empty` would be an explanation nothing reads,
    /// and a `failed` without one is a failure whose reason was lost.
    fn set_state(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let mut parts = raw.splitn(3, ',');
        let id = parts.next().unwrap_or_default().trim();
        let word = parts
            .next()
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<state>[,<why>]")))?
            .trim();
        let detail = parts.next().map(str::trim).filter(|d| !d.is_empty());
        Self::card_of(state, id)?;
        let next = parse_state(word, detail).map_err(InvokeError::rejected)?;
        let remedy = next.remedy();
        state.update_card(id, |card| card.set_state(next.clone()));
        state.last_event.set(format!("{id} is {}", next.wire()));
        Ok(IntrospectValue::Text(format!(
            "{id} {} {}",
            next.wire(),
            remedy_word(remedy)
        )))
    }
}

/// The wire word for a card's remedy, with the absence spelled out.
///
/// `"none"` is the answer for a ready card and it is NOT one of
/// [`Remedy::ALL`] — a reader that treats every answer here as a remedy would
/// otherwise invent a seventh one.
fn remedy_word(remedy: Option<Remedy>) -> &'static str {
    remedy.map_or("none", Remedy::wire)
}

fn offered_words(card: &Card) -> String {
    card.chrome()
        .offered()
        .into_iter()
        .map(CardAffordance::wire)
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse `<state>` plus its optional detail into a [`CardState`].
///
/// The arity check is here rather than at each call site because it is a fact
/// about the vocabulary: two arms take a reason and four do not.
fn parse_state(word: &str, detail: Option<&str>) -> Result<CardState, String> {
    let carried = |what: &str| -> Result<std::borrow::Cow<'static, str>, String> {
        detail
            .map(|d| std::borrow::Cow::Owned(d.to_string()))
            .ok_or_else(|| format!("{what} carries a reason: send \"<card>,{what},<why>\""))
    };
    let plain = |made: CardState| -> Result<CardState, String> {
        if detail.is_some() {
            return Err(format!(
                "{} carries no reason; the same explanation every time is not one",
                made.wire()
            ));
        }
        Ok(made)
    };
    match word {
        "ready" => plain(CardState::Ready),
        "loading" => plain(CardState::Loading),
        "empty" => plain(CardState::Empty),
        "opaque" => plain(CardState::Opaque),
        "failed" => Ok(CardState::Failed(carried("failed")?)),
        "denied" => Ok(CardState::Denied(carried("denied")?)),
        other => Err(format!(
            "{other:?} is not a card state; they are {}",
            CardState::ALL
                .iter()
                .map(CardState::wire)
                .collect::<Vec<_>>()
                .join(" / ")
        )),
    }
}

/// Everything this surface publishes.
///
/// Lifted out of `schema()` so the function is its name and this is its
/// content: a `const` table is a value, and reading it should not mean
/// scrolling past a hundred lines to find where the method ends.
const FIELDS: &[SchemaField] = const {
    &[
        // --- the app bar: four writable slots -----------------------
        SchemaField::new("source", "string"),
        SchemaField::new("sources", "string"),
        SchemaField::new("capturing", "bool"),
        SchemaField::new("search", "string"),
        SchemaField::new("theme", "string"),
        // --- the rail and the board ---------------------------------
        SchemaField::new("rail", "string"),
        SchemaField::new("cards", "string"),
        SchemaField::new("card_count", "int"),
        SchemaField::new("layout", "string"),
        SchemaField::new("maximized", "string"),
        SchemaField::new("restore_to", "string"),
        SchemaField::new("floating", "string"),
        // --- named layout presets -----------------------------------
        SchemaField::new("preset", "string"),
        SchemaField::new("presets", "string"),
        // --- the transport ------------------------------------------
        SchemaField::new("transport", "string"),
        SchemaField::new("playhead", "int"),
        // --- the published vocabularies -----------------------------
        // Published so a client can enumerate rather than hard-code
        // them, and so `act` / `set_state` accept exactly what is
        // advertised (R1616).
        SchemaField::new("affordances", "string"),
        SchemaField::new("states", "string"),
        SchemaField::new("remedies", "string"),
        SchemaField::new("last_event", "string"),
        // --- per-card reads, each taking the card's id --------------
        SchemaField::action_with(
            "title",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "chrome",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "state",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "detail",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "section",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "remedy",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "actionable",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        // --- the verbs ----------------------------------------------
        SchemaField::action_with(
            "act",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("affordance", "string", "affordances"),
                ]
            },
        ),
        SchemaField::action_with(
            "set_state",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("state", "string", "states"),
                    SchemaArg::key("why", "string", "states").optional(),
                ]
            },
        ),
        SchemaField::action("maximize", "string"),
        SchemaField::action("dock_back", "string"),
        // --- the direct-manipulation surface ------------------------
        // The pointer and the keyboard the person uses ARE these, so
        // an agent drives the same handlers rather than a parallel set
        // that can disagree with what a hand does.
        SchemaField::action_with(
            "point",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("x", "int", "cursor"),
                    SchemaArg::key("y", "int", "cursor"),
                ]
            },
        ),
        SchemaField::action("send", "string"),
        SchemaField::action("key", "string"),
        SchemaField::new("cursor", "string"),
        SchemaField::new("selected", "string"),
        SchemaField::new("rail_focus", "string"),
        SchemaField::new("hit", "string"),
        SchemaField::new("keymap", "string"),
        SchemaField::action("restore", "string"),
        SchemaField::action("save_preset", "string"),
        SchemaField::action("seek", "string"),
    ]
};

impl ExternalIntrospect for ShellOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let text = |s: String| Some(IntrospectValue::Text(s));
        let clock = &state.clock;
        match path {
            "source" => text(state.source.get()),
            "sources" => text(SOURCES.join(",")),
            "capturing" => Some(IntrospectValue::Bool(state.capturing.get())),
            "search" => text(state.search.get()),
            "theme" => text(theme_word(&state.theme)),
            // Each entry with the number of its cards still on the board, so
            // the rail's own claim is checkable without a pixel.
            "rail" => text(
                RAIL.iter()
                    .map(|name| format!("{name}:{}", section_count(state, name)))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "cards" => text(state.card_ids()),
            "card_count" => Some(IntrospectValue::Int(
                i64::try_from(state.cards.get().len()).unwrap_or(i64::MAX),
            )),
            "layout" => text(
                serde_json::to_string(&state.board.get()).unwrap_or_else(|why| why.to_string()),
            ),
            "maximized" => text(
                state
                    .maximized
                    .get()
                    .map_or_else(String::new, |m| m.id().as_str().to_string()),
            ),
            // The arrangement the way home holds, so a client can SEE what a
            // restore will do without doing it. Empty when nothing is
            // maximised, which is a different answer from an empty board.
            "restore_to" => text(state.maximized.get().map_or_else(String::new, |m| {
                serde_json::to_string(m.peek()).unwrap_or_else(|why| why.to_string())
            })),
            "floating" => text(state.floating.get()),
            "preset" => text(state.preset.get()),
            "presets" => text(state.preset_names()),
            "transport" => text(transport_word(clock.status(), state.capturing.get())),
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a playhead fraction is 0.0..=1.0, so per-mille is 0..=1000"
            )]
            "playhead" => Some(IntrospectValue::Int(i64::from(
                (clock.position() * 1000.0).round() as i32,
            ))),
            "cursor" => {
                let (x, y) = state.cursor.get();
                text(format!("{x},{y}"))
            }
            "selected" => text(state.selected.get().unwrap_or_default()),
            "rail_focus" => text(state.rail_focus.get().unwrap_or_default()),
            // What is under the cursor RIGHT NOW, named. The one read that
            // makes the hit test checkable without pixels — and the thing an
            // agent needs before it presses, for the same reason a person
            // needs to see the button before clicking it.
            "hit" => {
                let (x, y) = state.cursor.get();
                text(hit_word(&Hit::at(state, x, y)))
            }
            // The keymap, published rather than only documented: a chord a
            // person can press is a chord an agent can send, and neither
            // should have to read this file to find out which.
            "keymap" => text(
                KEYMAP
                    .iter()
                    .map(|(chord, what)| format!("{chord}={what}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "affordances" => text(CardAffordance::ALL.map(CardAffordance::wire).join(",")),
            "states" => text(
                CardState::ALL
                    .iter()
                    .map(CardState::wire)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "remedies" => text(Remedy::ALL.map(Remedy::wire).join(",")),
            "last_event" => text(state.last_event.get()),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self
            .state
            .as_ref()
            .ok_or(InterveneError::UnknownPath)?
            .clone();
        match path {
            "source" => {
                let IntrospectValue::Text(name) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let chosen = SOURCES.iter().find(|s| **s == name.trim()).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a source; they are {}",
                        SOURCES.join(", ")
                    ))
                })?;
                state.source.set((*chosen).to_string());
                state.last_event.set(format!("source {chosen}"));
                Ok(())
            }
            "capturing" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.capturing.set(on);
                state
                    .last_event
                    .set(format!("capture {}", if on { "on" } else { "off" }));
                Ok(())
            }
            "search" => {
                let IntrospectValue::Text(needle) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.search.set(needle.clone());
                state.last_event.set(format!("search {needle:?}"));
                Ok(())
            }
            "theme" => {
                let IntrospectValue::Text(word) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let mode = match word.trim() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    other => {
                        return Err(InterveneError::out_of_range(format!(
                            "{other:?} is not a theme; they are light / dark / system"
                        )));
                    }
                };
                state.theme.set_mode(mode);
                state.last_event.set(format!("theme {}", word.trim()));
                Ok(())
            }
            "preset" => {
                let IntrospectValue::Text(name) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let stored = state.presets.borrow().get(name.trim()).cloned();
                let board = stored.ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a saved layout; they are {}",
                        state.preset_names()
                    ))
                })?;
                // Applying a preset while maximised would leave a token whose
                // way home is an arrangement nobody is on any more.
                state.maximized.set(None);
                state.board.set(board);
                state.preset.set(name.trim().to_string());
                state.last_event.set(format!("preset {}", name.trim()));
                Ok(())
            }
            "sources" | "cards" | "card_count" | "layout" | "maximized" | "restore_to"
            | "floating" | "presets" | "transport" | "playhead" | "affordances" | "states"
            | "remedies" | "last_event" | "rail" | "cursor" | "selected" | "rail_focus" | "hit"
            | "keymap" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        if let Some(answer) = Self::card_read(&state, path, &args) {
            return answer;
        }
        match path {
            "act" => Self::act(&state, &args),
            "set_state" => Self::set_state(&state, &args),
            "maximize" => Self::maximize(&state, Self::text(&args)?.trim()),
            "restore" => Self::restore(&state),
            // The other half of tear-off. A lifecycle with no way back is half
            // a feature, and the event is the dock chart's own `dock_back`
            // rather than a flag this file clears.
            "dock_back" => {
                let id = Self::text(&args)?.trim().to_string();
                Self::card_of(&state, &id)?;
                if !state.is_floating(&id) {
                    return Err(InvokeError::rejected(format!(
                        "card {id:?} is not torn off"
                    )));
                }
                state.dock_send(&id, DockPanelEvent::DockBack);
                state.last_event.set(format!("{id} docked back"));
                Ok(IntrospectValue::Text(state.floating.get()))
            }
            "save_preset" => {
                let name = Self::text(&args)?.trim().to_string();
                if name.is_empty() {
                    return Err(InvokeError::rejected("a layout preset needs a name"));
                }
                state
                    .presets
                    .borrow_mut()
                    .insert(name.clone(), state.board.get());
                state.preset.set(name.clone());
                state.last_event.set(format!("saved preset {name}"));
                Ok(IntrospectValue::Text(state.preset_names()))
            }
            // Put the cursor somewhere, in view pixels. The framework's own
            // pointer path calls the same `pointer_move`, so a scripted press
            // and a real one land on one code path.
            "point" => {
                let raw = Self::text(&args)?;
                let (x, y) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <x>,<y>")))?;
                let parse = |what: &str, s: &str| -> Result<u32, InvokeError> {
                    s.trim()
                        .parse::<u32>()
                        .map_err(|_| InvokeError::rejected(format!("{what} is a pixel, got {s:?}")))
                };
                let (x, y) = (parse("x", x)?, parse("y", y)?);
                if x >= WIN_W || y >= WIN_H {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {WIN_W}x{WIN_H} shell"
                    )));
                }
                state.cursor.set((x, y));
                Ok(IntrospectValue::Text(hit_word(&Hit::at(&state, x, y))))
            }
            // The symbolic pointer events the framework's router sends on a
            // real press and release.
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => Self::press(&state),
                    "PointerUp" => Self::release(&state),
                    // A cancel drops the latch WITHOUT performing it — the
                    // difference between letting go and being interrupted.
                    "PointerLeave" | "PointerCancel" => {
                        state.pressed.borrow_mut().take();
                        state.grab.borrow_mut().take();
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerLeave / PointerCancel"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.last_event.get()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(Self::key(&state, chord.trim())))
            }
            "seek" => {
                let raw = Self::text(&args)?;
                let per_mille: i32 = raw
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{raw:?} is not 0..=1000")))?;
                if !(0..=1000).contains(&per_mille) {
                    return Err(InvokeError::rejected(format!(
                        "a playhead is 0..=1000 per mille, got {per_mille}"
                    )));
                }
                let clock = &state.clock;
                clock.pause();
                clock.seek(f32::from(i16::try_from(per_mille).unwrap_or(0)) / 1000.0);
                state.capturing.set(false);
                state.last_event.set(format!("seek {per_mille}"));
                Ok(IntrospectValue::Int(i64::from(per_mille)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- Direct manipulation: the same verbs, driven by a hand -------------------

impl ShellOracle {
    /// A press: latch what is under the cursor, and open a board drag if it
    /// was a card body.
    ///
    /// Nothing is *performed* here. A control fires on release over the same
    /// target it was pressed on, which is what lets a person press a close
    /// button, think better of it, slide off and let go — the behaviour every
    /// desktop toolkit has and the reason a press-to-fire button feels wrong.
    fn press(state: &Rc<ShellState>) {
        let (px, py) = state.cursor.get();
        let hit = Hit::at(state, px, py);
        if let Some(id) = hit.card_id() {
            state.selected.set(Some(id.to_string()));
            state.rail_focus.set(Some(section_of(id).to_string()));
        }
        if let Hit::Card(id) = &hit {
            let board = state.board.get();
            let tile_id = TileId::new(id.clone());
            if let Some(tile) = board.tile(&tile_id) {
                let (col, row) = cell_at(&board, px, py);
                *state.grab.borrow_mut() = Some((
                    tile_id,
                    col.saturating_sub(tile.col),
                    row.saturating_sub(tile.row),
                ));
            }
        }
        *state.pressed.borrow_mut() = Some(hit);
    }

    /// A release: perform the latched control if the cursor is still on it.
    fn release(state: &Rc<ShellState>) {
        let latched = state.pressed.borrow_mut().take();
        let dragged = state.grab.borrow_mut().take().is_some();
        let (px, py) = state.cursor.get();
        let Some(latched) = latched else { return };
        // A drag that actually moved a card is not also a click on it: the
        // release lands wherever the card ended up, and firing "select" again
        // there is harmless, but firing an affordance would not be.
        if Hit::at(state, px, py) != latched {
            return;
        }
        match latched {
            Hit::Chip(chip) => Self::toggle_chip(state, chip),
            Hit::Rail(name) => {
                state.rail_focus.set(Some(name.to_string()));
                state.last_event.set(format!(
                    "rail {name} ({} cards)",
                    section_count(state, name)
                ));
            }
            Hit::Affordance(id, affordance) => {
                let outcome = Self::act(
                    state,
                    &IntrospectValue::Text(format!("{id},{}", affordance.wire())),
                );
                if let Err(why) = outcome {
                    // A refusal a person triggered has to be visible to that
                    // person, not only to the wire that would have read it.
                    state.last_event.set(format!("refused: {why:?}"));
                }
            }
            Hit::Remedy(id) => Self::apply_remedy(state, &id),
            Hit::Card(id) => {
                if !dragged {
                    state.last_event.set(format!("{id} selected"));
                }
            }
            Hit::Nothing => {}
        }
    }

    /// One app-bar chip, pressed.
    fn toggle_chip(state: &Rc<ShellState>, chip: BarChip) {
        match chip {
            BarChip::Source => {
                // A cycle rather than a menu: three sources and a 40px bar, and
                // the menu widget belongs to a round about menus.
                let now = state.source.get();
                let at = SOURCES.iter().position(|s| *s == now).unwrap_or(0);
                let next = SOURCES[(at + 1) % SOURCES.len()];
                state.source.set(next.to_string());
                state.last_event.set(format!("source {next}"));
            }
            BarChip::Capture => {
                let on = !state.capturing.get();
                state.capturing.set(on);
                state
                    .last_event
                    .set(format!("capture {}", if on { "on" } else { "off" }));
            }
            BarChip::Theme => {
                let dark = theme_word(&state.theme) != "dark";
                state.theme.set_mode(if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                });
                state
                    .last_event
                    .set(format!("theme {}", if dark { "dark" } else { "light" }));
            }
        }
    }

    /// What pressing an actionable remedy does.
    ///
    /// The framework decides *which* remedy a state has; what a remedy MEANS
    /// for this data is the application's, which is why this lives here and
    /// `Remedy` has no `perform`. Each one is the smallest honest response:
    /// a retry goes back to loading, widening clears the filter that excluded
    /// everything, and authorising is what a grant would look like arriving.
    fn apply_remedy(state: &Rc<ShellState>, id: &str) {
        let Some(card) = state.card(id) else { return };
        let Some(remedy) = card.remedy().filter(|r| r.is_actionable()) else {
            // Unreachable through the pointer — `Hit::at` only resolves a
            // remedy control on an actionable one — and stated rather than
            // assumed, because the wire can call this path too.
            state
                .last_event
                .set(format!("{id}: nothing to do about this"));
            return;
        };
        let next = match remedy {
            Remedy::Retry => CardState::Loading,
            Remedy::Widen => {
                state.search.set(String::new());
                CardState::Ready
            }
            Remedy::Authorize => CardState::Ready,
            Remedy::Wait | Remedy::Nothing => return,
        };
        state.update_card(id, |card| card.set_state(next.clone()));
        state
            .last_event
            .set(format!("{id}: {} -> {}", remedy.wire(), next.wire()));
    }

    /// The keymap, as one function so the wire and a real keyboard drive the
    /// same one rather than two that drift.
    ///
    /// Returns whether the chord meant something here, which is what a key
    /// handler owes its caller: an unclaimed chord must stay unclaimed.
    fn key(state: &Rc<ShellState>, chord: &str) -> bool {
        if state.searching.get() {
            return Self::search_key(state, chord);
        }
        if chord == "/" {
            state.searching.set(true);
            state
                .last_event
                .set("searching (Enter or Escape leaves)".to_string());
            return true;
        }
        let selected = state.selected.get();
        let (modifiers, base) = chord.rsplit_once('+').map_or(("", chord), |(m, b)| (m, b));
        let shift = modifiers.contains("Shift");
        let alt = modifiers.contains("Alt");
        let direction = match base {
            "ArrowLeft" => Some(TileDirection::Left),
            "ArrowRight" => Some(TileDirection::Right),
            "ArrowUp" => Some(TileDirection::Up),
            "ArrowDown" => Some(TileDirection::Down),
            _ => None,
        };
        if let Some(direction) = direction {
            return Self::arrow(state, direction, shift, alt);
        }
        match base {
            "Enter" => {
                let Some(id) = selected else { return false };
                if state.maximized.get().is_some() {
                    Self::restore(state).is_ok()
                } else {
                    Self::maximize(state, &id).is_ok()
                }
            }
            "Escape" => Self::restore(state).is_ok(),
            "Delete" | "Backspace" => selected.is_some_and(|id| {
                Self::act(state, &IntrospectValue::Text(format!("{id},close"))).is_ok()
            }),
            "o" | "O" => selected.is_some_and(|id| {
                if state.is_floating(&id) {
                    state.dock_send(&id, DockPanelEvent::DockBack);
                    state.last_event.set(format!("{id} docked back"));
                    true
                } else {
                    Self::act(state, &IntrospectValue::Text(format!("{id},tear_off"))).is_ok()
                }
            }),
            "r" | "R" => selected.is_some_and(|id| {
                Self::apply_remedy(state, &id);
                true
            }),
            "c" | "C" => {
                Self::toggle_chip(state, BarChip::Capture);
                true
            }
            "t" | "T" => {
                Self::toggle_chip(state, BarChip::Theme);
                true
            }
            "s" | "S" => {
                Self::toggle_chip(state, BarChip::Source);
                true
            }
            _ => false,
        }
    }

    /// Keystrokes while the search box has them.
    ///
    /// Deliberately narrow: text, backspace, and two ways out. Everything else
    /// is REFUSED rather than falling through to the board, because a chord
    /// that quietly did something else while the caret was in a text box is the
    /// worst thing a mode can do.
    fn search_key(state: &Rc<ShellState>, chord: &str) -> bool {
        match chord {
            "Enter" | "Escape" => {
                state.searching.set(false);
                state
                    .last_event
                    .set(format!("search {:?}", state.search.get()));
                true
            }
            "Backspace" => {
                let mut text = state.search.get();
                let had = text.pop().is_some();
                state.search.set(text);
                had
            }
            "Space" => {
                state.search.set(format!("{} ", state.search.get()));
                true
            }
            one if one.chars().count() == 1 && !one.chars().any(char::is_control) => {
                state.search.set(format!("{}{one}", state.search.get()));
                true
            }
            _ => false,
        }
    }

    /// The four arrows, and the three things a modifier makes them mean.
    ///
    /// Plain moves the selection; `Shift` moves the card; `Alt` resizes it —
    /// the same twelve chords `TileNudge` already spells, reused rather than
    /// re-invented, so a person who learned the board in one application knows
    /// it in this one.
    fn arrow(state: &Rc<ShellState>, direction: TileDirection, shift: bool, alt: bool) -> bool {
        let Some(id) = state.selected.get() else {
            // Nothing selected: the first arrow picks the first card, so the
            // keyboard has a way in that does not require a click.
            let first = state
                .cards
                .get()
                .first()
                .map(|c| c.id().as_str().to_string());
            let had = first.is_some();
            state.selected.set(first);
            return had;
        };
        let tile = TileId::new(id.clone());
        if !shift && !alt {
            let mut board = state.board.get();
            let next = board.neighbour(&tile, direction).map(|t| t.id.clone());
            let Some(next) = next else { return false };
            let _ = &mut board;
            state.selected.set(Some(next.as_str().to_string()));
            state
                .rail_focus
                .set(Some(section_of(next.as_str()).to_string()));
            state.last_event.set(format!("{next} selected"));
            return true;
        }
        let nudge = match (shift, alt) {
            (true, false) => TileNudge::Move(direction),
            (false, true) => TileNudge::Grow(direction),
            _ => TileNudge::Shrink(direction),
        };
        let mut board = state.board.get();
        match board.nudge(&tile, nudge) {
            Ok(reflow) => {
                state.board.set(board);
                state.last_event.set(if reflow.is_clean() {
                    format!("{id} {nudge:?}")
                } else {
                    format!(
                        "{id} {nudge:?}, displacing {}",
                        reflow
                            .displaced()
                            .iter()
                            .map(|d| d.id.as_str().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                });
                true
            }
            Err(why) => {
                state.last_event.set(format!("refused: {why}"));
                false
            }
        }
    }
}

/// The board cell a view pixel lands on.
///
/// The inverse of [`cell_rect`], and the only place the two directions meet —
/// so a drag snaps to the cell a card is drawn in rather than to one computed
/// from a second copy of the column arithmetic.
fn cell_at(board: &TileGrid, px: u32, py: u32) -> (u32, u32) {
    let usable = WIN_W.saturating_sub(RAIL_W);
    let col_w = (usable / board.columns().max(1)).max(1);
    let col = px.saturating_sub(RAIL_W) / col_w;
    let row = py.saturating_sub(APP_BAR_H) / ROW_H;
    (col.min(board.columns().saturating_sub(1)), row)
}

impl External for ShellOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Keep the cursor while a press is held, so a drag that strays off a card
    /// keeps moving it rather than being cancelled by a stray pixel.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Hover positions too: the shell tracks the cursor so a press knows where
    /// it landed, and the press event itself carries no coordinates.
    fn wants_hover_move(&self) -> bool {
        true
    }

    /// Track the cursor, and drag the grabbed card.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window fraction times a window size is a pixel inside it"
    )]
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let px = (x_rel.clamp(0.0, 1.0) * WIN_W as f32) as u32;
        let py = (y_rel.clamp(0.0, 1.0) * WIN_H as f32) as u32;
        state.cursor.set((px, py));
        let grab = state.grab.borrow().clone();
        let Some((id, dx, dy)) = grab else { return };
        let mut board = state.board.get();
        let (col, row) = cell_at(&board, px, py);
        let target = (col.saturating_sub(dx), row.saturating_sub(dy));
        if board.tile(&id).is_some_and(|t| (t.col, t.row) == target) {
            return;
        }
        if let Ok(reflow) = board.move_to(&id, target.0, target.1) {
            state.board.set(board);
            state.last_event.set(if reflow.is_clean() {
                format!("{id} moved to {},{}", target.0, target.1)
            } else {
                format!(
                    "{id} moved, displacing {}",
                    reflow
                        .displaced()
                        .iter()
                        .map(|d| d.id.as_str().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

/// Which theme the shell is showing, as the app bar's toggle reads it.
fn theme_word(theme: &ThemeProvider) -> String {
    match theme.mode() {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
        // `ThemeMode` is `#[non_exhaustive]`: a mode this shell has not been
        // taught is reported as itself rather than folded into one of the
        // three, which would make the app bar claim a setting nobody chose.
        other => return format!("{other:?}").to_lowercase(),
    }
    .to_string()
}

/// The three transport words the capability list names.
///
/// Derived from the clock and the capture toggle rather than stored: "live" is
/// not a fourth clock state, it is the absence of a replay while capture is on.
fn transport_word(status: TransportStatus, capturing: bool) -> String {
    match status {
        TransportStatus::Playing => "replaying",
        // Stopped while capturing is "live": nothing is replaying and packets
        // are arriving. Stopped while capture is off and Paused are the SAME
        // word on purpose — the board is frozen either way, and the capability
        // list names three states, not four.
        TransportStatus::Stopped if capturing => "live",
        TransportStatus::Paused | TransportStatus::Stopped => "paused",
    }
    .to_string()
}

// --- Geometry: ONE source, read by the paint and by the gesture ---------------
//
// [[debt-paint-and-gesture-read-two-facts]] is an open debt in this project
// because a surface whose painter and hit test compute their rectangles
// separately can drift into a control that is drawn where it cannot be clicked.
// Every rectangle below is therefore computed once here and used by BOTH
// `*_scene` and `Hit::at`. A change to a slot's width moves the paint and the
// gesture together or it does not compile.

/// The pixel rectangle of a board cell rectangle.
fn cell_rect(board: &TileGrid, tile: &Tile) -> Rect {
    let usable = WIN_W.saturating_sub(RAIL_W);
    let col_w = usable / board.columns().max(1);
    Rect::new(
        RAIL_W + tile.col * col_w + CARD_PAD,
        APP_BAR_H + tile.row * ROW_H + CARD_PAD,
        (tile.w * col_w).saturating_sub(CARD_PAD * 2).max(1),
        (tile.h * ROW_H).saturating_sub(CARD_PAD * 2).max(1),
    )
}

/// A container's own coordinate space: its size at the origin.
///
/// ★ Everything inside an absolutely-positioned container is placed RELATIVE TO
/// IT, so a child written in window coordinates lands at the parent's origin
/// plus its own — twice as far along both axes as intended. This shell's first
/// draft did exactly that and nothing noticed, because the demo compared tags
/// and text and never a rectangle. The counterfactual that broke the hit test's
/// geometry is what surfaced it: the hit test was computing window coordinates
/// correctly while the paint was not, so the two really were reading different
/// facts and neither the tests nor the eye had said so.
///
/// The fix is this function plus the rule it names: **the sub-rectangles below
/// are local**, the painter passes `local(rect)`, and the hit test subtracts
/// the container's origin before asking. One space, one set of helpers.
const fn local(rect: Rect) -> Rect {
    Rect::new(0, 0, rect.w, rect.h)
}

/// One card's header strip, in the card's own space.
const fn header_rect(card: Rect) -> Rect {
    Rect::new(card.x, card.y, card.w, HEADER_H)
}

/// One card's body, in the card's own space.
const fn body_rect(card: Rect) -> Rect {
    Rect::new(
        card.x,
        card.y + HEADER_H,
        card.w,
        card.h.saturating_sub(HEADER_H),
    )
}

/// How wide one header affordance's hit slot is.
const SLOT_W: u32 = 26;

/// Where the `n`th of `count` affordances sits in a header.
///
/// Right-aligned, in declaration order, so the rightmost slot is the last
/// affordance the vocabulary declares — `Close`, wherever a card offers it.
const fn affordance_rect(header: Rect, count: u32, n: u32) -> Rect {
    let from_right = count.saturating_sub(n);
    Rect::new(
        (header.x + header.w).saturating_sub(from_right * SLOT_W),
        header.y,
        SLOT_W,
        HEADER_H,
    )
}

/// Where a not-ready card's remedy control sits in its body.
const fn remedy_rect(body: Rect) -> Rect {
    Rect::new(body.x + 6, body.y + 24, body.w.saturating_sub(12), REMEDY_H)
}

const REMEDY_H: u32 = 20;

/// The app bar's clickable chips, left to right after the title.
///
/// Three, because three of the bar's four parts are toggles a pointer can
/// operate; the fourth (global search) is text, and typing goes through the
/// keyboard rather than a chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarChip {
    Source,
    Capture,
    Theme,
}

impl BarChip {
    const ALL: [Self; 3] = [Self::Source, Self::Capture, Self::Theme];

    const fn tag(self) -> &'static str {
        match self {
            Self::Source => "shell.appbar.source",
            Self::Capture => "shell.appbar.capture",
            Self::Theme => "shell.appbar.theme",
        }
    }

    /// How wide this chip is.
    ///
    /// The source chip is wider because it shows a file name; the two toggles
    /// share a width, and saying so once is the difference between a table and
    /// a coincidence that would drift the first time one of them changed.
    const fn width(self) -> u32 {
        const TOGGLE_W: u32 = 120;
        match self {
            Self::Source => 210,
            Self::Capture | Self::Theme => TOGGLE_W,
        }
    }

    /// Where this chip sits. Derived by summing the widths of the chips before
    /// it, so inserting one shifts the rest without a second table.
    fn rect(self) -> Rect {
        let mut x = BAR_CHIPS_X;
        for chip in Self::ALL {
            if chip == self {
                break;
            }
            x += chip.width() + 6;
        }
        Rect::new(x, 6, self.width(), APP_BAR_H - 12)
    }
}

/// Where the app bar's chips start — after the shell's name.
const BAR_CHIPS_X: u32 = 92;

/// Where the `n`th rail entry sits.
const fn rail_rect(n: u32) -> Rect {
    Rect::new(4, APP_BAR_H + 10 + n * 32, RAIL_W.saturating_sub(8), 24)
}

/// How many slots a header has, as the geometry counts them.
///
/// The conversion is here rather than at four call sites, and it saturates:
/// a vocabulary with more than `u32::MAX` arms is not a thing, and the
/// alternative (a cast) is the lint this exists to answer honestly.
fn slots(offered: &[CardAffordance]) -> u32 {
    u32::try_from(offered.len()).unwrap_or(u32::MAX)
}

/// One slot index, likewise.
fn slot_n(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

/// What is under a point.
///
/// Resolved from the rectangles above — the same ones the painter uses — so a
/// control that is drawn is a control that can be clicked, by construction
/// rather than by agreement.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// One of the app bar's three toggles.
    Chip(BarChip),
    /// A rail section.
    Rail(&'static str),
    /// One header affordance of one card.
    Affordance(String, CardAffordance),
    /// A not-ready card's remedy control. Only ever an *actionable* remedy —
    /// `Wait` and `Nothing` are painted as prose and resolve to the card body,
    /// so there is nothing to press where nothing can be done.
    Remedy(String),
    /// A card's body or title: press to select, drag to move.
    Card(String),
    Nothing,
}

impl Hit {
    /// What a point lands on, front to back: chips and rail sit above the
    /// board, and a card's affordances sit above its own body.
    fn at(state: &ShellState, px: u32, py: u32) -> Self {
        if py < APP_BAR_H {
            for chip in BarChip::ALL {
                if contains(chip.rect(), px, py) {
                    return Self::Chip(chip);
                }
            }
            return Self::Nothing;
        }
        if px < RAIL_W {
            for (n, name) in RAIL.iter().enumerate() {
                if contains(rail_rect(slot_n(n)), px, py - APP_BAR_H) {
                    return Self::Rail(name);
                }
            }
            return Self::Nothing;
        }
        let board = state.board.get();
        for card in &state.cards.get() {
            if state.is_floating(card.id().as_str()) {
                continue;
            }
            let Some(tile) = board.tile(card.id()) else {
                continue;
            };
            let rect = cell_rect(&board, tile);
            if !contains(rect, px, py) {
                continue;
            }
            // Into the card's own space, which is the space the painter placed
            // its children in.
            let (lx, ly) = (px - rect.x, py - rect.y);
            let inside = local(rect);
            let id = card.id().as_str().to_string();
            let header = header_rect(inside);
            if contains(header, lx, ly) {
                let offered = card.chrome().offered();
                for (n, affordance) in offered.iter().enumerate() {
                    if contains(affordance_rect(header, slots(&offered), slot_n(n)), lx, ly) {
                        return Self::Affordance(id, *affordance);
                    }
                }
                return Self::Card(id);
            }
            if card.remedy().is_some_and(Remedy::is_actionable)
                && contains(remedy_rect(body_rect(inside)), lx, ly)
            {
                return Self::Remedy(id);
            }
            return Self::Card(id);
        }
        Self::Nothing
    }

    /// The card this hit is on, if it is on one.
    fn card_id(&self) -> Option<&str> {
        match self {
            Self::Affordance(id, _) | Self::Remedy(id) | Self::Card(id) => Some(id),
            Self::Chip(_) | Self::Rail(_) | Self::Nothing => None,
        }
    }
}

/// The colours every painter in this file reads.
///
/// One value rather than five parameters. `card_scene` had eight arguments and
/// clippy said so, which is the lint working as a design signal: five of them
/// were always the same five theme roles resolved at the same moment, so they
/// were one thing arriving as five.
#[derive(Debug, Clone, Copy)]
struct Palette {
    ink: Color,
    muted: Color,
    accent: Color,
    surface: Color,
    bar: Color,
    outline: Color,
}

fn label(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(
        text,
        rect,
        TextStyle::new().with_size_px(px).with_fg(fg),
    ))
}

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
}

/// One card's header: its title, and one tagged label per affordance it offers.
///
/// The order comes from [`CardChrome::offered`], which is the enum's
/// declaration order — so every card on this board, and every card in any other
/// application, lays its header out the same way.
fn header_scene(card: &Card, rect: Rect, ink: Color, muted: Color) -> Vec<Scene> {
    let mut out = vec![label(
        card.title(),
        Rect::new(rect.x + 6, rect.y + 3, rect.w.saturating_sub(120), 15),
        TITLE_FONT_PX,
        ink,
    )];
    let offered = card.chrome().offered();
    for (n, affordance) in offered.iter().enumerate() {
        let slot = affordance_rect(rect, slots(&offered), slot_n(n));
        out.push(Scene::Container(
            ContainerNode::new(vec![label(
                affordance_glyph(*affordance),
                Rect::new(slot.x + 6, slot.y + 4, slot.w - 8, 13),
                BODY_FONT_PX,
                muted,
            )])
            .with_tag(format!("card.{}.{}", card.id().as_str(), affordance.wire()))
            .with_layout(absolute(slot)),
        ));
    }
    out
}

/// The word a header affordance shows. Short because a card header is 20px
/// tall; distinct because a person has to tell them apart without a tooltip.
const fn affordance_glyph(affordance: CardAffordance) -> &'static str {
    match affordance {
        CardAffordance::Settings => "set",
        CardAffordance::TearOff => "out",
        CardAffordance::Maximize => "max",
        CardAffordance::Close => "x",
    }
}

/// What a card's body paints.
///
/// The two branches are the design: a ready card paints its own content, and
/// **every** not-ready card paints the same two things — the state's sentence
/// and its derived remedy — so the twelve kinds cannot disagree about what an
/// encrypted link offers.
fn body_scene(card: &Card, rect: Rect, ink: Color, muted: Color, accent: Color) -> Vec<Scene> {
    if card.state().is_ready() {
        return ready_body(card, rect, ink, accent);
    }
    let mut out = vec![label(
        &state_sentence(card.state()),
        Rect::new(rect.x + 8, rect.y + 8, rect.w.saturating_sub(16), 14),
        BODY_FONT_PX,
        muted,
    )];
    if let Some(remedy) = card.remedy() {
        // The remedy is painted as a control only when the person is the one
        // expected to act. `Wait` is the card's own job and `Nothing` is
        // nobody's, and neither gets a button — which is the derivation doing
        // the deciding rather than this function.
        let text = if remedy.is_actionable() {
            format!("[ {} ]", remedy_label(remedy))
        } else {
            remedy_label(remedy).to_string()
        };
        let slot = remedy_rect(rect);
        out.push(Scene::Container(
            ContainerNode::new(vec![label(
                &text,
                Rect::new(slot.x + 4, slot.y + 4, slot.w.saturating_sub(8), 14),
                BODY_FONT_PX,
                if remedy.is_actionable() {
                    accent
                } else {
                    muted
                },
            )])
            .with_tag(format!("card.{}.remedy", card.id().as_str()))
            .with_layout(absolute(slot)),
        ));
    }
    out
}

/// A ready card's own content. Small on purpose — what this file demonstrates
/// is the shell, and the panes themselves have their own examples.
fn ready_body(card: &Card, rect: Rect, ink: Color, accent: Color) -> Vec<Scene> {
    if card.id().as_str() == "kpi" {
        // The capability list's KPI stat tile: a box, a label and a sparkline.
        // Assembled here rather than shipped by the framework, which is exactly
        // what that row's verdict claims — so this is the claim, executed.
        let spark = Rect::new(
            rect.x + 8,
            rect.y + 26,
            rect.w.saturating_sub(16).max(8),
            rect.h.saturating_sub(34).max(8),
        );
        return vec![
            label(
                "17.3 Mb/s",
                Rect::new(rect.x + 8, rect.y + 6, rect.w.saturating_sub(16), 16),
                TITLE_FONT_PX,
                ink,
            ),
            Scene::Container(
                ContainerNode::new(vec![
                    Sparkline::new(KPI_SERIES.to_vec())
                        .with_color(accent)
                        .with_tag_prefix("kpi.spark")
                        .build(Rect::new(0, 0, spark.w, spark.h), &ChartStyle::default()),
                ])
                .with_tag("card.kpi.sparkline")
                .with_layout(absolute(spark)),
            ),
        ];
    }
    vec![label(
        &format!("{} content", card.title().to_lowercase()),
        Rect::new(rect.x + 8, rect.y + 10, rect.w.saturating_sub(16), 14),
        BODY_FONT_PX,
        ink,
    )]
}

/// The sentence a not-ready card shows. One per arm, and the two that carry a
/// reason say it.
fn state_sentence(state: &CardState) -> String {
    match state {
        CardState::Ready => "showing content".to_string(),
        CardState::Loading => "loading...".to_string(),
        CardState::Empty => "nothing matched this filter".to_string(),
        CardState::Failed(why) => format!("could not load: {why}"),
        CardState::Denied(what) => format!("not permitted: {what}"),
        CardState::Opaque => "link is encrypted; content unavailable".to_string(),
    }
}

/// What the remedy reads as to a person.
const fn remedy_label(remedy: Remedy) -> &'static str {
    match remedy {
        Remedy::Wait => "waiting",
        Remedy::Retry => "Retry",
        Remedy::Widen => "Widen filter",
        Remedy::Authorize => "Request access",
        Remedy::Nothing => "nothing can be done",
    }
}

/// What one app-bar chip reads as. The chip is the control, so its label has to
/// say both the setting's name and its current value — a chip reading only
/// "dark" leaves a person guessing what pressing it does.
fn chip_label(state: &ShellState, chip: BarChip) -> String {
    match chip {
        BarChip::Source => format!("source: {}", state.source.get()),
        BarChip::Capture => format!(
            "capture: {}",
            if state.capturing.get() { "on" } else { "off" }
        ),
        BarChip::Theme => format!("theme: {}", theme_word(&state.theme)),
    }
}

fn app_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let Palette {
        ink,
        muted,
        surface: chip_fill,
        bar: fill,
        outline,
        ..
    } = palette;
    let clock = &state.clock;
    let mut children = vec![label(
        "Analyzer",
        Rect::new(12, 11, 76, 16),
        TITLE_FONT_PX,
        ink,
    )];
    // Three pressable chips, drawn at the rectangles the hit test resolves —
    // one geometry, so a chip cannot be painted where it cannot be pressed.
    for chip in BarChip::ALL {
        let rect = chip.rect();
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                &chip_label(state, chip),
                Rect::new(rect.x + 8, rect.y + 6, rect.w.saturating_sub(12), 14),
                BODY_FONT_PX,
                ink,
            )])
            .with_tag(chip.tag())
            .with_style(
                BoxStyle::filled(chip_fill)
                    .with_corner_radius(4)
                    .with_border(Border::new(outline, 1)),
            )
            .with_layout(absolute(rect)),
        ));
    }
    // The search box says which of its two states it is in: a caret when it is
    // taking keystrokes, and how to get there when it is not. A box that looked
    // the same either way would leave a person typing into the board.
    let tail = format!(
        "search: {}   |   {}",
        if state.searching.get() {
            format!("{}|", state.search.get())
        } else if state.search.get().is_empty() {
            "(press / to search)".to_string()
        } else {
            state.search.get()
        },
        transport_word(clock.status(), state.capturing.get()),
    );
    let tail_x = BarChip::Theme.rect().x + BarChip::Theme.width() + 14;
    children.push(label(
        &tail,
        Rect::new(tail_x, 13, WIN_W.saturating_sub(tail_x + 8), 14),
        BODY_FONT_PX,
        muted,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.appbar")
            .with_style(BoxStyle::filled(fill))
            .with_layout(absolute(Rect::new(0, 0, WIN_W, APP_BAR_H))),
    )
}

fn rail_scene(state: &ShellState, palette: Palette) -> Scene {
    let (fill, muted, ink) = (palette.bar, palette.muted, palette.ink);
    let mut entries = Vec::new();
    let focus = state.rail_focus.get();
    for (n, name) in RAIL.iter().enumerate() {
        let rect = rail_rect(slot_n(n));
        // The count is of the cards still ON the board: closing one is
        // supposed to move this number, which is what makes the rail a
        // navigation aid rather than a static list of words.
        let here = section_count(state, name);
        let on = focus.as_deref() == Some(*name);
        entries.push(Scene::Container(
            ContainerNode::new(vec![label(
                &format!("{} {here}", &name[..3.min(name.len())]),
                Rect::new(rect.x + 4, rect.y + 5, rect.w.saturating_sub(6), 13),
                BODY_FONT_PX,
                if on { ink } else { muted },
            )])
            .with_tag(format!("shell.rail.{name}"))
            .with_style(if on {
                BoxStyle::filled(fill).with_border(Border::new(ink, 1))
            } else {
                BoxStyle::filled(fill)
            })
            .with_layout(absolute(rect)),
        ));
    }
    Scene::Container(
        ContainerNode::new(entries)
            .with_tag("shell.rail")
            .with_style(BoxStyle::filled(fill))
            .with_layout(absolute(Rect::new(
                0,
                APP_BAR_H,
                RAIL_W,
                WIN_H.saturating_sub(APP_BAR_H),
            ))),
    )
}

fn card_scene(card: &Card, rect: Rect, selected: bool, palette: Palette) -> Scene {
    let Palette {
        ink,
        muted,
        accent,
        surface,
        outline,
        ..
    } = palette;
    let inside = local(rect);
    let mut children = header_scene(card, header_rect(inside), ink, muted);
    children.extend(body_scene(card, body_rect(inside), ink, muted, accent));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{}", card.id().as_str()))
            .with_style(
                BoxStyle::filled(surface)
                    .with_corner_radius(6)
                    // The selection ring: one card is the keyboard's subject and
                    // a person has to be able to see which. Two pixels of accent
                    // rather than a different fill, so a selected card that is
                    // also failing still reads as failing.
                    .with_border(if selected {
                        Border::new(accent, 2)
                    } else {
                        Border::new(outline, 1)
                    }),
            )
            .with_layout(absolute(rect)),
    )
}

fn view(_state: (), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_shell_state();
    let palette = Palette {
        ink: theme.resolve(ColorRole::OnSurface),
        muted: theme.resolve(ColorRole::OnSurfaceMuted),
        accent: theme.resolve(ColorRole::Accent),
        surface: theme.resolve(ColorRole::SurfaceContainerHigh),
        bar: theme.resolve(ColorRole::SurfaceContainer),
        outline: theme.resolve(ColorRole::Outline),
    };

    let board = state.board.get();
    let selected = state.selected.get();
    let mut children = vec![app_bar_scene(&state, palette), rail_scene(&state, palette)];

    for card in &state.cards.get() {
        // A torn-off card KEEPS ITS PLACE. The board is the arrangement the
        // user made — where a card belongs — and the statechart says where it
        // is being shown; reflowing the board on tear-off and again on
        // dock-back is how a dashboard loses a layout to a gesture that was
        // meant to be temporary. So the tile stays and only the paint moves,
        // which is also why `dock_back` needs no way-home token the way
        // `maximize` does.
        if state.is_floating(card.id().as_str()) {
            continue;
        }
        if let Some(tile) = board.tile(card.id()) {
            children.push(card_scene(
                card,
                cell_rect(&board, tile),
                selected.as_deref() == Some(card.id().as_str()),
                palette,
            ));
        }
    }

    children.push(Scene::Container(
        ContainerNode::new(vec![label(
            &format!(
                "{} — {} card(s), layout \"{}\", selection {}   ·   {}",
                state.last_event.get(),
                state.cards.get().len(),
                state.preset.get(),
                selected.as_deref().unwrap_or("none"),
                HELP_STRIP,
            ),
            Rect::new(8, 6, WIN_W.saturating_sub(RAIL_W + 16), 14),
            BODY_FONT_PX,
            palette.muted,
        )])
        .with_tag("shell.status")
        .with_layout(absolute(Rect::new(
            RAIL_W,
            WIN_H.saturating_sub(STATUS_H),
            WIN_W.saturating_sub(RAIL_W),
            STATUS_H,
        ))),
    ));

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct AnalyzerShellView;

impl WidgetCore for AnalyzerShellView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ShellOracle::new();
        oracle.attach_state(use_shell_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-analyzer-shell (R1648 §5.21 analysis-tool dashboard shell)"
    }
}

impl WidgetA11y for AnalyzerShellView {
    /// The board is a group, and **every card is a node that says what it is
    /// showing**.
    ///
    /// This is the half a paint cannot carry, and the reason the state had to
    /// be a value rather than a rendered sentence: a card that failed announces
    /// its failure and its remedy, so a screen-reader user learns that the
    /// latency collector is unreachable and that a retry exists. Measured on
    /// the toolkit at 6.11, no panel or view class has a content-state concept
    /// at all, so this is not something an assistive technology can be told
    /// there.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_shell_state();
        let mut nodes = vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Analyzer dashboard")
                .with_value(AccessValue::Text(format!(
                    "{} cards on layout \"{}\", source {}",
                    state.cards.get().len(),
                    state.preset.get(),
                    state.source.get(),
                ))),
        ];
        for card in &state.cards.get() {
            let announce = match card.remedy() {
                None => state_sentence(card.state()),
                Some(remedy) => {
                    format!("{}; {}", state_sentence(card.state()), remedy_label(remedy))
                }
            };
            nodes.push(
                AccessNode::new(format!("card.{}", card.id().as_str()), AriaRole::Group)
                    .with_name(card.title())
                    .with_value(AccessValue::Text(announce))
                    .with_state(AccessState::default()),
            );
        }
        nodes
    }
}

impl WidgetView for AnalyzerShellView {
    type Renderer = HelloAnalyzerShellRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AnalyzerShellView>();
}

#[cfg(test)]
mod tests;
