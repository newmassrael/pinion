//! `hello-tile-dashboard` — R1608 §5.21 — a **monitoring-tool-class tile dashboard** over the
//! R1560 CSS Grid, driven by a press-drag and by the wire.
//!
//! R1607 built the model ([`TileGrid`]) and measured that the existing grid
//! layout holds a dashboard, and then deliberately stopped: it had no consumer,
//! so it could not answer whether the reflow's *locality* feels right under a
//! real cursor. This is that consumer, and it is what closes the gesture axis of
//! the tile-dashboard gap.
//!
//! ## The three layers, and which one is new
//!
//! * **Layout** — `Display::Grid` with twelve `Fr(1.0)` columns and fixed-height
//!   rows. Not new: R1560 added it for a text table's rowspans.
//! * **Arrangement** — [`TileGrid`], which holds cards that do not overlap and
//!   reports what a move displaced. Not new: R1607.
//! * **Gesture** — this file. A press latches the card under the cursor *and
//!   where inside it the grab was*; each move snaps the cursor to a grid cell
//!   and asks the model to move the card there; the release lets go.
//!
//! ## Why the grab offset is the whole trick
//!
//! Snapping the cursor's cell and moving the card's top-left there makes a
//! card jump under the pointer the moment you grab it anywhere but its corner.
//! The latch therefore stores the cell the grab happened at *relative to the
//! card*, and the move subtracts it — so the card stays under the finger,
//! which is the difference between a drag and a teleport. The dashboard tool
//! does the same thing; what it does not do is tell you which other cards it
//! pushed.
//!
//! ## The AI-first witness (§2 #7)
//!
//! The arrangement is a value, so the whole dashboard is one JSON read:
//! `scene/query /external/layout`. Every gesture is also a verb —
//! `scene/invoke /external/move_to "cpu,0,0"` / `resize "cpu,6,2"` / `compact` —
//! so an agent rearranges the board with no pixel, and `last_reflow` reports
//! what the last edit displaced. See `tools/demos/r1608_tile_dashboard.py`.
//!
//! ## R1609 — the board is editable without a pointer
//!
//! R1608 left the board **drag-and-RPC only**: there was no keyboard channel at
//! all, so the assistive-technology nodes it added let a screen-reader user
//! *read* the arrangement and never change it, and a resize existed only as a
//! wire verb with no handle to grab. Both close here.
//!
//! **One focus stop, a roving current card.** The board is the Tab stop and
//! the selected card is its `aria-activedescendant` — the pattern `hello-grid-nav`'s current cell and the
//! toolbar's roving item already use. Thirty cards must not be thirty Tab
//! stops, and the toolkit's MDI alternative (`activateNextSubWindow`, a walk down a list in
//! creation order) is the reason it needs a *spatial* move instead: plain
//! arrows change the selection through [`TileGrid::neighbour`], which is total over a legal
//! arrangement.
//!
//! **The keymap is twelve chords and no mode**, because [`TileNudge`] is twelve
//! values:
//!
//! | chord | effect |
//! |---|---|
//! | <kbd>←↑→↓</kbd> | move the *selection* to the neighbouring card |
//! | <kbd>Shift</kbd>+arrow | `Move` — slide the card one cell |
//! | <kbd>Alt</kbd>+arrow | `Grow` — push that side out |
//! | <kbd>Alt</kbd>+<kbd>Shift</kbd>+arrow | `Shrink` — pull that side in |
//! | <kbd>Escape</kbd> | cancel the session, board and all |
//! | <kbd>Enter</kbd> | commit it |
//!
//! The toolkit reaches the same behaviours only after a system-menu round trip
//! into `isInInteractiveMode`, warps the physical mouse cursor to do it, and treats
//! <kbd>Escape</kbd> and <kbd>Enter</kbd> identically so a keyboard move there
//! cannot be abandoned.
//!
//! **Eight handles, hit-tested from the same cell arithmetic.** A press resolves
//! [`TileHandle::at`] over the grabbed card, so landing on a side or a corner
//! resizes and landing in the middle drags — one press path, and the handle a
//! card is showing carries [`TileHandle::cursor`], which is what forced
//! `CursorHint`'s two diagonal arms (R1189 had commanded them for a *window*
//! corner since ~R1189 and no scene node could ask for one).
//!
//! **The displacement is announced.** The board carries a `polite` [`AccessLive`] region
//! whose text names what the last edit pushed, because the cards that moved
//! are exactly the ones the user is not on. No widget in the toolkit fires an
//! announcement for this — the three translation units implementing its MDI
//! child window, its MDI area and its size grip contain no accessibility
//! notification of any kind, so a toolkit MDI window that moves or resizes is
//! silent to a screen reader even though `state()` advertises `movable` and `sizeable`.

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y,
};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaField, ThreadOwnership,
};
use pinion_core::input::{Modifiers, PointerReading};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    BoxStyle, FlexDirection, GridPlacement, GridTrack, LayoutStyle, Size, SizeValue, TextStyle,
    TrackMax, TrackMin,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::tile_grid::{
    Reflow, Tile, TileDirection, TileEdit, TileGrid, TileHandle, TileId, TileNudge,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTileDashboardRenderer, HelloTileDashboardRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 420;
const ROOT_TAG: &str = "dashboard";
/// (R1650 §5.35) Every painted sub-region of the board is addressed as a
/// **composite** tag of the board's own — `dashboard#card.loss`, not
/// `card.loss`.
///
/// Not cosmetic. The §5.35 router resolves the deepest *tagged* node under the
/// cursor and then looks the **primary half** of that tag up as an `External`;
/// a top-level `card.loss` resolves to nothing, so the press was dropped in
/// silence and this board was **dead to a real mouse** from R1608 until here —
/// while its demos passed, because `invoke` reaches the handler by name and
/// never asks the router. `pinion_runtime::pointer_reach` found it, and
/// `scene/drag` confirmed it: the arrangement did not change under a
/// router-driven drag and did under the wire verb.
///
/// The other repair — declaring the cards
/// [`pointer_transparent`](pinion_core::style::LayoutStyle::with_pointer_transparent)
/// — is the right one for pure decoration and the WRONG one here: it takes the
/// whole subtree out of the hit test, and the grip ring's eight resize cursors
/// are declared on nodes inside it. A composite tag keeps them hit-testable
/// *and* delivers the press, which is what makes it the idiom the data grid
/// (`data_grid#0_4`) and the node editor already use.
fn sub_tag(sub: &str) -> String {
    format!("{ROOT_TAG}#{sub}")
}
const THEME_TAG: &str = "app";
/// (R1609) The live region that announces what an edit displaced.
const REFLOW_TAG: &str = "dashboard.reflow";

/// The dashboard's columns — the one number a dashboard tool layout declares.
const COLUMNS: u32 = 12;
/// A grid row's pixel height. Rows are fixed so a card's height is a count.
const ROW_H: u32 = 64;
/// How many rows the grid always offers, so a card can be dragged into empty
/// space below the ones that exist.
const MIN_ROWS: u32 = 5;

const TITLE_FONT_PX: u32 = 15;
const CARD_FONT_PX: u32 = 12;

/// The board a fresh window opens with: the shape the R1607 measurement laid
/// out, so the example and the layout test describe the same dashboard.
fn seed() -> TileGrid {
    let mut grid = TileGrid::new(COLUMNS);
    for tile in [
        Tile::new("throughput", 0, 0, 12, 1),
        Tile::new("latency", 0, 1, 6, 1),
        Tile::new("loss", 6, 1, 6, 1),
        Tile::new("topology", 0, 2, 4, 2),
        Tile::new("alarms", 4, 2, 8, 1),
    ] {
        grid.place(tile).expect("the seed board is legal");
    }
    grid
}

/// How much of each side of a card is a resize handle rather than its interior.
///
/// A quarter, so a card two cells wide still has a middle to drag by. The
/// fraction is the binding's — [`TileHandle::at`] takes it as an argument
/// exactly so the crate does not decide a card's feel, the same reason R1606
/// made a hex dump's separator a parameter.
const HANDLE_BAND: f32 = 0.25;

/// The ring's middle track — whatever the two bands leave.
///
/// Derived rather than stated, so the painted ring and the hit-test cannot
/// describe two different shapes. See [`handle_ring`].
const HANDLE_CORE: f32 = 1.0 - 2.0 * HANDLE_BAND;

/// What a press latched: which card, where inside it the grab happened, and
/// whether it grabbed a handle.
///
/// The offset is the reason a drag is a drag — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Grab {
    id: TileId,
    /// Columns between the card's left edge and the grabbed cell.
    dx: u32,
    /// Rows between the card's top edge and the grabbed cell.
    dy: u32,
    /// `Some` when the press landed on a side or a corner, so every following
    /// move is a resize of that handle rather than a move of the whole card.
    /// Latched at the press because the pointer travels away from the band
    /// immediately and re-deriving it per move would turn a resize into a drag
    /// the moment the cursor left the edge.
    handle: Option<TileHandle>,
}

/// The board's state, held in signals the oracle and the view SHARE.
///
/// `WidgetCore::State` is `Copy`, and an arrangement is a `Vec` — so the model
/// cannot travel through it. The canonical shape for a model this size is the
/// one `hello-node-groups` uses: the state lives in an `Owner::cache`d signal,
/// the external mutates it, and the view reads it. That also means the painter
/// and the wire read **one** arrangement rather than two copies that could
/// disagree.
#[derive(Debug)]
struct BoardState {
    grid: Signal<TileGrid>,
    /// Which card a press latched, and where inside it the grab happened.
    grab: Signal<Option<Grab>>,
    /// Set on a press, cleared once the following move has resolved the latch.
    pending: Signal<bool>,
    /// What the last edit displaced, as text the wire can read.
    last_reflow: Signal<String>,
    /// Why the last edit was refused, empty when it was not.
    refusal: Signal<String>,
    /// (R1609) The roving current card — the board's `aria-activedescendant`,
    /// and what a keyboard chord acts on. `None` before anything is selected.
    current: Signal<Option<TileId>>,
    /// (R1609) The open keyboard session, if any: the undo point
    /// <kbd>Escape</kbd> restores. Opened lazily by the first editing chord,
    /// so there is no mode to enter — the toolkit needs a system-menu action
    /// for this.
    session: Signal<Option<TileEdit>>,
    /// (R1609) The sentence the board's live region carries. What a screen reader
    /// says when an edit pushes cards the user is not looking at.
    announcement: Signal<String>,
}

impl BoardState {
    fn new() -> Self {
        Self {
            grid: Signal::new(seed()),
            grab: Signal::new(None),
            pending: Signal::new(false),
            last_reflow: Signal::new("clean".to_owned()),
            refusal: Signal::new(String::new()),
            current: Signal::new(None),
            session: Signal::new(None),
            announcement: Signal::new(String::new()),
        }
    }
}

const STATE_KEY: &str = "tile-dashboard-state";

fn use_board_state() -> std::rc::Rc<BoardState> {
    Owner::current()
        .expect("use_board_state requires an active Owner scope")
        .cache(STATE_KEY, BoardState::new)
}

/// The dashboard's root external. It owns **no model** — the arrangement lives
/// in [`BoardState`]'s signals, which the view reads too, so the painter and the
/// wire cannot hold two arrangements that disagree.
#[derive(Debug)]
struct DashboardOracle {
    state: Option<std::rc::Rc<BoardState>>,
}

impl DashboardOracle {
    fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: std::rc::Rc<BoardState>) {
        self.state = Some(state);
    }

    fn bound(&self) -> Option<&BoardState> {
        self.state.as_deref()
    }

    /// How many rows the painted grid offers — the cards' own height, never less
    /// than [`MIN_ROWS`] so there is empty board to drag into.
    fn painted_rows(grid: &TileGrid) -> u32 {
        grid.rows().max(MIN_ROWS)
    }

    /// The cell a reading over the board's rect lands on.
    ///
    /// This is the snap: a pointer anywhere inside a cell means that cell, which
    /// is what makes a dashboard's drag land on a slot rather than a pixel.
    ///
    /// ★★★★★ R1727 — **the two axes read the reading differently, and that is
    /// the whole repair.** A column is a twelfth of the board however wide the
    /// board is, so the horizontal axis is a FRACTION. A row is
    /// [`ROW_H`] pixels tall whatever the board's height is, so the vertical
    /// axis is [`PointerReading::px`] divided by that height — NOT the fraction
    /// times a row count.
    ///
    /// The fraction times a row count is what this did until R1727, and it was
    /// wrong in a way nothing could see: the fraction is taken over the rect the
    /// last **paint** produced and `rows` is read from the **model**, which this
    /// very drag has already grown by displacing cards downward. The two are
    /// only equal when a frame happened in between. Measured, dragging one card
    /// from the same pixel to the same pixel:
    ///
    /// ```text
    /// a frame between the moves    loss@0,4   latency@0,5   topology@0,6
    /// no frame between the moves   loss@0,10  latency@0,7   topology@0,11
    /// ```
    ///
    /// A hand usually supplies the frame, so the defect hid behind the right
    /// answer and every wire-driven test agreed with a picture nobody sees.
    /// `px()` is `cursor − board.origin` whatever the rect's size, so both
    /// deliveries now answer the same thing —
    /// [`assert_gesture_reads_one_fact`](../../../tools/rpc_verify.py) is the
    /// gate that keeps it that way.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a dashboard's column and row counts round-trip f32 exactly, and \
                  both ends are clamped"
    )]
    fn cell_at(rows: u32, at: PointerReading) -> (u32, u32) {
        let col = (at.u().clamp(0.0, 0.999) * COLUMNS as f32) as u32;
        let row = (at.px().1.max(0.0) / ROW_H as f32) as u32;
        (col.min(COLUMNS - 1), row.min(rows.saturating_sub(1)))
    }

    /// The card covering a cell, if any.
    fn tile_at(grid: &TileGrid, col: u32, row: u32) -> Option<&Tile> {
        grid.tiles()
            .iter()
            .find(|t| col >= t.col && col < t.right() && row >= t.row && row < t.bottom())
    }

    /// A reflow as `"id:from>to, .."`, or `clean`.
    fn reflow_text(reflow: &Reflow) -> String {
        if reflow.is_clean() {
            return "clean".to_owned();
        }
        reflow
            .displaced()
            .iter()
            .map(|d| format!("{}:{}>{}", d.id, d.from, d.to))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The arrangement as `"id@col,row+wxh"` rows — the compact peer of the JSON
    /// `layout`, for a reader that wants one line rather than a document.
    fn tiles_text(grid: &TileGrid) -> String {
        grid.tiles()
            .iter()
            .map(|t| format!("{}@{},{}+{}x{}", t.id, t.col, t.row, t.w, t.h))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `"<id>,<a>,<b>"` — the wire form both gesture verbs take.
    fn parse_triple(args: &IntrospectValue) -> Option<(TileId, u32, u32)> {
        let IntrospectValue::Text(spec) = args else {
            return None;
        };
        let mut parts = spec.split(',');
        let id = TileId::new(parts.next()?.trim());
        let a = parts.next()?.trim().parse().ok()?;
        let b = parts.next()?.trim().parse().ok()?;
        Some((id, a, b))
    }

    /// (R1609) Where inside a card a board-relative point falls, as `[0, 1]`
    /// fractions — what [`TileHandle::at`] hit-tests.
    ///
    /// Exact rather than approximate, because the card's own extent is derived
    /// from the same cell arithmetic the snap uses: a card occupies
    /// `col/COLUMNS ..= right/COLUMNS` of the board's width. So the handle band
    /// is a fraction of the *card*, which is what makes a two-cell card and a
    /// twelve-cell card both grabbable in the middle.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a dashboard's column and row counts round-trip f32 exactly"
    )]
    ///
    /// R1727 — the vertical half reads [`PointerReading::px`] for the reason
    /// [`cell_at`](Self::cell_at) does: a row's height is a constant, and
    /// scaling by a row COUNT the drag itself changes is the defect that round
    /// removed. `rows` is no longer needed here at all.
    fn card_fraction(tile: &Tile, at: PointerReading) -> (f32, f32) {
        let u = (at.u() * COLUMNS as f32 - tile.col as f32) / tile.w as f32;
        let v = (at.px().1 / ROW_H as f32 - tile.row as f32) / tile.h as f32;
        (u, v)
    }

    /// (R1609) The sentence the live region carries after an edit.
    ///
    /// Two clauses: where the edited card now is, and what it pushed. The second
    /// is the one that needs a live region at all — a displaced card is by
    /// definition not the one the user is on, so navigating to it is not an
    /// option. Slots are stated in CSS's one-based lines, the same numbers the
    /// per-card AT value uses, because a user hearing "column 5" from one channel
    /// and "column 4" from another has no way to tell which is the board's.
    fn announce(grid: &TileGrid, id: &TileId, reflow: &Reflow) -> String {
        let Some(tile) = grid.tile(id) else {
            return String::new();
        };
        let mut sentence = format!(
            "{} at column {}, row {}, {} by {}",
            id,
            tile.col + 1,
            tile.row + 1,
            tile.w,
            tile.h
        );
        if !reflow.is_clean() {
            let pushed = reflow
                .displaced()
                .iter()
                .map(|d| format!("{} to row {}", d.id, d.to + 1))
                .collect::<Vec<_>>()
                .join(", ");
            sentence.push_str("; pushed ");
            sentence.push_str(&pushed);
        }
        sentence
    }

    /// (R1609) The current card, defaulting to the first one so a fresh Tab into
    /// the board has something to act on.
    fn current_id(state: &BoardState) -> Option<TileId> {
        state
            .current
            .get()
            .filter(|id| state.grid.get().tile(id).is_some())
            .or_else(|| state.grid.get().tiles().first().map(|t| t.id.clone()))
    }

    /// (R1609) `"[Ctrl+][Alt+][Shift+]<key>"` — the chord vocabulary, so a
    /// keyboard gesture is drivable over the wire exactly as it arrives from the
    /// platform. Fixed modifier order, so one chord has one spelling.
    fn chord(key: &str, modifiers: Modifiers) -> String {
        let mut spelled = String::new();
        if modifiers.command_key() {
            spelled.push_str("Ctrl+");
        }
        if modifiers.alt_key() {
            spelled.push_str("Alt+");
        }
        if modifiers.shift_key() {
            spelled.push_str("Shift+");
        }
        spelled.push_str(key);
        spelled
    }

    /// (R1609) A chord back into `(key, alt, shift)`. Unknown prefixes make the
    /// whole chord unrecognised rather than being skipped, so a typo on the wire
    /// is refused instead of silently doing something else.
    fn parse_chord(chord: &str) -> Option<(&str, bool, bool)> {
        let mut alt = false;
        let mut shift = false;
        let mut rest = chord;
        loop {
            if let Some(tail) = rest.strip_prefix("Ctrl+") {
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("Alt+") {
                alt = true;
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("Shift+") {
                shift = true;
                rest = tail;
            } else if rest.contains('+') {
                return None;
            } else {
                return (!rest.is_empty()).then_some((rest, alt, shift));
            }
        }
    }

    /// (R1609) The arrow key a chord names, if it names one.
    fn arrow(key: &str) -> Option<TileDirection> {
        match key {
            "ArrowLeft" => Some(TileDirection::Left),
            "ArrowRight" => Some(TileDirection::Right),
            "ArrowUp" => Some(TileDirection::Up),
            "ArrowDown" => Some(TileDirection::Down),
            _ => None,
        }
    }

    /// (R1609) Run one keyboard chord. `true` when it was this board's.
    ///
    /// **No mode.** The chord says whether it navigates, moves, grows or
    /// shrinks; the toolkit reads the same four arrow keys against a `currentOperation` that
    /// a system-menu action set, so its meaning depends on invisible state.
    fn key(state: &BoardState, chord: &str) -> bool {
        let Some((key, alt, shift)) = Self::parse_chord(chord) else {
            return false;
        };
        let Some(id) = Self::current_id(state) else {
            return false;
        };
        if let Some(direction) = Self::arrow(key) {
            // Plain arrows move the SELECTION, spatially. The cards are one Tab
            // stop with a roving active descendant, so thirty cards are not
            // thirty stops — and `neighbour` is what makes that navigable in two
            // dimensions where `activateNextSubWindow` walks a creation-order
            // list.
            if !alt && !shift {
                let Some(next) = state
                    .grid
                    .get()
                    .neighbour(&id, direction)
                    .map(|t| t.id.clone())
                else {
                    // Nothing that way: the selection stays put rather than
                    // wrapping, which would move focus somewhere the arrow did
                    // not point.
                    return true;
                };
                state.current.set(Some(next.clone()));
                state.announcement.set(Self::announce(
                    &state.grid.get(),
                    &next,
                    &Reflow::default(),
                ));
                return true;
            }
            let nudge = match (alt, shift) {
                (false, _) => TileNudge::Move(direction),
                (true, false) => TileNudge::Grow(direction),
                (true, true) => TileNudge::Shrink(direction),
            };
            // The session opens on the first editing chord and is what Escape
            // restores — including the cards this edit displaces, which is why it
            // holds the whole board and not the card's rectangle.
            if state.session.get().is_none() {
                if let Ok(session) = TileEdit::begin(&state.grid.get(), &id) {
                    state.session.set(Some(session));
                }
            }
            let before = state.grid.get();
            let outcome = Self::edit(state, |grid| grid.nudge(&id, nudge));
            let after = state.grid.get();
            if outcome.is_ok() {
                state.announcement.set(if before == after {
                    // A held arrow at a bound stops. Saying so beats repeating
                    // an unchanged slot, and beats silence.
                    format!("{id} is already at the edge")
                } else {
                    Self::announce(&after, &id, &Reflow::between(&before, &after, &id))
                });
            }
            return true;
        }
        match key {
            "Escape" => {
                // ★ the toolkit stores `oldGeometry` on entering interactive mode and
                // never restores it — Escape, Return and Enter share one arm
                // there, so Escape commits.
                if let Some(session) = state.session.get() {
                    let restored = session.cancel();
                    let sentence = format!("{} restored", Self::describe(&restored));
                    state.grid.set(restored);
                    state.session.set(None);
                    state.last_reflow.set("clean".to_owned());
                    state.announcement.set(sentence);
                }
                true
            }
            "Enter" => {
                if let Some(session) = state.session.get() {
                    let reflow = session.reflow(&state.grid.get());
                    let id = session.id().clone();
                    state.session.set(None);
                    state.announcement.set(format!(
                        "{} committed",
                        Self::announce(&state.grid.get(), &id, &reflow)
                    ));
                }
                true
            }
            _ => false,
        }
    }

    /// A whole arrangement in one clause, for the cancel announcement.
    fn describe(grid: &TileGrid) -> String {
        format!("{} cards on {} columns", grid.tiles().len(), grid.columns())
    }

    /// (R1609) `"<id>,<handle>,<col>,<row>"` — a handle drag named rather than
    /// hit-tested, so an agent reaches all eight handles with no pointer.
    ///
    /// Its own function because it owns its own parsing, and because the handle
    /// name is resolved **against [`TileHandle::ALL`]** — so the vocabulary this
    /// accepts and the one the `handles` slot advertises cannot drift apart,
    /// which is R1564's rule for a refused `send` applied to a refused drag.
    fn invoke_drag_handle(
        state: &BoardState,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let bad = || {
            InvokeError::Rejected(
                "expected \"<id>,<handle>,<col>,<row>\" with a handle from the \
                 `handles` slot"
                    .into(),
            )
        };
        let IntrospectValue::Text(spec) = args else {
            return Err(bad());
        };
        let mut parts = spec.split(',').map(str::trim);
        let (Some(id), Some(handle), Some(col), Some(row), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(bad());
        };
        let Some(handle) = TileHandle::ALL
            .into_iter()
            .find(|h| format!("{h:?}").eq_ignore_ascii_case(handle))
        else {
            return Err(bad());
        };
        let (Ok(col), Ok(row)) = (col.parse(), row.parse()) else {
            return Err(bad());
        };
        let id = TileId::new(id);
        let before = state.grid.get();
        let outcome = Self::edit(state, |grid| grid.drag_handle(&id, handle, col, row))
            .map_err(|sentence| InvokeError::Rejected(sentence.into()))?;
        let after = state.grid.get();
        state.announcement.set(Self::announce(
            &after,
            &id,
            &Reflow::between(&before, &after, &id),
        ));
        Ok(IntrospectValue::Text(outcome))
    }

    /// Apply an edit to the shared grid and record its outcome.
    fn edit(
        state: &BoardState,
        apply: impl FnOnce(&mut TileGrid) -> Result<Reflow, pinion_core::widgets::tile_grid::TileError>,
    ) -> Result<String, String> {
        let mut grid = state.grid.get();
        match apply(&mut grid) {
            Ok(reflow) => {
                state.grid.set(grid);
                let text = Self::reflow_text(&reflow);
                state.last_reflow.set(text.clone());
                state.refusal.set(String::new());
                Ok(text)
            }
            Err(error) => {
                // A refused edit leaves the grid alone: the model checks before
                // it mutates, so the copy is simply dropped.
                let sentence = error.to_string();
                state.last_reflow.set("clean".to_owned());
                state.refusal.set(sentence.clone());
                Err(sentence)
            }
        }
    }
}

impl External for DashboardOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Take pointer capture so a press-drag keeps forwarding
    /// [`pointer_move`](Self::pointer_move) while held.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// (R1650 §5.35) Normalise against the **board**, not the grabbed card.
    ///
    /// The pair to [`sub_tag`]: once a card is `dashboard#card.loss`, the
    /// router's default would hand [`pointer_move`](Self::pointer_move) the
    /// cursor relative to that CARD's rect, and
    /// [`cell_at`](DashboardOracle::cell_at) reads a fraction of the whole
    /// grid — so every drag would land in the wrong cell, deterministically and
    /// silently. The declaration says which rect the value spans, which is the
    /// same reason the range slider grabs a thumb and measures the track.
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Primary
    }

    /// Snap to a cell, then either latch (a fresh press) or move the latched
    /// card so the grabbed cell stays under the cursor.
    fn pointer_move(&mut self, at: PointerReading) {
        let Some(state) = self.bound() else {
            return;
        };
        let grid = state.grid.get();
        let rows = Self::painted_rows(&grid);
        let (col, row) = Self::cell_at(rows, at);
        if state.pending.get() {
            state.pending.set(false);
            let selected = Self::current_id(state);
            let latched = Self::tile_at(&grid, col, row).map(|tile| Grab {
                id: tile.id.clone(),
                dx: col - tile.col,
                dy: row - tile.row,
                // R1609 — the handle is resolved ONCE, at the press, for two
                // separate reasons.
                //
                // *Once*, because the pointer leaves the band on the very first
                // move: re-deriving it per move would turn every resize into a
                // drag the instant it started.
                //
                // ★ And only on the card that is **already selected**, because
                // that is the only card painting a grip ring. The first draft
                // hit-tested every card and a test caught it: a press near any
                // card's edge resized it with nothing on screen saying it would,
                // so the paint and the gesture were reading two different facts.
                // Selecting first and grabbing second is also what every real
                // board does.
                handle: (selected.as_ref() == Some(&tile.id))
                    .then(|| {
                        let (u, v) = Self::card_fraction(tile, at);
                        TileHandle::at(u, v, HANDLE_BAND)
                    })
                    .flatten(),
            });
            if let Some(grab) = &latched {
                // Pressing a card selects it, so the pointer and the keyboard
                // share one current card rather than two that drift apart.
                state.current.set(Some(grab.id.clone()));
                // And it opens the same session a chord opens, so Escape
                // mid-drag puts the board back — including the cards the drag
                // has already pushed.
                if let Ok(session) = TileEdit::begin(&grid, &grab.id) {
                    state.session.set(Some(session));
                }
            }
            state.grab.set(latched);
            return;
        }
        let Some(grab) = state.grab.get() else {
            return;
        };
        let outcome = if let Some(handle) = grab.handle {
            // A resize needs no grab offset: the dragged side follows the cursor
            // and the opposite one holds still, which `set_edge` guarantees.
            Self::edit(state, |grid| grid.drag_handle(&grab.id, handle, col, row))
        } else {
            // ★ The grab offset is what keeps the card under the finger rather
            // than teleporting its corner to the cursor.
            let target_col = col.saturating_sub(grab.dx);
            let target_row = row.saturating_sub(grab.dy);
            Self::edit(state, |grid| grid.move_to(&grab.id, target_col, target_row))
        };
        if outcome.is_ok() {
            let after = state.grid.get();
            let reflow = Reflow::between(&grid, &after, &grab.id);
            state
                .announcement
                .set(Self::announce(&after, &grab.id, &reflow));
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for DashboardOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("columns", "int"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("tile_count", "int"),
                    // The whole arrangement, as the value it is.
                    SchemaField::new("layout", "json"),
                    SchemaField::new("tiles", "string"),
                    SchemaField::new("last_reflow", "string"),
                    SchemaField::new("last_refusal", "string"),
                    SchemaField::new("violations", "int"),
                    SchemaField::new("dragging", "string"),
                    // R1609 — the keyboard half, readable so an agent can see
                    // what a chord would act on and what a screen reader will
                    // say. The toolkit keeps every one of these private: the
                    // current subwindow's interactive mode, its saved geometry
                    // and its announcement (which no widget sends) are all
                    // unreachable.
                    SchemaField::new("current", "string"),
                    SchemaField::new("handle", "string"),
                    SchemaField::new("editing", "string"),
                    SchemaField::new("session_reflow", "string"),
                    SchemaField::new("announcement", "string"),
                    SchemaField::new("handles", "string"),
                    SchemaField::new("neighbours", "string"),
                    // The gestures, as verbs.
                    SchemaField::action("move_to", "string"),
                    SchemaField::action("resize", "string"),
                    SchemaField::action("compact", "string"),
                    SchemaField::action("remove", "string"),
                    SchemaField::action("send", "string"),
                    SchemaField::action("key", "string"),
                    SchemaField::action("select", "string"),
                    SchemaField::action("drag_handle", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .bound()
            .ok_or_else(|| ReadRefusal::unavailable("no dashboard state is bound"))?;
        let grid = state.grid.get();
        let count = |n: usize| Ok(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)));
        match path {
            "columns" => Ok(IntrospectValue::Int(i64::from(grid.columns()))),
            "row_count" => Ok(IntrospectValue::Int(i64::from(Self::painted_rows(&grid)))),
            "tile_count" => count(grid.tiles().len()),
            "layout" => Ok(
                serde_json::to_value(&grid).map_or(IntrospectValue::Null, IntrospectValue::Json)
            ),
            "tiles" => Ok(IntrospectValue::Text(Self::tiles_text(&grid))),
            "last_reflow" => Ok(IntrospectValue::Text(state.last_reflow.get())),
            "last_refusal" => Ok(IntrospectValue::Text(state.refusal.get())),
            "violations" => count(grid.violations().len()),
            "dragging" => Ok(IntrospectValue::Text(
                state
                    .grab
                    .get()
                    .map_or_else(String::new, |g| g.id.to_string()),
            )),
            // R1609 — the roving current card, defaulted the same way a chord
            // defaults it so the wire and the keyboard never disagree about what
            // is selected.
            "current" => Ok(IntrospectValue::Text(
                Self::current_id(state).map_or_else(String::new, |id| id.to_string()),
            )),
            // Which handle the live press grabbed, empty for an interior drag
            // or no press. `Operation` is private, so the toolkit cannot be asked this
            // at all.
            "handle" => Ok(IntrospectValue::Text(
                state
                    .grab
                    .get()
                    .and_then(|g| g.handle)
                    .map_or_else(String::new, |h| format!("{h:?}")),
            )),
            // The open session's card, empty when no edit is in flight — the
            // difference between "Escape restores something" and "Escape is inert".
            "editing" => Ok(IntrospectValue::Text(
                state
                    .session
                    .get()
                    .map_or_else(String::new, |s| s.id().to_string()),
            )),
            // What the whole session has displaced, as a DIFFERENCE against the
            // arrangement it opened on rather than a sum over its chords.
            "session_reflow" => Ok(IntrospectValue::Text(
                state
                    .session
                    .get()
                    .map_or_else(String::new, |s| Self::reflow_text(&s.reflow(&grid))),
            )),
            // The live region's text: what an AT will say. Readable because a
            // live region is declared, where the toolkit's announcement is a
            // fired event that leaves nothing behind to ask about.
            "announcement" => Ok(IntrospectValue::Text(state.announcement.get())),
            // Every handle a card offers, with the cursor each asks for —
            // derived from `TileHandle::ALL`, so a client can enumerate the resize
            // affordances. The toolkit's operation map is private and its enum
            // is in a `_p.h`.
            "handles" => Ok(IntrospectValue::Text(
                TileHandle::ALL
                    .iter()
                    .map(|h| format!("{h:?}:{:?}", h.cursor()))
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            // Where each arrow key would take the selection — the spatial
            // relation `activateNextSubWindow` does not have.
            "neighbours" => Self::current_id(state)
                .map(|id| {
                    IntrospectValue::Text(
                        TileDirection::ALL
                            .iter()
                            .map(|dir| {
                                let to = grid
                                    .neighbour(&id, *dir)
                                    .map_or("-", |t| t.id.as_str())
                                    .to_owned();
                                format!("{dir:?}:{to}")
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                })
                .ok_or_else(|| ReadRefusal::unavailable("no tile is current")),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    /// Read-only over every slot: a gesture is a **verb**, so nothing here is a
    /// writable field — and a read-only refusal is told apart from an unknown
    /// path, which is R1566's rule rather than this binding's invention.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if self.query(path).is_ok() || self.schema().field_for(path).is_some() {
            return Err(InterveneError::ReadOnly);
        }
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self
            .bound()
            .ok_or_else(|| InvokeError::Rejected("the board is not attached yet".into()))?;
        match path {
            "move_to" | "resize" => {
                let (id, a, b) = Self::parse_triple(&args).ok_or_else(|| {
                    InvokeError::Rejected(
                        if path == "move_to" {
                            "expected \"<id>,<col>,<row>\""
                        } else {
                            "expected \"<id>,<w>,<h>\""
                        }
                        .into(),
                    )
                })?;
                let moving = path == "move_to";
                Self::edit(state, |grid| {
                    if moving {
                        grid.move_to(&id, a, b)
                    } else {
                        grid.resize(&id, a, b)
                    }
                })
                .map(IntrospectValue::Text)
                .map_err(|sentence| InvokeError::Rejected(sentence.into()))
            }
            "compact" => {
                let mut grid = state.grid.get();
                let reflow = grid.compact();
                state.grid.set(grid);
                let text = Self::reflow_text(&reflow);
                state.last_reflow.set(text.clone());
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(text))
            }
            "remove" => {
                let IntrospectValue::Text(id) = &args else {
                    return Err(InvokeError::Rejected("expected \"<id>\"".into()));
                };
                let mut grid = state.grid.get();
                match grid.remove(&TileId::new(id.trim())) {
                    Ok(tile) => {
                        state.grid.set(grid);
                        state.last_reflow.set("clean".to_owned());
                        state.refusal.set(String::new());
                        Ok(IntrospectValue::Text(tile.id.to_string()))
                    }
                    Err(error) => {
                        let sentence = error.to_string();
                        state.refusal.set(sentence.clone());
                        Err(InvokeError::Rejected(sentence.into()))
                    }
                }
            }
            "send" => {
                if let IntrospectValue::Text(raw) = &args {
                    // R1650 §5.35 — the router composes the R51.42 payload
                    // `"<sub>:<EventName>"` when the pressed tag is composite,
                    // and a bare `"<EventName>"` when it is not. Both arrive
                    // here now that a card is `dashboard#card.loss`, so this
                    // decodes through the `:` grammar SSOT and falls back to the
                    // raw string — the shape the widget catalog already uses.
                    // Matching the raw string alone silently ignored every press
                    // the router delivered, which is how a board that finally
                    // received its input still did nothing with it.
                    let event = pinion_core::composite_tag::split_send_payload(raw)
                        .map_or(raw.as_str(), |sent| sent.event);
                    match event {
                        "PointerDown" => state.pending.set(true),
                        "PointerUp" | "PointerLeave" | "PointerCancel" => {
                            state.pending.set(false);
                            state.grab.set(None);
                            // R1609 — a release commits the pointer drag's
                            // session, so Escape *during* a drag cancels it the
                            // same way it cancels a keyboard edit. One undo
                            // point for both entry points rather than one each.
                            state.session.set(None);
                        }
                        _ => {}
                    }
                }
                Ok(IntrospectValue::Null)
            }
            // R1609 — the keyboard, as a verb. The chord arrives spelled exactly
            // as the platform path spells it (`Alt+Shift+ArrowLeft`), so an agent
            // drives the same keymap a person does rather than a parallel one.
            "key" => {
                let IntrospectValue::Text(chord) = &args else {
                    return Err(InvokeError::Rejected(
                        "expected a chord, e.g. \"Alt+ArrowRight\"".into(),
                    ));
                };
                Ok(IntrospectValue::Bool(Self::key(state, chord)))
            }
            // Move the roving selection outright. Refused by name for a card that
            // is not there, so a stale id cannot silently select nothing.
            "select" => {
                let IntrospectValue::Text(id) = &args else {
                    return Err(InvokeError::Rejected("expected \"<id>\"".into()));
                };
                let id = TileId::new(id.trim());
                if state.grid.get().tile(&id).is_none() {
                    let sentence = format!("no tile {id} in this grid");
                    state.refusal.set(sentence.clone());
                    return Err(InvokeError::Rejected(sentence.into()));
                }
                state.current.set(Some(id.clone()));
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(id.to_string()))
            }
            // A handle drag as one call. The pointer path resolves the handle
            // from where the press landed; this names it, so a client can
            // exercise all eight without pixels.
            "drag_handle" => Self::invoke_drag_handle(state, &args),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The board's paint: one grid container, one child per card.
///
/// Every card's placement comes from [`TileGrid::placement`] — the one place the
/// zero-based model becomes CSS's one-based lines — so the painter cannot add
/// one separately and disagree with the model, the wire and the AT tree.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the view-fn signature `WidgetCore::view` hands down is `&Frame`; \
              taking it by value here would put a copy at the one call site"
)]
fn view(
    grid: &TileGrid,
    rows: u32,
    dragging: Option<&TileId>,
    current: Option<&TileId>,
    _frame: &Frame,
) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let card = theme.resolve(ColorRole::Outline);
    let accent = theme.resolve(ColorRole::Accent);
    let ink = theme.resolve(ColorRole::OnSurface);

    let mut children = Vec::with_capacity(grid.tiles().len());
    for tile in grid.tiles() {
        let Some((col, row)) = grid.placement(&tile.id) else {
            continue;
        };
        let held = dragging == Some(&tile.id);
        let selected = current == Some(&tile.id);
        let mut card_children = vec![Scene::Text(
            TextNode::styled(
                format!("{}  {}x{}", tile.id, tile.w, tile.h),
                Rect::default(),
                TextStyle::new().with_size_px(CARD_FONT_PX).with_fg(ink),
            )
            .with_tag(sub_tag(&format!("card.{}.label", tile.id))),
        )];
        // R1609 — the handle ring, painted by iterating `TileHandle::ALL` rather
        // than spelling eight positions out. Each one carries the cursor its own
        // handle derives, so a corner asks for the diagonal `CursorHint` this
        // round added and a side asks for the axis arrow. Only the selected card
        // shows them, which is what keeps a twelve-card board from painting
        // ninety-six grips.
        if selected {
            card_children.push(handle_ring(&tile.id, accent));
        }
        children.push(Scene::Container(
            ContainerNode::new(card_children)
                .with_tag(sub_tag(&format!("card.{}", tile.id)))
                .with_style(
                    BoxStyle::filled(if held { accent } else { card }).with_corner_radius(6),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_grid_column(col)
                        .with_grid_row(row)
                        .with_padding(Rect::new(8, 8, 8, 8)),
                ),
        ));
    }

    Scene::Container(
        ContainerNode::new(vec![
            Scene::Text(
                TextNode::styled(
                    "pinion tile dashboard",
                    Rect::default(),
                    TextStyle::new().with_size_px(TITLE_FONT_PX).with_fg(ink),
                )
                .with_tag("title"),
            ),
            Scene::Container(
                ContainerNode::new(children).with_tag(ROOT_TAG).with_layout(
                    LayoutStyle::new()
                        // ★★★★★ R2013 — `minmax(0, 1fr)` and not `1fr`, which
                        // is the one place on this screen the difference is
                        // visible. A bare `1fr` column carries CSS's implicit
                        // `auto` minimum, so the WIDEST CARD LABEL IN A COLUMN
                        // sets that column's width and the other eleven share
                        // what is left — a board whose geometry depends on how
                        // long somebody named a card. The floor says the twelve
                        // columns are twelve equal shares and a card that does
                        // not fit is the card's problem, which is what a
                        // dashboard grid means. Until this round the vocabulary
                        // could not say it: see `GridTrack::MinMax`.
                        .grid_columns(vec![
                            GridTrack::MinMax {
                                min: TrackMin::Px(0),
                                max: TrackMax::Fr(1.0),
                            };
                            COLUMNS as usize
                        ])
                        .with_grid_rows(vec![GridTrack::Px(ROW_H); rows as usize])
                        // R1609 — ONE Tab stop for the whole board. The selected
                        // card is its active descendant, so thirty cards are not
                        // thirty stops in the focus ring.
                        .with_focusable(true),
                ),
            ),
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(8)
                .with_padding(Rect::new(12, 12, 12, 12)),
        ),
    )
}

/// (R1609) The whole grip ring for one card, as a 3x3 grid overlay.
///
/// ★ **The first draft placed each grip at an absolutely-positioned pixel inset
/// off two stated constants, and the round's own close audit found that wrong**:
/// a card's resolved width is a layout-pass output, so any inset a view function
/// can state is a guess that happens to fit one card size — the right-hand grips
/// of a twelve-column card would have floated somewhere in its middle. The
/// framework cannot express it that way either, because
/// [`LayoutStyle::absolute_position`] anchors left/top only and CSS's `right: 0`
/// has no analogue here.
///
/// A grid says it exactly and says it in one place: three tracks per axis, the
/// outer two a grip thick and the middle one `Fr(1.0)`, so the ring is on the
/// card's edges at every size with no arithmetic to be wrong. That makes this
/// R1560's grid used a second time inside one round — the tile board's layout is
/// the first — and the ring is one overlay node rather than eight positioned
/// ones, so it can be turned on and off without touching the card's own content.
///
/// Every grip's grid cell **and** its [`CursorHint`](pinion_core::style::CursorHint)
/// come from [`TileHandle::horizontal`] / [`TileHandle::vertical`], so a ninth handle would need no new painting code
/// and a grip cannot sit on a side its own drag does not move. The toolkit's
/// nine `operationMap.insert` rows pair a private region with a cursor by hand, and `updateDirtyRegions` has to
/// rebuild every one of them whenever the widget's geometry changes.
fn handle_ring(id: &TileId, ink: pinion_core::style::Color) -> Scene {
    use pinion_core::widgets::tile_grid::TileEdge;

    /// 1-based CSS grid line for an axis: the near track, the middle, or the far.
    fn line(edge: Option<TileEdge>) -> u16 {
        match edge {
            Some(TileEdge::Left | TileEdge::Top) => 1,
            None => 2,
            Some(_) => 3,
        }
    }

    let grips = TileHandle::ALL
        .into_iter()
        .map(|handle| {
            Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_tag(sub_tag(&format!("card.{id}.handle.{handle:?}")))
                    .with_style(BoxStyle::filled(ink).with_corner_radius(2))
                    .with_layout(
                        LayoutStyle::new()
                            .with_grid_column(GridPlacement::at(line(handle.horizontal())))
                            .with_grid_row(GridPlacement::at(line(handle.vertical())))
                            .with_cursor(handle.cursor()),
                    ),
            )
        })
        .collect();

    // ★ The tracks are the HIT-TEST BAND, not a pixel. A grip drawn a fixed
    // eight pixels thick beside a band that is a quarter of the card is two
    // numbers describing one thing, and they disagree at every card size but
    // one: on a wide card a strip resizes with no grip under it, and on a narrow
    // one part of the painted grip does not resize. Fractional tracks make the
    // ring the band by construction, on both axes and at any size.
    let tracks = || {
        vec![
            GridTrack::Fr(HANDLE_BAND),
            GridTrack::Fr(HANDLE_CORE),
            GridTrack::Fr(HANDLE_BAND),
        ]
    };
    Scene::Container(
        ContainerNode::new(grips)
            .with_tag(sub_tag(&format!("card.{id}.handles")))
            .with_layout(
                LayoutStyle::new()
                    // ★★★★★ R2033 — the ring is the CARD'S OWN EXTENT, stated
                    // as a share rather than as a guess. What stood here relied
                    // on a sentence that is false: an `Auto` size on an absolute
                    // child does NOT resolve to the parent's content rect, it
                    // shrinks to fit — and this ring's content is fractional
                    // tracks, which resolve to nothing against an indefinite
                    // size. So the ring was `0 x 0`, eight grips of it, and the
                    // resize band a person aims at was not drawn at all.
                    .with_absolute_position(0, 0)
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Percent(100))
                            .with_height(SizeValue::Percent(100)),
                    )
                    .grid_columns(tracks())
                    .with_grid_rows(tracks()),
            ),
    )
}

struct DashboardView;

impl WidgetCore for DashboardView {
    /// `()` — the arrangement is a `Vec`, and `State` must be `Copy`. The model
    /// lives in [`use_board_state`], which the oracle and the view share, so
    /// there is one arrangement rather than a copy per reader
    /// (`hello-node-groups`' shape).
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = DashboardOracle::new();
        oracle.attach(use_board_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), frame: &Frame) -> Scene {
        let state = use_board_state();
        let grid = state.grid.get();
        let rows = DashboardOracle::painted_rows(&grid);
        let dragging = state.grab.get().map(|g| g.id);
        let current = DashboardOracle::current_id(&state);
        view(&grid, rows, dragging.as_ref(), current.as_ref(), frame)
    }

    /// (R1609) The keyboard, routed to the External's own `key` verb.
    ///
    /// Through the verb rather than into the state directly, so the platform
    /// path, the assistive-technology path and an RPC client all drive **one**
    /// keymap — §2 #2. The chord is spelled here because `forward_key_to_external`
    /// carries no modifiers and this board's vocabulary is three-quarters
    /// modified.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        if focused != Some(ROOT_TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke(
                "key",
                IntrospectValue::Text(DashboardOracle::chord(key, modifiers)),
            ),
            Ok(IntrospectValue::Bool(true))
        )
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-tile-dashboard (R1608 §5.21 drag-and-snap tile grid)"
    }
}

impl WidgetA11y for DashboardView {
    /// The board as an `application` whose value counts the cards, plus one
    /// `group` per card **naming its slot** — so a screen-reader user can read
    /// the arrangement, which a dashboard tool board does not expose at all.
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_board_state();
        let grid = state.grid.get();
        let current = DashboardOracle::current_id(&state);
        // R1609 — every card is declared a child of the board, which is what
        // makes the roving `aria-activedescendant` resolvable: the shell looks
        // the active descendant up among the parent's declared children.
        let board = grid.tiles().iter().fold(
            AccessNode::new(ROOT_TAG, AriaRole::Group)
                .with_name("Tile dashboard")
                .with_value(AccessValue::Text(format!(
                    "{} card(s) on {} columns",
                    grid.tiles().len(),
                    grid.columns()
                )))
                .with_state(AccessState {
                    focused: focused == Some(ROOT_TAG),
                    ..AccessState::default()
                }),
            |node, tile| node.with_child(sub_tag(&format!("card.{}", tile.id))),
        );
        let mut nodes = vec![board];
        for tile in grid.tiles() {
            let mut node = AccessNode::new(sub_tag(&format!("card.{}", tile.id)), AriaRole::Group)
                .with_name(tile.id.to_string())
                .with_value(AccessValue::Text(format!(
                    "column {}, row {}, {} by {}",
                    tile.col + 1,
                    tile.row + 1,
                    tile.w,
                    tile.h
                )));
            // R1609 — `aria-selected` on the roving current card. The toolkit's `state()`
            // advertises `movable` and `sizeable` and then nothing tells an AT which
            // subwindow the keyboard would act on, because interactive mode is
            // private.
            if current.as_ref() == Some(&tile.id) {
                node = node.with_selected(true);
            }
            nodes.push(node);
        }
        // R1609 — ★ the announcement channel. The cards an edit displaces are by
        // definition not the one the user is on, so a per-card value cannot carry
        // the fact; a `polite` live region can, because an AT re-reads it when it
        // changes without anyone navigating there.
        //
        // The toolkit has the capability (accessible announcement event, 6.8+)
        // and no widget uses it: the three translation units implementing its
        // MDI child window, its MDI area and its size grip contain no
        // accessibility notification at all, so a toolkit MDI window that moves
        // is silent.
        nodes.push(
            AccessNode::new(REFLOW_TAG, AriaRole::Status)
                .with_name("Layout change")
                .with_live(AccessLive::Polite)
                .with_value(AccessValue::Text(state.announcement.get())),
        );
        nodes
    }

    /// (R1609) The board owns the focus; the selected card is its active
    /// descendant — the roving pattern, so the focus ring frames the current card
    /// while the Tab ring holds one stop.
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(ROOT_TAG) {
            return focused.map(AccessFocus::atomic);
        }
        let state = use_board_state();
        Some(AccessFocus {
            focus_tag: ROOT_TAG.to_owned(),
            active_descendant: DashboardOracle::current_id(&state)
                .map(|id| sub_tag(&format!("card.{id}"))),
        })
    }
}

impl WidgetView for DashboardView {
    type Renderer = HelloTileDashboardRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DashboardView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// The node carrying `tag`, anywhere under `scene`.
    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
            Scene::Scroll(s) => find(&s.content, tag),
            _ => None,
        }
    }

    /// ★★★★★ (R2033) The grip ring's whole claim is that it IS the card's
    /// extent at any card size, and until this test nothing checked it — the
    /// ring's own doc asserted it in prose while the ring was measured `0 x 0`
    /// and its eight grips were not drawn at all. The defect survived because
    /// every other test here drives the ORACLE: a handle is enumerable, named,
    /// and drivable through arithmetic that never consults the painted ring, so
    /// the resize band worked perfectly with nothing under the cursor.
    ///
    /// So this places the real view through the real taffy pass and compares
    /// two rects. An `Auto` size cannot pass it: fractional tracks resolve to
    /// nothing against an indefinite size, which is exactly how the ring came
    /// to be a zero.
    #[test]
    fn r2033_the_grip_ring_is_the_card_it_rings() {
        let (card, ring) = Owner::new().run(|| {
            let mut oracle = DashboardOracle::new();
            oracle.attach(use_board_state());
            let selected = match oracle.query("current") {
                Ok(IntrospectValue::Text(id)) => id,
                other => panic!("current answered {other:?}"),
            };
            assert!(
                !selected.is_empty(),
                "a card must be selected for the ring to be built at all"
            );
            let mut scene = <DashboardView as WidgetCore>::view((), &Frame::new());
            let mut cache = pinion_text::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
            let rect_of = |tag: &str| match find(&scene, tag) {
                Some(node) => node.rect(),
                None => panic!("{tag} is in the scene"),
            };
            (
                rect_of(&sub_tag(&format!("card.{selected}"))),
                rect_of(&sub_tag(&format!("card.{selected}.handles"))),
            )
        });
        assert!(
            ring.w > 0 && ring.h > 0,
            "the ring must have an extent to be drawn or found at all, got {ring:?}"
        );
        assert_eq!(
            (ring.w, ring.h),
            (card.w, card.h),
            "the ring is the card's own extent: card {card:?}, ring {ring:?}"
        );
    }

    /// An attached oracle inside a live `Owner` scope — the same binding the
    /// shell builds, so a test drives the real thing rather than a stand-in.
    fn drive<T>(body: impl FnOnce(&mut DashboardOracle) -> T) -> T {
        Owner::new().run(|| {
            let mut oracle = DashboardOracle::new();
            oracle.attach(use_board_state());
            body(&mut oracle)
        })
    }

    fn text(oracle: &DashboardOracle, path: &str) -> String {
        match oracle.query(path) {
            Ok(IntrospectValue::Text(s)) => s,
            other => panic!("{path} answered {other:?}"),
        }
    }

    fn press(oracle: &mut DashboardOracle) {
        oracle
            .invoke("send", IntrospectValue::Text("PointerDown".to_owned()))
            .unwrap();
    }

    /// The width the board paints at in a 760px window — only the horizontal
    /// axis reads it, and only as a fraction, so it is the one measured number
    /// these readings need.
    const BOARD_W: f32 = 736.0;

    /// A reading at `at` over a board painting `rows` rows, the way the router
    /// hands one over.
    ///
    /// ★ R1727 — `rows` is an ARGUMENT because it is a property of the
    /// **rectangle**, not of the model. A test that states the wrong one is
    /// describing a board that was never painted, which is precisely the
    /// confusion the round removed from the widget: it used to take the fraction
    /// from the paint and the row count from the model and multiply them
    /// together.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a dashboard's row count round-trips f32 exactly"
    )]
    fn over(rows: u32, at: (f32, f32)) -> PointerReading {
        PointerReading::new(at, (BOARD_W, rows as f32 * ROW_H as f32))
    }

    #[test]
    fn the_seed_board_is_the_arrangement_the_layout_measurement_lays_out() {
        drive(|oracle| {
            assert_eq!(oracle.query("columns"), Ok(IntrospectValue::Int(12)));
            assert_eq!(oracle.query("tile_count"), Ok(IntrospectValue::Int(5)));
            assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
            assert_eq!(
                text(oracle, "tiles"),
                "throughput@0,0+12x1 latency@0,1+6x1 loss@6,1+6x1 topology@0,2+4x2 alarms@4,2+8x1"
            );
            assert_eq!(
                oracle.query("row_count"),
                Ok(IntrospectValue::Int(5)),
                "four rows of cards, floored at MIN_ROWS so there is board to drag into"
            );
        });
    }

    #[test]
    fn the_whole_arrangement_is_one_json_read() {
        drive(|oracle| {
            let Ok(IntrospectValue::Json(value)) = oracle.query("layout") else {
                panic!("layout is a document")
            };
            let back: TileGrid = serde_json::from_value(value).unwrap();
            assert_eq!(back.tiles().len(), 5);
            assert!(back.violations().is_empty());
        });
    }

    #[test]
    fn a_move_verb_reports_what_it_displaced() {
        drive(|oracle| {
            let reply = oracle
                .invoke("move_to", IntrospectValue::Text("topology,0,0".to_owned()))
                .unwrap();
            assert_eq!(
                reply,
                IntrospectValue::Text("throughput:0>2, latency:1>3, alarms:2>4".to_owned()),
                "the displaced cards are NAMED, which the dashboard tool cannot answer"
            );
            assert_eq!(
                text(oracle, "last_reflow"),
                "throughput:0>2, latency:1>3, alarms:2>4",
                "the reflow is TRANSITIVE: alarms was pushed by the throughput \
                 that topology pushed, and then again by the latency below it"
            );
            assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn a_move_that_fits_is_clean() {
        drive(|oracle| {
            oracle
                .invoke("move_to", IntrospectValue::Text("topology,8,3".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "last_reflow"), "clean");
        });
    }

    #[test]
    fn a_refused_gesture_says_why_and_changes_nothing() {
        drive(|oracle| {
            let before = text(oracle, "tiles");
            let wide = oracle.invoke("resize", IntrospectValue::Text("latency,20,1".to_owned()));
            assert!(matches!(wide, Err(InvokeError::Rejected(_))));
            assert_eq!(
                text(oracle, "last_refusal"),
                "a tile 20 columns wide does not fit a grid of 12"
            );
            let ghost = oracle.invoke("move_to", IntrospectValue::Text("ghost,0,0".to_owned()));
            assert!(matches!(ghost, Err(InvokeError::Rejected(_))));
            assert_eq!(text(oracle, "last_refusal"), "no tile ghost in this grid");
            assert_eq!(text(oracle, "tiles"), before, "no refusal moved a card");
        });
    }

    #[test]
    fn a_malformed_argument_is_refused_apart_from_a_wrong_one() {
        drive(|oracle| {
            match oracle.invoke("move_to", IntrospectValue::Text("latency,x".to_owned())) {
                Err(InvokeError::Rejected(reason)) => {
                    assert!(reason.to_string().contains("expected"), "{reason:?}");
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn compaction_is_a_verb_here_too_and_removing_a_card_leaves_its_gap() {
        drive(|oracle| {
            oracle
                .invoke("remove", IntrospectValue::Text("throughput".to_owned()))
                .unwrap();
            assert!(
                text(oracle, "tiles").starts_with("latency@0,1"),
                "the gap stays until someone asks for it to close"
            );
            oracle.invoke("compact", IntrospectValue::Null).unwrap();
            assert!(text(oracle, "tiles").starts_with("latency@0,0"));
            assert_eq!(
                text(oracle, "last_reflow"),
                "latency:1>0, loss:1>0, topology:2>1, alarms:2>1"
            );
        });
    }

    #[test]
    fn a_press_drag_keeps_the_card_under_the_finger() {
        // ★ The gesture R1607 could not test without a consumer. Grab
        // `topology` (columns 0..4 on rows 2..4) TWO columns in from its left
        // edge, then drag the cursor to column 5: the card's left edge must
        // land at 3, not at the cursor's column 5.
        //
        // `alarms` was the first choice and it could not move at all — eight
        // columns wide in a grid of twelve pins it to column 4, which the
        // model's own clamp does correctly. The fixture had to be a card
        // narrow enough to have somewhere to go.
        drive(|oracle| {
            press(oracle);
            oracle.pointer_move(over(5, (2.5 / 12.0, 2.5 / 5.0)));
            assert_eq!(text(oracle, "dragging"), "topology");

            oracle.pointer_move(over(5, (5.5 / 12.0, 2.5 / 5.0)));
            assert!(
                text(oracle, "tiles").contains("topology@3,2+4x2"),
                "the grab offset moved with the card — a corner-snap would have \
                 put it at column 5. Got {}",
                text(oracle, "tiles")
            );

            oracle
                .invoke("send", IntrospectValue::Text("PointerUp".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "dragging"), "", "the release let go");
            let settled = text(oracle, "tiles");
            oracle.pointer_move(over(5, (0.05, 0.05)));
            assert_eq!(
                text(oracle, "tiles"),
                settled,
                "and a move after the release moves nothing"
            );
        });
    }

    #[test]
    fn a_press_on_empty_board_latches_nothing() {
        drive(|oracle| {
            press(oracle);
            // Row 4 is below every card.
            oracle.pointer_move(over(5, (0.5, 4.5 / 5.0)));
            assert_eq!(text(oracle, "dragging"), "");
            let before = text(oracle, "tiles");
            oracle.pointer_move(over(5, (0.1, 0.1)));
            assert_eq!(text(oracle, "tiles"), before);
        });
    }

    #[test]
    fn a_drag_displaces_and_the_board_stays_legal() {
        drive(|oracle| {
            press(oracle);
            oracle.pointer_move(over(5, (0.02, 2.5 / 5.0))); // grab `topology`'s corner
            assert_eq!(text(oracle, "dragging"), "topology");
            oracle.pointer_move(over(5, (0.02, 0.02))); // drag it to the top-left
            assert!(text(oracle, "tiles").contains("topology@0,0"));
            assert_ne!(text(oracle, "last_reflow"), "clean");
            assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn every_slot_is_read_only_and_an_unknown_one_is_told_apart() {
        drive(|oracle| {
            assert_eq!(
                oracle.intervene("tiles", IntrospectValue::Text(String::new())),
                Err(InterveneError::ReadOnly),
                "a gesture is a verb, so no slot here is writable"
            );
            assert_eq!(
                oracle.intervene("nonesuch", IntrospectValue::Text(String::new())),
                Err(InterveneError::UnknownPath)
            );
            assert!(matches!(
                oracle.invoke("nonesuch", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            ));
        });
    }

    #[test]
    fn the_at_tree_reads_the_same_slots_the_painter_places_from() {
        Owner::new().run(|| {
            let nodes = DashboardView::access_node(&(), Some(ROOT_TAG));
            assert_eq!(
                nodes.len(),
                7,
                "the board, one group per card, and R1609's live region — which \
                 is the seventh and is why this count moved"
            );
            let alarms = nodes
                .iter()
                .find(|n| n.tag == sub_tag("card.alarms"))
                .expect("a node per card");
            assert_eq!(
                alarms.value,
                Some(AccessValue::Text("column 5, row 3, 8 by 1".to_owned())),
                "the AT tree states CSS's one-based lines, derived from the same \
                 placement the painter uses"
            );
        });
    }

    // ---- R1609: the board is editable without a pointer ---------------------

    fn key(oracle: &mut DashboardOracle, chord: &str) -> bool {
        matches!(
            oracle.invoke("key", IntrospectValue::Text(chord.to_owned())),
            Ok(IntrospectValue::Bool(true))
        )
    }

    #[test]
    fn r1609_plain_arrows_move_the_selection_spatially() {
        drive(|oracle| {
            // The seed board: throughput full width on row 0, latency|loss on
            // row 1, topology (0..4) and alarms (4..12) on row 2.
            assert_eq!(
                text(oracle, "current"),
                "throughput",
                "a fresh board selects its first card so a Tab into it can act"
            );
            assert!(key(oracle, "ArrowDown"));
            assert_eq!(text(oracle, "current"), "latency");
            assert!(key(oracle, "ArrowRight"));
            assert_eq!(
                text(oracle, "current"),
                "loss",
                "Right stays in the row band rather than jumping to a card that \
                 happens to start at a lower column further down"
            );
            assert!(key(oracle, "ArrowDown"));
            assert_eq!(text(oracle, "current"), "alarms");
            assert!(key(oracle, "ArrowLeft"));
            assert_eq!(text(oracle, "current"), "topology");

            // Nothing that way: the selection holds rather than wrapping to
            // somewhere the arrow did not point.
            assert!(key(oracle, "ArrowLeft"));
            assert_eq!(text(oracle, "current"), "topology");
            assert_eq!(
                text(oracle, "tiles"),
                "throughput@0,0+12x1 latency@0,1+6x1 loss@6,1+6x1 topology@0,2+4x2 alarms@4,2+8x1",
                "navigating moved no card"
            );
        });
    }

    #[test]
    fn r1609_every_arrow_from_a_card_is_published() {
        drive(|oracle| {
            oracle
                .invoke("select", IntrospectValue::Text("loss".to_owned()))
                .unwrap();
            assert_eq!(
                text(oracle, "neighbours"),
                "Left:latency Right:- Up:throughput Down:alarms",
                "the spatial relation `activateNextSubWindow` does not have"
            );
        });
    }

    #[test]
    fn r1609_the_keymap_has_no_mode_and_reaches_all_twelve_nudges() {
        drive(|oracle| {
            oracle
                .invoke("select", IntrospectValue::Text("topology".to_owned()))
                .unwrap();

            // Shift = Move, Alt = Grow, Alt+Shift = Shrink. The same four arrow
            // keys, and which one it is never depends on invisible state.
            assert!(key(oracle, "Shift+ArrowRight"));
            assert!(text(oracle, "tiles").contains("topology@1,2+4x2"));
            assert!(key(oracle, "Alt+ArrowRight"));
            assert!(text(oracle, "tiles").contains("topology@1,2+5x2"));
            assert!(key(oracle, "Alt+Shift+ArrowRight"));
            assert!(text(oracle, "tiles").contains("topology@1,2+4x2"));
            assert!(key(oracle, "Alt+ArrowLeft"));
            assert!(
                text(oracle, "tiles").contains("topology@0,2+5x2"),
                "growing the LEFT side changed the column and the width together \
                 — one edge moved, the other held. Got {}",
                text(oracle, "tiles")
            );
            assert!(key(oracle, "Alt+ArrowDown"));
            assert!(text(oracle, "tiles").contains("topology@0,2+5x3"));
            assert!(key(oracle, "Alt+Shift+ArrowUp"));
            assert!(text(oracle, "tiles").contains("topology@0,3+5x2"));
            assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn r1609_escape_restores_the_whole_board_and_enter_commits() {
        drive(|oracle| {
            let before = text(oracle, "tiles");
            oracle
                .invoke("select", IntrospectValue::Text("topology".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "editing"), "", "no session until an edit");

            for _ in 0..2 {
                assert!(key(oracle, "Shift+ArrowUp"));
            }
            assert_eq!(
                text(oracle, "editing"),
                "topology",
                "the first editing chord opened the session — no menu round trip, \
                 which is how the toolkit enters interactive mode"
            );
            assert_ne!(text(oracle, "session_reflow"), "clean");
            assert_ne!(text(oracle, "tiles"), before);

            assert!(key(oracle, "Escape"));
            assert_eq!(
                text(oracle, "tiles"),
                before,
                "★ Escape restored the cards the edit DISPLACED as well as the one \
                 being edited; the toolkit saves `oldGeometry` and never reads it back"
            );
            assert_eq!(text(oracle, "editing"), "");
            assert!(text(oracle, "announcement").contains("restored"));

            // And Enter keeps the edit, closing the session.
            assert!(key(oracle, "Shift+ArrowUp"));
            let moved = text(oracle, "tiles");
            assert!(key(oracle, "Enter"));
            assert_eq!(text(oracle, "editing"), "");
            assert_eq!(text(oracle, "tiles"), moved);
            assert!(text(oracle, "announcement").contains("committed"));
            // Escape after a commit has nothing to restore, and says so by
            // leaving the board alone rather than reverting an older session.
            assert!(key(oracle, "Escape"));
            assert_eq!(text(oracle, "tiles"), moved);
        });
    }

    #[test]
    fn r1609_a_session_reflow_is_a_difference_over_the_whole_session() {
        drive(|oracle| {
            oracle
                .invoke("select", IntrospectValue::Text("throughput".to_owned()))
                .unwrap();
            // Walk the full-width header DOWN through the board: it collides with
            // a card on every press, so a per-press sum would count that card
            // repeatedly.
            for _ in 0..3 {
                assert!(key(oracle, "Shift+ArrowDown"));
            }
            let session = text(oracle, "session_reflow");
            let per_press = text(oracle, "last_reflow");
            assert!(
                session.matches("latency").count() == 1,
                "a card that moved several times appears ONCE in the session's \
                 difference: {session}"
            );
            assert!(
                session.contains("latency:1>"),
                "and it names where the session STARTED, not the last hop: {session}"
            );
            assert_ne!(
                session, per_press,
                "the session's difference and the last press's reflow are \
                 different answers ({session} vs {per_press})"
            );
        });
    }

    #[test]
    fn r1609_a_chord_at_a_bound_stops_and_says_so() {
        drive(|oracle| {
            oracle
                .invoke("select", IntrospectValue::Text("throughput".to_owned()))
                .unwrap();
            let before = text(oracle, "tiles");
            // Twelve columns wide in a grid of twelve: it cannot move sideways
            // and cannot grow.
            for chord in ["Shift+ArrowLeft", "Shift+ArrowUp", "Alt+ArrowRight"] {
                assert!(key(oracle, chord));
                assert_eq!(text(oracle, "tiles"), before, "{chord} moved something");
                assert!(
                    text(oracle, "announcement").contains("already at the edge"),
                    "{chord} left the announcement stale: {}",
                    text(oracle, "announcement")
                );
            }
        });
    }

    #[test]
    fn r1609_an_unknown_chord_is_declined_so_the_shell_keeps_tab() {
        drive(|oracle| {
            assert!(!key(oracle, "Tab"), "Tab belongs to the focus ring");
            assert!(!key(oracle, "PageDown"));
            assert!(!key(oracle, ""), "an empty chord names no key");
            assert!(
                !key(oracle, "Hyper+ArrowLeft"),
                "an unrecognised modifier makes the whole chord unrecognised \
                 rather than being skipped into a different gesture"
            );
            match oracle.invoke("key", IntrospectValue::Null) {
                Err(InvokeError::Rejected(why)) => {
                    assert!(why.to_string().contains("chord"), "{why:?}");
                }
                other => panic!("{other:?}"),
            }
        });
    }

    /// ★★★★★ R1727 — **the grab band is read in pixels too, and a board that
    /// has GROWN is the only fixture that can tell.**
    ///
    /// Found by a counterfactual that PASSED: putting the old expression back
    /// into [`card_fraction`](DashboardOracle::card_fraction) — a fraction times
    /// a frozen row count — changed nothing any test or demo could see. Every
    /// existing fixture drives a five-row board, where `v * 5` and
    /// `px / ROW_H` are the same number by construction, so the whole class was
    /// invisible one function over from where the round found it.
    ///
    /// Here the board is pushed to EIGHT rows first, and the press goes in the
    /// card's BOTTOM band. `v = 0.4844` of an eight-row board is 0.9375 of the
    /// card read as pixels — its bottom quarter — and 0.2109 read as `v * 5`,
    /// which is its TOP quarter. The two answers name opposite edges.
    ///
    /// ★ The first draft pressed the top band and could not tell them apart:
    /// the broken reading gave `-0.336`, and a band test asking `v < 0.25` says
    /// Top for that as happily as for `0.06`. A fixture has to make the two
    /// expressions land on DIFFERENT answers, not merely on different numbers.
    #[test]
    fn r1727_the_grab_band_is_read_in_pixels_on_a_board_that_has_grown() {
        drive(|oracle| {
            oracle
                .invoke("move_to", IntrospectValue::Text("alarms,4,7".to_owned()))
                .expect("the board has room at row 7");
            assert_eq!(
                oracle.query("row_count"),
                Ok(IntrospectValue::Int(8)),
                "the board now paints eight rows, not the seed's five"
            );

            // Select `topology` (columns 0..4, rows 2..4) by its middle.
            press(oracle);
            oracle.pointer_move(over(8, (2.0 / 12.0, 192.0 / 512.0)));
            assert_eq!(text(oracle, "dragging"), "topology");
            assert_eq!(text(oracle, "handle"), "", "the middle is not a band");
            oracle
                .invoke("send", IntrospectValue::Text("PointerUp".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "current"), "topology");

            // Now 8 px above its bottom edge — inside the card's BOTTOM band.
            press(oracle);
            oracle.pointer_move(over(8, (2.0 / 12.0, 248.0 / 512.0)));
            assert_eq!(text(oracle, "dragging"), "topology");
            assert_eq!(
                text(oracle, "handle"),
                "Bottom",
                "the press is in the card's bottom quarter read as pixels \
                 (0.9375 of it) and in its TOP quarter read as a fraction of \
                 five rows (0.2109) — only the pixel reading names the edge \
                 the person aimed at on a board that has grown"
            );
        });
    }

    #[test]
    fn r1609_a_press_on_a_handle_resizes_and_a_press_in_the_middle_moves() {
        drive(|oracle| {
            // `topology` covers columns 0..4 on rows 2..4 of a five-row board.
            // ★ A grip is live only where one is PAINTED, so the first press on
            // an unselected card is a move even on its edge — select, then grab.
            press(oracle);
            oracle.pointer_move(over(5, (3.9 / 12.0, 2.5 / 5.0)));
            assert_eq!(text(oracle, "dragging"), "topology");
            assert_eq!(
                text(oracle, "handle"),
                "",
                "an unselected card shows no grip, so its edge does not resize"
            );
            oracle
                .invoke("send", IntrospectValue::Text("PointerUp".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "current"), "topology", "the press selected it");

            // Now its RIGHT edge: u lands past 1 - HANDLE_BAND.
            press(oracle);
            oracle.pointer_move(over(5, (3.9 / 12.0, 2.5 / 5.0)));
            assert_eq!(
                text(oracle, "handle"),
                "Right",
                "the press landed in the right band of the selected card"
            );

            // Drag to column 7: the right edge follows and the left holds.
            oracle.pointer_move(over(5, (7.5 / 12.0, 2.5 / 5.0)));
            assert!(
                text(oracle, "tiles").contains("topology@0,2+8x2"),
                "a resize needs no grab offset — the dragged side tracks the \
                 cursor. Got {}",
                text(oracle, "tiles")
            );
            oracle
                .invoke("send", IntrospectValue::Text("PointerUp".to_owned()))
                .unwrap();

            // Now the middle of the same card: no handle, so it moves.
            press(oracle);
            oracle.pointer_move(over(5, (4.0 / 12.0, 2.5 / 5.0)));
            assert_eq!(
                text(oracle, "handle"),
                "",
                "the interior is a drag, not a resize"
            );
            oracle.pointer_move(over(5, (5.0 / 12.0, 2.5 / 5.0)));
            assert!(
                text(oracle, "tiles").contains("topology@1,2+8x2"),
                "the card moved and kept its size. Got {}",
                text(oracle, "tiles")
            );
            assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn r1609_escape_cancels_a_pointer_drag_too() {
        drive(|oracle| {
            let before = text(oracle, "tiles");
            press(oracle);
            oracle.pointer_move(over(5, (0.02, 2.5 / 5.0)));
            assert_eq!(text(oracle, "editing"), "topology");
            oracle.pointer_move(over(5, (0.02, 0.02)));
            assert_ne!(text(oracle, "tiles"), before);
            assert!(key(oracle, "Escape"));
            assert_eq!(
                text(oracle, "tiles"),
                before,
                "one undo point serves both entry points rather than one each"
            );
        });
    }

    #[test]
    fn r1609_all_eight_handles_are_enumerable_and_drivable_by_name() {
        drive(|oracle| {
            let handles = text(oracle, "handles");
            for name in [
                "Left",
                "Right",
                "Top",
                "Bottom",
                "TopLeft",
                "TopRight",
                "BottomLeft",
                "BottomRight",
            ] {
                assert!(handles.contains(name), "{name} missing from {handles}");
            }
            assert!(
                handles.contains("TopLeft:NwseResize") && handles.contains("TopRight:NeswResize"),
                "each handle publishes the cursor it derives: {handles}"
            );

            // Every one is drivable, which is what makes a corner testable at all
            // — the pointer path can only ever produce one handle per press.
            for name in ["TopLeft", "BottomRight", "Top", "bottomleft"] {
                oracle
                    .invoke(
                        "drag_handle",
                        IntrospectValue::Text(format!("topology,{name},2,2")),
                    )
                    .unwrap_or_else(|e| panic!("{name} refused: {e:?}"));
                assert_eq!(oracle.query("violations"), Ok(IntrospectValue::Int(0)));
            }

            // A name outside the published set is refused, so the accepted and
            // advertised vocabularies cannot drift.
            for spec in ["topology,Middle,2,2", "topology,Top,2", "topology,Top,x,2"] {
                match oracle.invoke("drag_handle", IntrospectValue::Text(spec.to_owned())) {
                    Err(InvokeError::Rejected(why)) => {
                        assert!(why.to_string().contains("handles"), "{spec}: {why:?}");
                    }
                    other => panic!("{spec} answered {other:?}"),
                }
            }
        });
    }

    #[test]
    fn r1609_a_stale_select_is_refused_by_name() {
        drive(|oracle| {
            assert_eq!(
                oracle.invoke("select", IntrospectValue::Text("loss".to_owned())),
                Ok(IntrospectValue::Text("loss".to_owned()))
            );
            match oracle.invoke("select", IntrospectValue::Text("ghost".to_owned())) {
                Err(InvokeError::Rejected(why)) => {
                    assert_eq!(why.to_string(), "no tile ghost in this grid");
                }
                other => panic!("{other:?}"),
            }
            assert_eq!(
                text(oracle, "current"),
                "loss",
                "a refused select left the selection alone"
            );
        });
    }

    #[test]
    fn r1609_the_displacement_is_announced_through_a_live_region() {
        Owner::new().run(|| {
            let mut oracle = DashboardOracle::new();
            oracle.attach(use_board_state());
            oracle
                .invoke("select", IntrospectValue::Text("topology".to_owned()))
                .unwrap();
            oracle
                .invoke("key", IntrospectValue::Text("Shift+ArrowUp".to_owned()))
                .unwrap();
            oracle
                .invoke("key", IntrospectValue::Text("Shift+ArrowUp".to_owned()))
                .unwrap();

            let nodes = DashboardView::access_node(&(), Some(ROOT_TAG));
            let region = nodes
                .iter()
                .find(|n| n.tag == REFLOW_TAG)
                .expect("the live region is in the tree");
            assert_eq!(
                region.live,
                Some(AccessLive::Polite),
                "★ the toolkit has accessible announcement event and no widget in \
                 the toolkit's widget module/src/widgets fires one"
            );
            let Some(AccessValue::Text(said)) = &region.value else {
                panic!("the region carries the sentence")
            };
            assert!(
                said.contains("topology at column 1, row 1"),
                "it states the edited card's slot in the SAME one-based lines the \
                 per-card value uses: {said}"
            );
            assert!(
                said.contains("pushed"),
                "and it names what moved out of the way, which is the half a \
                 per-card value cannot carry: {said}"
            );

            // The board is one focus stop with the selected card as its active
            // descendant — thirty cards are not thirty Tab stops.
            let focus = DashboardView::access_focus_target(&(), Some(ROOT_TAG))
                .expect("the board owns the focus");
            assert_eq!(focus.focus_tag, ROOT_TAG);
            assert_eq!(
                focus.active_descendant.as_deref(),
                Some(sub_tag("card.topology").as_str())
            );
            let selected: Vec<&str> = nodes
                .iter()
                .filter(|n| n.selected == Some(true))
                .map(|n| n.tag.as_str())
                .collect();
            assert_eq!(
                selected,
                vec![sub_tag("card.topology")],
                "exactly one current card"
            );
            assert!(
                nodes
                    .iter()
                    .filter(|n| n.tag == ROOT_TAG)
                    .all(|n| n.children.len() == 5),
                "every card is declared a child, which is what resolves the \
                 active descendant"
            );
        });
    }

    #[test]
    fn r1609_only_the_selected_card_paints_its_handle_ring() {
        Owner::new().run(|| {
            use pinion_core::style::CursorHint;

            /// A grip's tag, the cursor it asks for, and the grid cell it sits in.
            type Grip = (String, Option<CursorHint>, (Option<u16>, Option<u16>));
            fn ring<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
                match scene {
                    Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
                    Scene::Container(c) => c.children.iter().find_map(|kid| ring(kid, tag)),
                    _ => None,
                }
            }
            fn grips(scene: &Scene, into: &mut Vec<Grip>) {
                if let Scene::Container(c) = scene {
                    if let Some(tag) = c.tag.as_deref()
                        && tag.contains(".handle.")
                    {
                        into.push((
                            tag.to_owned(),
                            c.layout.cursor,
                            (
                                c.layout.grid_column.and_then(|p| p.start_line),
                                c.layout.grid_row.and_then(|p| p.start_line),
                            ),
                        ));
                    }
                    for child in &c.children {
                        grips(child, into);
                    }
                }
            }
            let scene = DashboardView::view((), &Frame::new());
            let mut found = Vec::new();
            grips(&scene, &mut found);
            assert_eq!(
                found.len(),
                8,
                "one ring on the one selected card — a five-card board painting \
                 every ring would be forty grips: {found:?}"
            );
            assert!(
                found
                    .iter()
                    .all(|(tag, ..)| tag.starts_with(&sub_tag("card.throughput.handle.")))
            );

            let grip = |name: &str| {
                found
                    .iter()
                    .find(|(tag, ..)| tag.ends_with(name))
                    .unwrap_or_else(|| panic!("{name} grip missing from {found:?}"))
            };
            // ★ Each grip's cursor comes from its own handle, which is what
            // forced `CursorHint`'s two diagonal arms — the icons the window
            // chrome has commanded since R1189 while no scene node could ask.
            assert_eq!(grip(".TopLeft").1, Some(CursorHint::NwseResize));
            assert_eq!(grip(".BottomRight").1, Some(CursorHint::NwseResize));
            assert_eq!(grip(".TopRight").1, Some(CursorHint::NeswResize));
            assert_eq!(grip(".BottomLeft").1, Some(CursorHint::NeswResize));
            assert_eq!(grip(".Left").1, Some(CursorHint::ColResize));
            assert_eq!(grip(".Bottom").1, Some(CursorHint::RowResize));

            // ★★ And each one's PLACE is a grid cell rather than a pixel inset.
            // The round's close audit found the first draft positioning grips at
            // an absolute offset off a stated card width, which is a layout-pass
            // output a view function cannot know — the right-hand grips of a
            // twelve-column card floated in its middle. This assertion is what
            // the fix needed: it is about where a grip IS, and the first version
            // of this test only checked tags and cursors, so it could not see the
            // defect at all.
            assert_eq!(grip(".TopLeft").2, (Some(1), Some(1)), "near / near");
            assert_eq!(grip(".Top").2, (Some(2), Some(1)), "middle / near");
            assert_eq!(grip(".TopRight").2, (Some(3), Some(1)), "far / near");
            assert_eq!(grip(".Left").2, (Some(1), Some(2)), "near / middle");
            assert_eq!(grip(".Right").2, (Some(3), Some(2)), "far / middle");
            assert_eq!(grip(".BottomLeft").2, (Some(1), Some(3)), "near / far");
            assert_eq!(grip(".Bottom").2, (Some(2), Some(3)), "middle / far");
            assert_eq!(grip(".BottomRight").2, (Some(3), Some(3)), "far / far");
            // No two grips share a cell, so the ring is a bijection onto the
            // eight cells around the middle one.
            let mut cells: Vec<_> = found.iter().map(|g| g.2).collect();
            cells.sort();
            cells.dedup();
            assert_eq!(cells.len(), 8, "two grips shared a cell: {found:?}");
            assert!(
                !cells.contains(&(Some(2), Some(2))),
                "the middle cell is the card's content, not a grip"
            );

            // The ring is ONE overlay node pinned to the card's content rect,
            // which is what lets it track a card of any size with no arithmetic.
            let overlay =
                ring(&scene, &sub_tag("card.throughput.handles")).expect("the ring is tagged");
            assert_eq!(overlay.layout.absolute_position, Some((0, 0)));
            // ★ The TRACK KINDS, not just their count: a broken counterfactual
            // exposed this gap — replacing all three with `Px(HANDLE_PX)` kept
            // every placement assertion above true while squeezing the card's
            // content into eight pixels, so the ring's shape needed asserting
            // too.
            for axis in [
                &overlay.layout.grid_template_columns,
                &overlay.layout.grid_template_rows,
            ] {
                assert_eq!(axis.len(), 3);
                // ★★ The grip's SIZE is the hit-test band, asserted against the
                // very constant `TileHandle::at` is called with. A pixel grip
                // beside a fractional band is two numbers for one thing and they
                // disagree at every card size but one — the same paint-and-
                // gesture split this round already paid for once, found the
                // second time by audit rather than by test.
                assert_eq!(axis[0], GridTrack::Fr(HANDLE_BAND), "a grip IS the band");
                assert_eq!(axis[2], GridTrack::Fr(HANDLE_BAND));
                assert_eq!(axis[1], GridTrack::Fr(HANDLE_CORE));
                let GridTrack::Fr(core) = axis[1] else {
                    panic!("the middle track is fractional")
                };
                let GridTrack::Fr(band) = axis[0] else {
                    panic!("a band track is fractional")
                };
                assert!(
                    (band * 2.0 + core - 1.0).abs() < f32::EPSILON,
                    "the three tracks cover the card exactly: {band} x2 + {core}"
                );
                assert!(core > 0.0, "a card must keep an interior to drag by");
            }
            assert_eq!(overlay.children.len(), 8);
        });
    }

    #[test]
    fn the_painted_board_is_a_grid_with_one_child_per_card() {
        fn board(scene: &Scene) -> Option<&ContainerNode> {
            match scene {
                Scene::Container(c) if c.tag.as_deref() == Some(ROOT_TAG) => Some(c),
                Scene::Container(c) => c.children.iter().find_map(board),
                _ => None,
            }
        }
        Owner::new().run(|| {
            let scene = DashboardView::view((), &Frame::new());
            let container = board(&scene).expect("the board container is tagged");
            assert_eq!(
                container.layout.display,
                pinion_core::style::Display::Grid,
                "twelve Fr tracks put it in grid mode"
            );
            assert_eq!(container.children.len(), 5);
            assert_eq!(container.layout.grid_template_columns.len(), 12);
        });
    }

    /// R1650 — every painted tag under the board is a COMPOSITE of the board's
    /// own, which is what makes a press on a card reach the board at all.
    ///
    /// It is a test rather than a convention because a counterfactual proved
    /// nothing else could fail: reverting `sub_tag` to a bare name left the
    /// whole suite green while the board went dead to a real mouse, which is
    /// precisely the state R1608 shipped in. The demo's boot gate catches it
    /// too, and demos do not gate a push.
    #[test]
    fn r1650_every_painted_tag_resolves_to_the_board() {
        fn tags_of(scene: &Scene, out: &mut Vec<String>) {
            if let Some(tag) = scene.tag() {
                out.push(tag.to_owned());
            }
            for child in scene.child_nodes() {
                tags_of(child, out);
            }
        }
        Owner::new().run(|| {
            let scene = DashboardView::view((), &Frame::new());
            let mut tags = Vec::new();
            tags_of(&scene, &mut tags);
            let inside: Vec<&String> = tags.iter().filter(|t| t.contains("card.")).collect();
            assert!(
                inside.len() >= 5,
                "the seed board paints at least one tag per card: {tags:?}"
            );
            for tag in inside {
                assert_eq!(
                    pinion_core::composite_tag::split_subindex(tag).0,
                    ROOT_TAG,
                    "`{tag}` must resolve to the board, or the router drops \
                     every press that lands on it"
                );
            }
        });
    }
}
