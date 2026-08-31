// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-node-lab` — R1651 §5.21 §5.51 §2 #7 — the analysis-tool **node graph
//! lab**, assembled as one application against a written-down specification of
//! the reference screen.
//!
//! ## What is new here, and why it is a crate and not this example
//!
//! The analysis-tool census puts a *node inspector that is the settings editor*
//! in its must-have tier and, measured at R1646, scored it `gap`: no crate held
//! a property grid, so the per-field applies badge the capability names had
//! nowhere to live. R1650 gave the **model** a home; R1651 gives it a painter
//! (`pinion_widget_paint::config_form`) and this screen is its first consumer.
//! What the inspector on the right does — a row per configuration path, a
//! HOT/RESTART badge per row, a defect shown on the row it is about, a launch
//! verdict derived from the rows rather than set beside them, and a deployable
//! document derived from the same rows — is framework code, not this file's.
//!
//! ## The specification is a value
//!
//! `spec` holds the reference screen as a table: which panes exist and how
//! wide, which roles the palette groups, which nodes the opening graph holds
//! and where, which fields the inspector shows and with what applies-scope.
//! This file is written *against* that table, the table is published on the
//! wire as `spec`, and the demo asserts the painted scene against it in **both
//! directions** — an element the screen is missing and an element the screen
//! invented are both failures. A round can claim it reproduced a reference; the
//! only thing that makes the claim checkable is putting the reference where a
//! machine can read it.
//!
//! ## The graph is the crate's, the taxonomy is this application's
//!
//! Nodes, frames, links and reachability are `pinion_node_graph::Document`, so
//! a link the model cannot hold is refused by the crate and named. What roles
//! exist, and what their pins carry, is `graph::Role` / `graph::Transport`
//! — which is exactly the split the census draws when it calls the palette
//! `app` and `connect` `have`.
//!
//! ## Run it
//!
//! ```text
//! cargo run -p hello-node-lab --release
//! ```
//!
//! Drag empty canvas to pan, wheel to zoom, drag a node to place it (hold ctrl
//! to snap), drag a pin to author a link, click a node to inspect it, click a
//! link to see its endpoint. The gate refuses to open Run while a value would
//! fail at start-up, and says what stands when it does open.
//!
//! See `tools/demos/r1651_the_node_lab_matches_the_reference.py`.

mod deploy;
mod graph;
mod judge;
mod persist;
mod scenario;
mod settings;
mod spec;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pinion_a11y::{
    AccessLive, AccessNode, AccessState, AccessValue, AriaCurrent, AriaRole, NavLink, WidgetA11y,
    navigation_link_nodes,
};
use pinion_core::availability::Unavailable;
use pinion_core::containment::{band_in, line_box, line_rect_in};
use pinion_core::describe::{Descriptions, Resting};
use pinion_core::edge_panel::EdgePlacement;
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ObjectArgs, PointerTarget,
    ReadRefusal, RepaintOwner, SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::reactive::{Signal, Tracked};
use pinion_core::scene::{
    ContainerNode, PathCommand, PathNode, PathPoint, Rect, ScrollAxis, ScrollNode, TextNode,
};
use pinion_core::selection::Selection;
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::style::{
    Border, BoxStyle, ChromeEdge, Color, Dash, DotLattice, LayoutStyle, PathStyle, Size, Stroke,
    TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::utterance::{Announced, Tone, Utterance};
use pinion_core::voice::Silence;
use pinion_core::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType, FormError, Source, Verdict,
};
use pinion_core::widgets::fault_injection::{self, Injection, Scope};
use pinion_core::widgets::overflow;
use pinion_core::widgets::picker::{Picked, Picker};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::{AutoScroll, ScrollState};
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::widgets::wheel::WheelDirection;
use pinion_core::{CellKind, Frame, Modifiers, Scene, WidgetCore, edit_field_keymap};
use pinion_node_graph::{
    Act, Camera, Document, Extent, Faces, Fit, Found, Item, LandError, Landfall, LinkId, LinkLayer,
    Margin, NameSource, Node, NodeBody, NodeId, PortPath, PortRef, ROOT, Relinked, Side, Socket,
    Tint, Violation, ZoomRange, palette_of, type_palette,
};
use pinion_platform_storage::AppStorage;
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::caption::{self, captioned};
use pinion_widget_paint::config_form::{
    FieldGrowth, FormGeometry, FormStyle, OpenPicker, RowWrap, Seat, form_geometry_showing,
    row_access_nodes, view_config_form_showing,
};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
use pinion_widget_paint::text_field as tf_paint;
use serde::{Deserialize, Serialize};

use deploy::Produced;
use graph::{Implementation, LabNode, Revisions, Role, Stack, Transport};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloNodeLabRenderer, HelloNodeLabRendererError);

// ── Geometry ────────────────────────────────────────────────────────────────

/// The width the specification's rectangles were measured against.
const DESIGN_W: u32 = 1440;
/// The size the window opens at — **never below the floor it declares**.
///
/// ★★★ R1688 — R1687 raised [`MIN_W`] to 1442 and left this at 1440, so
/// [`initial_size_strategy`](NodeLabView::initial_size_strategy) asked the shell
/// to open a window two pixels narrower than the minimum it handed it in the
/// same call, and every headless probe laid the screen out at a width the screen
/// says it does not support. Nothing failed, because the toolbar's two clusters
/// happened to have two pixels of slack left — which is precisely the kind of
/// quiet that ends when somebody moves a seat.
///
/// The design width stays a stated number ([`DESIGN_W`]) and the floor stays a
/// decision somebody makes; what is derived is only the invariant *between*
/// them, which is not a judgement and should never again be one round's
/// oversight. This round moves the floor again, and the design size follows it
/// rather than being caught out by it.
const WIN_W: u32 = if DESIGN_W > MIN_W { DESIGN_W } else { MIN_W };
const WIN_H: u32 = 900;

/// ★★ R1688 — the invariant, at COMPILE time, where it now cannot be false.
///
/// A runtime assertion for this would be `assert!(true)` — clippy says so, and
/// it is right: [`WIN_W`] is derived, so the check folds away. That is the
/// point, and it is also why the check belongs here rather than in a test that
/// would read like coverage. What the tests assert instead is the pair actually
/// handed to the shell (see
/// `r1687_the_toolbars_declared_width_covers_what_it_paints`), which is a real
/// value and is where R1687's inconsistency lived.
const _: () = assert!(
    WIN_W >= MIN_W && WIN_H >= MIN_H,
    "the window cannot open smaller than the minimum it declares"
);
const VIEW_TAG: &str = "node_lab";
/// R1662 — the input-router tags the two scrolling side panes answer to,
/// **taken from the specification** rather than spelled a second time here. The
/// gate reads the same column, so a pane whose body tag is edited in one place
/// cannot pass by being edited in two.
const PALETTE_SCROLL: &str = match spec::PANES[1].body {
    Some(tag) => tag,
    None => panic!("the palette declares a scrolling body"),
};
const INSPECTOR_SCROLL: &str = match spec::PANES[3].body {
    Some(tag) => tag,
    None => panic!("the inspector declares a scrolling body"),
};
const THEME_TAG: &str = "app";

const RAIL_W: u32 = spec::PANES[0].width;

/// ★★★★★ R1725 — **the rail's width where it is being drawn**, which is zero
/// when the place this screen was put in already has a navigation.
///
/// [`RAIL_W`] stays a constant because it is also a *window policy* number —
/// `MIN_W` and `FLOOR_W` are what this screen asks an operating system for when
/// it owns a window, and that ask does not change with where a page happens to
/// be. What changes is the layout, so only the layout reads this.
///
/// Standalone it answers `RAIL_W` and every rectangle is what it was, which is
/// why the standalone screen's own rectangle assertions still hold unedited.
fn rail_w() -> u32 {
    if draws_own_rail() { RAIL_W } else { 0 }
}

/// ★★★★★ R1725 — **the one question every half of the rail asks.**
///
/// The paint, the accessibility tree, the keyboard ring, the hit test and the
/// width above are five readers of one fact, and this project's recurring
/// defect is exactly what happens when two of them read it separately: a screen
/// that publishes a rule it does not keep. So the question is asked once, here,
/// and the five derive from it.
///
/// ★★★★★ R1825 — and asked of the SURFACE, for the reason spelled out on
/// [`draws_own_app_bar`]. Two of those five readers — the hit test and the
/// keyboard — run outside the scope the host wraps the build in, so they were
/// answering `NONE` and placing a rail this screen no longer paints. It cost
/// nothing visible **only because the host's own rail occupies those same
/// pixels and resolves a press first**; the bar's twin had no such cover and
/// put 41 regions out of step. A defect that is invisible because something
/// else happens to be in the way is still the defect.
fn draws_own_rail() -> bool {
    !pinion_core::chrome::host_chrome_for(VIEW_TAG).provides(pinion_core::chrome::Part::Navigation)
}

/// ★★★★★ R1822 — **the app bar's twin of [`draws_own_rail`]**, and the reason
/// it was 97 rounds late.
///
/// R1725 built the rail's half of this and wrote the general paragraph for it —
/// *a page inside an application that already has one must not contribute a
/// second one to the tree either, and the only way that is a property rather
/// than a rule is for the node not to exist*. It then declined to declare
/// [`Part::ApplicationBar`](pinion_core::chrome::Part::ApplicationBar) on the
/// host, on the argument that a host's bar carries the application's subject
/// and a page's bar carries the page's, so the two are different sentences.
///
/// 🟥 **That argument is plausible, reads as settled, and the same round's own
/// measurement of the behaviour canon contradicts it.** The canon has ONE bar,
/// the host's, on all three screens; a graph's name and run state are not in it
/// and are drawn on the canvas toolbar instead. Which is where this screen
/// already draws them: `lab.toolbar.title` is the same `GRAPH_NAME`, and
/// `lab.toolbar.run.label` reads *running n/n*. So mounted, all three things
/// this screen's bar carries are already on this screen somewhere else — the
/// third being the words *node lab*, which restate an identity the host's bar
/// and the rail's active seat both already give.
///
/// ⇒ the bar is not a different sentence mounted. It is the same sentence,
/// twice, in a strip 54 pixels tall that the canon does not have.
///
/// ★★★★★ R1825 — **asked of the SURFACE, not of the ambient scope.**
/// `host_chrome()` answers only inside the build the host wraps; every hook the
/// framework calls on this screen afterwards — what is under a pointer, where a
/// press lands, what the wheel does — runs outside it and got `NONE`, which is
/// not "no answer" but *you are standalone*. So this screen laid its panes out
/// without the bar and hit-tested them with it, and every rectangle below the
/// strip was 54 pixels out of step. Measured: **41** regions addressed a
/// DIFFERENT region at their own centre when mounted, and **0** did standalone
/// at the same size. (No denominator — see `pinion_core::chrome`'s header for
/// why one would be a fact about the frame rather than about the defect.)
fn draws_own_app_bar() -> bool {
    !pinion_core::chrome::host_chrome_for(VIEW_TAG)
        .provides(pinion_core::chrome::Part::ApplicationBar)
}

/// The app bar's height where it is being drawn — zero where the host draws it.
///
/// [`APP_BAR_H`] stays a constant for the same reason [`RAIL_W`] does: it is
/// also a *window policy* number, and [`MIN_H`] and [`floor_height`] are what
/// this screen asks an operating system for when it owns a window. Only the
/// layout reads this.
fn app_bar_h() -> u32 {
    if draws_own_app_bar() { APP_BAR_H } else { 0 }
}

const PALETTE_W: u32 = spec::PANES[1].width;
const INSP_W: u32 = spec::PANES[3].width;
const APP_BAR_H: u32 = spec::APP_BAR_H;
const TOOLBAR_H: u32 = spec::TOOLBAR_H;
const PAD: u32 = 14;

const FONT_TITLE: u32 = 14;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
/// ★ R1834 — taken from the specification rather than spelled twice: a gate has
/// to ask what this face scales to, and two copies of a size are two answers.
const FONT_TINY: u32 = spec::FONT_TINY_PX;

/// The canvas: what is left between the palette and the inspector.
/// The window this frame is being painted into.
///
/// ★ R1654 — read from the shell rather than assumed. The screen used to be
/// [`WIN_W`] x [`WIN_H`] constants everywhere, so enlarging the window left the
/// content in the top-left corner with the rest of the surface black, and
/// shrinking it painted the inspector off the edge. Reported from the running
/// window; invisible to every test here, because a test that calls
/// `compute_layout(scene, WIN_W, WIN_H)` has assumed the very thing that was
/// wrong.
///
/// `use_viewport_size` is a tracked read, so the view re-runs on a resize. It
/// answers `(0, 0)` where no shell has published one — a headless probe, a unit
/// test — and the declared design size is the honest fallback there: it is what
/// the specification's rectangles were measured against.
fn window_size() -> (u32, u32) {
    // ★★★★★ R1700 — the whole expression is the framework's now, not just the
    // number. This screen had the case analysis right; the capture viewer's
    // copy of it did not, and the shell had a third version again. See
    // `layout_size` for what the three spellings were and which one shipped
    // broken. One further correction comes with the move: the branch below read
    // the WINDOW inside a view and this SURFACE outside one, two quantities
    // that agree only because this surface happens to fill its window.
    // ★ R1712 — the clamp is the POLICY's comfortable size, not a constant
    // repeated here. The window may now be smaller than this (see [`SHRINK`]),
    // which is exactly what the two branches below have always meant: below its
    // floor the layout stops shrinking and the window clips.
    // ★★★★★ R1821 — read through [`comfortable_size`], which is that policy's
    // comfortable width less whatever chrome the host draws in this screen's
    // place. This line said `SHRINK.comfortable()` directly, and reading it
    // directly is what would make it a SECOND number the moment the subtraction
    // existed: `judge` compares a host's grant against `comfortable_size`, so
    // the width the layout stops at and the width a frame is judged against
    // would have stopped being one width. Standalone the two spellings are
    // equal by construction — `host_provided_width` is zero there — so this
    // changes nothing for the screen that owns its window.
    pinion_core::external::layout_size(VIEW_TAG, comfortable_size(), (WIN_W, WIN_H))
}

/// The smallest window this screen lays out in.
///
/// Below it the panes would overlap, so the layout stops shrinking and the
/// window clips instead — the same choice a fixed minimum size makes, stated
/// here rather than left to arithmetic that would produce negative widths.
/// ★ R1656 — the width the toolbar's RIGHT-anchored cluster needs. The zoom
/// pair, the readout, the view reset, the two export buttons and the run button
/// are all placed by subtracting a constant from the pane's right edge, so a
/// pane narrower than this paints them off its own left side — and the floor
/// below is what declares that width supported.
///
/// ★★★★★ R1687 — **it was 300 and it had been wrong since R1678**, which added
/// the view-reset seat 340 px in from the right. Nothing read this constant:
/// R1656 wrote it as a sentence about the layout and then wrote the layout
/// somewhere else, so the two drifted the moment a seat was added and no gate
/// asked. That is [[debt-a-stated-limit-is-not-checked-by-anything]] exactly —
/// a limit stated in prose is re-derived by whoever is next and nobody looks
/// back at it.
///
/// It is now checked: `r1687_the_toolbars_declared_width_covers_what_it_paints`
/// derives the requirement from the rectangles themselves and fails if any
/// right-anchored seat reaches further in than this says. Adding the script
/// button is what made the drift matter — but the gate is what makes it the
/// last time.
///
/// ★★ R1688 — 426 → 431, and the gate is what said 431. The zoom pill grew a
/// fit seat and lost the separate `home` seat, so the two changes very nearly
/// cancel: five pixels, against the fifty-four a new seat beside the old ones
/// would have cost. [`MIN_W`] is derived from this, so it moved with it, and
/// [`WIN_W`] now follows [`MIN_W`] rather than being left behind by it.
///
/// ★★★ R1689 — 431 → 609, and **the cost is stated rather than absorbed**. The
/// file pill is three more seats in the toolbar's right cluster, which is what
/// sets this screen's minimum width, so [`MIN_W`] goes 1447 → 1625 and a
/// 1600-wide display no longer holds this screen. That is a real loss and it is
/// written here rather than discovered: the reference puts these three buttons
/// in exactly this place, and the alternative — hiding them somewhere the
/// reference does not — would be a further deliberate divergence bought with
/// pixels.
///
/// ★ The number is the gate's, not mine. The first draft said 594 — arithmetic
/// done by hand over the seat widths — and
/// `r1687_the_toolbars_declared_width_covers_what_it_paints` answered 609,
/// having derived it from the rectangles. Two other gates failed alongside it
/// for the same reason (a floor too small paints the left cluster into the
/// right one), which is what a derived limit buys over a stated one.
///
/// ★★★★★ R1791 — **the constant this paragraph described is gone**, and that is
/// the round. It read `const TOOLBAR_RIGHT_CLUSTER: u32 = 609` and ended:
/// *"what would take it back is an overflow affordance on the toolbar, which
/// this tree does not have and which is a round of its own; until then a screen
/// whose chrome outgrows its window clips"*. A reader then opened the shipped
/// window and reported exactly that clip.
///
/// Two things replaced it, and neither is a hand-written number.
/// [`ToolGroup::width`] derives what each group needs from its own seats, so
/// the cluster's comfortable width is a function of what it contains rather
/// than a total somebody re-added; and [`TOOLBAR_RIGHT_FLOOR`] is what it needs
/// at its NARROWEST, which is the number [`MIN_W`] actually has to make room
/// for. R1687 derived 609 rather than stating it, which was the right move
/// available then; deriving it per group is the same move one level down, and
/// it is the level at which a row can give something up.
///
/// # What replaced it: the cluster at its NARROWEST
///
/// The launch seat, which is [`overflow::WhenTight::Keep`], and the control
/// that holds everything else. This is the number that decides whether the
/// inspector is cut.
///
/// ★★★★★ **What the measurement found, and it is worse than the report.** The
/// cluster's groups need 607 with their gaps. Available: 410 at this screen's
/// own design width, 358 in the page the shell gives it — and **595 at the 1625
/// it used to declare as its minimum**. It did not fit at its own floor either.
/// The old `TOOLBAR_RIGHT_CLUSTER = 609` was a **reach** — how far the rightmost
/// seat came in, measured off the rectangles — and a reach is not a sum: it
/// holds only if the two clusters are flush, with no gap between them. So the
/// seats were painted hard against the left cluster at the floor and 197px past
/// the pane at the shipped size, which is the clip a reader reported.
///
/// Every term here is a real one: the clear space at the right edge, the seat
/// that may not move, the gap before it, the control, and the gap that keeps
/// the control off the left cluster.
const TOOLBAR_RIGHT_FLOOR: u32 = RUN_INSET + RUN_W + CLUSTER_GAP + OVERFLOW_W + CLUSTER_GAP;

/// ★ R1656 — the canvas pane's floor is DERIVED from what the chrome above it
/// needs, not asserted at 240. The size axis found the difference on its first
/// run: at the old floor the zoom readout was not painted at all, because
/// `right - 300` had gone past the pane's own left edge. A declared minimum the
/// screen cannot actually paint is a claim nobody was checking.
const MIN_W: u32 = floor_width(RAIL_W);

/// ★ R1656 — the toolbar's LEFT half: the graph title, the node/link counts and
/// the launch-gate chip, which is placed after them. Named for the same reason
/// its sibling is — the floor has to be wide enough for both halves, and the
/// size axis found the gate chip painted past the pane's right edge when it
/// was not.
const TOOLBAR_LEFT_CLUSTER: u32 = 420;
/// What the canvas needs vertically once the side panes stop dictating the
/// floor: the launch-gate panel is anchored to the canvas bottom and the hint
/// strip sits under it, so a canvas shorter than the two together paints one
/// over the other.
const CANVAS_FLOOR: u32 = 260;

/// The smallest height, likewise.
///
/// ★ R1662 — R1656 wrote the answer down and could not take it: "the panes do
/// not scroll, so the floor IS their content height; making them scroll would
/// let this number come back down and is the better answer". They scroll now,
/// so it came down — from 680 to what the CANVAS chrome needs, which is the
/// same derivation the width already used. A floor set by content nobody could
/// scroll to is a window a person cannot make small, and it was 420 pixels of
/// it ([[debt-the-node-lab-panes-do-not-scroll]]).
const MIN_H: u32 = floor_height(APP_BAR_H, true);

/// ★★★★★ R1822 — **the height floor as ONE expression, evaluated twice.**
///
/// [`MIN_H`] is the standalone answer and a `const`, because [`SHRINK`] is a
/// const and the window policy must stay one. [`layout_min_h`] is the same
/// expression for the configuration this page is actually in. They are not two
/// spellings that have to be kept in step — they are one function, and the
/// arguments are the two facts a host can change.
///
/// 🟥 **The draft of this round subtracted [`APP_BAR_H`] flat and was wrong**,
/// which is what made this necessary rather than tidy. Measured: the rail term
/// exceeds the content term, so [`MIN_H`] is the RAIL branch — and mounted, this
/// screen draws no rail EITHER, so the rail branch does not apply at all. A flat
/// subtraction charges a page for seats that are not on it, by exactly the
/// amount the rail term wins by. ⇒ ★★★★★ a derivation that reads one of two
/// facts is the same defect as a number with two readers, wearing the other hat.
///
/// ⚠ **This paragraph carried the wrong figures until the closing audit ran the
/// code.** It said the rail floor is 368 against 360 and the error is 8 pixels;
/// 368 is a SEVEN-seat rail, the count R1773 measured as drifted and restored to
/// eight. The relation was right and every figure derived from it was wrong, in
/// four files at once. The numbers are gone rather than restated — the test
/// `r1822_the_height_floor_drops_every_strip_the_host_provides` asserts the
/// relation and prints the amount.
const fn floor_height(app_bar: u32, own_rail: bool) -> u32 {
    let content = app_bar + TOOLBAR_H + CANVAS_FLOOR;
    // ★★★★★ R1773's rail floor, which was a `RAIL_FLOOR` constant beside this
    // until R1822 folded it in — the expression is unchanged and everything
    // that round argued still holds. It is derived from the roster rather than
    // asserted, so a ninth seat moves the floor by itself: R1773 measured the
    // rail having DRIFTED to seven seats while the reference states eight, and
    // the eighth overflowed by 37 pixels the moment it was restored, because
    // `spec::PANES` had justified giving the rail no scrollable body with *the
    // rail's content is the specification's own list and cannot outgrow the
    // pane* — true only of the shorter list.
    //
    // ⚠ It is a floor for CHROME, which is why the rail gets one at all where
    // R1662's other panes just scroll: the rail is how a reader reaches every
    // other section, and a destination you must scroll a chrome strip to find
    // is a destination the screen has hidden.
    //
    // ★ R1822 makes it CONDITIONAL. Where a host draws the navigation there are
    // no seats on this page to fit, so charging for them declines pages for
    // nothing — and this is the arm the flat subtraction kept anyway.
    //
    // ⚠ No figure here on purpose: this line carried one until R1822.1's
    // self-grep, which found it still naming the draft's wrong amount three
    // paragraphs under the docstring that says the numbers are gone. The
    // amount is whatever this arm exceeds `content` by, and
    // `r1822_the_height_floor_drops_every_strip_the_host_provides` prints it.
    let rail = if own_rail {
        app_bar + RAIL_SEAT_TOP + RAIL_SEAT_PITCH * SEATS + RAIL_SEAT_TOP
    } else {
        0
    };
    if rail > content { rail } else { content }
}

/// The height floor for the configuration this page is in — the height half of
/// [`comfortable_size`], which is its caller.
///
/// ★ Named rather than inlined so a test can state the property against
/// [`MIN_H`] on its own axis: the pair returns a tuple, and an assertion that
/// has to index one out of it says less about which half it is about.
fn layout_min_h() -> u32 {
    floor_height(app_bar_h(), draws_own_rail())
}

/// How many seats the rail draws — the specification's own count.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the roster is a compile-time constant of eight entries"
)]
const SEATS: u32 = spec::RAIL.len() as u32;

/// ★★★★★ R1712 — the width the WINDOW stops at, which is no longer the width
/// the LAYOUT stops at.
///
/// At the time, [`MIN_W`] was 1625 and R1689 wrote down what that cost: *"a
/// 1600-wide display no longer holds this screen. That is a real loss."* It was
/// a real loss and nothing could be done about it, because one constant was
/// doing two jobs — the size the layout stops reflowing at, and the size the
/// window refuses to shrink past — and lowering it would have moved both.
///
/// ⚠ **The number in that first sentence was written in the PRESENT tense until
/// R1821**, and had been false since R1791 replaced `TOOLBAR_RIGHT_CLUSTER`
/// with [`TOOLBAR_RIGHT_FLOOR`] — 437 pixels off [`MIN_W`] in one round, and
/// this paragraph went on saying 1625. Read the constant, never this sentence.
/// ★ Worth the note because of what the paragraph is ABOUT: one constant doing
/// two jobs. R1821 found the same shape one level over — the same comfortable
/// width answering both *what do I ask a window for* and *is this grant enough
/// room* — so the defect this paragraph describes outlived the fix it describes.
///
/// [`ShrinkPolicy`] separates them, so this is the second number: below it the
/// app bar's right end and the inspector are **clipped**, and above it nothing
/// is. That is a decision, and [`SHRINK`] is where it is written down so
/// `scene/size_floor` can check it against the screen every time it is asked.
///
/// ★★★★★ Measured, three times, and the first two answers were wrong.
///
/// | round | answer | how it was measured | why it was wrong |
/// |---|--:|---|---|
/// | R1712 | 1506 | `scene/scroll_reach`'s `lost` | that predicate judged each mark against its nearest scrolling ancestor, so the nine actions the window had removed **fit the inspector pane** and were never reported |
/// | R1712.1 | 1595 | tagged marks in the paint whose whole box is past the window | the tag map cannot see an untagged mark, and the marks that go first here are the `×` **glyphs inside** the remove buttons |
/// | R1713 | **1601** | `scroll_reach`'s `lost` again, with the clip chain folded and `clipped` split from `lost` | — |
///
/// R1713 fixed the predicate rather than working around it: reachability now
/// composes down the clip chain, so a mark inside a pane the window slices is
/// judged against the slice, and `lost` (no pixel is ever reachable) is a
/// different answer from `clipped` (the reader reaches all but an edge). Driven
/// across the whole band, **1601 is the width at which nothing is `lost`, and at
/// 1600 five marks are** — the five row `×` glyphs, whose ink starts 286 pixels
/// into a pane that is offered 286 at that width. The same boundary at 360 tall
/// and at 900.
///
/// ★★★★★ R1714 — and the answer is no longer a number the glyphs decide.
///
/// R1713's 1601 was the width at which this screen stopped **losing** things,
/// and it missed R1689's 1600-pixel display by one pixel that could not be
/// bought: below `comfortable` the layout stops reflowing, so what the window
/// cut was simply gone. The window PANS now
/// ([`ShrinkPolicy::panning`] — measured, nothing is out of reach at 400 pixels
/// wide), so nothing is lost at any width and the floor stops being a
/// measurement and becomes what [`pinion_core::shrink`]'s own doc always said
/// it was: *a product decision — how small is usable is not derivable from
/// geometry*.
///
/// So it is decided, and this is the decision: **the four things this screen is
/// for, side by side.** The icon rail, the palette, one whole node card and the
/// inspector — below that the reader is panning to see a single card, which is
/// the point at which the pan has stopped being a convenience. Everything the
/// toolbar's two clusters need, which is what set the old floor, is a pan away.
///
/// Derived from the specification rather than written down, for the reason
/// [`MIN_W`] is (R1687: a limit stated in prose is re-derived by whoever is
/// next). The card term is in CANVAS units, which is the conservative
/// direction — the canvas opens at 84%, so the card takes fewer window pixels
/// than this reserves for it.
const FLOOR_W: u32 = RAIL_W + PALETTE_W + WIDEST_CARD + INSP_W;

/// The widest card the opening graph draws, in canvas units.
const WIDEST_CARD: u32 = {
    let mut widest = 0;
    let mut i = 0;
    while i < spec::NODES.len() {
        let (_, _, w) = spec::NODES[i].rect;
        if w > widest {
            widest = w;
        }
        i += 1;
    }
    widest
};

/// The two floors, declared once so they cannot drift apart.
///
/// [`window_size`] clamps the layout at [`ShrinkPolicy::comfortable`] and
/// [`NodeLabView::initial_size_strategy`] floors the window at
/// [`ShrinkPolicy::floor`]; neither writes a number of its own.
///
/// ★★★★★ R1714 — `panning`, not `conceding`, and the difference is what the
/// band costs a reader: **nothing**. The framework wraps this screen in a
/// viewport onto its own layout whenever the window is the smaller of the two,
/// so the app bar's right end and the inspector are one gesture away rather
/// than gone — which is why there is no list of what is given up any more, and
/// why declaring one here would not compile.
///
/// The HEIGHT axis still concedes nothing, and that stays a decision rather
/// than an oversight: [`MIN_H`] is far below every display this screen opens
/// on, so a height band would buy a reader nothing measurable. The width was
/// the axis R1689 wrote a real loss against.
///
/// ⚠ **That sentence used to say "`MIN_H` is 360".** It is not: the floor is a
/// `max` and the RAIL's term wins, which is the very fact this round needed and
/// which the number here would have denied. No figure replaces it — the argument
/// is *far below any display*, and the figure never carried it. Read
/// [`floor_height`], and `r1822_the_height_floor_drops_every_strip_the_host_provides`
/// for the assertion that keeps the relation honest.
///
/// 🟥 R1822 first replaced 360 with **368**, which is also wrong: that is a
/// seven-seat rail's floor, and the rail has had eight seats since R1773. ⇒
/// ★★★★★ correcting a rotted number by writing another number is the same act
/// that rotted the first one.
const SHRINK: ShrinkPolicy = ShrinkPolicy::panning((MIN_W, MIN_H), (FLOOR_W, MIN_H));

/// ★ R1770 — the size this screen declares it lays out at, for a reader inside
/// this crate that is not the layout.
///
/// [`SHRINK`] is private and should stay so — it is one screen's policy, not a
/// fact anybody else may set. But `judge` has to know it: a verdict read from a
/// frame narrower than this is a verdict about a slice, and the module that
/// says so must read the declared number rather than restate it. See
/// `judge::built` for the measurement that made this necessary.
///
/// ★★★★★ R1822 — **it is the floor EVALUATED for this configuration, not the
/// policy with a subtraction applied.** R1821 introduced the subtraction, and
/// one round later the second axis proved the idiom wrong: the height floor is
/// a `max` of two terms, so "policy minus the strip the host draws" quietly
/// over-charged by whatever the winning term exceeds the other by. See
/// [`floor_height`] for the measurement and the test that prints the amount.
///
/// A subtraction has to know what the whole was made of; a derivation just
/// answers for the arguments it is given. So both axes are now one expression
/// each — [`floor_width`] and [`floor_height`] — called once with what this
/// screen draws and once, as [`MIN_W`] and [`MIN_H`], with what it draws when
/// it owns the window. Standalone the two calls are the same call, which is why
/// nothing standalone moves.
pub(crate) fn comfortable_size() -> (u32, u32) {
    (floor_width(rail_w()), layout_min_h())
}

/// ★★★★★ **The width floor for a page whose rail is `rail` wide** — the width
/// half of [`comfortable_size`], and [`MIN_W`] when it is given the rail this
/// screen draws when it owns the window.
///
/// R1821's finding is what it exists for, and is why [`comfortable_size`]
/// stopped being a constant.
///
/// [`SHRINK`]'s comfortable width is a *window policy* number: what this screen
/// asks an operating system for when it owns a window, which R1725 argued must
/// not change with where a page happens to be. That argument is right, and
/// nothing here disturbs it — `SHRINK.comfortable()` is untouched and is still
/// what the layout clamps against.
///
/// But the same number had a second reader. `judge` compares it against the
/// extent a HOST granted, to decide whether a frame is this screen laid out or
/// a slice of one. For that question a rail the host draws is not room this
/// screen needs, and charging for it makes the screen decline a page that
/// would in fact have fitted. ⇒ **one number was answering two questions**,
/// which is this tree's most-recorded defect class, and the half that was wrong
/// is the half that reads a grant rather than asks for a window.
///
/// ★ The argument is [`rail_w`]'s answer, not [`draws_own_rail`]. R1725 named
/// the rail's five readers and put the question in one place; a sixth reader of
/// that predicate would be the very thing it exists to prevent, so the caller
/// passes the *width* the layout actually uses.
///
/// ★ Standalone the two calls are **the same call** — `host_chrome()` is `NONE`,
/// `rail_w()` answers [`RAIL_W`], so `comfortable_size()` is `floor_width(RAIL_W)`
/// which is [`MIN_W`], exactly as before. The screen that owns its window is
/// unaffected by construction rather than by a branch, which is why no
/// standalone assertion needed editing.
///
/// ⚠ **R1821 wrote this as a SUBTRACTION** — `host_provided_width()` returning
/// `RAIL_W - rail_w()`, which the paragraphs above were written about — and
/// R1822 replaced it with this derivation. Those paragraphs are still why a
/// mounted screen stopped being charged for the rail. What changed is the FORM:
/// on the height axis the same subtraction charged a mounted page for rail seats
/// that are not on it, because that floor is a `max` and the term that wins
/// standalone is the one that does not apply mounted. A subtraction has to know
/// what the whole was made of; a derivation only answers for its arguments.
const fn floor_width(rail: u32) -> u32 {
    rail + PALETTE_W + (TOOLBAR_RIGHT_FLOOR + TOOLBAR_LEFT_CLUSTER) + INSP_W
}

/// ★★★★★ R1902 — where a pane opens, **read from the specification** rather
/// than written here beside it.
///
/// These were two `const`s spelling an arrangement the specification also
/// described, and the two were free to disagree: the spec said which edges a
/// pane admits and whether it folds, and the constants said where it started,
/// and nothing compared them. The palette opening folded is the change this
/// round is about, and it is one field of one row of `spec::PANES` now — not an
/// edit here that the declaration would have gone on contradicting.
///
/// ⚠ Totality rather than a `Option::unwrap`: a tag this screen asks for is one
/// it declares, so the fallback is unreachable — but a panicking geometry
/// helper is the thing `placements` documents itself as refusing to be.
fn opens_at(tag: &str) -> EdgePlacement {
    spec::PANES
        .iter()
        .find(|pane| pane.tag == tag)
        .map_or(EdgePlacement::open(ChromeEdge::Left, 0), |pane| pane.opens)
}

/// Where the palette opens, as `spec::PANES` declares it.
fn palette_opens_at() -> EdgePlacement {
    opens_at("lab.palette")
}

/// Where the inspector opens, as `spec::PANES` declares it.
fn inspector_opens_at() -> EdgePlacement {
    opens_at("lab.inspector")
}
/// What a folded side panel leaves behind — the strip a person grabs to open it
/// again. Not zero, which is what makes a fold different from a hide.
const PANEL_STRIP_W: u32 = 18;

/// The two side panels' placements.
///
/// ★★★★★ R1802 — read straight off the thread-local state rather than threaded
/// through the layout, which is what makes this round small. R1801's carry said
/// this was 46 call sites of three functions; **that estimate was wrong**, and
/// wrong in the way this project keeps recording: it counted the callers of a
/// function without asking whether the function could reach the state itself.
/// `use_lab_state` has been thread-local since long before this axis, so the
/// answer was zero call sites.
///
/// Read WITHOUT `use_lab_state`, deliberately: that function constructs the
/// state if it is absent and registers an animation with the current owner, and
/// a geometry helper must do neither. Before the state exists — which is what a
/// test asking for a rectangle first sees — the answer is where the panels open.
fn placements() -> (EdgePlacement, EdgePlacement) {
    STATE.with(|slot| {
        slot.borrow().as_ref().map_or_else(
            || (palette_opens_at(), inspector_opens_at()),
            |s| (s.palette_at.get(), s.inspector_at.get()),
        )
    })
}

/// How much room the side panels take on each edge, as `(left, right)`.
///
/// A folded panel contributes its strip rather than nothing, so the canvas does
/// not swallow the handle that would open it again.
fn side_bands() -> (u32, u32) {
    let (palette, inspector) = placements();
    let mut left = 0;
    let mut right = 0;
    for at in [palette, inspector] {
        let t = at.thickness(PANEL_STRIP_W);
        match at.edge {
            ChromeEdge::Left => left += t,
            _ => right += t,
        }
    }
    (left, right)
}

/// Where one side panel lands, given what is stacked before it on its edge.
///
/// The palette is declared first, so on a shared edge it sits outermost — the
/// same order the specification lists them in, which is the only ordering a
/// reader of that table would predict.
fn side_panel_rect(at: EdgePlacement, before: u32) -> Rect {
    let (w, h) = window_size();
    let t = at.thickness(PANEL_STRIP_W);
    let x = match at.edge {
        ChromeEdge::Left => rail_w() + before,
        _ => w.saturating_sub(before + t),
    };
    Rect::new(x, app_bar_h(), t, h - app_bar_h())
}

fn canvas_rect() -> Rect {
    let (w, h) = window_size();
    let (left, right) = side_bands();
    Rect::new(
        rail_w() + left,
        app_bar_h() + TOOLBAR_H,
        w - rail_w() - left - right,
        h - app_bar_h() - TOOLBAR_H,
    )
}

fn palette_rect() -> Rect {
    side_panel_rect(placements().0, 0)
}

fn inspector_rect() -> Rect {
    let (palette, inspector) = placements();
    let before = if palette.edge == inspector.edge {
        palette.thickness(PANEL_STRIP_W)
    } else {
        0
    };
    side_panel_rect(inspector, before)
}

fn rail_rect() -> Rect {
    Rect::new(0, app_bar_h(), rail_w(), window_size().1 - app_bar_h())
}

/// ★★★★★ R1887 — **derived from the same bands the canvas is**, and it was not.
///
/// This function read [`PALETTE_W`] and [`INSP_W`] directly, which is the
/// opening placement rather than the live one. At rest the two agree, and R1802
/// recorded the divergence as a defect that was not yet alive because nothing
/// could move a panel. This round is the round that makes a panel move, so it
/// is also the round in which "not yet alive" expires: with the palette flipped
/// to the right, a toolbar computed from the opening widths starts under the
/// rail and runs past the inspector.
///
/// ⇒ ★ **A latent divergence is a defect with a date, and the date is whenever
/// somebody builds the thing that was missing.**
fn toolbar_rect() -> Rect {
    let (w, _) = window_size();
    let (left, right) = side_bands();
    Rect::new(
        rail_w() + left,
        app_bar_h(),
        w.saturating_sub(rail_w() + left + right),
        TOOLBAR_H,
    )
}

/// The height of a movable panel's own header — the band that holds its title
/// and the two controls that place it.
///
/// ★ [`panel_content`] says in as many words that a panel which GROWS a header
/// has to come back to it. This is that panel, and [`side_panel_content`] is
/// that visit: the band is reserved rather than drawn over the body, so the
/// rows inside keep every guarantee this screen's containment gates give them.
const PANEL_HEAD_H: u32 = 26;

/// The side panels a reader can place, as a value.
///
/// ★★★★★ R1887 — an enum rather than a pair of tags, because three things have
/// to agree about which panel is which: the press, the wire verb, and the
/// specification row that says where it may live. Two of those did not exist
/// before this round, and writing them against string tags is how they would
/// come to disagree.
/// ★★★★★ R1889 — serde, because a drag holds one of these and a `Drag` rides in
/// a `Signal`.
///
/// The same forcing R1801 recorded when placement became state: this framework
/// requires a signal's payload to be `Serialize + DeserializeOwned`, so
/// anything a gesture parks is a value a reader can read back. Not a
/// concession — it is why the arrangement of this screen is inspectable at all,
/// where the floor's equivalent round-trips as opaque bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum SidePanel {
    Palette,
    Inspector,
}

impl SidePanel {
    /// Both of them, in the order the specification lists them.
    const ALL: [Self; 2] = [Self::Palette, Self::Inspector];

    /// The painted tag this panel owns.
    const fn tag(self) -> &'static str {
        match self {
            Self::Palette => "lab.palette",
            Self::Inspector => "lab.inspector",
        }
    }

    /// The word a client names it by on the wire.
    const fn word(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::Inspector => "inspector",
        }
    }

    /// The panel a wire word names, if any.
    fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.word() == word)
    }

    /// This panel's row of the specification — where its policy is declared.
    ///
    /// Looked up by tag rather than by index: an index would be a second
    /// statement of the table's order, and the order is the table's.
    fn spec(self) -> &'static spec::PaneSpec {
        spec::PANES
            .iter()
            .find(|pane| pane.tag == self.tag())
            .expect("every placeable panel is a row of the pane specification")
    }

    /// Where it is now.
    fn at(self, state: &LabState) -> EdgePlacement {
        match self {
            Self::Palette => state.palette_at.get(),
            Self::Inspector => state.inspector_at.get(),
        }
    }

    /// ★ R1889 — the rectangle it currently occupies, in window coordinates.
    ///
    /// Asked through here rather than by calling one of the two rect functions
    /// at each site, because which of them applies is a fact about the panel
    /// and the callers that want it are about to grow: a caller that picked the
    /// wrong one would ask about the other panel and be right most of the time,
    /// which is the worst kind of wrong.
    fn rect(self) -> Rect {
        match self {
            Self::Palette => palette_rect(),
            Self::Inspector => inspector_rect(),
        }
    }

    /// Put it somewhere. Private to [`place_panel`], which is the only caller:
    /// a writer that skipped the policy would be the floor's own defect.
    fn put(self, state: &LabState, at: EdgePlacement) {
        match self {
            Self::Palette => state.palette_at.set(at),
            Self::Inspector => state.inspector_at.set(at),
        }
    }

    /// The edge this panel would flip to — the next edge its policy admits,
    /// cycling, so one control reaches every declared edge.
    ///
    /// ★ Derived from the declaration rather than written as "the other side".
    /// The policy admits two edges today and the specification is free to admit
    /// a third; a control spelled `Left => Right, _ => Left` would silently go
    /// on offering two of them.
    fn next_edge(self, state: &LabState) -> Option<ChromeEdge> {
        let allowed = self.spec().policy.allowed;
        let here = self.at(state).edge;
        let n = allowed.iter().position(|edge| *edge == here)?;
        allowed.get((n + 1) % allowed.len()).copied()
    }
}

/// What a caller is asking of a panel's placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaceAsk {
    /// Move it to this edge.
    Edge(ChromeEdge),
    /// Fold it to its strip, or unfold it.
    Fold(bool),
    /// ★ R1889 — make it this many logical pixels across.
    ///
    /// The third of the reference editor's region operators, and the last field
    /// of `EdgePlacement` nothing could change. Asked EXACTLY, not clamped: a
    /// drag clamps through `Resize::clamp` before it gets here, so this arm
    /// carries a number somebody meant and the policy is free to refuse it.
    Extent(u32),
}

/// ★★★★★ R1887 — **the one rule a placement change goes through**, whichever
/// channel asked.
///
/// The press and the wire verb both call this, so the screen and an agent
/// cannot come to disagree about what is allowed — the discipline this tree
/// applies wherever a gesture and a verb reach the same state.
///
/// The decision is [`pinion_core::edge_panel::EdgePolicy`]'s, not this
/// function's: it reads the panel's
/// own declared policy out of the specification and returns whatever that says.
/// ⇒ a panel's row of `spec::PANES` is the whole statement of what may happen
/// to it, and there is no second place to look.
///
/// # Errors
///
/// Whatever the policy refuses, carrying the edge asked for and the edges
/// allowed. ★ The floor accepts a disallowed move in silence (measured R1801);
/// this returns the refusal and the caller shows it.
fn place_panel(
    state: &LabState,
    panel: SidePanel,
    ask: PlaceAsk,
) -> Result<EdgePlacement, pinion_core::edge_panel::EdgeRefusal> {
    let policy = panel.spec().policy;
    let from = panel.at(state);
    let to = match ask {
        PlaceAsk::Edge(edge) => policy.admit(from, edge)?,
        PlaceAsk::Fold(folded) => policy.admit_fold(from, folded)?,
        PlaceAsk::Extent(extent) => policy.admit_extent(from, extent)?,
    };
    panel.put(state, to);
    Ok(to)
}

/// What a reader who never sees the drawing is told about placing this panel.
///
/// ★★★★★ R1887 — the names are DERIVED from the placement, so each says where
/// pressing would put it rather than naming the control. "Move the palette" is
/// a label; "move the palette to the right edge" is the fact a reader needs
/// before pressing, and it is the fact that changes when the panel moves.
///
/// A folded panel offers its strip and nothing else, which is what it paints:
/// an accessibility tree naming two controls a folded panel does not draw would
/// be describing a screen that is not there.
fn side_panel_access(state: &LabState, which: SidePanel) -> Vec<AccessNode> {
    let at = which.at(state);
    if at.folded {
        return vec![
            // ⚠ R1887.1 — the panel itself keeps its group node. It is still a
            // painted, addressable region when folded, and leaving it out left
            // `lab.palette` classified NOWHERE in the folded state, which the
            // census reported the moment that state was swept.
            AccessNode::new(which.tag(), AriaRole::Group)
                .with_name(which.spec().title)
                .with_value(AccessValue::Text("folded".to_owned())),
            AccessNode::new(format!("{}.strip", which.tag()), AriaRole::Button)
                .with_name(format!("{} is folded, open it again", which.spec().title)),
        ];
    }
    let mut nodes = Vec::new();
    if let Some(edge) = which.next_edge(state) {
        nodes.push(
            AccessNode::new(format!("{}.flip", which.tag()), AriaRole::Button).with_name(format!(
                "move the {} to the {} edge",
                which.word(),
                edge_word(edge)
            )),
        );
    }
    if which.spec().policy.foldable {
        nodes.push(
            AccessNode::new(format!("{}.fold", which.tag()), AriaRole::Button)
                .with_name(format!("fold the {} to its strip", which.word())),
        );
    }
    // ★★★★★ R1889 — the grip, published as a VALUE IN A RANGE rather than as a
    // button.
    //
    // A slider is what a resize grip is: a reader drags it and what changes is
    // one number between two bounds. Spelling it as a button would publish the
    // gesture and drop the numbers, and the numbers are the whole of what a
    // reader who never sees the drawing needs — they are also what makes the
    // wire verb usable, since `place palette,width=N` is refused outside them.
    // `AccessValue::Float` carries `valuenow` / `valuemin` / `valuemax`
    // together, so the three cannot be published out of step.
    if let pinion_core::edge_panel::Resize::Between { min, max } = which.spec().policy.resize {
        nodes.push(
            AccessNode::new(format!("{}.grip", which.tag()), AriaRole::Slider)
                .with_name(format!("resize the {}", which.word()))
                .with_value(AccessValue::Float {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a logical-pixel extent is three digits; f32 is exact here"
                    )]
                    value: at.extent as f32,
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "declared bounds, three digits, exact in f32"
                    )]
                    min: min as f32,
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "declared bounds, three digits, exact in f32"
                    )]
                    max: max as f32,
                }),
        );
    }
    nodes
}

/// What a reader is told when a panel will not go where it was asked.
///
/// ★★★★★ R1887 — the sentence names the panel, what was asked, and **what was
/// allowed**, because "no" is not something a reader can act on. The refusal
/// type already carries all three; this is the one place they become a
/// sentence, so the press and the wire cannot phrase it differently.
fn panel_refusal_sentence(
    which: SidePanel,
    refused: &pinion_core::edge_panel::EdgeRefusal,
) -> String {
    use pinion_core::edge_panel::EdgeRefusal;
    match refused {
        EdgeRefusal::EdgeNotAllowed { asked, allowed } => format!(
            "the {} does not go on the {} edge; it admits {}",
            which.word(),
            edge_word(*asked),
            if allowed.is_empty() {
                "no edge at all".to_owned()
            } else {
                allowed
                    .iter()
                    .map(|edge| edge_word(*edge))
                    .collect::<Vec<_>>()
                    .join(" and ")
            }
        ),
        EdgeRefusal::NotFoldable => format!("the {} does not fold", which.word()),
        // ★ R1889 — the same shape as `EdgeNotAllowed` above: what was asked
        // AND what is allowed, so the sentence names the number to ask next.
        EdgeRefusal::NotResizable => format!(
            "the {}'s width is fixed by this screen's specification",
            which.word()
        ),
        EdgeRefusal::ExtentOutOfRange { asked, min, max } => format!(
            "the {} does not go to {asked}; it resizes between {min} and {max}",
            which.word()
        ),
    }
}

/// A movable panel's header band, in the panel's own frame.
fn side_panel_head(rect: Rect) -> Rect {
    Rect::new(0, 0, rect.w, PANEL_HEAD_H.min(rect.h))
}

/// One of the two square controls in a panel's header, from the right.
fn side_panel_control(rect: Rect, nth: u32) -> Rect {
    let head = side_panel_head(rect);
    let size = head.h.saturating_sub(8);
    let right = head.w.saturating_sub(PAD);
    Rect::new(
        right.saturating_sub((nth + 1) * (size + 4)),
        head.y + 4,
        size,
        size,
    )
}

/// ★★★★★ R1889 — how wide the band a reader grabs to resize a panel is.
///
/// Six, which is the reference editor's own `AZONE_REGION` edge width and wide
/// enough to hit without the panel's own border becoming a target. It is a
/// constant rather than a specification column because it is a property of
/// POINTING, not of any one panel: every draggable edge on this screen wants
/// the same grab width, and a per-panel number would be four chances to make
/// one of them unhittable.
const PANEL_GRIP_W: u32 = 6;

/// ★★★★★ R1889 — the band on a panel's OUTER edge that a reader drags to
/// resize it, in the panel's own frame.
///
/// Outer, always: the edge that faces the canvas. A left-hand panel is grabbed
/// on its right side and a right-hand panel on its left, which is what makes
/// the gesture read as *pushing the boundary between the two* rather than as
/// *moving the panel*. Derived from the placement rather than written per
/// panel, so a flipped panel's grip flips with it — the half-derivation R1887
/// paid for.
/// ⚠ Inside [`panel_content`], not `(0, 0, w, h)` — and this is the SECOND time
/// this file has paid for the difference. R1887.1 drew a folded panel's strip at
/// the panel's own rectangle and it overhung the frame by a pixel on every side;
/// the first draft of this grip did the same thing and the containment gate
/// reported both grips the moment it ran. A box with a border is not the
/// rectangle inside it.
fn side_panel_grip(rect: Rect, at: EdgePlacement) -> Rect {
    let inside = panel_content(rect);
    let w = PANEL_GRIP_W.min(inside.w);
    let x = match at.edge {
        ChromeEdge::Left => inside.x + inside.w.saturating_sub(w),
        _ => inside.x,
    };
    Rect::new(x, inside.y, w, inside.h)
}

/// Whether this panel offers a grip at all: it must resize, and it must not be
/// folded.
///
/// ★ The fold half is not fussiness. A folded panel is an 18-pixel strip whose
/// whole area is already the affordance that brings it back (R1887), so a grip
/// there would put two gestures on one strip and the reader would get whichever
/// the hit test asked first.
fn side_panel_has_grip(state: &LabState, which: SidePanel) -> bool {
    which.spec().policy.resize.is_draggable() && !which.at(state).folded
}

/// What is left of a movable panel for its body, once its header is reserved.
///
/// The visit [`panel_content`] asks for by name.
fn side_panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
        &[pinion_core::style::Chrome::header(PANEL_HEAD_H)],
    )
}

/// The clearance a palette row keeps between its card and the words inside it.
///
/// ★ R1874 — named because three things spend it: the colour swatch, the two
/// stacked text lines, and [`PAL_ROW_H`] itself. It was the literal `6` in each
/// of them, so the swatch and the words agreed by coincidence.
const PAL_ROW_INSET: u32 = 6;

/// The height of one palette row, and the gap under a group heading.
///
/// ★★★★★ R1874 — **DERIVED, and deriving it is what the repair cost.** A row
/// holds two lines — a role's name over its gist — inside a card that keeps
/// [`PAL_ROW_INSET`] above and below them. Authored `40`, it held neither: the
/// two boxes were 14 and 12 tall for faces wanting 20 and 17, which is the only
/// reason 40 was ever enough, and the containment gate said so the moment the
/// boxes started consulting their faces — the words ran 1px past the card at
/// top and bottom, in every state, on all eight rows.
///
/// ⇒ **A box that respects its face forces the row that holds it to.** The
/// alternative — keeping 40 and shrinking the boxes back — is the defect this
/// campaign exists to remove, and it would have been invisible again.
///
/// The palette scrolls (R1662), so a taller row costs scroll extent and no
/// content: `legend_top` is derived from this constant and follows it.
const PAL_ROW_H: u32 = line_box(FONT_SMALL + 1) + line_box(10) + PAL_ROW_INSET * 2;
const PAL_HEAD_H: u32 = 22;
/// A pin's diameter.
const PIN: u32 = 11;
/// The zoom range, in percent, and the step a press moves it.
const ZOOM_MIN: u32 = 25;
const ZOOM_MAX: u32 = 400;
const ZOOM_STEP: u32 = 8;
/// The grid a ctrl-held node drag snaps to.
const SNAP: i32 = 22;

/// (R1653) Where world unit `0` sits inside the world surface.
///
/// The surface is a fixed extent the canvas viewport slides over, so a
/// coordinate on it is unsigned; the margin is what lets a node dragged to a
/// negative world position, or a pan to the left, still land on it.
const WORLD_ORIGIN: i32 = 2_000;
/// The world surface's extent, both axes.
const WORLD: i32 = WORLD_ORIGIN * 2 + 2_400;

// ── The reference's own colour tokens ───────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The lab's palette.
///
/// Two of these resolve from the theme's own roles rather than from a literal —
/// `Warning` and `Error` — because the gate's two outcomes are a framework
/// vocabulary (`ConfigDefect::blocks`) and painting them from a local constant
/// would be a second opinion about what "this warns" looks like. R1651 added
/// the warning tier to the theme for exactly this row.
#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    surface: Color,
    raised: Color,
    outline: Color,
    outline_2: Color,
    text: Color,
    text_2: Color,
    text_3: Color,
    accent: Color,
    accent_soft: Color,
    accent_line: Color,
    ok: Color,
    warn: Color,
    err: Color,
    grid: Color,
}

fn ink(theme: &Theme) -> Ink {
    Ink {
        bg: rgb(0x0E_0F12),
        surface: rgb(0x16_181D),
        raised: rgb(0x1E_2127),
        outline: rgb(0x2A_2E36),
        outline_2: rgb(0x3A_404B),
        text: rgb(0xE8_EBEF),
        text_2: rgb(0x98_A2AD),
        text_3: rgb(0x69_7180),
        accent: rgb(0xEC_5AA0),
        accent_soft: Color::rgba(0x9A, 0x00, 0x4F, 0x60),
        accent_line: Color::rgba(0xEC, 0x5A, 0xA0, 0x73),
        ok: rgb(0x35_C08B),
        warn: theme.resolve(ColorRole::Warning),
        err: theme.resolve(ColorRole::Error),
        grid: rgb(0x20_242C),
    }
}

/// The colour a transport is drawn in, which is what an accept pin's ring
/// means.
///
/// ★★★★★ R1926 — **derived from the taxonomy now, not written here.** A pin's
/// colour is a fact about the socket type it carries, so `Transport::tint` is
/// the one statement of it and this is the `Color` a scene needs. Before this
/// round the table lived here, which is how the canvas came to colour every
/// pin — including the halves a split put on the frame — by the **node's**
/// transport rather than by the port's own type.
const fn transport_ink(transport: Transport) -> Color {
    ink_of(transport.tint())
}

/// ★ R1926 — a model colour as a scene colour. Opaque, because a
/// [`Tint`] has no alpha to lose.
const fn ink_of(tint: Tint) -> Color {
    Color::rgba(tint.r, tint.g, tint.b, 255)
}

/// The colour a role is drawn in on the canvas card and the palette swatch.
const fn role_ink(role: Role) -> Color {
    match role {
        Role::Router => rgb(0xEC_5AA0),
        Role::Peer => rgb(0x2D_6CDF),
        Role::Client => rgb(0x69_7180),
        Role::Store => rgb(0x1F_8A4C),
        Role::Publisher | Role::Subscriber => rgb(0x8A_5CF6),
        Role::Querier | Role::Responder => rgb(0xC7_7800),
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// A drag in flight, and which kind it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Drag {
    /// The canvas is being panned from this cursor position.
    Pan { from: (u32, u32), start: (i32, i32) },
    /// A node is being placed. Held: the node, the grab offset in canvas units.
    Node {
        node: NodeId,
        grab: (i32, i32),
        snap: bool,
    },
    /// A link is being authored out of this node's dial pin.
    ///
    /// ★★★★★ R1915 — `port` is WHICH dial pin, because a split one is several.
    ///
    /// A RESOLVED INDEX and not a [`PortPath`], which is the opposite of the
    /// choice every other new site this round made — and it is the right one
    /// here for a reason worth stating. A drag is a gesture in progress: what
    /// it holds is *the port on the screen the hand started from*, and a
    /// resolved index is exactly that. Nothing can split a pin while a pointer
    /// is down, so the index cannot move underneath the drag. `path_of`
    /// converts at the moment of release, which is when the address is what the
    /// verb wants. (It also keeps this enum `Copy`, which is what the cell
    /// holding it needs — a consequence rather than the reason.)
    Wire { from: NodeId, port: u32 },
    /// ★ R1681 — a link that is already there is being re-aimed: it was picked
    /// up off the accept pin it lands on, and it follows the cursor from the
    /// pin that dials it.
    ///
    /// The link stays in the document for the whole drag, which the reference
    /// cannot do — its author-a-link has no way to move an end, so it splices
    /// the wire out on pick-up and re-adds it on drop, and a release that
    /// refuses has to remember to put it back. Here the move is one atomic
    /// verb, so nothing is taken out until something takes its place.
    Rewire { link: LinkId, from: NodeId },
    /// A host frame is being moved, and every card it holds moves with it.
    Frame { frame: NodeId, from: (i32, i32) },
    /// ★★★★★ R1889 — a side panel's width is being dragged by its grip.
    ///
    /// It holds only WHICH panel, and no grab offset or start width — unlike
    /// every other arm here. That is deliberate and it is what makes the
    /// gesture right: the boundary the reader is pushing IS the pointer, so the
    /// width is a function of the cursor's current position and the panel's
    /// edge, computed fresh each move. Storing a start width and adding a delta
    /// would let the two drift apart the moment a clamp bites — the pointer
    /// would keep accumulating past the bound and the panel would not come back
    /// until the cursor had travelled all the way home.
    PanelWidth { panel: SidePanel },
}

/// Which link the canvas has picked out (R1681).
///
/// Two arms because there are two kinds of link on this canvas and only one of
/// them is in the graph. A reported link is a **claim about** the topology, it
/// carries no [`LinkId`] because it is not a link, and the affordance it offers
/// is the opposite one: an authored link can be deleted, and a reported link
/// can only be taken into the drawing. The reference reaches the same split and
/// spells it as a flag on the wire plus a predicate; the difference is that
/// here a reported link cannot be handed to anything that takes a `LinkId`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum LinkPick {
    /// One somebody drew.
    Authored(LinkId),
    /// One a source reported, named by the pair it runs between — which is the
    /// only identity an observation has.
    Observed(Socket, Socket),
}

impl LinkPick {
    /// The authored link, or `None` for a reported one.
    const fn authored(self) -> Option<LinkId> {
        match self {
            Self::Authored(id) => Some(id),
            Self::Observed(..) => None,
        }
    }
}

/// ★★★ R1683 — what the screen's ONE text field is editing, or `None` while it
/// is closed.
///
/// One field with a target rather than a field per site, which is the sibling
/// node editor's arrangement and the reason it is worth copying: a second field
/// is a second focus owner, a second keymap and a second commit path, and the
/// three sites here want exactly the same behaviour over different values.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Editing {
    /// The selected card's name.
    Name(NodeId),
    /// A configuration path being typed into the selected card's form, which
    /// the catalogue does not offer.
    Key(NodeId),
    /// ★★★ R1684 — the VALUE on one row of that form, named by the
    /// configuration path the row is about.
    ///
    /// The third target of the one field and the one that made the target
    /// worth having: a form has as many rows as the document has keys, so a
    /// box per row would be a box per key. The field moves to the row instead
    /// — see [`edit_box`].
    Value {
        /// Whose form.
        node: NodeId,
        /// Which row, by the configuration path it is about.
        key: String,
        /// ★★ Which ELEMENT of a list row, or `None` for the row's whole
        /// value.
        ///
        /// A list is painted as a row per element and a row that adds one, and
        /// the affordance that adds one puts a *placeholder* there — so
        /// without this the screen could grow a list and never say what went
        /// in it. The reference has no per-element row at all (it types the
        /// whole list as one separated string, which is still what `None`
        /// does here), so this is the half that is better rather than the half
        /// that is the same.
        element: Option<usize>,
    },
}

impl Editing {
    /// The word the wire reads this back as.
    ///
    /// ★ A `String` since R1684, because the third target has to say WHICH row:
    /// `value` alone would read back the same for every row of the form, and a
    /// slot that cannot distinguish the thing being edited is a slot an agent
    /// cannot drive from.
    fn wire(&self) -> String {
        match self {
            Self::Name(_) => "name".to_owned(),
            Self::Key(_) => "key".to_owned(),
            Self::Value {
                key, element: None, ..
            } => format!("value:{key}"),
            Self::Value {
                key,
                element: Some(n),
                ..
            } => format!("value:{key}[{n}]"),
        }
    }

    /// What the keystroke gate lets through.
    ///
    /// ★★ `Text` for every target INCLUDING an integer row, and that is a
    /// decision rather than an oversight. The form's own three defect kinds are
    /// its report — a value out of range, a value of the wrong type, a value
    /// missing — and a keymap that refused the letters would make
    /// [`ConfigDefect::WrongType`] unreachable by any person, leaving a defect
    /// arm only an agent could produce. The reference does the same and says
    /// why beside its own parser: swallowing a bad keystroke tells the person
    /// nothing, while holding it and failing validation tells them exactly
    /// what is wrong.
    ///
    /// [`ConfigDefect::WrongType`]: pinion_core::widgets::config_form::ConfigDefect::WrongType
    const fn kind(&self) -> CellKind {
        match self {
            Self::Name(_) | Self::Key(_) | Self::Value { .. } => CellKind::Text,
        }
    }
}

/// The tag the shared field is painted under, which is also what owns focus
/// while it is open.
const EDIT_TAG: &str = "lab.edit";

/// Everything the screen is.
struct LabState {
    doc: Tracked<Document<LabNode>>,
    forms: Tracked<BTreeMap<NodeId, ConfigForm>>,
    frames: RefCell<BTreeMap<NodeId, String>>,
    /// ★★ R1679 — where each card came into being: its canvas position and the
    /// host it started on.
    ///
    /// A record rather than a derivation, and the reason is the one case a
    /// derivation cannot cover. The opening cards' placement IS in
    /// [`spec::NODES`] and R1678 compared against it directly — but a card the
    /// palette adds is not in the specification at all, so it had no baseline,
    /// and the layout predicate answered "unchanged" for a card a person had
    /// visibly dragged across the canvas.
    ///
    /// Written once at creation, from the specification for the opening cards
    /// and from the placement arithmetic for an added one, so the two kinds of
    /// card answer the same question the same way and the scope has ONE
    /// population instead of a rule with an exception in it.
    opened_at: RefCell<BTreeMap<NodeId, Placement>>,
    /// ★★★ R1706 — the selected cards, and which of them the inspector
    /// follows.
    ///
    /// Was `Option<NodeId>` — a leader with no set — and the reference's own
    /// frame gesture is *select the group, then move it*, so "select this
    /// host's cards" had nowhere to land. A set with no leader is the other
    /// half of the same hole and this tree has that one too: the material node
    /// editor holds a `BTreeSet` and its `selected` slot answers nothing at all
    /// whenever two cards are selected.
    ///
    /// [`Selection`] holds both, with the leader an index into the members so
    /// that "the inspector follows something that is not selected" is
    /// unrepresentable rather than merely avoided.
    /// ★★★★★ R1726 — **the order the cards are stacked in**, front last.
    ///
    /// The card order was the specification's, so picking a card up raised it
    /// for exactly as long as the gesture lasted and dropping it put it back
    /// underneath whatever it was dropped on. Measured: during a drag the held
    /// card painted at index 101 against the other's 77, and the moment it was
    /// released it went back to 70 against 80 — the two overlapped and the one
    /// just placed was the hidden one.
    ///
    /// So which card is in front is a fact this screen has to OWN, not one a
    /// gesture can lend it: the transient half is
    /// [`ContainerNode::with_held`](pinion_core::scene::ContainerNode::with_held),
    /// and this is the half that survives the release. Only cards named here
    /// are ordered by it; anything else keeps its declared position, so a card
    /// nobody has touched is exactly where the specification puts it.
    ///
    /// Position is NOT changed by this — a node's place on the canvas is what
    /// the person meant by putting it there, and the free-canvas rule (which
    /// every node editor keeps, and which this tree's tile dashboard
    /// deliberately does not) is that a drop displaces nothing.
    stacking: RefCell<Vec<NodeId>>,
    selection: Signal<Selection<NodeId>>,
    selected_link: Signal<Option<LinkPick>>,
    /// ★★★★★ R1919 — **what a reader is looking for**, and nothing more.
    ///
    /// The HITS are not kept beside it: they are derived from this and the
    /// document on every read (`found`), so a search cannot go stale against a
    /// node that was renamed, added or deleted after it ran. Both references
    /// keep a result list and both have to invalidate it; a search that is a
    /// function of the document has nothing to invalidate.
    searching: Signal<String>,
    zoom: Signal<u32>,
    pan: Signal<(i32, i32)>,
    running: Signal<bool>,
    /// R1789 — **what happens to this graph, and when**: named lanes of timed
    /// acts, which the census recorded as having no authoring surface at all.
    ///
    /// A `RefCell` and not a `Signal`, like the document beside it: a schedule
    /// is edited in place by four verbs and read whole, so a signal would fire
    /// the whole screen for a change to one entry.
    scenario: RefCell<scenario::Plan>,
    /// ★★★★★ R1791 — whether the toolbar's overflow control is open.
    ///
    /// A moved group has to stay REACHABLE, or the round trades a visual defect
    /// for a functional one — which is worse, and is what the gates caught on
    /// the first run: `lab.toolbar.config` stopped being pressable at all. The
    /// floor's extension button opens a menu for exactly this reason.
    toolbar_open: Signal<bool>,
    /// ★★★★★ R1802 — where the palette sits, as a VALUE.
    ///
    /// R1801 gave this screen's specification a placement POLICY — the palette
    /// may live on the left or the right — and left the layout drawing it from
    /// a `const` on the left, always. A published rule the code does not keep,
    /// which is the shape this project keeps paying for, and the closing audit
    /// of that round said so. This is the other half.
    ///
    /// Seeded to exactly where the hand-written layout put it, so adopting the
    /// value moved no pixel; what changed is that it can now be somewhere else.
    palette_at: Signal<EdgePlacement>,
    /// Where the inspector sits. See [`LabState::palette_at`].
    inspector_at: Signal<EdgePlacement>,
    /// Where the scenario's playhead stands, in seconds.
    ///
    /// ★ Advanced explicitly (`advance`), never by a wall clock — R1600's
    /// division, and the only one under which an assertion about this screen
    /// does not depend on how fast the machine is.
    playhead: Signal<f32>,
    /// ★★★★★ R1844 — what the scenario's checkpoints have decided so far.
    ///
    /// A verdict has to OUTLIVE the advance that raised it, which is why this
    /// is state and the other four acts need none: `start` is finished the
    /// instant it is crossed, and a `check` with a timeout is not — it is
    /// waiting, and something has to remember that it is. Cleared when the
    /// playhead restarts, because a verdict from the previous run of a plan is
    /// the most misleading thing this screen could show.
    checks: RefCell<Vec<scenario::Checkpoint>>,
    /// ★★★★★ R1866 — **what THIS run of the scenario has done so far**, as the
    /// timeline a regression is computed from.
    ///
    /// One mark per act the playhead crossed, at the act's own moment. Kept
    /// beside `checks` and cleared with it for the same reason: a mark from the
    /// previous run of a plan would make the comparison below answer about two
    /// runs that were never separate.
    ///
    /// ⚠ Seconds and not steps. The census row asks for two runs compared "on
    /// order and latency distribution", and this screen has a real clock
    /// (`advance` takes the seconds), so the order is recoverable from the
    /// times and the latency is not recoverable from an order. Recording the
    /// coarser scale would have thrown away the half that cannot be rebuilt.
    tape: RefCell<pinion_core::regression::Timeline>,
    /// ★★★★★ R1866 — the run this screen compares against: a tape somebody
    /// deliberately kept.
    ///
    /// `None` until a reader records one, and that is the honest opening state
    /// rather than seeding it with the first run: a regression against a
    /// baseline nobody chose is a comparison with an accident.
    baseline: RefCell<Option<pinion_core::regression::Timeline>>,
    /// The master auto-discovery switch: off by default, because a graph whose
    /// links are all authored is the one whose behaviour is a function of what
    /// is on the canvas.
    discovery: Signal<bool>,
    cursor: Signal<(u32, u32)>,
    /// ★★★★★ R1916 — whether the pointer is in the window at all.
    ///
    /// A second signal beside `cursor` and not an `Option` inside it, because
    /// the two answer different questions and both have callers: every gesture
    /// that reads `cursor` wants *the last place the pointer was*, which is
    /// still the right answer while it is away (a drag released outside the
    /// window commits where it left), and only the hover derivations want *is
    /// anybody pointing at anything*. Folding them would have made every
    /// existing reader handle an absence it does not care about.
    pointer_inside: Signal<bool>,
    drag: Signal<Option<Drag>>,
    /// ★★★★★ R1924 — the cards that would take the wire being re-aimed, worked
    /// out once when it is picked up.
    ///
    /// A set of CARDS rather than of sockets, because a drop opens the slot it
    /// lands on — the port a socket would name does not exist until the wire
    /// arrives. Empty while nothing is being re-aimed, which is also the honest
    /// answer for a wire that has nowhere else to go.
    ///
    /// Computed at pick-up rather than per frame because the document cannot
    /// change while a pointer is down, and re-deriving it on every pointer move
    /// would clone the document once per pixel. It is filled in the same
    /// statement that sets `drag`, and `drag` is the signal the canvas already
    /// re-reads, so this holds no reactivity of its own.
    rewire_targets: RefCell<BTreeSet<NodeId>>,
    /// ★ R1924 — the card the verdict was last said about, so a drag says it
    /// once on arrival rather than on every pixel of travel across the card.
    rewire_over: Cell<Option<NodeId>>,
    pressed: RefCell<Option<Hit>>,
    /// ★★★★★ R1719 — the last thing this screen SAID, and what kind of thing
    /// it was.
    ///
    /// `None` is "has not said anything yet", which is now the only way to
    /// express that: an [`Utterance`] refuses an empty clause, so the empty
    /// string this field used to hold — a screen announcing, and announcing
    /// nothing — has no spelling.
    /// ★★★★★ R1778 — and its LIFETIME, in the holder the framework owns. This
    /// screen's message used to stay up forever; a reader saw it stacked under
    /// the host's over this very canvas.
    toast: Rc<pinion_core::utterance::Saying>,
    /// ★★ R1683 — what the shared field is editing, or `None` while it is shut.
    editing: Signal<Option<Editing>>,
    /// ★★★★★ R1732 — the inspector row whose roster is open, and where in it
    /// the reader is.
    ///
    /// `None` is "nothing is open", which is the only way to express it: a
    /// [`Picker`] exists exactly while a roster does, so "shut and highlighting
    /// the fourth option" has no spelling. The pair is the row's key and the
    /// picking, because a picker does not know which row it belongs to and a
    /// key does not know where the reader is — two facts, and a screen that
    /// derived either from the other could not show a reader moving away from
    /// the word the document holds.
    ///
    /// A signal rather than a `RefCell`, because opening one has to repaint.
    picking: Signal<Option<(String, Picker)>>,
    /// The buffer that field holds.
    ///
    /// ★★★ **The hook's own object, taken once when the screen is built — not
    /// a second one.** `use_text_edit_state` resolves through the shell's root
    /// owner, which lives as long as the application, so the painter and this
    /// hold ONE buffer; but it PANICS outside an owner scope, and this screen's
    /// pointer handlers and its wire both run outside one (the same fact R1662
    /// met with the scroll offsets). So the reference is taken where an owner
    /// is guaranteed — here, since this type is only ever constructed from
    /// inside one — and kept for the paths that have none.
    ///
    /// ★ Taken at construction rather than at the first paint, and the gate is
    /// what forced it: an agent that opened the editor before the screen had
    /// painted got "there is no field", which is a state no session reaches and
    /// a refusal nobody should have to think about.
    ///
    /// That the two are one object is asserted rather than assumed —
    /// `r1683_the_screen_and_the_painter_hold_one_buffer` compares them with
    /// `Rc::ptr_eq` after a paint, which is what would fail if the owner were
    /// ever per-frame.
    buffer: Rc<TextEditState>,
    /// R1662 — the two side panes' scroll offsets, held here rather than
    /// reached for with `use_scroll_state` because the paint and the hit test
    /// both need them and only the paint runs inside an `Owner` scope. One
    /// object, so the two cannot read two facts.
    palette_scroll: Rc<ScrollState>,
    inspector_scroll: Rc<ScrollState>,
    /// ★★ R1687 — the two artifacts that leave this screen, once they have.
    ///
    /// See [`Produced`] for why they are latched rather than derived: producing
    /// one is an operation, and an operation whose slot already held the answer
    /// would have nothing to witness.
    produced: RefCell<Produced>,
    /// ★★ R1689 — where a saved graph goes.
    ///
    /// Taken at construction for the reason the edit buffer above is: the hook
    /// resolves through the shell's root owner and PANICS outside one, and this
    /// screen's pointer handlers and its wire both run outside an owner scope.
    storage: Rc<AppStorage>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<LabState>>> = const { RefCell::new(None) };
}

/// Put the screen back to the state it opens in.
///
/// ★ R1677 — the operation gate needs it, and needs it for a reason worth
/// stating: it asks of each operation "does causing it change something", which
/// is only a fair question from a screen that has not already been changed by
/// the operation before. The swept states next door are deliberately
/// cumulative — a session with the tool is one edit on top of another — and
/// that is the wrong shape for a gate whose rows have to be independent.
///
/// Test-only because production has exactly one screen and never wants a
/// second: a reset reachable from the running application would be an
/// operation nobody declared.
#[cfg(test)]
fn reset_lab_state() {
    STATE.with(|slot| *slot.borrow_mut() = None);
}

/// ★★★★★ R1909 — **a gate declaring that it examines an OPEN inspector**, and
/// putting the panel back when it is done.
///
/// The specification opens this pane folded from R1909 on, so every check that
/// asks about the pane's contents — where a form row is, which control answers
/// a press, whether a grip is offered — now has to say which state it is asking
/// about. This is that sentence, spelled once.
///
/// # Why a guard rather than a call
///
/// Because [`use_lab_state`] hands back a `thread_local` that OUTLIVES the
/// owner: a test that unfolds the inspector and returns has changed the screen
/// for every later test on that thread, and the two R1889 grip gates would then
/// pass or fail on their position in the run order. The gates in this file that
/// move a panel already put it back by hand, which is a rule; a guard that
/// restores on drop is the same discipline made unforgettable — and it restores
/// through the panel's own handle rather than by clearing the `thread_local`,
/// which is the mistake that once cost this file a diagnosis (see
/// `r1802_a_folded_panel_leaves_a_strip_the_canvas_does_not_take`).
///
/// ⚠ It restores to the SPECIFICATION's opening placement, not to whatever was
/// there before. That is deliberate and is what a test wants: a gate must not
/// be able to hand the next one a state a previous one invented.
#[cfg(test)]
#[must_use = "the guard restores the panel when it is dropped; binding it to `_` \
              drops it immediately and unfolds nothing"]
struct WithTheInspectorOpen;

#[cfg(test)]
impl WithTheInspectorOpen {
    /// Unfold the inspector, through the same rule a press goes through.
    fn now(state: &LabState) -> Self {
        place_panel(state, SidePanel::Inspector, PlaceAsk::Fold(false))
            .expect("the inspector declares that it folds, so it unfolds");
        Self
    }
}

#[cfg(test)]
impl Drop for WithTheInspectorOpen {
    fn drop(&mut self) {
        // Through the handle the layout reads, so the restore lands where the
        // next test looks. Not `place_panel`: this must succeed while a test is
        // panicking, and a policy refusal during unwind would abort.
        STATE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state.inspector_at.set(inspector_opens_at());
            }
        });
    }
}

/// The scroll state a pane body tag names, or `None` for a tag that is not a
/// pane body.
///
/// ★ R1662 — one lookup, so a caller that has a `scene/scroll_reach` row (which
/// names the viewport by tag) can act on it without a second table mapping tags
/// to states. Test-only here because the screen's own two call sites reach
/// their state by name; the wire drives a pane through `scene/scroll`, which
/// resolves the node's own [`ScrollState`] and never needs this table.
/// A press, moved into the frame a scrolling pane's rectangles are stated in.
///
/// ★ R1662 — the pane's content slides under a fixed viewport, so the paint of
/// a row is `row - offset` and the only way for a hit test written against
/// `row` to stay true is to ask about `point + offset`. One direction, one
/// place: the alternative is a second set of rectangles that has to be kept in
/// step, which is what [`FormGeometry::translated`] avoids on the other pane —
/// there the geometry itself is published to assistive technology, so it is the
/// geometry that moves.
///
/// ★★★ R1684.2 — **[`PANEL_FRAME`] is part of it, and its absence was the same
/// defect the inspector had.** The palette's rectangles are written in window
/// coordinates that embed the pane's origin and NOT the outline the body is
/// drawn inside, while the paint hands the body [`panel_content`] — so every
/// row was hit-tested one pixel up and left of where it is painted. Found by
/// generalising R1684's corner check from one tag family to every pressable tag
/// the screen declares: 243 of 2464 corners answered `nothing`, and the pattern
/// was the signature — the top-left corner of each palette row resolved and the
/// other three did not, which is exactly what a rectangle shifted by one looks
/// like.
///
/// That is the second instance of the class in one screen, and both were
/// invisible for the same reason: every check aimed at a centre.
fn in_pane(scroll: &ScrollState, pane: Rect, px: u32, py: u32) -> (u32, u32) {
    let (ox, oy) = scroll.offset();
    // ★★★★★ R1887 — the panel's own content origin, not [`PANEL_FRAME`]. Those
    // were the same number exactly while a side panel reserved nothing of
    // itself; the header this round adds moved the origin, and the constant
    // spelling of it put every palette row a header's height out of step with
    // the paint — which `r1662_a_control_one_scroll_away_is_pressable_after_
    // that_scroll` reported as 36 published offsets that did not deliver.
    // ★★★★★ R1887 — the SCROLL OFFSET alone. It also folded [`PANEL_FRAME`],
    // which said that what the pane paints sits one pixel below what its rows
    // state; that is no longer true, because the palette's rectangles are
    // stated from the body's own origin now (`palette_body_origin`) and the
    // paint subtracts exactly the same origin. Painted equals stated, so the
    // only thing between a press and a row is how far the pane has scrolled.
    let _ = pane;
    let fold = |v: u32, by: i32| -> u32 {
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "clamped into u32's range on the line above the cast"
        )]
        let folded = (i64::from(v) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
        folded
    };
    (fold(px, ox), fold(py, oy))
}

#[cfg(test)]
fn pane_scroll(state: &LabState, body: &str) -> Option<Rc<ScrollState>> {
    match body {
        PALETTE_SCROLL => Some(Rc::clone(&state.palette_scroll)),
        INSPECTOR_SCROLL => Some(Rc::clone(&state.inspector_scroll)),
        // ★★ R1714 — the window's own pan is a viewport a `scroll_reach` row can
        // name, so it has to resolve here too. It was the one name in that
        // report this table could not answer, which made every recipe naming it
        // look like "a mark off the window that cannot be scrolled to" — the
        // exact reading the pan exists to make false.
        pinion_core::shrink::PAN_TAG => Some(pinion_core::shrink::pan_state(VIEW_TAG)),
        _ => None,
    }
}

/// Where a saved graph goes — the machine's data directory in a running
/// application, and memory under a test.
///
/// ★★ R1689 — **stated once, here, rather than injected at each test's call
/// site.** The node editor learned this in R852 and wrote it in a helper every
/// one of its fixtures has to remember to use; this screen has forty tests and
/// only two of them are about saving, so the one that forgot would silently
/// write into the developer's real data directory and the next run would open
/// with somebody else's graph.
///
/// The real backend is not left unproven by that: it is what the demo exercises,
/// under an isolated directory, across two launches — which is also the only
/// place the file half can be proven, since a unit test in one process cannot
/// show that a graph survived the process.
fn app_storage() -> Rc<AppStorage> {
    #[cfg(test)]
    let storage = pinion_core::reactive::Owner::current()
        .expect("a lab state is only built inside an owner scope")
        .cache(persist::STORAGE_CACHE_KEY, || {
            AppStorage::new(Box::new(pinion_core::storage::InMemoryStorage::new()))
        });
    #[cfg(not(test))]
    let storage =
        pinion_platform_storage::use_app_storage(persist::STORAGE_CACHE_KEY, persist::STORAGE_APP);
    storage
}

fn use_lab_state() -> Rc<LabState> {
    let state = STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(state) = slot.as_ref() {
            return Rc::clone(state);
        }
        let state = Rc::new(LabState::opening());
        *slot = Some(Rc::clone(&state));
        state
    });
    // ★★★★★ R1778 — the toast's clock, registered ONCE PER OWNER.
    //
    // The sibling shell registers through `register_animation_once`, which
    // builds the holder inside the owner's cache. That cannot be copied here,
    // and the difference is the point: THIS screen's state is a `thread_local`
    // that OUTLIVES owners. A holder built per owner would be a second one,
    // leaving the state's own untouched; a holder built once and registered on
    // every pass would be counted down once per view.
    //
    // So the holder belongs to the state and the REGISTRATION is what the
    // owner-scoped marker makes once. Without it a fresh owner — which is what
    // every test builds — would keep the state's toast and tick nothing, and
    // the lifetime would be silently absent exactly where it is checked.
    if let Some(owner) = pinion_core::reactive::Owner::current()
        && !owner.cache_contains::<()>(TOAST_TICKER_KEY)
    {
        owner.register_animation(
            Rc::clone(&state.toast) as Rc<dyn pinion_core::animation::Tickable>
        );
        let _marker = owner.cache(TOAST_TICKER_KEY, || ());
    }
    state
}

impl LabState {
    /// The graph the screen opens with — built from [`spec`], so the screen and
    /// the specification cannot disagree about what it holds.
    fn opening() -> Self {
        let mut doc: Document<LabNode> = Document::new(spec::GRAPH_NAME);
        let mut ids = BTreeMap::new();
        let mut frames = BTreeMap::new();
        let mut frame_ids: BTreeMap<&str, NodeId> = BTreeMap::new();

        for frame in spec::FRAMES {
            let (x, y, _, _) = frame.rect;
            let id = doc
                .add_node(
                    ROOT,
                    NodeBody::Frame,
                    i32::try_from(x).unwrap_or(0),
                    i32::try_from(y).unwrap_or(0),
                )
                .expect("the root tree exists");
            if let Some(node) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(id)) {
                node.label = Some(format!("{} · {}", frame.name, frame.gist));
            }
            frame_ids.insert(frame.name, id);
            frames.insert(id, frame.name.to_owned());
        }

        let mut forms = BTreeMap::new();
        seed_nodes(&mut doc, &frame_ids, &mut ids, &mut forms);

        let selected_link = seed_links(&mut doc, &forms, &ids).map(LinkPick::Authored);

        let selection = ids
            .get(spec::SELECTED_NODE)
            .copied()
            .map_or_else(Selection::empty, Selection::one);
        // R1679 — the opening placement of every card the specification
        // describes, recorded from the specification itself so the record and
        // the graph cannot have been built from different numbers.
        let opened_at: BTreeMap<NodeId, Placement> = spec::NODES
            .iter()
            .filter_map(|want| {
                let id = *ids.get(want.id)?;
                let (x, y, _) = want.rect;
                Some((
                    id,
                    Placement {
                        at: (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
                        host: Some(want.frame.to_owned()),
                        opened_as: Some(want.id.to_owned()),
                    },
                ))
            })
            .collect();
        // ★ R1682 — `ids` above is a BUILD-TIME convenience and is dropped
        // here. It used to be kept as a field, which made it a second record of
        // what a card is called; the document holds the one record now, and a
        // map that dies at the end of this function cannot drift from anything.
        Self {
            doc: Tracked::new(doc),
            forms: Tracked::new(forms),
            frames: RefCell::new(frames),
            opened_at: RefCell::new(opened_at),
            // Empty: nothing has been picked up yet, so every card is exactly
            // where the specification declares it.
            stacking: RefCell::new(Vec::new()),
            selection: Signal::new(selection),
            selected_link: Signal::new(selected_link),
            searching: Signal::new(String::new()),
            zoom: Signal::new(spec::OPENING_ZOOM),
            pan: Signal::new((0, 0)),
            running: Signal::new(false),
            scenario: RefCell::new(scenario::Plan::new()),
            toolbar_open: Signal::new(false),
            palette_at: Signal::new(palette_opens_at()),
            inspector_at: Signal::new(inspector_opens_at()),
            playhead: Signal::new(0.0),
            checks: RefCell::new(Vec::new()),
            tape: RefCell::new(pinion_core::regression::Timeline::new(
                pinion_core::regression::Scale::Seconds,
            )),
            baseline: RefCell::new(None),
            discovery: Signal::new(false),
            cursor: Signal::new((0, 0)),
            // ★ False on the opening frame, and that is the honest state: a
            // screen nobody has pointed at yet has no pointer in it. The first
            // `move_cursor` sets it.
            pointer_inside: Signal::new(false),
            drag: Signal::new(None),
            rewire_targets: RefCell::new(BTreeSet::new()),
            rewire_over: Cell::new(None),
            pressed: RefCell::new(None),
            // ★ R1778 — silent at open, which every screen of this tool now
            // spells the same way: a holder with nothing said yet.
            toast: Rc::new(pinion_core::utterance::Saying::new(TOAST_SECONDS)),
            editing: Signal::new(None),
            picking: Signal::new(None),
            buffer: use_text_edit_state(EDIT_TAG),
            palette_scroll: Rc::new(ScrollState::with_tag(PALETTE_SCROLL)),
            inspector_scroll: Rc::new(ScrollState::with_tag(INSPECTOR_SCROLL)),
            produced: RefCell::new(Produced::default()),
            storage: app_storage(),
        }
    }

    /// The deployment plan this graph describes right now.
    ///
    /// ★★ **One derivation, and both artifacts are rendered from it.** The
    /// order is the model's ([`Document::launch_order`]), so a disabled card
    /// drops out of both at once and neither rendering has an opinion about it.
    fn plan(&self) -> deploy::Plan {
        // R1788 — the derivation is the framework's (`Document::deployment`)
        // and the ORDER is derived inside it. Until this round the screen read
        // `launch_order` itself and handed the sequence to a plan builder
        // living in this binary, so nothing stopped the two from disagreeing
        // and no second consumer could reach either.
        self.doc.borrow().deployment(
            ROOT,
            |node| self.host_of(node),
            |node| {
                self.role_of(node)
                    .map(|role| deploy::program_of(role).to_owned())
            },
            // 🟥★★★★★ R1716 — the form the screen SHOWS, and this line is the
            // round's own defect caught in its own terms. It read the STORED
            // form, which is the authored half: the plan a person exports would
            // have carried neither the mode a node's role implies nor the
            // addresses its drawn links dial, while the `document` read beside
            // it carried both. Two answers to "what is this node's
            // configuration", one of them shipped — which is the exact shape
            // this round exists to end, made one round later by the round
            // itself. Found by driving the plan rather than by reading it.
            |node| deploy::configured(shown_form(self, node)),
        )
    }

    /// Say something to the person in front of the screen.
    ///
    /// ★★★★★ R1719 — takes an [`Utterance`] and not a string, which is what
    /// makes the 58 call sites below each answer "what KIND of thing is this".
    /// Before this round they all handed over a `String`, so the one fact
    /// downstream needs — is this a refusal? — was carried in a `"refused: "`
    /// prefix at five of them and nowhere at all at the rest. Both the live
    /// region's urgency and the toast's colour now derive from the tone, so
    /// neither can be set to a constant that is right for half of what this
    /// screen says.
    fn say(&self, what: Utterance) {
        self.toast.say(what);
    }

    /// The node the canvas labels `id`, or `None`.
    ///
    /// ★★ R1682 — the DOCUMENT's answer. This screen kept its own
    /// `BTreeMap<String, NodeId>` beside the document's own
    /// [`Node::label`](pinion_node_graph::Node::label) until the rename arrived
    /// and made the duplication load-bearing: two records of one fact, and a
    /// rename that updated either one alone would leave the canvas and the wire
    /// calling the same card two different things. The model owns names now —
    /// it is the thing that can *refuse* a name already taken — so there is one
    /// record and no way to update half of it.
    fn node_of(&self, id: &str) -> Option<NodeId> {
        self.doc.borrow().node_labelled(ROOT, id)
    }

    /// The card the inspector follows — the leader of whatever is selected.
    ///
    /// Named for the question rather than for the field, so the twenty-odd
    /// readers that only ever wanted "the one card" did not each have to learn
    /// that a selection is now a set.
    fn active_card(&self) -> Option<NodeId> {
        self.selection.get().active().copied()
    }

    fn name_of(&self, node: NodeId) -> String {
        self.doc
            .borrow()
            .tree(ROOT)
            .and_then(|tree| tree.node(node))
            .map_or_else(|| format!("#{}", node.0), Node::display_name)
    }

    fn role_of(&self, node: NodeId) -> Option<Role> {
        match self.doc.borrow().tree(ROOT)?.node(node)?.body {
            NodeBody::Kind(ref kind) => Some(kind.role),
            _ => None,
        }
    }

    /// The host frame a card sits in, by name — `None` for a card in none.
    ///
    /// 🟥★★★★★ R1716 — **one derivation, where there were two and one of them
    /// was wrong.** `frames` is keyed by the FRAME's node, so the question
    /// "which host is this card on" is a walk to the card's parent and then a
    /// lookup. The wire's `frames` read did that walk; the launch plan handed
    /// the same map a CARD and asked it directly, which can only ever miss —
    /// and measured before this round, the exported plan put **all eight
    /// nodes** on one host called `unplaced` while the canvas drew them across
    /// two host frames. The script started everything in one place and nothing
    /// said so.
    ///
    /// So the walk lives here, and everything that wants a host asks: the
    /// plan, the inspector's own placement row, and the address a drawn link
    /// dials.
    fn frame_of(&self, node: NodeId) -> Option<String> {
        let parent = self.doc.borrow().tree(ROOT)?.node(node)?.parent?;
        self.frames.borrow().get(&parent).cloned()
    }

    /// Where a card runs, naming the somewhere a card in no frame still runs.
    ///
    /// A plan with a hole in it would be one whose script silently skipped a
    /// process, which is why this is total — see [`Self::frame_of`] for the
    /// half that says the truth about a card with no frame.
    fn host_of(&self, node: NodeId) -> String {
        // ★★ R1716 — a card that has been TOLD where it runs runs there. The
        // placement row is worked out from the frame until somebody takes it
        // over, and after that the row is the fact — otherwise taking it over
        // would produce a value the screen shows and the plan ignores, which is
        // the two-answers-to-one-question this round exists to end. The canon
        // reads its own field first for the same reason.
        //
        // The STORED form, deliberately: the derived row is built from this, so
        // reading the shown one would be a loop.
        self.forms
            .borrow()
            .get(&node)
            .and_then(|form| form.field("host").map(|f| f.value().trim().to_owned()))
            .filter(|written| !written.is_empty())
            .or_else(|| self.frame_of(node))
            .unwrap_or_else(|| deploy::UNPLACED.to_owned())
    }

    /// Every node the canvas draws a card for: the declared ones in
    /// specification order, then anything added since.
    ///
    /// ★ Derived from the **document**, not from the specification table. It
    /// was the table until the real-pointer run showed a node added from the
    /// palette never reaching the canvas — the specification says what the
    /// screen OPENS with, and a screen that could only ever draw that is a
    /// picture.
    fn cards(&self) -> Vec<NodeId> {
        let opening: Vec<NodeId> = spec::NODES
            .iter()
            .filter_map(|n| self.node_of(n.id))
            .collect();
        let doc = self.doc.borrow();
        let Some(tree) = doc.tree(ROOT) else {
            return opening;
        };
        let mut all = opening.clone();
        for node in tree.nodes() {
            if matches!(node.body, NodeBody::Kind(_)) && !opening.contains(&node.id) {
                all.push(node.id);
            }
        }
        // ★★★★★ R1726 — then the cards that have been PICKED UP, in the order
        // they were, last. A card nobody has touched keeps the position the
        // specification gives it, which is why this is a stable partition
        // rather than a sort: the screen still opens exactly as declared.
        let raised = self.stacking.borrow();
        let mut resting: Vec<NodeId> = all
            .iter()
            .copied()
            .filter(|id| !raised.contains(id))
            .collect();
        resting.extend(raised.iter().copied().filter(|id| all.contains(id)));
        resting
    }

    /// ★★★★★ R1726 — this card was picked up, so it is in front from now on.
    ///
    /// Called when a drag STARTS rather than when it ends, for two reasons: a
    /// press that turns out not to move still means "I am working on this one",
    /// and a card that only came forward on release would spend the whole
    /// gesture underneath the thing it is being dragged over — which is the
    /// defect this pays off, moved later rather than removed.
    fn raise(&self, node: NodeId) {
        let mut stacking = self.stacking.borrow_mut();
        stacking.retain(|id| *id != node);
        stacking.push(node);
    }

    /// Every address a card on this canvas can be reached at.
    ///
    /// ★★ R1717 — resolved the same way [`dialled_from`] resolves a link's
    /// landing, host substitution and all, because the two sets are compared
    /// and a set built by a second rule would report every address as strange.
    fn listen_addresses(&self) -> BTreeSet<String> {
        let mut known = BTreeSet::new();
        for card in self.cards() {
            let Some(form) = shown_form(self, card) else {
                continue;
            };
            let Some(listen) = form.field("listen.endpoints") else {
                continue;
            };
            let host = self.host_of(card);
            let shown = listen.value();
            for endpoint in FieldType::elements(&shown) {
                known.insert(endpoint.replace("0.0.0.0", &host).replace("[::]", &host));
            }
        }
        known
    }

    /// The addresses **somebody wrote** on this card that nothing on this
    /// canvas listens on.
    ///
    /// ★★★★★ R1717 — the behaviour canon's own warning, and the one that
    /// survives now that a drawn link always reaches the row. It is asked of
    /// the **written** half alone: a derived address is one this canvas drew,
    /// so it is inside the graph by construction and asking about it would
    /// report the drawing to itself.
    fn dials_outside(&self, node: NodeId) -> Vec<String> {
        let Some(written) = shown_form(self, node)
            .as_ref()
            .and_then(|form| form.field(DIALLED_KEY))
            .and_then(ConfigField::written)
            .map(str::to_owned)
        else {
            return Vec::new();
        };
        let known = self.listen_addresses();
        FieldType::elements(&written)
            .filter(|address| !known.contains(*address))
            .map(str::to_owned)
            .collect()
    }

    /// The gate's findings: every form's own defects, plus the three the
    /// *graph* raises.
    ///
    /// All three graph warnings are derived rather than listed. A node whose
    /// role can be dialled and whose listen endpoint is empty is a pin nobody
    /// can reach; a node that has turned discovery on can acquire links this
    /// canvas did not author, which is the same fact the master switch states
    /// for the graph; and a node told to dial an address nothing here listens
    /// on has made the drawing stop being the whole picture.
    fn defects(&self) -> Vec<(String, Finding)> {
        let mut found = Vec::new();
        // ★★★★★ R1818 — the forms this walk already resolved, kept for the
        // set-level check below. Asking `shown_form` a second time would be a
        // SECOND WALK, and this function's own R1717 note is that the count and
        // the list must come from one — the same reason `problems` renders
        // `defects` rather than re-deriving it.
        let mut seen: Vec<(String, ConfigForm)> = Vec::new();
        // ★★★★★ R1927 — the framework's answers for the WHOLE graph, asked
        // ONCE. [`Document::warnings`] is the call the reference has no
        // equivalent of — there the badge is decided inside the node widget, so
        // *what is wrong on this canvas* has to be assembled by whoever wants
        // it — and this is the consumer that would otherwise have left it
        // driven by a proof alone. Asking node by node inside the loop below
        // would also be this screen re-deriving the list the crate already
        // publishes, which is the R1717 shape this function exists to avoid.
        let said: BTreeMap<NodeId, String> = self
            .doc
            .borrow()
            .warnings(ROOT)
            .into_iter()
            .map(|held| (held.node, held.sentence))
            .collect();
        for node in self.cards() {
            let name = self.name_of(node);
            // ★★ R1716 — the form the screen SHOWS. A gate over the stored half
            // would not look at the rows this screen works out, and those rows
            // reach the document like any other.
            if let Some(form) = shown_form(self, node) {
                seen.push((name.clone(), form.clone()));
                let form = &form;
                for defect in form.defects() {
                    found.push((name.clone(), Finding::Value(defect)));
                }
                // ★★★★★ R1717 — **this card dials outside the graph.** R1716
                // warned about the mirror image of this — a drawn link the card
                // did not dial — which was compensation for a row that could
                // not hold two contributions at once. It can now, so every
                // drawn address is in the row by construction and that warning
                // could never fire again.
                //
                // What is left is the fact underneath it, and it is the one the
                // behaviour canon raises: an address somebody wrote that
                // nothing in this graph listens on. It is not an error — a node
                // may legitimately be told to reach an already-running router —
                // but it means the drawing is no longer the whole picture, so
                // anything this screen concludes about what reaches what is
                // being concluded from a partial graph. That is worth saying
                // and is not worth blocking a launch for.
                for outside in self.dials_outside(node) {
                    found.push((name.clone(), Finding::DialsOutside(outside)));
                }
                let listens = form
                    .field("listen.endpoints")
                    .is_some_and(|f| !f.value().trim().is_empty());
                if self.role_of(node).is_some_and(Role::accepts) && !listens {
                    found.push((name.clone(), Finding::NothingListening));
                }
                // ★★★★★ R1927 — **the framework's own answer**, folded into the
                // one walk rather than computed a second time here. The rule
                // lives on the kind because it is a fact about this node in
                // this graph; what belongs to the screen is only where to put
                // the sentence.
                if let Some(sentence) = said.get(&node) {
                    found.push((name.clone(), Finding::Unwired(sentence.clone())));
                }
                if form
                    .field("discovery.multicast.enabled")
                    .is_some_and(|f| f.value().trim() == "true")
                {
                    found.push((name.clone(), Finding::DiscoveryOn));
                }
            }
        }
        // ★★★★★ R1818 — the check the loop above CANNOT make, and the reason it
        // is written after it rather than inside it.
        //
        // Every finding above is decided by looking at one card. Uniqueness is
        // a property of the SET, so a per-card pass is structurally unable to
        // reach it — which is exactly why the identifier's declared shape has
        // been enforced since R1690 while two cards holding the same one went
        // unremarked by everything. `ConfigSchema::collisions` is the framework
        // vocabulary that was missing; the schema declares which paths must be
        // unique and this asks that question of every card at once.
        //
        // The forms are collected rather than re-derived per collision:
        // `shown_form` is the form the screen SHOWS (R1716), and asking for it
        // twice would let the gate judge a document nobody is looking at.
        let named = seen;
        for collision in settings::schema().collisions(named.iter().map(|(n, f)| (n.clone(), f))) {
            for holder in &collision.holders {
                // Reported against EVERY holder, so whichever card a reader is
                // looking at says so — a collision named only on one of them
                // would make the other look clean.
                let others: Vec<String> = collision
                    .holders
                    .iter()
                    .filter(|h| *h != holder)
                    .cloned()
                    .collect();
                found.push((
                    holder.clone(),
                    Finding::Collision {
                        path: collision.path.clone(),
                        value: collision.value.clone(),
                        others,
                    },
                ));
            }
        }
        // ★★★★★ R1885 — **a wire between two builds that cannot negotiate.**
        //
        // A property of a PAIR, so the per-card loop above cannot reach it for
        // the same structural reason the collision check cannot — and one step
        // further out, because a collision is a property of a set of cards and
        // this is a property of a set of cards AND the wires between them.
        //
        // The verdict is the crate's (`Document::validate`) rather than a walk
        // written here. `Document::connect` already refuses such a wire, so the
        // only way to be holding one is to have made it legal and then changed
        // a node's build — which is exactly what a person does on this screen,
        // and exactly what a validation pass is for.
        {
            let doc = self.doc.borrow();
            for violation in doc.validate() {
                let Violation::Incompatible { link, refusal, .. } = violation else {
                    continue;
                };
                let Some(link) = doc.tree(ROOT).and_then(|t| t.link(link)) else {
                    continue;
                };
                // Named on the end the refusal blames, because that is the card
                // a person has to change; the sentence names the other.
                let (blamed, peer) = match refusal.end {
                    Side::Output => (link.from.node, link.to.node),
                    Side::Input => (link.to.node, link.from.node),
                };
                found.push((
                    self.name_of(blamed),
                    Finding::Incompatible {
                        peer: self.name_of(peer),
                        because: refusal.because.clone(),
                    },
                ));
            }
        }
        found
    }

    /// ★★★ R1688 — **the gate's findings with the card each one is ON**, which
    /// is the fact the panel had been throwing away.
    ///
    /// [`gate_lines`](Self::gate_lines) formatted a name into a sentence and
    /// returned the sentence; the card's identity was in the string and nowhere
    /// else. That was enough while the only consumer was a list of lines, and it
    /// stopped being enough the moment a person could ask to be TAKEN to the
    /// first one — a second walk over the same forms, re-deriving the same
    /// order, is how two answers to "what is wrong first" come to exist.
    ///
    /// So the walk is here, once, and the panel is a rendering of it. Same order
    /// as the cards, which is the reference's own (its jump takes issue zero of
    /// its validation list, in node order, warnings and errors alike): a person
    /// asking for "the first problem" means the first one they would read.
    fn problems(&self) -> Vec<Problem> {
        self.defects()
            .into_iter()
            .map(|(who, defect)| {
                let sentence = format!("{who} · {}", defect.sentence());
                Problem {
                    // A card whose name no longer resolves is not skipped: the
                    // problem is real and a `None` says the screen cannot take
                    // anyone to it, which is the honest answer and one the jump
                    // reports rather than swallowing.
                    node: self.node_of(&who),
                    blocks: defect.blocks(),
                    sentence,
                }
            })
            .collect()
    }

    /// ★ R1717 — built from the SAME walk the panel renders, so the count and
    /// the list cannot disagree. A graph warning is carried as an unknown key
    /// here for one reason: [`Verdict`] counts blocking against non-blocking
    /// and that arm is the framework's non-blocking one, so the arithmetic is
    /// the framework's rather than a second rule written here.
    fn verdict(&self) -> Verdict {
        let defects: Vec<ConfigDefect> = self
            .defects()
            .into_iter()
            .map(|(who, finding)| match finding {
                Finding::Value(defect) => defect,
                other => ConfigDefect::UnknownKey {
                    key: format!("{who} · {}", other.sentence()),
                },
            })
            .collect();
        Verdict::over(&defects)
    }

    /// The sentence the gate shows for a defect — the framework's when the
    /// framework raised it, this application's when it did.
    ///
    /// ★ R1688 — a rendering of [`problems`](Self::problems), not a second walk.
    fn gate_lines(&self) -> Vec<(bool, String)> {
        self.problems()
            .into_iter()
            .map(|problem| (problem.blocks, problem.sentence))
            .collect()
    }

    fn link_count(&self) -> usize {
        self.doc.borrow().tree(ROOT).map_or(0, |t| t.links().len())
    }

    /// How many links arrive at, and leave, a node.
    fn degree(&self, node: NodeId) -> (usize, usize) {
        let doc = self.doc.borrow();
        let Some(tree) = doc.tree(ROOT) else {
            return (0, 0);
        };
        let inbound = tree.links().iter().filter(|l| l.to.node == node).count();
        let outbound = tree.links().iter().filter(|l| l.from.node == node).count();
        (inbound, outbound)
    }
}

/// ★★★ R1688 — one thing wrong with the graph, and **which card it is on**.
///
/// The identity is the whole reason this type exists. The gate panel only ever
/// needed a sentence and a severity, so that is all
/// [`LabState::gate_lines`] answered — and "take me to the first problem" is
/// unanswerable from a list of sentences without parsing the name back out of
/// one, which is a second derivation of a fact the first walk had and dropped.
struct Problem {
    /// The card, or `None` when the finding names something no card answers to.
    node: Option<NodeId>,
    /// Whether it stops the launch, as opposed to merely being worth saying.
    blocks: bool,
    /// The sentence the gate panel shows, and the toast the jump says.
    sentence: String,
}

/// One thing the launch gate found — **a value's defect, or the graph's.**
///
/// ★★★★★ R1717 — found by LOOKING at the running screen. The graph's three
/// warnings had no type of their own, so each was smuggled inside a
/// [`ConfigDefect::UnknownKey`] whose `key` was really a sentence, and
/// [`LabState::problems`] recovered the sentence by **sniffing the key's
/// suffix**. Two of the three had an arm and read correctly; the third fell
/// through to the framework's own wording and the panel said
///
/// > R-01 · R-01 · connect.endpoints · tcp/10.0.0.21:7449 reaches outside
/// > is not a key the target knows; it starts and ignores it
///
/// — the card named twice and a sentence about unknown keys glued onto a fact
/// that is not about one. **R1716 shipped that and every gate was green**,
/// because the checks over it asked whether the address was NAMED and never
/// whether the sentence read. A photograph of the panel answered it in one
/// look.
///
/// So the vocabulary is a type. A defect of a value is one arm and each thing
/// the *graph* can be wrong about is its own, the sentence comes from the arm,
/// and there is no string left to sniff.
#[derive(Debug, Clone, PartialEq, Eq, pinion_derive::VariantCensus)]
enum Finding {
    /// A value this form cannot carry — the framework's own three.
    Value(ConfigDefect),
    /// A node that can be dialled and is listening nowhere.
    NothingListening,
    /// A node that has turned discovery on, so links can appear that this
    /// canvas did not author.
    DiscoveryOn,
    /// An address somebody wrote that nothing on this canvas listens on, so the
    /// drawing is no longer the whole picture.
    DialsOutside(String),
    /// ★★★★★ R1927 — what the node's own KIND says is questionable about it,
    /// carried verbatim.
    ///
    /// The sentence is the framework's answer, not this screen's: a second
    /// wording here would be a second statement about one finding, free to
    /// disagree with the one a consumer of the model reads.
    Unwired(String),
    /// ★★★★★ R1818 — **a value the schema says is unique, held by more than one
    /// card.**
    ///
    /// The first finding here that is not a property of ONE card. Every other
    /// arm is decided by looking at a single form; this one cannot be, which is
    /// why the identifier's SHAPE was enforced from R1690 and its UNIQUENESS by
    /// nothing at all: a form is one document and is structurally unable to see
    /// its siblings.
    ///
    /// It BLOCKS. An identifier two nodes answer to is not a partial picture
    /// the way a dialled-outside address is — it is a graph in which "the node
    /// called `beef`" does not name one node, so anything launched from it acts
    /// on whichever the tool happened to reach first.
    Collision {
        /// The schema path that must be unique.
        path: String,
        /// The value more than one card holds.
        value: String,
        /// The other cards holding it, named — because "this is a duplicate" is
        /// not actionable without saying *of what*.
        others: Vec<String>,
    },
    /// ★★★★★ R1885 — **a wire between two builds that share no wire revision.**
    ///
    /// The first finding on this screen that is a property of a LINK. Every
    /// other arm is decided from one card or from the set of cards; this one
    /// needs the wire as well, which is why it could not exist until
    /// `NodeKind::admits` gave the crate somewhere to ask about a pair.
    ///
    /// It BLOCKS. Two peers that cannot negotiate are not a partial picture the
    /// way a dialled-outside address is — the drawing asserts a session that
    /// cannot be established, so anything launched from it fails at the wire.
    Incompatible {
        /// The card at the other end, named: "this one is incompatible" is not
        /// actionable without saying *with what*.
        peer: String,
        /// The taxonomy's own sentence, which names both builds and both spans.
        because: String,
    },
}

impl Finding {
    /// Whether it stops a launch.
    ///
    /// Every graph warning is `false` and says so here rather than by being
    /// counted somewhere: a graph the tool does not fully know is a reason to
    /// tell somebody, never a reason to refuse to start what they drew.
    fn blocks(&self) -> bool {
        match self {
            Self::Value(defect) => defect.blocks(),
            Self::NothingListening
            | Self::DiscoveryOn
            | Self::DialsOutside(_)
            // R1927 — the same class as `DialsOutside` and for the same
            // reason: it says the drawing is partial, not that it is wrong.
            | Self::Unwired(_) => false,
            // The two that BLOCK, and they block for one reason stated two
            // ways: what the graph says would happen cannot happen. A name two
            // nodes answer to does not name one node, so a launch acts on
            // whichever the tool reached first; a wire between two builds that
            // share no revision asserts a session that cannot be established.
            // Neither is a partial picture — the other three arms are — and
            // that is the line this function draws.
            Self::Collision { .. } | Self::Incompatible { .. } => true,
        }
    }

    /// The sentence the gate panel shows, without the card's name — the caller
    /// puts that in front, once.
    fn sentence(&self) -> String {
        match self {
            Self::Value(defect) => defect.sentence(),
            Self::NothingListening => "nothing is listening, so no node can dial it".to_owned(),
            // R1927 — the framework's sentence, verbatim.
            Self::Unwired(said) => said.clone(),
            Self::DiscoveryOn => {
                "discovery is on, so links may appear that nobody authored".to_owned()
            }
            Self::DialsOutside(address) => format!(
                "{DIALLED_KEY} reaches {address}, which nothing here listens on — \
                 the drawing is not the whole picture"
            ),
            Self::Collision {
                path,
                value,
                others,
            } => format!(
                "{path} is {value}, which {} already holds — it must be unique",
                others.join(" and ")
            ),
            // The taxonomy's own words, verbatim: the rule that refused the
            // wire is the only thing that knows why, and paraphrasing it here
            // would put a second author on one sentence.
            Self::Incompatible { peer, because } => {
                format!("cannot reach {peer}: {because}")
            }
        }
    }
}

/// Put every declared node on the canvas, in its declared frame, holding the
/// form its role opens with.
///
/// A node's transport — which is the colour its accept pin is drawn in and the
/// type a link to it must match — is **derived from its endpoint** rather than
/// declared beside it, so the canvas cannot show a colour the configuration
/// does not have.
fn seed_nodes(
    doc: &mut Document<LabNode>,
    frame_ids: &BTreeMap<&str, NodeId>,
    ids: &mut BTreeMap<String, NodeId>,
    forms: &mut BTreeMap<NodeId, ConfigForm>,
) {
    for node in spec::NODES {
        let role = Role::from_name(node.role).expect("the spec names a role that exists");
        let form = form_for(node.id, role);

        let listen = form
            .field("listen.endpoints")
            .map(|f| f.value().into_owned())
            .unwrap_or_default();
        let transport = Transport::of_locator(&listen)
            .or_else(|| {
                form.field("connect.endpoints")
                    .and_then(|f| Transport::of_locator(&f.value()))
            })
            .unwrap_or(Transport::Tcp);
        let (x, y, _) = node.rect;
        let id = doc
            .add_node(
                ROOT,
                NodeBody::Kind(LabNode {
                    role,
                    transport,
                    listening: !listen.is_empty(),
                    implementation: opening_implementation(node.id),
                }),
                i32::try_from(x).unwrap_or(0),
                i32::try_from(y).unwrap_or(0),
            )
            .expect("the root tree exists");
        if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(id)) {
            slot.label = Some(node.id.to_owned());
        }
        if let Some(&frame) = frame_ids.get(node.frame) {
            doc.set_parent(ROOT, id, Some(frame)).ok();
        }
        ids.insert(node.id.to_owned(), id);
        forms.insert(id, form);
    }
}

/// ★★★ R1678 — what a reset puts back, and the fact its affordance is derived
/// from.
///
/// The reference tool keeps every edit as an OVERLAY on the opening state, so
/// each of its five resets is one `clear` and each "is there anything to put
/// back" is one `is_empty`. This screen mutates its document in place instead,
/// so both facts are derived by comparing against [`crate::spec`] — which is
/// where the opening state came from in the first place (`seed_nodes` /
/// `seed_links` build it from exactly these constants), so there is no snapshot
/// that could fall out of step with anything.
///
/// **The two halves are one type on purpose.** A screen that decided for itself
/// when to show a "put it back" affordance would be a second author of the
/// rule, and the failure mode is silent in both directions: an affordance shown
/// over an unchanged screen does nothing when pressed, and one hidden over a
/// changed screen strands the change. [`changed`](Self::changed) and
/// [`apply`](Self::apply) are asserted against each other — after an apply, the
/// scope reports unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResetScope {
    /// Which cards exist — the palette's additions go away.
    Nodes,
    /// Where the cards sit AND which host each starts on.
    ///
    /// One scope covering two facts because that is what the reference does
    /// (measured: its layout reset clears both maps and says so in its own
    /// toast). They belong together — a card dragged onto another host has
    /// moved and been re-parented by one gesture, so putting one back without
    /// the other leaves a state no gesture could have produced.
    Layout,
    /// Every form: values, and rows added or taken away.
    Fields,
    /// The authored links, and which one is selected.
    Links,
    /// Pan and zoom.
    View,
}

impl ResetScope {
    /// The census. Consumers iterate this rather than re-listing the arms.
    const ALL: [Self; 5] = [
        Self::Nodes,
        Self::Layout,
        Self::Fields,
        Self::Links,
        Self::View,
    ];

    /// The scope words, as the declaration publishes them.
    ///
    /// Built FROM [`ALL`](Self::ALL) rather than listed beside it, so the
    /// vocabulary an agent is offered and the arms this type has cannot come
    /// apart — a hand-written copy would still compile with an arm missing.
    const WIRE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut out = [""; Self::ALL.len()];
        let mut n = 0;
        while n < Self::ALL.len() {
            out[n] = Self::ALL[n].wire();
            n += 1;
        }
        out
    };

    /// The word the wire and the specification call this scope.
    const fn wire(self) -> &'static str {
        match self {
            Self::Nodes => "nodes",
            Self::Layout => "layout",
            Self::Fields => "fields",
            Self::Links => "links",
            Self::View => "view",
        }
    }

    /// Whether this scope has an affordance ON THE PANEL, which it has only
    /// when there is something to put back.
    ///
    /// ★ The view is deliberately not one of these — measured on the reference,
    /// its four graph resets are wrapped in a conditional and its VIEW reset is
    /// not, sitting unconditionally in the zoom cluster. That asymmetry is a
    /// judgement worth keeping: pan and zoom always have a home to go to and
    /// the button is one glyph wide, while a graph reset that appears out of
    /// nowhere over an untouched screen is an invitation to destroy work.
    const fn gated(self) -> bool {
        !matches!(self, Self::View)
    }

    /// Whether the screen differs from what it opened as, in this scope.
    fn changed(self, state: &LabState) -> bool {
        match self {
            // ★★ R1682 — by IDENTITY, not by re-deriving the opening set from
            // what the cards are currently called. A card is a stray when
            // nothing recorded it opening; an opening card differs when it no
            // longer shows the name it opened as. Comparing the name list
            // against the specification answered both questions with one
            // string comparison, and a rename makes those two questions give
            // opposite answers about the same card.
            Self::Nodes => {
                let cards = state.cards();
                let opened = state.opened_at.borrow();
                cards.len() != spec::NODES.len()
                    || cards.iter().any(|node| {
                        opened.get(node).and_then(|born| born.opened_as.as_deref())
                            != Some(state.name_of(*node)).as_deref()
                    })
            }
            // ★ R1679 — over EVERY card, against where each came into being.
            // The population was `spec::NODES`, which cannot see a card the
            // palette added: measured, dragging one moved it 60 by 36 and this
            // answered false.
            Self::Layout => state
                .cards()
                .into_iter()
                .any(|node| placed_as_opened(state, node) == Some(false)),
            // The form answers for itself — values and shape both. See
            // `ConfigForm::edited`, which is where that question belongs.
            Self::Fields => state.forms.borrow().values().any(ConfigForm::edited),
            Self::Links => {
                let doc = state.doc.borrow();
                let Some(tree) = doc.tree(ROOT) else {
                    return false;
                };
                let mut now: Vec<(String, String)> = tree
                    .links()
                    .iter()
                    .map(|l| (state.name_of(l.from.node), state.name_of(l.to.node)))
                    .collect();
                let mut want: Vec<(String, String)> = spec::LINKS
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect();
                now.sort();
                want.sort();
                now != want
            }
            Self::View => state.zoom.get() != spec::OPENING_ZOOM || state.pan.get() != (0, 0),
        }
    }

    /// Put this scope back to what the screen opened with.
    fn apply(self, state: &Rc<LabState>) {
        match self {
            Self::Nodes => put_node_set_back(state),
            Self::Layout => put_cards_back(state),
            Self::Fields => {
                let nodes: Vec<NodeId> = state.forms.borrow().keys().copied().collect();
                for form in state.forms.borrow_mut().values_mut() {
                    form.revert();
                }
                // The pins are DERIVED from the form (`sync_node`), so a revert
                // that stopped at the values would leave a card drawing the
                // transport of an endpoint it no longer holds.
                for node in nodes {
                    sync_node(state, node);
                }
            }
            // ★★ R1679 — a DIFF, not a rebuild, and the gate is what forced it.
            //
            // The first version cleared every link and re-authored the whole
            // set from the specification. Correct in what it left behind and
            // wrong in what it published: the model assigns a fresh identifier
            // to each new link, so putting an UNTOUCHED graph back renumbered
            // all seven of them. `r1679_a_reset_affordance_is_painted_exactly_
            // when_it_would_do_something` caught it in all eight swept states —
            // the affordance was correctly absent and pressing it would still
            // have changed the screen.
            //
            // The reference's reset is idempotent because it drops an overlay
            // that is already empty. This one has to earn that: it removes only
            // the links the specification does not have and adds only the ones
            // it is missing, so a link nobody touched keeps its identity and a
            // reset over an unchanged graph does nothing at all.
            //
            // ★ Deliberately does NOT consult `changed`. An `apply` that
            // early-returned on "nothing differs" would make the gate above
            // compare the predicate with itself, which is the tautology this
            // whole round exists to remove.
            Self::Links => {
                let mut want: Vec<(String, String)> = spec::LINKS
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect();
                let mut drop_these: Vec<LinkId> = Vec::new();
                {
                    let doc = state.doc.borrow();
                    if let Some(tree) = doc.tree(ROOT) {
                        for link in tree.links() {
                            let pair = (state.name_of(link.from.node), state.name_of(link.to.node));
                            // One `want` entry per live link, so a duplicated
                            // pair keeps exactly as many as the specification
                            // declares and no more.
                            match want.iter().position(|p| *p == pair) {
                                Some(at) => {
                                    want.remove(at);
                                }
                                None => drop_these.push(link.id),
                            }
                        }
                    }
                }
                {
                    let mut doc = state.doc.borrow_mut();
                    for link in drop_these {
                        doc.disconnect(ROOT, link).ok();
                    }
                }
                for (from, to) in want {
                    let (Some(a), Some(b)) = (state.node_of(&from), state.node_of(&to)) else {
                        continue;
                    };
                    connect(state, a, b).ok();
                }
                if let Some(id) = state
                    .node_of(spec::SELECTED_LINK.0)
                    .zip(state.node_of(spec::SELECTED_LINK.1))
                    .and_then(|(a, b)| {
                        let doc = state.doc.borrow();
                        let tree = doc.tree(ROOT)?;
                        tree.links()
                            .iter()
                            .find(|l| l.from.node == a && l.to.node == b)
                            .map(|l| l.id)
                    })
                {
                    state.selected_link.set(Some(LinkPick::Authored(id)));
                }
            }
            Self::View => {
                state.zoom.set(spec::OPENING_ZOOM);
                state.pan.set((0, 0));
            }
        }
    }
}

/// Put the card SET back: the palette's additions go, and every opening card
/// answers to the name it opened as.
///
/// ★★ R1682 — the two halves are one operation and the second is what renaming
/// forced. A stray is a card with **no opening record**; a renamed opening card
/// is not one. Selecting strays by "its name is not in the specification" — the
/// only question there was to ask before names could change — deleted the very
/// card whose name this scope exists to put back.
fn put_node_set_back(state: &Rc<LabState>) {
    let strays: Vec<NodeId> = state
        .cards()
        .into_iter()
        .filter(|n| {
            state
                .opened_at
                .borrow()
                .get(n)
                .and_then(|born| born.opened_as.as_deref())
                .is_none()
        })
        .collect();
    // Which names have to go back, decided BEFORE the strays are removed: a
    // name freed by a deletion is one a rename may have taken.
    let restore: Vec<(NodeId, String)> = state
        .cards()
        .into_iter()
        .filter(|n| !strays.contains(n))
        .filter_map(|n| {
            let born = state.opened_at.borrow().get(&n)?.opened_as.clone()?;
            (state.name_of(n) != born).then_some((n, born))
        })
        .collect();
    {
        let mut doc = state.doc.borrow_mut();
        for node in &strays {
            doc.remove_node(ROOT, *node).ok();
        }
    }
    state
        .forms
        .borrow_mut()
        .retain(|id, _| !strays.contains(id));
    // ★ R1679 close-audit — and its placement with it. Every other per-card map
    // is cleaned here; `opened_at` was added that session and missed, which
    // would leave a placement behind for a card that no longer exists.
    // Harmless today because the model does not reuse an identifier, and
    // exactly the kind of "harmless today" that stops being so without a diff.
    state
        .opened_at
        .borrow_mut()
        .retain(|id, _| !strays.contains(id));
    for (node, name) in restore {
        // Through the same verb the rename action uses, so "put the name back"
        // and "change the name" cannot be two rules about what a name is. It
        // cannot be refused here: the name is the one this card opened with,
        // and whatever had taken it was either renamed away or is a stray now
        // gone.
        rename_card(state, node, &name).ok();
    }
    // ★ R1706 — a selection is a set, so a stray is PRUNED out of it rather
    // than the whole selection being thrown away because one of its members
    // went. The lead only moves when the card holding it is one of the strays,
    // and a selection emptied that way falls back to the opening card.
    let mut selection = state.selection.get();
    if selection.retain(|n| !strays.contains(n)).changed() {
        if selection.is_empty() {
            selection = state
                .node_of(spec::SELECTED_NODE)
                .map_or_else(Selection::empty, Selection::one);
        }
        state.selection.set(selection);
    }
}

/// Put every card back where it came into being, on the host it started on.
///
/// One function because the two halves are one operation — see [`Placement`].
fn put_cards_back(state: &Rc<LabState>) {
    for node in state.cards() {
        let Some(opened) = state.opened_at.borrow().get(&node).cloned() else {
            continue;
        };
        let frame = opened.host.and_then(|want| {
            state
                .frames
                .borrow()
                .iter()
                .find(|(_, name)| **name == want)
                .map(|(id, _)| *id)
        });
        let mut doc = state.doc.borrow_mut();
        if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(node)) {
            slot.x = opened.at.0;
            slot.y = opened.at.1;
        }
        doc.set_parent(ROOT, node, frame).ok();
    }
}

/// Where a card came into being: its canvas position, and the host it started
/// on — `None` for a card the palette added, which belongs to no host until it
/// is dropped on one.
///
/// A named pair rather than a tuple because both halves are put back TOGETHER
/// by one operation (the reference's layout reset clears position and host in
/// one call and says so in its own message), and a caller holding two loose
/// values can restore one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Placement {
    at: (i32, i32),
    host: Option<String>,
    /// ★★ R1682 — the name in [`spec::NODES`] this card came into being as, or
    /// `None` for one the palette added.
    ///
    /// A third field on the same record rather than a second map keyed the same
    /// way, for the R1679 reason this record exists at all: it is written ONCE,
    /// where the card is created, so the two kinds of card answer the same
    /// question the same way. It is a *different* scope's business — the node
    /// reset puts names back, the layout reset puts the pair above back — and
    /// [`placed_as_opened`] deliberately does not look at it.
    ///
    /// **Renaming is what forced it.** Before it, the node reset told an
    /// opening card from an added one by comparing its NAME against the
    /// specification, which is exactly the thing a rename changes: a renamed
    /// opening card read as a stray, and the reset that was supposed to put its
    /// name back would have deleted the node instead. The same lesson R1679
    /// wrote for the link reset — put back by identity, never by re-deriving
    /// from what a thing is currently called.
    ///
    /// ★ R1689 — owned rather than `&'static str`, because this record is
    /// SAVED now and a borrowed name cannot come back from a file. The static
    /// lifetime was carrying a second claim — "this is one of the
    /// specification's" — which is a fact about the value and not about how
    /// long it lives; [`declared_card`] asks the specification directly, which
    /// is also what makes a saved name that the specification no longer has
    /// answer "no opinion" instead of dangling.
    opened_as: Option<String>,
}

/// Whether this card sits where it came into being, on the host it started on.
///
/// `None` when nothing recorded its opening placement — which cannot happen for
/// a card this screen created, and is answered as "no opinion" rather than as
/// "unchanged" so a gap in the record can never read as a clean screen.
fn placed_as_opened(state: &LabState, node: NodeId) -> Option<bool> {
    let opened = state.opened_at.borrow().get(&node).cloned()?;
    let doc = state.doc.borrow();
    let slot = doc.tree(ROOT).and_then(|t| t.node(node))?;
    let host = slot
        .parent
        .and_then(|f| state.frames.borrow().get(&f).cloned());
    Some((slot.x, slot.y) == opened.at && host == opened.host)
}

/// The scopes with something to put back, in census order — the panel's
/// affordances, and the one list both the paint and the hit test read.
fn changed_scopes(state: &LabState) -> Vec<ResetScope> {
    ResetScope::ALL
        .into_iter()
        .filter(|scope| scope.gated() && scope.changed(state))
        .collect()
}

/// Author the opening links onto `doc`, and answer which one the screen opens
/// with selected.
///
/// ★ R1678 — lifted out of `opening` because a reset PUTS THESE BACK, and the
/// port-picking below is the kind of arithmetic that is quietly wrong in a
/// second copy: an accept pin is a variadic run, so which slot a link lands in
/// depends on what is already there. Two implementations would agree on the
/// opening graph (nothing is there yet) and disagree the moment a reset ran
/// over a graph somebody had edited — which is exactly when nobody is looking.
fn seed_links(
    doc: &mut Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    ids: &BTreeMap<String, NodeId>,
) -> Option<LinkId> {
    let mut selected_link = None;
    for (from, to) in spec::LINKS {
        let (Some(&a), Some(&b)) = (ids.get(*from), ids.get(*to)) else {
            continue;
        };
        // ★ R1681 — the SAME endpoint arithmetic the canvas uses, not a second
        // copy of it. Port 0 on the dial side: the taxonomy declares one.
        let Ok(endpoint) = landing_endpoint(doc, forms, a, b) else {
            continue;
        };
        let Some(port) = open_slot_in(doc, b, endpoint.as_deref()) else {
            continue;
        };
        match doc.connect(ROOT, Socket::new(a, 0), Socket::new(b, port)) {
            Ok(made) => {
                if (*from, *to) == spec::SELECTED_LINK {
                    selected_link = Some(made.link);
                }
            }
            Err(_) => {
                doc.remove_item(ROOT, b, Side::Input, port).ok();
            }
        }
    }
    // ★ R1681 — what a source REPORTED, beside what the specification drew.
    // Seeded here because it is part of the opening state a reset puts back,
    // and because an "adopt" affordance with nothing to adopt is an affordance
    // no test and no person can reach.
    for (from, to) in spec::OBSERVED {
        let (Some(&a), Some(&b)) = (ids.get(*from), ids.get(*to)) else {
            continue;
        };
        let Ok(endpoint) = landing_endpoint(doc, forms, a, b) else {
            continue;
        };
        let Some(port) = open_slot_in(doc, b, endpoint.as_deref()) else {
            continue;
        };
        if doc
            .observe(ROOT, Socket::new(a, 0), Socket::new(b, port))
            .is_err()
        {
            doc.remove_item(ROOT, b, Side::Input, port).ok();
        }
    }
    // ★★★★★ R1914 — **the slot nobody has dialled still knows what it listens
    // on**, and that is a fact about the card rather than about the wire.
    //
    // `open_slot_in` gives a slot the address the link that opened it dials, so
    // every WIRED accept pin carries one. The floor slot — the one the run's
    // `at_least(1)` keeps on a card nothing has reached yet — had none, so the
    // only pins carrying an address were exactly the pins the reference refuses
    // to split. Measured at R1914 on the running screen: splitting produced a
    // host and a service holding the taxonomy's declared defaults rather than
    // this card's own address, which is the state the reference's own split
    // avoids by parsing the parent's value on the way down.
    for (&node, form) in forms {
        let listening = form
            .field("listen.endpoints")
            .map(|f| f.value().trim().to_owned())
            .filter(|v| !v.is_empty());
        let (Some(listening), Some(signature)) = (listening, doc.signature(ROOT, node)) else {
            continue;
        };
        for index in 0..u32::try_from(signature.inputs.len()).unwrap_or(0) {
            let at = PortRef::input(index);
            if doc.port_value(ROOT, node, at).is_none() {
                let _ = doc.set_port_value(ROOT, node, at, listening.clone());
            }
        }
        // ★ And the DIAL pin's resting value is this card's own address, which
        // is what the taxonomy's `evaluate` already implies: a node hands on
        // the locator it was reached by, so a node nobody has reached yet hands
        // on its own. R1594's rule — a source node's resting constant lives on
        // its output port — read for the one source every card is.
        for index in 0..u32::try_from(signature.outputs.len()).unwrap_or(0) {
            let at = PortRef::output(index);
            if doc.port_value(ROOT, node, at).is_none() {
                let _ = doc.set_port_value(ROOT, node, at, listening.clone());
            }
        }
    }
    selected_link
}

/// The configuration form a node of that role opens with.
///
/// The five rows the reference shows on its selected node are the specification
/// for the router; every other role gets the same shape with its own opening
/// values, because a form whose rows depended on which node was clicked would
/// make "the key is the configuration path" untrue for all but one of them.
fn form_for(id: &str, role: Role) -> ConfigForm {
    let listen = match id {
        "R-01" => "tcp/0.0.0.0:7447",
        "P-01" => "tcp/0.0.0.0:7448",
        "P-02" => "tcp/0.0.0.0:7449",
        "P-03" => "tcp/0.0.0.0:7451",
        _ if role.accepts() => "",
        _ => "",
    };
    // ★★★★★ R1690 — **the shape comes from the option surface, not from
    // here.** Every row below used to name its own, and one of them was wrong
    // for the whole life of this screen: `id` is read by a parser and was
    // offered as free text, so a node called `zz!` went in without a word and
    // would not have come up. A shape written beside a row is a claim about
    // what the target accepts, made in the one place that has no way to check
    // it. `settings::shape_or_free` reads the declaration instead, and the
    // reach meter's `mistyped` column is what fails if anybody goes back.
    let shape = settings::shape_or_free;
    let mut fields = vec![
        ConfigField::new("id", "id", Applies::Restart, opening_id(id)).with_shape(shape("id")),
        ConfigField::new("listen.endpoints", "address[]", Applies::Restart, listen)
            .with_shape(shape("listen.endpoints")),
        // ★★★★★ R1716 — `connect.endpoints` is NOT here any more, and its
        // absence is the round's screen change. It used to open holding an
        // address written beside this line, and measured before the change,
        // `R-01` showed one address nothing in the graph listens on while the
        // canvas drew three links out of it — so the exported configuration
        // dialled a node it was not drawn to reach and missed one it was.
        // The row is worked out from the wires instead ([`dialled_row`]), and
        // a person who needs an address this canvas does not draw takes it over.
        // ★★★★★ R1842 — two boolean rows where one set-valued row stood. The
        // target declares the two permissions separately and holds them as an
        // object of booleans; the single row composed an array at a path that
        // is not a leaf, so what this screen exported was refused by the thing
        // it configures. A router grants both, everything else reads only —
        // which is what the one row said, now said in the target's own shape.
        ConfigField::new("admin.permissions.read", "bool", Applies::Restart, "true")
            .with_shape(shape("admin.permissions.read")),
        ConfigField::new(
            "admin.permissions.write",
            "bool",
            Applies::Restart,
            if role == Role::Router {
                "true"
            } else {
                "false"
            },
        )
        .with_shape(shape("admin.permissions.write")),
        ConfigField::new(
            "transport.link.tx.batch_size",
            "int",
            Applies::Restart,
            "65535",
        )
        .with_shape(shape("transport.link.tx.batch_size")),
    ];
    // The two peers the reference draws with a warning dot have discovery on.
    if matches!(id, "P-01" | "P-02") {
        fields.push(
            ConfigField::new(
                "discovery.multicast.enabled",
                "bool",
                Applies::Restart,
                "true",
            )
            .with_shape(shape("discovery.multicast.enabled")),
        );
    }
    let addable = spec::ADDABLE
        .iter()
        .filter(|key| !fields.iter().any(|f| f.key() == **key))
        .map(|key| offered(key))
        .collect();
    ConfigForm::new(fields, addable)
}

/// **How much of the option surface this tool's palette reaches.**
///
/// ★★★ R1690 — over EVERY role's catalogue, not the selected node's. The
/// reference does the same, and the reason is what the meter is for: "can I
/// configure the thing with this tool" is a question about the tool, and
/// answering it from whichever card happens to be selected would make the
/// number move when a person clicks about.
///
/// Nothing is cached. The figure has to fall on its own when a field is
/// dropped, and a stored one falls when somebody remembers to update it.
fn palette_reach() -> pinion_core::widgets::config_schema::Reach {
    let forms: Vec<ConfigForm> = Role::ALL
        .iter()
        .map(|role| form_for(spec::SELECTED_NODE, *role))
        .collect();
    let catalogue: Vec<(&str, &FieldType)> = forms
        .iter()
        .flat_map(|form| form.fields().iter().chain(form.addable()))
        .map(|field| (field.key(), field.shape()))
        .collect();
    settings::reach(&catalogue)
}

/// Which build each node of the opening graph is running (R1885).
///
/// ★★★★★ **The opening graph is HETEROGENEOUS and valid**, which is what makes
/// it a compatibility test graph rather than a picture of one deployment: two
/// of its peers run builds that are not the reference one, and every drawn wire
/// still negotiates. A graph where everything is the same build could never
/// answer the question the axis exists for, and a graph that opened broken
/// would be asserting a defect rather than offering a test.
///
/// Two of its cards run the independent re-implementation, whose span overlaps
/// the reference's, so no wire this graph draws is refused. That pair of facts —
/// more than one build, and every wire still negotiating — is asserted by
/// `r1885_the_opening_graph_is_heterogeneous_and_still_negotiates`, and putting
/// a card on the legacy build is the edit the assembly walk performs.
///
/// ⚠ R1885.3 — this paragraph previously said "the two non-reference **builds**"
/// and cited that test before it was written. Both were false: the spans were
/// changed mid-round when the walk found no refusal was reachable, which put
/// both non-reference cards on ONE build, and the citation named a test nobody
/// had written. The prose did not follow the fix it was describing, and nothing
/// gates a citation in Rust doc prose — so the test now exists and says so.
fn opening_implementation(id: &str) -> Implementation {
    let stack = match id {
        // An independent re-implementation that has not caught up to the newest
        // revision, and does not need to: it overlaps the reference, so every
        // wire it is on still negotiates.
        //
        // ⚠ The legacy build is deliberately NOT in the opening graph. It
        // shares no revision with the reference, so a node running it would
        // open the screen already blocked — a graph asserting a defect rather
        // than a compatibility test that passes. It is the build an edit
        // introduces, which is the act this graph exists to support.
        "P-03" | "S-01" => Stack::Independent,
        _ => Stack::Reference,
    };
    // The span comes from [`spec_revisions`] and not from here, so the opening
    // graph, the palette and an edit cannot disagree about what a build is.
    Implementation {
        stack,
        speaks: spec_revisions(stack),
    }
}

/// The identifier a node opens holding.
///
/// ★★★★★ R1690 — **three of these were values the target would refuse**, and
/// nothing could say so while the row was free text. `t1`, `t2` and `q1` were
/// written as role initials, and `t` and `q` are outside the hexadecimal
/// alphabet the identifier is read with. They sat in the opening graph of every
/// run of this screen; the launch gate is what found them, on the first drive
/// after the shape came from the option surface.
///
/// The fallback was the worse half: every card the palette added opened holding
/// the same `q1`, so the defect was not merely seeded — it was *produced*, once
/// per added node. It is derived now, in the format's own alphabet, from the
/// number the card's name already carries.
fn opening_id(id: &str) -> String {
    match id {
        "R-01" => "a1".to_owned(),
        "P-01" => "b1".to_owned(),
        "P-02" => "b2".to_owned(),
        "P-03" => "b3".to_owned(),
        "S-01" => "c1".to_owned(),
        "T-01" => "d1".to_owned(),
        "T-02" => "d2".to_owned(),
        "Q-01" => "e1".to_owned(),
        // ⚠ R1818 considered replacing this with hex of the whole name, on the
        // theory that `f{trailing number:x}` is not injective — `R-02` and
        // `S-02` would both be `f2` — and REVERTED IT, because the theory was
        // never measured and the measurement does not support it: an added
        // card is named from the document's own node counter
        // (`{badge}-{id.0:02}`), so two adds cannot share an ordinal and the
        // collision this guarded against is unreachable. The round's new
        // uniqueness check reported the same count with and without the
        // change, which is what said so.
        //
        // ⇒ left as it was. A fix for a defect nobody measured is a change
        // whose only certain effect is that the code is different.
        added => {
            let n: u32 = added
                .rsplit('-')
                .next()
                .and_then(|tail| tail.parse().ok())
                .unwrap_or(0);
            format!("f{n:x}")
        }
    }
}

/// A key the inspector offers to add, with the shape it will hold.
///
/// ★★★ R1690 — the shape is [`settings::shape_or_free`]'s, so this decides only
/// what a chip is CALLED, when it takes effect and what it opens holding. Those
/// three are properties of the palette; the shape is a property of the thing
/// being configured, and a palette that decided it could offer a key the target
/// would refuse.
fn offered(key: &str) -> ConfigField {
    let (word, applies, opening) = match key {
        "discovery.multicast.enabled"
        | "timestamping.enabled"
        | "transport.unicast.compression.enabled" => ("bool", Applies::Restart, "false"),
        "namespace" => ("path", Applies::Restart, "demo"),
        "routing.peer.mode" => ("mode", Applies::Restart, "peer_to_peer"),
        // ★★ R1716 — offered so a card the canvas draws no link out of can
        // still be told to dial something: the graph is what this tool draws,
        // not the boundary of what the configuration may reach. A card that
        // does have links holds the derived row, so the chip is not offered
        // there — `addable` takes it out for exactly the right reason.
        "connect.endpoints" => ("address[]", Applies::Hot, ""),
        _ => ("name[]", Applies::Restart, ""),
    };
    ConfigField::new(key.to_owned(), word, applies, opening)
        .with_shape(settings::shape_or_free(key))
}

// ── Canvas transform ────────────────────────────────────────────────────────

/// Canvas units to world-surface pixels, at a **stated** zoom.
///
/// ★ R1688 — the zoom is a parameter and not read off the screen, and the
/// convenience wrapper that read it is gone rather than kept unused: every
/// caller is now inside a derivation that has already had to say which scale it
/// means (see [`scaled_by`]), so a version that answered "whatever the screen is
/// at" would be the ambiguity this parameter exists to remove, sitting one call
/// away.
fn to_content_at(cx: i32, cy: i32, at: u32) -> (u32, u32) {
    let zoom = f64::from(at) / 100.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a canvas point times a zoom is a pixel inside the world surface"
    )]
    let scale = |v: i32| (f64::from(v) * zoom) as i32;
    let origin = WORLD_ORIGIN;
    // The margin is in WORLD units and scales with the surface, so the range a
    // position may take does not shrink when the zoom grows. `clamp_to_world`
    // is what keeps this conversion total; the saturation below is the
    // belt-and-braces half of the same statement.
    (
        u32::try_from(scale(origin + cx)).unwrap_or(0),
        u32::try_from(scale(origin + cy)).unwrap_or(0),
    )
}

/// The range a node's world position may take.
///
/// ★ A bound stated where positions are SET, rather than a saturation where
/// they are painted. The world surface is finite, so some answer has to be
/// given for a position outside it, and a silent clamp at paint time is the bad
/// one: the node keeps a coordinate nothing can draw and the card appears in
/// the corner with no explanation. Clamping the drag says the same thing where
/// the user can see it — the node stops at the edge of the world.
const fn clamp_to_world(v: i32) -> i32 {
    if v < -WORLD_ORIGIN {
        -WORLD_ORIGIN
    } else if v > WORLD - WORLD_ORIGIN {
        WORLD - WORLD_ORIGIN
    } else {
        v
    }
}

/// Where the world surface is held against the viewport, which is what the pan
/// gesture moves.
///
/// ★ R1653 — the pan used to be added to every world rectangle and the result
/// converted back to pane-local by subtracting the pane's origin in `u32`. A
/// pan to the LEFT makes that subtraction underflow: the debug build panics and
/// the release build wraps to a coordinate near four billion, so a screen whose
/// hint strip advertises "drag empty space = pan" crashed on half of the
/// gesture it advertised. Nothing saw it because every gate drove the screen
/// from its opening state, where the pan is zero.
///
/// The fix is not a clamp. A pan is a *viewport* moving over a surface, the
/// framework has that primitive, and using it also gives the pane the clipping
/// it never had — panned content used to be painted over the palette and the
/// inspector rather than cut off at the canvas edge.
fn world_offset(state: &LabState, pan: (i32, i32)) -> (i32, i32) {
    let origin = i32::try_from(scaled(state, WORLD_ORIGIN.unsigned_abs())).unwrap_or(i32::MAX);
    (origin - pan.0, origin - pan.1)
}

/// A window point in the coordinates the world surface is painted in.
fn window_to_content(state: &LabState, px: u32, py: u32) -> (i64, i64) {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    (
        i64::from(px) - i64::from(canvas.x) + i64::from(ox),
        i64::from(py) - i64::from(canvas.y) + i64::from(oy),
    )
}

/// The inverse of [`window_to_content`]: where a point on the world surface
/// lands in the window, or `None` when the viewport is not over it.
///
/// `None` is the honest answer rather than a clamped coordinate: the canvas
/// clips, so a point the viewport has scrolled past is not on screen at all,
/// and handing back the nearest visible pixel would let a caller press
/// something the user cannot see.
///
/// Only the tests need this direction — the screen paints in surface
/// coordinates and resolves presses into them, so nothing in the running app
/// ever converts back. It is `cfg(test)` rather than dead production code, and
/// `r1653_the_two_canvas_conversions_invert_each_other` is what keeps it from
/// drifting away from the forward one it is supposed to invert.
#[cfg(test)]
fn content_to_window(state: &LabState, cx: i64, cy: i64) -> Option<(u32, u32)> {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    let x = cx - i64::from(ox) + i64::from(canvas.x);
    let y = cy - i64::from(oy) + i64::from(canvas.y);
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    contains(canvas, x, y).then_some((x, y))
}

/// Does this world-surface rectangle hold that content-space point?
const fn holds(rect: Rect, cx: i64, cy: i64) -> bool {
    cx >= rect.x as i64
        && cx < (rect.x + rect.w) as i64
        && cy >= rect.y as i64
        && cy < (rect.y + rect.h) as i64
}

/// Window pixels back to canvas units — the exact inverse of [`to_content_at`]
/// composed with [`world_offset`].
///
/// Lifted at its third copy: the node drag, the press that starts one and the
/// palette's "put it in the middle" each need it, and three sites free to
/// disagree about a transform's inverse is how a drag ends up half a pan out.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a window pixel divided by a zoom is a canvas unit; both fit an i32"
)]
fn to_canvas(state: &LabState, px: u32, py: u32) -> (i32, i32) {
    let canvas = canvas_rect();
    let (pan_x, pan_y) = state.pan.get();
    let zoom = f64::from(state.zoom.get()) / 100.0;
    let unscale = |v: i64| (v as f64 / zoom) as i32;
    (
        unscale(i64::from(px) - i64::from(canvas.x) - i64::from(pan_x)),
        unscale(i64::from(py) - i64::from(canvas.y) - i64::from(pan_y)),
    )
}

fn scaled(state: &LabState, v: u32) -> u32 {
    scaled_by(v, state.zoom.get())
}

/// The same scaling at a zoom that is **stated** rather than read off the
/// screen.
///
/// ★★★ R1688 — the fit needs the cards' sizes in canvas units, and a card's box
/// is derived from the zoom it is drawn at (the faces have a legibility floor
/// and the rows collapse below it, both deliberately). Measuring what is on
/// screen and dividing the zoom back out would make "frame the graph" a function
/// of where you were already looking — press it from two different zooms and get
/// two different answers, which is the drift the reference is documented to
/// have. So the extent is asked at [`UNZOOMED`], once, and the fit is a function
/// of the graph.
const fn scaled_by(v: u32, zoom: u32) -> u32 {
    v * zoom / 100
}

/// The scale the diagram's own units are stated at.
const UNZOOMED: u32 = 100;

/// The specification row this card came into being as, or `None` for one the
/// palette added.
///
/// ★★ R1682 — by the name the card OPENED with, never by what it is called
/// now. Two derivations keyed off the current name — the card's digest rows and
/// its width — and a rename silently changed both: a renamed card stopped
/// matching any specification row, so it fell through to the palette-added
/// path, redrew its digest from the first three form fields and snapped to the
/// default width. Nothing was broken enough to fail, which is how it would have
/// stayed.
fn declared_card(state: &LabState, node: NodeId) -> Option<&'static spec::NodeSpec> {
    let opened_as = state.opened_at.borrow().get(&node)?.opened_as.clone()?;
    spec::NODES.iter().find(|n| n.id == opened_as)
}

/// Whether this card is drawn small.
fn card_collapsed(state: &LabState, node: NodeId) -> bool {
    state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .is_some_and(|slot| slot.appearance.collapsed)
}

/// The digest lines a node's card shows.
///
/// The declared ones for a node the specification opens with, and otherwise the
/// first three rows of the node's own form — so a node added from the palette
/// is a card like any other rather than an empty box.
fn card_rows(state: &LabState, node: NodeId) -> Vec<(String, String)> {
    if let Some(declared) = declared_card(state, node) {
        // ★ R1651.1 — the KEYS are the specification's (they are a per-role
        // digest, and which fields are worth showing is a design decision), but
        // the VALUES are re-read from the form whenever the form has that path.
        // They were the table's until an audit edited an endpoint and watched
        // the card keep the old one: a card showing a frozen copy of the
        // configuration is a second source, and the whole round argues against
        // exactly that.
        let forms = state.forms.borrow();
        let form = forms.get(&node);
        return declared
            .rows
            .iter()
            .map(|(k, v)| {
                let live = form.and_then(|f| digest_value(f, digest_paths(k)));
                ((*k).to_owned(), live.unwrap_or_else(|| (*v).to_owned()))
            })
            .collect();
    }
    // ★ R1717 — the SHOWN form, so a card the specification does not declare a
    // digest for reads the same rows its inspector does. Reading the store here
    // would show a card whose connect row lost the wires the canvas draws out
    // of it, which is the second-source failure the branch above records.
    shown_form(state, node)
        .as_ref()
        .map_or_else(Vec::new, |form| {
            form.fields()
                .iter()
                .take(3)
                .map(|f| (f.key().to_owned(), f.value().into_owned()))
                .collect()
        })
}

/// The configuration paths a card's digest line is about, when it is about any.
///
/// A card row is a *label* a person reads at a glance (`listen`), and the form
/// is keyed by the configuration path (`listen.endpoints`); this is the one
/// mapping between them, so a digest line that names a path tracks it.
///
/// ★★ R1842 — a *slice*, because one line can be about more than one path. The
/// permissions line is the case that forced it: the target declares the two
/// permissions as separate boolean leaves, so a digest line reading only one of
/// them would report half a fact and look like the whole one.
const fn digest_paths(key: &str) -> &'static [&'static str] {
    match key.as_bytes() {
        b"listen" => &["listen.endpoints"],
        b"id" => &["id"],
        b"control" => &["admin.permissions.read", "admin.permissions.write"],
        b"discovery" => &["discovery.multicast.enabled"],
        _ => &[],
    }
}

/// What a digest line reads off the form, when the form has what it names.
///
/// One path is its value. Several are read as a **set of named booleans** and
/// rendered as the last segment of each one that is on — which is the spelling
/// the specification's own digest uses (`read, write`) and the spelling the
/// reference paints. `None` when the form carries none of them, so the caller
/// falls back to the declared value rather than showing an empty line.
fn digest_value(form: &ConfigForm, paths: &[&str]) -> Option<String> {
    match paths {
        [] => None,
        [only] => form.field(only).map(|f| f.value().into_owned()),
        many => {
            let on: Vec<&str> = many
                .iter()
                .filter(|path| form.field(path).is_some_and(|f| f.value().trim() == "true"))
                .filter_map(|path| path.rsplit('.').next())
                .collect();
            many.iter()
                .any(|path| form.field(path).is_some())
                .then(|| on.join(", "))
        }
    }
}

/// How wide a collapsed card is drawn, in canvas units.
///
/// Narrower than any card the specification declares, so collapsing is visible
/// at a glance rather than only by counting rows. The reference collapses to a
/// fixed width for the same reason, and everything that follows the card — its
/// pins, the wires into them, the frame that bounds it — is derived from this
/// width and follows without being told.
const CARD_COLLAPSED_W: u32 = 92;

/// The width a node's card is drawn at, in canvas units.
fn card_width(state: &LabState, node: NodeId) -> u32 {
    if card_collapsed(state, node) {
        return CARD_COLLAPSED_W;
    }
    declared_card(state, node).map_or(146, |declared| declared.rect.2)
}

/// Every rectangle a node's card is made of, derived once.
///
/// ★ R1656 — the card's own box is computed FROM the rows it paints, and the
/// rows are placed in the same pass. Before this they were two derivations of
/// one fact: the height was `scaled(HDR + rows * ROW_H + 6)` while the rows
/// were placed at `y + HDR + n * ROW_H` **unscaled**, so at any zoom below 100%
/// the last row was painted below the border — measured at the size the screen
/// opens in, seven of eight cards spilled by three to five pixels, and a person
/// reported it before any check here did.
///
/// Written as a shape rather than as a rule ("remember to scale both") for the
/// reason [`pinion_core::widgets::config_form`]'s row parts are: a rule can be
/// half-applied and a derivation cannot. `rect.h` is the union of the parts,
/// so a row that does not fit is not expressible.
struct CardShape {
    /// The card's box, in window pixels.
    rect: Rect,
    /// The identity label, relative to `rect`.
    id: Rect,
    /// The role badge and the text inside it, relative to `rect`.
    badge: Rect,
    /// The badge's label, relative to `rect`.
    badge_text: Rect,
    /// ★★★★★ R1927 — the ISSUE DOT's seat, relative to `rect`.
    ///
    /// Always reserved, drawn only when this card has a problem. Reserved
    /// rather than inserted, because a header whose identity label changed
    /// width the moment a warning appeared would move text under a reader's
    /// eye for a reason that has nothing to do with the text — and this
    /// screen's paint sweep asserts that nothing a card draws leaves the card,
    /// which conditional geometry is the usual way to break.
    issue: Rect,
    /// The face the identity label is drawn at — scaled, so it shrinks with
    /// the diagram it belongs to.
    id_font: u32,
    /// The face a digest row is drawn at.
    row_font: u32,
    /// The face the role badge is drawn at.
    badge_font: u32,
    /// One (key, value) pair of rectangles per digest row, relative to `rect`.
    rows: Vec<(Rect, Rect)>,
}

/// The size a face on the canvas is drawn at: it scales with the zoom, because
/// a node card is part of the diagram and not chrome over it.
///
/// ★ R1656 — this scaling did not exist. The card's BOX was scaled while its
/// font and its row pitch were not, so at any zoom below 100% the two disagreed
/// and the disagreement was painted: rows placed 15px apart inside a box sized
/// for `zoom * 15`. Floored rather than allowed to reach zero, because a face
/// of 0px is not a smaller label, it is an invisible one — the same reason
/// R1653 scaled the pins.
///
/// ★★★★★ R1834 — **the floor was 6 and that is what broke proportionality.**
///
/// A floor at 6 does not merely clip small text: it makes the face STOP
/// shrinking while the diagram keeps shrinking, so a card grows relative to its
/// neighbours all the way down. That is what produced the overlap R1656
/// measured, and the level-of-detail collapse this function's caller used to
/// carry was a repair aimed at the symptom.
///
/// Measured against the behaviour reference: it applies ONE transform to the
/// whole graph, so everything scales together by construction, and its 195 KB
/// of application logic contains **zero** conditionals on zoom — no
/// level-of-detail anywhere, at any zoom in its 0.25–2.5 range. Our own zoom
/// floor is the same 25%, so between 25% and 66% this screen was hiding rows
/// the reference draws.
///
/// So the floor is **1**, not 6: zero is still refused for the reason above,
/// and legibility at 25% is not a property the reference has either. What is
/// restored is that a face and the diagram it sits in shrink together, which is
/// the relationship the overlap was a symptom of losing.
fn canvas_font(state: &LabState, px: u32) -> u32 {
    canvas_font_by(px, state.zoom.get())
}

/// The same, at a stated zoom — see [`scaled_by`].
fn canvas_font_by(px: u32, zoom: u32) -> u32 {
    scaled_by(px, zoom).max(1)
}

/// Derive a node's card: where every part goes, and therefore how big it is.
fn card_shape(state: &LabState, node: NodeId) -> Option<CardShape> {
    card_shape_at(state, node, state.zoom.get())
}

/// The same derivation at a **stated** zoom, which is what makes a card's size
/// in canvas units askable (see [`scaled_by`]).
#[allow(
    clippy::too_many_lines,
    reason = "one card is one derivation; splitting it would be the two-answers \
              defect R1656 wrote this function to end"
)]
fn card_shape_at(state: &LabState, node: NodeId, zoom: u32) -> Option<CardShape> {
    let (nx, ny) = {
        let doc = state.doc.borrow();
        let held = doc.tree(ROOT)?.node(node)?;
        (held.x, held.y)
    };
    let scaled = |v: u32| scaled_by(v, zoom);
    let (x, y) = to_content_at(nx, ny, zoom);
    let w = scaled(card_width(state, node));
    let pad = scaled(10).max(3);
    let id_font = canvas_font_by(FONT_SMALL, zoom);
    let row_font = canvas_font_by(FONT_TINY, zoom);
    let id_line = line_box(id_font);
    let row_line = line_box(row_font);
    // The header band is as tall as the identity line it holds, plus the gap
    // above and below it — not a constant that happens to fit at one zoom.
    // Tight on purpose: `line_box` already over-reserves (it is a
    // font-independent floor, not a measurement), so padding it generously
    // again compounds — measured, five lines of that made a card 9px taller
    // than the spacing the reference lays its graph out on, and the cards
    // started covering each other.
    let id_top = 2;
    let hdr = id_top + id_line + 2;
    let row_pitch = row_line + 1;
    let key_w = scaled(40).max(8);
    let gap = scaled(2).max(1);
    // ★ R1656 — LEVEL OF DETAIL: below the zoom at which a row's face would be
    // drawn at the legibility floor, the card shows its identity band alone.
    //
    // Derived from a real tension rather than chosen for looks. `canvas_font`
    // floors the face at 6px, because a 0px label is not a smaller label but an
    // invisible one — so below that zoom the TEXT stops shrinking while the
    // graph's spacing keeps shrinking, and a card with rows grows relative to
    // the diagram until it covers its neighbour. Measured: the press sweep found
    // 34 of 2,556 points on a card reaching a different one at the minimum zoom.
    // Every node editor this is judged against collapses a node's contents on
    // the way out for the same reason.
    //
    // ★★ R1682 — a COLLAPSE is the same request, made deliberately instead of
    // derived from the zoom, so it lands in the same place. A second path that
    // hid rows its own way would be a second answer to "what is on this card",
    // and the height — which IS the content — would be free to disagree with
    // it.
    // ★★★★★ R1834 — the zoom half of this is GONE, and it is a divergence being
    // repaid rather than a feature being dropped.
    //
    // It read `&& scaled(FONT_TINY) >= 6`, which collapsed every row below 67%
    // zoom. Measured against the behaviour reference: it has NO level of detail
    // — zero conditionals on zoom in its whole application logic — and its zoom
    // range bottoms at 25%, which is this screen's floor too. So this line hid
    // rows the reference draws, across 25%..=66%.
    //
    // What remains is the reader's own collapse, which the reference does have.
    // The overlap that motivated the zoom half is repaired at its cause instead
    // — see `canvas_font_by`, where a face floor of 6 was what stopped a card
    // shrinking with the diagram around it.
    //
    // ★ The specification is READ rather than restated: if it is ever measured
    // that the reference DOES collapse, this is the branch that turns back on,
    // and the gate reads the same constant from the other side.
    let reader_collapsed = card_collapsed(state, node);
    let detailed = if spec::REFERENCE_COLLAPSES_CARD_DETAIL_AT_LOW_ZOOM {
        !reader_collapsed && scaled(FONT_TINY) >= 6
    } else {
        !reader_collapsed
    };
    let rows: Vec<(Rect, Rect)> = card_rows(state, node)
        .iter()
        .take(if detailed { usize::MAX } else { 0 })
        .enumerate()
        .map(|(n, _)| {
            let top = hdr + u32::try_from(n).unwrap_or(0) * row_pitch;
            (
                Rect::new(pad, top, key_w, row_line),
                Rect::new(
                    pad + key_w + gap,
                    top,
                    w.saturating_sub(pad * 2 + key_w + gap).max(8),
                    row_line,
                ),
            )
        })
        .collect();
    // The height IS the content: the lowest edge any part reaches, plus the
    // bottom padding. Nothing here can disagree with what the painter draws,
    // because the painter draws exactly these rectangles.
    let content_bottom = rows
        .iter()
        .map(|(_, value)| value.y + value.h)
        .max()
        .unwrap_or(hdr);
    let badge_w = scaled(38).max(10);
    let badge_font = canvas_font_by(8, zoom);
    let badge_line = line_box(badge_font);
    // ★ R1927 — the issue dot's size, scaled like everything else on a card:
    // it is part of the diagram, not chrome over it.
    let dot = scaled(7).max(3);
    Some(CardShape {
        rect: Rect::new(x, y, w, content_bottom + 3),
        id: Rect::new(
            pad,
            id_top,
            w.saturating_sub(pad * 2 + badge_w + dot + gap).max(8),
            id_line,
        ),
        issue: Rect::new(
            w.saturating_sub(badge_w + pad / 2 + dot + gap),
            id_top + id_line.saturating_sub(dot) / 2,
            dot,
            dot,
        ),
        badge: Rect::new(
            w.saturating_sub(badge_w + pad / 2),
            id_top + id_line.saturating_sub(badge_line) / 2,
            badge_w,
            badge_line,
        ),
        badge_text: Rect::new(
            w.saturating_sub(badge_w + pad / 2) + gap,
            id_top + id_line.saturating_sub(badge_line) / 2,
            badge_w.saturating_sub(gap * 2).max(4),
            badge_line,
        ),
        id_font,
        row_font,
        badge_font,
        rows,
    })
}

/// The rectangle a node's card occupies, in window pixels.
fn card_rect(state: &LabState, node: NodeId) -> Option<Rect> {
    card_shape(state, node).map(|shape| shape.rect)
}

/// ★★★ R1688 — a card's size in **canvas units**: how much of the diagram it
/// takes up, with no zoom in it.
///
/// What a fit has to know, and what [`card_rect`] cannot answer: that one is in
/// window pixels at whatever scale the screen happens to be at. See
/// [`scaled_by`] for why dividing the zoom back out of it would have been the
/// wrong answer rather than a rounding one.
fn card_extent(state: &LabState, node: NodeId) -> Option<Extent> {
    let shape = card_shape_at(state, node, UNZOOMED)?;
    Some(Extent::new(
        i32::try_from(shape.rect.w).unwrap_or(i32::MAX),
        i32::try_from(shape.rect.h).unwrap_or(i32::MAX),
    ))
}

/// ★★ R1688 — **everything the canvas draws**, positioned and sized in canvas
/// units: the cards, and the host frames around them.
///
/// The frames are in it because they are drawn and a fit that left them out
/// would cut their borders — the reference frames its node boxes alone and gets
/// away with it because its frames are derived from their members and its
/// padding is wider than the frame's own. Ours are derived the same way, so
/// including them can only ever grow the box, never move it: it is the strictly
/// safer half of a choice, and it means "fit" means what it says.
fn drawn_boxes(state: &LabState) -> Vec<((i32, i32), Extent)> {
    // ★ Everything at [`UNZOOMED`], where a world pixel IS a canvas unit — so
    // the fit is a function of the graph and there is no division to lose a
    // pixel in. The world origin is the only thing to take back off.
    let whole = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    let unit = |rect: Rect| {
        (
            (whole(rect.x) - WORLD_ORIGIN, whole(rect.y) - WORLD_ORIGIN),
            Extent::new(whole(rect.w), whole(rect.h)),
        )
    };
    let mut boxes: Vec<((i32, i32), Extent)> = Vec::new();
    for node in state.cards() {
        // ★ R1774 — through `drawn_box_of`, so the placement search and this fit
        // read a card's drawn position from ONE derivation. They did not, and
        // the placement search was in the wrong frame for 86 rounds.
        if let Some(held) = drawn_box_of(state, node) {
            boxes.push(held);
        }
    }
    for (frame, _) in frames_of(state) {
        boxes.push(unit(frame_rect_at(state, frame, UNZOOMED)));
    }
    boxes
}

/// A pin's rectangle. `dial` is the outgoing pin on the right edge.
///
/// ★ R1653 — the pin SCALES with the canvas, because it is part of the diagram
/// rather than chrome over it. Held at a fixed size it kept its pixels while
/// the cards shrank, so at the minimum zoom a dial pin and its neighbour's
/// accept pin covered the same pixels and the one drawn second could not be
/// pressed at all — the painted screen offered a control the pointer could
/// never reach, which is the class this round exists to make visible.
fn pin_rect(state: &LabState, card: Rect, dial: bool) -> Rect {
    let pin = scaled(state, PIN).max(3);
    // ★ R1656 — the header's HALF, read from the same derivation the card is
    // built from. It was `scaled(CARD_HDR)/2` against a constant the card no
    // longer uses, which is one fact in two places by construction.
    let y = card.y + (line_box(canvas_font(state, FONT_SMALL)) / 2).max(2);
    if dial {
        Rect::new(card.x + card.w.saturating_sub(pin / 2), y, pin, pin)
    } else {
        Rect::new(card.x.saturating_sub(pin / 2), y, pin, pin)
    }
}

/// The frames the document holds, in declaration order.
fn frames_of(state: &LabState) -> Vec<(NodeId, String)> {
    let frames = state.frames.borrow();
    let doc = state.doc.borrow();
    let Some(tree) = doc.tree(ROOT) else {
        return Vec::new();
    };
    let mut out: Vec<(NodeId, String)> = tree
        .nodes()
        .filter(|n| matches!(n.body, NodeBody::Frame))
        .filter_map(|n| frames.get(&n.id).map(|name| (n.id, name.clone())))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The nodes inside `frame`.
fn members_of(state: &LabState, frame: NodeId) -> Vec<NodeId> {
    let doc = state.doc.borrow();
    doc.tree(ROOT).map_or_else(Vec::new, |tree| {
        tree.nodes()
            .filter(|n| n.parent == Some(frame) && !matches!(n.body, NodeBody::Frame))
            .map(|n| n.id)
            .collect()
    })
}

/// The tab strip at the top of a frame — its name, and its drag handle.
const FRAME_TAB: u32 = 18;
/// How much room a frame leaves around the cards it holds.
const FRAME_PAD: u32 = 14;

/// A host frame's rectangle, **derived from the cards it holds**.
///
/// ★ R1654 — the reference derives this (`the frame rect`, `apply frame`, `drag
/// the frame` are three of its nine frame verbs) and this screen had it as a
/// constant out of the specification table, so a frame did not grow when a card
/// was dragged into it, did not shrink when one left, and could not be moved at
/// all. Reported as "the group behaviour does not match".
///
/// A frame with no members keeps its own stored position and paints an empty
/// box, because a group you cannot see is a group you cannot drop anything into.
fn frame_rect_of(state: &LabState, frame: NodeId) -> Rect {
    frame_rect_at(state, frame, state.zoom.get())
}

/// The same derivation at a **stated** zoom.
///
/// ★★★ R1688 — needed for the same reason [`card_shape_at`] is, and the fit's
/// own test is what said so: the first draft asked for the frames at whatever
/// zoom the screen was at and divided it back out, so framing the graph from two
/// different views answered two cameras a pixel apart. A frame's box is derived
/// from its members' *painted* rectangles and its padding is a scaled quantity,
/// so it has no size of its own to ask for — the scale has to be stated.
fn frame_rect_at(state: &LabState, frame: NodeId, zoom: u32) -> Rect {
    let members = members_of(state, frame);
    let boxes: Vec<Rect> = members
        .iter()
        .filter_map(|n| card_shape_at(state, *n, zoom).map(|shape| shape.rect))
        .collect();
    if boxes.is_empty() {
        let (x, y) = state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.node(frame).map(|n| (n.x, n.y)))
            .unwrap_or((0, 0));
        let (cx, cy) = to_content_at(x, y, zoom);
        return Rect::new(
            cx,
            cy,
            scaled_by(150, zoom),
            scaled_by(90, zoom).max(FRAME_TAB),
        );
    }
    let pad = scaled_by(FRAME_PAD, zoom).max(4);
    let tab = scaled_by(FRAME_TAB, zoom).max(10);
    let left = boxes.iter().map(|r| r.x).min().unwrap_or(0);
    let top = boxes.iter().map(|r| r.y).min().unwrap_or(0);
    let right = boxes.iter().map(|r| r.x + r.w).max().unwrap_or(0);
    let bottom = boxes.iter().map(|r| r.y + r.h).max().unwrap_or(0);
    Rect::new(
        left.saturating_sub(pad),
        top.saturating_sub(pad + tab),
        right - left + pad * 2,
        bottom - top + pad * 2 + tab,
    )
}

/// The frame whose box holds this content-space point, innermost first.
fn frame_at(state: &LabState, cx: i64, cy: i64) -> Option<NodeId> {
    frames_of(state)
        .into_iter()
        .filter(|(id, _)| holds(frame_rect_of(state, *id), cx, cy))
        .min_by_key(|(id, _)| {
            let r = frame_rect_of(state, *id);
            u64::from(r.w) * u64::from(r.h)
        })
        .map(|(id, _)| id)
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

// ── Hit testing ─────────────────────────────────────────────────────────────

/// What is under the cursor.
///
/// One enumeration read by both the press handler and the demo's sweep, from
/// the same rectangles the painter draws — the property R1648 lost by keeping a
/// second copy of the layout and R1649's two-direction sweep exists to hold.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Rail(&'static str),
    Role(Role),
    /// ★★★★★ R1885 — set which build the SELECTED node runs.
    ///
    /// Its own hit rather than a row of the configuration form, because the
    /// form's rows are paths of the thing being configured and this is a fact
    /// about which program is running at all. It is the act a compatibility
    /// test graph is built to perform: change one peer's build and read what
    /// the launch gate then says about the wires it is on.
    Build(Stack),
    DiscoveryToggle,
    Zoom(bool),
    /// ★★ R1688 — point the canvas at the whole graph.
    Fit,
    /// ★★ R1688 — the launch chip, which is the way to the first thing wrong.
    ///
    /// The verdict and the way to what caused it are one control on the
    /// reference too, and that is the design rather than a saving: the chip is
    /// the only thing on screen that says a graph will not start, so it is where
    /// a person looks and therefore where they press.
    Problem,
    /// R1678 — an affordance that puts one scope back to what it opened as.
    Reset(ResetScope),
    /// ★★ R1687 — the seat that takes the whole graph's configuration off the
    /// screen. It sat here answering the SELECTED card's key count, which is a
    /// different question from a different scope and belonged to nothing.
    Config,
    /// The seat beside it that renders the same plan as a script.
    Script,
    /// ★★ R1689 — the file pill's three, which the reference groups together.
    ///
    /// Three arms and not one with a scope argument — unlike [`Self::Reset`],
    /// and the difference is real: the five reset scopes are one act over a
    /// closed set of subjects, while writing a file, reading one and throwing
    /// the whole thing away are three different acts that happen to be adjacent.
    SaveGraph,
    /// Read the saved graph back — or a graph handed over on the wire.
    OpenGraph,
    /// Discard everything, including what is on disk.
    ClearGraph,
    /// ★★★★★ R1791 — the control that holds the groups a narrow toolbar moved.
    ///
    /// A `Hit` of its own rather than a flag on the moved seats, because it is
    /// a different act: pressing `save` saves, and pressing this asks *where
    /// did save go*. The floor conflates them — its hidden action still
    /// reports itself visible, so the two questions have one answer there.
    More,
    Run,
    Node(NodeId),
    /// ★★★★★ R1915 — a pin, **by the address that names it**.
    ///
    /// It carried a `bool` until this round, which said *dial or accept* and
    /// had structurally nowhere to put WHICH MEMBER of a split pin was pressed.
    /// R1914 put member pins on the frame and announced them; they could be
    /// seen and not touched, which is R1890's class arriving on the hit axis —
    /// the surface was there, the address was not.
    ///
    /// A root path is the pin itself, so every gesture that used to read
    /// `dial: true` now reads `side == Side::Output`, and the ones that used to
    /// ignore members now cannot.
    Pin {
        node: NodeId,
        side: Side,
        at: PortPath,
    },
    Link(LinkId),
    /// ★ R1681 — a link a source reported, which is not in the graph and so
    /// cannot be named by a [`LinkId`].
    Observed(Socket, Socket),
    /// The picked link's one act: delete it, or — when it is a reported one —
    /// take it into the drawing.
    LinkAct,
    /// One endpoint seat of the picked link's target.
    Endpoint(usize),
    /// A host frame's tab strip — its handle.
    Frame(NodeId),
    /// ★★ R1682 — one of the selected card's own three acts.
    NodeAct(NodeAct),
    /// ★★ R1683 — the seat that opens the one text field on the card's name.
    Rename,
    /// The seat that opens it on a configuration path instead.
    AddKey,
    Field(String),
    AddField(String),
    /// ★★ R1686 — the seat that takes a row out of the form.
    ///
    /// Its own arm rather than a [`Self::Part`], because the painter publishes
    /// it in its own field for the same reason: `parts` means *inside the
    /// control*, and this seat is cut out of the header.
    RemoveField(String),
    /// ★★ R1716 — the same seat on a row nobody wrote: it takes the value
    /// **over**, so the row becomes theirs holding what it was worked out to be.
    ///
    /// Its own arm because it is its own act. The seat's rectangle is shared
    /// and its meaning is not, and a press that resolved to `remove` on a row
    /// the form refuses to remove would report an act that never happened.
    AuthorField(String),
    /// ★★★ R1717 — the same seat on a row with **two** contributors: it gives
    /// the written half back, and the row stays because the derivation is still
    /// true.
    ///
    /// Its own arm for the reason the one above is: the rectangle is shared and
    /// the act is not. A press that answered `remove` here would name an act
    /// that did not happen — the row is still on the screen — and a reader
    /// comparing what they pressed with what they see would conclude the tool
    /// ignored them. The *request* it sends is the same one, because the form
    /// decides what taking a half out means from the row's own provenance and a
    /// second rule here would be a second answer.
    DisownField(String),
    /// An affordance inside a control: an option, a stepper, a checkbox, a list
    /// row. `part` is the painter's own tag suffix, so this arm covers every
    /// shape and a seventh needs no new arm here.
    Part {
        key: String,
        part: String,
    },
    /// ★★★★★ R1887 — a movable panel's own chrome: the two controls in its
    /// header, and the strip a folded one leaves.
    Panel(SidePanel, PanelAct),
    /// ★ R1889 — a press on the band that resizes a panel.
    ///
    /// Its own arm rather than a third [`PanelAct`], because the two are
    /// different KINDS of act: flip and fold are decided the moment the button
    /// goes down, and this one only starts something whose value arrives with
    /// the pointer. Folding it in would give `PanelAct::ask` an arm that has no
    /// ask.
    PanelGrip(SidePanel),
    Canvas,
}

/// What pressing a panel's own chrome asks of its placement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PanelAct {
    /// Move it to the next edge its policy admits.
    Flip,
    /// Fold it to its strip.
    Fold,
    /// Bring a folded one back.
    Unfold,
}

impl PanelAct {
    /// The ask this act makes of [`place_panel`], given where the panel is.
    ///
    /// ★ Not a second copy of the policy: this turns a press into a QUESTION
    /// and `place_panel` answers it. `Flip` has no ask when the panel sits on
    /// an edge its own declaration no longer admits — which is a state a
    /// specification edit can produce — and saying so here is better than
    /// inventing an edge.
    fn ask(self, state: &LabState, which: SidePanel) -> Option<PlaceAsk> {
        match self {
            Self::Flip => which.next_edge(state).map(PlaceAsk::Edge),
            Self::Fold => Some(PlaceAsk::Fold(true)),
            Self::Unfold => Some(PlaceAsk::Fold(false)),
        }
    }
}

/// Where a press lands on a movable panel's own chrome, if it does.
///
/// Asked BEFORE the panel's body, and before the scroll offset is folded into
/// the query: a header does not scroll with what it heads, so a press on it
/// must not be asked in the body's frame.
fn panel_chrome_hit(
    state: &LabState,
    which: SidePanel,
    rect: Rect,
    px: u32,
    py: u32,
) -> Option<Hit> {
    if which.at(state).folded {
        // The strip is one affordance and covers the whole panel, so anything
        // inside it brings the panel back.
        return Some(Hit::Panel(which, PanelAct::Unfold));
    }
    let (lx, ly) = (px.saturating_sub(rect.x), py.saturating_sub(rect.y));
    if contains(side_panel_control(rect, 0), lx, ly) {
        return Some(Hit::Panel(which, PanelAct::Flip));
    }
    if contains(side_panel_control(rect, 1), lx, ly) {
        return Some(Hit::Panel(which, PanelAct::Fold));
    }
    // ★★★★★ R1889 — the grip, asked AFTER the two header controls and before
    // the body. After, because the grip runs the panel's full height and would
    // otherwise swallow the right-hand end of the header on a left-hand panel;
    // before the body, for the reason the header is — the band is chrome and
    // does not scroll with what it sits beside.
    if side_panel_has_grip(state, which) && contains(side_panel_grip(rect, which.at(state)), lx, ly)
    {
        return Some(Hit::PanelGrip(which));
    }
    None
}

impl Hit {
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per addressable region of the screen, in painted order — \
                  splitting it would put the order in two places, and the order is \
                  what makes the front-to-back resolution right"
    )]
    fn at(state: &LabState, px: u32, py: u32) -> Self {
        // The inspector, front to back: its own geometry is the form painter's.
        if contains(inspector_rect(), px, py) {
            // ★★★★★ R1887 — the panel's own chrome first, in the panel's frame
            // and before the body's scroll offset is applied.
            if let Some(hit) =
                panel_chrome_hit(state, SidePanel::Inspector, inspector_rect(), px, py)
            {
                return hit;
            }
            // ★ R1682 — the node's-life seats first: they sit above the form in
            // the same scrolling body, and the form's own rows begin below
            // them, so the two cannot overlap — but asking in painted order is
            // what keeps that true if either moves.
            if state.active_card().is_some() {
                for act in NodeAct::ALL {
                    if contains(node_act_seat(state, act), px, py) {
                        return Self::NodeAct(act);
                    }
                }
                // ★ R1683 — the rename seat, and the box beside it.
                //
                // ★★★ **R1684 corrects what R1683 recorded here.** That round
                // wrote that a press inside the open box "reaches the field's
                // own router, which is what puts the caret where the pointer
                // is", because the field is a real external and an external
                // owns its rectangle. Measured this round: it does not. Every
                // press on this screen is routed to the ONE root external that
                // does the screen's own hit test (R1655), and the field's
                // external is a focus owner and a keystroke sink — it never
                // sees a pointer. The behaviour R1683 described was right only
                // by accident: the box arm below is guarded on the field being
                // SHUT, so while it is open a press there falls through to
                // nothing, which looks the same as standing aside and is not.
                //
                // What actually puts the caret under the pointer is
                // `NodeLabView::position_caret_for_point`, which R1684 had to
                // write because nothing was doing it.
                let (box_rect, apply, key) = rename_row();
                if contains(in_body(state, apply), px, py) {
                    return Self::Rename;
                }
                // The shut box is the field's own seat: it looks like somewhere
                // to type, so pressing it opens the thing it looks like. While
                // the field is open the press belongs to the field.
                if state.editing.get().is_none() && contains(in_body(state, box_rect), px, py) {
                    return Self::Rename;
                }
                if contains(in_body(state, key), px, py) {
                    return Self::AddKey;
                }
            }
            // ★★★ R1684 — the field, when it is standing on a form row, owns
            // its own rectangle: the screen stands aside so the caret hook can
            // put the caret where the pointer landed. Answering `Field` again here
            // would re-open the box and throw away what has been typed.
            if matches!(state.editing.get(), Some(Editing::Value { .. }))
                && contains(in_body(state, edit_box(state)), px, py)
            {
                return Self::Nothing;
            }
            let geometry = inspector_geometry(state);
            // ★★★★★ R1732 — the open roster is a LAYER and is asked first.
            // Walking the rows first would resolve a press on an option to
            // whichever row the roster happens to cover, which is why the
            // popup is published apart from `rows` rather than as more of
            // them. A press inside the roster's box but between two options is
            // still the roster's — letting it fall through is how a reader
            // dismisses a menu by accident.
            if let Some(popup) = &geometry.popup {
                if let Some(part) = geometry.option_at(px, py) {
                    return Self::Part {
                        key: popup.key.clone(),
                        part: part.to_owned(),
                    };
                }
                if geometry.on_popup(px, py) {
                    return Self::Nothing;
                }
            }
            for row in &geometry.rows {
                // ★★ R1686 — the seat that takes the row away, asked FIRST
                // because it is cut out of the header and a header that
                // answered first would swallow it. It is asked by name rather
                // than through `parts`, which means "inside the control" and
                // is relied on to by the option painter and the containment
                // gate.
                //
                // ★★ R1716 — and which act it is comes from the seat itself.
                // Reading the form again here would be a second answer to
                // "who owns this row", and the two would part on the day a
                // derivation was added on one side only.
                //
                // ★★ R1717 — giving a shared row back is the same request as
                // removing an authored one: the form decides what "take my
                // half out of this row" means from the row's own provenance,
                // so a third target here would be a second copy of that rule.
                if contains(row.seat.rect(), px, py) {
                    return match row.seat {
                        Seat::Remove(_) => Self::RemoveField(row.key.clone()),
                        Seat::TakeOver(_) => Self::AuthorField(row.key.clone()),
                        Seat::GiveBack(_) => Self::DisownField(row.key.clone()),
                    };
                }
                // Every affordance inside a control, from the geometry the
                // painter published — never a second layout.
                for (suffix, rect) in &row.parts {
                    if contains(*rect, px, py) {
                        return Self::Part {
                            key: row.key.clone(),
                            part: suffix.clone(),
                        };
                    }
                }
                if contains(row.control, px, py) || contains(row.header, px, py) {
                    return Self::Field(row.key.clone());
                }
            }
            for (key, rect) in &geometry.chips {
                if contains(*rect, px, py) {
                    return Self::AddField(key.clone());
                }
            }
            return Self::Nothing;
        }
        if contains(rail_rect(), px, py) {
            for (n, (name, _)) in spec::RAIL.iter().enumerate() {
                if contains(rail_seat(n), px, py) {
                    return Self::Rail(name);
                }
            }
            return Self::Nothing;
        }
        if contains(palette_rect(), px, py) {
            // ★★★★★ R1887 — the panel's own chrome first. See the inspector's
            // twin above, and `panel_chrome_hit` for why it is before the
            // scroll offset rather than after it.
            if let Some(hit) = panel_chrome_hit(state, SidePanel::Palette, palette_rect(), px, py) {
                return hit;
            }
            // ★ R1662 — the palette body SCROLLS, so a press has to be asked
            // in the frame the rows are stated in. Every rectangle here
            // (`palette_row`, `discovery_rect`) is written in the unscrolled
            // window frame, which is where the painter also reads it, so
            // folding the offset into the QUERY keeps one set of rectangles
            // rather than two. Without this the paint moved and the hit test
            // did not: R1662's end-to-end probe pressed the centre of the
            // scrolled-to `Querier` row and the screen answered `Publisher`,
            // which is the R1656 class exactly.
            let (px, py) = in_pane(&state.palette_scroll, palette_rect(), px, py);
            for (n, role) in Role::ALL.into_iter().enumerate() {
                if contains(palette_row(n), px, py) {
                    return Self::Role(role);
                }
            }
            if contains(discovery_rect(), px, py) {
                return Self::DiscoveryToggle;
            }
            return Self::Nothing;
        }
        if contains(toolbar_rect(), px, py) {
            return Self::on_toolbar(state, px, py);
        }
        if let Some(hit) = Self::in_overflow_menu(state, px, py) {
            return hit;
        }
        // ★ R1678 — the gate panel's reset row, BEFORE the canvas: the panel
        // floats over the canvas, so a press inside it that fell through to
        // the world would pan the graph out from under the button.
        for (scope, seat) in reset_seats(state) {
            if contains(seat, px, py) {
                return Self::Reset(scope);
            }
        }
        if contains(canvas_rect(), px, py) {
            return Self::on_canvas(state, px, py);
        }
        Self::Nothing
    }

    /// ★★★★★ R1791 — what a press inside the OPEN overflow menu reaches, asked
    /// **before the canvas** and for R1678's reason one control over.
    ///
    /// The menu hangs BELOW the toolbar pane, so the `contains(toolbar_rect())`
    /// test above is false for it and a press would fall through and pan the
    /// graph out from under the button somebody just opened. Measured: without
    /// this the menu painted, the seat was in the roster, and pressing `config`
    /// exported nothing.
    fn in_overflow_menu(state: &LabState, px: u32, py: u32) -> Option<Self> {
        if !state.toolbar_open.get() {
            return None;
        }
        let tag = overflow_menu_seats()
            .into_iter()
            .find(|(_, rect)| contains(*rect, px, py))
            .map(|(tag, _)| tag)?;
        Some(
            toolbar_seats(state)
                .into_iter()
                .find(|seat| seat.tag == tag)
                .map_or(Self::Nothing, |seat| seat.hit),
        )
    }

    /// What a press inside the canvas toolbar reaches — **from the roster**, so
    /// a seat cannot be pressable and unnamed, or named and unpressable.
    fn on_toolbar(state: &LabState, px: u32, py: u32) -> Self {
        toolbar_seats(state)
            .into_iter()
            .find(|seat| contains(seat.rect, px, py))
            .map_or(Self::Nothing, |seat| seat.hit)
    }

    /// ★★★★★ R1736 — the diagram's own marks, resolved from **the paint the
    /// framework kept** rather than worked out a second time.
    ///
    /// A card, a pin and the picked link's chrome are all things this screen
    /// DREW, under a camera transform, and until this round the press path
    /// re-derived every one of their rectangles from the model. That is the
    /// two-derivation shape this repository has repaired by hand four times —
    /// R1648's doubled offset, R1651.1's pane-versus-window frame, R1662's
    /// unscrolled palette, R1700's stale window size — and each repair left the
    /// structure that produced it standing.
    ///
    /// It is gone here. `pinion_core::painted` holds the tagged rectangle of
    /// every mark the last frame drew, in paint order, so "where is the card"
    /// has one answer and the press reads it.
    ///
    /// ★ **The priority is the paint's too.** The hand-written rule this
    /// replaces said "the chrome first, then pins, then cards, back to front",
    /// which is a z-order kept beside the painter's — and the pin is on top
    /// because the painter draws it on top. Last drawn is what the reader sees
    /// and now what a press reaches.
    ///
    /// Only the families the paint NAMES are resolved here. A wire carries no
    /// tag of its own, so `link_at` still parses the model for it; that is
    /// stated rather than hidden, and it is the remaining half of this axis on
    /// this screen.
    fn on_diagram_from_paint(state: &LabState, px: u32, py: u32) -> Option<Self> {
        // The paint is published in the WINDOW's frame and this screen resolves
        // in the LAYOUT's; below the comfortable size the two differ by exactly
        // the pan (R1714's relationship, read in the other direction).
        let (pan_x, pan_y) = pinion_core::shrink::window_pan(VIEW_TAG);
        let (wx, wy) = (px.checked_sub(pan_x)?, py.checked_sub(pan_y)?);
        let marks = pinion_core::painted::painted_regions(VIEW_TAG)?;
        marks
            .stack_at(wx, wy)
            .filter(|(tag, _)| {
                tag.starts_with("lab.node.")
                    || tag.starts_with("lab.pin.")
                    || tag.starts_with("lab.link.")
            })
            .map(|(tag, _)| Self::of_tag(state, tag))
            .find(|hit| !matches!(hit, Self::Nothing))
    }

    /// What a press inside the canvas viewport reaches.
    fn on_canvas(state: &LabState, px: u32, py: u32) -> Self {
        // ★★★★★ R1736 — the cards, the pins and the picked link's chrome, from
        // the paint. Their relative priority comes from the paint too.
        if let Some(hit) = Self::on_diagram_from_paint(state, px, py) {
            return hit;
        }
        // ★ The canvas is a viewport onto a world surface, so a press is
        // resolved in the surface's coordinates — the same ones the painter
        // places cards in. One conversion, at the boundary.
        let (cx, cy) = window_to_content(state, px, py);
        if let Some(link) = link_at(state, cx, cy) {
            return Self::Link(link);
        }
        // Reported links AFTER drawn ones: where the two run together the drawn
        // one is the one somebody made a decision about.
        if let Some((from, to)) = observed_at(state, cx, cy) {
            return Self::Observed(from, to);
        }
        // The frame's TAB, not its interior: the interior is where the cards
        // are, and a group that swallowed presses over its own members would
        // make a node undraggable the moment it joined one.
        for (id, _) in frames_of(state) {
            let r = frame_rect_of(state, id);
            let tab = Rect::new(r.x, r.y, r.w, scaled(state, FRAME_TAB).max(10));
            if holds(tab, cx, cy) {
                return Self::Frame(id);
            }
        }
        Self::Canvas
    }

    /// ★★★★★ R1700 — what the thing painted under `tag` addresses.
    ///
    /// The by-name half of the pair `scene/pointer_target` holds against the
    /// paint. Written to READ the tables the painter reads rather than to
    /// invert it: the toolbar comes out of `toolbar_seats`, which already pairs
    /// each seat's tag with its hit and which the press path resolves in the
    /// same order, so the two cannot drift. The rail, the palette and the reset
    /// scopes come out of their own declared rosters for the same reason. Only
    /// the parametric families — a card, its pins, a frame's tab, a link — are
    /// parsed here, and each parse is checked against the state that names them
    /// so an id nothing paints answers nothing.
    ///
    /// Answering [`None`] is a real answer for a caption or a rule. What it must
    /// not be is a shrug for something addressable: the census publishes how
    /// many painted rectangles a surface says are addressable, so an
    /// under-answer shows up as a number rather than as a pass.
    fn of_tag(state: &LabState, tag: &str) -> Self {
        if let Some(seat) = toolbar_seats(state).into_iter().find(|s| s.tag == tag) {
            return seat.hit;
        }
        if let Some(name) = tag.strip_prefix("lab.rail.")
            && let Some((name, _)) = spec::RAIL.iter().find(|(n, _)| *n == name)
        {
            return Self::Rail(name);
        }
        if let Some(name) = tag.strip_prefix("lab.palette.role.")
            && let Some(role) = Role::ALL.into_iter().find(|r| r.name() == name)
        {
            return Self::Role(role);
        }
        // R1885 — read BEFORE the `lab.node.` prefix below, which would
        // otherwise swallow `lab.node.build.reference` and look for a card of
        // that name. The longer prefix wins, which is the rule this router
        // already follows for `lab.pin.` and `lab.link.endpoint.`.
        if let Some(word) = tag.strip_prefix("lab.node.build.")
            && let Some(stack) = Stack::from_word(word)
        {
            return Self::Build(stack);
        }
        if tag == "lab.palette.discovery" {
            return Self::DiscoveryToggle;
        }
        if let Some(name) = tag.strip_prefix("lab.node.")
            && let Some(id) = state.node_of(name)
        {
            return Self::Node(id);
        }
        // ★★★★★ R1915 — `lab.pin.<card>.<pin>[.<member>…]`, split at the FIRST
        // dot after the card's name rather than the last.
        //
        // 🟥 `rsplit_once` was the defect: it read the last dotted segment as
        // the side, so `…accept.host` resolved its side to `host`, matched
        // nothing and answered `Nothing` — a member pin that was drawn,
        // announced, and unreachable by any press. The card's name cannot
        // contain a dot (`node_of` is asked, so an unknown name still answers
        // nothing), which is what makes splitting at the first dot correct
        // rather than merely different.
        if let Some(rest) = tag.strip_prefix("lab.pin.")
            && let Some((name, address)) = rest.split_once('.')
            && let Some(node) = state.node_of(name)
            && let Some((side, at)) = pin_address(address).ok()
        {
            return Self::Pin { node, side, at };
        }
        if let Some(name) = tag.strip_prefix("lab.frame.")
            && let Some((id, _)) = frames_of(state).into_iter().find(|(_, n)| n == name)
        {
            return Self::Frame(id);
        }
        if tag == "lab.link.act" {
            return Self::LinkAct;
        }
        if let Some(n) = tag
            .strip_prefix("lab.link.endpoint.")
            .and_then(|n| n.parse::<usize>().ok())
        {
            return Self::Endpoint(n);
        }
        if let Some(act) = NodeAct::ALL.into_iter().find(|a| a.tag() == tag)
            && state.active_card().is_some()
        {
            return Self::NodeAct(act);
        }
        if tag == "lab.inspector.rename" {
            return Self::Rename;
        }
        if tag == "lab.inspector.addkey" {
            return Self::AddKey;
        }
        if let Some(key) = tag.strip_prefix("lab.form.row.") {
            return Self::Field(key.to_owned());
        }
        if let Some(key) = tag.strip_prefix("lab.form.remove.") {
            return Self::RemoveField(key.to_owned());
        }
        if let Some(key) = tag.strip_prefix("lab.form.disown.") {
            return Self::DisownField(key.to_owned());
        }
        if tag == "lab.canvas" {
            return Self::Canvas;
        }
        Self::Nothing
    }

    /// The word the wire answers a press with, in the shape the framework's
    /// pointer census reads (R1700): `Nothing` where a press addresses nothing,
    /// so that "there is nothing here" and "here is a thing called nothing"
    /// cannot be confused.
    fn target(&self, state: &LabState) -> PointerTarget {
        match self {
            Self::Nothing => PointerTarget::Nothing,
            other => PointerTarget::Word(other.word(state)),
        }
    }

    /// The word the wire answers a press with.
    fn word(&self, state: &LabState) -> String {
        match self {
            Self::Nothing => "nothing".into(),
            Self::Rail(name) => format!("rail:{name}"),
            Self::Role(role) => format!("role:{}", role.name()),
            Self::Build(stack) => format!("build:{}", stack.word()),
            Self::DiscoveryToggle => "discovery".into(),
            Self::Zoom(up) => format!("zoom:{}", if *up { "in" } else { "out" }),
            Self::Fit => "fit".into(),
            Self::Problem => "problem".into(),
            Self::Reset(scope) => format!("reset:{}", scope.wire()),
            Self::Config => "config".into(),
            Self::Script => "script".into(),
            Self::SaveGraph => "save".into(),
            Self::OpenGraph => "open".into(),
            Self::ClearGraph => "clear".into(),
            // ★ R1887 — the word names the panel AND the act, because "you
            // pressed a panel control" does not say which of the two.
            Self::Panel(which, act) => format!(
                "panel:{}:{}",
                which.word(),
                match act {
                    PanelAct::Flip => "flip",
                    PanelAct::Fold => "fold",
                    PanelAct::Unfold => "unfold",
                }
            ),
            // ★ R1889 — its own word, so a wire reader can tell a press that
            // PLACES a panel from one that starts resizing it. Same shape as
            // the arm above and deliberately not folded into it: the two answer
            // different questions and a client branching on `panel:` would have
            // to re-split the string.
            Self::PanelGrip(which) => format!("panel-grip:{}", which.word()),
            // ★★★★★ R1791 — the word CARRIES what moved, so the wire answers
            // *where did save go* rather than only *you pressed the control*.
            // The floor has no member that names what its extension holds.
            Self::More => format!(
                "more:{}",
                right_cluster()
                    .moved()
                    .iter()
                    .map(|g| g.word())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Run => "run".into(),
            Self::Node(id) => format!("node:{}", state.name_of(*id)),
            // ★ R1915 — the member is in the word, so a driver reading what it
            // is standing on can tell `pin:P-02:dial` from `pin:P-02:dial.host`.
            Self::Pin { node, side, at } => {
                format!("pin:{}:{}", state.name_of(*node), pin_word(*side, at))
            }
            Self::Link(id) => format!("link:{}", id.0),
            Self::Observed(from, to) => format!(
                "observed:{}>{}",
                state.name_of(from.node),
                state.name_of(to.node)
            ),
            Self::LinkAct => "link:act".into(),
            Self::Endpoint(n) => format!("link:endpoint:{n}"),
            Self::Frame(id) => format!(
                "frame:{}",
                state.frames.borrow().get(id).cloned().unwrap_or_default()
            ),
            // R1682 — named by the act rather than by the card, because the
            // card is whatever is selected and the wire reads that separately.
            Self::NodeAct(act) => format!("card:{}", act.wire()),
            Self::Rename => "card:rename".into(),
            Self::AddKey => "card:addkey".into(),
            Self::Field(key) => format!("field:{key}"),
            Self::AddField(key) => format!("add:{key}"),
            Self::RemoveField(key) => format!("remove:{key}"),
            Self::AuthorField(key) => format!("author:{key}"),
            Self::DisownField(key) => format!("disown:{key}"),
            Self::Part { part, .. } => part.clone(),
            Self::Canvas => "canvas".into(),
        }
    }
}

/// The link whose wire passes within a few pixels of the cursor, in the world
/// surface's own coordinates.
fn link_at(state: &LabState, px: i64, py: i64) -> Option<LinkId> {
    let doc = state.doc.borrow();
    let tree = doc.tree(ROOT)?;
    for link in tree.links() {
        let (Some(a), Some(b)) = (
            card_rect(state, link.from.node),
            card_rect(state, link.to.node),
        ) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, a, true));
        let (bx, by) = centre(pin_rect(state, b, false));
        // Sample the straight chord: the wire is drawn as a curve between the
        // same two points, and the chord is within the tolerance a finger has.
        for step in 0..=20u32 {
            let t = f64::from(step) / 20.0;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a lerp between two pixels is a pixel"
            )]
            let (lx, ly) = (
                (f64::from(ax) + (f64::from(bx) - f64::from(ax)) * t) as i64,
                (f64::from(ay) + (f64::from(by) - f64::from(ay)) * t) as i64,
            );
            if px.abs_diff(lx) <= 6 && py.abs_diff(ly) <= 6 {
                return Some(link.id);
            }
        }
    }
    None
}

/// The reported link whose wire passes within a few pixels of the cursor
/// (R1681).
///
/// The same chord sampling as [`link_at`], over the other layer. Two functions
/// and not one because what they answer with is different — an observation has
/// no id — and folding them together would mean inventing one.
fn observed_at(state: &LabState, px: i64, py: i64) -> Option<(Socket, Socket)> {
    for seen in state.doc.borrow().observations(ROOT) {
        let (Some(a), Some(b)) = (
            card_rect(state, seen.from.node),
            card_rect(state, seen.to.node),
        ) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, a, true));
        let (bx, by) = centre(pin_rect(state, b, false));
        for step in 0..=20u32 {
            let t = f64::from(step) / 20.0;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a lerp between two pixels is a pixel"
            )]
            let (lx, ly) = (
                (f64::from(ax) + (f64::from(bx) - f64::from(ax)) * t) as i64,
                (f64::from(ay) + (f64::from(by) - f64::from(ay)) * t) as i64,
            );
            if px.abs_diff(lx) <= 6 && py.abs_diff(ly) <= 6 {
                return Some((seen.from, seen.to));
            }
        }
    }
    None
}

/// The drawn link landing on `node`'s accept pin that a cursor at `at` is
/// nearest to — the one a press on that pin picks up (R1681).
///
/// The reference's rule, and the reason it is the nearest and not the first: an
/// accept pin can hold several wires and the one a person means is the one they
/// are pointing at. A **reported** link is deliberately not eligible — it is
/// not in the drawing, so there is nothing to pick up.
fn link_into_pin(state: &LabState, node: NodeId, at: (i64, i64)) -> Option<LinkId> {
    let doc = state.doc.borrow();
    let mut best: Option<(u64, LinkId)> = None;
    for link in doc.tree(ROOT)?.links() {
        if link.to.node != node {
            continue;
        }
        let Some(card) = card_rect(state, link.from.node) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, card, true));
        let reach = at.0.abs_diff(i64::from(ax)).pow(2) + at.1.abs_diff(i64::from(ay)).pow(2);
        if best.is_none_or(|(held, _)| reach < held) {
            best = Some((reach, link.id));
        }
    }
    best.map(|(_, id)| id)
}

// ── Chrome rectangles ───────────────────────────────────────────────────────

/// Where the first seat sits below the app bar, and the gap between seats.
///
/// ★ R1773 — named because [`floor_height`] derives the screen's minimum height
/// from them. They were literals inside `rail_seat`, which is why no floor
/// could account for the rail: the numbers existed in one expression and
/// nothing else could read them.
const RAIL_SEAT_TOP: u32 = 10;
const RAIL_SEAT_PITCH: u32 = 42;

fn rail_seat(n: usize) -> Rect {
    Rect::new(
        8,
        app_bar_h() + RAIL_SEAT_TOP + u32::try_from(n).unwrap_or(0) * RAIL_SEAT_PITCH,
        38,
        38,
    )
}

/// ★★★★★ R1887 — **where the palette's body starts, in window coordinates**,
/// and every rectangle inside it is stated from here.
///
/// It was `rail_w() + PAD` and `app_bar_h() + 56` — the panel's OPENING place,
/// spelled out. R1802 made `palette_rect` derive from a placement that can
/// change and stopped there, so the panel could move and its contents could
/// not: the round that gives a reader the gesture is the round in which that
/// becomes a defect a person can see, and the first thing it would have done is
/// paint the palette on the right with its rows still on the left.
///
/// ⇒ ★ **Deriving the container and leaving the contents is half a derivation**,
/// and half a derivation is stable exactly until somebody uses the half that
/// moved.
fn palette_body_origin() -> (u32, u32) {
    // ★ R1909 — asked through the width, so the two derivations of "is there a
    // body" cannot disagree: an origin inside a strip is as meaningless as a
    // width of it, and a caller that got one but not the other would place
    // children into a pane that is not drawn.
    let _ = palette_body_w();
    let rect = palette_rect();
    let inside = side_panel_content(rect);
    (rect.x + inside.x, rect.y + inside.y)
}

/// The clearance the palette's first group heading keeps below the body's top.
///
/// It was folded into the `56` that stated the first row's position from the
/// window's top; separated because that `56` was two facts — where the body
/// starts, which the panel now answers, and how far in the content begins,
/// which is this.
const PAL_BODY_TOP: u32 = 56;

/// How wide a row of the palette's body is.
fn palette_body_w() -> u32 {
    palette_body_across().unwrap_or_else(|| {
        panic!(
            "the palette is folded to its strip and has no body, so there is no \
             width here to derive from. `side_panel` builds a folded panel's \
             body on no path at all, so reaching this is a caller measuring a \
             pane that draws nothing"
        )
    })
}

/// The palette's body width, or `None` while it is folded to its strip.
///
/// ★ R1909 — the palette's half of the [`EdgePlacement::content_extent`]
/// adoption. See [`inspector_body_across`] for what the `Option` is protecting
/// against and what a saturating subtraction cost when it was not there.
fn palette_body_across() -> Option<u32> {
    let (at, _) = placements();
    at.content_extent()?;
    Some(palette_rect().w.saturating_sub(PAD * 2))
}

/// ★★★★★ R1889 — how wide a row of the INSPECTOR's body is, and the half of
/// R1887's repair that round could not yet need.
///
/// Ten rectangles in this file derived their width from the `INSP_W` constant —
/// the pane's OPENING width. That was true while nothing could change
/// the width, which is exactly the arrangement R1887 wrote down as a defect
/// with a date: *a latent divergence is a defect whose date is the round that
/// builds the missing thing*, and R1802 had said the same sentence about
/// `toolbar_rect` one round before it came true.
///
/// This round builds the width drag, so this round is that date. The palette
/// got this treatment at R1887 ([`palette_body_w`]); the inspector gets it here,
/// BEFORE the grip exists, because the alternative is shipping a drag that
/// moves the panel and leaves its contents at the opening width.
fn inspector_body_w() -> u32 {
    inspector_body_across().unwrap_or_else(|| {
        panic!(
            "the inspector is folded to its strip and has no body, so there is \
             no width here to derive from. `side_panel` builds a folded panel's \
             body on no path at all, so reaching this is a caller measuring a \
             pane that draws nothing"
        )
    })
}

/// The inspector's body width, or `None` while it is folded to its strip.
///
/// ★★★★★ R1909 — through [`EdgePlacement::content_extent`], which is what
/// separates *this panel takes eighteen pixels* from *this panel has a body
/// eighteen pixels wide*. The strip is a real number and the arithmetic went
/// through: `inspector_rect().w.saturating_sub(PAD * 2)` answered **0** for a
/// folded pane, and one row deriving `body - 20` from that underflowed.
/// This screen's gates failed in a heap at that one line the first time a pane
/// here was declared to open folded, all of them asking about a body that was
/// not there.
///
/// ⚠ No count is given, and that is deliberate: the number this comment
/// originally carried was inherited from a run that had already repaired half
/// of it, so it could not be re-measured. *A figure nobody in this round could
/// reproduce is worse than no figure* — see the round's own ledger entry for
/// the counts that WERE measured here.
///
/// ⚠ `saturating_sub` is why it was quiet. A saturating operator turns *this
/// question has no answer* into a plausible number, which is the failure a
/// `None` exists to make impossible — the reason the framework's answer is an
/// `Option` and not a smaller `u32`.
fn inspector_body_across() -> Option<u32> {
    let (_, at) = placements();
    // The panel's own answer first: whether there is a body at all is not a
    // question about the rectangle, which is the strip either way.
    at.content_extent()?;
    Some(inspector_rect().w.saturating_sub(PAD * 2))
}

fn palette_row(n: usize) -> Rect {
    let n = u32::try_from(n).unwrap_or(0);
    // Two groups of four, each under its own heading.
    let group = n / 4;
    let within = n % 4;
    let (x, y) = palette_body_origin();
    Rect::new(
        x + PAD,
        y + PAL_BODY_TOP
            + group * (PAL_HEAD_H + 4 * PAL_ROW_H + 12)
            + PAL_HEAD_H
            + within * PAL_ROW_H,
        palette_body_w(),
        PAL_ROW_H - 5,
    )
}

/// Where the pin legend starts — under both palette groups.
fn legend_top() -> u32 {
    palette_body_origin().1 + PAL_BODY_TOP + 2 * (PAL_HEAD_H + 4 * PAL_ROW_H + 12)
}

fn legend_row(n: usize) -> Rect {
    Rect::new(
        palette_body_origin().0 + PAD,
        legend_top() + PAL_HEAD_H + u32::try_from(n).unwrap_or(0) * 20,
        palette_body_w(),
        18,
    )
}

/// ★ R1874 — the border a transport chip draws around its word, above and
/// below it. Named because the chip's HEIGHT is derived from it and the caption
/// has to clear it: a caption is placed inside the box's rectangle, and the box
/// paints a 1px stroke on that rectangle's own edge.
const CHIP_BORDER: u32 = 1;

fn protocol_chip(n: usize) -> Rect {
    Rect::new(
        palette_body_origin().0 + PAD + u32::try_from(n).unwrap_or(0) * 40,
        legend_top() + PAL_HEAD_H + 3 * 20 + 6,
        36,
        // ★★★★★ R1874 — DERIVED. Authored `18`, which held a 10px word only
        // because the caption was being given a box of exactly its face size;
        // once `caption` started asking for a line box (17) the word cleared
        // the chip's own border by nothing and the containment gate reported
        // all five chips 1px past their box, in every state.
        line_box(10) + CHIP_BORDER * 2,
    )
}

fn discovery_rect() -> Rect {
    Rect::new(
        palette_body_origin().0 + PAD,
        legend_top() + PAL_HEAD_H + 3 * 20 + 6 + 18 + 20 + PAL_HEAD_H,
        palette_body_w(),
        58,
    )
}

/// The clearance the launch chip keeps between its frame and its word, above
/// and below.
const GATE_PAD: u32 = 1;

/// The toolbar's LEFT cluster, window-absolute: the graph's name.
///
/// ★★★ R1688 — these three were written inline in the painter, in the
/// toolbar's own frame, while every seat of the right cluster came from a
/// function. That asymmetry is why [`TOOLBAR_LEFT_CLUSTER`] was still a number
/// nothing could measure after R1687 derived its sibling: there was nothing to
/// measure it *from*. It is the same class as
/// [[debt-a-stated-limit-is-not-checked-by-anything]], sitting beside the round
/// that closed the other half of it.
///
/// ★ Each box ends where the next begins — a text node's rectangle is the box
/// the run is wrapped into, and two that overlap paint one string over another
/// as soon as either grows.
fn toolbar_title_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(bar.x + PAD, bar.y + 15, 152, 16)
}

/// The node and link counts, beside the name.
fn toolbar_meta_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(bar.x + PAD + 160, bar.y + 17, 132, 13)
}

/// The launch-gate chip: the verdict, and — since R1688 — the way to the first
/// thing wrong with it.
fn gate_chip_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(
        bar.x + PAD + 300,
        bar.y + 12,
        104,
        line_box(FONT_SMALL) + (PANEL_FRAME + GATE_PAD) * 2,
    )
}

/// How far in from the toolbar's right edge each seat of the zoom pill ends.
///
/// ★★★ R1688 — **the pill is the reference's, in the reference's order**:
/// `−` · the read-out · `+` · fit. The read-out is not a label beside the
/// buttons, it is the control that puts the view back — which is what the
/// reference makes it, and what this screen had spelled as a separate seat
/// captioned `home` since R1678. Two facts in two places became one control
/// that shows the scale and restores it.
///
/// Every width here is [`seat_w`] of the **widest word the seat can ever
/// hold**, not of the word it holds now: a seat sized to `84%` would grow when
/// the zoom reached three digits, and a control that changes width as you use it
/// is a target that moves under the pointer.
const PILL_GAP: u32 = 6;
/// The clear space between the pill and the cluster of buttons beside it.
const CLUSTER_GAP: u32 = 12;
/// A zoom stepper's side.
const ZOOM_BTN: u32 = 24;

/// The zoom read-out's seat, which is also the view reset's — see [`PILL_GAP`].
///
/// Sized for `400%`, the widest reading [`ZOOM_MAX`] allows.
fn view_read_w() -> u32 {
    seat_w(&format!("{ZOOM_MAX}%"))
}

/// The fit seat's, sized for its own caption.
fn fit_w() -> u32 {
    seat_w("fit")
}

/// One of the three file seats the reference groups into a pill of its own.
///
/// ★★★ R1689 — **the reference's own grouping, in the reference's own place**:
/// between the launch-script button and the run button, three small buttons
/// sharing one background. It is a pill rather than three loose buttons for the
/// same reason the zoom cluster is: they act on one subject — the file — and a
/// person reads the group before reading the words.
const FILE_SEATS: [(&str, Hit); 3] = [
    ("save", Hit::SaveGraph),
    ("open", Hit::OpenGraph),
    ("clear", Hit::ClearGraph),
];

/// The clear space at the toolbar's right edge, and the run seat's width.
const RUN_INSET: u32 = 14;
const RUN_W: u32 = 106;
/// A captioned action button's width — `config` and `script`.
const ACTION_W: u32 = 66;

/// ★★★★ R1689 — **every right-anchored seat's inset is DERIVED from the one to
/// its right**, where three of them were written as the constants `120`, `196`
/// and `268`.
///
/// Adding a seat in the middle of that is what made the difference matter: the
/// three constants encoded the *gaps* between five seats, so inserting a sixth
/// meant re-deriving all of them by hand and re-checking the arithmetic against
/// a screenshot. R1687 already paid for this once on
/// `TOOLBAR_RIGHT_CLUSTER` — a width stated in prose and re-derived by
/// whoever came next — and this is the same fact one level down.
fn file_pill_w() -> u32 {
    FILE_SEATS.iter().map(|(word, _)| seat_w(word)).sum::<u32>() + PILL_GAP * 2
}

/// Where the file pill's right edge sits, in from the toolbar's right edge.
///
/// R1791 — asked of the laid-out cluster rather than added up here, so it
/// answers where the pill is when a narrow toolbar has moved something.
fn file_pill_right() -> u32 {
    group_right(ToolGroup::File).unwrap_or(0)
}

/// The seat of file button `n`, left to right inside the pill.
fn file_rect(n: usize) -> Rect {
    let bar = toolbar_rect();
    let left = bar.x + bar.w - file_pill_right() - file_pill_w();
    let before: u32 = FILE_SEATS
        .iter()
        .take(n)
        .map(|(word, _)| seat_w(word) + PILL_GAP)
        .sum();
    Rect::new(
        left + before,
        bar.y + 11,
        seat_w(FILE_SEATS.get(n).map_or("", |(word, _)| word)),
        ZOOM_BTN,
    )
}

/// ★★★★★ R1791 — **the right cluster's groups**, right to left, and what each
/// one does when the toolbar runs out of room.
///
/// # Why groups and not seats
///
/// Two of the pairings here are DECISIONS somebody already made and wrote down,
/// and an overflow that took a seat at a time would quietly undo them. R1687:
/// *"its sibling renders the plan as a document and this one renders it as a
/// script, so they belong beside each other and not one behind a menu"* — so
/// `config` and `script` are one group. The three file seats are already drawn
/// as one pill. Grouping is how a stated decision survives a narrow window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ToolGroup {
    /// The zoom steppers, the read-out and the fit seat — the canvas controls.
    Zoom,
    /// The pair that takes the plan off the screen.
    Export,
    /// Save, open and clear.
    File,
    /// The launch seat.
    ///
    /// ★ [`overflow::WhenTight::Keep`]. A person opens this screen to run the
    /// graph, and
    /// a toolbar that hid the run button to make room for the zoom read-out
    /// would have got the trade exactly backwards.
    Run,
}

impl ToolGroup {
    /// Left to right, which is the order a reader meets them in and therefore
    /// the order [`overflow::lay`] gives them up from the end of.
    const IN_ROW: [Self; 4] = [Self::Zoom, Self::Export, Self::File, Self::Run];

    /// What this group needs, its own seats and their inner gaps.
    fn width(self) -> u32 {
        match self {
            Self::Zoom => {
                ZOOM_BTN + PILL_GAP + view_read_w() + PILL_GAP + ZOOM_BTN + PILL_GAP + fit_w()
            }
            Self::Export => ACTION_W + CLUSTER_GAP + ACTION_W,
            Self::File => file_pill_w(),
            Self::Run => RUN_W,
        }
    }

    /// The seats this group holds, left to right — which is the order they
    /// appear in the overflow menu when the group has moved.
    const fn seats(self) -> &'static [&'static str] {
        match self {
            Self::Zoom => &[
                "lab.toolbar.zoom.out",
                "lab.reset.view",
                "lab.toolbar.zoom.in",
                "lab.toolbar.fit",
            ],
            Self::Export => &["lab.toolbar.config", "lab.toolbar.script"],
            Self::File => &["lab.toolbar.save", "lab.toolbar.open", "lab.toolbar.clear"],
            Self::Run => &["lab.toolbar.run"],
        }
    }

    /// Tags this group PAINTS that are not seats — a caption drawn inside one
    /// of its seats rather than a control of its own.
    ///
    /// ★★★★★ R1791.1 — they go with the group and they are not menu rows, which
    /// is exactly the distinction that was missing. `seats()` alone answers
    /// "where do the rows go"; this pair answers "what stopped being painted",
    /// and a reader who needs the second and is given the first calls a moved
    /// caption LOST. Measured: `r1709` read `lab.toolbar.zoom` — the zoom
    /// read-out's label, painted inside the view-reset seat — as a declared
    /// region the reader could not bring into view, while the in-process gate
    /// [`in_toolbar_overflow`] knew better from a hand-written case of its own.
    /// Two spellings of one fact, and the wire had the incomplete one.
    const fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Zoom => &["lab.toolbar.zoom"],
            Self::Export | Self::File | Self::Run => &[],
        }
    }

    /// Every tag this group is responsible for painting: its seats and its
    /// captions. What "this group moved" means, in the vocabulary a gate reads.
    fn tags(self) -> impl Iterator<Item = &'static str> {
        self.seats()
            .iter()
            .copied()
            .chain(self.labels().iter().copied())
    }

    /// The tag the overflow control lists this group under.
    const fn word(self) -> &'static str {
        match self {
            Self::Zoom => "zoom",
            Self::Export => "export",
            Self::File => "file",
            Self::Run => "run",
        }
    }
}

/// The overflow control's own width, and the clear space it keeps.
///
/// ★ Charged only when something moves — see [`overflow::lay`]. A row that
/// fits does not carry a control that opens onto nothing.
const OVERFLOW_W: u32 = 28;

/// ★★★★★ R1791 — the right cluster, **decided for the width it actually has**.
///
/// # What this is the repair of
///
/// A reader opened the assembled tool and reported the inspector cut off.
/// Measured: the shipped window is 1440, this screen's page gets 1388, and this
/// screen declared it needed 1625 — and 1029 of that was this cluster plus its
/// sibling, in a rigid row. The comment on `TOOLBAR_RIGHT_CLUSTER` had
/// already written the answer and could not take it: *"what would take it back
/// is an overflow affordance on the toolbar, which this tree does not have"*.
fn right_cluster() -> overflow::Row<ToolGroup> {
    let room = toolbar_rect()
        .w
        .saturating_sub(TOOLBAR_LEFT_CLUSTER + RUN_INSET);
    let items: Vec<overflow::Item<ToolGroup>> = ToolGroup::IN_ROW
        .iter()
        .map(|group| {
            let item = overflow::Item::new(group.width() + CLUSTER_GAP, *group);
            if *group == ToolGroup::Run {
                item.kept()
            } else {
                item
            }
        })
        .collect();
    overflow::lay(room, OVERFLOW_W + CLUSTER_GAP, items).unwrap_or_else(|_| {
        // The control is wider than the whole cluster: there is no arrangement,
        // and a screen this narrow has already been refused by its own
        // `ShrinkPolicy`. Laying the run seat alone keeps the paint total.
        overflow::lay(u32::MAX, 0, vec![overflow::Item::new(0, ToolGroup::Run)])
            .expect("an unbounded row always fits")
    })
}

/// How far in from the toolbar's right edge `group` sits, or `None` when it
/// moved into the overflow.
///
/// Derived by walking the SHOWN groups right to left, which is the same chain
/// the hand-written `*_right` functions were — with the one difference that it
/// walks what is on screen rather than what the code assumed always would be.
fn group_right(group: ToolGroup) -> Option<u32> {
    let laid = right_cluster();
    let mut inset = RUN_INSET;
    for shown in laid.shown().iter().rev() {
        if *shown == group {
            return Some(inset);
        }
        inset += shown.width() + CLUSTER_GAP;
    }
    None
}

/// What the groups ON the row need, right edge inward — the derived answer to
/// the question `TOOLBAR_RIGHT_CLUSTER`'s hand-written 609 used to give.
///
/// Counts only what is shown, plus the overflow control when there is one, so
/// it is true at a narrow size as well as a wide one. A constant could not be:
/// it cannot know that something moved.
fn right_cluster_wants() -> u32 {
    let laid = right_cluster();
    let mut wants = RUN_INSET;
    for shown in laid.shown() {
        wants += shown.width() + CLUSTER_GAP;
    }
    if laid.needs_affordance() {
        wants += OVERFLOW_W + CLUSTER_GAP;
    }
    wants
}

/// ★★★★★ R1791 — where a moved group's seats sit **when the control is open**:
/// a column under it, in row order, each seat keeping its own tag.
///
/// Keeping the tags is the decision. A moved control that changed its name
/// would be a second control doing the same thing, and every gate that presses
/// `lab.toolbar.config` would be pressing something else; keeping them means a
/// seat MOVES rather than being replaced, which is also what a person
/// experiences.
fn overflow_menu_seats() -> Vec<(&'static str, Rect)> {
    let Some(control) = overflow_rect() else {
        return Vec::new();
    };
    let mut out: Vec<(&'static str, Rect)> = Vec::new();
    let mut y = control.y + control.h + MENU_GAP;
    for group in right_cluster().moved() {
        for tag in group.seats() {
            out.push((tag, Rect::new(control.x, y, MENU_W, MENU_ROW_H)));
            y += MENU_ROW_H;
        }
    }
    out
}

/// The overflow menu's own geometry: wide enough for the longest caption it
/// holds, and clear of the control it hangs from.
const MENU_W: u32 = 96;
const MENU_ROW_H: u32 = 26;
const MENU_GAP: u32 = 4;

/// Where the overflow control sits, when there is one.
fn overflow_rect() -> Option<Rect> {
    if !right_cluster().needs_affordance() {
        return None;
    }
    let bar = toolbar_rect();
    // The control's own width and gap are the last thing `right_cluster_wants`
    // adds, so taking them back off lands on its right edge — one walk, read
    // two ways, rather than two walks that can disagree.
    let inset = right_cluster_wants() - OVERFLOW_W - CLUSTER_GAP;
    Some(Rect::new(
        bar.x + bar.w - inset - OVERFLOW_W,
        bar.y + 11,
        OVERFLOW_W,
        ZOOM_BTN,
    ))
}

/// Where the launch-script seat's right edge sits, in from the toolbar's right.
fn script_right() -> u32 {
    group_right(ToolGroup::Export).unwrap_or(0)
}

/// Where the zoom pill's right edge sits, in from the toolbar's right edge.
fn pill_right() -> u32 {
    group_right(ToolGroup::Zoom).unwrap_or(0)
}

fn zoom_rect(plus: bool) -> Rect {
    let bar = toolbar_rect();
    let right = bar.x + bar.w;
    // Right to left: fit, `+`, the read-out, `-`.
    let inset = if plus {
        pill_right() + fit_w() + PILL_GAP + ZOOM_BTN
    } else {
        pill_right()
            + fit_w()
            + PILL_GAP
            + ZOOM_BTN
            + PILL_GAP
            + view_read_w()
            + PILL_GAP
            + ZOOM_BTN
    };
    Rect::new(right - inset, bar.y + 11, ZOOM_BTN, ZOOM_BTN)
}

/// ★★ R1688 — the seat that frames the whole graph, at the pill's trailing end,
/// which is where the reference puts it.
fn fit_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(
        bar.x + bar.w - pill_right() - fit_w(),
        bar.y + 11,
        fit_w(),
        ZOOM_BTN,
    )
}

fn config_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(
        bar.x + bar.w - script_right() - ACTION_W - CLUSTER_GAP - ACTION_W,
        bar.y + 9,
        ACTION_W,
        28,
    )
}

/// ★★ R1687 — the second of the pair the reference puts side by side. Its
/// sibling renders the plan as a document and this one renders it as a script,
/// so they belong beside each other and not one behind a menu.
fn script_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(
        bar.x + bar.w - script_right() - ACTION_W,
        bar.y + 9,
        ACTION_W,
        28,
    )
}

fn run_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(bar.x + bar.w - RUN_INSET - RUN_W, bar.y + 9, RUN_W, 28)
}

/// One seat of the canvas toolbar: where it is, what pressing it does, and what
/// it announces as.
struct ToolbarSeat {
    /// The paint tag, which is also how a test aims at it.
    tag: &'static str,
    /// Where it is, window-absolute.
    rect: Rect,
    /// What a press on it reaches.
    hit: Hit,
    /// What it announces as — what the control *does right now*, not what it is
    /// called in general.
    name: String,
}

/// ★★★★ R1688 — **the toolbar's seats, once**: the hit test finds a press in
/// this list, the accessibility tree names this list, and the width gate
/// measures this list.
///
/// It exists because of the shape of the defect this round would otherwise have
/// added. R1687 derived the right cluster's declared width from the rectangles
/// — a real improvement over the prose it replaced — but it derived it from a
/// list of seven rectangles written out *inside the test*. This round added an
/// eighth seat, and that gate went on measuring seven and reporting the old
/// answer: green, and blind to the very change it was written for. A gate that
/// enumerates by hand is a gate that measures the screen as it was on the day
/// it was written ([[debt-a-stated-limit-is-not-checked-by-anything]], third
/// occurrence in three rounds).
///
/// ★ The order is the reader's, left to right, because that is also the order a
/// press is resolved in and the two must not be two orders.
fn toolbar_seats(state: &LabState) -> Vec<ToolbarSeat> {
    let seat = |tag, rect, hit, name: String| ToolbarSeat {
        tag,
        rect,
        hit,
        name,
    };
    vec![
        // ★★ The launch chip. Its name is the verdict AND what pressing it
        // does: a reader told only "go to the first problem" has not been told
        // whether there is one.
        seat(
            "lab.toolbar.gate",
            gate_chip_rect(),
            Hit::Problem,
            match state.problems().len() {
                0 => "gate passed, nothing to go to".to_owned(),
                1 => "gate: 1 finding, go to it".to_owned(),
                n => format!("gate: {n} findings, go to the first"),
            },
        ),
        seat(
            "lab.toolbar.zoom.out",
            zoom_rect(false),
            Hit::Zoom(false),
            "zoom out".to_owned(),
        ),
        // ★★ The accessible name CONTAINS the visible one, because the read-out
        // is this control's own caption: a button labelled `84%` whose name was
        // only "reset the view" is the label-in-name failure, and this seat is
        // exactly the case that rule is about.
        seat(
            "lab.reset.view",
            view_reset_rect(),
            Hit::Reset(ResetScope::View),
            format!("zoom {}%, reset the view", state.zoom.get()),
        ),
        seat(
            "lab.toolbar.zoom.in",
            zoom_rect(true),
            Hit::Zoom(true),
            "zoom in".to_owned(),
        ),
        seat(
            "lab.toolbar.fit",
            fit_rect(),
            Hit::Fit,
            "fit the graph to the view".to_owned(),
        ),
        seat(
            "lab.toolbar.config",
            config_rect(),
            Hit::Config,
            "export the configuration".to_owned(),
        ),
        seat(
            "lab.toolbar.script",
            script_rect(),
            Hit::Script,
            "produce the launch script".to_owned(),
        ),
        // ★★ R1689 — the file pill. `open` announces whether there is anything
        // to open, because that is the fact a person cannot see from the button
        // and the one that decides whether pressing it does anything.
        seat(
            "lab.toolbar.save",
            file_rect(0),
            Hit::SaveGraph,
            "save the graph".to_owned(),
        ),
        seat(
            "lab.toolbar.open",
            file_rect(1),
            Hit::OpenGraph,
            if persist::stored(state).is_empty() {
                "open the saved graph, nothing saved yet".to_owned()
            } else {
                "open the saved graph".to_owned()
            },
        ),
        seat(
            "lab.toolbar.clear",
            file_rect(2),
            Hit::ClearGraph,
            "clear back to the graph this screen opens with".to_owned(),
        ),
        seat(
            "lab.toolbar.run",
            run_rect(),
            Hit::Run,
            if state.running.get() {
                "stop".to_owned()
            } else if state.verdict().may_launch() {
                "run".to_owned()
            } else {
                "run blocked".to_owned()
            },
        ),
    ]
    .into_iter()
    // ★★★★★ R1791 — a seat whose GROUP moved is not on the row: it is in the
    // menu, at the rect the menu gives it, keeping its own tag. This is the
    // floor's third answer inverted — measured at 6.11, a hidden action's own
    // `isVisible()` still answers true, so a reader asking what a toolbar can
    // do is told about controls a person cannot see. Here the roster and the
    // paint are one list, and a moved seat MOVES rather than vanishing.
    .filter_map(|s| relocate_if_moved(state, s))
    // ★★ And the control that holds them NAMES them, which the floor has no
    // member for at all.
    .chain(overflow_control_seat(state))
    .collect()
}

/// ★★★★★ R1791 — the overflow control as a seat, **named for what it holds**.
///
/// The floor's extension button has no member that says what is behind it, so a
/// reader is left to work it out from what is missing. This one's accessible
/// name is the list, and it says whether pressing it opens or closes — the two
/// are different offers and a person told only "more" has not been told which.
/// ★★★★★ R1791 — open or close the overflow, and say which.
///
/// A toggle and not a one-way open, because the control is the only way back:
/// the seats it holds are painted over the canvas, and a person who opened it
/// to look has to be able to stop looking.
fn toggle_overflow(state: &LabState) {
    let open = !state.toolbar_open.get();
    state.toolbar_open.set(open);
    let held: Vec<&str> = right_cluster().moved().iter().map(|g| g.word()).collect();
    state.say(Utterance::done(if open {
        format!("showing {}", held.join(", "))
    } else {
        "closed".to_owned()
    }));
}

fn overflow_control_seat(state: &LabState) -> Option<ToolbarSeat> {
    let rect = overflow_rect()?;
    let held: Vec<&str> = right_cluster().moved().iter().map(|g| g.word()).collect();
    Some(ToolbarSeat {
        tag: "lab.toolbar.more",
        rect,
        hit: Hit::More,
        name: format!(
            "{} {}",
            if state.toolbar_open.get() {
                "close"
            } else {
                "more:"
            },
            held.join(", ")
        ),
    })
}

/// ★★★★★ R1791 — whether `tag` is a toolbar element the row has **moved behind
/// the overflow control**.
///
/// The one question a gate needs in order to tell *this control was lost* from
/// *this control is one press away, and the thing holding it says so*. The
/// floor cannot answer it: measured at 6.11, a hidden action's own `isVisible`
/// still reports true, so there is nothing to ask.
///
/// Answers `false` for a tag that is not a cluster seat at all, so the grant it
/// gives is exactly the seats whose group moved and nothing else.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the wire asks it through `toolbar_overflow`; \
    this spelling is the per-TAG one the paint gates need"
    )
)]
pub(crate) fn in_toolbar_overflow(tag: &str) -> bool {
    // ★ R1791.1 — through `tags()`, which is seats AND the captions painted
    // inside them. This used to carry its own hand-written case for
    // `lab.toolbar.zoom` while the wire's `moved_seats` carried none, so the two
    // answers to "what moved" disagreed by exactly that caption.
    right_cluster()
        .moved()
        .iter()
        .flat_map(|group| group.tags())
        .any(|moved| moved == tag)
}

/// ★★★★★ R1909 — **the pane that holds this declared element**, from the
/// specification's own [`spec::PaneSpec::holds`] lists.
///
/// One lookup, so "which pane is this in" is answered in the same place for
/// every caller instead of by a prefix each of them writes. `None` for a tag no
/// pane claims, which is a state the census below is what refuses — an
/// unclassified element is not an excused one.
///
/// ⚠ Longest prefix wins, because the lists are allowed to nest: the canvas
/// claims `lab.canvas` and the panes each claim their own tag, and a screen
/// that later names something `lab.canvas.overlay.inspector` must not be
/// claimed by whichever list happens to be checked first.
pub(crate) fn pane_holding(tag: &str) -> Option<&'static spec::PaneSpec> {
    spec::PANES
        .iter()
        .filter(|pane| pane.holds.iter().any(|prefix| tag.starts_with(prefix)))
        .max_by_key(|pane| {
            pane.holds
                .iter()
                .filter(|prefix| tag.starts_with(*prefix))
                .map(|prefix| prefix.len())
                .max()
                .unwrap_or(0)
        })
}

/// ★★★★★ R1909 — whether this declared element is inside a pane that is
/// **currently folded**, and therefore correctly not painted.
///
/// The peer of [`in_toolbar_overflow`] below and excused the same way and for
/// the same reason: a control the screen has put away is not a control the
/// screen lost. The difference is that a folded pane's contents are not
/// reachable either — folding is not scrolling — so this excuse is the stronger
/// one, and it is checked in BOTH directions by
/// `r1909_a_folded_pane_hides_exactly_what_it_holds`: everything under a folded
/// pane's prefixes must be gone, and everything under an open one's must be
/// back. An excuse list that only ever excused would be the escape hatch that
/// disables its own gate.
pub(crate) fn in_folded_pane(tag: &str) -> bool {
    let Some(pane) = pane_holding(tag) else {
        return false;
    };
    SidePanel::ALL
        .into_iter()
        .find(|which| which.tag() == pane.tag)
        .is_some_and(|which| {
            STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map_or(pane.opens.folded, |state| which.at(state).folded)
            })
        })
}

/// ★★★★★ R1791 — a seat whose group the row gave up: where it is now, or
/// nothing while the control that holds it is closed.
///
/// It keeps its own tag and its own name — a seat MOVES rather than being
/// replaced, which is what a person experiences and what lets every gate that
/// presses `lab.toolbar.config` go on pressing the configuration export. The
/// floor's answer to the same question is that a hidden action reports itself
/// visible, so nothing can tell where it went.
fn relocate_if_moved(state: &LabState, seat: ToolbarSeat) -> Option<ToolbarSeat> {
    match seat_group(seat.tag) {
        Some(group) if right_cluster().moved().contains(&group) => {
            if !state.toolbar_open.get() {
                return None;
            }
            let rect = overflow_menu_seats()
                .into_iter()
                .find(|(tag, _)| *tag == seat.tag)
                .map(|(_, rect)| rect)?;
            Some(ToolbarSeat { rect, ..seat })
        }
        _ => Some(seat),
    }
}

/// Which right-cluster group a toolbar seat belongs to, or `None` for one that
/// is not in the cluster at all (the launch chip, which sits with the title).
fn seat_group(tag: &str) -> Option<ToolGroup> {
    match tag {
        "lab.toolbar.zoom.out" | "lab.toolbar.zoom.in" | "lab.reset.view" | "lab.toolbar.fit" => {
            Some(ToolGroup::Zoom)
        }
        "lab.toolbar.config" | "lab.toolbar.script" => Some(ToolGroup::Export),
        "lab.toolbar.save" | "lab.toolbar.open" | "lab.toolbar.clear" => Some(ToolGroup::File),
        "lab.toolbar.run" => Some(ToolGroup::Run),
        _ => None,
    }
}

/// The launch gate panel, bottom right of the canvas.
///
/// ★ R1678 — it grows a row when there is something to put back. The height is
/// derived from [`changed_scopes`] rather than reserved, because a permanently
/// reserved strip is a band of empty panel on the screen a person spends the
/// most time looking at, and the reference makes the same choice (its reset
/// affordances are conditional, not disabled).
/// The left edge of the launch panel — where the canvas's bottom band runs out.
///
/// ★ R1700 — its own function because THREE things need it and only its height
/// depends on the state: the panel itself, the toast that must not run under
/// it, and the gesture strip, which until this round was a constant that
/// happened to clear it at one window size.
fn gate_panel_x() -> u32 {
    let canvas = canvas_rect();
    canvas.x + canvas.w - 262
}

fn gate_rect(state: &LabState) -> Rect {
    let canvas = canvas_rect();
    let (shown, hidden) = gate_shown(state);
    let rows = u32::try_from(shown.len() + usize::from(hidden > 0)).unwrap_or(0);
    let resets = u32::from(!changed_scopes(state).is_empty()) * RESET_ROW_H;
    let h = GATE_TOP_H + rows * GATE_LINE_H + resets;
    Rect::new(
        gate_panel_x(),
        canvas.y + canvas.h - h - GATE_MARGIN,
        250,
        h,
    )
}

/// The panel's chrome and its verdict row, above the problem lines.
const GATE_TOP_H: u32 = 54;
/// One problem line.
const GATE_LINE_H: u32 = 20;
/// ★★★★★ R1927 — the face the verdict and every problem line are set in.
///
/// Named because three places wrote `9` and a fourth wrote the `13` that was
/// meant to hold it — and `line_box(9)` is 15, so all three boxes were two
/// pixels short of their own descenders for the panel's whole life. A face
/// with a name is a face the box beside it can be derived from.
const GATE_LINE_FONT: u32 = 9;
/// How far the panel floats from the canvas edge.
const GATE_MARGIN: u32 = 12;

/// **The problem lines the gate panel shows, and how many it has no room for.**
///
/// ★★★★★ R1690 — the panel's height was a function of how many problems the
/// graph has, and that is unbounded: enough of them and the box was placed
/// above the top of the canvas, where the pane-local conversion underflowed and
/// the screen panicked. Found the first time the identifier's declared shape
/// was enforced — three of the opening graph's own values turned out to be
/// unparseable, and **three extra lines were enough to reach it**. So the
/// affordance had been one bad graph away from a crash for its whole life, and
/// what hid it was that nothing on this screen could produce many problems at
/// once.
///
/// The panel says what it is not showing rather than stopping at the edge. A
/// silent truncation would be the worse failure: the launch verdict is derived
/// from **all** the problems, so a panel that showed four of seven would have a
/// gate that reads closed for reasons the reader cannot see.
fn gate_shown(state: &LabState) -> (Vec<(bool, String)>, usize) {
    let canvas = canvas_rect();
    let resets = u32::from(!changed_scopes(state).is_empty()) * RESET_ROW_H;
    let room = canvas
        .h
        .saturating_sub(GATE_MARGIN * 2 + GATE_TOP_H + resets);
    let fits = usize::try_from(room / GATE_LINE_H).unwrap_or(0);
    let all = state.gate_lines();
    if all.len() <= fits {
        return (all, 0);
    }
    // One of the rows that fit is spent saying how many do not.
    let keep = fits.saturating_sub(1);
    let hidden = all.len() - keep;
    (all.into_iter().take(keep).collect(), hidden)
}

/// The height the reset row adds to the gate panel: the buttons plus the gap
/// above them.
const RESET_ROW_H: u32 = 32;
/// One reset button's height.
const RESET_BTN_H: u32 = 22;

/// Where each reset affordance sits, window-absolute — **the one list the paint
/// and the hit test both read**.
///
/// R1651.1 is why it is one list: that round painted the option chips
/// content-hugging and hit-tested them by equal division, so the second chip
/// answered for the first. A seat computed twice is two layouts.
fn reset_seats(state: &LabState) -> Vec<(ResetScope, Rect)> {
    let scopes = changed_scopes(state);
    if scopes.is_empty() {
        return Vec::new();
    }
    let gate = gate_rect(state);
    let inner = gate.w - 24;
    let gap = 6;
    let count = u32::try_from(scopes.len()).unwrap_or(1);
    let each = (inner + gap).saturating_sub(gap * count) / count;
    let y = gate.y + gate.h - RESET_BTN_H - 8;
    scopes
        .into_iter()
        .enumerate()
        .map(|(n, scope)| {
            let n = u32::try_from(n).unwrap_or(0);
            (
                scope,
                Rect::new(gate.x + 12 + n * (each + gap), y, each, RESET_BTN_H),
            )
        })
        .collect()
}

/// The view reset's seat: **the zoom read-out itself**, between the two
/// steppers.
///
/// Unconditional, in the zoom cluster, which is where the reference keeps it —
/// see [`ResetScope::gated`] for why this one is not on the panel.
///
/// ★★★ R1688 — it was a seat of its own captioned `home`, which the reference
/// does not have: there, the percentage *is* the button that puts the view back.
/// Merging them is not tidying. It is one control for one subject — a reading of
/// the scale and the way to undo it — where this screen had a number that could
/// not be pressed sitting next to a word that did not say what it restored. It
/// also costs the toolbar less width than the two did, which is the difference
/// between adding the fit seat and not being able to.
///
/// ★ R1687 had to widen the old seat from 34 to 48 because `home` was painted
/// `ho…`. Nothing here is picked: the width is [`seat_w`] of the widest reading
/// this screen can show, and the truncation gate is what settles it.
fn view_reset_rect() -> Rect {
    let out = zoom_rect(false);
    Rect::new(out.x + ZOOM_BTN + PILL_GAP, out.y, view_read_w(), ZOOM_BTN)
}

/// Where a toolbar seat's caption goes: the seat, inset.
///
/// ★★★★★ R1687 — **derived, because a counterfactual walked through the hole
/// it left.** The captions were authored as their own constants — `home` in a
/// 40-wide box on a 34-wide seat, `config` in a 60-wide box on a 66-wide seat
/// starting 12 in — so two of the three were painted PAST the button they name.
/// Narrowing a seat to prove the truncation gate fired did not fire it: the
/// caption kept its own width, so it neither elided nor sat inside the seat the
/// gate was asking about, and the check went quiet on a worse defect than the
/// one it was written for.
///
/// A caption that cannot leave its seat can only answer by eliding, which is
/// what the gate reads.
///
/// ★★★★★ R1689 — **the height is the FONT's line box, centred, and the round
/// obligation to look at the screen is what found that.** It was `seat.h - 12`,
/// an inset guessed on both edges: on a 24-high seat that leaves 12 px for an
/// 11 px face whose line box reserves 18, so the `p` of `open` was painted with
/// its descender cut off at the button's border. The seats already on this
/// toolbar at that height carry `-`, `+`, `84%` and `fit` — **not one of them
/// has a descender** — which is why six rounds of gates never saw it.
///
/// It was short on the 28-high seats too, by two pixels, so `config` and
/// `script` were being trimmed as well, just not enough to notice. A guessed
/// inset is a guess about a font; [`line_box`] is what the font actually asks
/// for, and centring what is left is what a caption in a button means.
///
/// ★ The gate that did not catch it is not wrong, it is aimed elsewhere:
/// R1687's asks whether the run's RECT sits inside the seat and whether the
/// text elided. Both were true. What overflowed is the INK inside that rect,
/// which is a different question and a registered one
/// ([[debt-an-overflow-policy-applies-to-the-runs-own-rect]]). What is checked
/// here now is the reservation itself —
/// `r1689_every_toolbar_caption_reserves_its_line` — because that is the half a
/// view function can settle without a shaper.
const fn seat_caption(seat: Rect) -> Rect {
    let line = line_box(FONT_SMALL);
    Rect::new(
        seat.x + SEAT_INSET,
        seat.y + seat.h.saturating_sub(line) / 2,
        seat.w.saturating_sub(SEAT_INSET * 2),
        line,
    )
}

/// How far a toolbar caption sits inside its seat, on each side.
///
/// ★ Measured, not chosen: at 10 the view reset's 48-wide seat leaves a 28 px
/// box and `home` elides to `ho…` — which is the defect this round found by
/// looking at the screen, reappearing from the other direction. The gate is
/// what settled the number.
const SEAT_INSET: u32 = 6;

/// Where the last thing this screen SAID is shown, or `None` when it has not
/// said anything yet.
///
/// ★★★★★ R1688 — **found by looking at the screen, and it had been false for
/// thirty-seven rounds.** Four comments in this file say a refusal "has already
/// reached the toast, which is where a person reads it", and there was no
/// toast: `say` writes a signal, the signal is published on the wire, and
/// nothing painted it. Every gate was green because every gate reads the wire.
/// So the last card refusing to be deleted, a name already taken, a value that
/// cannot be expressed — each was a control that appeared to do nothing.
///
/// It is [[debt-a-stated-limit-is-not-checked-by-anything]] from the other
/// direction: not a limit stated and unchecked, but a CAPABILITY stated and
/// unbuilt, which no census can see because the thing it would count is absent.
///
/// # Two deliberate differences from the reference
///
/// * **It does not time out.** The reference clears its toast after 2.6
///   seconds. A message that vanishes on a wall clock makes every check that
///   reads it a race against a timer ([[zero-flake-policy]]), and it is also the
///   complaint people have about toasts — you look up and it has gone. This one
///   stands until the screen says something else, which is what a professional
///   tool's status line does.
/// * **It sits above the hint strip and left of the launch panel**, rather than
///   centred on the window. Both of those are already there, and a message
///   painted over either is a message competing with the two things this screen
///   most needs to keep readable. The room it is centred in is what is left.
fn toast_rect(state: &LabState) -> Option<Rect> {
    // ★ R1719 — `None` is "nothing has been said", and it is now the ONLY
    // spelling of that: the emptiness test this line used to make could not
    // tell a screen that had said nothing from one that had announced an empty
    // sentence, and an `Utterance` cannot be the second thing.
    let said = state.toast.showing()?.sentence();
    let canvas = canvas_rect();
    let hint = hint_rect();
    let gate = gate_rect(state);
    let h = line_box(FONT_SMALL) + (PANEL_FRAME + TOAST_PAD) * 2;
    // The band left of the launch panel: its own left edge, less the gap.
    let room = gate.x.saturating_sub(canvas.x + 24).max(TOAST_MIN_W);
    let w = (seat_w(&said) + TOAST_DOT + TOAST_PAD * 2).min(room);
    Some(Rect::new(
        canvas.x + 12 + room.saturating_sub(w) / 2,
        hint.y.saturating_sub(h + 10),
        w,
        h,
    ))
}

/// The clearance the toast keeps between its frame and its content.
const TOAST_PAD: u32 = 6;
/// The dot the reference draws before the message, and the gap after it.
const TOAST_DOT: u32 = 18;
/// A toast is never narrower than this, however little room the canvas has —
/// below it the message would be an ellipsis and nothing else.
const TOAST_MIN_W: u32 = 120;

/// What the strip at the foot of the canvas says a pointer can do — the one
/// place this screen states its gestures.
///
/// ★ R1700 — lifted out of the painter so [`hint_rect`] can be the size of what
/// it holds. Two readers, one string.
fn hint_text() -> String {
    spec::GESTURES
        .iter()
        .map(|(gesture, what)| format!("{gesture} = {what}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn hint_rect() -> Rect {
    let canvas = canvas_rect();
    // ★ R1656 — clamped to the pane it sits in. It was a flat 470, so on a
    // canvas narrower than that the strip advertising the screen's gestures was
    // painted over the inspector beside it.
    //
    // ★★ R1700 — and it is now the size of the SENTENCE, not a number chosen
    // at the design size. Found by looking at a maximised window: the strip
    // read "... drag a pin = author a li…" with sixteen hundred pixels of empty
    // canvas beside it, because 470 was a constant that had to keep a relation
    // to a quantity that moves. The toast below already sized itself this way
    // (`seat_w`); the strip did not, and nothing could see the difference
    // because both are correct at 1440.
    //
    // ★★★ The ROOM is the band left of the launch panel, which is the same
    // room the toast takes and for the same reason — and the first draft of
    // this took the whole canvas instead, whereupon the text-smear gate refused
    // the screen on its next boot with the strip painted over the launch
    // panel's last finding. A constant replaced by a measurement still needs
    // the bound the constant was accidentally providing.
    let room = gate_panel_x().saturating_sub(canvas.x + 24);
    let w = (seat_w(&hint_text()) + 4).min(room).max(80);
    Rect::new(canvas.x + 12, canvas.y + canvas.h - 34, w, 24)
}

// ── The inspector ───────────────────────────────────────────────────────────

fn selected_form(state: &LabState) -> Option<ConfigForm> {
    let node = state.active_card()?;
    selected_form_of(state, node)
}

/// One named card's form, whichever card is selected (R1684).
///
/// The edit paths name the card they are about rather than reading the
/// selection, so a commit cannot land on a different card than the one the
/// field was opened over.
fn selected_form_of(state: &LabState, node: NodeId) -> Option<ConfigForm> {
    shown_form(state, node)
}

/// **The form a card SHOWS**: the rows somebody wrote, plus the rows this
/// screen works out from the graph.
///
/// ★★★★★ R1716 — composed on every read and stored nowhere. The three
/// sources move — a card is dragged into another host frame, a wire is drawn,
/// a node is deleted — and a stored copy would need somebody to remember to
/// rebuild it at each of those places. That is the call-site habit the round
/// before this one took out of the focus path; here it never gets in, because
/// the derivation IS the read. The behaviour canon does the same thing in the
/// same place, and its comment says why in one line: values that derive from
/// values must have exactly one path, or the two recurse.
///
/// What is stored is the authored half alone — see [`amend`], which is the one
/// way anything changes a form.
fn shown_form(state: &LabState, node: NodeId) -> Option<ConfigForm> {
    let stored = state.forms.borrow().get(&node).cloned()?;
    let role = state.role_of(node)?;
    let mut rows: Vec<ConfigField> = Vec::with_capacity(stored.fields().len() + 3);
    rows.push(mode_row(role));
    // ★★★★★ R1717 — the connect row is held back and composed at the end,
    // because it is the one row a person and this canvas BOTH have something to
    // say about. Leaving the written half in place here and appending the wires
    // after it would put two rows at one path, which is a configuration with no
    // single value. The behaviour canon holds the same key back in the same
    // loop for the same reason.
    rows.extend(
        stored
            .fields()
            .iter()
            .filter(|f| f.key() != DIALLED_KEY)
            .cloned(),
    );
    if let Some(host) = host_row(state, node, &stored) {
        rows.push(host);
    }
    if let Some(dialled) = dialled_row(state, node, &stored) {
        rows.push(dialled);
    }
    let offered: Vec<ConfigField> = stored.addable().into_iter().cloned().collect();
    Some(ConfigForm::new(rows, offered))
}

/// The `mode` row: what session a node of this role comes up as.
///
/// ★★ Two sources and not one: a role that decides the mode is named as the
/// source, and a role that does not gets the mode the example programs start
/// in — said in those words, because "peer" with no provenance would read as a
/// setting somebody chose for this node.
fn mode_row(role: Role) -> ConfigField {
    let (value, from) = match role.mode() {
        Some(implied) => (implied, "role"),
        None => ("peer", "example default"),
    };
    // ★ R1842 — the shape comes from the option surface like every other row's.
    // `mode` is a path the target declares, so the words it takes are the
    // surface's business; this row spelled them again from `Role::MODES`, which
    // is the shape-beside-the-row arrangement R1690 removed everywhere else.
    // The surface refines `mode` FROM `Role::MODES`, so there is still one set.
    ConfigField::new("mode", "mode", Applies::Restart, value)
        .with_shape(settings::shape_or_free("mode"))
        .derived_from(from)
}

/// The `host` row — **only once there is more than one host to be on**.
///
/// ★★ The canon's rule, and it is about what a reader can act on: a graph that
/// runs everywhere in one place has nothing to say here, and a row answering
/// "the only host" on every card is noise that trains people to skip the panel.
///
/// It goes ASIDE. The host is not a key the target has; it is where the process
/// is started, which the plan already carries per node — so a row that shipped
/// it inside the configuration would put one fact in two files, one of which
/// the target would warn about and ignore.
fn host_row(state: &LabState, node: NodeId, stored: &ConfigForm) -> Option<ConfigField> {
    if stored.field("host").is_some() {
        // Somebody owns it — the stored row is the one the screen shows, and
        // `host_of` already reads it.
        return None;
    }
    let hosts: BTreeSet<String> = state
        .cards()
        .into_iter()
        .map(|card| state.host_of(card))
        .collect();
    if hosts.len() < 2 {
        return None;
    }
    let host = state.host_of(node);
    Some(
        ConfigField::new("host", "text", Applies::Restart, host)
            .derived_from("frame")
            .goes_aside("placement"),
    )
}

/// The configuration path a node's outgoing links land on.
///
/// Named once because three things read it — the row that composes it, the
/// loop that holds it back, and the gate that judges it — and a fourth spelling
/// of a dotted path is how a screen starts editing a key nothing ships.
const DIALLED_KEY: &str = "connect.endpoints";

/// R1778 — the owner-scoped marker that registers this screen's toast clock once.
const TOAST_TICKER_KEY: &str = "hello-node-lab/toast-ticker";

/// How long this screen's message stays, in seconds — the reference's own
/// number, the same one its two sibling screens use.
const TOAST_SECONDS: f32 = 2.6;

/// The `connect.endpoints` row: **what somebody wrote and what this canvas
/// draws, composed**.
///
/// ★★★★★ R1716 measured the defect: `R-01` showed one address nothing in the
/// graph listens on while the canvas drew three links out of it, and the
/// exported configuration shipped that one address — so the plan dialled a node
/// it was not drawn to reach and missed one it was. R1716 fixed half of it, by
/// deriving the row **whenever nobody had written one**, and paid for the other
/// half with a gate warning: a written value took the whole row, and the wires
/// stopped reaching the configuration.
///
/// ★★★★★ R1717 closes it. A node may be told to dial something this canvas
/// does not draw at all — an already-running router, say — and it is still
/// wired to what the canvas *does* draw. Those are two contributions to one
/// list, not two answers to one question, so the row holds both: what somebody
/// wrote first, then every drawn address they had not already named. The
/// behaviour canon composes in exactly that order.
///
/// A dialled address is the target's listen endpoint read from where the target
/// actually runs: `0.0.0.0` and `[::]` mean *every* address and cannot be
/// dialled, so the host frame's name stands in — which is the canon's rule and
/// the reason the host row above is not decoration.
///
/// Reported links are left out. They are what a source SAW, not what this graph
/// says to do, and a configuration built from them would dial connections
/// nobody drew.
fn dialled_row(state: &LabState, node: NodeId, stored: &ConfigForm) -> Option<ConfigField> {
    let written = stored.field(DIALLED_KEY);
    let addresses = dialled_from(state, node);
    match (written, addresses.is_empty()) {
        // Nobody wrote one and nothing is drawn: there is no row.
        (None, true) => None,
        // Only the canvas has something to say.
        (None, false) => Some(
            ConfigField::new(DIALLED_KEY, "address[]", Applies::Hot, addresses.join(", "))
                .with_shape(settings::shape_or_free(DIALLED_KEY))
                .derived_from("wire"),
        ),
        // Only a person has.
        (Some(written), true) => Some(written.clone()),
        // Both. The shape is a list, so `with_derived` cannot refuse — and the
        // refusal is kept rather than unwrapped because a shape that stopped
        // being a list would otherwise become a panic on a screen instead of a
        // row that quietly shows the written half.
        (Some(written), false) => written
            .clone()
            .with_derived("wire", addresses.join(", "))
            .ok(),
    }
}

/// Every address this node's drawn links dial, in link order.
fn dialled_from(state: &LabState, node: NodeId) -> Vec<String> {
    let landings: Vec<(NodeId, String)> = {
        let doc = state.doc.borrow();
        let Some(tree) = doc.tree(ROOT) else {
            return Vec::new();
        };
        tree.links()
            .iter()
            .filter(|link| link.from.node == node)
            .filter_map(|link| {
                let endpoint = endpoint_of(&doc, link.to)?;
                (!endpoint.trim().is_empty()).then_some((link.to.node, endpoint))
            })
            .collect()
    };
    let mut out: Vec<String> = Vec::new();
    for (target, endpoint) in landings {
        // ★ `0.0.0.0` and `[::]` mean EVERY address, so they are not something
        // to dial: what reaches that node is the host it runs on. A link whose
        // target sits in no frame therefore dials the word the plan uses for
        // that, which is honest and is visibly not an address.
        let host = state.host_of(target);
        let dialled = endpoint.replace("0.0.0.0", &host).replace("[::]", &host);
        if !out.contains(&dialled) {
            out.push(dialled);
        }
    }
    out
}

/// **Amend a card's form.** The one way anything changes one.
///
/// ★★★★★ R1716 — the request is answered against the form the screen SHOWS,
/// and what is kept is the half somebody authored. Both halves matter:
///
/// * Answering from the store would tell a person that `mode` is *no such
///   field* — true of the store, false of the screen, and unactionable either
///   way. The framework's own refusal ("worked out from the role") is the one
///   worth reading, and it can only come from a form that holds the row.
/// * Keeping the store as the authored half means a row taken over
///   **materialises** here, in the same act, with the value it was derived to —
///   and no other code has to know that is what a take-over is.
///
/// The reconciliation is total rather than a replay of the operation: every
/// authored row of the shown form is made to be in the store, and every stored
/// row the shown form no longer holds goes. One rule covers set, add, remove
/// and take-over, so a seventh act needs nothing here.
fn amend<T>(
    state: &LabState,
    node: NodeId,
    op: impl FnOnce(&mut ConfigForm) -> Result<T, FormError>,
) -> Result<T, FormError> {
    let mut shown =
        shown_form(state, node).ok_or_else(|| FormError::NoSuchField("this card".to_owned()))?;
    let answer = op(&mut shown)?;
    let mut forms = state.forms.borrow_mut();
    let stored = forms
        .get_mut(&node)
        .ok_or_else(|| FormError::NoSuchField("this card".to_owned()))?;
    for field in shown.fields() {
        // ★★★★★ R1717 — **the written half, never the shown one.** A row with
        // two contributors shows the composition, and a store that kept that
        // would freeze the canvas's addresses into somebody's configuration in
        // the same act as their first keystroke — after which deleting the link
        // would leave an address behind that nobody could explain. The floor
        // does exactly this, measured: writing into a derived value ends the
        // derivation and the value never follows its source again.
        //
        // It is the ROW that is narrowed, not only the text: a shared row put
        // away whole would carry its derivation, and the next read would
        // compose the wires onto a value that already held them.
        let Some(mine) = field.written_row() else {
            continue;
        };
        match stored.field(field.key()) {
            Some(held) if held.written() == mine.written() => {}
            Some(_) => stored.set(field.key(), mine.value())?,
            None => stored.add_typed(mine)?,
        }
    }
    // ★★★ R1717 — "no longer somebody's writing", not "no longer on the
    // screen". A shared row given back stays on the screen — the wires still
    // draw it — and the whole point of the act is that the store stops holding
    // the written half. A rule that looked only at membership would leave that
    // value behind and compose it back onto the row one read later.
    let gone: Vec<String> = stored
        .fields()
        .iter()
        .filter(|f| {
            shown
                .field(f.key())
                .and_then(ConfigField::written)
                .is_none()
        })
        .map(|f| f.key().to_owned())
        .collect();
    for key in gone {
        stored.remove(&key)?;
    }
    Ok(answer)
}

/// Where the inspector's form is laid out.
///
/// `WrapAll` + `AllGrow`, which is the reference inspector's own choice and the
/// right one for the reason its own screen shows: a configuration path is long,
/// and a key column wide enough for `transport.link.tx.batch_size` would leave
/// no room for its value.
fn form_style() -> FormStyle {
    FormStyle::default()
        .with_width(inspector_body_w())
        .with_policy(RowWrap::WrapAll, FieldGrowth::AllGrow)
}

/// How far down the inspector's body the reach meter's pill sits.
///
/// ★★★ R1690 — under the edit row and above the form, which is where the
/// reference puts it: the pill is about the palette the chips below come from,
/// so it reads as a heading for them rather than as another fact about the
/// selected node.
const REACH_ROW_Y: u32 = EDIT_ROW_Y + NODE_ACT_H + 8;

/// How tall the reach meter's pill is.
const REACH_H: u32 = 20;

/// Where the inspector's identity block ends and its form begins.
///
/// R1682 moved it down by one row: the node's-life seats sit between the degree
/// box and the form. R1683 moved it down by another: the one text field and the
/// seat that opens it sit under those. R1690 moved it down by a third, for the
/// reach meter — and derives it from that row rather than restating a number,
/// which is what the two moves before it did and what left this constant to be
/// re-checked by hand each time.
const INSP_HEAD_H: u32 = REACH_ROW_Y + REACH_H + 6;

/// ★★ R1682 — what a person can do to the selected card itself.
///
/// The reference puts exactly these in its inspector beside the node's
/// identity, which is the right place for the same reason it gives: they are
/// the only affordances that act on the *card* rather than on one of its
/// fields, and a canvas gesture for them would collide with placing and wiring,
/// the two things a press on a card already means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeAct {
    /// Draw it small, or full size again.
    Collapse,
    /// Switch it off, or back on.
    Disable,
    /// Take it off the canvas.
    Delete,
    /// ★★★★★ R1912 — put the card's unwired pins away, or bring them all back.
    ///
    /// The DCC's socket-hide operator, whose own description is *toggle unused
    /// node socket display*, and the engine's *restore all structure pins* —
    /// one seat, because they are the two directions of one gesture and the
    /// reference spells them as one toggle for that reason.
    ///
    /// ⚠ The per-pin scopes the engine also has (*remove this pin*, *remove all
    /// other pins*) are not seats: a press on a pin in this lab already means
    /// *start a wire*, which is the collision the comment above this enum names.
    /// They are reachable through the declared `put_away_pins` action, which is
    /// where this screen puts a verb a gesture cannot carry.
    Pins,
}

impl NodeAct {
    /// The census. Consumers iterate this rather than re-listing the arms.
    const ALL: [Self; 4] = [Self::Collapse, Self::Disable, Self::Delete, Self::Pins];

    /// The word a press on this seat answers with, and the action that does the
    /// same thing — one name, so the two channels cannot drift.
    const fn wire(self) -> &'static str {
        match self {
            Self::Collapse => "collapse",
            Self::Disable => "disable",
            Self::Delete => "delete_node",
            Self::Pins => "put_away_pins",
        }
    }

    /// The tag the seat is painted under, which is also what a driver presses.
    const fn tag(self) -> &'static str {
        match self {
            Self::Collapse => "lab.inspector.collapse",
            Self::Disable => "lab.inspector.disable",
            Self::Delete => "lab.inspector.delete",
            Self::Pins => "lab.inspector.pins",
        }
    }

    /// What the seat says, given what the card is doing now.
    ///
    /// The two toggles name the act they would perform rather than the state
    /// the card is in — the reference's own choice, and the one that makes a
    /// button readable without first working out which way round it is.
    fn word(self, collapsed: bool, disabled: bool, pins_away: bool) -> &'static str {
        match self {
            Self::Collapse if collapsed => "expand",
            Self::Collapse => "collapse",
            Self::Disable if disabled => "switch on",
            Self::Disable => "switch off",
            Self::Delete => "delete",
            // ★ R1912 — the reference's own toggle direction: if anything is
            // away, the press brings it back; otherwise it puts the unused ones
            // away. Naming the ACT rather than the state, like the two above.
            Self::Pins if pins_away => "show pins",
            Self::Pins => "hide pins",
        }
    }

    /// Where the seat sits in the frame the inspector's body is drawn in — the
    /// same frame the identity labels above it and the form below it use, so
    /// the whole pane scrolls as one thing.
    ///
    /// ⚠ R1912 — the divisor is [`ALL`](NodeAct::ALL)'s length and was the
    /// literal `3`. A fourth seat would have been laid out three-across and
    /// drawn off the end of the pane; the count and the census were two facts
    /// free to disagree, which is what this workspace lifts on sight.
    fn local_seat(self) -> Rect {
        let n = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        let across = u32::try_from(Self::ALL.len()).unwrap_or(1);
        let width = (inspector_body_w() - NODE_ACT_GAP * (across - 1)) / across;
        let step = u32::try_from(n).unwrap_or(0) * (width + NODE_ACT_GAP);
        Rect::new(PAD + step, NODE_ACT_Y, width, NODE_ACT_H)
    }
}

/// A node's-life seat in WINDOW coordinates — where a pointer meets it.
///
/// Derived from the painted rectangle by the pane's placement and its scroll
/// offset. A second set of rectangles written in window coordinates is how the
/// paint and the gesture come to disagree once the pane is scrolled (R1662).
///
/// ★ [`PANEL_FRAME`] is part of the transform and was missing from the first
/// draft: the seats are painted inside the panel's border, so the body's origin
/// is the panel's origin plus its frame. Measured — the seat answered one pixel
/// left and one pixel up of where the layout put it, which a 90-wide seat
/// absorbs and a narrow one would not.
fn node_act_seat(state: &LabState, act: NodeAct) -> Rect {
    in_body(state, act.local_seat())
}

/// A rectangle stated in the inspector body's own frame, in WINDOW coordinates.
///
/// The one transform every affordance in that pane goes through, so a second
/// one cannot be written in window coordinates and drift once the pane scrolls
/// (R1662). [`PANEL_FRAME`] is part of it: the body is drawn inside the panel's
/// border.
fn in_body(state: &LabState, local: Rect) -> Rect {
    let (dx, dy) = body_origin(state);
    let shift = |v: u32, by: i32| -> u32 {
        u32::try_from((i64::from(v) + i64::from(by)).max(0)).unwrap_or(0)
    };
    Rect::new(shift(local.x, dx), shift(local.y, dy), local.w, local.h)
}

/// ★★★ R1684 — where the inspector's scrolling body sits, **signed**, and the
/// single definition [`in_body`] and [`inspector_geometry`] both stand on.
///
/// Signed because the two consumers disagree about what to do when the scroll
/// carries a rectangle off the top, and both are right: a single affordance is
/// clamped to the pane (it is still the nearest thing to a press at the edge),
/// while a form row is DROPPED (`FormGeometry::translated`), because reporting
/// it at the edge would have a screen reader announce a row the reader has
/// scrolled away. Folding the clamp into the shared term would force one answer
/// on both.
fn body_origin(state: &LabState) -> (i32, i32) {
    let pane = inspector_rect();
    let (ox, oy) = state.inspector_scroll.offset();
    // ★★★★★ R1887 — through [`side_panel_content`], which is the same function
    // the painter hands the scroll pane. It read `pane.y + PANEL_FRAME`, which
    // was the content origin exactly while the panel reserved nothing of
    // itself; the header this round adds moved that origin, and a second
    // spelling of it would have put every affordance in this pane a header's
    // height above where it is drawn — the R1656 class this transform exists to
    // prevent, reintroduced by the round that grew the chrome.
    let body = side_panel_content(pane);
    (
        i32::try_from(pane.x + body.x).unwrap_or(i32::MAX) - ox,
        i32::try_from(pane.y + body.y).unwrap_or(i32::MAX) - oy,
    )
}

/// ★★ R1683 — where the one text field sits in the inspector's body, and the
/// seat that opens it on the card's name.
///
/// Under the node's-life row, because it IS a node's-life operation — the one
/// that needs a value typed. The reference puts its name box in the same place
/// for the same reason.
fn rename_row() -> (Rect, Rect, Rect) {
    let width = inspector_body_w();
    let apply = seat_w("rename");
    let key = seat_w("+ key");
    let box_w = width.saturating_sub(apply + key + NODE_ACT_GAP * 2);
    (
        Rect::new(PAD, EDIT_ROW_Y, box_w, NODE_ACT_H),
        Rect::new(PAD + box_w + NODE_ACT_GAP, EDIT_ROW_Y, apply, NODE_ACT_H),
        Rect::new(
            PAD + box_w + apply + NODE_ACT_GAP * 2,
            EDIT_ROW_Y,
            key,
            NODE_ACT_H,
        ),
    )
}

/// How wide a seat holding this word is, at the inspector's small face.
///
/// ★★★★★ R1794 — **the SHAPER answers, with the per-character estimate kept only
/// for where nothing can shape.**
///
/// It read `word.len() * FONT_SMALL * 7 / 10 + 16`, which is the per-character
/// fallback `measured_text_extent`'s own doc names as the defect class: *"a
/// caller falls back to its own per-character estimate, so any layout derived
/// from measured text came out one way in the paint and another way in a
/// pointer handler ... Both boxes were 'right' and only their derivation
/// disagreed"*. Fifteen call sites here sized seats that way, and the caption
/// inside each was then drawn `Start`-aligned in a box that was not the word's
/// width — which is why a reader reported `delete`, `collapse` and `switch off`
/// as not centred while every gate in this tree was green.
///
/// Measured: the estimate makes `delete` 58px wide; the shaper makes the glyphs
/// **27**. The seat is padded around what the word actually measures now, so
/// `SEAT_PAD` is the only number left and it is a design choice rather than an
/// approximation of one.
fn seat_w(word: &str) -> u32 {
    let ink = pinion_core::measured_text_extent(word, &run_style(FONT_SMALL, MEASURING_INK), None)
        .map_or_else(
            // Headless, before any provider has shaped anything: the old
            // estimate, kept so a unit test that paints outside a shell still
            // lays out deterministically.
            || u32::try_from(word.len()).unwrap_or(6) * FONT_SMALL * 7 / 10,
            pinion_core::TextExtent::width,
        );
    ink + SEAT_PAD * 2
}

/// A colour to build a style for MEASURING with, and only that.
///
/// The shaper needs a `TextStyle` and the colour is the one field that cannot
/// affect an advance, so it is named rather than left as a bare literal a reader
/// would take for the seat's ink.
const MEASURING_INK: Color = Color::rgb(0, 0, 0);

/// The clear space a seat keeps on each side of its word.
///
/// 8 rather than 16-split-two-ways: the old `+ 16` was padding *and* whatever
/// slack the per-character estimate happened to leave, and separating them is
/// what lets this be a decision.
const SEAT_PAD: u32 = 8;

/// How far down the inspector's body the text field's row sits.
///
/// ★ R1706 — derived from the node's-life row it sits under. It was `134`,
/// which was that arithmetic written out, so inserting a row above meant
/// finding this number and redoing it in one's head.
const EDIT_ROW_Y: u32 = NODE_ACT_Y + NODE_ACT_H + 2;

/// ★★★ R1684 — **where the one field is, which is wherever it is editing.**
///
/// The name and the key are typed in the box under the node's-life row; a
/// VALUE is typed **on the row it belongs to**, so the box appears over that
/// row's control and the person types where they pressed.
///
/// This is the whole reason the field was built with a target at R1683. A form
/// has as many rows as the document has keys, and giving each one a live box
/// would mean a focus owner, a keymap and a commit path per key — the reference
/// gets away with it because the browser owns all three. Here there is one
/// field and it MOVES, which costs one function and keeps the count at one.
///
/// Stated in the inspector body's own frame, like every other rectangle in that
/// pane, so it rides the scroll rather than being shifted by hand.
fn edit_box(state: &LabState) -> Rect {
    let shut = rename_row().0;
    let Some(Editing::Value { key, element, .. }) = state.editing.get() else {
        return shut;
    };
    let geometry = inspector_geometry_local(state);
    let Some(row) = geometry.rows.iter().find(|row| row.key == key) else {
        return shut;
    };
    match element {
        None => row.control,
        // ★ The ELEMENT's own rectangle, from the parts the painter published
        // — never re-derived here. A list's rows are laid out by the form
        // painter and a second arithmetic for where the third one is would be
        // the R1651.1 defect in a new place.
        Some(n) => row.part(&format!("item.{key}.{n}")).unwrap_or(row.control),
    }
}

/// ★★★ R1706 — where the selection-count chip sits, under the degree pill and
/// over the node's-life row.
///
/// The reference puts it in exactly that gap, and the placement is an argument
/// rather than a copy: everything above it is about ONE card — its identifier,
/// its role, how many links reach it — and everything below it acts on
/// something. The chip is what says how many things the acts below are about,
/// so it belongs at the seam.
const SEL_COUNT_Y: u32 = 112;
/// How tall the selection-count chip is.
const SEL_COUNT_H: u32 = 22;

/// The node's-life row: how far down the inspector it sits, how tall its seats
/// are, and the gap between them.
///
/// ★ R1706 — derived from the chip above rather than restated, which is the
/// discipline the rest of this column already keeps: three rounds moved this
/// block down and each one that wrote a fresh number left the next reader to
/// re-check it by hand.
const NODE_ACT_Y: u32 = SEL_COUNT_Y + SEL_COUNT_H + 4;
/// How tall a node's-life seat is.
///
/// ★ R1683 trimmed it from 24 to 20, and the reason is a measurement rather
/// than taste: the head grew twice this session, and at 24 the settings form's
/// add-a-key chips fell below the pane's fold on the opening screen. The pane
/// scrolls, so nothing was unreachable — but a person should not have to
/// scroll to reach an affordance the screen opens with.
const NODE_ACT_H: u32 = 20;
/// The gap between two node's-life seats.
const NODE_ACT_GAP: u32 = 6;

/// The form's geometry in the frame the PAINTER draws it in: inside the
/// inspector's scrolling body, so it rides the scroll instead of being shifted
/// by hand every time the offset moves.
fn inspector_geometry_local(state: &LabState) -> FormGeometry {
    let form = selected_form(state).unwrap_or_default();
    let picking = state.picking.get();
    let open = picking.as_ref().map(|(key, picker)| OpenPicker {
        key,
        picker,
        room: inspector_room(state),
    });
    form_geometry_showing(&form, (PAD, INSP_HEAD_H), &form_style(), open)
}

/// ★★★★★ R1732 — **the room an open roster has**, in the frame the form's
/// rectangles are stated in.
///
/// The pane is what knows, which is why the widget asks instead of deciding:
/// the form is taller than its viewport and scrolls inside it, so a roster that
/// flipped against the form's own extent would open downward off the bottom of
/// the visible pane for every row but the last few. Derived from the two facts
/// that make the viewport — where the body has been scrolled to, and how tall
/// it is — so a reader who scrolls gets the direction their screen justifies.
fn inspector_room(state: &LabState) -> Rect {
    let body = side_panel_content(inspector_rect());
    let (ox, oy) = state.inspector_scroll.offset();
    #[allow(
        clippy::cast_sign_loss,
        reason = "a scroll offset is never negative; the type is signed for the shift"
    )]
    Rect::new(ox.max(0) as u32, oy.max(0) as u32, body.w, body.h)
}

/// The same geometry in WINDOW coordinates — where a pointer meets it and
/// where assistive technology is told it is.
///
/// ★ R1662 — derived from the pane-local one by the pane's placement and its
/// current scroll offset, through the one translation
/// [`FormGeometry::translated`] owns. Computing it a second time here is how
/// the paint and the gesture come to disagree, and a row the scroll has carried
/// off the top is dropped rather than reported at the edge.
/// ★★★ R1684 — the translation is [`in_body`]'s, not a second copy of it.
///
/// **Measured while giving the text field a form row to stand on, and it was
/// wrong by [`PANEL_FRAME`].** This function wrote `pane.origin - offset` while
/// every other rectangle in the pane goes through `in_body`, which is
/// `pane.origin + PANEL_FRAME - offset` — the body is drawn INSIDE the panel's
/// outline, and `scroll_pane` is handed [`panel_content`] precisely so it is.
/// So the form's rows were reported one pixel up and one pixel left of where
/// they are painted, to the hit test and to assistive technology both.
///
/// One pixel, and it survived because nothing could see it: a press aimed at
/// the centre of a 200-wide control lands in the right row whether or not the
/// rectangle is a pixel out, which is the same reason R1682 found the identical
/// term missing from the node's-life seats and needed a 90-wide seat to say so.
/// The repair is not to add the term here — it is to stop having two
/// translations, which is what [[debt-paint-and-gesture-read-two-facts]] is
/// about.
fn inspector_geometry(state: &LabState) -> FormGeometry {
    let (dx, dy) = body_origin(state);
    inspector_geometry_local(state).translated(dx, dy)
}

// ── Paint helpers ───────────────────────────────────────────────────────────

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

/// A text run at an exact rectangle inside its container.
///
/// ★★ **The layout style is load-bearing and its absence is the defect R1653
/// found.** A [`TextNode`] carries a `rect`, which reads like a position and is
/// not one: with no [`LayoutStyle`] the engine treats the run as a flow child
/// and stacks it under its predecessor, so a screen that computes every
/// rectangle correctly still paints its text in a column down the left edge of
/// whatever contains it. `hello-analyzer-shell` learned this at R1649 and this
/// example was written afterwards without it — every label here flowed, all of
/// the canvas's card text landed in one stripe, and six rounds of gates passed
/// anyway because a run carries no tag and every gate was tag-keyed.
fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, run_style(px, fg)).with_layout(absolute(rect)))
}

/// A run that can be addressed — the lifted
/// [`text_run`], which is where the
/// rectangle-used-twice and pointer-transparency decisions now live (R1694,
/// the third identical copy).
fn tagged_label(tag: &str, text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    text_run(tag, text, rect, run_style(px, fg))
}

/// ★★★★★ R1812 — the same run, built from a [`caption::Placed`] so it carries
/// the alignment the placement was made under.
///
/// [`tagged_label`] takes a rectangle, which is all a *painter* needs and half
/// of what a *reader* needs. A caption derived through `caption::place` has an
/// author who said where it should sit; passing only `placed.run()` throws that
/// away, and the scene then declares the framework default on a caption that was
/// explicitly centred. `caption::Survey` counts such a run as saying nothing,
/// which is exactly what it was doing.
fn placed_label(
    tag: &str,
    text: impl Into<String>,
    placed: caption::Placed,
    px: u32,
    fg: Color,
) -> Scene {
    text_run(
        tag,
        text,
        placed.run(),
        run_style(px, fg).with_align(placed.declares()),
    )
}

/// The style every run on this screen carries.
///
/// ★ R1654 — including an overflow policy, because the box is exact and the
/// content is not: an endpoint, a key expression and a node identifier are all
/// user data, and a run wider than the box it was given wraps to a second line
/// that lands on the row below. Two rounds of this screen shipped that smear —
/// R1653 gave every run its exact box and could not see the overflow, because
/// its check measures boxes and this is about glyphs.
///
/// `Ellipsis` rather than `Clip` for the same reason a person reads it: a hard
/// cut leaves no evidence that anything was removed, so `tcp/0.0.0.0:744` and
/// `tcp/0.0.0.0:7447` are indistinguishable on screen.
fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

/// A run whose content is a PATH, where the tail is what distinguishes one from
/// another — so the middle gives way rather than the end.
fn path_style(px: u32, fg: Color) -> TextStyle {
    run_style(px, fg).with_overflow(TextOverflow::EllipsisMiddle)
}

/// A value cell on a node card: the right-hand column of a digest row.
fn value_label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, path_style(px, fg)).with_layout(absolute(rect)))
}

/// The width of the outline [`panel`] and [`box_at`] stroke INSIDE their box.
///
/// Named so [`panel_content`] and the border below are one number: a content
/// inset that remembers the frame's width separately from the frame is an
/// inset that goes wrong the day the frame changes.
const PANEL_FRAME: u32 = 1;

/// A bordered panel's CONTENT rectangle in its own space: its box less the
/// [`PANEL_FRAME`] outline [`panel`] draws inside it.
///
/// ★ R1672 — the placement half of
/// [`pinion_core::containment::content_rect`], which is the check half. A pane
/// that handed its scrolling body `(0, 0, rect.w, rect.h)` put the body over
/// its own outline, and the channel could not say so until it learned the
/// border-box / content-box distinction. Named here so the two halves cannot
/// drift: change the frame's width and both follow.
fn panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
        // A plain panel reserves no band of itself: it draws a frame and gives
        // everything inside it away. R1674 made this an argument rather than a
        // default so a panel that GROWS a header has to come back here.
        &[],
    )
}

fn panel(tag: &str, rect: Rect, fill: Color, border: Option<Color>, children: Vec<Scene>) -> Scene {
    let mut style = BoxStyle::filled(fill);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, PANEL_FRAME));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

/// ★★★ R1691 — declare a painted region deliberately voiceless, **at the site
/// that paints it**.
///
/// The alternative was one table beside the screen listing the quiet tags, and
/// it is the wrong shape for the same reason every other second copy in this
/// tree is: the table would be edited by whoever noticed, not by whoever moved
/// the region, and a tag that stopped being painted would keep its entry
/// forever. Here the reason travels with the node, so deleting the paint
/// deletes the declaration.
///
/// Fails loud on a node with no layout sidecar rather than declaring nothing: a
/// silence that quietly did not attach would read as an undeclared region in
/// the census, and the author would be looking for a missing name instead of a
/// missing carrier.
///
/// R1693 — the mechanism moved to [`Scene::silenced`] when a second screen
/// wanted it. This is the call-shape this file already reads in, kept so the
/// several hundred call sites below say what they said.
fn quiet(scene: Scene, silence: Silence) -> Scene {
    scene.silenced(silence)
}

/// ★★★★★ R1822 — **quiet only while the node that says the word is there.**
///
/// A [`Silence::name_of`] is a REFERENCE: it says *the thing that announces
/// this is over there*. Where "over there" is a pane this screen only draws
/// when the host draws none, the reference is conditional too — and a silence
/// whose referent left the tree is worse than no silence, because the label is
/// still painted and now nothing announces it at all.
///
/// Reads the same [`draws_own_app_bar`] the paint and the layout do, so the
/// three cannot disagree about whether the bar exists.
fn quiet_while_the_app_bar_says_it(scene: Scene, silence: Silence) -> Scene {
    if draws_own_app_bar() {
        scene.silenced(silence)
    } else {
        scene
    }
}

fn box_at(tag: &str, rect: Rect, fill: Color, border: Option<Color>, radius: u32) -> Scene {
    box_holding(tag, rect, fill, border, radius, Vec::new())
}

/// The same box, **holding something** — for a box whose caption is its own
/// child rather than a run drawn beside it.
///
/// ★ R1813 — `box_at` has always built a childless container, which is why
/// every caption on this screen started life as a sibling: there was nowhere to
/// put one. `caption::inside` produces the node and this is where it goes.
fn box_holding(
    tag: &str,
    rect: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    children: Vec<Scene>,
) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, 1));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

/// A wire between two pins, drawn as the reference draws it: a horizontal-ease
/// cubic, so a link leaves a dial pin going right and arrives at an accept pin
/// coming from the left whatever the vertical distance.
fn wire(tag: &str, from: (u32, u32), to: (u32, u32), colour: Color, width: u32) -> Scene {
    dashed_wire(tag, from, to, colour, width, None)
}

/// The same wire under a dash rhythm (R1681.2).
///
/// ★★ The rhythm is the framework's, not a second one: `Stroke` has carried
/// `dash` since R1575 and the sibling screen that draws the same two layers
/// already spells a reported link `Dash::DOTTED`. This screen drew its reported
/// links in the warning colour alone and R1681 wrote down "the wire primitive
/// carries no dash pattern" — which was false, and false in the direction that
/// invents a limit instead of reaching for what is there.
fn dashed_wire(
    tag: &str,
    from: (u32, u32),
    to: (u32, u32),
    colour: Color,
    width: u32,
    dash: Option<Dash>,
) -> Scene {
    let fx = f32::from(u16::try_from(from.0).unwrap_or(u16::MAX));
    let fy = f32::from(u16::try_from(from.1).unwrap_or(u16::MAX));
    let tx = f32::from(u16::try_from(to.0).unwrap_or(u16::MAX));
    let ty = f32::from(u16::try_from(to.1).unwrap_or(u16::MAX));
    let bow = ((tx - fx).abs() * 0.5).max(24.0);
    let bounds = Rect::new(
        from.0.min(to.0),
        from.1.min(to.1),
        from.0.abs_diff(to.0).max(1),
        from.1.abs_diff(to.1).max(1),
    );
    Scene::Path(
        PathNode::new(
            bounds,
            vec![
                PathCommand::MoveTo(PathPoint::new(fx, fy)),
                PathCommand::CurveTo {
                    c1: PathPoint::new(fx + bow, fy),
                    c2: PathPoint::new(tx - bow, ty),
                    end: PathPoint::new(tx, ty),
                },
            ],
            PathStyle::stroked(match dash {
                Some(rhythm) => Stroke::new(colour, width).with_dash(rhythm),
                None => Stroke::new(colour, width),
            }),
        )
        .with_tag(tag.to_owned())
        // ★ R1655 — a wire's BOUNDING BOX is most of the canvas, and a tagged
        // node that is not transparent is what the §5.35 router resolves as the
        // hit target: it looks the tag up as an `External`, finds none, and
        // forwards nothing. Measured by reverting it: with the wires opaque the
        // app never received the cursor at all (it reported 0,0 after a real
        // warp onto the canvas), which is exactly "sometimes a node presses and
        // sometimes it does not". The link stays selectable — `link_at`
        // hit-tests the CHORD in the app's own resolver, which is where a
        // wire's shape lives.
        .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    )
}

// ── The view ────────────────────────────────────────────────────────────────

fn app_bar(state: &LabState, ink: Ink) -> Scene {
    let running = state.running.get();
    panel(
        "lab.appbar",
        Rect::new(0, 0, window_size().0, APP_BAR_H),
        ink.surface,
        Some(ink.outline),
        vec![
            label("node lab", Rect::new(16, 19, 90, 16), FONT_TITLE, ink.text),
            quiet(
                tagged_label(
                    "lab.appbar.graph",
                    spec::GRAPH_NAME,
                    Rect::new(118, 20, 200, 14),
                    FONT_SMALL,
                    ink.text_2,
                ),
                Silence::name_of("lab.appbar"),
            ),
            tagged_label(
                "lab.appbar.state",
                if running { "running" } else { "stopped" },
                Rect::new(window_size().0 - 120, 20, 100, 14),
                FONT_SMALL,
                if running { ink.ok } else { ink.text_3 },
            ),
        ],
    )
}

fn rail(ink: Ink) -> Scene {
    let rect = rail_rect();
    // ★ R1651.1 — LOCAL coordinates. These seats were painted at their window
    // rectangles inside a container already placed at `rail_rect()`, so every
    // one of them drew a pane-height below where the hit test looks and a press
    // on `packets` answered `keys`. R1648's double-offset defect, again, in the
    // one pane R1651's sweep did not probe.
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children = vec![];
    for (n, (name, reserved_for)) in spec::RAIL.iter().enumerate() {
        let seat = local(rail_seat(n));
        let active = *name == spec::RAIL_ACTIVE;
        let mut box_node = box_at(
            &format!("lab.rail.{name}"),
            seat,
            if active { ink.accent_soft } else { ink.surface },
            Some(if active { ink.accent_line } else { ink.surface }),
            10,
        );
        // ★ R1669 — a reserved seat is DECLARED unavailable with its booking,
        // not merely drawn in a dimmer ink. The declaration is what makes it
        // inert to the pointer, fades it, announces the reason to a screen
        // reader and puts it on `scene/disabled`; the dim ink did one of those
        // four and nothing could check it.
        if let Some(why) = reserved_for
            && let Some(layout) = box_node.layout_style_mut()
        {
            *layout = layout.clone().with_unavailable(Unavailable::reserved(*why));
        }
        children.push(box_node);
        children.extend(rail_icon(
            name,
            seat,
            if active {
                ink.accent
            } else if reserved_for.is_some() {
                ink.text_3
            } else {
                ink.text_2
            },
        ));
    }
    panel("lab.rail", rect, ink.surface, Some(ink.outline), children)
}

/// A rail seat's icon, drawn as marks rather than written as a character.
///
/// ★ R1654 — these were a `\u{2022}` and a `\u{00B7}` in a 12px face, which is
/// a dot and a smaller dot: seven destinations that a reader cannot tell apart,
/// reported from the running window as "the icons are not visible". A glyph
/// font is not an option here (vendoring one is forbidden in this tree), so the
/// marks are composed from the primitives the scene already has, and each one
/// says what its destination IS: a board of tiles, a stack of messages, a key,
/// lines of a log, two joined nodes, a hub with spokes, a session's two panes.
fn rail_icon(name: &str, seat: Rect, ink: Color) -> Vec<Scene> {
    let (ox, oy) = (seat.x + 11, seat.y + 11);
    let pip = |x: u32, y: u32, w: u32, h: u32| {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(BoxStyle::filled(ink).with_corner_radius(1))
                .with_layout(absolute(Rect::new(ox + x, oy + y, w, h))),
        )
    };
    let ring = |x: u32, y: u32, d: u32| {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(
                    BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                        .with_border(Border::new(ink, 1))
                        .with_corner_radius(d / 2),
                )
                .with_layout(absolute(Rect::new(ox + x, oy + y, d, d))),
        )
    };
    match name {
        // A board of tiles.
        "dashboard" => vec![
            pip(0, 0, 7, 7),
            pip(9, 0, 7, 7),
            pip(0, 9, 7, 7),
            pip(9, 9, 7, 7),
        ],
        // A stack of messages, the top one shorter because it is the newest.
        "packets" => vec![pip(0, 1, 10, 3), pip(0, 7, 16, 3), pip(0, 13, 13, 3)],
        // A key: the bow, the shaft, two teeth.
        "keys" => vec![
            ring(0, 4, 8),
            pip(8, 7, 8, 2),
            pip(12, 9, 2, 4),
            pip(15, 9, 1, 3),
        ],
        // Lines of a log, ragged as text is.
        "logs" => vec![pip(0, 1, 16, 2), pip(0, 6, 11, 2), pip(0, 11, 14, 2)],
        // Two nodes and the link between them — this screen.
        "lab" => vec![ring(0, 0, 7), ring(9, 9, 7), pip(6, 6, 5, 2)],
        // A hub with three spokes.
        "topology" => vec![
            ring(5, 5, 7),
            pip(0, 8, 5, 1),
            pip(12, 8, 4, 1),
            pip(8, 12, 1, 4),
        ],
        // Two panes of one session.
        _ => vec![pip(0, 0, 7, 16), pip(9, 0, 7, 16)],
    }
}

/// A window rectangle in the palette BODY's own frame.
///
/// ★★★★★ R1887 — the one transform every child of that pane goes through, and
/// the peer of [`in_body`] next door. It was written out three times as
/// `r - rect.origin`, which was right while the pane sat at the panel's own
/// origin plus a one-pixel frame; the header this round adds moved the pane, so
/// three spellings of one transform became three chances to move two of them.
fn in_palette_body(r: Rect) -> Rect {
    let (x, y) = palette_body_origin();
    Rect::new(r.x.saturating_sub(x), r.y.saturating_sub(y), r.w, r.h)
}

/// ★★★★★ R1909 — the panel FIRST, its body inside the closure, exactly as
/// [`inspector`] does.
///
/// The twin of the split next door, and it is here because a repair adopted
/// once is a proposal and adopted twice is a demonstration. This function used
/// to build the whole pane — headings, eight role cards, a legend — and hand it
/// to [`side_panel`], which threw it away when the palette was folded. What
/// kept that from being a defect rather than merely waste was a `folded` check
/// at this function's own CALL SITE: a rule, written in one place, that the
/// inspector's caller did not follow and that nothing made either of them
/// follow.
///
/// Taking the body as a closure retires the rule for both panels: a folded
/// panel does not build its body because there is no path on which the closure
/// is called. That is what makes [`palette_body_origin`] safe to panic on a
/// folded panel — which is the strongest statement available that a body is not
/// there, and one no caller can forget to check.
fn palette(state: &LabState, ink: Ink) -> Scene {
    let rect = palette_rect();
    side_panel(SidePanel::Palette, state, rect, &ink, || {
        palette_body(state, ink, rect)
    })
}

/// What an OPEN palette holds. Called only through [`side_panel`]'s closure, so
/// nothing here has to ask whether the panel is folded.
fn palette_body(state: &LabState, ink: Ink, rect: Rect) -> Scene {
    let local = in_palette_body;

    // ★ R1874 — the pane's usable width, named once. The three headings and the
    // two header lines each carried their own hand-picked clipping width (180,
    // 200, 120, 140, 160) and none of them was related to the pane; every one
    // is now the room the pane actually has, which only ever delays a clip.
    // ⚠ R1887.2 — [`palette_body_w`], not the OPENING width. The two are equal
    // today and are two derivations of one number, which is the arrangement
    // this round's own debt (`a panel's extent is a value nobody can drag`) says
    // becomes false the moment somebody builds the width drag. Written as the
    // derivation now, so that round has one fewer place to remember.
    let body_w = palette_body_w();
    // ★★★★★ R1874 — the title and its subtitle are TWO LINES IN ONE SEAT, and
    // the seat is the room above the first group heading rather than a number.
    // They were `Rect::new(PAD, 14, 180, 16)` and `Rect::new(PAD, 34, 200, 12)`
    // beside 13px and 10px faces wanting 21 and 17 — both short, and their
    // relation (a `+20` between two hand-picked tops) was exactly the shape
    // R1873 measured putting a column heading one pixel below its own cells.
    // ★ R1887 — measured from the BODY's origin, which is where this pane's
    // children are stated from now, and not from the panel's: the panel keeps a
    // header band of its own above the body, and subtracting the panel origin
    // would give this strip that band's height as well as its own.
    let header = Rect::new(
        PAD,
        0,
        body_w,
        (palette_row(0).y - palette_body_origin().1).saturating_sub(PAL_HEAD_H),
    );
    let [title_band, blurb_band] =
        pinion_core::containment::stacked_line_rects(header, PAD, body_w, [FONT_BODY + 1, 10]);
    let mut children = vec![
        label(spec::PANES[1].title, title_band, FONT_BODY + 1, ink.text),
        label("click to add one at the centre", blurb_band, 10, ink.text_3),
    ];

    for (group_n, group) in ["infrastructure", "traffic"].into_iter().enumerate() {
        let head = palette_row(group_n * 4);
        children.push(palette_heading(
            group,
            head.y - palette_body_origin().1 - PAL_HEAD_H,
            body_w,
            ink,
        ));
    }
    for (n, role) in Role::ALL.into_iter().enumerate() {
        let row = local(palette_row(n));
        children.push(box_at(
            &format!("lab.palette.role.{}", role.name()),
            row,
            ink.raised,
            Some(ink.outline),
            8,
        ));
        // ★ R1874 — the card's inside, which is what the swatch and the words
        // both sit in. It was the literal `6`/`-12` here and nothing said it was
        // the same clearance the row's height is built from.
        let inside = Rect::new(
            row.x,
            row.y + PAL_ROW_INSET,
            row.w,
            row.h.saturating_sub(PAL_ROW_INSET * 2),
        );
        children.push(quiet(
            box_at(
                &format!("lab.palette.swatch.{}", role.name()),
                Rect::new(row.x + 9, inside.y, 3, inside.h),
                role_ink(role),
                None,
                2,
            ),
            Silence::decorative("a colour band keying this role to its wires"),
        ));
        // ★★★★★ R1874 — the role's name over its gist, stacked in the card's
        // INSIDE rather than in the card. `+6` with a 14px box and `+20` with a
        // 12px box was two offsets and two heights that nothing related to the
        // faces (12px wants 20, 10px wants 17) or to each other; a `PAL_ROW_H`
        // change moved neither and a face change moved neither.
        //
        // ⚠ The seat is the inside and NOT the row, which the containment gate
        // is what taught: stacked in the row, the two lines cleared the card's
        // own border by nothing and were reported 1px past it at top and bottom
        // on all eight rows in every state. A card with a border and a corner
        // radius is not the same rectangle as the space inside it.
        let [name_band, gist_band] = pinion_core::containment::stacked_line_rects(
            inside,
            inside.x + 20,
            inside.w.saturating_sub(20 + PAL_ROW_INSET),
            [FONT_SMALL + 1, 10],
        );
        children.push(label(role.name(), name_band, FONT_SMALL + 1, ink.text));
        children.push(label(role.gist(), gist_band, 10, ink.text_3));
    }

    children.extend(palette_legend(ink));
    children.extend(palette_determinism(state, ink));
    // ★ R1662 — the pane scrolls. Its content is taller than any window this
    // screen declares a floor for, and before this the overflow was simply
    // painted past the bottom edge: `scene/scroll_reach` reported the last
    // rows `lost`, meaning no gesture of any kind reached them. The extent is
    // derived from `children` by the pane rather than declared here, so it
    // cannot go stale as rows are added
    // ([[debt-the-node-lab-panes-do-not-scroll]]).
    quiet(
        scroll_pane(
            &state.palette_scroll,
            side_panel_content(rect),
            (0, PAD),
            // Every press on this screen belongs to the one root `External`
            // that does the screen's own hit test, so the pane must be
            // invisible to the router (R1655).
            PanePointer::PassesThrough,
            children,
        ),
        Silence::layout("scrolls the palette; the pane above it is what a reader lands on"),
    )
}

/// ★★★★★ R1887 — **a movable panel, with the header that makes it movable by a
/// person and the strip that makes a fold reversible by one.**
///
/// # Why the header exists at all
///
/// Before this round the two side panels had a placement that was a value, a
/// layout that honoured it, and gates over both — and **nothing that could
/// change it**. Measured at entry: outside `tests.rs`, `palette_at` and
/// `inspector_at` had no writer in the tree. A reader asked three times across
/// eleven rounds why these panels cannot be moved, and the honest answer had
/// become *they can, and nobody can*.
///
/// # Why folding paints a strip rather than a narrow panel
///
/// ⚠ The entry re-measurement sharpened this debt rather than confirming it.
/// The record said the strip was drawn and had no control in it; measured, the
/// fold was **geometric only** — a folded panel kept its whole body and painted
/// it into eighteen pixels, because nothing branched on `folded` in the paint
/// at all. So the branch is here: a folded panel is a strip and the strip is
/// one affordance, which is what makes a fold reversible by the person who did
/// it. The floor has no fold at all — its nearest gesture removes the panel
/// from the layout, leaving nothing to press to bring it back.
/// ★★★★★ R1909 — **`body` arrives UNBUILT**, and that is what makes the fold
/// safe rather than a rule every caller has to remember.
///
/// It used to be a `Scene`, so a caller built the whole panel body and this
/// function then threw it away when the panel was folded. That is waste while
/// the body is merely expensive, and a DEFECT the moment a body's geometry is
/// derived from the panel's live width: a folded inspector's content width is
/// zero, and one row's `inspector_body_w() - 20` overflowed — measured at
/// R1909, the round that first made a pane of this screen open folded.
///
/// Taking the body as a closure means a folded panel does not build its body
/// because there is no path on which the closure is called. ⇒ *a rule nobody
/// can forget is one that cannot be written down.* Both panels go through it,
/// which is what lets [`palette_body_w`] and [`inspector_body_w`] panic rather
/// than saturate: reaching them under a fold is now unreachable rather than
/// merely discouraged.
fn side_panel(
    which: SidePanel,
    state: &LabState,
    rect: Rect,
    ink: &Ink,
    body: impl FnOnce() -> Scene,
) -> Scene {
    let at = which.at(state);
    if at.folded {
        // The whole strip is the affordance: eighteen pixels is not room for a
        // control inside a panel, and a strip whose only job is to come back
        // does not need one.
        //
        // ⚠ R1887.1 — `panel_content` and not the panel's own rectangle. Drawn
        // at `(0, 0, w, h)` the strip overhung its own panel's border by a
        // pixel on every side, which the containment gate reported the first
        // time the sweep reached a folded panel. A box with a border is not the
        // same rectangle as the space inside it — the note R1874 left on the
        // palette's rows, met again one level out.
        return panel(
            which.tag(),
            rect,
            ink.raised,
            Some(ink.outline),
            vec![box_at(
                &format!("{}.strip", which.tag()),
                panel_content(rect),
                ink.raised,
                None,
                0,
            )],
        );
    }
    let head = side_panel_head(rect);
    let flip = side_panel_control(rect, 0);
    let fold = side_panel_control(rect, 1);
    // ★★★★★ R1889 — the resize band, painted only when the panel declares it
    // resizes. A grip a reader can see and cannot drag is worse than none: it
    // is an affordance that lies, and the specification is what decides.
    let mut chrome = Vec::new();
    if side_panel_has_grip(state, which) {
        chrome.push(box_at(
            &format!("{}.grip", which.tag()),
            side_panel_grip(rect, at),
            ink.outline,
            None,
            0,
        ));
    }
    // The title's box is the line box its own face needs, centred in the header
    // band — the derivation this screen uses everywhere a run sits in a seat, so
    // a face change moves the box with it (R1874's rule).
    let title_band = line_rect_in(
        head,
        head.x + PAD,
        flip.x.saturating_sub(head.x + PAD),
        FONT_SMALL,
    );
    panel(
        which.tag(),
        rect,
        ink.surface,
        Some(ink.outline),
        vec![
            quiet(
                tagged_label(
                    &format!("{}.head", which.tag()),
                    which.spec().title,
                    title_band,
                    FONT_SMALL,
                    ink.text_2,
                ),
                Silence::name_of(which.tag()),
            ),
            box_at(
                &format!("{}.flip", which.tag()),
                flip,
                ink.raised,
                Some(ink.outline),
                4,
            ),
            box_at(
                &format!("{}.fold", which.tag()),
                fold,
                ink.raised,
                Some(ink.outline),
                4,
            ),
            body(),
        ]
        .into_iter()
        .chain(chrome)
        .collect(),
    )
}

/// ★★★★★ R1874 — one heading of the palette: a line set in the `PAL_HEAD_H`
/// strip that sits above whatever it heads, centred in that strip.
///
/// Lifted because there are THREE authoring sites, which is this project's
/// threshold: the two group headings, the pin legend's `pins`, and the
/// determinism switch's caption. All three were `Rect::new(x, top, <a width>,
/// 12)` beside a 10px face wanting 17 — the same mistake written out three
/// times, so a single repair would have had to be remembered three times.
///
/// The strip is the seat, and the seat is `PAL_HEAD_H` because that is the
/// space `palette_row` and `legend_row` actually leave above themselves. So the
/// heading follows a change to that constant, which is what it could not do
/// while its box was a number.
fn palette_heading(text: &str, strip_top: u32, w: u32, ink: Ink) -> Scene {
    let strip = Rect::new(PAD, strip_top, w, PAL_HEAD_H);
    label(
        text,
        pinion_core::containment::line_rect_in(strip, strip.x, w, 10),
        10,
        ink.text_3,
    )
}

/// The pin legend and the transport chips: three appearances and what each one
/// means, next to the colours an accept pin is drawn in.
/// ⚠ R1887.2 — it took the panel's rectangle until R1887 made the body's origin
/// its own derivation, and then kept taking it with a `let _ = rect;` to keep
/// the compiler quiet. That is the shape this project has a rule about: an
/// unused thing SILENCED is a signal turned off, so the parameter is gone.
fn palette_legend(ink: Ink) -> Vec<Scene> {
    let local = in_palette_body;
    let mut children = vec![palette_heading(
        "pins",
        legend_top() - palette_body_origin().1,
        palette_body_w(),
        ink,
    )];
    for (n, (kind, meaning)) in spec::PIN_LEGEND.iter().enumerate() {
        let row = local(legend_row(n));
        let colour = match *kind {
            "dial" => ink.accent,
            "accept" => transport_ink(Transport::Tcp),
            _ => ink.text_3,
        };
        // ★★★★★ R1862 — **the sample and the words share a centre because both
        // are derived from the row**, not because two hand-picked offsets
        // happened to agree. They did not: this was `row.y + 3` for BOTH, on an
        // 11-pixel pin and a 12-pixel label in an 18-pixel row, so the pin's
        // centre sat at +8 and the label's at +9 — and a reader said the words
        // did not line up with the box beside them. The label's box was five
        // pixels short of `line_box(10)` as well, which is the same authoring
        // habit and the same repair: one call answers the height AND the
        // position, exactly as R1859 found on the inspector's rename row.
        children.push(box_at(
            &format!("lab.palette.pin.{kind}"),
            band_in(row, row.x, PIN, PIN),
            if *kind == "dial" { colour } else { ink.surface },
            Some(colour),
            PIN / 2,
        ));
        children.push(label(
            *meaning,
            line_rect_in(row, row.x + 20, 190, 10),
            10,
            if *kind == "closed" {
                ink.err
            } else {
                ink.text_2
            },
        ));
    }
    for (n, transport) in Transport::ALL.into_iter().enumerate() {
        let chip = local(protocol_chip(n));
        // ★★ R1691 — a colour key. What a reader who never sees the colours
        // loses is the MEMBERSHIP of the set, not the individual chips, so the
        // set is announced once as the pane's value and each chip says it is
        // part of that. Five nodes saying one word each would be five stops on
        // a reader's way through the palette for one fact.
        // ★★★★★ R1792 — through `captioned`, which puts the word INSIDE the
        // box. These five chips are what a reader reported as not centred, and
        // measured through the paint it was worse: the caption was placed at
        // `chip.x + 7` with a width of 32 in a box 36 wide, so **3px of every
        // one of them hung off the right edge**. The `+7` was arithmetic
        // against a rectangle the word was not inside — they were siblings, so
        // nothing in the tree related them and no gate could ask.
        let word = transport.word();
        let tag = format!("lab.palette.protocol.{word}");
        let mut style = BoxStyle::filled(ink.surface).with_corner_radius(4);
        style = style.with_border(Border::new(transport_ink(transport), 1));
        let (chip_scene, _) = captioned(
            &tag,
            chip,
            style,
            // ★★★★★ R1794 — no size. R1792 passed `(32, 12)` here because 32 was
            // the number the hand-written code before it used, and 32 was the
            // BOX rather than the text: the glyphs advance 15, so a 32-wide run
            // rectangle centred in a 36-wide chip left the ink 8.5px off centre.
            // A reader saw it; the gate could not, because the gate measured
            // rectangles. The shaper answers now.
            &caption::Caption::new(word, run_style(10, transport_ink(transport)))
                .centred()
                // ★★ R1691's decision, unchanged and now carried by the node
                // that makes it: what a reader who never sees the colours loses
                // is the MEMBERSHIP of the set, not the individual chips, so the
                // set is announced once as the pane's value and each chip says
                // it is part of that. Five nodes saying one word each would be
                // five stops on a reader's way through the palette for one fact.
                .silent(Silence::part_of("lab.palette")),
            // A colour key, not a control: a press falls through to the pane.
            caption::Pointer::Transparent,
        );
        children.push(quiet(chip_scene, Silence::part_of("lab.palette")));
    }

    children
}

/// What the determinism switch's caption says at each position — one
/// derivation, read by the paint and by the announcement it is the description
/// for.
const fn discovery_caption(on: bool) -> &'static str {
    if on {
        "discovery on · links may appear"
    } else {
        "discovery off · fully specified"
    }
}

/// The switch's position without the clause that explains it.
///
/// ★★★★★ R1794 — because [`discovery_caption`] does not fit the seat it is
/// painted in, and a reader saw it as `discovery off · fully specif…`.
///
/// The ellipsis was silent: `TextOverflow::Ellipsis` shortens whatever it is
/// given and nothing in this tree reported that it had. (R1792 made it worse by
/// narrowing the caption to win back a right margin — a repair that traded one
/// defect for another because nothing said the text had stopped fitting.)
///
/// The split rather than a shorter sentence: the clause is what the position
/// MEANS, and it is still announced — [`discovery_caption`] is the
/// `AccessNode`'s name, so a reader who lands on the switch hears the whole
/// thing. What is dropped is the painted half, and only when it does not fit.
const fn discovery_word(on: bool) -> &'static str {
    if on { "discovery on" } else { "discovery off" }
}

/// How far in from the determinism switch's left edge its caption starts —
/// clear of the track, which is 10 in and 30 wide.
const DISCOVERY_CAPTION_INSET: u32 = 48;

/// The tag the switch's caption carries, **derived rather than spelled**.
///
/// ★ R1813 — it was `lab.palette.discovery.state`, a name this file chose. The
/// caption is now the switch box's own child, built by `caption::inside`, and
/// the `.caption` suffix is what tells `caption::Survey` whose caption it is; a
/// second spelling here could drift from the framework's in an edit nothing
/// would catch. The a11y id is the same string on purpose — the switch's
/// `described_by` points at a painted region a reader can also walk onto.
fn discovery_caption_tag() -> String {
    format!("lab.palette.discovery{}", caption::CAPTION_SUFFIX)
}

/// The determinism switch, off by default.
/// ⚠ R1887.2 — the panel rectangle it used to take is gone, for the reason on
/// [`palette_legend`].
fn palette_determinism(state: &LabState, ink: Ink) -> Vec<Scene> {
    let local = in_palette_body;
    let mut children: Vec<Scene> = Vec::new();
    let toggle = local(discovery_rect());
    let on = state.discovery.get();
    children.push(palette_heading(
        "graph determinism",
        toggle.y - PAL_HEAD_H,
        palette_body_w(),
        ink,
    ));
    // ★ The caption IS the switch's description — the switch points at it with
    // `described_by`, so it is announced when a reader lands on the control
    // rather than as a separate stop beside it.
    // ★★★★★ R1794 — WHICH caption is a question about room, and it is ASKED
    // rather than assumed. The full sentence is drawn when it fits and the
    // position alone when it does not; either way `discovery_caption` is what
    // the switch ANNOUNCES, so the clause is never lost, only unpainted. Before
    // this the full sentence was drawn and silently ellipsised to
    // `discovery off · fully specif…` — a reader reported it and nothing in the
    // tree had said so.
    //
    // ★★★★★ R1792 — the room is what the BOX has left after the track, not what
    // the PANE has left. This read `PALETTE_W - toggle.x - 48 - PAD`, derived
    // against the pane, and the toggle's own right edge is already
    // `PALETTE_W - PAD` — so subtracting `PAD` once landed the caption exactly
    // flush with the box's right border. Measured off the paint: caption
    // (117, 657, 154, 13) in box (69, 647, 202, 58), **48px of gap on the left
    // and ZERO on the right**, which is what a reader sees as a word jammed
    // against the panel edge.
    //
    // ★★★★★ R1813 — and now it is not derived at all. The caption is the
    // switch's own CHILD, placed by `caption::inside`, which is what makes the
    // relation a fact the scene carries rather than a rectangle two edits could
    // drift apart: R1792 repaired the seat and left the pairing a guess, which
    // is why the round after it could still be told the caption belonged to the
    // palette. A press still reaches the switch: the run is pointer-transparent.
    //
    // The room is stated PER SIDE — 48 in from the left because the track is
    // drawn there, the panel's margin on the right.
    //
    // 🟥🟥 R1813's closing audit REFUTED THE SENTENCE THAT FIRST STOOD HERE. It
    // said a symmetric pair "would have swapped the sentence for its short
    // form", and the sentence is already its short form: measured, the caption
    // is 66px in a 202-wide box, which fits the symmetric room (106) as well as
    // the per-side one (140), so nothing about today's ink would move. What
    // per-side actually buys is those 34px of FIT BUDGET — the query above is
    // what decides whether the clause can ever be painted — and a padding
    // readback that is TRUE, where a symmetric 48 would declare 48px reserved
    // on the right of a box that leaves 88. Pinned by
    // `r1813_the_switch_paints_the_position_and_the_pair_would_not_have_moved_it`,
    // because a counterfactual argued in prose is what got this wrong.
    let caption_pad = caption::Padding::each(DISCOVERY_CAPTION_INSET, 10, PAD, 0);
    let style = run_style(FONT_SMALL, ink.text);
    let full = discovery_caption(on);
    let says = if caption::place(
        toggle,
        &caption::Caption::new(full, style.clone()).padded(caption_pad),
    )
    .fit()
    .fits()
    {
        full
    } else {
        discovery_word(on)
    };
    let (state_caption, _) = caption::inside(
        "lab.palette.discovery",
        toggle,
        &caption::Caption::new(says, style).padded(caption_pad),
    );
    children.push(box_holding(
        "lab.palette.discovery",
        toggle,
        ink.raised,
        Some(if on { ink.warn } else { ink.outline }),
        9,
        vec![state_caption],
    ));
    children.push(quiet(
        box_at(
            "lab.palette.discovery.track",
            Rect::new(toggle.x + 10, toggle.y + 12, 30, 16),
            if on { ink.warn } else { ink.outline_2 },
            None,
            8,
        ),
        Silence::decorative("the switch's track, whose position the switch announces"),
    ));
    // ★★★★★ R1889 — the pane's width, ASKED rather than the constant it opens
    // at. This is the last reader of `PALETTE_W` outside the four places that
    // legitimately want the opening number (its own definition, the floor
    // width, the toolbar's floor, and the opening placement), and it was found
    // by the closing measurement of this round's own debt rather than by the
    // round that built the drag — which is the whole shape that debt describes:
    // *a latent divergence is a defect whose date is the round that builds the
    // missing thing*. Today's arithmetic is preserved exactly, because at the
    // opening width `palette_rect().w` IS `PALETTE_W`; what changes is that the
    // sentence now follows a hand that widens the palette instead of staying at
    // the width it was born with.
    children.push(label(
        "turning it on lets nodes acquire links nobody authored",
        Rect::new(
            toggle.x + 48,
            toggle.y + 28,
            palette_rect()
                .w
                .saturating_sub(toggle.x)
                .saturating_sub(48 + PAD),
            24,
        ),
        9,
        ink.text_2,
    ));

    children
}

fn toolbar(state: &LabState, ink: Ink) -> Scene {
    /// The clearance a word keeps inside the launch chip, left and right —
    /// wider than [`GATE_PAD`], because a word set flush against a vertical
    /// rule reads as touching it long before it overlaps.
    const GATE_TEXT_PAD: u32 = 9;
    let bar = toolbar_rect();
    let local = |r: Rect| Rect::new(r.x - bar.x, r.y - bar.y, r.w, r.h);
    let verdict = state.verdict();
    let nodes = state.cards().len();
    let links = state.link_count();

    let gate_word = if verdict.may_launch() {
        "gate passed"
    } else {
        "gate blocked"
    };
    let gate_colour = if verdict.may_launch() {
        ink.ok
    } else {
        ink.err
    };

    // ★ R1653 — each label's box ends where the next one begins. A text node's
    // rectangle is not a hint, it is the box the run is wrapped into: two boxes
    // that overlap paint one string over another as soon as either string grows
    // to fill the space it was promised, and the strings here are a graph name
    // and a count. ★ R1688 — from the same three functions the hit test and the
    // width gate read, rather than written out here.
    let mut children = vec![
        quiet_while_the_app_bar_says_it(
            tagged_label(
                "lab.toolbar.title",
                spec::GRAPH_NAME,
                local(toolbar_title_rect()),
                FONT_TITLE,
                ink.text,
            ),
            // The graph's name, painted twice on this screen. The application
            // bar is where a reader is told it; a second stop saying the same
            // word is a second stop saying nothing.
            //
            // ★★★★★ R1822 — **which stops being true the moment the bar is not
            // there.** A silence is a reference to the node that DOES say the
            // word, so where the host draws the application bar and this screen
            // draws none, this silence points at a node that is not in the tree
            // and the graph's name is announced by nobody. Mounted, this label
            // IS the stop that says it, so it is not quiet at all.
            //
            // ⇒ the same predicate the paint and the layout read. A screen that
            // decided this separately would be the two-readers defect R1821
            // measured, in the one layer where nobody looks at the pixels.
            Silence::name_of("lab.appbar"),
        ),
        tagged_label(
            "lab.toolbar.meta",
            format!("{nodes} nodes · {links} links"),
            local(toolbar_meta_rect()),
            FONT_SMALL,
            ink.text_3,
        ),
        // The word lives INSIDE the chip rather than beside it, so it is the
        // chip's own content and cannot drift out of it.
        //
        // ★★ R1672 — and the chip's height and the word's seat are both DERIVED
        // from the line box and the frame. They were a picked `22` and a picked
        // `y = 4`, which happened to put the word's last row on the chip's own
        // outline: exactly one pixel, on the bottom edge, invisible until
        // `containment` learned that a border is ink the box owns. The numbers
        // below come out at the same 22 — the pixels do not move, the way they
        // are arrived at does.
        {
            let line = line_box(FONT_SMALL);
            let seat = local(gate_chip_rect());
            let inner = panel_content(seat);
            panel(
                "lab.toolbar.gate",
                seat,
                ink.raised,
                Some(gate_colour),
                vec![label(
                    gate_word,
                    // ★ R1656 — the LINE box of the face, not the face's size.
                    Rect::new(
                        inner.x + GATE_TEXT_PAD,
                        inner.y + GATE_PAD,
                        inner.w.saturating_sub(GATE_TEXT_PAD * 2),
                        line,
                    ),
                    FONT_SMALL,
                    gate_colour,
                )],
            )
        },
    ];

    children.extend(toolbar_controls(state, ink));
    panel("lab.toolbar", bar, ink.surface, Some(ink.outline), children)
}

/// The toolbar's right-hand cluster: zoom, the configuration read-out, and the
/// run control the gate governs.
fn toolbar_controls(state: &LabState, ink: Ink) -> Vec<Scene> {
    let bar = toolbar_rect();
    let local = |r: Rect| Rect::new(r.x - bar.x, r.y - bar.y, r.w, r.h);
    let mut children: Vec<Scene> = Vec::new();
    // ★★★★★ R1791 — paint the groups that are ON the row. A moved group's seats
    // have no place, and painting them anyway would put them at the row's right
    // edge on top of the run seat, which is the clip this round exists to end
    // wearing a different coat.
    let laid = right_cluster();
    let showing = |group: ToolGroup| laid.shown().contains(&group);
    children.extend(toolbar_overflow(state, bar, ink));
    for plus in [false, true] {
        if !showing(ToolGroup::Zoom) {
            break;
        }
        let seat = local(zoom_rect(plus));
        children.push(box_at(
            if plus {
                "lab.toolbar.zoom.in"
            } else {
                "lab.toolbar.zoom.out"
            },
            seat,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label(
            if plus { "+" } else { "-" },
            Rect::new(seat.x + 9, seat.y + 5, 12, 14),
            FONT_BODY,
            // ★★ R1688 — the pill's three BUTTONS are one weight and the
            // read-out is the emphasised one, which is the reference's own
            // relation. Found by looking: the fit seat arrived at `text_3` (the
            // weight the old `home` seat had) beside two steppers at `text`, so
            // one pill showed three controls in two states and the new one read
            // as disabled.
            ink.text_2,
        ));
    }
    // ★★★ R1688 — the read-out IS the view reset, which is what the reference
    // makes it. Unconditional, in the zoom cluster; see `ResetScope::gated`.
    // The percentage is this seat's own caption, so it is inside the control it
    // names by construction rather than by a constant that has to be kept in
    // step — R1687 had to move that constant by 4 px to stop the number being
    // painted inside the `+` button beside it.
    let view_reset = local(view_reset_rect());
    if showing(ToolGroup::Zoom) {
        children.push(box_at(
            "lab.reset.view",
            view_reset,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        // ★ The percentage is painted INSIDE the reset seat, and that seat's name
        // already carries it ("zoom 84%, reset the view") — one stop, both facts.
        children.push(quiet(
            tagged_label(
                "lab.toolbar.zoom",
                format!("{}%", state.zoom.get()),
                seat_caption(view_reset),
                FONT_SMALL,
                ink.text,
            ),
            Silence::name_of("lab.reset.view"),
        ));
        // ★★ R1688 — the pill's trailing seat: frame the whole graph.
        let fit = local(fit_rect());
        children.push(box_at(
            "lab.toolbar.fit",
            fit,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label("fit", seat_caption(fit), FONT_SMALL, ink.text_2));
    }

    // ★★ R1687 — the pair the reference puts side by side, because they are one
    // derivation rendered two ways. Painted from one loop so a change to either
    // seat's look cannot land on only one of them — and R1791 moves them as ONE
    // group for the same reason: an overflow that took `script` and left
    // `config` would undo a decision somebody wrote down.
    if showing(ToolGroup::Export) {
        for (tag, text, seat) in [
            ("lab.toolbar.config", "config", local(config_rect())),
            ("lab.toolbar.script", "script", local(script_rect())),
        ] {
            children.push(box_at(tag, seat, ink.raised, Some(ink.outline), 7));
            children.push(label(text, seat_caption(seat), FONT_SMALL, ink.text_2));
        }
    }

    // ★★ R1689 — the file pill: three seats sharing one background, which is
    // what makes them read as one subject. `clear` is the quieter weight
    // because it is the destructive one and the reference gives it the same
    // treatment — the emphasis a control carries is part of what it says.
    for (n, (word, _)) in FILE_SEATS.iter().enumerate() {
        if !showing(ToolGroup::File) {
            break;
        }
        let seat = local(file_rect(n));
        children.push(box_at(
            &format!("lab.toolbar.{word}"),
            seat,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label(
            *word,
            seat_caption(seat),
            FONT_SMALL,
            if *word == "clear" {
                ink.text_3
            } else {
                ink.text_2
            },
        ));
    }

    children.extend(toolbar_run_seat(state, local(run_rect()), ink));
    children
}

/// ★★★★★ R1791 — the overflow control, **which says what it is holding**.
///
/// The floor, measured at 6.11: a tool bar squeezed to a fifth of its width
/// shows 1 of 10 actions and puts 9 behind an extension button — and there is
/// no member that names them, while each hidden action's own `isVisible()`
/// still answers true. So a reader asking *what can this toolbar do right now*
/// is told about controls a person cannot see, and cannot be told about the
/// ones behind the button.
///
/// Here the name IS the list. A person reading it hears which groups moved, and
/// the same fact is on the wire at `toolbar_overflow`, so neither channel has to
/// infer it from what is missing.
/// ★★★★★ R1791 — the overflow control, and the menu it opens onto.
///
/// The menu's seats keep their own tags, so a press aimed at
/// `lab.toolbar.config` still lands on the configuration export: it is
/// somewhere else, not something else.
fn toolbar_overflow(state: &LabState, bar: Rect, ink: Ink) -> Vec<Scene> {
    let local = |r: Rect| Rect::new(r.x - bar.x, r.y - bar.y, r.w, r.h);
    let Some(control) = overflow_rect() else {
        return Vec::new();
    };
    let mut children = toolbar_overflow_seat(local(control), ink);
    if state.toolbar_open.get() {
        for (tag, rect) in overflow_menu_seats() {
            let seat = local(rect);
            children.push(box_at(tag, seat, ink.raised, Some(ink.outline), 6));
            children.push(label(
                tag.rsplit('.').next().unwrap_or(tag),
                seat_caption(seat),
                FONT_SMALL,
                ink.text_2,
            ));
        }
    }
    children
}

fn toolbar_overflow_seat(seat: Rect, ink: Ink) -> Vec<Scene> {
    vec![
        box_at("lab.toolbar.more", seat, ink.raised, Some(ink.outline), 6),
        // ★ The glyph is the caption and the SEAT carries the list — see
        // `toolbar_seats`, where this control's name is built from what the
        // row actually moved. Announcing it here as well would say it twice.
        quiet(
            tagged_label(
                "lab.toolbar.more.label",
                "\u{2026}".to_owned(),
                seat_caption(seat),
                FONT_SMALL,
                ink.text_2,
            ),
            Silence::name_of("lab.toolbar.more"),
        ),
    ]
}

/// The launch control: the chip, and the word that says what pressing it will
/// do right now.
fn toolbar_run_seat(state: &LabState, run: Rect, ink: Ink) -> Vec<Scene> {
    let verdict = state.verdict();
    let running = state.running.get();
    let nodes = state.cards().len();
    let run_ink = if !verdict.may_launch() {
        ink.text_3
    } else if running {
        ink.ok
    } else {
        ink.accent
    };
    vec![
        box_at("lab.toolbar.run", run, ink.raised, Some(run_ink), 7),
        // ★ The caption IS the seat's name — `toolbar_seats` builds both from
        // the same two facts, so announcing it here would say "run blocked"
        // twice.
        quiet(
            tagged_label(
                "lab.toolbar.run.label",
                if running {
                    format!("running {nodes}/{nodes}")
                } else if verdict.may_launch() {
                    "run".to_string()
                } else {
                    "run blocked".to_string()
                },
                seat_caption(run),
                FONT_SMALL,
                run_ink,
            ),
            Silence::name_of("lab.toolbar.run"),
        ),
    ]
}

fn canvas_world(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();

    // ★ R1705 — no grid here any more: the pips were a texture on this surface
    // and are now a `DotLattice` on the viewport above it, which is what makes
    // them endless and free. See `canvas_lattice`.
    let dragged_frame = match state.drag.get() {
        Some(Drag::Frame { frame, .. }) => Some(frame),
        _ => None,
    };
    for (id, name) in frames_of(state) {
        let box_rect = frame_rect_of(state, id);
        let gist = spec::FRAMES
            .iter()
            .find(|f| f.name == name)
            .map_or("", |f| f.gist);
        // The tab is the frame's handle: the interior belongs to the cards, so
        // a group can be moved without a press inside it stealing a node drag.
        //
        // ★★★★★ R1812 derived this caption from the frame's own rectangle
        // instead of computing it beside one — it was `Rect::new(x + 12, y + 3,
        // (w - 24).max(40), 13)`, and that `.max(40)` is a width floor that does
        // not know how wide the box is, so at a frame 44 wide it put a 40px run
        // 12px in and **8px outside the frame**.
        //
        // ★★★★★ R1813 makes it the frame's own CHILD, which is the half R1812
        // could not reach: `captioned` builds the box, and this box already
        // exists and carries a border, a drag handle and a hover state, so the
        // caption stayed a sibling and stayed a guess to every check that reads
        // the paint. `caption::inside` is the node for a box its caller builds,
        // and it writes the `.caption` tag that says whose caption it is.
        //
        // A press still reaches the frame: `caption::inside` builds through
        // `text_run`, which is pointer-transparent, so the tab drags the group
        // exactly as it did when the caption was drawn beside it.
        let cap = caption::Caption::new(frame_caption(&name, gist), run_style(10, ink.text_3))
            .padding(12, 3)
            .silent(Silence::name_of(format!("lab.frame.{name}")));
        let (title, _) = caption::inside(&format!("lab.frame.{name}"), box_rect, &cap);
        children.push(box_holding(
            &format!("lab.frame.{name}"),
            box_rect,
            Color::rgba(0x16, 0x18, 0x1D, 0x6b),
            Some(if dragged_frame == Some(id) {
                ink.accent
            } else {
                ink.outline_2
            }),
            12,
            vec![title],
        ));
    }
    children.extend(canvas_wires(state, ink));
    children.extend(canvas_cards(state, ink));
    // ★★★ R1681.3 — the picked link's affordances paint LAST, because they are
    // what a press over them reaches.
    //
    // They were painted with the wires, which is where they belong visually and
    // is wrong: a card drawn afterwards covers them, while `Hit::at` tests them
    // FIRST. So the screen showed a card and answered `link:act` for the same
    // pixel — the paint and the hit test disagreeing about which thing is on
    // top, which is this screen's oldest defect class and the reason R1656
    // exists. Found by looking at the running app, not by any check here.
    if let Some(chrome) = link_chrome(state) {
        children.extend(link_affordances(&chrome, ink));
    }
    children
}

/// The reference's dot grid: a pip every 22 canvas units, moving with the pan
/// so the canvas reads as a surface being moved rather than a viewport sliding
/// over a static picture.
/// How big the world surface is, at this zoom and this window.
///
/// ★ R1656 — ONE derivation, because two of them disagreed. The surface sized
/// itself here while the pip texture on it ran to the VIEWPORT's width, and at
/// the minimum zoom the world is narrower than the viewport — so the texture
/// marched thousands of dots past the edge of the thing it was decorating. The
/// containment check reported 11,472 of them the first time the size axis
/// visited "zoomed out, maximised"; before that nothing looked, because a pip
/// carries no tag and every other gate here is tag-keyed.
fn world_extent(state: &LabState) -> (u32, u32) {
    let rect = canvas_rect();
    (
        scaled(state, WORLD.unsigned_abs()).max(rect.w),
        scaled(state, WORLD.unsigned_abs()).max(rect.h),
    )
}

/// ★★★★★ R1705 — the reference's dot grid, as ONE node.
///
/// This function used to emit one `Scene::Container` per pip and cut the walk
/// to the world surface's extent, because the framework had no repeating fill
/// and a consumer cannot invent one. Both of the things a person reported about
/// this canvas came from that, and they were the same fact:
///
/// * **"zooming out is slow."** The pitch shrinks with the zoom (down to a 6 px
///   floor), so zooming out packed more pips into the viewport: the painted
///   scene went from 12,879 nodes to **95,131** and a zoom step from 23 ms to
///   **155 ms**. Every one of those pips was laid out, hit-tested, cached and
///   published, for a 1 px dot.
/// * **"the dots stop, so it doesn't feel infinite."** An enumerated lattice
///   has to stop somewhere, and the `.min(world_w)` above is where — pan past
///   the 6,400-unit surface and the canvas went blank.
///
/// [`DotLattice`] answers both by construction. The lattice is a declaration on
/// the canvas's own box, so it is bounded by the box (never by a world extent),
/// and the dots are emitted at paint time rather than materialised. The phase
/// is the world offset, which is what makes the grid travel with the surface
/// instead of the window — the same parameterisation the behaviour canon uses
/// (`background-size` from the zoom, `background-position` from the pan).
fn canvas_lattice(state: &LabState, ink: Ink) -> DotLattice {
    let pitch = scaled(state, 22).max(6);
    // The pan, not the world offset: the lattice sits on the viewport, so its
    // phase is how far the surface has been dragged under it.
    let (pan_x, pan_y) = state.pan.get();
    DotLattice::new(pitch, 1, ink.grid).phased(pan_x, pan_y)
}

/// Where the picked link's own affordances sit, in the world surface's
/// coordinates (R1681).
///
/// ★ ONE authority, read by the painter and by the hit test. R1653 found three
/// consecutive rounds of defects that hid because a control's paint and its
/// press were two arithmetics that agreed until they did not; every rectangle
/// on this screen that a person aims at comes from a single derivation for that
/// reason, and these three are no different.
struct LinkChrome {
    /// The type size every part of it is drawn at, and therefore the size every
    /// part of it is derived FROM.
    font: u32,
    /// The endpoint caption at the wire's middle.
    label: Rect,
    /// The word the label carries.
    caption: String,
    /// One seat per endpoint the target listens on — **empty unless there is
    /// more than one**, because a choice between one thing is not a choice, and
    /// the reference draws the row only when the target listens twice.
    chips: Vec<(String, Rect)>,
    /// Which of those the link took.
    current: usize,
    /// The delete-or-adopt seat below the wire.
    act: Rect,
    /// Whether that seat adopts (a reported link) rather than deletes (a drawn
    /// one). The reference's one button with two meanings, and it is one button
    /// because the two are the same question — *should this link be in the
    /// drawing* — answered from opposite sides.
    adopt: bool,
}

/// The chrome of whichever link is picked, or `None` when none is.
fn link_chrome(state: &LabState) -> Option<LinkChrome> {
    let pick = state.selected_link.get()?;
    let (from_socket, to_socket, adopt) = match pick {
        LinkPick::Authored(id) => {
            let link = state.doc.borrow().tree(ROOT)?.link(id).copied()?;
            (link.from, link.to, false)
        }
        LinkPick::Observed(from, to) => (from, to, true),
    };
    let dials = card_rect(state, from_socket.node)?;
    let accepts = card_rect(state, to_socket.node)?;
    let from = centre(pin_rect(state, dials, true));
    let to = centre(pin_rect(state, accepts, false));
    let mid = (u32::midpoint(from.0, to.0), u32::midpoint(from.1, to.1));

    let endpoints = endpoints_of(state, to_socket.node);
    let taken = endpoint_at(state, to_socket);
    let current = taken
        .as_ref()
        .and_then(|one| endpoints.iter().position(|e| e == one))
        .unwrap_or(0);
    // The caption is the endpoint the link took — which is the whole point of
    // there being an endpoint per link rather than per node.
    let caption = taken
        .or_else(|| endpoints.first().cloned())
        .unwrap_or_default();

    // ★★ R1681 — the chrome SCALES with the canvas, and every box is derived
    // from the type size rather than sized beside it. Both halves have a round
    // behind them. R1653: a part of the diagram held at fixed pixels keeps them
    // while the cards shrink, and at low zoom it covers a card the pointer can
    // then never reach — measured here as a `delete` seat swallowing the whole
    // of one card at the zoomed-out sweep. R1656: a box sized by a number
    // somebody typed, beside a run sized by the shaper, is two derivations of
    // one fact and the run wins.
    let font = canvas_font(state, FONT_SMALL);
    let line = line_box(font);
    let pad = (font / 2).max(3);
    let seat_h = line + pad;
    // Seven tenths of the type size per character, in the spirit of `line_box`:
    // over-reserve a little rather than clip a run. Both bounds either side of
    // this were MEASURED on the running screen — a whole em made a sixteen-
    // character address a seat wider than the card it belongs to, and three
    // fifths elided `tcp/0.0.0.0:7447` down to `…0.7447`, losing the scheme,
    // which is the half that carries the transport.
    let seat_w =
        |text: &str| -> u32 { u32::try_from(text.len()).unwrap_or(8) * font * 7 / 10 + pad * 2 };
    let gap = (font / 2).max(2);

    let mut chips = Vec::new();
    if endpoints.len() > 1 {
        let total: u32 = endpoints
            .iter()
            .map(|e| seat_w(e) + gap)
            .sum::<u32>()
            .saturating_sub(gap);
        let mut left = mid.0.saturating_sub(total / 2);
        let top = mid.1.saturating_sub(seat_h * 2 + line + gap);
        for one in &endpoints {
            let width = seat_w(one);
            chips.push((one.clone(), Rect::new(left, top, width, seat_h)));
            left += width + gap;
        }
    }

    let label_w = seat_w(&caption).max(seat_w("delete"));
    let label = Rect::new(
        mid.0.saturating_sub(label_w / 2),
        mid.1.saturating_sub(seat_h + line / 2),
        label_w,
        seat_h,
    );
    let act = Rect::new(
        mid.0.saturating_sub(seat_w("delete") / 2),
        mid.1 + line / 2,
        seat_w("delete"),
        seat_h,
    );

    // ★★★ R1681.3 — the column sits ON its wire, and is only moved to stay
    // inside what the canvas is showing.
    //
    // R1681 moved it clear of every card as well, to satisfy this screen's
    // invariant that a press ANYWHERE on a card reaches that card (R1655). The
    // running screen is what showed the price: measured, the picked link's
    // label and its `delete` seat ended up **240 pixels below the wire they
    // belong to**, past two other cards, because up and down were blocked at
    // every step until they were not. An annotation that far from what it
    // annotates is not an annotation.
    //
    // So the invariant is the thing that gives, and precisely: a press covered
    // by the PICKED LINK'S OWN CHROME is not an unexplained hole in a card, it
    // is an affordance the person summoned by picking that link, and the
    // reference draws it exactly there. The gate learns that exception from
    // THIS function rather than from a list beside it, so a chrome that wandered
    // somewhere absurd would still be caught covering cards it has no business
    // covering. What is left unmoved is the viewport clamp — an affordance
    // nudged off-screen would trade one unreachable thing for another, which is
    // what the first draft of R1681 measured itself doing.
    let mut parts: Vec<Rect> = chips
        .iter()
        .map(|(_, seat)| *seat)
        .chain([label, act])
        .collect();
    let shift = placement(state, &parts, line.max(2));
    for part in &mut parts {
        *part = Rect::new(
            part.x.saturating_add_signed(shift.0),
            part.y.saturating_add_signed(shift.1),
            part.w,
            part.h,
        );
    }
    let act = parts.pop().unwrap_or(act);
    let label = parts.pop().unwrap_or(label);

    Some(LinkChrome {
        font,
        label,
        caption,
        chips: chips
            .into_iter()
            .zip(parts)
            .map(|((one, _), seat)| (one, seat))
            .collect(),
        current,
        act,
        adopt,
    })
}

/// What the picked link carries: its caption, its endpoint seats and its one
/// act (R1681).
///
/// ★ The text of each seat is that seat's CHILD, not its neighbour. A floating
/// annotation is drawn over the diagram on purpose, so it is its own layer, and
/// saying that structurally is what keeps "no two runs of one widget overlap"
/// true without an exception list beside the rule.
fn link_affordances(chrome: &LinkChrome, ink: Ink) -> Vec<Scene> {
    // ★ R1656 — every run's box is the LINE BOX the shaper produces at this
    // type size, not a number the author guessed. One helper, so the three
    // seats cannot be sized three ways.
    //
    // ★★★★★ R1794 — and it is the WORD's box now, centred, rather than the
    // seat's whole inner width. It gave the caption `seat.w - pad*2` and the
    // shaper then laid the glyphs `Start`-aligned at the left of that, so the
    // seat was centred vertically and left-biased horizontally: measured on the
    // wire, `delete` sat 4px from its left edge and 14px from its right. A
    // reader reported it; no gate here could, because every gate compared
    // rectangles and this is about where the glyphs landed inside one.
    //
    // Through `caption::place` rather than a fourth local derivation — the
    // framework measures the word with the same shaper the frame paints with,
    // so the rectangle and the ink are one fact.
    // ★★★★★ R1812 — the placement is carried whole rather than reduced to a
    // rectangle. `.centred()` below is a DECLARATION, and until this round it
    // was consumed to produce a number and then dropped: the run went into the
    // scene carrying `TextAlign::Start`, so the paint told every reader — a
    // gate, a conformance check, a person asking over the wire — the opposite
    // of what this call says. `Placed::declares` is what carries it across
    // without stating it twice, and `caption::Survey` is what would have
    // noticed.
    let inner = |seat: Rect, word: &str| -> caption::Placed {
        let pad = (chrome.font / 2).max(3);
        caption::place(
            Rect::new(0, 0, seat.w, seat.h),
            &caption::Caption::new(word, run_style(chrome.font, MEASURING_INK))
                .centred()
                .padding(pad, 0),
        )
    };
    let mut out = vec![panel(
        "lab.link.label",
        chrome.label,
        ink.accent_soft,
        Some(ink.accent_line),
        vec![quiet(
            placed_label(
                "lab.link.label.text",
                chrome.caption.clone(),
                inner(chrome.label, &chrome.caption),
                chrome.font,
                ink.accent,
            ),
            Silence::name_of("lab.link.label"),
        )],
    )];
    for (n, (endpoint, seat)) in chrome.chips.iter().enumerate() {
        let picked = n == chrome.current;
        out.push(panel(
            &format!("lab.link.endpoint.{n}"),
            *seat,
            ink.surface,
            Some(if picked { ink.accent } else { ink.outline }),
            vec![quiet(
                placed_label(
                    &format!("lab.link.endpoint.{n}.text"),
                    endpoint.clone(),
                    inner(*seat, endpoint),
                    chrome.font,
                    if picked { ink.accent } else { ink.text_2 },
                ),
                Silence::name_of(format!("lab.link.endpoint.{n}")),
            )],
        ));
    }
    let (word, edge) = if chrome.adopt {
        ("adopt", ink.warn)
    } else {
        ("delete", ink.err)
    };
    out.push(panel(
        "lab.link.act",
        chrome.act,
        ink.surface,
        Some(edge),
        vec![quiet(
            placed_label(
                "lab.link.act.text",
                word.to_owned(),
                inner(chrome.act, word),
                chrome.font,
                edge,
            ),
            Silence::name_of("lab.link.act"),
        )],
    ));
    out
}

/// The picked link's own chrome, announced: what the wire is, which endpoint it
/// took, and the one act offered on it.
///
/// ★ Derived from [`link_chrome`], the same authority the painter and the hit
/// test read. A second derivation here is how the announcement would come to
/// name an endpoint the screen is not showing.
fn link_chrome_access(state: &LabState) -> Vec<AccessNode> {
    let Some(chrome) = link_chrome(state) else {
        return Vec::new();
    };
    let mut nodes = vec![
        AccessNode::new("lab.link.label", AriaRole::Status).with_name(chrome.caption.clone()),
        AccessNode::new("lab.link.act", AriaRole::Button).with_name(if chrome.adopt {
            "adopt this reported link into the drawing"
        } else {
            "delete this link"
        }),
    ];
    for (n, (endpoint, _)) in chrome.chips.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("lab.link.endpoint.{n}"), AriaRole::RadioButton)
                .with_name(endpoint.clone())
                .with_state(AccessState {
                    checked: Some(n == chrome.current),
                    ..AccessState::default()
                })
                .with_set_position(n, chrome.chips.len()),
        );
    }
    nodes
}

/// Whether the picked link's own chrome covers this window point (R1681.3).
///
/// ★★ The exception the card sweeps take, derived from the function that PAINTS
/// the chrome rather than restated beside them. A summoned overlay covering
/// part of a card is what the reference does and what a person expects; a
/// screen that moved the overlay instead put it 240 pixels from the wire it
/// annotates, which is the thing this exception exists to avoid re-doing.
#[cfg(test)]
fn chrome_covers(state: &LabState, px: u32, py: u32) -> bool {
    let Some(chrome) = link_chrome(state) else {
        return false;
    };
    let (cx, cy) = window_to_content(state, px, py);
    holds(chrome.act, cx, cy)
        || holds(chrome.label, cx, cy)
        || chrome.chips.iter().any(|(_, seat)| holds(*seat, cx, cy))
}

/// Where the picked link's column goes, as an offset from the wire's middle
/// (R1681, narrowed R1681.3, bounded R1704).
///
/// One rule: it must be inside what the canvas is showing. Answers `(0, 0)`
/// whenever the reference's own placement — on the wire — already fits, which
/// is nearly always.
///
/// ★★★★★ R1704 — **and `(0, 0)` when the link has left the view entirely**,
/// which is the condition this nudge was missing and a person reported.
///
/// The nudge exists because R1681's first draft of the card-avoidance search
/// pushed both endpoint seats out of the visible world — it keeps a column that
/// is being moved AROUND SOMETHING from being moved out of sight. It was never
/// meant to fetch a column back that the PAN carried away, and unbounded that
/// is what it did: dragged far enough, `dx` pinned the caption and the `delete`
/// chip to the canvas edge and held them there while the link itself clipped
/// away. Measured before the fix, panning until neither endpoint was painted:
/// the chip stayed at x=316 through pans of -949, -1898, -2847 and -3796 — and
/// **pressing it deleted the link**, 7 links to 6, with nothing on screen to
/// say which one had gone.
///
/// The behaviour canon settles the shape rather than this being a judgement
/// call: it places the caption at the wire's own midpoint (`abs(mx, my - 24)`)
/// with no clamp of any kind, and lets the viewport clip. So a column whose
/// natural place is off the canvas is simply left there, and R1653's clipping
/// takes it away with the link it belongs to.
fn placement(state: &LabState, parts: &[Rect], step: u32) -> (i32, i32) {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    let shown = Rect::new(
        u32::try_from(ox).unwrap_or(0),
        u32::try_from(oy).unwrap_or(0),
        canvas.w,
        canvas.h,
    );
    // ★ R1704 — nothing to keep in view, so nothing to move. `any` rather than
    // `all`: a column with one part still showing is the case the nudge is FOR,
    // and only a column entirely gone is the case it must keep its hands off.
    let touches = |r: &Rect| {
        r.x < shown.x.saturating_add(shown.w)
            && shown.x < r.x.saturating_add(r.w)
            && r.y < shown.y.saturating_add(shown.h)
            && shown.y < r.y.saturating_add(r.h)
    };
    if !parts.iter().any(touches) {
        return (0, 0);
    }
    let moved = |part: &Rect, by: (i32, i32)| -> Rect {
        Rect::new(
            part.x.saturating_add_signed(by.0),
            part.y.saturating_add_signed(by.1),
            part.w,
            part.h,
        )
    };
    let inside = |by: (i32, i32)| {
        parts.iter().all(|part| {
            let r = moved(part, by);
            r.x >= shown.x
                && r.y >= shown.y
                && r.x + r.w <= shown.x + shown.w
                && r.y + r.h <= shown.y + shown.h
        })
    };
    // The horizontal nudge is a clamp, not a search: a column wider than the
    // gap it sits in has one place to be, against whichever edge crowds it.
    let left = parts.iter().map(|p| p.x).min().unwrap_or(shown.x);
    let right = parts.iter().map(|p| p.x + p.w).max().unwrap_or(shown.x);
    let dx = if left < shown.x {
        i32::try_from(shown.x - left).unwrap_or(0)
    } else if right > shown.x + shown.w {
        -i32::try_from(right - (shown.x + shown.w)).unwrap_or(0)
    } else {
        0
    };

    // Vertically it moves only when it has to, and then by whole rows so the
    // column does not creep pixel by pixel as the graph is panned.
    let step = i32::try_from(step).unwrap_or(2).max(2);
    (0..64i32)
        .flat_map(|off| [(dx, -off * step), (dx, off * step)])
        .find(|by| inside(*by))
        .unwrap_or((dx, 0))
}

/// The wires — drawn and reported — and the affordances the picked one carries.
fn canvas_wires(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selected_link = state.selected_link.get();
    // A link being re-aimed is drawn following the cursor instead of where it
    // still is, so the person sees where it is going rather than where it was.
    let moving = match state.drag.get() {
        Some(Drag::Rewire { link, .. }) => Some(link),
        _ => None,
    };
    {
        let doc = state.doc.borrow();
        if let Some(tree) = doc.tree(ROOT) {
            for link in tree.links() {
                if moving == Some(link.id) {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    card_rect(state, link.from.node),
                    card_rect(state, link.to.node),
                ) else {
                    continue;
                };
                let chosen = selected_link == Some(LinkPick::Authored(link.id));
                let from = centre(pin_rect(state, a, true));
                let to = centre(pin_rect(state, b, false));
                children.push(wire(
                    &format!("lab.link.{}", link.id.0),
                    from,
                    to,
                    if chosen { ink.accent } else { ink.accent_line },
                    if chosen { 2 } else { 1 },
                ));
            }
        }
        // ★★ R1681 — what a source reported: the warning colour AND the dash
        // rhythm, because it is not in the graph and must not read as though it
        // were. ★ R1681.2 — the rhythm is `Dash::DOTTED`, which is what the
        // sibling screen drawing these same two layers already uses for a
        // reported link; R1681 said this primitive had no dash and reached for
        // colour alone, which was false.
        for seen in doc.observations(ROOT) {
            let (Some(a), Some(b)) = (
                card_rect(state, seen.from.node),
                card_rect(state, seen.to.node),
            ) else {
                continue;
            };
            let chosen = selected_link == Some(LinkPick::Observed(seen.from, seen.to));
            children.push(dashed_wire(
                &format!(
                    "lab.observed.{}.{}",
                    state.name_of(seen.from.node),
                    state.name_of(seen.to.node)
                ),
                centre(pin_rect(state, a, true)),
                centre(pin_rect(state, b, false)),
                ink.warn,
                if chosen { 3 } else { 2 },
                Some(Dash::DOTTED),
            ));
        }
    }

    // A link in flight follows the cursor, so a drag shows what it will do
    // before it does it — the reference commits on release. ★ R1681 — ONE
    // block for both drags: authoring and re-aiming draw the same preview from
    // the same pin, and the only difference is whether a wire already exists at
    // the far end. Two copies of it is how the second one would come to be
    // drawn from somewhere else.
    let in_flight = match state.drag.get() {
        Some(Drag::Wire { from, .. } | Drag::Rewire { from, .. }) => Some(from),
        _ => None,
    };
    if let Some(from) = in_flight
        && let Some(card) = card_rect(state, from)
    {
        let cursor = state.cursor.get();
        let (cx, cy) = window_to_content(state, cursor.0, cursor.1);
        children.push(wire(
            "lab.link.preview",
            centre(pin_rect(state, card, true)),
            (
                u32::try_from(cx).unwrap_or(0),
                u32::try_from(cy).unwrap_or(0),
            ),
            ink.accent,
            2,
        ));
    }

    children
}

/// ★★★★★ R1919 — **what this screen's search is showing right now.**
///
/// Derived from `searching` and the document on every call rather than stored
/// beside them, so a hit cannot outlive the node it names. That is the property
/// the census rows this closes could not have: both references keep a result
/// list built when the query ran, and both then have to invalidate it.
fn found(state: &LabState) -> Vec<Found> {
    state.doc.borrow().find(ROOT, &state.searching.get())
}

/// The nodes the search is currently showing, as a set the painter can ask.
fn found_nodes(state: &LabState) -> Vec<NodeId> {
    found(state).into_iter().map(|hit| hit.node).collect()
}

/// One card per node: its identity band, its digest rows, and its pins.
///
/// ★ R1656 — the card's parts are its CHILDREN, not its siblings.
///
/// They were siblings until a person reported text outside the border and
/// nothing here could see it. §5.15's containment read judges a mark against
/// its parent, `scene/text_painted` names a run's nearest tagged ancestor, and
/// the smear gate groups by that ancestor — so with the parts flattened into
/// the canvas's child list, every one of those three questions was being asked
/// about the CANVAS, which is big enough to hold anything. The scene was
/// describing a screen that did not exist (§2 #7), and the checks were honest
/// answers to the wrong question.
///
/// Nesting costs the card-local coordinates below: an absolutely-positioned
/// child is placed relative to its parent (R1648 measured what forgetting that
/// looks like — every mark at twice its offset), which is why `CardShape`
/// hands out local rectangles for the parts and a window rectangle for the card.
fn canvas_cards(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selection = state.selection.get();
    // ★ R1919 — the search's hits are a THIRD axis over a card, beside the two
    // the selection already has. A hit is not a selection: a search can show
    // six cards at once and the inspector still follows one, so folding them
    // together would make "found" mean "selected" and lose whichever a reader
    // asked for last.
    let hits = found_nodes(state);
    // ★★★★★ R1927 — which cards have a problem, and whether the worst of it
    // blocks, worked out ONCE for the whole canvas.
    let troubled = troubled_cards(state);
    for node in state.cards() {
        let Some(shape) = card_shape(state, node) else {
            continue;
        };
        let name = state.name_of(node);
        let rows = card_rows(state, node);
        let role = state.role_of(node).unwrap_or(Role::Peer);
        // ★★ R1706 — three states, not two, which is what a selection of many
        // needs and what the reference's own canvas paints: the card the
        // inspector follows takes the accent outright, the rest of the
        // selection takes it weakened, and everything else keeps the plain
        // outline. Two states would make a group selection look exactly like
        // one card selected plus five that are not.
        let chosen = selection.is_active(&node);
        let in_selection = selection.contains(&node);
        let mut parts: Vec<Scene> = Vec::new();
        // ★ R1691 — the identifier and the role chip are what the CARD is
        // called and what it is. Its own announcement carries both, so a node
        // here would read the identifier twice before saying anything new.
        // ★★★★★ R1921 — the letters are CHOSEN for contrast against the fill
        // they sit on, never authored. That is the half the reference leaves to
        // each subclass, and leaving it there is how a title becomes
        // unreadable; `Faces::title_text` makes that unreachable.
        let ink_for_name = card_tint(state, node).map_or(ink.text, |tint| {
            let letters = Faces::of(tint).title_text;
            Color::rgba(letters.r, letters.g, letters.b, 255)
        });
        parts.push(quiet(
            tagged_label(
                &format!("lab.node.{name}.id"),
                name.clone(),
                shape.id,
                shape.id_font,
                ink_for_name,
            ),
            Silence::name_of(format!("lab.node.{name}")),
        ));
        parts.push(quiet(
            box_at(
                &format!("lab.node.{name}.badge"),
                shape.badge,
                ink.surface,
                Some(role_ink(role)),
                4,
            ),
            Silence::part_of(format!("lab.node.{name}")),
        ));
        parts.push(label(
            role.badge(),
            shape.badge_text,
            shape.badge_font,
            role_ink(role),
        ));
        // ★★★★★ R1927 — **the behaviour canon's per-node issue dot**, which this
        // screen did not have. There the card carries a small round mark
        // whenever the validation names that node, coloured by whether what it
        // named blocks; here the same, and the colour is the framework's
        // blocking/non-blocking split rather than a second rule.
        //
        // Read from `troubled`, which is `problems()` — the ONE walk this
        // screen already renders its panel and its jump from — indexed once
        // ABOVE this loop. Asking per card would be a full walk of every
        // finding for every card drawn, and a second walk here is exactly the
        // shape R1717's note on that function refuses.
        if let Some(&blocks) = troubled.get(&node) {
            parts.push(quiet(
                box_at(
                    &format!("lab.node.{name}.issue"),
                    shape.issue,
                    if blocks { ink.err } else { ink.warn },
                    Some(ink.surface),
                    shape.issue.w / 2,
                ),
                Silence::part_of(format!("lab.node.{name}")),
            ));
        }
        for ((key, value), (key_rect, value_rect)) in rows.iter().zip(shape.rows.iter()) {
            parts.push(label(key.clone(), *key_rect, shape.row_font, ink.text_3));
            // The value column holds user data — an endpoint, a key
            // expression — and its TAIL is what distinguishes one from another,
            // so the middle gives way rather than the end.
            parts.push(value_label(
                value.clone(),
                *value_rect,
                shape.row_font,
                ink.text_2,
            ));
        }
        // ★★★★★ R1919 — a search hit is drawn WIDER, and the edge COLOUR stays
        // the selection axis's. Two channels of one edge, one per axis, because
        // a card can be *found and selected*, *found and not*, *selected and
        // not found*, or neither, and a reader has to be able to tell which.
        //
        // ⚠ The first draft of this round wrote that sentence and then gave a
        // hit `accent_line` — the colour `in_selection` already owns — so
        // "found" and "selected" were THE SAME EDGE on the frame and the very
        // collapse this comment argues against had already happened. The walk
        // caught it only because it was taught to read the edge at all: it had
        // been comparing rectangles, and a border is not a rectangle. So the
        // orthogonality is now ASSERTED rather than described — see
        // `r1919_a_name_is_looked_for_across_the_document.py` (C) and (H).
        let found_here = hits.contains(&node);
        // ★★★★★ R1921 — a card a person coloured is FILLED with that colour;
        // one nobody coloured keeps the surface its kind gives.
        //
        // ⚠ The `title` face and not `body`, and the first draft of this line
        // had it wrong in a way worth recording: `title_text` is chosen for
        // contrast against `title`, so painting the fill with the DARKER body
        // face while lettering it for the lighter title face reintroduces
        // exactly the unreadable combination `Faces` exists to prevent. A
        // card here has no separate header band — the whole card IS the band —
        // so `title` is the face it wears, and the walk holds the contrast
        // against whatever fill it finds rather than against the one this
        // comment claims.
        let fill = card_tint(state, node).map_or(ink.surface, |tint| {
            let title = Faces::of(tint).title;
            Color::rgba(title.r, title.g, title.b, 255)
        });
        let mut style = BoxStyle::filled(fill).with_corner_radius(9);
        let edge = if chosen {
            ink.accent
        } else if in_selection {
            ink.accent_line
        } else {
            ink.outline_2
        };
        style = style.with_border(Border::new(edge, if found_here { 3 } else { 1 }));
        children.push(Scene::Container(
            ContainerNode::new(parts)
                .with_tag(format!("lab.node.{name}"))
                .with_style(style)
                .with_layout(absolute(shape.rect)),
        ));
        children.extend(canvas_pins(state, node, shape.rect, role, ink));
    }
    children
}

/// ★★★★★ R1928 — **what a reader who cannot see the canvas is told about one
/// pin**: the card it is on, what kind of pin it is, what the MODEL calls it,
/// and what can be done with it.
///
/// The third clause is the one this round added, and it is the only clause this
/// file does not own — [`Document::port_label`] resolves it from the kind's
/// declaration, the item's authored label, or the kind's own answer, and a
/// second spelling here would be free to disagree with what a client reading
/// the model sees.
///
/// ⚠ A [`Silent`](pinion_node_graph::PortName::Silent) port drops the clause rather than announcing an
/// empty one. That is the distinction the type exists for: the pin is still
/// announced and still says what it is for, and what is absent is a NAME, said
/// so in as many words.
fn pin_announcement(
    state: &LabState,
    node: NodeId,
    at: PortRef,
    card: &str,
    kind: &str,
    verb: &str,
) -> String {
    match state
        .doc
        .borrow()
        .port_label(ROOT, node, at)
        .and_then(|held| held.text)
    {
        Some(name) => format!("{card} {kind} · {name} — {verb}"),
        None => format!("{card} {kind}, unnamed — {verb}"),
    }
}

/// ★★★★★ R1927 — **which cards have a problem, and whether the worst of each
/// blocks** — the whole canvas in one walk.
///
/// A card is absent from the map when nothing is wrong with it, present with
/// `false` when only non-blocking findings name it, and present with `true`
/// when at least one blocking finding does. Three answers rather than a bool,
/// because the canon's dot is coloured by exactly that distinction and a bool
/// would make "no problem" and "a warning" the same mark.
///
/// ⚠ Written over the whole SET rather than per card on purpose. The first
/// draft took a `node` and filtered [`LabState::problems`] for it, which is one
/// complete walk of every finding **per card drawn** — quadratic in the graph,
/// and invisible at the eight cards the specification opens with. The gate
/// panel, the jump and the dots are all renderings of one walk (R1717); asking
/// that walk once per mark is that same defect wearing the correct answer.
fn troubled_cards(state: &LabState) -> BTreeMap<NodeId, bool> {
    let mut worst: BTreeMap<NodeId, bool> = BTreeMap::new();
    for problem in state.problems() {
        if let Some(node) = problem.node {
            let held = worst.entry(node).or_insert(false);
            *held = *held || problem.blocks;
        }
    }
    worst
}

/// A node's pins. Their appearance **is** the rule the legend states: filled =
/// can dial, ringed in the transport's colour = can be dialled, grey = the role
/// listens and this node has nowhere to.
fn canvas_pins(state: &LabState, node: NodeId, card: Rect, role: Role, ink: Ink) -> Vec<Scene> {
    let name = state.name_of(node);
    let mut children: Vec<Scene> = Vec::new();
    // ★★★★★ R1912 — a pin a hand PUT AWAY is not drawn, and the model is what
    // says so. `visible_ports` answers over both reasons a port can be off the
    // frame; asking it here rather than reading the appearance is what keeps
    // this painter from becoming a second copy of that rule.
    //
    // The lab's dial pin is output 0 and its accept pin is input 0 — the
    // variadic run's first item — which is the mapping `LabNode`'s signature
    // declares.
    let drawn = state.doc.borrow().visible_ports(ROOT, node);
    let shows = |side: Side, index: u32| -> bool {
        drawn.as_ref().is_none_or(|v| {
            let list = match side {
                Side::Input => &v.inputs,
                Side::Output => &v.outputs,
            };
            list.contains(&index)
        })
    };
    {
        let listening = state.forms.borrow().get(&node).is_some_and(|f| {
            f.field("listen.endpoints")
                .is_some_and(|v| !v.value().trim().is_empty())
        });
        let transport = state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.node(node))
            .and_then(|n| match &n.body {
                NodeBody::Kind(kind) => Some(kind.transport),
                _ => None,
            })
            .unwrap_or(Transport::Tcp);
        if shows(Side::Output, 0) {
            children.push(box_at(
                &format!("lab.pin.{name}.dial"),
                pin_rect(state, card, true),
                ink.accent,
                Some(ink.accent),
                PIN / 2,
            ));
        }
        if role.accepts() && shows(Side::Input, 0) {
            // ★★★★★ R1924 — a card that would take the wire being re-aimed
            // wears the accent while the hand is carrying it.
            //
            // The BORDER, because that is the property a pin's identity lives
            // in here — R1919 measured the cost of asserting a rectangle when
            // the change is an edge. The set is the crate's answer, so a card
            // is lit exactly when `may_relink` would say yes: there is no
            // second rule on this side deciding what "will take it" means.
            let lit = state.rewire_targets.borrow().contains(&node);
            children.push(box_at(
                &format!("lab.pin.{name}.accept"),
                pin_rect(state, card, false),
                ink.surface,
                Some(if lit {
                    ink.accent
                } else if listening {
                    transport_ink(transport)
                } else {
                    ink.text_3
                }),
                PIN / 2,
            ));
        }
        // ★★★★★ R1914 — the pins a SPLIT put there, under the parent's place.
        //
        // The parent is hidden by `shows` above (the model answers
        // `Hidden::Split` for it), so without this a split would take a pin off
        // the frame and put nothing back — which is a picture that says the
        // gesture destroyed something. The reference draws its sub-pins in the
        // parent's place for the same reason.
        //
        // The member word comes from the model's own port name, so the tag a
        // client presses and the address `split_pin` accepts are the same
        // spelling by construction.
        for (side, dial) in [(Side::Output, true), (Side::Input, false)] {
            for (ordinal, (path, port)) in member_pins(state, node, side).into_iter().enumerate() {
                children.push(box_at(
                    // ★ R1915 — the tag is `pin_word`'s output, which is the
                    // same function `Hit::of_tag` parses back and the same one
                    // `split_pin` accepts. One spelling, so a client can press
                    // what it read.
                    &format!("lab.pin.{name}.{}", pin_word(side, &path)),
                    member_pin_rect(state, card, dial, ordinal),
                    ink.surface,
                    // ★★★★★ R1926 — the MEMBER's own type colour, asked of the
                    // model with the port in hand.
                    //
                    // This line used to read `transport_ink(transport)` — the
                    // NODE's transport — so a locator's two halves were drawn
                    // in one colour, the parent's, and a reader could not tell
                    // the host from the service nor either from the whole. That
                    // is what the reference's per-pin colour hook is for, and
                    // the crate now derives it from one declaration
                    // (`NodeKind::type_colour`) so this cannot drift from it.
                    Some(
                        palette_of::<graph::LabNode>(&port.flow)
                            .own()
                            .map_or(ink.text_3, ink_of),
                    ),
                    PIN / 2,
                ));
            }
        }
    }
    children
}

/// R1914 — the member ports a split put on one side of `node`, in draw order.
///
/// Asked of the model rather than derived from the appearance, because the
/// model is where the expansion lives: a screen that walked the split
/// declaration itself would be a second expansion, free to disagree with the
/// one the signature was spliced by.
fn member_pins(
    state: &LabState,
    node: NodeId,
    side: Side,
) -> Vec<(PortPath, pinion_node_graph::Port<graph::Endpoint, String>)> {
    state
        .doc
        .borrow()
        .resolved_ports(ROOT, node, side)
        .into_iter()
        .filter(|(path, _)| path.depth() > 0)
        .collect()
}

/// R1914 — where the `ordinal`-th member pin of a split sits.
///
/// Under the pin it came out of, one pin's height apart. The parent's own
/// place is left empty, which is what makes a split READ as one: the pin that
/// was there is gone and the things it came apart into are where it was.
fn member_pin_rect(state: &LabState, card: Rect, dial: bool, ordinal: usize) -> Rect {
    let seat = pin_rect(state, card, dial);
    let step = seat.h + seat.h / 2;
    let down = u32::try_from(ordinal).unwrap_or(0) * step;
    Rect::new(seat.x, seat.y + down, seat.w, seat.h)
}

/// The two things that float over the canvas: the launch gate and the gesture
/// hint.
fn canvas_overlays(state: &LabState, ink: Ink) -> Vec<Scene> {
    let rect = canvas_rect();
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children: Vec<Scene> = Vec::new();
    let gate = local(gate_rect(state));
    let verdict = state.verdict();
    children.push(box_at(
        "lab.gate",
        gate,
        ink.surface,
        Some(ink.outline_2),
        10,
    ));
    // ★★★★★ R1927 — every box in this panel is as tall as the face it holds,
    // `line_box` and not a written-down 13. All three were 13 while a 9-pixel
    // face reserves 15 and an 11-pixel one 18, so **every line this panel has
    // ever drawn** was in a box too short for its own descenders — and the
    // defect was invisible in the round-level number because it scales with how
    // many findings the graph has. This round added a seventh finding, the
    // sweep's short-run count rose by exactly the lines it added, and that is
    // what pointed here. The seats (10 / 28 / 48) and the 20-pixel pitch are
    // unchanged and still hold the taller boxes: R1874's lesson is that a box
    // consulting its face forces the row around it to, and here the row was
    // already generous enough.
    children.push(quiet(
        tagged_label(
            "lab.gate.head",
            "pre-launch check",
            Rect::new(gate.x + 12, gate.y + 10, 150, line_box(FONT_SMALL)),
            FONT_SMALL,
            ink.text,
        ),
        Silence::name_of("lab.gate"),
    ));
    children.push(quiet(
        tagged_label(
            "lab.gate.verdict",
            verdict.sentence(),
            Rect::new(
                gate.x + 12,
                gate.y + 28,
                gate.w - 24,
                line_box(GATE_LINE_FONT),
            ),
            GATE_LINE_FONT,
            if verdict.may_launch() {
                ink.ok
            } else {
                ink.err
            },
        ),
        // The sentence IS the panel's value; the panel is one stop and the
        // findings under it are the list a reader walks.
        Silence::part_of("lab.gate"),
    ));
    let (shown, hidden) = gate_shown(state);
    let line_at = |n: usize| {
        Rect::new(
            gate.x + 12,
            gate.y + 48 + u32::try_from(n).unwrap_or(0) * GATE_LINE_H,
            gate.w - 24,
            line_box(GATE_LINE_FONT),
        )
    };
    for (n, (blocks, sentence)) in shown.iter().enumerate() {
        children.push(tagged_label(
            &format!("lab.gate.line.{n}"),
            sentence.clone(),
            line_at(n),
            GATE_LINE_FONT,
            if *blocks { ink.err } else { ink.warn },
        ));
    }
    // ★ R1690 — what the panel has no room for, counted rather than dropped.
    if hidden > 0 {
        children.push(tagged_label(
            "lab.gate.more",
            format!("+{hidden} more — the verdict counts all of them"),
            line_at(shown.len()),
            GATE_LINE_FONT,
            ink.text_3,
        ));
    }
    // ★ R1678 — one button per scope that has something to put back, from the
    // SAME list the hit test resolves against.
    for (scope, seat) in reset_seats(state) {
        let seat = local(seat);
        children.push(box_at(
            &format!("lab.reset.{}", scope.wire()),
            seat,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label(
            scope.wire(),
            Rect::new(seat.x + 7, seat.y + 5, seat.w.saturating_sub(14), 12),
            FONT_SMALL,
            ink.text_2,
        ));
    }

    let hint = local(hint_rect());
    children.push(box_at("lab.hint", hint, ink.surface, Some(ink.outline), 8));
    children.push(quiet(
        tagged_label(
            "lab.hint.text",
            hint_text(),
            Rect::new(hint.x + 10, hint.y + 6, hint.w - 20, 13),
            9,
            ink.text_3,
        ),
        // The strip's whole content is the run inside it, and the strip is what
        // announces it — this screen's only statement of what the pointer can
        // do, which a reader needs most and could not hear at all.
        Silence::name_of("lab.hint"),
    ));

    children.extend(canvas_toast(state, ink));
    children.extend(pin_tip(state, ink, rect));

    children
}

/// ★★★★★ R1916 — the description of the pin a reader is resting on, drawn
/// beside it.
///
/// The canon puts a `title` on 25 of its controls; the assembled tool mounted
/// none, because the framework's tooltip was its own anchor and nothing could
/// say *that mark over there has a sentence*. `Descriptions` is that, and this
/// is the first thing to draw what it answers.
///
/// ⚠ It is placed beside the DESCRIBED MARK rather than at the cursor, and that
/// is WCAG 1.4.13's *hoverable* obligation read honestly: a body that follows
/// the pointer is a body the pointer can never reach. The anchor is the pin's
/// own rectangle, so the two are contiguous.
fn pin_tip(state: &LabState, ink: Ink, canvas: Rect) -> Vec<Scene> {
    let Some((tag, sentence)) = pin_description_shown(state) else {
        return Vec::new();
    };
    let Some(anchor) =
        pinion_core::painted::painted_regions(VIEW_TAG).and_then(|marks| marks.rect_of(&tag))
    else {
        return Vec::new();
    };
    // ★★★★★ R1918 — WHERE it goes AND what it looks like are the substrate's
    // now. R1916 lifted only the placement and left the box, the run and the
    // run's deferral here; the round that needed four more screens to draw one
    // lifted the rest. What stays this screen's is the INKS and the FACE, which
    // are properties of the surface a description has to sit legibly on.
    vec![pinion_widget_paint::described::view_description(
        TOOLTIP_TAG,
        &sentence,
        anchor,
        canvas,
        (canvas.x, canvas.y),
        pinion_widget_paint::described::DescriptionStyle::COMPACT,
        pinion_widget_paint::described::DescriptionInk {
            surface: ink.surface,
            outline: Some(ink.outline_2),
            ink: ink.text_2,
        },
    )]
}

/// The bullet's colour, which is what a sighted reader learns the tone from.
///
/// ★★★★★ R1719 — this used to be `ink.accent`, unconditionally, so a refusal
/// and a confirmation were the same picture. It is the seeing half of the pair
/// whose hearing half is the live region's urgency, and both now come off the
/// same [`Tone`] rather than off two constants that could disagree.
///
/// The mature toolkit gives its dialogs five icons for the same job and never
/// joins them to what a screen reader is told; here one value decides both.
const fn toast_ink(tone: Tone, ink: Ink) -> Color {
    match tone {
        Tone::Done => ink.accent,
        Tone::Refused => ink.err,
        // Not `ok` and not `err`: nothing happened, so the bullet says "heard
        // you" rather than "did it" — the same grey the screen uses for text
        // that is present and not the point.
        Tone::Unchanged => ink.text_3,
    }
}

/// ★★★★★ R1688 — **the last thing the screen said**, which nothing painted
/// until this round. See [`toast_rect`]: four comments in this file already
/// described a person reading it.
fn canvas_toast(state: &LabState, ink: Ink) -> Option<Scene> {
    let pane = canvas_rect();
    let seat = toast_rect(state)?;
    let seat = Rect::new(seat.x - pane.x, seat.y - pane.y, seat.w, seat.h);
    let inner = panel_content(seat);
    let said = state.toast.showing()?;
    let dot = Rect::new(inner.x + TOAST_PAD, inner.y + TOAST_PAD + 3, 7, 7);
    Some(panel(
        "lab.toast",
        seat,
        ink.raised,
        Some(ink.outline_2),
        vec![
            quiet(
                box_at("lab.toast.dot", dot, toast_ink(said.tone(), ink), None, 4),
                Silence::decorative("the bullet before the message"),
            ),
            quiet(
                tagged_label(
                    "lab.toast.text",
                    said.sentence(),
                    Rect::new(
                        dot.x + TOAST_DOT,
                        inner.y + TOAST_PAD,
                        inner.w.saturating_sub(TOAST_DOT + TOAST_PAD * 2).max(1),
                        line_box(FONT_SMALL),
                    ),
                    FONT_SMALL,
                    ink.text,
                ),
                Silence::name_of("lab.toast"),
            ),
        ],
    ))
}

/// The canvas pane: its layers, in the order a reader meets them — the surface,
/// the host frames, the wires, the node cards, then the two things that float
/// over all of it.
fn canvas(state: &LabState, ink: Ink) -> Scene {
    let rect = canvas_rect();
    // ★ R1653 — the world surface, and the viewport the pan slides over it.
    // The alternative the screen shipped with was to add the pan to every
    // rectangle: that has no clip (panned content painted over the palette and
    // the inspector) and it underflows on a leftward pan, which is a crash on
    // half of the gesture the hint strip advertises.
    let (world_w, world_h) = world_extent(state);
    // ★★★★★ R1726 — the world says what it is HOLDING, and the card being
    // dragged paints in front, is pressed first and is raised, all from that
    // one word. Before it, a dragged card went UNDER the card it was over:
    // measured by paint order, index 70 against the stationary card's 80. The
    // owner reported that as three things — it goes grey, they do not overlap,
    // is not overlapping better — and all three were this.
    let held = match state.drag.get() {
        Some(Drag::Node { node, .. }) => Some(format!("lab.node.{}", state.name_of(node))),
        _ => None,
    };
    let mut world_surface = ContainerNode::new(canvas_world(state, ink));
    if let Some(tag) = held {
        world_surface = world_surface.with_held(tag);
    }
    let world = Scene::Container(
        world_surface
            // ★★★★★ R1705 — TRANSPARENT, and this line is the whole reason the
            // first draft of the lattice drew nothing. The surface used to
            // carry the same `ink.bg` the canvas behind it carries, which was
            // invisible while it was the only ground; once the canvas grew a
            // lattice, this opaque fill covered every dot of it. The pixels
            // said so — 1,731 grid-coloured pixels across the canvas before the
            // change and ZERO after — while the scene, the node count and the
            // published declaration all looked right, and a glance at the two
            // screenshots looked right too.
            //
            // The surface is a coordinate space, not a colour: the canvas is
            // what has a ground.
            .with_style(BoxStyle::filled(Color::TRANSPARENT))
            .with_layout(LayoutStyle::new().with_size(Size::px(world_w, world_h))),
    );
    let (ox, oy) = world_offset(state, state.pan.get());
    let viewport = Scene::Scroll(
        ScrollNode::new(panel_content(rect), world)
            .with_axis(ScrollAxis::Both)
            .with_offset(ox, oy)
            // A drag on this surface is a pan, not a selection sweep, so the
            // edge must not carry the content away under the cursor.
            .with_auto_scroll(AutoScroll::off()),
    );
    let mut children = vec![viewport];
    // The gate panel and the hint strip are chrome: they float over the canvas
    // and do not pan with it.
    children.extend(canvas_overlays(state, ink));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("lab.canvas")
            // ★★★★★ R1705 — the grid rides the VIEWPORT, not the world surface,
            // which is what makes it endless. A lattice on the world would be
            // bounded by the world exactly as the enumerated pips were, and
            // panning past 6,400 units would blank the canvas again. On the
            // viewport it is always full, and the phase carries the pan so the
            // surface still reads as a thing being moved. The behaviour canon
            // puts its background on the viewport element for the same reason.
            .with_style(BoxStyle::filled(ink.bg).with_lattice(canvas_lattice(state, ink)))
            .with_layout(absolute(rect)),
    )
}

/// ★★★★★ R1909 — the panel FIRST, its body inside the closure.
///
/// The two `side_panel` calls this function used to make were reached only
/// after the body had been built, so a folded inspector built a body whose
/// widths derive from a panel that is now a strip. One row's
/// `inspector_body_w() - 20` underflowed the moment this screen's inspector was
/// declared to open folded.
///
/// Calling `side_panel` at the top makes the fold the OUTER decision, which is
/// what it is: whether there is a body at all comes before what the body says.
fn inspector(state: &LabState, field: (TextFieldState, u32), theme: &Theme, ink: Ink) -> Scene {
    let rect = inspector_rect();
    side_panel(SidePanel::Inspector, state, rect, &ink, || {
        inspector_body(state, field, theme, ink, rect)
    })
}

/// What an OPEN inspector holds. Called only through [`side_panel`]'s closure,
/// so nothing here has to ask whether the panel is folded.
fn inspector_body(
    state: &LabState,
    field: (TextFieldState, u32),
    theme: &Theme,
    ink: Ink,
    rect: Rect,
) -> Scene {
    let mut children = vec![label(
        spec::PANES[3].title,
        Rect::new(PAD, 14, 180, 16),
        FONT_BODY + 1,
        ink.text,
    )];

    let Some(node) = state.active_card() else {
        children.push(label(
            "no node selected",
            Rect::new(PAD, 48, 200, 14),
            FONT_SMALL,
            ink.text_3,
        ));
        // ★ R1690 — the meter stands with nothing selected too. It is a fact
        // about the palette, and this is the pane state a reader sizing the
        // tool up is most likely to be looking at.
        children.extend(inspector_reach(ink));
        return quiet(
            scroll_pane(
                &state.inspector_scroll,
                side_panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            ),
            Silence::layout("scrolls the inspector; the pane above it is what a reader lands on"),
        );
    };
    children.extend(inspector_identity(state, node, ink));
    children.extend(inspector_edit(state, ink));
    children.extend(inspector_reach(ink));
    inspector_pane(state, field, theme, ink, children)
}

/// **The reach meter**: how much of the option surface this palette can author.
///
/// ★★★ R1690 — painted whether or not a card is selected, because it is a fact
/// about the tool and not about a node. That is also why it survives the
/// early return above: the pane with nothing selected is exactly where a reader
/// is deciding whether this tool can configure their system.
///
/// # Why the warning ink means "wrong" and not "incomplete"
///
/// The reference colours its pill by whether any top-level section is
/// unreached, which on its own surface means a regression — its palette covers
/// all twenty. This surface is deliberately larger than the palette (a leaf
/// nobody offers a chip for is typed in by hand, which is what that affordance
/// is for), so the same rule would leave the pill permanently amber and stop
/// meaning anything. [`Reach::sound`] is the same INTENT correctly mapped: a
/// key offered at a shape the target refuses, or a key on no line of the
/// surface at all, is a defect at any coverage.
///
/// [`Reach::sound`]: pinion_core::widgets::config_schema::Reach::sound
fn inspector_reach(ink: Ink) -> Vec<Scene> {
    let reach = palette_reach();
    let seat = Rect::new(PAD, REACH_ROW_Y, inspector_body_w(), REACH_H);
    let (fill, edge, text) = if reach.sound() {
        (ink.surface, ink.outline, ink.text_3)
    } else {
        (ink.surface, ink.warn, ink.warn)
    };
    let caption = seat_caption(seat);
    vec![
        box_at("lab.inspector.reach", seat, fill, Some(edge), 6),
        // ★ R1691 — the run inside the pill is the pill's own words. Announcing
        // it too would read the same two figures out twice; the pill carries
        // them.
        quiet(
            tagged_label(
                "lab.inspector.reach.text",
                reach_caption(),
                caption,
                FONT_SMALL,
                text,
            ),
            Silence::name_of("lab.inspector.reach"),
        ),
    ]
}

/// What the reach pill says — **one derivation**, read by the paint and by the
/// announcement.
///
/// Written twice they drift, and the drift is the worst kind here: a reader is
/// told a coverage figure that the screen does not show.
fn reach_caption() -> String {
    format!(
        "{} · {}",
        palette_reach().label(),
        settings::strings().label()
    )
}

/// Whether a card is drawn small and whether it is switched off — the pair the
/// node's-life row reads to choose its words.
fn card_switches(state: &LabState, node: NodeId) -> (bool, bool) {
    state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .map_or((false, false), |slot| {
            (slot.appearance.collapsed, slot.disabled)
        })
}

/// ★★★★★ R1912 — whether a hand has put any of this card's pins away.
///
/// Read from `visible_ports`' put-away lists rather than from the appearance's
/// own vectors, so the seat's word, the paint and the wire all answer from the
/// one derivation that also knows about the node's unused-port rule.
fn pins_are_away(state: &LabState, node: NodeId) -> bool {
    state
        .doc
        .borrow()
        .visible_ports(ROOT, node)
        .is_some_and(|v| !v.put_away_inputs.is_empty() || !v.put_away_outputs.is_empty())
}

// ── R1853: the fault-injection panel ────────────────────────────────────────

/// The faults the selected node's settings admit, as one value a client reads in
/// a round trip.
///
/// ★★★★★ §2 #7 on this axis: an agent asking *what can I break about this node*
/// gets the answer as DATA — the path, the arm, whether injecting it stops a
/// launch, the badge that says whether a running node would even see it, and the
/// part of the declaration that admits it. The reference toolkit's own answer to
/// the same question is measured in `tools/demos/r1853_*.py`.
fn faults_json(state: &LabState) -> serde_json::Value {
    serde_json::Value::Array(
        fault_rows(state)
            .iter()
            .map(|row| {
                serde_json::json!({
                    "key": row.key,
                    "kind": row.kind.wire(),
                    "value": row.value,
                    "blocks": row.blocks(),
                    "applies": row.applies.map(pinion_core::widgets::config_form::Applies::wire),
                    "admitted_by": row.admitted_by,
                })
            })
            .collect(),
    )
}

/// How tall one fault row is.
///
/// ★ Derived from the two lines it holds rather than chosen: a row carries the
/// path-and-arm on one line and the badge-and-reason on the next, and each line's
/// box is `line_box` of its own face. R1851 measured what a hand-picked number
/// costs here — a box shorter than its face is a clipped descender the ink
/// census counts and nobody sees.
const FAULT_LINE_H: u32 = pinion_core::containment::line_box(FAULT_PX) * 2 + 8;

/// The face a fault row is set in.
const FAULT_PX: u32 = 10;

/// ★★★★★ R1912 — the closed set of scope words `put_away_pins` accepts.
///
/// The three the references have, plus this screen's two pin names. Written as
/// one list because it is one vocabulary: the declaration an agent reads and
/// the words the verb parses must be the same set, and two lists is how they
/// stop being.
///
/// ⚠ `others:dial` and `others:accept` are spelled out rather than left as a
/// prefix an agent has to infer. A domain that says "anything starting with
/// `others:`" is a domain a client cannot enumerate, which is the whole point
/// of publishing one.
const PIN_SCOPES: [&str; 6] = [
    "unwired",
    "restore",
    "dial",
    "accept",
    "others:dial",
    "others:accept",
];

/// ★★★★★ R1914 — the closed set of words `split_pin` accepts.
///
/// The pin to act on, and — for a pin already split — the **member** of it. The
/// engine has four commands here (`SplitPin` / `RecombinePin` on its schema,
/// `SplitStructPin` / `RecombineStructPin` on its editor) and they are one
/// question in two directions about one address, so this is one verb with an
/// address and a direction rather than four entry points.
///
/// ⚠ The member words are the taxonomy's own, and **a `const` cannot project
/// them**: `NodeKind::composition` allocates a `Vec<Port>` with `String` names,
/// so there is nothing const-evaluable to build this from. Written out, and
/// then held to the taxonomy by `r1914_the_published_pin_addresses_are_the_
/// taxonomys_members` — which is the shape this project reaches for when a
/// derivation is not available: compare against the derivation's OUTPUT rather
/// than re-spell the rule and hope.
const PIN_PARTS: [&str; 2] = ["host", "service"];

/// The addresses `split_pin` will act on, as a client reads them.
///
/// `dial` and `accept` are the two pins this card draws; `accept.host` is the
/// host member of the accept pin once it has come apart. A dotted address and
/// not a second argument, because it is ONE address — the crate's
/// [`PortPath`] — and giving it two spellings on the wire is how a client comes
/// to believe there are two things.
const PIN_ADDRESSES: [&str; 6] = [
    "dial",
    "accept",
    "dial.host",
    "dial.service",
    "accept.host",
    "accept.service",
];

/// The closed set of fault arms the `inject` verb accepts.
///
/// ★ Projected from `DefectKind::ALL` in a `const` block rather than written
/// out, so a fourth arm is a build failure here instead of a silently short
/// declaration — R1630's ratchet, applied to a domain this screen publishes.
/// ★★★★★ R1925 — the `section` verb's command words.
///
/// One list, read by the schema declaration and by the refusal a wrong word
/// meets, so an agent that discovers them by asking and one that discovers them
/// by being refused learn the same three. Two spellings of a closed set is the
/// shape R1678 removed for the reset scopes.
const SECTION_COMMANDS: [&str; 3] = ["add", "fold", "remove"];

const FAULT_KINDS: [&str; fault_injection::DefectKind::ALL.len()] = {
    let mut out = [""; fault_injection::DefectKind::ALL.len()];
    let mut at = 0;
    while at < fault_injection::DefectKind::ALL.len() {
        out[at] = fault_injection::DefectKind::ALL[at].wire();
        at += 1;
    }
    out
};

/// The faults the selected node's own settings admit, DERIVED from its form.
///
/// ★★★★★ R1853 — the whole point of this function is that there is no list.
/// `pinion_core::widgets::fault_injection::injectable` reads the declared
/// [`FieldType`] of every row and
/// asks the encoder whether the value it would offer really produces the arm it
/// claims — so a field added to `node_form` appears here with nothing edited,
/// and a field whose shape admits nothing contributes nothing.
///
/// ⚠ That shape is not a preference. This workspace has paid four times for the
/// opposite one — a correct gate over a population written down beside the thing
/// it was about (R1738, R1784, R1795, R1798) — and a hand-kept fault list beside
/// a declaration that decides the answer would be the fifth.
fn fault_rows(state: &LabState) -> Vec<Injection> {
    selected_form(state)
        .as_ref()
        .map(fault_injection::injectable)
        .unwrap_or_default()
}

/// What the panel says it cannot derive, and why — in the framework's own words.
///
/// ★★★★★ An absence nobody names is indistinguishable from an oversight. This
/// panel offers three kinds of configuration fault next to a tool whose whole
/// subject is a network, so a reader will take it as claiming those are the
/// faults there are. [`Scope::World`] is where the boundary is stated, and this
/// is where the screen shows it.
fn fault_scope_notes() -> Vec<(&'static str, String)> {
    Scope::ALL
        .into_iter()
        .filter(|scope| !scope.injectable())
        .map(|scope| {
            (
                scope.wire(),
                format!(
                    "{} faults are not offered — {}",
                    scope.wire(),
                    scope.because()
                ),
            )
        })
        .collect()
}

/// The same boundary as one sentence, for a reader who hears the panel rather
/// than sees it.
fn fault_scope_note() -> String {
    fault_scope_notes()
        .into_iter()
        .map(|(_, sentence)| sentence)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The panel: a heading, one row per derived fault, and the boundary it cannot
/// cross.
fn fault_panel(state: &LabState, top: u32, ink: Ink) -> Vec<Scene> {
    let rows = fault_rows(state);
    let width = inspector_body_w();
    let line = pinion_core::containment::line_box(FAULT_PX);
    let head_h = pinion_core::containment::line_box(FONT_SMALL);
    let notes = fault_scope_notes();
    // ★★★★★ THE BOUNDARY SITS ABOVE THE OFFERS, and that is a measurement
    // rather than a taste: with the scope lines last, R1853's own gate found
    // only ONE of the two painted — the offer list had grown until the second
    // fell below the inspector's fold. A statement of what is *not* here is
    // worthless at the mercy of how much *is*, so it goes where the row count
    // cannot move it.
    let scope_top = top + 10 + head_h + 8;
    let rows_top = scope_top + u32::try_from(notes.len()).unwrap_or(0) * line + 8;
    let height = rows_top - top + u32::try_from(rows.len()).unwrap_or(0) * FAULT_LINE_H + 8;
    // ★★★★★ R1857 — every tag below comes from `spec::FAULT_PANEL`, which is
    // where the screen's other elements are declared. R1853 spelled them here
    // and nowhere else, so the specification did not know the panel existed and
    // the backward gate reported twenty-eight invented elements the first time
    // anything asked about the whole screen.
    let panel = &spec::FAULT_PANEL;
    let mut out = vec![
        box_at(
            panel.tag,
            Rect::new(PAD, top, width, height),
            ink.raised,
            Some(ink.outline_2),
            8,
        ),
        quiet(
            tagged_label(
                panel.head,
                format!("fault injection — {} from this node's settings", rows.len()),
                Rect::new(PAD + 10, top + 10, width - 20, head_h),
                FONT_SMALL,
                ink.text,
            ),
            Silence::name_of(panel.tag),
        ),
    ];
    for (n, row) in rows.iter().enumerate() {
        let y = rows_top + u32::try_from(n).unwrap_or(0) * FAULT_LINE_H;
        // ★ The ink is the framework's verdict, not this screen's opinion:
        // `Injection::blocks` delegates to `ConfigDefect::blocks`, so a fault
        // that only warns cannot be painted as one that stops a launch.
        let ink_for = if row.blocks() { ink.err } else { ink.warn };
        out.push(box_at(
            &panel.row(n),
            Rect::new(PAD + 8, y, width - 16, FAULT_LINE_H - 4),
            ink.surface,
            Some(ink_for),
            6,
        ));
        out.push(quiet(
            tagged_label(
                &panel.what(n),
                format!("{} · {}", row.key, row.kind.wire()),
                Rect::new(PAD + 16, y + 2, width - 32, line),
                FAULT_PX,
                ink_for,
            ),
            Silence::name_of(panel.row(n)),
        ));
        out.push(quiet(
            tagged_label(
                &panel.badge(n),
                // ★ HOT/RESTART comes from the FIELD. Every offer has one, since
                // the settings' third fault — a key the declaration lacks — is
                // not offered at all; `None` here would be a badge about a row
                // that does not exist.
                row.applies
                    .map_or_else(|| "form".to_string(), |applies| applies.wire().to_string()),
                Rect::new(PAD + 16, y + 2 + line, 60, line),
                FAULT_PX,
                ink.text_3,
            ),
            Silence::part_of(panel.row(n)),
        ));
        out.push(quiet(
            tagged_label(
                &panel.why(n),
                row.admitted_by.clone(),
                Rect::new(PAD + 80, y + 2 + line, width - 96, line),
                FAULT_PX,
                ink.text_3,
            ),
            Silence::part_of(panel.row(n)),
        ));
    }
    // ★★★★★ One run per scope the panel does NOT offer, derived from
    // `Scope::ALL` rather than written out: a fourth scope would be named here
    // without this function being edited, and a scope that became injectable
    // would stop being named. An absence stated by derivation cannot fall out
    // of step with the boundary it states.
    for (n, (wire, sentence)) in notes.iter().enumerate() {
        out.push(quiet(
            tagged_label(
                &panel.scope(wire),
                sentence.clone(),
                Rect::new(
                    PAD + 10,
                    scope_top + u32::try_from(n).unwrap_or(0) * line,
                    width - 20,
                    line,
                ),
                FAULT_PX,
                ink.text_3,
            ),
            Silence::part_of(panel.tag),
        ));
    }
    out
}

/// What the restart note says — one derivation, read by the paint and by the
/// announcement it is the words of.
fn restart_note(form: &ConfigForm) -> String {
    let pending = form.pending_restart();
    if pending.is_empty() {
        let hot: Vec<&str> = form
            .fields()
            .iter()
            .filter(|f| f.applies() == Applies::Hot)
            .map(pinion_core::widgets::config_form::ConfigField::key)
            .collect();
        format!("only {} reaches a running node", hot.join(", "))
    } else {
        format!(
            "{} edited; restart to apply",
            pending
                .iter()
                .map(|f| f.key())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// What a host frame's tab says. **One derivation, read by the paint and by the
/// announcement** — R1692 found them to be two: the tab painted `host-a · core`
/// while the group announced `host host-a — core`, so the caption declared
/// itself that node's NAME and the node said something else. A speech-input
/// user reading the tab aloud reaches nothing, which is what WAI-ARIA's
/// label-in-name is about, and R1691's census could not see it: it checks that
/// a redirect ARRIVES somewhere that speaks, not that what arrives is what was
/// painted.
fn frame_caption(name: &str, gist: &str) -> String {
    if gist.is_empty() {
        name.to_owned()
    } else {
        format!("{name} · {gist}")
    }
}

/// What the inspector's role line says: the card's role and the host it starts
/// on. One derivation, read by the paint and by the announcement.
fn identity_caption(state: &LabState, node: NodeId) -> String {
    let role = state.role_of(node).unwrap_or(Role::Peer);
    let frame = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.node(node))
        .and_then(|n| n.parent)
        .and_then(|p| state.frames.borrow().get(&p).cloned())
        .unwrap_or_else(|| "unframed".to_owned());
    format!("{} · frame {frame}", role.name())
}

/// What the inspector's degree pill says.
fn degree_caption(state: &LabState, node: NodeId) -> String {
    let (inbound, outbound) = state.degree(node);
    format!("{inbound} inbound · {outbound} outbound")
}

/// ★★★ R1706 — what the selection-count chip says: how many cards are picked,
/// and which of them this panel is showing.
///
/// # Why it names the leader and not only the count
///
/// Because the count alone leaves the reader with the question it raises. Six
/// cards are outlined on the canvas and one panel is open; "6 selected" invites
/// "…of which, this one?" and the answer is already on screen a few pixels
/// above, in the identifier. Naming it here closes the loop in the one place a
/// person is looking when they wonder.
///
/// # The reference's own chip claims more than it does
///
/// It reads *N selected · common fields edited together*, and its field-setter
/// takes the leader's identifier — measured in the behaviour prototype, where
/// the write goes to the single active node and nothing fans it out. So the
/// second half of that sentence is a promise the prototype does not keep, and
/// this screen does not repeat it: every gate here exists because *a screen
/// that advertises an operation it does not answer* is the defect a person
/// reported over and over on this tool. The count and the leader are both true.
/// Editing many at once is a real operation and can have a row in the table on
/// the day it works.
fn selection_caption(state: &LabState) -> String {
    let selection = state.selection.get();
    let leader = selection
        .active()
        .map_or_else(|| "none".to_owned(), |node| state.name_of(*node));
    if selection.is_empty() {
        "nothing selected".to_owned()
    } else if selection.is_multiple() {
        format!("{} selected · showing {leader}", selection.len())
    } else {
        format!("1 selected · {leader}")
    }
}

/// Who the inspected node is: its identifier, its role and frame, and how many
/// links reach it.
fn inspector_identity(state: &LabState, node: NodeId, ink: Ink) -> Vec<Scene> {
    let name = state.name_of(node);
    let mut parts = vec![
        tagged_label(
            "lab.inspector.id",
            name,
            Rect::new(PAD, 44, 180, 20),
            18,
            ink.text,
        ),
        tagged_label(
            "lab.inspector.role",
            identity_caption(state, node),
            Rect::new(PAD, 68, 260, 13),
            FONT_SMALL,
            ink.text_3,
        ),
        box_at(
            "lab.inspector.degree",
            Rect::new(PAD, 86, inspector_body_w(), 24),
            ink.accent_soft,
            Some(ink.accent_line),
            8,
        ),
        quiet(
            tagged_label(
                "lab.inspector.degree.text",
                degree_caption(state, node),
                Rect::new(PAD + 10, 92, 220, 13),
                FONT_SMALL,
                ink.accent,
            ),
            Silence::name_of("lab.inspector.degree"),
        ),
        box_at(
            "lab.inspector.selcount",
            Rect::new(PAD, SEL_COUNT_Y, inspector_body_w(), SEL_COUNT_H),
            ink.accent_soft,
            Some(ink.accent_line),
            8,
        ),
        quiet(
            tagged_label(
                "lab.inspector.selcount.text",
                selection_caption(state),
                Rect::new(PAD + 10, SEL_COUNT_Y + 5, inspector_body_w() - 20, 13),
                FONT_SMALL,
                ink.accent,
            ),
            Silence::name_of("lab.inspector.selcount"),
        ),
    ];
    // ★★ R1682 — the node's-life row. Painted for a selected card only, which
    // is the same condition the hit test asks: an act on "the selected card"
    // with no card selected is a button that cannot mean anything.
    let (collapsed, disabled) = card_switches(state, node);
    let pins_away = pins_are_away(state, node);
    for act in NodeAct::ALL {
        let seat = act.local_seat();
        // Delete is the one that cannot be undone, so it is the one drawn in
        // the warning ink — the reference does the same, and a row of three
        // identical buttons where the third destroys work is a row that
        // invites the wrong press.
        let (fill, edge, text) = match act {
            NodeAct::Delete => (ink.surface, ink.warn, ink.warn),
            _ if act == NodeAct::Collapse && collapsed
                || act == NodeAct::Disable && disabled
                || act == NodeAct::Pins && pins_away =>
            {
                (ink.accent_soft, ink.accent_line, ink.accent)
            }
            _ => (ink.raised, ink.outline, ink.text_2),
        };
        // ★★★★★ R1794 — through `captioned`, which measures the word and centres
        // it. This drew the box and then a SIBLING label spanning
        // `seat.w - 12`, so the shaper laid `collapse`, `switch off` and
        // `delete` `Start`-aligned at the left of a rectangle far wider than
        // them. A reader named all three; no gate here could, because a gate
        // that compares rectangles cannot see where glyphs sit inside one.
        let mut style = BoxStyle::filled(fill).with_corner_radius(6);
        style = style.with_border(Border::new(edge, 1));
        let (scene, _) = captioned(
            act.tag(),
            seat,
            style,
            &caption::Caption::new(
                act.word(collapsed, disabled, pins_away),
                run_style(FONT_SMALL, text),
            )
            .centred()
            .padding(6, 0)
            // The seat's own name says the word; a second stop reading it
            // back would be two stops for one fact.
            .silent(Silence::name_of(act.tag())),
            // The hit test resolves these by rectangle, as `box_at` did.
            caption::Pointer::Transparent,
        );
        parts.push(scene);
    }
    parts
}

/// ★★★ R1683 — the one text field, and the seat that opens it on the name.
///
/// The field is the framework's own (`TextEditState` + the text-field painter),
/// so this screen gets the caret, the selection, the clipboard, the undo stack
/// and the IME composition path without writing any of them — and gets them the
/// same way the sibling node editor does, which is the fourth call site of the
/// lifted edit keymap rather than a fifth implementation of one.
fn inspector_edit(state: &LabState, ink: Ink) -> Vec<Scene> {
    let (box_rect, seat, key_seat) = rename_row();
    let editing = state.editing.get();
    let mut parts = Vec::new();
    // ★ R1684 — the shut box is drawn whenever the field is not standing on
    // it, which now includes the case where the field is open on a FORM ROW
    // somewhere below. Painting nothing there would leave a hole in the head of
    // the inspector while a person types further down the pane.
    if !matches!(editing, Some(Editing::Name(_) | Editing::Key(_))) {
        parts.push(box_at(
            "lab.inspector.name",
            box_rect,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        // ★★★★★ R1859 — DERIVED from the face, not written down. A reader
        // reported this run by name: *"'type a name or a key' 는 아예 아래
        // 글씨가 일부 잘려서 보여"*. Measured through the wire the debt asked
        // for: `h=13 px=11 ink_h=15 over_h=2 short_by=5` — the box was five
        // pixels short of `line_box(11)`, so the descenders of `y`, `p` and `y`
        // had nowhere to go. `line_rect_in` gives a box that holds one line of
        // this face AND centres it in the seat, which closes the reader's other
        // sentence about this same row in the same call.
        parts.push(label(
            "type a name or a key",
            pinion_core::containment::line_rect_in(
                box_rect,
                box_rect.x + 8,
                box_rect.w.saturating_sub(12),
                FONT_SMALL,
            ),
            FONT_SMALL,
            ink.text_3,
        ));
    }
    let word = if editing.is_some() { "apply" } else { "rename" };
    parts.push(box_at(
        "lab.inspector.rename",
        seat,
        ink.accent_soft,
        Some(ink.accent_line),
        6,
    ));
    // ★ R1859 — the same derivation, and the same reader's other sentence:
    // *"'rename','+key' 같은 버튼은 여전히 바닥에 붙어있어"*. A hand-picked
    // `+6` into a 20-pixel seat put an 13-pixel box 6 from the top and 1 from
    // the bottom; centring a box that holds the face puts it 1 from each.
    parts.push(label(
        word,
        pinion_core::containment::line_rect_in(
            seat,
            seat.x + 8,
            seat.w.saturating_sub(12),
            FONT_SMALL,
        ),
        FONT_SMALL,
        ink.accent,
    ));
    // The second target. A seat of its own rather than a mode on the first,
    // because "rename this card" and "give it a configuration path it does not
    // have" are different requests and a person should not have to know that
    // one button means both.
    parts.push(box_at(
        "lab.inspector.addkey",
        key_seat,
        ink.raised,
        Some(ink.outline),
        6,
    ));
    parts.push(label(
        "+ key",
        pinion_core::containment::line_rect_in(
            key_seat,
            key_seat.x + 8,
            key_seat.w.saturating_sub(12),
            FONT_SMALL,
        ),
        FONT_SMALL,
        ink.text_2,
    ));
    parts
}

/// The three seats [`inspector_edit`] paints, for the gate that holds them to
/// the rule.
///
/// ★★★★★ R1859 — named here rather than re-derived in the test, because the
/// surface being gated has to be the surface being painted. A gate that listed
/// these by tag would pass the day one of them was renamed, which is the shape
/// `debt-a-published-wire-name-is-checked-only-by-a-demo` records one level up.
#[cfg(test)]
pub(crate) fn rename_row_seats() -> [Rect; 3] {
    let (box_rect, seat, key_seat) = rename_row();
    [box_rect, seat, key_seat]
}

/// ★★★ R1684 — the live field itself, painted **last** so it stands over
/// whatever it is editing.
///
/// Split out of [`inspector_edit`] for one reason, and it is a reason R1681.3
/// wrote down next door: paint order and hit order are different questions. The
/// field's rectangle can now be a form row's control, and the form is painted by
/// the framework after the head of the inspector — so a field composed with the
/// head would be drawn UNDER the row it is standing on and a person would type
/// into something they cannot see.
///
/// Answers `None` while the field is shut, so the caller pushes nothing rather
/// than an empty container: an empty container is still a node the layout walks
/// and a tag census counts.
fn inspector_field(state: &LabState, field: (TextFieldState, u32), theme: &Theme) -> Option<Scene> {
    let what = state.editing.get()?;
    let rect = edit_box(state);
    let style = edit_field_style(rect);
    // The label is what the box is FOR, so it says the target rather than a
    // constant: a screen reader on a field standing over a form row would
    // otherwise announce every row of the form as "name".
    let name = what.wire();
    Some(Scene::Container(
        ContainerNode::new(vec![tf_paint::view_field(
            EDIT_TAG, field.0, field.1, theme, &style, &name,
        )])
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    ))
}

/// Which byte of the open field a window point is on, or `None` when the field
/// is shut, unfocused, or the point is outside it (R1684).
///
/// The one hit-test funnel the press hook and the drag hook share: two of them
/// would let a drag select to a different byte than the press caret landed on,
/// which is the reason the sibling text-field bindings have exactly one too.
/// The rectangle comes from the painted scene, so it is the box a person sees.
fn field_byte_at(
    interaction: TextFieldState,
    scene: &Scene,
    focused: Option<&str>,
    x: f32,
    y: f32,
) -> Option<usize> {
    if focused != Some(EDIT_TAG) {
        return None;
    }
    let rect = pinion_shell::rect_for_tag(scene, EDIT_TAG)?;
    // Compared in the pointer's own units rather than by casting it to the
    // rectangle's: a cast would round a point just outside the left edge INTO
    // the box, and a press half a pixel above it out of one it is in.
    //
    // ★ The containment question is THIS screen's, not the helper's: every
    // press here is routed to one root external, so "was it in the box" cannot
    // be answered by the tag comparison the sibling bindings use.
    if !rect.contains_point(x, y) {
        return None;
    }
    tf_paint::byte_for_scene_point(
        EDIT_TAG,
        interaction,
        scene,
        x,
        y,
        &use_theme(THEME_TAG).theme_animated(),
        &edit_field_style(rect),
    )
}

/// How the one field is drawn at a given rectangle.
///
/// ★★ R1684 — a function because the PAINT and the click-to-caret hit test
/// must resolve against the same shaping: `byte_for_field_point` re-shapes the
/// `(text, style, width)` the painter used and asks parley which glyph a point
/// landed on, so a second style here would put the caret on a different letter
/// from the one under the cursor.
fn edit_field_style(rect: Rect) -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle {
        field_w: rect.w,
        field_h: rect.h,
        field_pad: 5,
        font_size_px: FONT_SMALL + 1,
        ..tf_paint::TextFieldStyle::m3_filled()
    }
}

/// The pane the identity block and the framework-painted form sit in.
///
/// The two are **siblings**, not nested: the form's geometry is in window
/// coordinates, so putting it inside a pane that is itself absolutely placed
/// would offset it twice — the R1648 defect, and the reason the painter carries
/// its origin.
fn inspector_pane(
    state: &LabState,
    field: (TextFieldState, u32),
    theme: &Theme,
    ink: Ink,
    mut children: Vec<Scene>,
) -> Scene {
    let rect = inspector_rect();
    // The form. Everything below this line is the framework's painter — the
    // rows, the type badges, the applies badges, the defects on their rows and
    // the chips that add a key.
    let form = selected_form(state).unwrap_or_default();
    // ★ R1662 — the PANE-LOCAL geometry: the form now lives inside the
    // inspector's scrolling body, so it is placed in the body's frame and the
    // window-coordinate consumers derive theirs from it.
    let geometry = inspector_geometry_local(state);
    // ★★★★★ R1732 — the picking is handed to the painter as well as to the
    // layout, because the two halves answer different questions: where the
    // options landed, and where the reader is among them.
    let picking = state.picking.get();
    let painted = view_config_form_showing(
        "lab.form",
        &form,
        &geometry,
        theme,
        picking.as_ref().map(|(_, picker)| picker),
    );

    let note_y = geometry.origin.1 + geometry.height + 16;

    children.push(box_at(
        "lab.inspector.note",
        Rect::new(PAD, note_y, inspector_body_w(), 40),
        ink.raised,
        Some(ink.warn),
        8,
    ));
    children.push(quiet(
        tagged_label(
            "lab.inspector.note.text",
            restart_note(&form),
            Rect::new(PAD + 10, note_y + 8, inspector_body_w() - 20, 26),
            10,
            ink.text_2,
        ),
        Silence::name_of("lab.inspector.note"),
    ));
    // ★★★★★ R1853 — the fault-injection panel, under the note, and every row of
    // it is DERIVED from the form above rather than listed here. See
    // `fault_rows` for the derivation and why a written list would be the
    // fourth of a class this workspace has already paid for four times.
    children.extend(fault_panel(state, note_y + 40 + 16, ink));
    // ★ R1662 — the form is now a CHILD of the pane body rather than a sibling
    // of the pane. It was a sibling because its geometry was in window
    // coordinates and nesting it under an absolutely-placed pane offset it
    // twice (R1648); with the geometry stated in the body's own frame that
    // reason is gone, and being a sibling is what kept the form from
    // scrolling. A list field has no bounded length, so no fixed pane floor is
    // enough — measured at R1652.1, a six-element list puts two chips and the
    // note below the window ([[debt-the-node-lab-panes-do-not-scroll]]).
    children.push(painted);
    // ★★ R1684 — the field goes ON TOP of the form, because it now stands over
    // a form row.
    children.extend(inspector_field(state, field, theme));
    // ★ R1909 — the panel wrapper moved to `inspector`, which now calls
    // `side_panel` BEFORE any of this is built. What this returns is the body.
    quiet(
        scroll_pane(
            &state.inspector_scroll,
            side_panel_content(rect),
            (0, PAD),
            // Every press on this screen belongs to the one root `External`
            // that does the screen's own hit test, so the pane must be
            // invisible to the router (R1655).
            PanePointer::PassesThrough,
            children,
        ),
        Silence::layout("scrolls the inspector; the pane above it is what a reader lands on"),
    )
}

fn view(field: (TextFieldState, u32), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_lab_state();
    let ink = ink(&theme);
    let win = window_size();

    // ★ R1714 — the window's own pan is NOT wrapped here. `SHRINK` declares it
    // and the framework builds it, once, for every binding that says so — see
    // `pinion_core::shrink::pan`. A screen that wrapped its own would be the
    // second place the rule lives, and the first one to drift.
    // ★★★★★ R1725 — the rail is built only where this screen is the one
    // providing it. Not painted-and-hidden and not zero-width: a page inside an
    // application that already has a navigation must not contribute a second
    // one to the tree either, and the only way that is a property rather than a
    // rule is for the node not to exist.
    // ★★★★★ R1822 — and the same for the application bar, which is the pane
    // that paragraph was written next to and did not cover. Mounted, every one
    // of the three things this bar carries is already on this screen: the
    // graph's name is `lab.toolbar.title`, the run state is the run seat's own
    // caption, and the words *node lab* restate what the host's bar and the
    // rail's active seat both say. Not a different sentence — the same one,
    // twice, in a strip the canon does not have.
    let mut panes = vec![];
    if draws_own_app_bar() {
        panes.push(app_bar(&state, ink));
    }
    if draws_own_rail() {
        panes.push(rail(ink));
    }
    panes.extend([
        palette(&state, ink),
        toolbar(&state, ink),
        canvas(&state, ink),
        inspector(&state, field, &theme, ink),
    ]);

    Scene::Container(
        ContainerNode::new(panes)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(ink.bg))
            // The root fills the surface the shell gave it, so a resize reflows
            // instead of leaving the rest of the window unpainted.
            .with_layout(LayoutStyle::new().with_size(Size::px(win.0, win.1))),
    )
}

// ── The wire ────────────────────────────────────────────────────────────────

/// ★★★★★ R1714 — and it no longer keeps a size.
///
/// R1656 gave this oracle a `surface` field because `External::pointer_move`
/// hands a FRACTION of the widget and not the widget's rectangle, so a screen
/// that wants pixels has to hold the basis itself. R1684.4 made the framework
/// answer that (`pinion_core::external::surface_size`) and left the field,
/// because the multiplication was still written here. This round moved the
/// multiplication too — `layout_point` — and the field became something written by
/// every resize and read by nobody, which is the shape a close audit deletes.
///
/// That is [[debt-an-external-reads-a-fraction-without-its-basis]] closed on
/// this screen: there is no second copy of the basis left to drift.
struct LabOracle {
    state: Option<Rc<LabState>>,
}

impl core::fmt::Debug for LabOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LabOracle").finish_non_exhaustive()
    }
}

impl LabOracle {
    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<LabState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<LabState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected("the lab has not been attached"))
    }

    fn text(args: &IntrospectValue) -> Result<String, InvokeError> {
        match args {
            IntrospectValue::Text(s) => Ok(s.clone()),
            // ★★★★ R1720 — the KIND, not the `Debug` spelling. This said
            // `Int(-987654) is not text`, and R1720's own gate is what read it:
            // once every refusal reaches the person, a Rust value's syntax in
            // one is a thing somebody has to read. R1699 fixed this shape on
            // the person's channel and left it here, because until this round
            // nothing on the agent's channel had a rule.
            other => Err(InvokeError::rejected(format!(
                "this action takes text and was given {}",
                other.kind()
            ))),
        }
    }

    /// The drawn link a caller named, refusing one that is not there (R1681).
    ///
    /// One parser, so `select_link`, `delete_link` and `relink` cannot disagree
    /// about what a link is called on the wire.
    ///
    /// ★ Two spellings, and the second is the one that matters: a link id is
    /// **minted in seeding order**, so an argument written as `3` is a caller
    /// asserting something about the order this screen happened to author its
    /// opening graph in. `P-01>R-01` names the same link by what it *is*. The
    /// pair form resolves against drawn links first and reported ones second,
    /// which is the order the canvas's own hit test uses.
    fn link_id(state: &LabState, raw: &str) -> Result<LinkId, InvokeError> {
        if let Some((from, to)) = raw.trim().split_once('>') {
            let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim())) else {
                return Err(InvokeError::rejected(format!(
                    "{:?} or {:?} is not a node on the canvas",
                    from.trim(),
                    to.trim()
                )));
            };
            return state
                .doc
                .borrow()
                .tree(ROOT)
                .and_then(|t| {
                    t.links()
                        .iter()
                        .find(|l| l.from.node == a && l.to.node == b)
                        .map(|l| l.id)
                })
                .ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "no link is drawn from {} to {}",
                        from.trim(),
                        to.trim()
                    ))
                });
        }
        let id: u32 = raw
            .trim()
            .parse()
            .map_err(|_| InvokeError::rejected(format!("{raw:?} is not a link id")))?;
        state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.link(LinkId(id)).map(|_| LinkId(id)))
            .ok_or_else(|| InvokeError::rejected(format!("no link {id} is drawn")))
    }

    /// The card a caller named, refusing one that is not on the canvas
    /// (R1682).
    ///
    /// One parser, so the four verbs of a node's life cannot disagree about
    /// what a card is called or how a wrong name is refused — the same reason
    /// [`Self::link_id`] exists next door.
    fn card(state: &LabState, name: &str) -> Result<NodeId, InvokeError> {
        state
            .node_of(name)
            .ok_or_else(|| InvokeError::rejected(format!("no node is called {name:?}")))
    }

    /// ★★ R1684 — a form row, and optionally one element of it, in the
    /// spelling `editing.target` reads back: `<key>` or `<key>[<n>]`.
    ///
    /// One parser for the wire's grammar, and it is the READ-BACK spelling
    /// rather than a second one — an agent that reads `value:listen.endpoints[2]`
    /// out of the field's own slot can hand exactly that back to `edit` and
    /// reach the same row. Written twice they drift, and the drift is
    /// invisible until somebody automates a loop over the form.
    ///
    /// # Errors
    ///
    /// A bracket that never closes, or an index that is not a number. Both are
    /// refused rather than read as part of the key: a key silently containing a
    /// `[` would make the refusal say "no such row" about a row that is there.
    fn row_target(spelled: &str) -> Result<(String, Option<usize>), InvokeError> {
        let Some((key, rest)) = spelled.split_once('[') else {
            return Ok((spelled.to_owned(), None));
        };
        let number = rest.strip_suffix(']').ok_or_else(|| {
            InvokeError::rejected(format!(
                "{spelled:?} opens an element index and never shuts it"
            ))
        })?;
        let at = number
            .trim()
            .parse::<usize>()
            .map_err(|_| InvokeError::rejected(format!("{number:?} is not an element number")))?;
        Ok((key.to_owned(), Some(at)))
    }

    /// A link on either layer, in the spelling `selected_link` reads back.
    fn link_pick(state: &LabState, raw: &str) -> Result<LinkPick, InvokeError> {
        if let Ok(drawn) = Self::link_id(state, raw) {
            return Ok(LinkPick::Authored(drawn));
        }
        let Some((from, to)) = raw.split_once('>') else {
            return Self::link_id(state, raw).map(LinkPick::Authored);
        };
        let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim())) else {
            return Err(InvokeError::rejected(format!(
                "{:?} or {:?} is not a node on the canvas",
                from.trim(),
                to.trim()
            )));
        };
        state
            .doc
            .borrow()
            .observations(ROOT)
            .into_iter()
            .find(|o| o.from.node == a && o.to.node == b)
            .map(|o| LinkPick::Observed(o.from, o.to))
            .ok_or_else(|| {
                InvokeError::rejected(format!(
                    "nothing was reported from {} to {}",
                    from.trim(),
                    to.trim()
                ))
            })
    }
}

/// ★★★ R1706 — the hosts an agent may name, built FROM the specification
/// table rather than listed beside it.
///
/// The same discipline `Scope::WIRE_NAMES` keeps a few hundred lines up: a
/// hand-written copy of a closed vocabulary still compiles when the thing it
/// copies gains an entry, and then the wire offers a set the canvas does not
/// draw.
const HOST_NAMES: [&str; spec::FRAMES.len()] = {
    let mut out = [""; spec::FRAMES.len()];
    let mut n = 0;
    while n < spec::FRAMES.len() {
        out[n] = spec::FRAMES[n].name;
        n += 1;
    }
    out
};

/// ★★★★★ R1885 — the builds the `build` action offers, derived from
/// [`Stack::ALL`] for the reason [`HOST_NAMES`] is derived from the frame table:
/// a hand-written copy of a closed vocabulary still compiles when the thing it
/// copies gains an entry, and then the wire offers a set the screen cannot set.
const BUILD_WORDS: [&str; Stack::ALL.len()] = {
    let mut out = [""; Stack::ALL.len()];
    let mut n = 0;
    while n < Stack::ALL.len() {
        out[n] = Stack::ALL[n].word();
        n += 1;
    }
    out
};

const FIELDS: &[SchemaField] = &{
    [
        SchemaField::new("spec", "string"),
        // ★★★★★ R1918 — what the marks on this frame say about themselves, with
        // the region they are drawn under. The lab was the first screen to draw
        // a description (R1916) and the last to PUBLISH its register: while it
        // was the only one, a reader could only find out by hovering, and a
        // gate asking "does this page describe anything" had nothing to read.
        SchemaField::new("described", "json"),
        // ★★★★★ R1919 — what a reader is looking for, and every node that
        // answers with the way IN to it. The census's six search rows.
        SchemaField::new("searching", "string"),
        SchemaField::new("found", "json"),
        // ★★★★★ R1920 — which edits this screen would ALLOW, published as a
        // read rather than asked one card at a time: an agent choosing what to
        // act on needs the whole row before it acts on any of it.
        SchemaField::new("editable", "json"),
        // ★★★★★ R1921 — each card's authored colour and the faces derived from
        // it, published together so a client never re-derives the contrast rule.
        SchemaField::new("tints", "json"),
        // ★★★★★ R1922 — what this graph would ACCEPT, body by body, so an agent
        // deciding what to place reads the row before it places anything.
        SchemaField::new("accepts", "json"),
        // ★★★★★ R1923 — what each card says about itself, and WHICH of its two
        // sources said it, which is what the reference's bare string cannot say.
        SchemaField::new("notes", "json"),
        // ★★★★★ R1924 — for the picked link's consuming end, which cards would
        // take it and, for each that would not, the crate's own reason. Asked
        // BEFORE the hand lets go, which is the whole difference from finding
        // out by dropping.
        SchemaField::new("rewire", "json"),
        // ★★★★★ R1928 — what each card calls its own ports, and WHO chose each
        // name: the kind's declaration, the item's authored label, or the
        // node's own answer. Published so the sentence a reader hears on a pin
        // and the name the model resolved can be checked against ONE answer
        // instead of two — the second spelling is exactly what this round
        // removed from the accessibility tree.
        SchemaField::new("port_names", "json"),
        // ★★★★★ R1927 — what is wrong with each card: the MODEL's own sentence
        // where it has one, whether this screen's walk names the card at all,
        // and whether the worst of it blocks. Published so the mark on the
        // canvas and the line in the gate panel can be checked against ONE
        // answer instead of two.
        SchemaField::new("wrong", "json"),
        // ★★★★★ R1926 — the colour every socket type of this taxonomy is drawn
        // in, and the colour each pin on the canvas takes from it. Published so
        // a client reads the derivation instead of re-implementing it — the
        // duplication that had this screen colouring a split's halves by the
        // NODE's transport until this round.
        SchemaField::new("inks", "json"),
        // ★★★★★ R1925 — the sections this graph's own face is gathered into,
        // and what the framework answers when this screen asks for a section
        // switch. Published rather than left to a gesture because this screen
        // has no pixels for it: a definition's face is not on the reference
        // mock-up at all, so the honest surface is the one an agent reads.
        SchemaField::new("sections", "json"),
        // ★★★★★ R1742 — how much of the inspector specification this build is
        // showing, published beside the specification itself. `json` rather
        // than the `string` its neighbours use because it is the framework's
        // own shape: an agent asking two sections how much of themselves they
        // are must not have to parse two answers.
        SchemaField::new("conformance", "json"),
        SchemaField::new("graph", "string"),
        SchemaField::new("selected", "string"),
        SchemaField::new("selected_ids", "string"),
        SchemaField::new("selected_link", "string"),
        SchemaField::new("zoom", "int"),
        SchemaField::new("pan", "string"),
        SchemaField::new("running", "bool"),
        SchemaField::new("discovery", "bool"),
        SchemaField::new("cursor", "string"),
        SchemaField::new("verdict", "string"),
        SchemaField::new("gate", "string"),
        SchemaField::new("form", "string"),
        // ★★★★★ R1853 — the faults this node's own settings ADMIT, and the
        // boundary of what that derivation can reach.
        //
        // Two slots and not one: the offers change with the selected node and
        // the boundary does not, and a client that had to re-read the sentence
        // about link-level faults on every selection would be re-reading a
        // constant. The scopes are published so the absence is enumerable
        // rather than inferred from a list that simply lacks them.
        SchemaField::new("faults", "json"),
        SchemaField::new("fault_scopes", "json"),
        // ★★★★★ R1857 — and WHERE the panel is, by name: every address it
        // occupies right now, derived from `spec::FAULT_PANEL` and the two
        // slots above.
        //
        // Three slots and not two, because "what the offers are" and "where
        // they are on screen" are different questions and R1853 answered only
        // the first. A client that wanted to press an offer — the gap that
        // round's own carry names — had to guess the addresses from the paint,
        // and a checker that wanted to know the panel had painted the whole of
        // itself had to spell the shape a second time. That second copy is what
        // this round is repaying one level up.
        SchemaField::new("faults_roster", "json"),
        // ★★★★★ R1850 — and what the form is WILLING to hold, which `form`
        // cannot say. `form` lists the rows that are there; a reader deciding
        // whether to take one off needs to know whether it comes back, and a
        // key that is merely present and a key the catalogue can re-offer look
        // identical on the screen. Published as the pair a caller actually
        // asks for — the restorable rows and the ones a removal would end.
        SchemaField::new("catalogue", "string"),
        SchemaField::new("document", "string"),
        SchemaField::new("nodes", "string"),
        SchemaField::new("links", "string"),
        // ★ R1677 — declared, because since R1637 the declaration is a
        // PRECONDITION of dispatch and not a description of it: an arm added to
        // `query` without a line here answers `UnknownIntrospectPath`, which is
        // what these two did on their first drive.
        SchemaField::new("layout", "string"),
        // R1682 — the node's-life switches, per card.
        SchemaField::new("cards", "string"),
        // R1683 — what the one text field is doing, and what it holds.
        SchemaField::new("editing", "string"),
        SchemaField::new("frames", "string"),
        SchemaField::new("changed", "string"),
        SchemaField::new("roles", "string"),
        SchemaField::new("toast", "string"),
        // ★★★★★ R1719 — the same fact with its KIND on it. `toast` above is
        // this projected through `sentence()`, which is what a person reads and
        // what twenty-four existing readers ask for; this is the value, so an
        // agent can ask whether the screen refused without matching a prefix.
        // Two derivations of one record, never two records.
        SchemaField::new("said", "object"),
        // ★★★★★ R1790 — how long what is being said has left.
        SchemaField::new("saying", "json"),
        // ★★★★★ R1791 — **what the toolbar moved, and what is still on the row.**
        // The floor has an overflow control and no member that names what is
        // behind it, and each hidden action's own `isVisible` still answers
        // true — so a client asking what this toolbar can do right now is told
        // about controls a person cannot see and is not told about the ones a
        // press away. Both halves are here, and `short_by` is the third: how
        // much room the row still does not have, which is zero at every size
        // this screen declares it can be shown in.
        SchemaField::new("toolbar_overflow", "json"),
        // ★★ R1687 — what this screen has PRODUCED, which is not what it could
        // produce. `document` next to it answers the selected card's own
        // configuration; this answers the whole graph's, once somebody has
        // asked for it. Two questions, two names — the rule R1676 wrote down.
        //
        // ★★★ Named `produced` and not `export`, for two reasons the wire found
        // before a reader would have. The first is mechanical: `export` is an
        // ACTION below, and one address holding both channels makes "what does
        // this answer" depend on which verb you happened to use — the rule
        // written beside `zoom_by`, which the first draft of this line broke and
        // the server rejected as `PathIsAReadSlot`. The second is the real one:
        // this slot holds BOTH artifacts, so pressing `script` would have
        // changed a slot called `export`.
        SchemaField::new("produced", "string"),
        // ★★★ R1689 — the two file reads, and they are two different questions.
        // `archive` is what a save WOULD write, which is a function of the
        // screen right now; `stored` is what is on disk, which is a function of
        // whatever was saved last. A single slot would have made "has this been
        // saved" unanswerable — the thing the reference's own meter exists to
        // ask. Both are the whole archive text rather than a summary, which is
        // §2 #7: the saved graph is a value an agent reads without the process
        // that wrote it.
        //
        // ★ `archive`, not `graph`: `graph` is already this screen's read for
        // the graph's NAME, and the server refuses a second declaration of one
        // address. The compiler said so first — an unreachable arm.
        SchemaField::new("archive", "string"),
        SchemaField::new("stored", "string"),
        // ★★★ R1690 — **how much of the option surface this palette reaches**,
        // and how much of that surface's string half is pinned down. The
        // reference publishes both beside its operation list and its save
        // partition; these are the remaining two of its four self-censuses.
        //
        // Read slots rather than a painted number an agent has to parse back
        // out of a label: the pill on screen is derived from this, so a test
        // that compares the two is comparing a rendering with its source
        // instead of two independent claims.
        SchemaField::new("reach", "string"),
        SchemaField::new("strings", "string"),
        // ★★★★★ R1840 — and what those two are fractions OF. `reach` divides
        // by the surface this crate DECLARES, so it answers "how much of what
        // we wrote down", and a declaration that falls behind its target loses
        // leaves from the denominator and makes the figure RISE. This slot is
        // the other side: the paths the target itself declares, read from
        // `docs/analyzer-config-surface.json`, and what the two disagree about.
        // Beside `reach` rather than folded into it, because they are answers
        // to different questions and one number would average them.
        SchemaField::new("surface", "string"),
        SchemaField::action("select", "string"),
        SchemaField::action("select_link", "string"),
        SchemaField::action_with(
            "set_field",
            "string",
            ArgForm::Delimited('='),
            const {
                &[
                    SchemaArg::key("key", "string", "form"),
                    SchemaArg::open("value", "string"),
                ]
            },
        ),
        // ★★★★★ R1853 — inject a fault the settings admit, by path and arm. The
        // arm's domain is the CLOSED set the framework publishes; the path's is
        // the `faults` slot, so a client picks from what is offered rather than
        // guessing — and the value is the derivation's, never the caller's.
        SchemaField::action_with(
            "inject",
            "string",
            ArgForm::Delimited(':'),
            const {
                &[
                    SchemaArg::key("key", "string", "faults"),
                    SchemaArg::one_of("kind", "string", &FAULT_KINDS),
                ]
            },
        ),
        SchemaField::action("add_field", "string"),
        SchemaField::action("remove_field", "string"),
        // ★★★★★ R1732 — the collapsed roster, on the wire. Two members and not
        // one, because they answer different questions: `picking` is WHERE THE
        // READER IS and `pick` is what opens or shuts the thing they are in.
        //
        // The read is the member the reference toolkit's own chooser does not
        // have at any maturity: measured at 6.11.1, of its 123 members the only
        // two naming the highlight are signals, so an agent that was not
        // already subscribed cannot ask what committing would choose.
        SchemaField::new("picking", "string"),
        SchemaField::action_with(
            "pick",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("key", "string", "form")] },
        ),
        // ★★ R1716 — take a row over. Declared beside its twin because they are
        // the two halves of one question: who owns this row's value.
        SchemaField::action("author_field", "string"),
        // ★★★★★ R1925 — arrange this graph's own face. One verb with a command
        // word for R1678's reason below, and the vocabulary is DECLARED so an
        // agent reads the three rather than discovering them by rejection.
        SchemaField::action_with(
            "section",
            "string",
            ArgForm::Scalar,
            const {
                &[
                    SchemaArg::one_of("command", "string", &SECTION_COMMANDS),
                    SchemaArg::key("header", "string", "sections"),
                ]
            },
        ),
        // R1678 — the scope vocabulary is published, so an agent reads the five
        // rather than discovering them by rejection.
        SchemaField::action_with(
            "reset",
            "string",
            ArgForm::Scalar,
            const {
                &[SchemaArg::one_of(
                    "scope",
                    "string",
                    &ResetScope::WIRE_NAMES,
                )]
            },
        ),
        // `zoom_by`, not `zoom`: `zoom` is a declared READ, and one address
        // holding both channels makes "what does this answer" depend on which
        // verb you happened to use. The determinism switch needs no action at
        // all — it is a boolean somebody sets, so it is a WRITE on its read.
        SchemaField::action("zoom_by", "string"),
        // ★★ R1688 — the view's other two. Neither takes an argument: "frame the
        // graph" is a function of the graph and "go to the first problem" of the
        // verdict, and a verb that let a caller name a subset or an index would
        // be inventing a scope the screen has no affordance for.
        //
        // ★ They answer the sentence the toast shows, like `export` and
        // `script`, so an agent learns what a person would have learnt — which
        // for `fit` includes whether the whole graph actually went in.
        SchemaField::action("fit", "string"),
        SchemaField::action("go_to_problem", "string"),
        SchemaField::action("run", "bool"),
        // ★★★★★ R1789 — **the scenario**: what happens to this graph and when,
        // which the census recorded as having no authoring surface. `scenario`
        // reads the lanes, their entries, how long it is, where the playhead
        // stands and which acts exist; `schedule` places one; `unschedule`
        // takes one off; `advance` moves the playhead and answers WHAT IT
        // CROSSED — the query the reference's keyframe API has no equivalent
        // of (measured at 6.11: a scrub answers a value, never the entries a
        // step passed, so "stop that node at eight seconds" is inexpressible).
        SchemaField::new("scenario", "json"),
        SchemaField::action_with(
            "schedule",
            "string",
            ArgForm::Object,
            const {
                &[
                    SchemaArg::open("at", "number"),
                    // ★★★★★ R1844 — `act` is a DISCRIMINANT now, not just a
                    // closed vocabulary: choosing `check` brings a timeout and
                    // the other four bring nothing, and the case table says so
                    // rather than leaving a caller to discover it from a
                    // refusal. The table is derived from `Act` itself.
                    SchemaArg::one_of_with("act", "string", &scenario::ACT_CASES),
                    SchemaArg::key("target", "string", "cards").optional(),
                    SchemaArg::open("lane", "string").optional(),
                ]
            },
        ),
        SchemaField::action_with(
            "unschedule",
            "string",
            ArgForm::Object,
            const {
                &[
                    SchemaArg::open("at", "number"),
                    SchemaArg::open("lane", "string").optional(),
                ]
            },
        ),
        SchemaField::action("advance", "json"),
        // ★★★★★ R1866 — the regression pair: a fact and the act that makes it
        // possible. `record` takes no argument, because what it keeps is the
        // run that has happened — a verb that let a caller name a different one
        // would be inventing a history this screen does not have.
        SchemaField::new("regression", "json"),
        SchemaField::action("record", "json"),
        // ★★ R1687 — what leaves the screen. Neither takes an argument: the
        // plan is a function of the graph, and a verb that let a caller name a
        // subset would be inventing a scope the screen has no affordance for.
        // They answer the sentence the toast shows, so a caller learns what a
        // person would have learnt without a second read.
        SchemaField::action("export", "string"),
        SchemaField::action("script", "string"),
        // ★★★ R1689 — the file. `save_graph` and `clear_graph` take no
        // argument, and `open_graph` takes one that may be EMPTY: empty means
        // "the saved one", any other text is a graph handed over directly.
        // That is the reference's own shape (its box loads the saved copy when
        // you leave it blank) and it is one act with one argument rather than
        // two verbs for one thing — the second of which would be the one that
        // drifts.
        SchemaField::action("save_graph", "string"),
        SchemaField::action("open_graph", "string"),
        SchemaField::action("clear_graph", "string"),
        SchemaField::action_with(
            "connect",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("from", "string", "nodes"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        // ★★ R1682 — a node's own life. Three take just the card's name and
        // one takes the new name beside it, so only that one declares a
        // grammar; `collapse` and `disable` are toggles and answer the state
        // they left the card in.
        SchemaField::action("delete_node", "string"),
        SchemaField::action_with(
            "rename",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("node", "string", "nodes"),
                    SchemaArg::open("name", "string"),
                ]
            },
        ),
        SchemaField::action("collapse", "string"),
        SchemaField::action("disable", "string"),
        // ★★★★★ R1923 — write a note on a card, or take it away. `<card>,none`
        // clears it, and clearing is not the same as the kind having nothing to
        // say: the card falls back to its role's own line, which the `notes`
        // row then reports as `kind` rather than `authored`.
        SchemaField::action_with(
            "note",
            "string",
            ArgForm::Scalar,
            const {
                &[
                    SchemaArg::open("card", "string"),
                    SchemaArg::open("sentence", "string"),
                ]
            },
        ),
        // ★★★★★ R1921 — give a card a colour, or take its colour away.
        // `<card>,#rrggbb` or `<card>,none` — the second is not a special case
        // bolted on but the OTHER value the model holds, which is the whole
        // point of an Option: the reference needs a flag beside the channels
        // and its own copy operator has to remember to clear it.
        SchemaField::action_with(
            "tint",
            "string",
            ArgForm::Scalar,
            const {
                &[
                    SchemaArg::open("card", "string"),
                    SchemaArg::open("colour", "string"),
                ]
            },
        ),
        // ★★★★★ R1919 — look for a name across the document. Declared with an
        // OPEN argument, which is the honest shape here and worth saying why:
        // every other action on this screen names a closed vocabulary because
        // its argument addresses something that exists, and a needle addresses
        // nothing — it is what a reader typed. A `one_of` here would be a
        // roster of the graph as it is now, which is exactly what a search is
        // for finding out.
        SchemaField::action_with(
            "find",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::open("needle", "string")] },
        ),
        // ★★★★★ R1912 — put a card's pins away, or bring them back. The scope
        // vocabulary is CLOSED and built from `PIN_SCOPES` rather than spelled
        // here, so the words an agent is offered cannot drift from the ones the
        // verb accepts — the same rule `build` follows one field down, and the
        // reason a declaration is worth having at all (R1637: a declaration is
        // a precondition of dispatch, so an undeclared word is refused before
        // it reaches the verb).
        SchemaField::action_with(
            "put_away_pins",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("node", "string", "nodes"),
                    SchemaArg::one_of("scope", "string", &PIN_SCOPES),
                ]
            },
        ),
        // ★★★★★ R1914 — take a pin apart into its members, or put it back. The
        // address vocabulary is CLOSED and the pin names and member names are
        // both in it, so an agent is offered `accept.host` rather than left to
        // infer that a dot means something. A leading `-` folds instead of
        // splitting: one verb, one address, two directions — which is what the
        // engine's four commands are.
        SchemaField::action_with(
            "split_pin",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("node", "string", "nodes"),
                    SchemaArg::one_of("address", "string", &PIN_ADDRESSES),
                ]
            },
        ),
        // ★★★★★ R1885 — put a card on another build. The build comes from a
        // CLOSED vocabulary and it is built from `Stack::ALL` rather than
        // spelled here, so the words an agent is offered cannot drift from the
        // builds the screen can actually set — the same rule `move_frame`'s
        // hosts follow, and the reason a declaration is worth having at all.
        SchemaField::action_with(
            "build",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("node", "string", "nodes"),
                    SchemaArg::one_of("build", "string", &BUILD_WORDS),
                ]
            },
        ),
        // ★★★ R1706 — the frame gesture's agent half. The host comes from a
        // closed vocabulary because this screen's hosts ARE closed — the
        // specification names them and no operation makes another — and it is
        // built from that table rather than spelled here, so the words an agent
        // is offered cannot drift from the frames the canvas draws.
        SchemaField::action_with(
            "move_frame",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::one_of("host", "string", &HOST_NAMES),
                    SchemaArg::open("dx", "int"),
                    SchemaArg::open("dy", "int"),
                ]
            },
        ),
        // ★★ R1683 — the one text field's own verbs. `edit` declares the two
        // things this screen types into, so an agent reads the vocabulary
        // rather than discovering it by rejection.
        SchemaField::action_with(
            "edit",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::one_of("target", "string", &["name", "key"])] },
        ),
        SchemaField::action("type", "string"),
        SchemaField::action("apply", "string"),
        SchemaField::action("add_key", "string"),
        // ★★ R1681 — the other half of a link's life. Four verbs and not one
        // with a mode, because they take different arguments and answer
        // different refusals: what a caller has to say to delete a link and
        // what it has to say to re-aim one are not the same sentence.
        SchemaField::action("delete_link", "string"),
        SchemaField::action_with(
            "relink",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("link", "string", "links"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        SchemaField::action("set_endpoint", "string"),
        SchemaField::action_with(
            "adopt",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("from", "string", "nodes"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        SchemaField::new("observed", "string"),
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
        // ★★★★★ R1887 — **place a side panel**, the wire's half of the gesture
        // the header now offers. §2 #2 makes RPC the agent's primary path, so
        // this is not a lesser channel: a placement a person can change and an
        // agent cannot would be a screen an agent cannot drive.
        //
        // Both arms of the ask go through one verb rather than two, because
        // they are one question about one value — where does this panel sit —
        // and `place palette,fold` reads as what it does. The panel's domain is
        // `panes`, so a client picks a name the surface published rather than
        // guessing one.
        SchemaField::action_with(
            "place",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("panel", "string", "panes"),
                    SchemaArg::open("where", "string"),
                ]
            },
        ),
    ]
};

impl ExternalIntrospect for LabOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(FIELDS)
    }

    #[allow(clippy::too_many_lines, reason = "one arm per published read")]
    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("the lab holds no document yet"))?;
        let text = |s: String| Ok(IntrospectValue::Text(s));
        match path {
            "spec" => text(spec_json().to_string()),
            // ★★★★★ R1918 — what the marks on this frame say about themselves.
            "described" => Ok(IntrospectValue::Json(described_wire(state))),
            // ★★★★★ R1919 — what a reader is looking for, and what answers.
            "searching" => text(state.searching.get()),
            "found" => Ok(IntrospectValue::Json(found_wire(state))),
            // ★★★★★ R1920 — what an agent may do, before it does it.
            "editable" => Ok(IntrospectValue::Json(editable_wire(state))),
            // ★★★★★ R1921 — what colour each card is, and what that derives.
            "tints" => Ok(IntrospectValue::Json(tints_wire(state))),
            // ★★★★★ R1922 — what this graph would accept, before anything is put in it.
            "accepts" => Ok(IntrospectValue::Json(accepts_wire(state))),
            // ★★★★★ R1923 — what each card says about itself, and who said it.
            "notes" => Ok(IntrospectValue::Json(notes_wire(state))),
            // ★★★★★ R1924 — where the picked wire's end may be re-aimed, and
            // why each card that refuses it does.
            "rewire" => Ok(IntrospectValue::Json(rewire_wire(state))),
            "sections" => Ok(IntrospectValue::Json(sections_wire(state))),
            "inks" => Ok(IntrospectValue::Json(inks_wire(state))),
            "wrong" => Ok(IntrospectValue::Json(wrong_wire(state))),
            "port_names" => Ok(IntrospectValue::Json(port_names_wire(state))),
            // ★ R1742 — the SAME value the host publishes for this section, so
            // "one build, two placements" is a fact a client can check rather
            // than a claim this file makes.
            "conformance" => Ok(IntrospectValue::Json(pinion_shell::conformance_json::<
                NodeLabView,
            >())),
            "graph" => text(spec::GRAPH_NAME.to_owned()),
            "selected" => text(state.active_card().map(|n| state.name_of(n)).unwrap_or_default()),
            // ★★★ R1706 — the whole selection, in arrival order, beside the
            // leader `selected` already answered. Added rather than folded in,
            // because an agent that reads `selected` to name "the" card is
            // asking a question that still has an answer when six are picked —
            // and the sibling node canvas shows what folding costs: its
            // `selected` slot answers NOTHING once two are selected, so the one
            // question a reader most often asks became unanswerable in exactly
            // the case a set exists for.
            "selected_ids" => text(
                state
                    .selection
                    .get()
                    .members()
                    .iter()
                    .map(|n| state.name_of(*n))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            // ★ R1681 — a drawn link answers its id and a reported one answers
            // the pair it runs between, which is the only name it has. The two
            // spellings are told apart by the `>`, and `select_link` admits
            // both, so what this reads back is what that takes.
            "selected_link" => text(
                state
                    .selected_link
                    .get()
                    .map(|pick| match pick {
                        LinkPick::Authored(id) => id.0.to_string(),
                        LinkPick::Observed(from, to) => format!(
                            "{}>{}",
                            state.name_of(from.node),
                            state.name_of(to.node)
                        ),
                    })
                    .unwrap_or_default(),
            ),
            "zoom" => Ok(IntrospectValue::Int(i64::from(state.zoom.get()))),
            "pan" => {
                let (x, y) = state.pan.get();
                text(format!("{x},{y}"))
            }
            "running" => Ok(IntrospectValue::Bool(state.running.get())),
            "discovery" => Ok(IntrospectValue::Bool(state.discovery.get())),
            "cursor" => {
                let (x, y) = state.cursor.get();
                text(format!("{x},{y}"))
            }
            // ★★★★★ R1853 — every fault the selected node's settings admit, each
            // saying which part of the DECLARATION admits it and what injecting
            // it would do. Derived per call; nothing is stored, so a form that
            // gains a row gains its faults here too.
            "faults" => Ok(IntrospectValue::Json(faults_json(state))),
            // The boundary, as data. `world` is not derivable from any
            // declaration and says so in its own words — an absence a client can
            // enumerate rather than one it has to notice.
            "fault_scopes" => Ok(IntrospectValue::Json(serde_json::Value::Array(
                Scope::ALL
                    .iter()
                    .map(|scope| {
                        serde_json::json!({
                            "scope": scope.wire(),
                            "injectable": scope.injectable(),
                            "because": scope.because(),
                        })
                    })
                    .collect(),
            ))),
            // ★★★★★ R1857 — the addresses the panel occupies, in painted order.
            // Composed from the SPECIFICATION's shape and the two derivations
            // above, so it cannot say the panel is made of something other than
            // what the painter draws without one of them being edited.
            "faults_roster" => Ok(IntrospectValue::Json(serde_json::Value::Array(
                spec::FAULT_PANEL
                    .roster(
                        fault_rows(state).len(),
                        &fault_scope_notes()
                            .iter()
                            .map(|(wire, _)| *wire)
                            .collect::<Vec<_>>(),
                    )
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ))),
            "verdict" => {
                let verdict = state.verdict();
                text(
                    serde_json::json!({
                        "may_launch": verdict.may_launch(),
                        "blocking": verdict.blocking(),
                        "warning": verdict.warning(),
                        "sentence": verdict.sentence(),
                    })
                    .to_string(),
                )
            }
            "gate" => text(
                serde_json::Value::Array(
                    state
                        .gate_lines()
                        .into_iter()
                        .map(|(blocks, sentence)| {
                            serde_json::json!({ "blocks": blocks, "sentence": sentence })
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★★★★★ R1850 — the catalogue, split the way a caller reads it.
            "catalogue" => {
                let form = selected_form(state)
                    .ok_or_else(|| ReadRefusal::unavailable("no node is selected"))?;
                let held: Vec<&str> = form.fields().iter().map(ConfigField::key).collect();
                // ⚠ `spec::ADDABLE` and NOT `form.catalogue()`. The form object
                // counts its own present rows as catalogue entries, so it would
                // answer "every row returns"; this screen rebuilds the form
                // each render from `ADDABLE`, so what actually returns is that
                // list. The two disagree — see `ConfigForm::catalogue`'s doc —
                // and the one a reader needs is the builder's.
                let known: Vec<&str> = spec::ADDABLE.to_vec();
                // `restorable` is the half nothing could answer before: rows
                // that are on the form AND offerable, so taking one off is
                // undoable. `ends_it` is its complement over the rows that are
                // there, and it is the one a reader is owed a warning about.
                let restorable: Vec<&str> =
                    held.iter().copied().filter(|k| known.contains(k)).collect();
                let ends_it: Vec<&str> = held
                    .iter()
                    .copied()
                    .filter(|k| !known.contains(k))
                    .collect();
                text(
                    serde_json::json!({
                        "known": known,
                        "offered": form
                            .addable()
                            .iter()
                            .map(|f| f.key())
                            .collect::<Vec<_>>(),
                        "restorable": restorable,
                        "ends_it": ends_it,
                    })
                    .to_string(),
                )
            }
            "form" => {
                let form = selected_form(state)
                .ok_or_else(|| ReadRefusal::unavailable("no node is selected"))?;
                text(
                    serde_json::Value::Array(
                        form.fields()
                            .iter()
                            .map(|f| {
                                // ★★★★★ R1732 — **the words this row will
                                // take**, when it takes only certain ones.
                                // Absent until now, so an agent looking at an
                                // enumeration could read the value and had no
                                // way at all to learn what else was allowed: it
                                // had to guess and be refused. `null` where the
                                // shape has no roster, spelled out rather than
                                // omitted so "this takes anything" and "this
                                // build does not say" do not read the same.
                                let of = f.shape().options();
                                let options = if of.is_empty() {
                                    serde_json::Value::Null
                                } else {
                                    serde_json::Value::Array(
                                        of.iter()
                                            .map(|w| serde_json::Value::String(w.to_string()))
                                            .collect(),
                                    )
                                };
                                serde_json::json!({
                                    "key": f.key(),
                                    "ty": f.ty(),
                                    "applies": f.applies().wire(),
                                    "value": f.value(),
                                    "edited": f.edited(),
                                    "hidden": f.hidden(),
                                    // ★★★ R1716 — where the value came from and
                                    // where it goes. An agent reading this is the
                                    // reader with no badge to look at, and without
                                    // these two an unwritable row is indistinguishable
                                    // from a broken one. `null` is the ordinary
                                    // answer in both, spelled out rather than absent
                                    // so a reader never has to tell "no source" from
                                    // "this build does not say".
                                    "source": f.source().derived_from(),
                                    "aside": f.goes().instead(),
                                    // ★★★★★ R1717 — the half somebody wrote,
                                    // which with `source` above is the whole
                                    // answer and needs no third key: nothing
                                    // written and a source named is a row this
                                    // screen works out, both is a row with two
                                    // contributors, and neither can be read off
                                    // the shown value. The count is published
                                    // beside it because recovering it means
                                    // re-implementing the splitter, and an
                                    // agent that got the separator wrong would
                                    // silently disagree with the screen.
                                    "written": f.written(),
                                    "derived_elements": f.derived_elements(),
                                    "options": options,
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            // ★★★★★ R1732 — where the reader is in an open roster, as a value.
            //
            // `null` when nothing is open, which is the whole of "shut": there
            // is no `open` flag to disagree with, because a picker exists for
            // exactly as long as a roster does. `holding` is the word the
            // document has when the roster does not offer it — a named
            // difference rather than the silent substitution the floor makes.
            "picking" => text(
                match state.picking.get().as_ref() {
                    Some((key, picker)) => serde_json::json!({
                        "key": key,
                        "at": picker.at(),
                        "highlighted": picker.highlighted(),
                        "options": picker.options()
                            .iter()
                            .map(std::string::ToString::to_string)
                            .collect::<Vec<_>>(),
                        "holding": picker.holding(),
                    }),
                    None => serde_json::Value::Null,
                }
                .to_string(),
            ),
            "document" => {
                let form = selected_form(state)
                .ok_or_else(|| ReadRefusal::unavailable("no node is selected"))?;
                match form.document() {
                    Ok(document) => text(document.to_string()),
                    Err(why) => text(serde_json::json!({ "refused": why.to_string() }).to_string()),
                }
            }
            // ★★ R1687 — the artifacts, or nulls where they are not. See
            // `Produced::wire` for why a null and not a missing key.
            "produced" => text(state.produced.borrow().wire().to_string()),
            // ★★★★★ R1789 — the scenario, whole: its lanes, its entries, how
            // long it is, where the playhead stands and what acts exist. One
            // read rather than five, because a lane and its entries are not
            // separately meaningful.
            "scenario" => Ok(IntrospectValue::Json(scenario::wire(state))),
            // ★★★★★ R1866 — what THIS run did differently from the one a reader
            // kept. A slot and not part of `scenario`, because a regression is
            // a fact about two runs and the scenario is one of them: folding it
            // in would make "the plan" and "the comparison" one read that
            // changes when either changes.
            "regression" => Ok(IntrospectValue::Json(scenario::regression_wire(state))),
            // ★★ R1689 — what a save would write, and what one did.
            "archive" => text(persist::graph_text(state)),
            "stored" => text(persist::stored(state)),
            // ★★★ R1690 — the two meters, as the numbers rather than as the
            // label. The label is a rendering of these; publishing the label
            // would make an agent parse a sentence back into figures, and would
            // make the screen and the wire two claims that can disagree.
            "reach" => {
                let reach = palette_reach();
                text(serde_json::json!({
                    "sections": format!("{}/{}", reach.root_hit(), reach.root_total),
                    "leaves": format!("{}/{}", reach.leaf_hit(), reach.leaf_total),
                    "sound": reach.sound(),
                    "complete": reach.complete(),
                    "roots_missing": reach.roots_missing,
                    "leaves_missing": reach.leaves_missing,
                    "mistyped": reach
                        .mistyped
                        .iter()
                        .map(pinion_core::widgets::config_schema::Mistyped::sentence)
                        .collect::<Vec<_>>(),
                    "unknown": reach.unknown,
                    "unauthorable": reach.unauthorable,
                })
                .to_string())
            }
            // ★★★★★ R1840 — the declared surface against the SOURCED one.
            "surface" => {
                let drift = settings::drift();
                let (hit, total) = drift.covered();
                text(serde_json::json!({
                    "covered": format!("{hit}/{total}"),
                    "sourced_only": drift.sourced_only,
                    "declared_only": drift.declared_only,
                    "sentence": drift.sentence(),
                })
                .to_string())
            }
            "strings" => {
                let census = settings::strings();
                text(serde_json::json!({
                    "pinned": format!("{}/{}", census.pinned(), census.total()),
                    "choices": census.choices,
                    "formats": census.formats,
                    "free": census.free,
                })
                .to_string())
            }
            "nodes" => text(
                state
                    .cards()
                    .into_iter()
                    .map(|n| state.name_of(n))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "links" => {
                let doc = state.doc.borrow();
                let tree = doc
                .tree(ROOT)
                .ok_or_else(|| ReadRefusal::no_such_member("the document has no root tree"))?;
                text(
                    serde_json::Value::Array(
                        tree.links()
                            .iter()
                            .map(|l| {
                                serde_json::json!({
                                    "id": l.id.0,
                                    "from": state.name_of(l.from.node),
                                    "to": state.name_of(l.to.node),
                                    // ★ R1681 — WHICH endpoint of the target
                                    // this link dialled. Published because it
                                    // is what the endpoint seats move and an
                                    // operation whose result cannot be read is
                                    // indistinguishable from one that did
                                    // nothing.
                                    "endpoint": endpoint_at(state, l.to).unwrap_or_default(),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            // ★★ R1681 — the other layer: what a source reported. Its own slot
            // rather than a flag on `links`, because it is not in the graph and
            // a reader that had to filter a mixed list would be one `if` away
            // from treating a claim about the world as a drawing.
            "observed" => {
                let doc = state.doc.borrow();
                text(
                    serde_json::Value::Array(
                        doc.observations(ROOT)
                            .into_iter()
                            .map(|seen| {
                                serde_json::json!({
                                    "from": state.name_of(seen.from.node),
                                    "to": state.name_of(seen.to.node),
                                    "endpoint": endpoint_of(&doc, seen.to).unwrap_or_default(),
                                    "layer": doc
                                        .link_layer(ROOT, seen.from, seen.to)
                                        .map(LinkLayer::name)
                                        .unwrap_or_default(),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            // ★★ R1678 — which scopes differ from what the screen opened as.
            //
            // Published because the affordances are DERIVED from it: a reset
            // button exists exactly when its scope is here, so a driver that
            // read the screen and a person looking at it are reading one fact.
            // The reference publishes the same predicates to its own view for
            // the same reason (measured — three of its four gated resets are
            // wrapped in a conditional on one of these).
            "changed" => text(
                serde_json::Value::Object(
                    ResetScope::ALL
                        .into_iter()
                        .map(|scope| {
                            (
                                scope.wire().to_owned(),
                                serde_json::Value::Bool(scope.changed(state)),
                            )
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★★ R1677 — WHERE the cards are, which nothing published until the
            // operation gate asked for it. Three of the reference's operations
            // move a node or a frame, and an agent driving any of them could
            // not observe its own result: the canvas positions existed only
            // inside the document and reached the wire nowhere. A gesture whose
            // effect cannot be read is one no test can distinguish from a
            // gesture that did nothing, which is exactly how "the frame drags
            // but does not select" survived.
            //
            // Canvas coordinates, not window ones: this is where a node sits in
            // the GRAPH, which is what a caller placing or comparing nodes
            // means. `scene/tag_rects` answers the window question for the same
            // cards, and the two are deliberately different reads.
            "layout" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .filter_map(|node| {
                            let doc = state.doc.borrow();
                            let slot = doc.tree(ROOT)?.node(node)?;
                            Some((
                                state.name_of(node),
                                serde_json::json!([slot.x, slot.y]),
                            ))
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★★ R1682 — the two switches a card carries: whether it is drawn
            // small, and whether it runs at all.
            //
            // Read together because the affordance shows them together — they
            // are the node's-life row, one press each — and published for the
            // R1677 reason: neither moves a name or a position, so without a
            // slot of their own an agent could collapse a card and have no way
            // to observe that it had. A gesture whose effect cannot be read is
            // one no test can tell from a gesture that did nothing.
            //
            // ★ They are two different KINDS of fact and the wire says so by
            // keeping them apart rather than folding them into one "state"
            // word: `collapsed` is a look, and the model keeps it with the
            // node's appearance; `disabled` is what the graph MEANS, and the
            // model keeps it beside the node's body. A reader that wanted only
            // one of them would otherwise have to know which half of a blended
            // answer to trust.
            "cards" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .filter_map(|node| {
                            let doc = state.doc.borrow();
                            let slot = doc.tree(ROOT)?.node(node)?;
                            Some((
                                slot.display_name(),
                                serde_json::json!({
                                    "collapsed": slot.appearance.collapsed,
                                    "disabled": slot.disabled,
                                    // ★★★★★ R1912 — each pin's state, and the
                                    // hidden ones say WHICH REASON. Neither
                                    // reference publishes this: one computes
                                    // socket visibility as a conjunction of
                                    // three facts and hands out the
                                    // conjunction, the other deletes the pin.
                                    // A client offering "bring it back" needs
                                    // to know the answer is a person's.
                                    "pins": pins_json(&doc, node),
                                }),
                            ))
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★ R1677 — which host each card starts on. The membership a drop
            // changes, and the other half of the same silence: `apply_frame`
            // re-parents a node and the only witness was a toast sentence.
            "frames" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .map(|node| {
                            // ★ R1716 — through the one walk, which the launch
                            // plan now shares. Two copies of this lookup is how
                            // the plan came to put every node on `unplaced`
                            // while this read said otherwise.
                            let frame = state.frame_of(node);
                            (
                                state.name_of(node),
                                frame.map_or(serde_json::Value::Null, serde_json::Value::String),
                            )
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★ R1683 — what the one field is doing, and what it holds. Both,
            // because "the editor is open" and "it says this" are different
            // facts and a driver checking its own typing needs the second.
            "editing" => text(
                serde_json::json!({
                    "target": state.editing.get().as_ref().map(Editing::wire),
                    "text": state.buffer.text(),
                })
                .to_string(),
            ),
            "roles" => text(
                Role::ALL
                    .into_iter()
                    .map(|r| format!("{}:{}", r.group(), r.name()))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            // ★ R1778 — the `map(..).unwrap_or_default()` chain that was here is
            // the holder's own method now; three screens were writing it.
            "toast" => text(state.toast.sentence()),
            "said" => Ok(IntrospectValue::Json(match state.toast.showing() {
                Some(said) => serde_json::to_value(&said)
                    .map_err(|_| ReadRefusal::UnknownPath)?,
                None => serde_json::Value::Null,
            })),
            // ★★★★★ R1790 — the sentence AND how long it has, so a gate advances
            // time by asking rather than by guessing a number this screen owns.
            "saying" => Ok(IntrospectValue::Json(state.toast.to_wire())),
            // ★★★★★ R1791 — what the toolbar moved, what is still on it, and
            // whether it fits at all.
            "toolbar_overflow" => {
                let laid = right_cluster();
                Ok(IntrospectValue::Json(serde_json::json!({
                    "on_the_row": laid.shown().iter().map(|g| g.word()).collect::<Vec<_>>(),
                    "moved": laid.moved().iter().map(|g| g.word()).collect::<Vec<_>>(),
                    // ★★★★★ R1791 — the moved groups' SEATS, by the tag a reader
                    // aims at. `moved` names the groups, which is what a person
                    // reads; this is the same fact in the vocabulary a gate and
                    // an agent work in, so neither has to know the grouping to
                    // tell "behind the control" from "gone". The floor
                    // publishes neither.
                    "moved_seats": laid
                        .moved()
                        .iter()
                        .flat_map(|g| g.tags())
                        .collect::<Vec<_>>(),
                    "open": state.toolbar_open.get(),
                    "control": laid.needs_affordance(),
                    "short_by": laid.short_by(),
                })))
            }
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self.state.as_ref().ok_or(InterveneError::UnknownPath)?;
        match path {
            "discovery" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.discovery.set(on);
                Ok(())
            }
            // ★ Refused as READ-ONLY rather than as an unknown path: these
            // three are published reads, and an agent that could write them
            // would be setting a fact the screen derives. Naming the action
            // that does move them is what keeps the refusal useful.
            "running" | "zoom" | "selected" => Err(InterveneError::ReadOnly),

            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// ★★★★★ R1720 — the refusal an agent was handed, put in front of the
    /// person watching this screen.
    ///
    /// One line, because the framework composed the sentence and chose the
    /// urgency: this names only the thing a screen actually owns, which is
    /// *where* speech goes. Before this round the canvas had the pair written
    /// out by hand at 2 of its 26 refusing verbs, and the other 24 changed
    /// nothing on screen at all.
    fn announce(&mut self, refused: &Utterance) -> Announced {
        let Some(state) = self.state.as_ref() else {
            return Announced::nowhere("the lab holds no document yet, so it has no toast");
        };
        state.say(refused.clone());
        Announced::at("lab.toast")
    }

    #[allow(clippy::too_many_lines, reason = "one arm per published action")]
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        match path {
            "select" => {
                let name = Self::text(&args)?;
                let node = state.node_of(name.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!("no node is called {:?}", name.trim()))
                })?;
                // ★ R1736 — the sentence moved INTO `select_card`, so this arm
                // no longer says it. Saying it here as well would tell the wire
                // twice and the pointer once, which is the asymmetry that line
                // exists to end.
                select_card(&state, Some(node));
                Ok(IntrospectValue::Text(name.trim().to_owned()))
            }
            // ★★ R1682 — the node's own life. Four verbs over one argument,
            // the card's name, which is the name the canvas shows and the wire
            // reads back.
            "delete_node" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                delete_card(&state, node).map(IntrospectValue::Text)
            }
            "rename" => {
                let raw = Self::text(&args)?;
                let (which, to) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <node>,<name>"))
                })?;
                let node = Self::card(&state, which.trim())?;
                rename_card(&state, node, to.trim()).map(IntrospectValue::Text)
            }
            // ★★★★★ R1885 — **put a card on another build.** The agent half of
            // the act a compatibility test graph is built to perform, and the
            // reason it is a verb rather than only a press: an agent asking
            // "would these two peers still talk if this one were the older
            // release?" has to be able to make the change and then read the
            // launch gate, and a press it cannot name is a question it cannot
            // ask. The refusal names the builds this screen offers, because a
            // rejection that does not say what WAS acceptable makes the caller
            // guess.
            "build" => {
                let raw = Self::text(&args)?;
                let (which, word) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <node>,<build>"))
                })?;
                let node = Self::card(&state, which.trim())?;
                let stack = Stack::from_word(word.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "{:?} is not a build this screen offers — {}",
                        word.trim(),
                        Stack::ALL
                            .iter()
                            .map(|s| s.word())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?;
                Ok(IntrospectValue::Text(set_build_on(&state, node, stack)))
            }
            // ★★★ R1706 — the frame gesture's agent half, and it is ONE verb
            // because the gesture is one act: the reference's frame-drag
            // handler selects the host's cards on its first line and then
            // carries them, so a `select_frame` and a `move_frame` here would
            // be two spellings a person has no way to perform separately.
            //
            // ★ It is also what makes this operation's row provable in both
            // columns. `move a frame and its members` had `verb: None` — a
            // person could do it and an agent could not, which on a screen
            // whose whole premise is that an agent drives it is the asymmetry
            // the table exists to surface.
            "move_frame" => {
                let raw = Self::text(&args)?;
                let (host, delta) = raw.trim().split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <host>,<dx>,<dy>"))
                })?;
                let (dx, dy) = delta.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <host>,<dx>,<dy>"))
                })?;
                let dx: i32 = dx.trim().parse().map_err(|_| {
                    InvokeError::rejected(format!("{:?} is not a whole number", dx.trim()))
                })?;
                let dy: i32 = dy.trim().parse().map_err(|_| {
                    InvokeError::rejected(format!("{:?} is not a whole number", dy.trim()))
                })?;
                move_frame(&state, host.trim(), (dx, dy)).map(IntrospectValue::Text)
            }
            // ★★ R1683 — the field's own three verbs. `edit` opens it on a
            // target, `type` puts text in it, `apply` does the thing. Three and
            // not one, because an agent that could only "rename with this
            // string" could never exercise the path a PERSON takes — which is
            // exactly the column every defect on this screen has lived in.
            "edit" => {
                let what = Self::text(&args)?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let target = match what.trim() {
                    "name" => Editing::Name(node),
                    "key" => Editing::Key(node),
                    // ★ R1684 — the third target names a ROW, so the word is a
                    // grammar rather than a constant: `value:<key>` for the
                    // row's own value and `value:<key>[<n>]` for one element of
                    // a list row. A path the form does not hold, or an element
                    // the row does not have, is refused here rather than
                    // opening a box over nothing — the wire's `edit` says which
                    // row it opened, and a row that is not there has no
                    // rectangle to say it about.
                    other => {
                        let spelled = other.strip_prefix("value:").ok_or_else(|| {
                            InvokeError::rejected(format!(
                                "{other:?} is not a thing this screen edits"
                            ))
                        })?;
                        let (key, element) = Self::row_target(spelled.trim())?;
                        let held = selected_form_of(&state, node)
                            .and_then(|form| form.field(&key).map(|f| f.value().into_owned()))
                            .ok_or_else(|| {
                                InvokeError::rejected(format!(
                                    "{key:?} is not a row of this card's settings"
                                ))
                            })?;
                        if let Some(n) = element {
                            if FieldType::elements(&held).nth(n).is_none() {
                                return Err(InvokeError::rejected(format!(
                                    "{key:?} has no element {n}"
                                )));
                            }
                        }
                        Editing::Value { node, key, element }
                    }
                };
                begin_edit(&state, target).map(IntrospectValue::Text)
            }
            "type" => {
                let text = Self::text(&args)?;
                if state.editing.get().is_none() {
                    return Err(InvokeError::rejected("nothing is being edited"));
                }
                state.buffer.set_text(text.clone());
                Ok(IntrospectValue::Text(text))
            }
            "apply" => commit_edit(&state).map(IntrospectValue::Text),
            // ★ The one-shot, beside the box's three — the same arrangement
            // `rename` has. An agent that knows the key it wants says so; the
            // box is for a person who is finding out.
            "add_key" => {
                let key = Self::text(&args)?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                add_key(&state, node, key.trim()).map(IntrospectValue::Text)
            }
            "collapse" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                collapse_card(&state, node).map(IntrospectValue::Text)
            }
            "disable" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                disable_card(&state, node).map(IntrospectValue::Text)
            }
            // ★★★★★ R1912 — `<card>,<scope>`, the two-part shape `place` and
            // `detach_home` already use on this screen. The scope words are the
            // references' own; see `put_away_pins`.
            // ★★★★★ R1919 — **look for a name.** The verb sets what is being
            // looked for and nothing else; the hits are a derivation of that
            // and the document, so there is no result to invalidate when the
            // graph changes underneath. Answers the COUNT, because "how many
            // answer to this" is what a caller decides with — an empty needle
            // clears the search and answers 0.
            "find" => {
                let needle = Self::text(&args)?;
                state.searching.set(needle.trim().to_owned());
                Ok(IntrospectValue::Int(
                    i64::try_from(found(&state).len()).unwrap_or(i64::MAX),
                ))
            }
            // ★★★★★ R1921 — **give a card a colour, or take it away.** One
            // assignment either way, because the model holds one value and not
            // a colour beside a flag. Answers what the card now carries, so a
            // caller learns the result without a second read.
            // ★★★★★ R1923 — **write a note on a card, or take it away.** Answers
            // the SOURCE the card now speaks from, so a caller learns whether it
            // is reading its own words back or the role's line — which is the
            // distinction the reference's bare string cannot carry.
            "note" => {
                let raw = Self::text(&args)?;
                let (which, sentence) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<sentence>"))
                })?;
                let node = Self::card(&state, which.trim())?;
                let wanted = match sentence.trim() {
                    "none" => None,
                    "" => {
                        return Err(InvokeError::rejected(
                            "an empty note is not a note — say `none` to clear one".to_owned(),
                        ));
                    }
                    said => Some(said.to_owned()),
                };
                {
                    let mut doc = state.doc.borrow_mut();
                    let slot = doc
                        .tree_mut(ROOT)
                        .and_then(|host| host.node_mut(node))
                        .ok_or_else(|| InvokeError::rejected("no such card"))?;
                    slot.description = wanted;
                }
                let now = state
                    .doc
                    .borrow()
                    .description(ROOT, node)
                    .map_or("none", |d| d.source.wire_word());
                let name = state.name_of(node);
                state.say(Utterance::done(format!("{name} now speaks from its {now}")));
                Ok(IntrospectValue::Text(now.to_owned()))
            }
            "tint" => {
                let raw = Self::text(&args)?;
                let (which, colour) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<colour>"))
                })?;
                let node = Self::card(&state, which.trim())?;
                let wanted = parse_tint(colour.trim())?;
                {
                    let mut doc = state.doc.borrow_mut();
                    let slot = doc
                        .tree_mut(ROOT)
                        .and_then(|host| host.node_mut(node))
                        .ok_or_else(|| InvokeError::rejected("no such card"))?;
                    slot.appearance.tint = wanted;
                }
                let shown = match wanted {
                    Some(t) => format!("#{:02x}{:02x}{:02x}", t.r, t.g, t.b),
                    None => "none".to_owned(),
                };
                let name = state.name_of(node);
                state.say(Utterance::done(match wanted {
                    Some(_) => format!("{name} coloured {shown}"),
                    None => format!("{name} back to its kind's colour"),
                }));
                Ok(IntrospectValue::Text(shown))
            }
            "put_away_pins" => {
                let raw = Self::text(&args)?;
                let (name, which) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<scope>"))
                })?;
                let node = Self::card(&state, name.trim())?;
                put_away_pins(&state, node, which.trim()).map(IntrospectValue::Text)
            }
            // ★★★★★ R1914 — `<card>,<address>`, the same two-part shape, with
            // a leading `-` on the address to fold rather than split. The
            // address is the model's `PortPath` spelled for a reader.
            "split_pin" => {
                let raw = Self::text(&args)?;
                let (name, address) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<address>"))
                })?;
                let node = Self::card(&state, name.trim())?;
                split_pin(&state, node, address.trim()).map(IntrospectValue::Text)
            }
            // ★ R1681 — either layer, told apart by the `>`. A reported link
            // has no id to name it by, so the pair is the name; refusing to let
            // one be picked at all would have made the adopt affordance
            // unreachable to everything but a pointer.
            "select_link" => {
                let raw = Self::text(&args)?;
                let pick = Self::link_pick(&state, raw.trim())?;
                state.selected_link.set(Some(pick));
                Ok(IntrospectValue::Text(raw.trim().to_owned()))
            }
            "set_field" => {
                let raw = Self::text(&args)?;
                let (key, value) = raw.split_once('=').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <key>=<value>"))
                })?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                set_value(&state, node, key.trim(), value.trim()).map(IntrospectValue::Text)
            }
            // ★★★★★ R1853 — inject one of the faults this node's settings admit,
            // named by its path and its arm.
            //
            // The value is NOT a parameter: it comes from the derivation, which
            // is the whole difference between injecting a fault and typing a bad
            // value. A caller naming `<key>:<kind>` gets the fault that kind
            // actually is at that field, or a refusal saying which faults the
            // field admits — so an agent discovers the vocabulary from the
            // surface instead of guessing at values.
            "inject" => {
                let raw = Self::text(&args)?;
                let (key, kind) = raw
                    .split_once(':')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <key>:<kind>")))?;
                let (key, kind) = (key.trim(), kind.trim());
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let offers = fault_rows(&state);
                let wanted = fault_injection::DefectKind::from_wire(kind).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "{kind:?} is not a fault kind; they are {}",
                        fault_injection::DefectKind::ALL
                            .map(fault_injection::DefectKind::wire)
                            .join(" / ")
                    ))
                })?;
                let offer = offers
                    .iter()
                    .find(|one| one.key == key && one.kind == wanted)
                    .ok_or_else(|| {
                        // ⚠ The refusal NAMES what this field admits, because
                        // *not offered* and *no such field* are different facts
                        // and a caller has to be able to tell them apart. A
                        // field whose shape admits nothing says exactly that.
                        let mine: Vec<&str> = offers
                            .iter()
                            .filter(|one| one.key == key)
                            .map(|one| one.kind.wire())
                            .collect();
                        // ★★★★★ THREE refusals and not two, and the third was
                        // found by R1853's own demo driving it: a key the
                        // declaration does not hold was being told its *declared
                        // shape accepts every value*, which is a sentence about a
                        // declaration that is not there. That is exactly the
                        // conflation the comment above promises not to make, so
                        // the form is asked whether it holds the key at all.
                        let held = selected_form(&state)
                            .is_some_and(|form| form.fields().iter().any(|f| f.key() == key));
                        InvokeError::rejected(if !held {
                            format!(
                                "{key:?} is not a row of this node's settings — ask \
                                 `faults` for the keys that are"
                            )
                        } else if mine.is_empty() {
                            format!(
                                "{key:?} admits no injectable fault — either its declared \
                                 shape accepts every value a person can write, or the row \
                                 is worked out from another and cannot receive one"
                            )
                        } else {
                            format!("{key:?} admits {}, not {kind}", mine.join(" / "))
                        })
                    })?
                    .clone();
                // ★ Every offer is a declared row's, so writing the row IS the
                // injection. The settings' third fault — a key the declaration
                // lacks — is not offered, because a form reports an undeclared
                // leaf unplaceable rather than taking it; `Scope::Document` is
                // where that boundary is stated and `fault_scopes` publishes it.
                set_value(&state, node, &offer.key, &offer.value)?;
                state.say(Utterance::done(format!(
                    "injected {} at {} — {}",
                    offer.kind.wire(),
                    offer.key,
                    if offer.blocks() {
                        "blocks a launch"
                    } else {
                        "warns and starts"
                    }
                )));
                Ok(IntrospectValue::Text(format!(
                    "{}:{}={}",
                    offer.key,
                    offer.kind.wire(),
                    offer.value
                )))
            }
            "add_field" => {
                let key = Self::text(&args)?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                amend(&state, node, |form| form.add(key.trim()))
                    .map_err(|why| InvokeError::rejected(why.to_string()))?;
                state.say(Utterance::done(format!("added {}", key.trim())));
                Ok(IntrospectValue::Text(key.trim().to_owned()))
            }
            // ★★★★★ R1732 — open the roster on a row, or shut whatever is
            // open. One verb rather than two, because the reader's act is the
            // same one: the control is a thing you are either in or not, and
            // an empty argument is the way out of it.
            //
            // It REFUSES rather than doing nothing when the row cannot be
            // picked from — a row nobody wrote, or one whose shape has no
            // roster — because an agent that got silence back would read it as
            // an open roster with nothing in it.
            "pick" => {
                let key = Self::text(&args)?;
                let key = key.trim();
                if key.is_empty() {
                    close_roster(&state);
                    return Ok(IntrospectValue::Text(String::new()));
                }
                state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                open_roster(&state, key);
                match state.picking.get().as_ref() {
                    Some((open, picker)) if open == key => {
                        Ok(IntrospectValue::Text(picker.highlighted().to_owned()))
                    }
                    _ => Err(InvokeError::rejected(format!(
                        "{key} is not a row this screen can pick from: it takes one of a \
                         fixed set of words, and somebody has to own it"
                    ))),
                }
            }
            // ★★★ R1716 — take a row over. The act the floor performs by
            // assigning to the value, with no name, no news and no way back.
            "author_field" => {
                let key = Self::text(&args)?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                author_row(&state, node, key.trim()).map(IntrospectValue::Text)
            }
            "remove_field" => {
                let key = Self::text(&args)?;
                let node = state
                    .active_card()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                // ★ R1686 — through [`remove_row`], which is now the one way a
                // row leaves a form. This arm used to be the only caller and
                // said nothing about what it had done.
                remove_row(&state, node, key.trim()).map(IntrospectValue::Text)
            }
            // ★★★★★ R1925 — **arrange this graph's own face**, one verb with a
            // command word rather than three verbs, for R1678's reason below.
            //
            // The wire names a section by its header, which is what a person
            // would call it; the framework addresses it by an id that survives a
            // rename. Two sections of one name would make that translation
            // ambiguous, so this refuses the second — a screen-level policy, not
            // the framework's, and it is stated in the refusal.
            "section" => {
                let raw = Self::text(&args)?;
                let (word, rest) = raw.trim().split_once(',').unwrap_or((raw.trim(), ""));
                section_command(&state, word.trim(), rest.trim()).map(IntrospectValue::Text)
            }
            // ★★ R1678 — one action with a scope argument, not five actions.
            // The scopes are a closed set the specification already names, and
            // five verbs would be five places for that set to drift; the
            // declaration below publishes the options, so an agent discovers
            // them rather than guessing.
            "reset" => {
                let word = Self::text(&args)?;
                let scope = ResetScope::ALL
                    .into_iter()
                    .find(|s| s.wire() == word.trim())
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "{:?} is not a scope; they are {}",
                            word.trim(),
                            ResetScope::ALL.map(ResetScope::wire).join(" / ")
                        ))
                    })?;
                scope.apply(&state);
                state.say(Utterance::done(format!(
                    "{} back to how it opened",
                    scope.wire()
                )));
                Ok(IntrospectValue::Text(scope.wire().to_owned()))
            }
            "zoom_by" => {
                let word = Self::text(&args)?;
                let next = match word.trim() {
                    "in" => zoom_stepped(&state, true),
                    "out" => zoom_stepped(&state, false),
                    other => other
                        .parse::<u32>()
                        .ok()
                        .filter(|z| (ZOOM_MIN..=ZOOM_MAX).contains(z))
                        .ok_or_else(|| {
                            InvokeError::rejected(format!(
                                "{other:?} is not `in`, `out`, or {ZOOM_MIN}..={ZOOM_MAX}"
                            ))
                        })?,
                };
                // ★ R1688 — anchored at the middle of the canvas, through the
                // same function the steppers press.
                Ok(IntrospectValue::Int(i64::from(zoom_to(&state, next))))
            }
            // ★★★ R1688 — the view's own two, through the same functions the
            // seats press. `fit` answers the sentence a person reads, including
            // the one the reference cannot say: that the graph is larger than
            // the zoom range can shrink it to.
            "fit" => Ok(IntrospectValue::Text(fit_view(&state))),
            "go_to_problem" => Ok(IntrospectValue::Text(go_to_problem(&state))),
            // ★★ R1687 — through the same two functions the seats press, so the
            // artifact an agent gets and the one a person gets cannot differ.
            "export" => Ok(IntrospectValue::Text(export_configuration(&state))),
            "script" => Ok(IntrospectValue::Text(produce_script(&state))),
            // ★★★ R1689 — the file, through the same three functions the pill
            // presses. `open_graph` is the one that can fail, and it fails by
            // NAME: the substrate's reading says which of four things stopped
            // it, and that sentence is what an agent gets and what the toast
            // shows, rather than the `false` this class of verb usually answers.
            "save_graph" => Ok(IntrospectValue::Text(persist::save(&state))),
            "open_graph" => {
                let text = match &args {
                    IntrospectValue::Text(text) => text.as_str(),
                    IntrospectValue::Null => "",
                    _ => return Err(InvokeError::TypeMismatch),
                };
                persist::open(&state, text)
                    .map(IntrospectValue::Text)
                    .map_err(InvokeError::rejected)
            }
            "clear_graph" => Ok(IntrospectValue::Text(persist::clear(&state))),
            // ★★★★★ R1789 — the scenario's three verbs.
            "schedule" => {
                let obj = ObjectArgs::of(&args, "schedule")?;
                let at = seconds(obj.number("at")?);
                Ok(IntrospectValue::Text(scenario::schedule(
                    &state,
                    obj.word("lane").unwrap_or(scenario::DEFAULT_LANE),
                    at,
                    obj.word("act").unwrap_or(""),
                    obj.word("target").unwrap_or(""),
                    // ★ R1844 — absent means ABSENT, not zero. A timeout of
                    // zero is a legitimate deadline (check it now, wait no
                    // longer) and `schedule` refuses a missing one by name, so
                    // defaulting here would turn a caller's omission into a
                    // silently different scenario.
                    obj.number("timeout").ok().map(seconds),
                )?))
            }
            "unschedule" => {
                let obj = ObjectArgs::of(&args, "unschedule")?;
                let at = seconds(obj.number("at")?);
                Ok(IntrospectValue::Text(scenario::unschedule(
                    &state,
                    obj.word("lane").unwrap_or(scenario::DEFAULT_LANE),
                    at,
                )?))
            }
            "advance" => {
                let by = match args {
                    IntrospectValue::Float(v) => v,
                    #[allow(clippy::cast_precision_loss, reason = "a scrub is seconds, not ticks")]
                    IntrospectValue::Int(v) => v as f64,
                    _ => {
                        return Err(InvokeError::rejected(format!(
                            "advance takes a number of seconds and was given {}",
                            args.kind()
                        )));
                    }
                };
                Ok(IntrospectValue::Json(scenario::advance(
                    &state,
                    seconds(by),
                )?))
            }
            // ★★★★★ R1866 — keep this run as the one to measure against. A
            // verb and not a slot, for the rule this screen already follows
            // (R1829): a fact is a slot, an act is a verb, and choosing a
            // baseline is something a reader DOES.
            "record" => Ok(IntrospectValue::Json(scenario::record(&state)?)),
            "run" => {
                let verdict = state.verdict();
                let want = match args {
                    IntrospectValue::Bool(b) => b,
                    _ => !state.running.get(),
                };
                if want && !verdict.may_launch() {
                    return Err(InvokeError::rejected(format!(
                        "the gate is closed: {}",
                        verdict.sentence()
                    )));
                }
                state.running.set(want);
                if want {
                    // A launch settles every form: what is running now IS what
                    // the screen shows, so nothing is pending a restart.
                    for form in state.forms.borrow_mut().values_mut() {
                        form.settle();
                    }
                }
                state.say(Utterance::done(if want { "running" } else { "stopped" }));
                Ok(IntrospectValue::Bool(want))
            }
            "connect" => {
                let raw = Self::text(&args)?;
                let (from, to) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <from>,<to>")))?;
                let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim()))
                else {
                    return Err(InvokeError::rejected(format!(
                        "{:?} or {:?} is not a node on the canvas",
                        from.trim(),
                        to.trim()
                    )));
                };
                connect(&state, a, b).map(IntrospectValue::Text)
            }
            // ★★ R1681 — a link's life after it is drawn.
            "delete_link" => {
                let id = Self::link_id(&state, &Self::text(&args)?)?;
                delete_link(&state, id).map(IntrospectValue::Text)
            }
            "relink" => {
                let raw = Self::text(&args)?;
                let (link, to) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <link>,<to>")))?;
                let id = Self::link_id(&state, link)?;
                let node = state.node_of(to.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!("{:?} is not a node on the canvas", to.trim()))
                })?;
                relink_to(&state, id, node).map(IntrospectValue::Text)
            }
            // A number, because the seats a person can press are a numbered row
            // and an agent addressing them by locator would be addressing a
            // different thing from the one on screen.
            "set_endpoint" => {
                let raw = Self::text(&args)?;
                let n: usize = raw.trim().parse().map_err(|_| {
                    InvokeError::rejected(format!("{raw:?} is not an endpoint number"))
                })?;
                choose_endpoint(&state, n).map(IntrospectValue::Text)
            }
            "adopt" => {
                let raw = Self::text(&args)?;
                let (from, to) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <from>,<to>")))?;
                let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim()))
                else {
                    return Err(InvokeError::rejected(format!(
                        "{:?} or {:?} is not a node on the canvas",
                        from.trim(),
                        to.trim()
                    )));
                };
                let seen = state
                    .doc
                    .borrow()
                    .observations(ROOT)
                    .into_iter()
                    .find(|o| o.from.node == a && o.to.node == b)
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "nothing was reported from {} to {}",
                            from.trim(),
                            to.trim()
                        ))
                    })?;
                adopt_link(&state, seen.from, seen.to).map(IntrospectValue::Text)
            }
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
                let (win_w, win_h) = window_size();
                if x >= win_w || y >= win_h {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {win_w}x{win_h} window"
                    )));
                }
                move_cursor(&state, x, y);
                Ok(IntrospectValue::Text(Hit::at(&state, x, y).word(&state)))
            }
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => press(&state),
                    "PointerUp" => release(&state),
                    "PointerLeave" | "PointerCancel" => {
                        state.pressed.borrow_mut().take();
                        state.drag.set(None);
                        // ★★★★★ R1916 — and the POINTER IS GONE, which is a
                        // different fact from where it last was. Measured this
                        // round on the running shell: a description shown under
                        // a resting pointer stayed on the frame after the
                        // pointer left the window, because the cursor signal
                        // kept the last position it had been given and nothing
                        // said it was no longer anybody's.
                        //
                        // A leave is not a move to somewhere else, so a screen
                        // that only cleared on a move would leave a sentence
                        // hanging over a window nobody is pointing at.
                        state.pointer_inside.set(false);
                    }
                    "WheelUp" | "WheelDown" => {
                        let now = state.zoom.get();
                        let next = if event.trim() == "WheelUp" {
                            (now + ZOOM_STEP).min(ZOOM_MAX)
                        } else {
                            now.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
                        };
                        state.zoom.set(next);
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerLeave / PointerCancel / WheelUp / WheelDown"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.toast.sentence()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(key(&state, chord.trim())))
            }
            // ★★★★★ R1887 — the wire's half of placing a panel, through
            // `place_panel`, which is the function the press calls. One rule,
            // two channels: a screen and an agent that disagreed about what a
            // panel admits would be two declarations of one policy.
            "place" => {
                let said = Self::text(&args)?;
                let (name, want) = said.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(
                        "`place` takes <panel>,<edge|fold|unfold|width=N> — the panel and \
                         where it goes"
                            .to_owned(),
                    )
                })?;
                let which = SidePanel::from_word(name.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "{:?} is not a panel this screen places; the `spec` slot's `panes` \
                         names the ones that are",
                        name.trim()
                    ))
                })?;
                let ask = match want.trim() {
                    "fold" => PlaceAsk::Fold(true),
                    "unfold" => PlaceAsk::Fold(false),
                    // ★★★★★ R1889 — `width=N`, a KEYED word rather than a bare
                    // number, because the third operator takes an argument and
                    // the other two do not. A bare `280` would make the verb's
                    // grammar depend on whether a word parses as an integer,
                    // which is the kind of rule a client cannot read off the
                    // published schema.
                    keyed if keyed.starts_with("width=") => {
                        let n = keyed.trim_start_matches("width=");
                        let extent = n.parse::<u32>().map_err(|_| {
                            InvokeError::rejected(format!(
                                "{n:?} is not a width; `width=` takes a whole number of \
                                 logical pixels"
                            ))
                        })?;
                        PlaceAsk::Extent(extent)
                    }
                    // ★ The edge vocabulary is the FRAMEWORK's, read back
                    // through the same function that publishes it, so the word
                    // a client is told and the word it may send are one word.
                    word => {
                        let edge = [
                            ChromeEdge::Left,
                            ChromeEdge::Right,
                            ChromeEdge::Top,
                            ChromeEdge::Bottom,
                        ]
                        .into_iter()
                        .find(|edge| edge_word(*edge) == word)
                        .ok_or_else(|| {
                            InvokeError::rejected(format!(
                                "{word:?} is neither an edge nor `fold` / `unfold` / `width=N`"
                            ))
                        })?;
                        PlaceAsk::Edge(edge)
                    }
                };
                match place_panel(&state, which, ask) {
                    // ★ R1889 — the width is in the answer now, for the reason
                    // R1887 put the edge there: a caller that cannot read back
                    // what changed has to re-query to find out whether the verb
                    // did anything.
                    Ok(at) => Ok(IntrospectValue::Text(format!(
                        "{} {} {}{}",
                        which.word(),
                        edge_word(at.edge),
                        at.extent,
                        if at.folded { ", folded" } else { "" }
                    ))),
                    // ★★★★★ The refusal reaches the caller AND the screen. The
                    // floor, measured at R1801, accepts a move its own
                    // declaration forbids and returns nothing, throws nothing
                    // and signals nothing — a constraint that can be walked
                    // past in silence is a comment.
                    Err(refused) => Err(InvokeError::rejected(format!(
                        "{} ({})",
                        panel_refusal_sentence(which, &refused),
                        refused.wire_word()
                    ))),
                }
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The specification, as the wire publishes it.
/// The wire spelling of a voice family's population — the name of the table its
/// members come from, so a reader on the wire expands it the same way the local
/// gate does.
const fn population_wire(population: spec::Population) -> &'static str {
    match population {
        spec::Population::One => "one",
        spec::Population::Roles => "roles",
        spec::Population::Rail => "rail",
        spec::Population::Nodes => "nodes",
        spec::Population::Links => "links",
        spec::Population::Fields => "fields",
        // ★★ R1716 — the axis a row is on is part of the specification the
        // wire publishes, so an agent expands these the way the local gate
        // does: the `source` and `aside` columns of `fields` decide membership.
        spec::Population::AuthoredFields => "fields.authored",
        spec::Population::DerivedFields => "fields.derived",
        spec::Population::AsideFields => "fields.aside",
        spec::Population::BadgedFields => "fields.badged",
        spec::Population::Protocols => "protocols",
        spec::Population::PinKinds => "pin_legend",
    }
}

/// An edge's wire word — lower case, matching how every other enum reaches this
/// surface, and a `match` so a fifth edge would land here as a compile error.
fn edge_word(edge: ChromeEdge) -> &'static str {
    match edge {
        ChromeEdge::Top => "top",
        ChromeEdge::Bottom => "bottom",
        ChromeEdge::Left => "left",
        ChromeEdge::Right => "right",
    }
}

/// Each pane of this screen, as the wire publishes it: what it is, where it may
/// go, and **where it is**.
///
/// ★★★★★ R1887 — `at` is the half that was missing. `edges` says where a panel
/// MAY live, which R1801 was right to call the defect a reader kept hitting;
/// but a client told only what is permitted cannot tell whether a `place` did
/// anything, and cannot draw the arrangement it is looking at. `name` is the
/// word the `place` verb takes, so the domain a client picks from and the
/// domain that verb accepts are one list rather than two.
fn panes_json() -> Vec<serde_json::Value> {
    // Through `placements`, which reads the live state without constructing it
    // — the same source the layout reads, so the wire cannot answer about an
    // arrangement the screen is not in.
    let (palette, inspector) = placements();
    spec::PANES
        .iter()
        .map(|pane| {
            let placed = SidePanel::ALL.into_iter().find(|s| s.tag() == pane.tag);
            serde_json::json!({
                "tag": pane.tag, "title": pane.title, "width": pane.width, "body": pane.body,
                "name": placed.map(SidePanel::word),
                "edges": pane.policy.allowed.iter().copied().map(edge_word).collect::<Vec<_>>(),
                "foldable": pane.policy.foldable,
                // ★★★★★ R1889 — the BOUNDS, or null for a pane whose width the
                // specification settles. Published for the reason `edges` is:
                // `place <panel>,width=N` refuses outside them, so a client that
                // cannot read them can only find them by being refused. And
                // `null` is a statement — *this one does not resize* — where an
                // absent key would be *nobody said*.
                "resize": match pane.policy.resize {
                    pinion_core::edge_panel::Resize::Fixed => serde_json::Value::Null,
                    pinion_core::edge_panel::Resize::Between { min, max } => {
                        serde_json::json!({ "min": min, "max": max })
                    }
                },
                "at": placed.map(|which| {
                    let at = match which {
                        SidePanel::Palette => palette,
                        SidePanel::Inspector => inspector,
                    };
                    serde_json::json!({
                        "edge": edge_word(at.edge),
                        "extent": at.extent,
                        "folded": at.folded,
                    })
                }),
                // ★★★★★ R1902 — where the pane OPENS, beside where it IS.
                //
                // Two facts, and a client had only one of them: `at` says a
                // panel is folded and says nothing about whether that is how it
                // arrived or something a person did. The same bit, two
                // different things to know — and a client restoring a session,
                // or offering "put it back", needs the second.
                //
                // Published in the same shape as `at` so the two can be
                // compared field by field without either side converting.
                "opens": serde_json::json!({
                    "edge": edge_word(pane.opens.edge),
                    "extent": pane.opens.extent,
                    "folded": pane.opens.folded,
                }),
            })
        })
        .collect()
}

fn spec_json() -> serde_json::Value {
    serde_json::json!({
        // ★ R1664 — `body` is published too. R1662 added the column to the
        // specification and to the Rust sweep and stopped there, so the pane
        // that scrolls painted a tag the WIRE's copy of the specification did
        // not declare, and the demo's backward check went red on CI while
        // every local test passed. A fact added to the model and published by
        // half is the shape this project keeps paying for.
        // ★★★★★ R1801 — `edges` and `foldable`, because the absence of exactly
        // this is what a reader ran into three times. Asked which panels this
        // surface lets a person move, the wire answered `clauses: []` — and it
        // was right: nothing had ever said. A pane that may move now names the
        // edges it admits, and a pane that may not answers `[]`, which is a
        // different statement from not being asked.
        // ★★★★★ R1887 — and `at`, which is WHERE IT IS. `edges` says where a
        // panel may live and R1801 was right that its absence was the defect a
        // reader kept hitting; but a client told only what is permitted cannot
        // tell whether a `place` did anything, and cannot render the
        // arrangement it is looking at. `name` is the word that verb takes, so
        // the domain a client picks from and the domain the verb accepts are
        // one list rather than two.
        "panes": panes_json(),
        // ★ R1681 — published so the demo's family pin is derived from the
        // specification rather than written down, the same way `links` is.
        "observed": spec::OBSERVED.iter().map(|(from, to)| serde_json::json!({
            "from": from, "to": to,
        })).collect::<Vec<_>>(),
        "rail": spec::RAIL.iter().map(|(name, reserved_for)| serde_json::json!({
            "name": name,
            "locked": reserved_for.is_some(),
            "reserved_for": reserved_for,
            "active": *name == spec::RAIL_ACTIVE,
        })).collect::<Vec<_>>(),
        // ★★★★★ R1848 — a role publishes the traffic parameters it carries, and
        // the vocabulary those come from is published beside it. A client is
        // told what CAN be carried instead of inferring it from whichever roles
        // this document happens to contain — the same reason `violation_kinds`
        // sits beside the violations on the capture screen.
        "traffic_parameters": spec::TRAFFIC_PARAMETERS
            .iter().map(|p| p.key()).collect::<Vec<_>>(),
        "roles": spec::ROLES.iter().map(|r| serde_json::json!({
            "name": r.name, "gist": r.gist, "group": r.group, "accepts": r.accepts,
            "carries": r.carries.iter().map(|p| p.key()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "pin_legend": spec::PIN_LEGEND.iter().map(|(k, m)| serde_json::json!({
            "kind": k, "means": m,
        })).collect::<Vec<_>>(),
        // ★★★★★ R1915 — the seats the inspector opens for the selected card,
        // published so a reader's count of them is DERIVED rather than pinned.
        //
        // 🟥 What forced it, measured: `r1651`'s `lab.inspector` family pin is
        // `18 + <the pane's chrome>`, and R1912 added a fourth seat without
        // moving the 18. The demo went red and STAYED red for three rounds
        // while the round that added the seat, and the two after it, each wrote
        // that the sweep was unjudged — it was not, it was judged and red, and
        // nobody read the run underneath the newest one.
        //
        // ⇒ the seats are a CLOSED vocabulary the screen already owns
        // (`NodeAct::ALL`), so a count of them is a derivation and a constant
        // was never the honest statement of that part. What stays pinned is the
        // rest of the pane, which no declaration composes.
        "card_seats": NodeAct::ALL.iter().map(|act| serde_json::json!({
            "tag": act.tag(), "action": act.wire(),
        })).collect::<Vec<_>>(),
        "protocols": spec::PROTOCOLS,
        "frames": spec::FRAMES.iter().map(|f| serde_json::json!({
            "name": f.name, "gist": f.gist, "rect": [f.rect.0, f.rect.1, f.rect.2, f.rect.3],
        })).collect::<Vec<_>>(),
        "nodes": spec::NODES.iter().map(|n| serde_json::json!({
            "id": n.id, "role": n.role, "badge": n.badge, "frame": n.frame,
            "rect": [n.rect.0, n.rect.1, n.rect.2],
            "rows": n.rows.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
            // ★★★★★ R1848 — DERIVED from the node's rows and its role's
            // declaration, not recorded a third time. `stated` is what this
            // card puts in front of a reader; `unstated` is what its role
            // carries and the card leaves out, which is a measurement the
            // screen could not make about itself before the taxonomy existed.
            "traffic_stated": spec::stated_traffic(n)
                .into_iter().map(spec::TrafficParameter::key).collect::<Vec<_>>(),
            "traffic_unstated": spec::unstated_traffic(n)
                .into_iter().map(spec::TrafficParameter::key).collect::<Vec<_>>(),
            // ★★★★★ R1885 — which build this card opens on, and the revisions
            // it speaks. Published because "is this graph heterogeneous?" is
            // the question that makes it a compatibility test, and before this
            // a reader could not ask it: the axis existed in the model and
            // nowhere a client could see. DERIVED from `opening_implementation`
            // and `spec_revisions`, so the wire cannot disagree with the canvas.
            "build": opening_implementation(n.id).stack.word(),
            "speaks": opening_implementation(n.id).speaks.word(),
        })).collect::<Vec<_>>(),
        "links": spec::LINKS.iter().map(|(a, b)| serde_json::json!([a, b])).collect::<Vec<_>>(),
        "selected_link": [spec::SELECTED_LINK.0, spec::SELECTED_LINK.1],
        "selected_node": spec::SELECTED_NODE,
        // ★★ R1716 — the axis columns travel with the row. An agent expanding
        // `fields.derived` or `fields.aside` reads them from here, the same way
        // the local gate does, so the two cannot come to disagree about which
        // population a voice family stands over.
        "fields": spec::FIELDS.iter().map(|f| serde_json::json!({
            "key": f.key, "ty": f.ty, "applies": f.applies, "value": f.value,
            "source": f.source, "aside": f.aside,
        })).collect::<Vec<_>>(),
        "addable": spec::ADDABLE,
        "gestures": spec::GESTURES.iter().map(|(g, w)| serde_json::json!([g, w])).collect::<Vec<_>>(),
        // ★ R1678 — the reset affordances, and which of them are CONDITIONAL.
        // Published rather than left for a reader to infer from the operations
        // list, because the conditional ones are the reason a backward check
        // must accept a tag that is not always there — and R1664 is what
        // happens when a family reaches the paint tree and not this table.
        "resets": ResetScope::ALL.iter().map(|scope| serde_json::json!({
            "scope": scope.wire(),
            "gated": scope.gated(),
        })).collect::<Vec<_>>(),
        // ★★ R1677 — what the screen can be asked to DO, beside what it has.
        // Published for the same reason every other row here is: a demo that
        // carried its own copy of this list would be checking the list against
        // itself. `absent` is derived rather than stored, so an operation
        // cannot be declared missing and reachable at once.
        "operations": spec::OPERATIONS.iter().map(|op| serde_json::json!({
            "name": op.name,
            "verb": op.verb.map(|(verb, arg)| serde_json::json!([verb, arg])),
            "gesture": op.gesture,
            "witness": op.witness,
            "needs": op.needs,
            "absent": op.verb.is_none() && !op.gesture,
        })).collect::<Vec<_>>(),
        // ★★★ R1689 — **what a save carries, and what it deliberately does
        // not.** Published for the reason the reference publishes its own
        // version of this: it is a claim about the tool that a reader — a
        // person deciding whether to close the window, an agent deciding
        // whether saving is enough — cannot get any other way. `keeps` is the
        // partition and `why` is where the thing lives, which is the half a
        // bare boolean cannot say.
        "kept": spec::KEPT.iter().map(|k| serde_json::json!({
            "witness": k.witness,
            "keeps": match k.keeps {
                spec::Keeps::Saved => "saved",
                spec::Keeps::Volatile => "volatile",
            },
            "why": k.why,
        })).collect::<Vec<_>>(),
        // ★★★★ R1691 — **what a reader is told this screen has**, as the split
        // between the regions that owe a voice and the ones that owe a declared
        // silence. Published for the reason every other table here is: a demo
        // that carried its own copy would be checking the copy.
        //
        // The `population` names the table a family expands from rather than
        // listing the expansion, so a ninth role or a sixth field is demanded
        // without anybody editing either copy.
        "voices": spec::VOICES.iter().map(|v| serde_json::json!({
            "tag": v.tag,
            "role": v.role,
            "population": population_wire(v.population),
        })).collect::<Vec<_>>(),
        "silences": spec::SILENCES.iter().map(|(tag, population, kind)| serde_json::json!({
            "tag": tag,
            "population": population_wire(*population),
            "reason": kind,
        })).collect::<Vec<_>>(),
        "graph": spec::GRAPH_NAME,
        "zoom": spec::OPENING_ZOOM,
        // ★★★ R1687 — the smallest window this screen says it can paint.
        //
        // Published because a demo needs it and two of them were carrying their
        // own copy of the number. R1687 moved the floor (one button in the
        // toolbar, and the toolbar is what dictates it) and the copies did not
        // move with it — a demo asked for 1440, the shell clamped to 1442, and
        // the demo failed on a fact about the screen rather than on a defect.
        // The same second-copy failure the operations table was published to
        // prevent, one level down.
        "floor": [MIN_W, MIN_H],
        // ★★★ R1688 — and the size the window OPENS at, which is a different
        // number and the one a demo taking a paint snapshot actually needs.
        //
        // R1687 published the floor and left the design size to be written out
        // by whoever needed it; this round moved BOTH (the zoom pill grew the
        // fit seat) and the second copy is exactly the thing that then does not
        // move. It is never below the floor — see [`WIN_W`], where that stopped
        // being something a round could get wrong.
        "design": [WIN_W, WIN_H],
        // ★★★★★ R1732 — **the inspector's own specification**, straight out of
        // the pin, and the configuration path whose roster the gates drive.
        //
        // Published for the reason every table above it is: a demo that wrote
        // down what the reference's row is made of would be checking its own
        // copy, and R1730 measured what that costs — five demos broke on
        // hand-written rosters in one round, one of which had been red for
        // three. The `owed` half travels with it, because "this part is not
        // there and here is why" is exactly the thing a reader cannot get any
        // other way.
        "inspector": spec::inspector_document().to_json(),
        // ★★★★★ R1857 — **the fault-injection panel's shape**, published for the
        // reason every table above it is: a demo that wrote down what the panel
        // is made of would be checking its own copy. R1853 published the panel's
        // CONTENTS (`faults`, `fault_scopes`) and never its shape, so the wire
        // said what the offers are and nothing said the screen has a panel at
        // all — and the backward check, which builds what it will accept out of
        // exactly this value, accepted none of it.
        //
        // The counts stay out on purpose: how many rows there are is a fact
        // about the selected card's declaration, and it is read from `faults`.
        "faults_panel": {
            "tag": spec::FAULT_PANEL.tag,
            "head": spec::FAULT_PANEL.head,
            "row_stem": spec::FAULT_PANEL.row_stem,
            "row_parts": spec::FAULT_PANEL.row_parts,
            "scope_stem": spec::FAULT_PANEL.scope_stem,
        },
        "enum_key": spec::ENUM_KEY,
    })
}

// ★ R1747 — `inspector_spec_json` lived here and is now
// `SpecDocument::to_json`. R1732 wrote it for this screen; the capture viewer
// needed it verbatim, and two sections publishing one document in two shapes is
// the defect R1738 exists to prevent one level up — a client walking a
// section's published specification must not have to know which section it is
// talking to.

/// Re-derive what a node's *pins* mean from its form.
///
/// The one place the two halves meet: an endpoint edited in the inspector
/// changes what the canvas draws and what the gate says, and it does so by
/// re-deriving rather than by a second write.
fn sync_node(state: &Rc<LabState>, node: NodeId) {
    let forms = state.forms.borrow();
    let Some(form) = forms.get(&node) else {
        return;
    };
    let listen = form
        .field("listen.endpoints")
        .map_or(String::new(), |f| f.value().into_owned());
    let transport = Transport::of_locator(&listen)
        .or_else(|| {
            form.field("connect.endpoints")
                .and_then(|f| Transport::of_locator(&f.value()))
        })
        .unwrap_or(Transport::Tcp);
    drop(forms);
    let mut doc = state.doc.borrow_mut();
    if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(node)) {
        if let NodeBody::Kind(kind) = &mut slot.body {
            kind.transport = transport;
            kind.listening = !listen.trim().is_empty();
        }
    }
}

/// The locators a node listens on, in the order its form lists them.
///
/// ★★ R1681 — the population behind every endpoint decision on this screen.
/// A node listening in two places can be dialled in two ways, and *which one a
/// link took* is a property of the link, not of the node: it is the first thing
/// the reference says about its own equivalent, and it is why an endpoint can
/// be re-chosen on a wire that is already drawn.
fn endpoints_in(forms: &BTreeMap<NodeId, ConfigForm>, node: NodeId) -> Vec<String> {
    forms
        .get(&node)
        .and_then(|form| {
            form.field("listen.endpoints")
                .map(|f| f.value().into_owned())
        })
        .map(|value| {
            FieldType::elements(&value)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn endpoints_of(state: &LabState, node: NodeId) -> Vec<String> {
    endpoints_in(&state.forms.borrow(), node)
}

/// Which endpoint the link landing on `socket` dialled.
///
/// The accept run carries one item per link that lands on the node, and the
/// item's **label is the endpoint**. That is the whole of the endpoint model:
/// one fact, in the place the model already keeps per-slot facts, so nothing
/// maintains a parallel table of which wire took which address.
fn endpoint_of(doc: &Document<LabNode>, socket: Socket) -> Option<String> {
    doc.items(ROOT, socket.node, Side::Input)?
        .get(socket.port as usize)
        .and_then(|item| item.label.clone())
}

fn endpoint_at(state: &LabState, socket: Socket) -> Option<String> {
    endpoint_of(&state.doc.borrow(), socket)
}

/// Make an accept slot on `to` that dials `endpoint`, and answer its port.
///
/// Always a fresh slot: an accept endpoint is a listening socket and is dialled
/// by as many peers as reach it, while the model's value input takes one
/// producer, so the run is how the many-ness is expressed and one item per
/// arriving link is what keeps the two consistent. The item is **typed** by the
/// endpoint's own transport, which is what makes a dial that cannot speak it a
/// refusal the model states rather than a defect the gate notices later.
/// `endpoint` is `None` for a node that can be dialled and has **nowhere to
/// listen** — a real state on this screen, and one the launch gate already
/// names rather than one the canvas should refuse to draw. The slot is then
/// unlabelled, which is exactly true: the link dials no address.
fn open_slot_in(doc: &mut Document<LabNode>, to: NodeId, endpoint: Option<&str>) -> Option<u32> {
    let arity = u32::try_from(doc.signature(ROOT, to)?.inputs.len()).unwrap_or(0);
    let item = match endpoint {
        Some(one) => Item::plain().named(one).typed(
            0,
            graph::Endpoint::Locator(Transport::of_locator(one).unwrap_or(Transport::Tcp)),
        ),
        None => Item::plain(),
    };
    doc.insert_item(ROOT, to, Side::Input, arity, item).ok()?;
    let mut port = arity;
    // ★ The run declares `at_least(1)`, so a node that has never been dialled
    // still carries one slot — and appending beside it would leave every
    // accepting node with a dead port forever. Measured: the opening graph had
    // one per accepting node, found by a census that counts BOTH directions.
    // Dropped here rather than never created, because the floor is the crate's
    // and this is the first moment a real slot exists to replace it.
    if let Some(dead) = spare_slot(doc, to, port) {
        if doc.remove_item(ROOT, to, Side::Input, dead).is_ok() && dead < port {
            port -= 1;
        }
    }
    // ★★★★★ R1914 — the slot carries the address it accepts as an AUTHORED
    // VALUE, not only as a label.
    //
    // The label is what the canvas draws; the value is what the model can take
    // apart. Until this round a locator on this screen lived only in a name, so
    // splitting the pin would have produced a host and a service with nothing
    // in them — which is exactly the state the reference's split avoids by
    // parsing the parent's value on the way down.
    if let Some(one) = endpoint {
        let _ = doc.set_port_value(ROOT, to, PortRef::input(port), one.to_owned());
    }
    Some(port)
}

/// A slot before `keep` that names no address and holds nothing (R1681).
fn spare_slot(doc: &Document<LabNode>, node: NodeId, keep: u32) -> Option<u32> {
    let items = doc.items(ROOT, node, Side::Input)?;
    (0..keep).find(|port| {
        let empty = items
            .get(*port as usize)
            .is_none_or(|item| item.label.is_none());
        let socket = Socket::new(node, *port);
        empty
            && !doc
                .tree(ROOT)
                .is_some_and(|t| t.links().iter().any(|l| l.to == socket))
            && !doc.observations(ROOT).iter().any(|o| o.to == socket)
    })
}

fn open_slot(state: &LabState, to: NodeId, endpoint: Option<&str>) -> Option<u32> {
    open_slot_in(&mut state.doc.borrow_mut(), to, endpoint)
}

/// The endpoint a new link from `from` would dial on `to`, or why it cannot be
/// made (R1681).
///
/// Three answers and not two: an endpoint, **no endpoint at all** for a node
/// that listens nowhere (which is drawable and is what the launch gate is for),
/// and a refusal when this dialler has already taken every address the target
/// offers — which is the reference's rule and is about the *pair*, since two
/// different peers may of course dial the same address.
fn landing_endpoint(
    doc: &Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    from: NodeId,
    to: NodeId,
) -> Result<Option<String>, ()> {
    if endpoints_in(forms, to).is_empty() {
        return Ok(None);
    }
    free_endpoints_in(doc, forms, from, to)
        .into_iter()
        .next()
        .map_or(Err(()), |one| Ok(Some(one)))
}

/// Drop the accept slot at `port`, now that nothing lands on it.
///
/// The run would otherwise only ever grow: every link that arrives opens a
/// slot, so every link that leaves has to close one. `remove_item` re-points
/// the links past it, which is the reason this is one crate call and not an
/// index fixup here.
fn close_slot(state: &LabState, node: NodeId, port: u32) {
    let still_used = state
        .doc
        .borrow()
        .tree(ROOT)
        .is_some_and(|t| t.links().iter().any(|l| l.to == Socket::new(node, port)))
        || state
            .doc
            .borrow()
            .observations(ROOT)
            .iter()
            .any(|o| o.to == Socket::new(node, port));
    if still_used {
        return;
    }
    state
        .doc
        .borrow_mut()
        .remove_item(ROOT, node, Side::Input, port)
        .ok();
}

/// Which endpoints of `to` this dialler has not already taken.
///
/// The reference's rule, and it is about the **pair**: a second wire between
/// the same two nodes has to dial a different address, because that is what a
/// second transport connection is, while two *different* peers may of course
/// dial the same one.
fn free_endpoints_in(
    doc: &Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    from: NodeId,
    to: NodeId,
) -> Vec<String> {
    // Reported links hold an endpoint too: the world took it, so a drawing
    // that claimed the same one would be describing a connection that is not
    // the one out there.
    let landed = doc
        .tree(ROOT)
        .map(|t| {
            t.links()
                .iter()
                .filter(|l| l.from.node == from && l.to.node == to)
                .map(|l| l.to)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reported = doc
        .observations(ROOT)
        .into_iter()
        .filter(|o| o.from.node == from && o.to.node == to)
        .map(|o| o.to);
    let used: Vec<String> = landed
        .into_iter()
        .chain(reported)
        .filter_map(|socket| endpoint_of(doc, socket))
        .collect();
    endpoints_in(forms, to)
        .into_iter()
        .filter(|one| !used.contains(one))
        .collect()
}

/// Author a link, letting the crate refuse it.
/// ★★★★★ R1915 — author a link between two **addressed** pins.
///
/// `connect` is this with both addresses at the root, and that is not a wrapper
/// for tidiness: a wire between two whole pins opens a slot on the run the
/// accept pin repeats, and a wire onto a MEMBER lands on a port that already
/// exists. Two different edits, one question — *which port* — so it is one verb
/// with an address on each end.
///
/// # Errors
///
/// A refusal the model made, in the model's words: an address naming no port, a
/// pair the taxonomy will not cross (a host half does not reach a whole
/// locator, which is the type rule doing exactly its job), or the slot-opening
/// refusals `connect` already had.
fn connect_at(
    state: &Rc<LabState>,
    from: NodeId,
    from_at: &PortPath,
    to: NodeId,
    to_at: &PortPath,
) -> Result<String, InvokeError> {
    // A wire between two whole pins is the gesture this screen has always had:
    // the accept side is a variadic run, so landing on it means OPENING a slot.
    if from_at.depth() == 0 && to_at.depth() == 0 {
        return connect(state, from, to);
    }
    let addressed = |node: NodeId, at: &PortPath, side: Side| -> Result<u32, InvokeError> {
        state
            .doc
            .borrow()
            .index_of(ROOT, node, side, at)
            .ok_or_else(|| {
                let said = Utterance::refused(&format!(
                    "{} has no {} pin — split it first",
                    state.name_of(node),
                    pin_word(side, at)
                ));
                state.say(said.clone());
                InvokeError::rejected(said.into_clause())
            })
    };
    let out = addressed(from, from_at, Side::Output)?;
    let into = addressed(to, to_at, Side::Input)?;
    let made = state
        .doc
        .borrow_mut()
        .connect(ROOT, Socket::new(from, out), Socket::new(to, into));
    match made {
        Ok(made) => {
            state.selected_link.set(Some(LinkPick::Authored(made.link)));
            let word = format!(
                "{}.{} -> {}.{}",
                state.name_of(from),
                pin_word(Side::Output, from_at),
                state.name_of(to),
                pin_word(Side::Input, to_at),
            );
            state.say(Utterance::done(format!("linked {word}")));
            Ok(word)
        }
        Err(why) => {
            // ★ Nothing to undo: a member port was not opened for this wire,
            // it was already there. That asymmetry with `connect` is the whole
            // difference between the two edits, and it is why the slot-closing
            // line below has no counterpart here.
            let said = Utterance::refused(&why);
            state.say(said.clone());
            Err(InvokeError::rejected(said.into_clause()))
        }
    }
}

fn connect(state: &Rc<LabState>, from: NodeId, to: NodeId) -> Result<String, InvokeError> {
    let name = state.name_of(to);
    let Ok(endpoint) = landing_endpoint(&state.doc.borrow(), &state.forms.borrow(), from, to)
    else {
        // ★★★ R1719 — the shape this file carries eight times: one sentence,
        // said to the person and handed to the agent. The person's copy is
        // framed (`refused: …`) and the agent's is not, because the agent's
        // channel is already a refusal — and both come off one value, so they
        // cannot drift.
        let said = Utterance::refused(&format!(
            "{} already dials every endpoint of {name}",
            state.name_of(from)
        ));
        state.say(said.clone());
        return Err(InvokeError::rejected(said.into_clause()));
    };
    let Some(port) = open_slot(state, to, endpoint.as_deref()) else {
        // ★★★★ R1720 — ONE sentence, where this site had two. It said
        // `{name} has no accept pin` to the person and
        // `{name} does not listen, so nothing can dial it` to the agent, about
        // the same fact — the drift R1719 built the shared value to prevent,
        // surviving three sites away from where it was written. The framework
        // now announces the refusal it hands back, so a second wording here
        // would not merely be untidy: it would be overwritten a moment later
        // by the one the agent got, and the person would read whichever the
        // path chose.
        let said = Utterance::refused(&format!("{name} does not listen, so nothing can dial it"));
        state.say(said.clone());
        return Err(InvokeError::rejected(said.into_clause()));
    };
    let made = state
        .doc
        .borrow_mut()
        .connect(ROOT, Socket::new(from, 0), Socket::new(to, port));
    match made {
        Ok(made) => {
            state.selected_link.set(Some(LinkPick::Authored(made.link)));
            let word = format!("{} -> {}", state.name_of(from), state.name_of(to));
            match &endpoint {
                Some(one) => state.say(Utterance::done(format!("linked {word} on {one}"))),
                None => state.say(Utterance::done(format!("linked {word}"))),
            }
            Ok(word)
        }
        Err(why) => {
            // The slot was opened for a link that did not arrive.
            close_slot(state, to, port);
            // ★ R1699 — `Display`, not `Debug`: this sentence reaches a person
            // in the toast AND an agent as the refusal's own reason, and `Debug`
            // puts Rust syntax with escaped quotes in front of both.
            // ★★ R1719 — and that is no longer a thing to remember:
            // `Utterance::refused` takes something that can say itself, and a
            // `Debug` spelling handed in anyway is a fault the constructor
            // names.
            let said = Utterance::refused(&why);
            state.say(said.clone());
            Err(InvokeError::rejected(said.into_clause()))
        }
    }
}

/// Remove a link somebody drew, and close the slot it was landing on (R1681).
fn delete_link(state: &Rc<LabState>, link: LinkId) -> Result<String, InvokeError> {
    let gone = state.doc.borrow_mut().disconnect(ROOT, link);
    match gone {
        Ok(gone) => {
            close_slot(state, gone.to.node, gone.to.port);
            if state.selected_link.get() == Some(LinkPick::Authored(link)) {
                state.selected_link.set(None);
            }
            let word = format!(
                "{} -> {}",
                state.name_of(gone.from.node),
                state.name_of(gone.to.node)
            );
            state.say(Utterance::done(format!("unlinked {word}")));
            Ok(word)
        }
        Err(why) => {
            let said = Utterance::refused(&why);
            state.say(said.clone());
            Err(InvokeError::rejected(said.into_clause()))
        }
    }
}

// ── A node's life ───────────────────────────────────────────────────────────

/// Take a card off the canvas, with everything that hung on it (R1682).
///
/// ★ **The last card cannot go.** The reference refuses the same way and for
/// the same reason: a graph editor with an empty canvas has no selection, so
/// the inspector, the gate panel and every affordance keyed to a selected node
/// vanish at once — a state a person reaches by pressing delete one time too
/// many and cannot leave.
fn delete_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let name = state.name_of(node);
    if state.cards().len() <= 1 {
        let said = Utterance::refused(&format!("{name} is the last card, so it stays"));
        state.say(said.clone());
        return Err(InvokeError::rejected(said.into_clause()));
    }
    // The document answers what the removal took with it, which is the half of
    // the edit that is not where the gesture happened.
    let taken = state
        .doc
        .borrow_mut()
        .remove_node(ROOT, node)
        .map_err(|why| InvokeError::rejected(why.to_string()))?;
    // Every accept run this card was dialling keeps a slot per link, so the
    // links it took with it have to give their seats back — the same close the
    // link deletion does, for the same reason (R1681.1).
    for link in &taken.links {
        if link.to.node != node {
            close_slot(state, link.to.node, link.to.port);
        }
    }
    state.forms.borrow_mut().remove(&node);
    state.opened_at.borrow_mut().remove(&node);
    if state.active_card() == Some(node) {
        select_card(state, state.cards().first().copied());
    }
    // A picked link that ran through this card is a name for something that is
    // no longer there.
    let dangling = match state.selected_link.get() {
        Some(LinkPick::Authored(id)) => taken.links.iter().any(|l| l.id == id),
        Some(LinkPick::Observed(from, to)) => from.node == node || to.node == node,
        None => false,
    };
    if dangling {
        state.selected_link.set(None);
    }
    state.say(Utterance::done(format!(
        "deleted {name}, and {} link(s)",
        taken.links.len()
    )));
    Ok(name)
}

// ── The one text field ──────────────────────────────────────────────────────

/// Open the shared field on `what`, seeded with the value it is about (R1683).
///
/// Seeded rather than blank, which is the reference's own choice for the name
/// box and the right one: a rename usually adjusts a name rather than replacing
/// it, and a person who wanted to replace it selects all. The key box opens
/// empty because there is no key yet.
/// Answers the seed rather than a `Result`: there is nothing here to refuse
/// once the buffer is the screen's own, which is what taking it at
/// construction bought.
fn begin_edit(state: &Rc<LabState>, what: Editing) -> Result<String, InvokeError> {
    // ★★★ R1684 — **opening it somewhere else applies what is in it.**
    //
    // Found by looking at the running screen: a value typed into one row and
    // then abandoned by pressing another row vanished without a word. The
    // house style is already commit-on-blur — R1683 mounted
    // `blur_committing_field_extra` for exactly this — but that external
    // never sees the pointer, because every press on this screen goes to its
    // one root external (measured this round). So the screen does it.
    //
    // A REFUSED commit refuses the move: the field stays where it is, holding
    // the text, and the toast says why. Switching anyway would destroy the
    // thing the refusal was about. Escape is still how a person abandons.
    if state.editing.get().is_some_and(|open| open != what) {
        commit_edit(state)?;
    }
    // ★★★★★ R1732 — and the mirror of the rule `open_roster` keeps. A roster
    // standing open while a field takes the keyboard is the same two-editors
    // state seen from the other side. Shutting it writes NOTHING: dismissing is
    // not choosing, so the row keeps the word it had.
    close_roster(state);
    let buffer = &state.buffer;
    let seed = match &what {
        Editing::Name(node) => state.name_of(*node),
        Editing::Key(_) => String::new(),
        // ★ R1684 — the row's CURRENT text, in the form's own spelling, so a
        // list arrives as the separated string the form reads back and a
        // person edits what they were looking at. The form is the one thing
        // that knows how a shape is written down; deriving the seed here would
        // be a second spelling of every shape — and an element is split out
        // with the form's own splitter for exactly that reason.
        Editing::Value { node, key, element } => selected_form_of(state, *node)
            .and_then(|form| {
                let field = form.field(key)?;
                // ★★★★★ R1717 — the box opens over the half being EDITED. A
                // shared row shows what somebody wrote composed with what the
                // canvas worked out, and a box seeded from the composition
                // would write the canvas's contribution into their half the
                // moment it was applied — the freeze this round exists to
                // prevent, arriving through the seed instead of the commit.
                let held = match field.written() {
                    Some(written) => written.to_owned(),
                    None => field.value().into_owned(),
                };
                Some(match element {
                    None => held,
                    Some(n) => FieldType::elements(&held).nth(*n)?.to_owned(),
                })
            })
            .unwrap_or_default(),
    };
    buffer.set_text(seed.clone());
    // Selected whole, so the first keystroke replaces rather than appends —
    // which is what a box that opens already holding a value has to do.
    buffer.set_selection(0, seed.len());
    let said = what.wire();
    state.editing.set(Some(what));
    pinion_core::focus_request::request(EDIT_TAG);
    state.say(Utterance::done(format!("editing the {said}")));
    Ok(seed)
}

/// What a press on one of the field's two seats does (R1683).
///
/// ★ One seat with two jobs and one with one, which is what makes the field's
/// state readable from the buttons: the name seat opens the box when it is shut
/// and applies what is in it when it is open, and the key seat always opens on
/// a path. The reference's own box works the same way.
fn edit_seat(state: &Rc<LabState>, which: &Hit) {
    let Some(node) = state.active_card() else {
        return;
    };
    // A refusal has already reached the toast, which is where a person reads
    // it — the same arrangement the node's-life seats have.
    let _ = match which {
        Hit::AddKey => begin_edit(state, Editing::Key(node)),
        _ if state.editing.get().is_some() => commit_edit(state),
        _ => begin_edit(state, Editing::Name(node)),
    };
}

/// Take what the field holds and do the thing it was opened for (R1683).
///
/// ★ The commit is the SAME verb the wire's own action calls, so a name typed
/// into the box and a name handed to `rename` cannot be refused differently.
/// A refusal leaves the field open with the text still in it, because a person
/// whose name was rejected wants to edit it, not to type it again.
fn commit_edit(state: &Rc<LabState>) -> Result<String, InvokeError> {
    let Some(what) = state.editing.get() else {
        return Err(InvokeError::rejected("nothing is being edited"));
    };
    let text = state.buffer.text();
    let done = match what {
        Editing::Name(node) => rename_card(state, node, text.trim()),
        Editing::Key(node) => add_key(state, node, text.trim()),
        // ★ R1684 — the same function the wire's `set_field` calls, with the
        // text as typed. What gets trimmed is the wire's ARGUMENT — the
        // `<key>=<value>` grammar needs it — and not the value, which is the
        // shape's business: the integer shape trims for itself and the text
        // shape must not, or a value with a space in it could never be typed.
        Editing::Value {
            node,
            key,
            element: None,
        } => set_value(state, node, &key, &text),
        Editing::Value {
            node,
            key,
            element: Some(n),
        } => set_element(state, node, &key, n, &text),
    };
    if done.is_ok() {
        end_edit(state);
    }
    done
}

/// Pick a card, and shut the field if that changes which card is inspected
/// (R1684).
///
/// ★★ One function for every site that moves the selection — the `select`
/// action, a press on a card, the card the palette just added, and the card a
/// deletion falls back to — because the field is opened OVER the inspected
/// card's row and a selection that moved without shutting it would leave a box
/// standing on a form that is no longer underneath it. The target names its own
/// card, so a stale commit would land correctly and the person would still have
/// typed into the wrong-looking place.
fn select_card(state: &Rc<LabState>, node: Option<NodeId>) {
    apply_selection(state, node);
}

/// ★★★ R1706 — select every card a host frame holds, the first of them
/// leading.
///
/// The reference's frame gesture is one gesture with two halves: its frame-drag
/// handler *selects the group* on its first line and then moves it, so a person
/// who grabs a host by its tab has both picked it and started to carry it. This
/// screen had only the second half — the drag moved the cards and the inspector
/// never noticed, so pressing a host that held six cards left the panel showing
/// whichever card had been selected before.
///
/// The frame itself is not a member. It has no configuration of its own to
/// inspect, and its rectangle is *derived* from the cards it holds
/// ([`frame_rect_of`]), so a frame in the set would be a member whose position
/// is a function of the other members — which the group drag would then move
/// twice.
fn select_frame(state: &Rc<LabState>, frame: NodeId) -> Vec<NodeId> {
    let members = members_of(state, frame);
    apply_selection(state, members.iter().copied());
    members
}

/// ★★★ R1706 — pick a host by name and carry it and its cards by `delta`.
///
/// The wire half of the frame gesture, and it goes through
/// [`select_frame`] and [`shift_cards`] — the same two calls the press and the
/// drag make — so the two channels cannot come to mean different things. A
/// host with no cards is refused rather than silently doing nothing: its
/// rectangle is derived from the cards it holds, so there is nothing on screen
/// to have moved.
fn move_frame(state: &Rc<LabState>, host: &str, delta: (i32, i32)) -> Result<String, InvokeError> {
    let frame = frames_of(state)
        .into_iter()
        .find(|(_, name)| name == host)
        .map(|(id, _)| id)
        .ok_or_else(|| InvokeError::rejected(format!("no host is called {host:?}")))?;
    let members = select_frame(state, frame);
    if members.is_empty() {
        return Err(InvokeError::rejected(format!(
            "host {host:?} holds no cards, so there is nothing to move"
        )));
    }
    shift_cards(state, &members, frame, delta);
    Ok(format!(
        "moved {host} and its {} card(s) by {},{}",
        members.len(),
        delta.0,
        delta.1
    ))
}

/// Move a host's cards, and the host's own stored position with them.
///
/// ★ R1706 — lifted out of the drag arm because the wire verb needs the same
/// arithmetic, and a second copy of "and the frame itself moves too" is exactly
/// how the two channels drift.
fn shift_cards(state: &LabState, members: &[NodeId], frame: NodeId, delta: (i32, i32)) {
    let mut doc = state.doc.borrow_mut();
    let Some(tree) = doc.tree_mut(ROOT) else {
        return;
    };
    for id in members.iter().copied().chain(std::iter::once(frame)) {
        if let Some(slot) = tree.node_mut(id) {
            slot.x = clamp_to_world(slot.x + delta.0);
            slot.y = clamp_to_world(slot.y + delta.1);
        }
    }
}

/// Move the selection, shutting the open field when the inspected card changes
/// (R1684).
///
/// ★★ One function for every site that moves the selection — the `select`
/// action, a press on a card, a press on a host frame, the card the palette just
/// added, and the card a deletion falls back to — because the field is opened
/// OVER the inspected card's row and a selection that moved without shutting it
/// would leave a box standing on a form that is no longer underneath it. The
/// target names its own card, so a stale commit would land correctly and the
/// person would still have typed into the wrong-looking place.
///
/// ★ R1706 — the field is shut on
/// [`active_moved`](pinion_core::selection::Change::active_moved), not on "the
/// selection changed". Growing a selection that keeps its leader leaves the
/// inspector showing the same card, so shutting the box there would take a
/// person's half-typed value away for a reason nothing on screen explains.
fn apply_selection<I: IntoIterator<Item = NodeId>>(state: &Rc<LabState>, cards: I) {
    let mut selection = state.selection.get();
    let change = selection.set_group(cards);
    if !change.changed() {
        return;
    }
    state.selection.set(selection);
    // ★★★★★ R1736 — and it SAYS so, whichever channel changed it.
    //
    // Measured three times before this line existed: the wire's `select` said
    // "selected T-01" and a press said nothing at all, because the sentence sat
    // at the INVOKE site rather than at the act. So the reader who uses the
    // pointer heard nothing and the agent who uses the wire heard everything —
    // the wrong way round, and R1720's rule ("a confirmation reaches a person
    // too") holed on its pointer side.
    //
    // Here rather than at either caller because this is the one place a
    // selection changes, and it is already the place that knows whether it DID
    // change: a press that re-selects the card already showing says nothing,
    // which is what makes the sentence worth reading when it appears.
    say_selection(state);
    if change.active_moved() && state.editing.get().is_some() {
        // ★ Applied, then shut — the same rule as opening the field somewhere
        // else, with the one difference the situation forces: a refusal cannot
        // keep the box open, because the card it was opened over is not the
        // inspected card any more. The refusal has already reached the toast.
        commit_edit(state).ok();
        end_edit(state);
    }
}

/// ★★★★★ R1736 — what the selection now is, in a sentence a person reads.
///
/// Three shapes because the selection has three, and a reader told "selected"
/// with no subject has been told nothing: nothing at all, one card, or a group
/// whose leading card is the one the inspector is showing. The count is stated
/// for the group because "selected P-01" over six selected cards would be true
/// of the inspector and false of the canvas.
fn say_selection(state: &Rc<LabState>) {
    let selection = state.selection.get();
    let Some(active) = selection.active().copied() else {
        state.say(Utterance::done("nothing selected"));
        return;
    };
    let name = state.name_of(active);
    let rest = selection.len().saturating_sub(1);
    state.say(Utterance::done(if rest == 0 {
        format!("selected {name}")
    } else {
        format!("selected {name} and {rest} more")
    }));
}

/// Shut the field, leaving whatever it was editing alone.
fn end_edit(state: &Rc<LabState>) {
    state.editing.set(None);
    state.buffer.set_text(String::new());
    pinion_core::focus_request::request(VIEW_TAG);
}

/// Add a configuration path the catalogue does not offer (R1683).
///
/// ★★ The half of "add a field" the chips cannot do. The catalogue is a list of
/// the paths worth reaching for, not the boundary of what a configuration has —
/// the reference says exactly that beside its own key box — so any path the
/// form will accept can be typed. An already-held key is refused rather than
/// silently duplicated.
fn add_key(state: &Rc<LabState>, node: NodeId, key: &str) -> Result<String, InvokeError> {
    if key.is_empty() {
        return Err(InvokeError::rejected(
            "a key with nothing in it is not a key",
        ));
    }
    // ★★ What a typed path IS — its type, its shape, whether it reaches a
    // running node — is this application's knowledge, not the widget's. A path
    // the catalogue already describes keeps that description; anything else
    // arrives as text that applies on restart, which is the safe reading of a
    // key nobody has classified.
    let described = spec::ADDABLE
        .iter()
        .find(|offered| **offered == key)
        .map_or_else(
            || ConfigField::new(key.to_owned(), "text", Applies::Restart, ""),
            |known| offered(known),
        );
    let outcome = amend(state, node, |form| form.add_typed(described));
    if let Err(why) = outcome {
        let said = Utterance::refused(&why);
        state.say(said.clone());
        return Err(InvokeError::rejected(said.into_clause()));
    }
    sync_node(state, node);
    state.say(Utterance::done(format!("added {key}")));
    Ok(key.to_owned())
}

/// Put a value on one row of a card's settings form (R1684).
///
/// ★★ **One function for the wire's `set_field` and for the box a person types
/// into**, which is the same discipline the rename has: a value typed and a
/// value handed over cannot be accepted differently, cannot be refused
/// differently, and cannot leave the card's pins derived from different text.
///
/// ★ The value is stored **as given**, defects and all. `ConfigForm::set` does
/// not validate, and that is the design the reference states beside its own
/// parser: a value that will not encode is held and reported on its row by the
/// launch gate, because silently reverting a person's typing tells them nothing
/// about why it went away. Only an unknown path is refused, which is the one
/// case where the request names something that is not there.
fn set_value(
    state: &Rc<LabState>,
    node: NodeId,
    key: &str,
    value: &str,
) -> Result<String, InvokeError> {
    // ★★ R1716 — through [`amend`], so the refusal a person meets on a row
    // nobody wrote is the framework's own sentence naming the source, rather
    // than "no such field" from a store that has never heard of it.
    let held = amend(state, node, |form| {
        form.set(key, value)?;
        Ok(form.field(key).map(|f| f.value().into_owned()))
    })
    .map_err(|why| InvokeError::rejected(why.to_string()))?;
    // The pins are DERIVED from the form, so a value that changes an endpoint
    // has to reach the canvas in the same act that changed it.
    sync_node(state, node);
    state.say(Utterance::done(format!("{key} = {value}")));
    Ok(held.unwrap_or_default())
}

/// Take a row out of the selected card's form (R1686).
///
/// ★★ **One function for the seat and for the wire**, which is the shape R1684
/// established for `set_field` after finding two paths that did different
/// things. Until this round the wire's arm was the only caller and it neither
/// said what it had done nor knew about the field — both of which stop being
/// optional the moment a person can press it.
///
/// What it has to do that the widget cannot:
///
/// * **Shut the field if it was open on this row.** A box standing over a row
///   that is gone is the [`select_card`] hazard one level down, and the value
///   in it is not applied — the reference drops the edit in the same act, and
///   applying to a row about to vanish would be writing to nothing.
/// * **Apply the field if it was open on a DIFFERENT row**, and let a refusal
///   refuse the removal, which is [`begin_edit`]'s rule: pressing a seat
///   elsewhere is leaving, and leaving applies.
/// * **Re-derive the card**, because a card's pins come from its form and
///   `listen.endpoints` is a row like any other.
///
/// # Errors
///
/// A card with no form, a key it does not hold, or a refused commit on the row
/// the field was left open on.
fn remove_row(state: &Rc<LabState>, node: NodeId, key: &str) -> Result<String, InvokeError> {
    match state.editing.get() {
        Some(Editing::Value {
            node: over,
            key: ref typed,
            ..
        }) if over == node && typed == key => end_edit(state),
        Some(_) => {
            commit_edit(state)?;
            end_edit(state);
        }
        None => {}
    }
    // ★★★ R1717 — asked BEFORE the act, because after it the row no longer
    // says it had two contributors. A row somebody shared with the canvas is
    // not removed by this; their half is given back, and a toast that said
    // "removed" over a row still on the screen would read as a tool that
    // ignored the press.
    let gave_back = shown_form(state, node)
        .as_ref()
        .and_then(|form| form.field(key))
        .and_then(|field| match field.source() {
            Source::Shared(from) => Some(from.into_owned()),
            Source::Authored | Source::Derived(_) => None,
        });
    amend(state, node, |form| form.remove(key)).map_err(|why| {
        let said = Utterance::refused(&why);
        state.say(said.clone());
        InvokeError::rejected(said.into_clause())
    })?;
    sync_node(state, node);
    match gave_back {
        Some(from) => state.say(Utterance::done(format!("{key} is the {from}'s again"))),
        None => state.say(Utterance::done(format!("removed {key}"))),
    }
    Ok(key.to_owned())
}

/// **Take a row over** — the row stops being worked out and becomes this
/// person's, holding what it was worked out to be (R1716).
///
/// One function for the seat and for the wire, the rule this screen has held
/// since R1684. What it adds beyond the widget's own act is the sentence: the
/// floor performs this silently, so a person who pressed it by accident has
/// nothing to read and nothing to undo by. Here the toast names the source that
/// was displaced, and the row is then removable like any other — which is the
/// way back.
///
/// # Errors
///
/// A card with no form, a key it does not hold, or a row that was already
/// theirs to write.
fn author_row(state: &Rc<LabState>, node: NodeId, key: &str) -> Result<String, InvokeError> {
    let took = amend(state, node, |form| form.author(key)).map_err(|why| {
        let said = Utterance::refused(&why);
        state.say(said.clone());
        InvokeError::rejected(said.into_clause())
    })?;
    sync_node(state, node);
    state.say(Utterance::done(took.sentence()));
    Ok(took.key)
}

/// Put one ELEMENT of a list row back, leaving its neighbours alone (R1684).
///
/// ★★ Text emptied removes the element, which is not a special case here: it
/// is what [`FieldType::elements`] already means, since the splitter the form
/// and the painter share drops the empty parts. So "clear it and apply" is how
/// a list loses a row, and the screen needs no delete affordance to answer for
/// the one the add affordance can create.
///
/// # Errors
///
/// A row this card does not have, or an element index the row does not hold —
/// which is a request naming something that is not there rather than a value
/// that is wrong, so it is refused instead of stored.
fn set_element(
    state: &Rc<LabState>,
    node: NodeId,
    key: &str,
    at: usize,
    text: &str,
) -> Result<String, InvokeError> {
    let row = selected_form_of(state, node)
        .and_then(|form| form.field(key).cloned())
        .ok_or_else(|| InvokeError::rejected(format!("{key:?} is not a row of this card")))?;
    // ★★★★★ R1717 — **an element the canvas contributed is not theirs to
    // write.** Driven the first time this state existed, an edit over one of
    // them wrote the canvas's whole contribution into their half AND moved its
    // neighbours; the refusal is the value's own, so every door into an
    // element passes it and none of them has to remember.
    if let Some(from) = row.element_source(at).derived_from() {
        let said = Utterance::refused(&FormError::Derived {
            key: format!("{key} element {}", at + 1),
            from: from.to_owned(),
        });
        state.say(said.clone());
        return Err(InvokeError::rejected(said.into_clause()));
    }
    // Only the WRITTEN half is rewritten; the derivation joins it again on the
    // next read, which is what keeps an edit from freezing the drawing.
    let held = row.written().unwrap_or_default().to_owned();
    let mut elements: Vec<String> = FieldType::elements(&held).map(str::to_owned).collect();
    let slot = elements
        .get_mut(at)
        .ok_or_else(|| InvokeError::rejected(format!("{key:?} has no element {at}")))?;
    text.trim().clone_into(slot);
    elements.retain(|element| !element.is_empty());
    set_value(state, node, key, &elements.join(FieldType::SEPARATOR))
}

/// Give a card a different name, keeping it the same card (R1682).
///
/// One verb for both callers — the rename action and the node reset putting a
/// name back — so "what a name is" has one definition. The refusal comes
/// straight from the model, which is the thing that knows whether a name is
/// already taken.
fn rename_card(state: &Rc<LabState>, node: NodeId, to: &str) -> Result<String, InvokeError> {
    let was = state.name_of(node);
    let done = state
        .doc
        .borrow_mut()
        .relabel(ROOT, node, Some(to))
        .map_err(|why| {
            let said = Utterance::refused(&why);
            state.say(said.clone());
            InvokeError::rejected(said.into_clause())
        })?;
    // ★ Nothing else to carry, and that is the measurement rather than an
    // omission: every other per-card record on this screen — the form, the
    // placement, the frame, the links — is keyed by the node's IDENTITY, which
    // a rename does not touch. The reference prototype has to move ten side
    // tables here because its rename remakes the node.
    // ★★★★★ R1719 — the `else` this round is named for. Renaming a card to the
    // name it already has changed nothing and said NOTHING, so the previous
    // message — measured: an earlier *refusal* — stayed on screen and a person
    // read a sentence about a different act. "It was already so" is a thing to
    // say, not a case to fall out of.
    state.say(if done.changed {
        Utterance::done(format!("{was} -> {to}"))
    } else {
        Utterance::unchanged(format!("{to} is already its name"))
    });
    Ok(to.to_owned())
}

/// Draw a card small, or full size again — a look, never a meaning (R1682).
fn collapse_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let now = {
        let mut doc = state.doc.borrow_mut();
        let slot = doc
            .tree_mut(ROOT)
            .and_then(|tree| tree.node_mut(node))
            .ok_or_else(|| InvokeError::rejected("no such card"))?;
        slot.appearance.collapsed = !slot.appearance.collapsed;
        slot.appearance.collapsed
    };
    let name = state.name_of(node);
    state.say(Utterance::done(format!(
        "{name} {}",
        if now { "collapsed" } else { "expanded" }
    )));
    Ok(now.to_string())
}

/// ★★★★★ R1912 — **put a card's pins away, or bring them all back.**
///
/// `which` is the reference's own scope vocabulary, spelled as one word so the
/// seat and the wire reach the same verb:
///
/// * `unwired` — the DCC's *toggle unused node socket display*, which is what
///   the inspector seat presses. It TOGGLES, like the reference's operator:
///   anything away means the press brings it all back.
/// * `dial` / `accept` — the engine's *remove this pin*, on the two this lab
///   draws.
/// * `others` — the engine's *remove all other pins*, about the pin named after
///   it (`others:dial`).
/// * `restore` — the engine's *restore all structure pins*.
///
/// Refusals are the model's, said in the model's words: a caller who names a
/// pin this card does not have, or a kind whose ports are the node, is told
/// which — and told it as a sentence a person can act on rather than as a
/// silent no-op, which is what both references do here.
fn put_away_pins(state: &Rc<LabState>, node: NodeId, which: &str) -> Result<String, InvokeError> {
    use pinion_node_graph::{PutAway, Side};

    let name = state.name_of(node);
    let pin = |word: &str| -> Result<(Side, u32), InvokeError> {
        match word {
            "dial" => Ok((Side::Output, 0)),
            "accept" => Ok((Side::Input, 0)),
            other => Err(InvokeError::rejected(format!(
                "{other:?} is not a pin of this card; it draws `dial` and `accept`"
            ))),
        }
    };
    let said = {
        let mut doc = state.doc.borrow_mut();
        if which == "restore" {
            let back = doc
                .restore_ports(ROOT, node)
                .ok_or_else(|| InvokeError::rejected("no such card"))?;
            format!("{name}: {back} pin(s) back")
        } else if which == "unwired" && pins_are_away_in(&doc, node) {
            // ★ The reference's toggle, and this is the half a scope word
            // cannot carry: what "hide unused" means the SECOND time is
            // "show them again".
            let back = doc
                .restore_ports(ROOT, node)
                .ok_or_else(|| InvokeError::rejected("no such card"))?;
            format!("{name}: {back} pin(s) back")
        } else {
            let scope = match which {
                "unwired" => PutAway::Unwired,
                rest => {
                    if let Some(word) = rest.strip_prefix("others:") {
                        let (side, index) = pin(word)?;
                        PutAway::AllOthers(side, index)
                    } else {
                        let (side, index) = pin(rest)?;
                        PutAway::Port(side, index)
                    }
                }
            };
            let done = doc
                .put_away_ports(ROOT, node, scope)
                .map_err(|why| InvokeError::rejected(why.to_string()))?;
            format!("{name}: {} pin(s) away", done.len())
        }
    };
    state.say(Utterance::done(said.clone()));
    Ok(said)
}

/// ★★★★★ R1914 — the pin address a client wrote, as the model's own
/// [`PortPath`].
///
/// `dial` / `accept` name the two pins this card draws; a dotted suffix names a
/// member of one. The refusal says which half was wrong and lists what it will
/// take, because a caller told only "no" cannot tell a pin it does not have
/// from a member word it spelled differently.
/// ★★★★★ R1915 — `pin_address`'s inverse: the word an address is written
/// under.
///
/// One pair and not two spellings. The paint tags, the accessibility names, the
/// `split_pin` argument, the hit's own word and `PIN_ADDRESSES` are all this
/// function's output — so a client that reads a tag off the frame can hand it
/// straight back to the verb, and nothing has to know that a dot means
/// something.
fn pin_word(side: Side, at: &PortPath) -> String {
    let pin = if side == Side::Output {
        "dial"
    } else {
        "accept"
    };
    let mut word = pin.to_owned();
    for member in &at.members {
        word.push('.');
        word.push_str(PIN_PARTS.get(*member as usize).copied().unwrap_or("?"));
    }
    word
}

fn pin_address(word: &str) -> Result<(Side, PortPath), InvokeError> {
    let (pin, member) = word
        .split_once('.')
        .map_or((word, None), |(p, m)| (p, Some(m)));
    let side = match pin {
        "dial" => Side::Output,
        "accept" => Side::Input,
        other => {
            return Err(InvokeError::rejected(format!(
                "{other:?} is not a pin of this card; it draws `dial` and `accept`"
            )));
        }
    };
    let mut path = PortPath::root(0);
    let Some(member) = member else {
        return Ok((side, path));
    };
    // ★★★★★ R1915 — EVERY level, not the first one. This taxonomy's locator is
    // one level deep (a host and a service are atoms), so a parser that took
    // only one level would be indistinguishable from a correct one here and
    // would be wrong the day a member gained members. That is R1891's rule —
    // "code that walks one step and code that walks a chain cannot be told
    // apart before depth 2 exists" — applied before the depth rather than
    // after, which is the only time it is cheap.
    for level in member.split('.') {
        let at = PIN_PARTS
            .iter()
            .position(|part| *part == level)
            .ok_or_else(|| {
                InvokeError::rejected(format!(
                    "{level:?} is not a member of a locator; it is made of {}",
                    PIN_PARTS.join(" and ")
                ))
            })?;
        path = path.then(u32::try_from(at).unwrap_or(0));
    }
    Ok((side, path))
}

/// ★★★★★ R1914 — **take a pin apart into its members, or put it back.**
///
/// The engine's four commands (`SplitPin` / `RecombinePin` on its schema,
/// `SplitStructPin` / `RecombineStructPin` on its editor) reached as one verb
/// over one address, because that is what they are: two directions of one
/// question about one pin.
///
/// Three things this says that the reference's own commands cannot:
///
/// * **why not**, in the model's words — a wired pin, a pin already split, a
///   member word this taxonomy does not have. The reference greys a menu entry
///   out and the reason is nowhere.
/// * **what moved** — the ports that changed index, which is what an undo and
///   an editor's own bookkeeping both need. Its command answers `void`.
/// * **what the value became** — a recombine composes the members back with
///   [`NodeKind::implode`](pinion_node_graph::NodeKind::implode), and says so.
fn split_pin(state: &Rc<LabState>, node: NodeId, address: &str) -> Result<String, InvokeError> {
    let name = state.name_of(node);
    let (word, folding) = address
        .strip_prefix('-')
        .map_or((address, false), |rest| (rest, true));
    let (side, path) = pin_address(word.trim())?;

    let said = {
        let mut doc = state.doc.borrow_mut();
        if folding {
            let back = doc
                .recombine_port(ROOT, node, side, &path)
                .map_err(|why| InvokeError::rejected(why.to_string()))?;
            let became = back
                .composed
                .map_or_else(|| "nothing".to_owned(), |value| format!("{value:?}"));
            format!(
                "{name}: {word} back together from {} split(s), now {became}",
                back.folded
            )
        } else {
            let apart = doc
                .split_port(ROOT, node, side, &path)
                .map_err(|why| InvokeError::rejected(why.to_string()))?;
            format!(
                "{name}: {word} apart into {} pin(s), {} pin(s) moved",
                apart.members.len(),
                apart.moved.len()
            )
        }
    };
    state.say(Utterance::done(said.clone()));
    Ok(said)
}

/// Whether any of `node`'s pins are away, read from a document already borrowed.
fn pins_are_away_in(doc: &pinion_node_graph::Document<graph::LabNode>, node: NodeId) -> bool {
    doc.visible_ports(ROOT, node)
        .is_some_and(|v| !v.put_away_inputs.is_empty() || !v.put_away_outputs.is_empty())
}

/// ★★★★★ R1912 — each of a card's pins, and for a hidden one the REASON.
///
/// `drawn`, or the reason word the model publishes
/// ([`pinion_node_graph::Hidden::wire_word`]) — so the vocabulary a client
/// reads is the model's and not a second one spelled here.
///
/// The two pins are the two this lab draws, named the way the canvas tags name
/// them: the dial is output 0 and the accept is the first of the variadic run.
fn pins_json(doc: &pinion_node_graph::Document<graph::LabNode>, node: NodeId) -> serde_json::Value {
    use pinion_node_graph::Side;

    let Some(seen) = doc.visible_ports(ROOT, node) else {
        return serde_json::Value::Null;
    };
    let word = |side: Side, index: u32| -> &'static str {
        seen.why_hidden(side, index)
            .map_or("drawn", pinion_node_graph::Hidden::wire_word)
    };
    // ★★★★★ Which pins are WIRED, because that is what the bulk scope selects
    // by and a host has to know whether pressing the seat would do anything.
    // The engine greys its own restore command on exactly this kind of fact
    // (*not all pins are shown*); neither reference publishes the other half,
    // so a client cannot tell a seat that will do nothing from one that will.
    let wired_out = doc
        .tree(ROOT)
        .is_some_and(|t| t.links().iter().any(|l| l.from == Socket::new(node, 0)));
    let wired_in = doc
        .tree(ROOT)
        .and_then(|t| t.link_into(Socket::new(node, 0)))
        .is_some();
    let mut wired: Vec<&str> = Vec::new();
    if wired_out {
        wired.push("dial");
    }
    if wired_in {
        wired.push("accept");
    }
    // ★★★★★ R1913 — and whether each pin SPLITS, with the reason when it does
    // not. The reference answers this as one boolean over five conditions, so
    // its own editor can only grey a menu entry out; here each condition is a
    // word, and two of them are reachable on THIS screen — a wired pin and an
    // unwired one give different answers, which is the whole difference.
    //
    // The vocabulary is the model's (`NotSplittable::wire_word`), not spelled
    // here: a second list drifts the first time an arm is added.
    let splits = |side: Side, index: u32| -> &'static str {
        doc.splittable(ROOT, node, side, index)
            .map_or_else(|why| why.wire_word(), |_| "yes")
    };
    // ★★★★★ R1914 — and what each pin has COME APART INTO, if anything: one
    // entry per resolved port that is a member, giving its address, the name
    // it draws under, and the value it carries.
    //
    // Published as a list of ADDRESSES rather than as indices, because an index
    // here moves whenever a pin before it splits and an address does not — the
    // whole reason the model carries both. A client that stored an index would
    // be re-pointed by the next split it did not perform.
    let members = |side: Side| -> Vec<serde_json::Value> {
        doc.resolved_ports(ROOT, node, side)
            .into_iter()
            .filter(|(path, _)| path.depth() > 0)
            .map(|(path, port)| {
                let at = doc.index_of(ROOT, node, side, &path);
                serde_json::json!({
                    "address": format!(
                        "{}.{}",
                        if side == Side::Output { "dial" } else { "accept" },
                        path.members
                            .iter()
                            .map(|m| PIN_PARTS.get(*m as usize).copied().unwrap_or("?"))
                            .collect::<Vec<_>>()
                            .join("."),
                    ),
                    "name": port.name,
                    "at": at,
                    "carries": at
                        .and_then(|index| doc.port_value(ROOT, node, PortRef { side, index }).cloned())
                        .or_else(|| port.flow.default_value().cloned()),
                })
            })
            .collect()
    };
    // ★★★★★ R1914 — the address each pin CARRIES, published beside the members
    // it comes apart into.
    //
    // Without it a reader cannot check the split against anything: the member
    // values and the taxonomy's declared defaults are both plausible strings,
    // and on the opening graph they happened to be the SAME two — so a walk
    // asserting "the members carry something" passed while the parent's value
    // was not being shared out at all. Measured at R1914, and the repair is to
    // publish the fact the comparison needs rather than to assert harder.
    let carries = |side: Side| -> Option<String> {
        doc.port_value(ROOT, node, PortRef { side, index: 0 })
            .cloned()
            .or_else(|| {
                doc.resolved_ports(ROOT, node, side)
                    .first()
                    .and_then(|(_, port)| port.flow.default_value().cloned())
            })
    };
    serde_json::json!({
        "dial": word(Side::Output, 0),
        "accept": word(Side::Input, 0),
        "wired": wired,
        "splits": {
            "dial": splits(Side::Output, 0),
            "accept": splits(Side::Input, 0),
        },
        "carries": {
            "dial": carries(Side::Output),
            "accept": carries(Side::Input),
        },
        "members": {
            "dial": members(Side::Output),
            "accept": members(Side::Input),
        },
        // ★ The fact neither reference can be asked for: this card has nothing
        // on the frame to wire to or from. Published rather than refused —
        // the DCC's own bulk operator reaches this state.
        "nothing_drawn": seen.nothing_drawn(),
    })
}

/// Switch a card off, or back on (R1682).
///
/// ★★ The model's [`Document::set_disabled`], not `set_bypassed`: this screen's
/// nodes are processes, and switching one off means it does not run and nothing
/// downstream hears from it. Bypassing would mean the opposite — traffic routed
/// straight through — which is a request this tool never makes.
/// R1789 — a wire number as the seconds a scenario keeps.
///
/// One place rather than three `as f32` casts with three `allow`s: the wire
/// carries `f64` and this screen's clock is `f32`, and where that narrowing
/// happens is a decision worth having exactly one of.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a scenario time is seconds at f32"
)]
fn seconds(wire: f64) -> f32 {
    wire as f32
}

fn disable_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let was = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .map(|slot| slot.disabled)
        .ok_or_else(|| InvokeError::rejected("no such card"))?;
    state
        .doc
        .borrow_mut()
        .set_disabled(ROOT, node, !was)
        .map_err(|why| InvokeError::rejected(why.to_string()))?;
    let name = state.name_of(node);
    state.say(Utterance::done(format!(
        "{name} {}",
        if was { "switched on" } else { "switched off" }
    )));
    Ok((!was).to_string())
}

/// Move a drawn link's consuming end onto `to`, dialling its first free
/// endpoint (R1681).
///
/// The link keeps its identity throughout — see `Document::relink`. What that
/// buys here is visible: the selection does not have to be repaired afterwards,
/// because the thing that was selected is the thing that moved.
fn relink_to(state: &Rc<LabState>, link: LinkId, to: NodeId) -> Result<String, InvokeError> {
    let name = state.name_of(to);
    // ★★★★★ R1924 — the act BEGINS by asking the question, which is the same
    // shape `Document::relink` takes inside the crate. Before this round the
    // two gates below lived here and the question knew only one of them, so a
    // drag could be told *it will take it* by a screen that then refused the
    // drop. One decision now, asked at two moments.
    let endpoint = match would_take(state, link, to) {
        Ok(endpoint) => endpoint,
        Err(why) => {
            let said = Utterance::refused(&why);
            state.say(said.clone());
            return Err(InvokeError::rejected(said.into_clause()));
        }
    };
    let source = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).map(|l| l.from.node));
    move_end(state, link, to, endpoint.as_deref()).map(|_| {
        let word = match source {
            Some(from) => format!("{} -> {name}", state.name_of(from)),
            None => name.clone(),
        };
        state.say(Utterance::done(format!("moved {word}")));
        word
    })
}

/// Take a reported link into the drawing (R1681).
///
/// `Document::adopt` runs the **authoring** rules on it, so a link the world
/// has and this model cannot express is *named* rather than quietly dropped.
/// That refusal is the finding the whole two-layer idea exists to produce, and
/// it is why this is not "copy the observation into the links list".
fn adopt_link(state: &Rc<LabState>, from: Socket, to: Socket) -> Result<String, InvokeError> {
    let taken = state.doc.borrow_mut().adopt(ROOT, from, to);
    match taken {
        Ok(made) => {
            state.selected_link.set(Some(LinkPick::Authored(made.link)));
            let word = format!("{} -> {}", state.name_of(from.node), state.name_of(to.node));
            state.say(Utterance::done(format!("adopted {word}")));
            Ok(word)
        }
        Err(why) => {
            let said = Utterance::refused(&why);
            state.say(said.clone());
            Err(InvokeError::rejected(said.into_clause()))
        }
    }
}

/// Put the picked link on the target's `n`th listening endpoint (R1681).
///
/// The link decides which endpoint it dials — not the node — which is why this
/// moves the link's end rather than editing anything about the target. The
/// reference sets an index on the wire and checks nothing; here the endpoint's
/// own transport is the accept slot's type, so dialling one this link cannot
/// speak is refused by the model, with both transports named.
fn choose_endpoint(state: &Rc<LabState>, n: usize) -> Result<String, InvokeError> {
    let picked = state
        .selected_link
        .get()
        .and_then(LinkPick::authored)
        .ok_or_else(|| InvokeError::rejected("no drawn link is picked"))?;
    let to = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(picked).map(|l| l.to.node))
        .ok_or_else(|| InvokeError::rejected("the picked link is not drawn"))?;
    let endpoints = endpoints_of(state, to);
    let endpoint = endpoints.get(n).cloned().ok_or_else(|| {
        InvokeError::rejected(format!(
            "{} listens on {} endpoint(s), so there is no {n}",
            state.name_of(to),
            endpoints.len()
        ))
    })?;
    move_end(state, picked, to, Some(&endpoint)).map(|_| {
        state.say(Utterance::done(format!("on {endpoint}")));
        endpoint
    })
}

/// ★★★★★ R1924 — **would this card take the wire being re-aimed, and if not,
/// why?** — asked before the hand lets go, and answering with the address the
/// drop would dial.
///
/// # ★★★★★ This IS the first half of [`relink_to`], not a prediction of it
///
/// Which is the whole design, and it was learned the expensive way inside this
/// round: the first draft asked the crate alone, so the canvas said *P-02 will
/// take it* and the drop then refused with *P-01 already dials every endpoint
/// of P-02* — a **screen-level** rule the question had never heard of. Two
/// oracles, one gesture, and the hand was told the wrong one. So the question
/// runs every gate the act runs, in the act's order, and the act begins by
/// asking it.
///
/// # ★★★★★ R1930 — the crate half is `Document::may_land` now, and the COPY
/// moved into it
///
/// R1924 wrote this function's crate half here: clone the document, open a slot
/// in the clone, ask `may_relink` about the port that appeared. That is exactly
/// [`Document::may_land`], and this screen having its own copy of it was one
/// derivation living in two places — so the clone, the slot and the dry run are
/// the crate's now, and this function keeps only what is genuinely the screen's:
/// the domain rule that a dialler may not take every endpoint of one peer.
///
/// The answer it returns is richer for the move: [`Landfall`] says whether an
/// existing pin takes the wire or a NEW one appears for it, which this canvas
/// could not say before because opening a slot was unconditional.
///
/// The refusal sentence is the crate's own, so a hand over a card that would
/// close a cycle is told **which path** closes it rather than that it did not
/// work.
fn would_take(state: &LabState, link: LinkId, to: NodeId) -> Result<Option<String>, String> {
    let held = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).copied())
        .ok_or_else(|| format!("no link {} is drawn", link.0))?;
    let name = state.name_of(to);
    let endpoint = landing_endpoint(
        &state.doc.borrow(),
        &state.forms.borrow(),
        held.from.node,
        to,
    )
    .map_err(|()| {
        format!(
            "{} already dials every endpoint of {name}",
            state.name_of(held.from.node)
        )
    })?;
    would_land(state, link, to).map(|_| endpoint)
}

/// ★★★★★ R1930 — **what the crate says releasing this wire on that card would
/// do**: an existing pin takes it, or one appears for it.
///
/// The screen's own domain gate is [`would_take`]'s and runs first; this is the
/// model's half, and it is one call because [`Document::may_land`] absorbed the
/// clone-and-open-a-slot dance this file used to do itself.
fn would_land(state: &LabState, link: LinkId, to: NodeId) -> Result<Landfall, String> {
    state
        .doc
        .borrow()
        .may_land(ROOT, link, Side::Input, to)
        .map_err(|why| match why {
            // ⚠ R1930 — the ONE arm this screen re-words, and the crate's own
            // header says so. Every other refusal names something the model
            // knows better than this file — a type, an arity, the path a wire
            // would close — and is carried whole for R1924's reason. This one
            // names a node id and a side; the card has a NAME here, and the
            // first walk to read the crate's version of this sentence reported
            // `node 3` where `Q-01` was available.
            LandError::NoRoom { .. } => {
                format!("{} has no accept pin", state.name_of(to))
            }
            other => Utterance::refused(&other).into_clause(),
        })
}

/// ★★★★★ R1924 — what one card would do with the wire being re-aimed.
///
/// **Three** answers and not two, and the third is why this is a type. The card
/// the wire is already on is neither a destination nor a refusal: dropping it
/// back there is a success that moves nothing, which is what the crate answers
/// and what this canvas has said since R1681.
///
/// ⚠ Folding that third state into *takes* is the defect this round's own
/// first draft shipped, and the walk caught it: the published verdict said
/// `takes` for the standing card while the canvas did not light it, because the
/// lit set and the published row were computed from two different populations
/// and nothing could notice they disagreed. There is one derivation now and
/// every reader takes it — the canvas lights [`Landing::Takes`], the wire
/// publishes the word, and the drag says the sentence.
enum Landing {
    /// Where the wire already is. A drop here moves nothing.
    Standing,
    /// A pin that is already there would take it.
    Takes,
    /// ★★★★★ R1930 — a pin would APPEAR for it, and the wire would land on that.
    ///
    /// The fourth answer, and it is a different thing to tell a hand: *this pin
    /// takes it* and *this card will grow one* are the two things the reference
    /// spells with three hard-coded strings in an out-parameter its own header
    /// calls an error channel. Here the crate answers a [`Landfall`] and this is
    /// the word for its second arm.
    Grows,
    /// It would not, and this is the crate's own reason.
    Refuses(String),
}

impl Landing {
    /// The word this goes onto the wire as.
    const fn word(&self) -> &'static str {
        match self {
            Self::Standing => "standing",
            Self::Takes => "takes",
            Self::Grows => "grows",
            Self::Refuses(_) => "refuses",
        }
    }

    /// Whether a drop here would land the wire — either onto a pin that is
    /// there or onto one that appears.
    const fn lands(&self) -> bool {
        matches!(self, Self::Takes | Self::Grows)
    }

    /// The sentence, for the answers that are not a refusal — `None`, because a
    /// reason beside a yes is a reason nobody can act on.
    fn because(self) -> Option<String> {
        match self {
            Self::Refuses(why) => Some(why),
            Self::Standing | Self::Takes | Self::Grows => None,
        }
    }
}

fn landing_for(state: &LabState, link: LinkId, card: NodeId) -> Landing {
    let standing = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).map(|l| l.to.node));
    if standing == Some(card) {
        return Landing::Standing;
    }
    // ★★★★★ R1930 — the screen's domain gate first (it is `would_take`'s), then
    // the crate's answer, and the crate's answer is now RICHER than a yes: it
    // says whether a pin is there or one appears.
    match would_take(state, link, card).and_then(|_| would_land(state, link, card)) {
        Ok(fall) if fall.is_new() => Landing::Grows,
        Ok(_) => Landing::Takes,
        Err(why) => Landing::Refuses(why),
    }
}

fn clear_rewire_marks(state: &LabState) {
    state.rewire_targets.borrow_mut().clear();
    state.rewire_over.set(None);
}

/// Every card on this canvas the wire's consuming end could move to (R1924).
///
/// Derived by asking [`landing_for`] of each card rather than by a rule about
/// which cards accept: the reference's *may relinking start here* is one bit
/// per pin, and this is that bit's whole content — the ports that will take the
/// wire, so the canvas can light them instead of leaving the hand to find out
/// by dropping.
fn rewire_targets_of(state: &LabState, link: LinkId) -> BTreeSet<NodeId> {
    let cards: Vec<NodeId> = state
        .doc
        .borrow()
        .tree(ROOT)
        .map(|t| t.nodes().map(|n| n.id).collect())
        .unwrap_or_default();
    cards
        .into_iter()
        // ★ R1930 — a card that would GROW a pin is lit too: what the hand is
        // being told is *release here and the wire lands*, and whether the pin
        // is already there is the sentence beside it, not the lighting.
        .filter(|card| landing_for(state, link, *card).lands())
        .collect()
}

/// Re-aim a link's consuming end at `to`, on the endpoint `endpoint` names.
///
/// ★★★★★ R1930 — **the open / move / close-on-the-way-out dance is gone.**
/// Until this round the screen opened a slot in the real document, asked the
/// crate to move the end onto it, and — when the crate refused — closed the slot
/// again. That undo path is exactly what the reference's own drop does not have
/// and what a person pays for when it is forgotten: a port nobody asked for,
/// left behind by a refusal.
///
/// [`Document::land`] is one act. It decides whether a pin that is there takes
/// the end or one has to appear, does the whole thing to a copy, and takes the
/// copy only on success — so a refusal leaves the document equal to what it was,
/// by construction rather than by this function remembering to tidy up.
///
/// The slot the end LEFT is still closed here, because that is not an undo: the
/// wire really did leave, and an accept run that kept a seat per wire it once
/// had would grow forever.
fn move_end(
    state: &Rc<LabState>,
    link: LinkId,
    to: NodeId,
    endpoint: Option<&str>,
) -> Result<Relinked, InvokeError> {
    let was = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).map(|l| l.to))
        .ok_or_else(|| InvokeError::rejected(format!("no link {} is drawn", link.0)))?;
    // What a grown pin would carry: the address this wire dials, as a label the
    // canvas draws AND an authored value the model can take apart (R1914).
    // Ignored when an existing pin takes the end, which is the crate's answer
    // and not this screen's to make.
    let item = match endpoint {
        Some(one) => Item::plain().named(one).typed(
            0,
            graph::Endpoint::Locator(Transport::of_locator(one).unwrap_or(Transport::Tcp)),
        ),
        None => Item::plain(),
    };
    let done = state
        .doc
        .borrow_mut()
        .land(ROOT, link, Side::Input, to, item);
    match done {
        Ok(landed) => {
            if let Some(one) = endpoint {
                set_port_address(state, landed.relinked.now, one);
            }
            // The old slot last: closing it re-points what is past it, and the
            // link has already left it.
            close_slot(state, was.node, was.port);
            Ok(landed.relinked)
        }
        Err(why) => {
            // ⚠ No undo here, and that is the point: nothing was changed.
            let said = Utterance::refused(&why);
            state.say(said.clone());
            Err(InvokeError::rejected(said.into_clause()))
        }
    }
}

/// Give the pin the end landed on the address it dials, as an authored value.
///
/// ★ R1930 — needed because [`Document::land`] may answer `Takes`, and a pin
/// that was already there does not carry this wire's address yet. The label the
/// item holds is the crate's; the VALUE on the port is what this screen's split
/// and inspector read, and R1914 measured what a locator that lives only in a
/// name costs when the pin comes apart.
fn set_port_address(state: &LabState, socket: Socket, endpoint: &str) {
    let mut doc = state.doc.borrow_mut();
    let _ = doc.set_port_value(
        ROOT,
        socket.node,
        PortRef::input(socket.port),
        endpoint.to_owned(),
    );
}

/// ★★★★★ R1889 — the width a panel would have if its outer boundary were at
/// `px`.
///
/// Derived from the EDGE, not written twice: a left-hand panel grows as the
/// pointer moves right and a right-hand one grows as it moves left, and a panel
/// that flips takes this with it. The inner face it is measured from is
/// whatever the layout already puts before it — the rail for the first
/// left-hand panel, the window's edge for a right-hand one, and the other
/// panel's thickness when both share a side — so this asks the panel's own
/// rectangle rather than re-deriving the stack.
fn panel_width_under(state: &LabState, which: SidePanel, px: u32) -> u32 {
    let rect = which.rect();
    match which.at(state).edge {
        ChromeEdge::Left => px.saturating_sub(rect.x),
        _ => rect.x.saturating_add(rect.w).saturating_sub(px),
    }
}

fn move_cursor(state: &Rc<LabState>, px: u32, py: u32) {
    state.cursor.set((px, py));
    // ★ R1916 — a move is what says the pointer is here again after a leave.
    state.pointer_inside.set(true);
    let Some(drag) = state.drag.get() else {
        return;
    };
    match drag {
        // ★★★★★ R1889 — the width the pointer is asking for, clamped BY THE
        // POLICY and then admitted by it. Two calls into one declaration:
        // `clamp` gives the drag its reading (a hand past the bound means the
        // bound) and `place_panel` applies the same policy's `admit_extent`, so
        // a clamped ask can never be refused and the bounds live in exactly one
        // place. See `Resize::clamp` for why the clamp is not written here.
        Drag::PanelWidth { panel } => {
            let want = panel_width_under(state, panel, px);
            if let Some(extent) = panel.spec().policy.resize.clamp(want) {
                // A refusal here would be a defect in the clamp, not a
                // legitimate answer — so it is reported rather than swallowed.
                if let Err(refused) = place_panel(state, panel, PlaceAsk::Extent(extent)) {
                    state.say(Utterance::refused(&panel_refusal_sentence(panel, &refused)));
                }
            }
        }
        Drag::Pan { from, start } => {
            let dx = i64::from(px) - i64::from(from.0);
            let dy = i64::from(py) - i64::from(from.1);
            state.pan.set((
                start.0 + i32::try_from(dx).unwrap_or(0),
                start.1 + i32::try_from(dy).unwrap_or(0),
            ));
        }
        Drag::Frame { frame, from } => {
            let (ux, uy) = to_canvas(state, px, py);
            let (dx, dy) = (ux - from.0, uy - from.1);
            if dx != 0 || dy != 0 {
                let members = members_of(state, frame);
                shift_cards(state, &members, frame, (dx, dy));
                state.drag.set(Some(Drag::Frame {
                    frame,
                    from: (ux, uy),
                }));
            }
        }
        Drag::Node { node, grab, snap } => {
            let (ux, uy) = to_canvas(state, px, py);
            let mut cx = ux - grab.0;
            let mut cy = uy - grab.1;
            if snap {
                cx = (cx + SNAP / 2) / SNAP * SNAP;
                cy = (cy + SNAP / 2) / SNAP * SNAP;
            }
            // A node stops at the edge of the world rather than acquiring a
            // position the surface cannot hold.
            cx = clamp_to_world(cx);
            cy = clamp_to_world(cy);
            if let Some(slot) = state
                .doc
                .borrow_mut()
                .tree_mut(ROOT)
                .and_then(|t| t.node_mut(node))
            {
                // ★ R1654 — no second clamp here. `clamp_to_world` above is the
                // bound, and a `.max(0)` beside it silently won: the world's
                // negative half exists so a node can be dragged UP and LEFT of
                // where the graph opened, and pinning the position at zero made
                // the card stop dead partway up the canvas. Two clamps for one
                // fact, and the tighter one decided.
                slot.x = cx;
                slot.y = cy;
            }
        }
        // ★★★★★ R1924 — a wire being re-aimed says, as it passes over a card,
        // whether that card will take it and why not.
        //
        // Before the hand lets go: that is the point, and it is what the
        // reference's hover response is for. The document is untouched — the
        // verdict is `may_relink` asked on a copy — so this is a reading, not a
        // rehearsal that has to be undone.
        Drag::Rewire { link, .. } => {
            let over = match Hit::at(state, px, py) {
                Hit::Pin {
                    node,
                    side: Side::Input,
                    ..
                }
                | Hit::Node(node) => Some(node),
                _ => None,
            };
            // Said once on arrival. Without this the same sentence would be
            // pushed onto the toast on every pixel of travel across one card,
            // which is a screen shouting rather than a screen answering.
            if state.rewire_over.get() == over {
                return;
            }
            state.rewire_over.set(over);
            if let Some(card) = over {
                let name = state.name_of(card);
                // The same derivation the canvas lights from and the wire
                // publishes, so the picture, the sentence and the answer an
                // agent reads cannot be three opinions.
                match landing_for(state, link, card) {
                    Landing::Standing => {
                        state.say(Utterance::unchanged("the link is already there"));
                    }
                    Landing::Takes => {
                        state.say(Utterance::unchanged(format!("{name} will take it")));
                    }
                    // ★★★★★ R1930 — a different sentence, because it is a
                    // different thing to be told: nothing on that card is
                    // waiting for this wire, and releasing it makes a pin.
                    Landing::Grows => {
                        state.say(Utterance::unchanged(format!(
                            "{name} will grow a pin for it"
                        )));
                    }
                    Landing::Refuses(why) => state.say(Utterance::new(Tone::Refused, why)),
                }
            }
        }
        // Follows the cursor and changes nothing until release: what the canvas
        // draws mid-drag comes from `cursor`.
        Drag::Wire { .. } => {}
    }
}

fn press(state: &Rc<LabState>) {
    let (px, py) = state.cursor.get();
    let hit = Hit::at(state, px, py);
    match &hit {
        Hit::Node(node) => {
            select_card(state, Some(*node));
            // ★★★★★ R1726 — picking a card up puts it in front, and it STAYS
            // there when it is put down. Position is untouched: on a free
            // canvas a drop displaces nothing, because where a node sits is
            // what the person meant by putting it there.
            state.raise(*node);
            let (cx, cy) = state
                .doc
                .borrow()
                .tree(ROOT)
                .and_then(|t| t.node(*node))
                .map_or((0, 0), |n| (n.x, n.y));
            let (ux, uy) = to_canvas(state, px, py);
            state.drag.set(Some(Drag::Node {
                node: *node,
                grab: (ux - cx, uy - cy),
                snap: false,
            }));
        }
        Hit::Pin {
            node,
            side: Side::Output,
            at,
        } => {
            // ★ The address the press carried, resolved to the port it names
            // right now. A pin the model cannot locate starts no drag, which is
            // a refusal rather than a wire from port 0.
            if let Some(port) = state.doc.borrow().index_of(ROOT, *node, Side::Output, at) {
                state.drag.set(Some(Drag::Wire { from: *node, port }));
            }
        }
        // ★★★★★ R1889 — the grip starts a drag rather than deciding anything.
        // The value this gesture carries is *where the pointer ends up*, so
        // there is nothing to decide on the way down; `move_cursor` turns each
        // position into an ask and `place_panel` answers it, which is how the
        // drag and the wire verb stay one rule.
        Hit::PanelGrip(which) => {
            state.drag.set(Some(Drag::PanelWidth { panel: *which }));
        }
        // ★★ R1681 — pressing an accept pin that already holds a wire PICKS IT
        // UP, which is the reference's rule and every node editor's. A dial pin
        // is fan-out and always starts a new wire; an accept pin holds what
        // arrived, so grabbing it means "move this one".
        Hit::Pin {
            node,
            side: Side::Input,
            ..
        } => {
            let at = window_to_content(state, px, py);
            if let Some(link) = link_into_pin(state, *node, at) {
                let source = state
                    .doc
                    .borrow()
                    .tree(ROOT)
                    .and_then(|t| t.link(link).map(|l| l.from.node));
                if let Some(from) = source {
                    state.selected_link.set(Some(LinkPick::Authored(link)));
                    // ★★★★★ R1924 — what would take it, worked out the moment
                    // it leaves the pin. The canvas lights those, and the
                    // reference's per-pin `may relinking start here` boolean is
                    // this set being non-empty.
                    let lit = rewire_targets_of(state, link);
                    *state.rewire_targets.borrow_mut() = lit;
                    state.rewire_over.set(None);
                    state.drag.set(Some(Drag::Rewire { link, from }));
                }
            }
        }
        // ★★★ R1706 — SELECT the host's cards, then start carrying them. One
        // gesture with two halves, which is what the reference does and what
        // this arm was missing: the drag moved six cards and the inspector went
        // on showing whichever card had been selected before.
        Hit::Frame(frame) => {
            select_frame(state, *frame);
            state.drag.set(Some(Drag::Frame {
                frame: *frame,
                from: to_canvas(state, px, py),
            }));
        }
        Hit::Canvas => {
            state.drag.set(Some(Drag::Pan {
                from: (px, py),
                start: state.pan.get(),
            }));
        }
        _ => {}
    }
    *state.pressed.borrow_mut() = Some(hit);
}

/// ★ R1654 — a card dropped inside a frame JOINS it, and one dropped outside
/// every frame leaves the one it was in.
///
/// Membership is what a frame's rectangle is derived from, so this is the whole
/// group gesture: the box follows the drop, rather than the drop being checked
/// against a box somebody typed into a table.
fn apply_frame(state: &Rc<LabState>, node: NodeId) {
    let landed = card_rect(state, node)
        .and_then(|r| frame_at(state, i64::from(r.x + r.w / 2), i64::from(r.y + r.h / 2)));
    let held = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.node(node).and_then(|n| n.parent));
    if landed == held {
        return;
    }
    if state
        .doc
        .borrow_mut()
        .set_parent(ROOT, node, landed)
        .is_ok()
    {
        let name = state.name_of(node);
        match landed.and_then(|f| state.frames.borrow().get(&f).cloned()) {
            Some(frame) => state.say(Utterance::done(format!("{name} now starts on {frame}"))),
            None => state.say(Utterance::done(format!("{name} is not on any host"))),
        }
    }
}

/// What a drag does when it is let go, over whatever it was let go on.
///
/// Its own function because there are now four kinds and each commits
/// differently — and because the two that involve a wire are the ones a reader
/// has to be able to compare side by side.
fn finish_drag(state: &Rc<LabState>, drag: Drag, now: &Hit) {
    match drag {
        // A wire commits onto whatever accept pin it was let go over.
        //
        // ★★★★★ R1915 — and onto WHICH one. Both ends carry an address now, so
        // a wire dragged from a split pin's host half onto another card's host
        // half lands there rather than on the whole pin. Dropped on a card's
        // body the address is the root, which is what it always was.
        Drag::Wire { from, port } => {
            let landing = match now {
                Hit::Pin {
                    node,
                    side: Side::Input,
                    at,
                } => Some((*node, at.clone())),
                Hit::Node(node) => Some((*node, PortPath::root(0))),
                _ => None,
            };
            // ★ The index the drag started from, read back as the address the
            // verb takes. A port that stopped existing between press and
            // release answers nothing, and a wire from nowhere is refused
            // rather than silently made from port 0.
            let leaving = state.doc.borrow().path_of(ROOT, from, Side::Output, port);
            match (landing, leaving) {
                (Some((node, onto)), Some(out)) if node != from => {
                    connect_at(state, from, &out, node, &onto).ok();
                }
                (Some(_), _) => {}
                (None, _) => {
                    state.say(Utterance::new(Tone::Refused, "a link needs an accept pin"));
                }
            }
        }
        // ★★ R1681 — a picked-up link commits the same way, except that it
        // MOVES rather than being made. Released over nothing it is let go,
        // which is the rule every node editor has and the reference states in
        // as many words: dropping a wire on empty canvas disconnects it.
        Drag::Rewire { link, .. } => match *now {
            Hit::Pin {
                node,
                side: Side::Input,
                ..
            }
            | Hit::Node(node) => {
                let landed = state
                    .doc
                    .borrow()
                    .tree(ROOT)
                    .and_then(|t| t.link(link).map(|l| l.to.node));
                if landed == Some(node) {
                    // Picked up and put back down where it was. The reference
                    // has to restore it; here there is nothing to restore,
                    // because a move that has not happened has taken nothing
                    // out.
                    state.say(Utterance::unchanged("the link is already there"));
                } else {
                    relink_to(state, link, node).ok();
                }
            }
            _ => {
                delete_link(state, link).ok();
            }
        },
        Drag::Node { node, .. } => apply_frame(state, node),
        // ★ R1889 — a width drag commits on every move, so letting go commits
        // nothing extra. Named beside the other two that do the same rather
        // than added to their arm, because *this one has already committed* and
        // *this one has nothing to commit* are different facts and the next
        // reader of this match should not have to work out which it is.
        Drag::Pan { .. } | Drag::Frame { .. } | Drag::PanelWidth { .. } => {}
    }
}

/// Start the graph, or stop it — and settle every form on the way in.
///
/// ★ R1716 — lifted out of [`release`] when that arm's function passed the
/// hundred-line bound. It is a whole act rather than a fragment: a launch
/// ACCEPTS the values, which is why the settle is here and not beside the
/// button, and the refusal it prints is the gate's own sentence.
fn toggle_running(state: &Rc<LabState>) {
    let verdict = state.verdict();
    if state.running.get() {
        state.running.set(false);
        state.say(Utterance::done("stopped"));
    } else if verdict.may_launch() {
        state.running.set(true);
        for form in state.forms.borrow_mut().values_mut() {
            form.settle();
        }
        state.say(Utterance::done("running"));
    } else {
        // ★★★ R1719 — the launch gate's verdict is the reason the graph did
        // not start, so it is a REFUSAL, and it was reaching the person in the
        // same voice as `running`.
        state.say(Utterance::refused(&verdict.sentence()));
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per hit this screen resolves — the peer of `Hit::at`, and \
              splitting it would separate a press from what the press does"
)]
fn release(state: &Rc<LabState>) {
    let (px, py) = state.cursor.get();
    let now = Hit::at(state, px, py);
    let was = state.pressed.borrow_mut().take();
    let drag = state.drag.get();
    state.drag.set(None);
    // ★ R1924 — the lit ports go out with the gesture that lit them. Cleared
    // here rather than in `finish_drag` so that every way a drag ends clears
    // them, which is the same reason `drag` itself is cleared here.
    clear_rewire_marks(state);

    if let Some(drag) = drag {
        finish_drag(state, drag, &now);
        return;
    }

    let Some(was) = was else { return };
    if was != now {
        return;
    }
    match now {
        Hit::Role(role) => add_node(state, role),
        Hit::Build(stack) => set_build(state, stack),
        Hit::DiscoveryToggle => {
            let next = !state.discovery.get();
            state.discovery.set(next);
            state.say(Utterance::done(if next {
                "discovery on"
            } else {
                "discovery off"
            }));
        }
        Hit::Reset(scope) => {
            scope.apply(state);
            state.say(Utterance::done(format!(
                "{} back to how it opened",
                scope.wire()
            )));
        }
        // ★★ R1682 — the node's-life seats, through the same three functions
        // the wire calls. A refusal (the last card) has already said so on the
        // toast, which is where a person reads it.
        Hit::NodeAct(act) => {
            if let Some(node) = state.active_card() {
                let _ = match act {
                    NodeAct::Collapse => collapse_card(state, node),
                    NodeAct::Disable => disable_card(state, node),
                    NodeAct::Delete => delete_card(state, node),
                    // ★ R1912 — through the same function the wire calls, with
                    // the reference's own bulk scope. Which direction the press
                    // means is the verb's business, not this branch's.
                    NodeAct::Pins => put_away_pins(state, node, "unwired"),
                };
            }
        }
        // ★★ R1683 — one seat, two jobs, which is what makes the field's state
        // readable from the button: shut, it opens on the name; open, it
        // applies what was typed. The reference's box works the same way.
        Hit::Rename | Hit::AddKey => edit_seat(state, &now),
        // ★ R1688 — through the same function the wire's `zoom_by` calls, so
        // the stepper and the verb anchor the same way.
        // ★★★★★ R1887 — the panel's own chrome, through `place_panel`, which is
        // the same function the wire verb calls. A refusal is SAID rather than
        // dropped: measured at R1801, the floor accepts a move its own
        // declaration forbids and reports nothing at all.
        Hit::Panel(which, act) => match act.ask(state, which) {
            None => state.say(Utterance::refused(&format!(
                "the {} sits on an edge its own declaration no longer admits",
                which.word()
            ))),
            Some(ask) => {
                if let Err(refused) = place_panel(state, which, ask) {
                    state.say(Utterance::refused(&panel_refusal_sentence(which, &refused)));
                }
            }
        },
        Hit::Zoom(up) => {
            zoom_to(state, zoom_stepped(state, up));
        }
        Hit::Fit => {
            fit_view(state);
        }
        Hit::Problem => {
            go_to_problem(state);
        }
        Hit::Run => toggle_running(state),
        // ★★ R1687 — the two artifacts, through the same two functions the wire
        // calls. Before this round the `config` seat reported the SELECTED
        // card's key count: a different question, at a different scope, that
        // nothing on the reference screen asks.
        Hit::Config => {
            export_configuration(state);
        }
        Hit::Script => {
            produce_script(state);
        }
        // ★★ R1689 — the file pill, through the same three functions the wire
        // calls. `open` with no argument is "the saved one", which is what the
        // button means and what the reference's own box does when it is left
        // empty.
        Hit::SaveGraph => {
            persist::save(state);
        }
        Hit::OpenGraph => {
            persist::open(state, "").ok();
        }
        Hit::ClearGraph => {
            persist::clear(state);
        }
        // ★★★★★ R1791 — open or close the overflow. A toggle and not a
        // one-way open, because the control is the only way back: the seats it
        // holds are painted over the canvas, and a person who opened it to look
        // has to be able to stop looking.
        Hit::More => toggle_overflow(state),
        Hit::Link(id) => state.selected_link.set(Some(LinkPick::Authored(id))),
        Hit::Observed(from, to) => state.selected_link.set(Some(LinkPick::Observed(from, to))),
        // ★★ R1681 — one seat, two meanings, chosen by which layer the picked
        // link is in. A drawn one can be removed; a reported one is a fact
        // about the world and the only thing to do with it is put it in the
        // drawing.
        Hit::LinkAct => match state.selected_link.get() {
            Some(LinkPick::Authored(id)) => {
                delete_link(state, id).ok();
            }
            Some(LinkPick::Observed(from, to)) => {
                adopt_link(state, from, to).ok();
            }
            None => {}
        },
        Hit::Endpoint(n) => {
            choose_endpoint(state, n).ok();
        }
        Hit::AddField(_)
        | Hit::Field(_)
        | Hit::RemoveField(_)
        | Hit::AuthorField(_)
        | Hit::DisownField(_)
        | Hit::Part { .. } => {
            act_on_form(state, now);
        }
        Hit::Rail(name) => state.say(Utterance::new(
            Tone::Refused,
            format!("{name} is not this screen"),
        )),
        // ★ R1889 — `Hit::PanelGrip` belongs here, and the compiler is what
        // decided that rather than a preference. It was written as its own
        // named arm with an empty body — a grip press starts a DRAG, so it is
        // handled in `press`, and a click that begins and ends on the grip
        // without moving is a resize to the width it already had — and
        // `clippy::match_same_arms` refused the arm as identical to this one.
        // ⇒ ★ a lint that forbids a named no-op is a lint that decides where
        // the reasoning lives, so the reasoning moved here rather than an
        // `allow` being added to keep it where it looked better.
        _ => {}
    }
}

/// What a press inside the settings form does — the four ways in, together.
///
/// ★ R1686 grouped them, and the grouping is the shape rather than a way to
/// keep a function short: each of these four is a press on the inspector's
/// FORM, they are the arms that need the selected card, and the two that were
/// added since R1684 are both here. `release` reads as the screen's regions
/// again with them folded away.
fn act_on_form(state: &Rc<LabState>, hit: Hit) {
    match hit {
        Hit::AddField(key) => {
            if let Some(node) = state.active_card() {
                amend(state, node, |form| form.add(&key)).ok();
            }
        }
        // ★★ R1716 — the seat on a row nobody wrote, through the one function
        // the wire also calls. A refusal has already reached the toast.
        Hit::AuthorField(key) => {
            if let Some(node) = state.active_card() {
                author_row(state, node, &key).ok();
            }
        }
        // ★★★ R1684 — the arm this screen NAMED and then dropped.
        //
        // The hit test has resolved a press to `field:<key>` since R1651 and
        // the wire answered that word, so an agent and a person both read the
        // press as handled — and the match ended in `_ => {}`. A person
        // reported it as "the text box does nothing", which is exactly what it
        // was: a declared arm with no implementation is worse than no arm,
        // because the declaration stops anyone looking.
        Hit::Field(key) => press_row(state, &key),
        // ★★ R1686 — the seat, through the one function the wire also calls.
        // A refusal has already reached the toast through `say`.
        // ★★ R1717 — and the seat on a row with two contributors, through the
        // SAME function: the form reads the row's provenance and decides
        // whether taking a half out removes the row or gives it back, so this
        // arm exists to name the act and not to repeat the rule.
        Hit::RemoveField(key) | Hit::DisownField(key) => {
            if let Some(node) = state.active_card() {
                remove_row(state, node, &key).ok();
            }
        }
        Hit::Part { key, part } => act_on_part(state, &key, &part),
        _ => {}
    }
}

/// ★★ R1687 — take the graph's configuration off the screen, and answer what
/// was said about it.
///
/// One function for the pointer and the wire, which is the rule this screen has
/// held since R1682: an affordance that did its own version of an operation is
/// how the two channels come to disagree about what the operation *is*.
///
/// ★ The verdict rides along. The reference's own toast reports it, and the
/// reason is that an exported configuration is a thing somebody is about to
/// **use** — a person who reads only "12 node configurations" and walks away
/// with a set of files that will not start has been told the wrong thing.
fn export_configuration(state: &LabState) -> String {
    let plan = state.plan();
    // R1788 — the framework renders the plan as the ARTIFACT it is, which is
    // text; this screen publishes it as structure because its wire is JSON.
    // The parse cannot fail on what we just wrote, and the fallback carries the
    // reason rather than an empty object, so a reader is never handed a plan
    // that quietly became nothing — which is the floor's own failure.
    let document = plan.to_document().map_or_else(
        |why| serde_json::json!({ "error": why.to_string() }),
        |text| {
            serde_json::from_str(&text)
                .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
        },
    );
    let verdict = state.verdict();
    let said = Utterance::done(deploy::export_sentence(
        &plan,
        (!verdict.may_launch())
            .then(|| verdict.sentence())
            .as_deref(),
    ));
    state.produced.borrow_mut().config = Some(document);
    state.say(said.clone());
    said.sentence()
}

/// The same plan, rendered as the script that starts it.
fn produce_script(state: &LabState) -> String {
    let plan = state.plan();
    // R1788 — a script refuses when two cards answer to one name, because it
    // would write two heredocs to one path. The refusal is the artifact here:
    // a person reading it is told what to fix, where a produced script with a
    // silently missing process would not be.
    let script = plan.to_script().unwrap_or_else(|why| format!("# {why}"));
    let said = Utterance::done(deploy::script_sentence(&plan));
    state.produced.borrow_mut().script = Some(script);
    state.say(said.clone());
    said.sentence()
}

// ── Where the canvas is pointed ─────────────────────────────────────────────

/// This screen's zoom bounds, as the substrate's validated range.
///
/// ★ One declaration read by the fit and by the anchored zoom, rather than
/// [`ZOOM_MIN`] and [`ZOOM_MAX`] re-clamped at each call — which is how the two
/// come to disagree about what "as far out as it goes" means.
fn zoom_range() -> ZoomRange {
    ZoomRange::new(f64::from(ZOOM_MIN) / 100.0, f64::from(ZOOM_MAX) / 100.0)
        .expect("ZOOM_MIN..=ZOOM_MAX are two ordered positive scales")
}

/// Where the canvas is pointed right now, as the substrate states it.
///
/// This screen keeps the zoom as a whole percentage (it is shown as one, and a
/// reading of `83.7%` is not a thing a person asked for) and the pan in whole
/// pixels; the camera is the same two facts as the affine the substrate owns.
fn camera_now(state: &LabState) -> Camera {
    let (pan_x, pan_y) = state.pan.get();
    Camera::new(
        f64::from(state.zoom.get()) / 100.0,
        (f64::from(pan_x), f64::from(pan_y)),
    )
}

/// Point the canvas at a camera the substrate answered, at the whole percentage
/// this screen stores its zoom in.
///
/// ★★ The percentage is the CALLER's, because the rounding is the caller's
/// question and not one answer fits both: a fit must round **down** (a fit that
/// rounded up no longer fits, by less than a percent, which is the worst kind of
/// wrong to look at) while a stepper already has the whole number it asked for
/// and must not be handed `28` for a `0.29` that a binary fraction spells
/// `0.28999999999999998`. Deriving it here made exactly that mistake.
///
/// ★ Re-pinning at the zoom that was actually TAKEN, rather than keeping the
/// camera's pan, is what keeps the anchor still: rounding the scale and not the
/// offset leaves the graph off by half the rounding, and worse the further from
/// the origin — the R1684.4 error shape in a new place.
///
/// ★★ R1703 — the `anchor` is the CALLER's too, and was the canvas middle
/// written here twice until a wheel needed the cursor. Baking it in was the
/// reason a wheel could not reuse this: the substrate's
/// [`Camera::zoomed_at`] takes an anchor, and this function then threw it away
/// and re-pinned at the middle, so a cursor-anchored zoom composed of the two
/// would have silently become a centred one. Both gestures now name the point
/// they hold, and the arithmetic between them is one path.
fn point_canvas_at(state: &LabState, percent: u32, camera: Camera, anchor: (f64, f64)) {
    let percent = percent.clamp(ZOOM_MIN, ZOOM_MAX);
    let settled = Camera::pinned(f64::from(percent) / 100.0, camera.unproject(anchor), anchor);
    // ★ NOT `clamp_to_world`: that bounds a NODE's position in canvas units,
    // and a pan is a window-pixel offset with no such bound — the drag does not
    // clamp it either, and clamping here would move the graph off the centre
    // this function exists to put it on.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a pan is a window pixel offset; it fits an i32"
    )]
    let whole = |v: f64| v.round() as i32;
    state.zoom.set(percent);
    state.pan.set((whole(settled.pan.0), whole(settled.pan.1)));
}

/// ★★ R1688 — a zoom step, **anchored at the middle of the canvas**.
///
/// The reference's own: its `+` and `−` zoom about the viewport centre. This
/// screen changed the scale and left the pan, which anchors at the canvas
/// ORIGIN — so zooming out from a graph you had panned to walked it off the
/// top-left corner of the screen. One function for the two steppers and for the
/// wire's `zoom_by`, so the seat and the verb cannot anchor differently.
/// One step in or out from where the zoom is now, inside the range.
fn zoom_stepped(state: &LabState, up: bool) -> u32 {
    let zoom = state.zoom.get();
    if up {
        (zoom + ZOOM_STEP).min(ZOOM_MAX)
    } else {
        zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
    }
}

fn zoom_to(state: &LabState, percent: u32) -> u32 {
    zoom_to_at(state, percent, canvas_middle())
}

/// The canvas viewport's middle, in the canvas-local pixels a zoom anchors in.
fn canvas_middle() -> (f64, f64) {
    let canvas = canvas_rect();
    (f64::from(canvas.w) / 2.0, f64::from(canvas.h) / 2.0)
}

/// ★★★ R1703 — a zoom to `percent` **holding the canvas point under `anchor`
/// still**, where `anchor` is canvas-local (the pane's own top-left is `0, 0`,
/// exactly the reference prototype's `clientX - rect.left`).
///
/// One function for both gestures, which is the whole point: the seats zoom
/// about the middle and the wheel zooms about the cursor, and those are the
/// same operation with a different point. Two copies of it is how a screen ends
/// up with a wheel that drifts and buttons that do not, or the reverse — and
/// the arithmetic itself is [`Camera`]'s, not this screen's, so the two
/// directions of the affine cannot come apart (R1653 found three copies of that
/// conversion here, and the graph slid under the cursor).
fn zoom_to_at(state: &LabState, percent: u32, anchor: (f64, f64)) -> u32 {
    let camera = camera_now(state).zoomed_at(f64::from(percent) / 100.0, anchor, &zoom_range());
    point_canvas_at(state, percent, camera, anchor);
    state.zoom.get()
}

/// R1703 — the multiplicative step one wheel event applies to the zoom.
///
/// The behaviour canon's own factor, and multiplicative rather than the seats'
/// additive [`ZOOM_STEP`] for a reason a test states
/// (`r1703_a_wheel_out_and_back_returns_to_the_same_zoom`): scaling by `k` and
/// then by `1/k` is the identity, so a person who overshoots and comes back
/// lands where they were. Adding and subtracting a percentage does not — from
/// 84 that is 92 and back to 84 only because the step happens to be constant,
/// and at the range's ends it silently is not.
const WHEEL_ZOOM_STEP: f64 = 1.12;

/// R1703 §5.45 — one wheel event over the canvas, in canvas-local pixels.
///
/// Returns whether the zoom moved. `false` — at the range's ends — is the
/// **decline**, so the wheel this canvas cannot spend reaches whatever scrolls
/// behind it, the same verdict the catalog's stepped widgets give.
fn wheel_zoom(state: &LabState, direction: WheelDirection, anchor: (f64, f64)) -> bool {
    let now = state.zoom.get();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a percentage scaled by 1.12 and rounded is a percentage; the \
                  range clamps it either way"
    )]
    let wanted = (f64::from(now) * direction.scaled(WHEEL_ZOOM_STEP)).round() as u32;
    let wanted = wanted.clamp(ZOOM_MIN, ZOOM_MAX);
    if wanted == now {
        return false;
    }
    zoom_to_at(state, wanted, anchor);
    true
}

/// How much clear canvas the fit keeps around the graph, in canvas units.
///
/// The reference's own number, and its own frame of reference: it pads the
/// bounding box in graph units rather than keeping a pixel gutter, so the
/// clearance is part of the diagram and shrinks with it. [`Margin`] is what lets
/// that be said instead of assumed — `hello-node-editor` frames with a **screen**
/// margin, and the two are different scales for the same graph.
const FIT_PAD: i32 = 60;

/// ★★★ R1688 — **point the canvas at the whole graph**, and say whether that
/// was possible.
///
/// The arithmetic is [`pinion_node_graph::Fit`]'s, not this screen's: two node
/// canvases in this tree were about to hold two copies of it. What is this
/// screen's is what counts as "the graph" ([`drawn_boxes`] — the cards *and* the
/// host frames) and what the units are.
fn fit_view(state: &LabState) -> String {
    let canvas = canvas_rect();
    let Some(fitted) = (Fit {
        zoom: zoom_range(),
        margin: Margin::Canvas(FIT_PAD),
    })
    .boxes(drawn_boxes(state), (canvas.w, canvas.h)) else {
        // Unreachable while a card exists — and `delete_node` refuses the last
        // one — so this is the honest answer to a state the screen does not
        // have rather than a case it expects.
        let said = Utterance::new(Tone::Refused, "nothing to frame");
        state.say(said.clone());
        return said.into_clause();
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a zoom the range clamped into ZOOM_MIN..=ZOOM_MAX is a percentage"
    )]
    let percent = (fitted.camera.zoom * 100.0).floor() as u32;
    point_canvas_at(state, percent, fitted.camera, canvas_middle());
    let said = Utterance::done(if fitted.complete {
        format!("the whole graph, at {}%", state.zoom.get())
    } else {
        // ★★ The sentence the reference cannot say. Its fit reports nothing, so
        // a graph larger than the zoom floor can shrink looks like a fit that
        // did not work — and the person presses the button again.
        format!(
            "as much as {}% shows — the graph is wider than the view can hold",
            state.zoom.get()
        )
    });
    state.say(said.clone());
    said.sentence()
}

/// How much clear canvas a jump keeps around the card it brings into view.
///
/// Smaller than [`FIT_PAD`]: a fit is a composition and wants air, a jump is an
/// answer to "where is it" and wants the card *on screen* with enough room that
/// it does not read as clipped.
const REVEAL_PAD: i32 = 24;

/// ★★★ R1688 — **go to the first thing wrong with the graph.**
///
/// Selecting it is what the reference does, and selecting it is what changes the
/// inspector — so the operation is observable even when the card was already in
/// view. Bringing it into view is this screen's own addition, and it is the
/// substrate's minimal reveal rather than a re-centring: a person who has just
/// panned somewhere deliberately should not have the view thrown away to show
/// them something that was already on it.
fn go_to_problem(state: &Rc<LabState>) -> String {
    let problems = state.problems();
    let Some(first) = problems.first() else {
        // ★ R1719 — "there is nowhere to go" is a state, not a failure: the
        // gate being clear is the good news. `Unchanged` is the arm for an act
        // that had nothing to do.
        let said = Utterance::unchanged("nothing to go to — the gate is clear");
        state.say(said.clone());
        return said.into_clause();
    };
    let Some(node) = first.node else {
        // The finding is real and no card answers to the name in it. Saying so
        // is the whole of what can be done, and it is better than a jump that
        // silently does nothing.
        let said = Utterance::refused(&format!(
            "{} · no card answers to that name",
            first.sentence
        ));
        state.say(said.clone());
        return said.into_clause();
    };
    select_card(state, Some(node));
    if let (Some((x, y)), Some(extent)) = (
        state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|tree| tree.node(node))
            .map(|held| (held.x, held.y)),
        card_extent(state, node),
    ) {
        let canvas = canvas_rect();
        let camera = camera_now(state).reveal(
            (
                x - REVEAL_PAD,
                y - REVEAL_PAD,
                x + extent.width + REVEAL_PAD,
                y + extent.height + REVEAL_PAD,
            ),
            (canvas.w, canvas.h),
        );
        point_canvas_at(state, state.zoom.get(), camera, canvas_middle());
    }
    let said = Utterance::done(first.sentence.clone());
    state.say(said.clone());
    said.sentence()
}

/// ★★★ R1684 — what a press on a form row's control does, decided by the
/// row's SHAPE.
///
/// "The type decides the control" is the reference's own rule, written beside
/// its inspector, and this is that rule at the pointer:
///
/// | shape | what a press on the control does | why |
/// |---|---|---|
/// | true/false | flips it | the reference makes the WHOLE control the target, not just the knob; ours had the knob only, so half of a switch's control was dead |
/// | everything else | opens the field over the row | the reference gives text, whole numbers and lists an input box, and a value is a value |
///
/// ★★★ **Every shape but the switch can be TYPED, including the two that have
/// pickers, and that is the round's own measurement rather than the
/// reference's design.** The reference paints a chip per option and leaves the
/// rest of the control inert; the gate that asks whether the middle of a
/// control does anything found exactly that, on `control.permissions`, in
/// every swept state. A bordered box with dead space inside it is a control
/// that lies. So the chips stay as the SHORTCUT and the row underneath them
/// can be typed — which also gives a person the only path to
/// [`ConfigDefect::OutOfRange`] on an option row, a defect that until now only
/// an agent could produce.
///
/// ★★ The whole-number row is the one that matters most, and not for typing's
/// sake: its stepper CLAMPS at the field's ceiling, correctly, so before this
/// there was no way for a person to put a value out of range — and the launch
/// gate's whole purpose is to catch a value out of range. An agent could close
/// that gate and a person could not.
///
/// [`ConfigDefect::OutOfRange`]: pinion_core::widgets::config_form::ConfigDefect::OutOfRange
fn press_row(state: &Rc<LabState>, key: &str) {
    let Some(node) = state.active_card() else {
        return;
    };
    let Some(field) = selected_form_of(state, node).and_then(|form| form.field(key).cloned())
    else {
        return;
    };
    // ★★★★★ R1716 — a row nobody wrote does not open a box. It would be a box
    // that cannot commit: the form refuses the write, so a person would type,
    // press apply, and watch the old value come back with nothing said. The
    // refusal is said HERE instead, in the framework's own words, and the seat
    // beside the row offers the act that does work.
    //
    // ★★ R1717 — asked as "may they write here", not "is anything deriving
    // it". A row with two contributors is both, and the half that is theirs is
    // theirs to open.
    if !field.source().writable() {
        let from = field.source().derived_from().unwrap_or_default().to_owned();
        state.say(Utterance::new(
            Tone::Refused,
            format!("{key} is worked out from the {from}; take it over to write it"),
        ));
        return;
    }
    let shape = field.shape().clone();
    // ★★★★★ R1837 — and this is now the ONLY way a pointer flips a boolean.
    // The form used to publish a `toggle.<key>` square inside the control, so a
    // press on the mark arrived at `act_on_part` and a press anywhere else on
    // the row arrived here. The square is gone and the control IS the switch,
    // which is the behaviour canon's own shape: its boolean box takes the
    // pointer across all of it.
    if shape == FieldType::Boolean {
        flip_boolean(state, key);
        return;
    }
    // ★★★★★ R1732 — a collapsed roster opens on a press ANYWHERE on the row,
    // not only on the chevron, which is what the reference's own control does
    // and what a person aiming at a 284-wide box expects. Opening a text box
    // over it would be worse than useless: the only words it accepts are in
    // the roster, so typing is a way to write a value the form will refuse.
    if matches!(shape, FieldType::Choice { .. }) {
        open_roster(state, key);
        return;
    }
    let _ = begin_edit(
        state,
        Editing::Value {
            node,
            key: key.to_owned(),
            element: None,
        },
    );
}

/// Open the field on one element of a list row (R1684).
///
/// ★★ The half the add affordance had been missing. `add_element` puts a
/// PLACEHOLDER in the list — it has to put something, since an empty element is
/// not an element — so a screen that could add one and never say what it was
/// left a person with an invented address they could not change. Pressing the
/// element is how they change it.
fn press_element(state: &Rc<LabState>, key: &str, at: usize) {
    let Some(node) = state.active_card() else {
        return;
    };
    // ★★★★★ R1717 — an element the canvas contributed does not open a box, for
    // the reason a whole worked-out row does not: the value refuses the write,
    // so a box over it is one that cannot commit. The sentence names the
    // ELEMENT, because a row-shaped one would read as "none of this is yours"
    // over a row whose other lines are.
    let derived = selected_form_of(state, node)
        .and_then(|form| form.field(key).cloned())
        .and_then(|field| field.element_source(at).derived_from().map(str::to_owned));
    if let Some(from) = derived {
        state.say(Utterance::new(
            Tone::Refused,
            format!(
                "{key} element {} is worked out from the {from}; it is there \
                 because the canvas draws it",
                at + 1
            ),
        ));
        return;
    }
    let _ = begin_edit(
        state,
        Editing::Value {
            node,
            key: key.to_owned(),
            element: Some(at),
        },
    );
}

/// Act on an affordance inside a control.
///
/// One dispatcher over the painter's part vocabulary, so the screen answers
/// every shape's affordance rather than the two R1651 drew.
fn act_on_part(state: &Rc<LabState>, key: &str, part: &str) {
    let family = part.split('.').next().unwrap_or_default();
    match family {
        // ★★★★★ R1732 — the chevron on a collapsed control opens its roster.
        "pick" => open_roster(state, key),
        "option" => {
            if let Some(word) = part.rsplit('.').next() {
                // ★★★★★ R1732 — an option belongs to one of two shapes now,
                // and they mean different things: a set's chip TOGGLES, and a
                // roster's row CHOOSES and shuts. Which one this is comes from
                // the field's shape rather than from whether a roster happens
                // to be open, so a driver pressing the same name gets the same
                // act whatever the screen was showing.
                if chooses_one(state, key) {
                    choose_option(state, key, word);
                } else {
                    toggle_option(state, key, word);
                }
            }
        }
        // ★★★★★ R1837 — `toggle` is GONE from this vocabulary, and the arm goes
        // with it. A boolean row publishes no part any more: the control IS the
        // switch, the way a text row's control is its box, so a press on it
        // arrives as `Hit::Field` and `press_row` flips it. Leaving a dead arm
        // here would say the form still names an affordance it does not.
        "step" => step_number(state, key, part.rsplit('.').next() == Some("up")),
        // ★★★ R1684 — a list's rows: `add` grows it, and a NUMBERED one opens
        // the field on that element. The numbered case fell through to nothing
        // until this round, which is the same defect as the dropped `Field`
        // arm one level up and was found by the same gate: the part resolved,
        // the wire named it, and the press died here.
        "item" => match part.rsplit('.').next() {
            Some("add") => add_element(state, key),
            Some(number) => {
                if let Ok(at) = number.parse::<usize>() {
                    press_element(state, key, at);
                }
            }
            None => {}
        },
        _ => {}
    }
}

/// Flip a boolean field, which is what its checkbox does.
fn flip_boolean(state: &Rc<LabState>, key: &str) {
    let now = state
        .forms
        .borrow()
        .get(&state.active_card().unwrap_or(NodeId(0)))
        .and_then(|f| f.field(key).map(|v| v.value().trim() == "true"));
    let Some(now) = now else { return };
    set_and_sync(state, key, if now { "false" } else { "true" });
}

/// Move a bounded integer by one, **clamped by the bounds the field declares**.
///
/// The reason a stepper is worth painting: the field knows its range, so the
/// control can refuse to leave it instead of the gate reporting it afterwards.
fn step_number(state: &Rc<LabState>, key: &str, up: bool) {
    let Some(node) = state.active_card() else {
        return;
    };
    let next = {
        let forms = state.forms.borrow();
        let Some(field) = forms.get(&node).and_then(|f| f.field(key)) else {
            return;
        };
        let FieldType::Integer { min, max } = *field.shape() else {
            return;
        };
        let now: i64 = field.value().trim().parse().unwrap_or(min);
        let step = if up { 1 } else { -1 };
        now.saturating_add(step).clamp(min, max)
    };
    set_and_sync(state, key, next.to_string());
}

/// Append an empty element to a list field, which is what its `+` row does.
fn add_element(state: &Rc<LabState>, key: &str) {
    let Some(node) = state.active_card() else {
        return;
    };
    let next = {
        let forms = state.forms.borrow();
        let Some(field) = forms.get(&node).and_then(|f| f.field(key)) else {
            return;
        };
        let shown = field.value();
        let mut held: Vec<String> = FieldType::elements(&shown).map(str::to_owned).collect();
        held.push(format!("tcp/0.0.0.0:{}", 7400 + held.len()));
        held.join(FieldType::SEPARATOR)
    };
    set_and_sync(state, key, next);
}

/// Write a field and re-derive what the canvas shows from it.
fn set_and_sync(state: &Rc<LabState>, key: &str, value: impl Into<String>) {
    let Some(node) = state.active_card() else {
        return;
    };
    // ★ R1684 — through [`set_value`], which is now the ONE way a value gets
    // onto a row. Before this round there were two: this, for the affordances
    // inside a control, and an arm of the wire that repeated it. A stepper and
    // an agent setting the same field went through different code, and only one
    // of them said what it had done.
    set_value(state, node, key, &value.into()).ok();
}

/// Turn one option of a choice or flags field on or off.
fn toggle_option(state: &Rc<LabState>, key: &str, word: &str) {
    let Some(node) = state.active_card() else {
        return;
    };
    let mut forms = state.forms.borrow_mut();
    let Some(form) = forms.get_mut(&node) else {
        return;
    };
    let Some(field) = form.field(key) else { return };
    let one_only = field.shape().one_only();
    let mut chosen: Vec<String> = field
        .value()
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if one_only {
        chosen = vec![word.to_owned()];
    } else if let Some(at) = chosen.iter().position(|w| w == word) {
        chosen.remove(at);
    } else {
        chosen.push(word.to_owned());
    }
    form.set(key, chosen.join(FieldType::SEPARATOR)).ok();
    drop(forms);
    sync_node(state, node);
}

/// Whether that row holds **exactly one** of a fixed set — the shape whose
/// control collapsed at R1732.
///
/// Read from the field rather than from whether a roster is open, so a press on
/// `option.<key>.<word>` means the same thing however it arrived.
fn chooses_one(state: &Rc<LabState>, key: &str) -> bool {
    state
        .active_card()
        .and_then(|node| {
            state
                .forms
                .borrow()
                .get(&node)
                .and_then(|form| form.field(key))
                .map(|field| matches!(field.shape(), FieldType::Choice { .. }))
        })
        .unwrap_or(false)
}

/// ★★★★★ R1732 — open the roster on that row, highlighting the word the
/// document holds.
///
/// A word the roster no longer offers is **kept**: [`Picker::over`] records it
/// and the highlight starts at the first option, so the value stands until the
/// reader chooses. The floor replaces it, silently and with no signal — which
/// is the behaviour a configuration editor can least afford, because the value
/// it quietly drops is the one somebody typed.
fn open_roster(state: &Rc<LabState>, key: &str) {
    let Some(node) = state.active_card() else {
        return;
    };
    // ★★★★★ R1732 — **opening a roster is opening the form somewhere else**,
    // so it applies what is in the field, exactly as [`begin_edit`] does when a
    // press moves between rows. Without this the two editors are open at once
    // and the roster takes the keyboard, so a person half-way through typing a
    // name loses every keystroke after the press with nothing said.
    //
    // A REFUSED commit refuses the move, for `begin_edit`'s reason: the field
    // stays where it is holding the text, the toast says why, and opening
    // anyway would destroy the thing the refusal was about.
    if state.editing.get().is_some() && commit_edit(state).is_err() {
        return;
    }
    let opened = {
        let forms = state.forms.borrow();
        let Some(field) = forms.get(&node).and_then(|form| form.field(key)) else {
            return;
        };
        // A derived row refuses every write, so offering to pick into it would
        // be the invitation R1716 took the chips away to stop making.
        if !field.source().writable() {
            return;
        }
        let FieldType::Choice { of } = field.shape() else {
            return;
        };
        Picker::over(of.clone(), field.value().trim()).ok()
    };
    let Some(picker) = opened else { return };
    state.picking.set(Some((key.to_owned(), picker)));
}

/// Shut the roster, whatever it was showing.
fn close_roster(state: &Rc<LabState>) {
    if state.picking.get().is_some() {
        state.picking.set(None);
    }
}

/// Write one word into a single-choice row and shut the roster.
///
/// The write and the shut are one act because choosing IS the end of the
/// picking, and a roster left standing over the value it just wrote would hide
/// the row whose change a reader came to see.
fn choose_option(state: &Rc<LabState>, key: &str, word: &str) {
    let Some(node) = state.active_card() else {
        return;
    };
    {
        let mut forms = state.forms.borrow_mut();
        let Some(form) = forms.get_mut(&node) else {
            return;
        };
        form.set(key, word).ok();
    }
    close_roster(state);
    sync_node(state, node);
}

/// Put a new node of that role at the middle of the canvas.
/// Where a new card can go without covering one that is already there, in the
/// CANVAS coordinates a node is stored in.
///
/// ★ R1656 — the centre, and then straight down until the spot is free.
///
/// It was the centre unconditionally, and a card dropped on top of another is
/// two cards that answer for the same pixels. The first repair searched in
/// WINDOW coordinates and compared against `card_rect`, which is in world ones —
/// `to_canvas` and `to_content` are not inverses, they map different pairs of
/// frames — so nothing ever looked occupied and six added nodes landed in one
/// stack. Measured by the test written for it, which is the point of writing
/// one: the round's own repair was wrong and said so on its first run.
///
/// ★★★★★ R1774 — and the SAME MISTAKE was still in it, one frame further out.
/// The search compared each card's STORED position against a measured size, and
/// a card's stored position is not where it is drawn: `drawn_boxes` has derived
/// that from `card_shape_at(.., UNZOOMED)` since R1688, because a card inside a
/// host frame is offset by the frame and by the world origin. Mixing the two is
/// the R1656 error above in a new place, and it cost three repairs — a constant
/// size for everything, then measuring the avoided cards, then measuring the
/// placed one too — each of which changed the count without fixing it (6, 6,
/// 18). The third told the truth only because the gate was made to print the
/// two rectangles; a tally of counts had said `ANOTHER card, 6` throughout.
///
/// So the search now runs entirely in DRAWN canvas units, and returns a DELTA.
/// A delta is what can cross the two frames safely: whatever the mapping from a
/// stored position to a drawn one is, it is additive, so moving the stored
/// position by `d` moves the drawn box by `d`. Nothing here has to know what the
/// frame offset is, which is why this cannot be half-right the way the previous
/// three were.
fn free_spot(state: &LabState, mover: NodeId) -> (i32, i32) {
    // A card that cannot be measured is the one case with no honest answer, so
    // it stays where it is rather than being moved by a guess.
    let Some((at, size)) = drawn_box_of(state, mover) else {
        return (0, 0);
    };
    let taken: Vec<((i32, i32), Extent)> = state
        .cards()
        .into_iter()
        .filter(|node| *node != mover)
        .filter_map(|node| drawn_box_of(state, node))
        .collect();
    let clear = |x: i32, y: i32| {
        taken.iter().all(|((hx, hy), held)| {
            x >= hx + held.width
                || *hx >= x + size.width
                || y >= hy + held.height
                || *hy >= y + size.height
        })
    };
    let step = size.height + 12;
    let (mut x, mut y) = at;
    // Bounded: a column full to its own depth wraps to the next one rather than
    // looping forever.
    for attempt in 0..64 {
        if clear(x, y) {
            break;
        }
        y += step;
        if attempt % 8 == 7 {
            x += size.width + 12;
            y = at.1;
        }
    }
    (x - at.0, y - at.1)
}

/// Where one card is DRAWN, in canvas units with the world origin taken off.
///
/// ★ R1774 — lifted out of [`drawn_boxes`], which has computed exactly this
/// since R1688 and was the only thing that knew a card's drawn position is not
/// its stored one. A second copy is how two callers come to disagree about
/// which frame they are in, and that disagreement is this function's whole
/// history.
fn drawn_box_of(state: &LabState, node: NodeId) -> Option<((i32, i32), Extent)> {
    let whole = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    let rect = card_shape_at(state, node, UNZOOMED)?.rect;
    Some((
        (whole(rect.x) - WORLD_ORIGIN, whole(rect.y) - WORLD_ORIGIN),
        Extent::new(whole(rect.w), whole(rect.h)),
    ))
}

/// Put the selected card on another build (R1885).
///
/// ★★★★★ **The act a compatibility test graph exists to perform.** Every other
/// edit on this screen changes what a node is *configured* to do; this changes
/// which program it is, and the launch gate then says whether the wires it is
/// on can still negotiate. The span comes with the build rather than being
/// chosen separately, because a person deploying an older release does not pick
/// which revisions it speaks — the release does.
///
/// Says what happened either way: a refusal a person cannot hear is a control
/// that looks broken, and "nothing is selected" is a different fact from "that
/// card already runs this build".
fn set_build(state: &Rc<LabState>, stack: Stack) {
    let Some(node) = state.active_card() else {
        state.say(Utterance::refused(&"select a card first"));
        return;
    };
    set_build_on(state, node, stack);
}

/// The one implementation the press and the wire verb share.
///
/// ★ Returning the sentence rather than only saying it is what lets the agent's
/// channel and the person's toast come off ONE value — the rule this screen
/// already follows for its refusals, so the two cannot drift.
fn set_build_on(state: &Rc<LabState>, node: NodeId, stack: Stack) -> String {
    let name = state.name_of(node);
    let want = Implementation {
        stack,
        speaks: spec_revisions(stack),
    };
    let changed = {
        let mut doc = state.doc.borrow_mut();
        match doc.tree_mut(ROOT).and_then(|t| t.node_mut(node)) {
            Some(slot) => match &mut slot.body {
                NodeBody::Kind(kind) if kind.implementation != want => {
                    kind.implementation = want;
                    true
                }
                _ => false,
            },
            None => false,
        }
    };
    let said = if changed {
        Utterance::done(format!(
            "{name} runs the {} build, {}",
            stack.word(),
            want.speaks.word()
        ))
    } else {
        Utterance::unchanged(format!("{name} already runs the {} build", stack.word()))
    };
    state.say(said.clone());
    said.into_clause()
}

/// Which revisions each build speaks (R1885).
///
/// One place, so the palette, the opening graph and the inspector cannot
/// disagree about what a build is.
///
/// 🟥🟥🟥 ★★★★★ **The first draft of these three spans made a refusal
/// UNREACHABLE, and the walk is what found it.** Every build started at `v4`,
/// so every pair overlapped and no edit a person could perform could ever
/// produce an incompatibility — the screen carried a rule, a violation, a
/// finding and a gate line for a situation that could not arise. The test drove
/// the edit, read the gate and got an empty list.
///
/// ⇒ **ask of a refusal what this project asks of a completion: is there a path
/// to it?** A vocabulary whose every member is compatible with every other is
/// not a compatibility model, it is decoration.
///
/// So the spans are chosen to make all three answers reachable: the reference
/// and the independent re-implementation OVERLAP (which is what lets the
/// opening graph be heterogeneous and still valid), and the legacy release
/// shares nothing with either (which is what makes it the build an edit can
/// introduce to break a wire).
fn spec_revisions(stack: Stack) -> Revisions {
    match stack {
        Stack::Reference => Revisions::new(6, 8),
        Stack::Independent => Revisions::new(5, 7),
        Stack::Legacy => Revisions::new(2, 4),
    }
}

fn add_node(state: &Rc<LabState>, role: Role) {
    let canvas = canvas_rect();
    let want = to_canvas(state, canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
    // ★★★★★ R1774 — the card is BUILT first, at the spot it would like, and
    // only then moved to one that is clear. A card's size follows its label and
    // its form, so nothing can measure it until both exist, and the previous
    // shape of this function had to guess instead: it searched with a constant
    // and the guess was wrong in both directions at once. Design units
    // throughout, because that is what a node's stored position is in — the
    // card is painted at `zoom` times this, and so is every other card.
    let id = {
        let mut doc = state.doc.borrow_mut();
        doc.add_node(
            ROOT,
            NodeBody::Kind(LabNode {
                role,
                transport: Transport::Tcp,
                listening: false,
                // R1885 — a node the palette adds runs the reference build, so
                // adding one never introduces an incompatibility a person did
                // not ask for. Choosing another build is an edit, not a default.
                implementation: Implementation::default(),
            }),
            want.0,
            want.1,
        )
    };
    let Ok(id) = id else { return };
    let name = format!("{}-{:02}", role.badge(), id.0);
    if let Some(slot) = state
        .doc
        .borrow_mut()
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(id))
    {
        slot.label = Some(name.clone());
    }
    state.forms.borrow_mut().insert(id, form_for(&name, role));
    // The card exists and can be measured now, so the spot it ends up in is
    // computed from where it is DRAWN and applied as a delta to where it is
    // STORED. See `free_spot` for why a delta rather than a position.
    let (dx, dy) = free_spot(state, id);
    let (cx, cy) = (want.0 + dx, want.1 + dy);
    if let Some(slot) = state
        .doc
        .borrow_mut()
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(id))
    {
        slot.x = cx;
        slot.y = cy;
    }
    // ★ R1679 — where this card came into being, which is the only thing a
    // layout reset can put it back to. A card the specification does not
    // describe has no other source, and without this the layout predicate was
    // blind to it: measured, dragging an added card moved it from [502,476] to
    // [562,512] while `changed.layout` stayed false.
    state.opened_at.borrow_mut().insert(
        id,
        Placement {
            at: (cx, cy),
            host: None,
            // ★ R1682 — `None` is what makes this card a stray to the node
            // reset. Not "the name it happens to have now", which a rename
            // moves; "the name the specification gave it", which nothing does.
            opened_as: None,
        },
    );
    select_card(state, Some(id));
    state.say(Utterance::done(format!("added {name}")));
}

/// ★★★★★ R1732 — **the keyboard half of the collapsed roster**, and the one
/// place both doors onto it go through.
///
/// Returns whether the key was the roster's. Answered before every other
/// binding on this screen, because an open roster is what the keyboard is
/// pointed at: `Space` runs the graph while nothing is open and chooses the
/// highlighted word while something is, and a screen that asked in the other
/// order would launch a deployment from inside a menu.
///
/// The reference has **no keyboard bindings at all** — measured, zero
/// `keydown` handlers across the whole prototype — so this is not reproduction,
/// it is the second pass over what the first one left pointer-only. It is
/// written as an addition and not a replacement: every act here is still
/// reachable by pointer and by wire.
fn roster_key(state: &Rc<LabState>, chord: &str) -> bool {
    let Some((key, mut picker)) = state.picking.get() else {
        return false;
    };
    match picker.key(chord) {
        Picked::Moved => {
            state.picking.set(Some((key, picker)));
            true
        }
        Picked::Chose(word) => {
            choose_option(state, &key, &word);
            true
        }
        Picked::Dismissed => {
            close_roster(state);
            true
        }
        Picked::Ignored => false,
    }
}

fn key(state: &Rc<LabState>, chord: &str) -> bool {
    if roster_key(state, chord) {
        return true;
    }
    match chord {
        "Escape" => {
            state.drag.set(None);
            state.selected_link.set(None);
            true
        }
        "Space" => {
            let verdict = state.verdict();
            if state.running.get() {
                state.running.set(false);
            } else if verdict.may_launch() {
                state.running.set(true);
            } else {
                state.say(Utterance::refused(&verdict.sentence()));
                return false;
            }
            true
        }
        "Plus" | "Minus" => {
            let zoom = state.zoom.get();
            state.zoom.set(if chord == "Plus" {
                (zoom + ZOOM_STEP).min(ZOOM_MAX)
            } else {
                zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
            });
            true
        }
        _ => false,
    }
}

impl External for LabOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// A drag that strays off a pin must keep previewing rather than being
    /// cancelled by a stray pixel.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// The screen tracks the cursor, because a press carries no coordinates.
    fn wants_hover_move(&self) -> bool {
        true
    }

    /// ★★★★★ R1703 §5.45 — **the wheel the hint strip has been advertising.**
    ///
    /// `spec::GESTURES` has said `wheel → zoom` since this screen existed, and
    /// painted it where a person reads it, and nothing answered: measured at
    /// the start of this round, eight wheel events and two `Ctrl`-wheel events
    /// over the canvas left `zoom` at 84 and `pan` at `0,0`. A person reported
    /// it (R1673); the reason no gate saw it is that the operation table's
    /// `zoom` row is satisfied by the zoom SEATS, which work, and the hint
    /// strip's four claims were painted and driven by nothing at all.
    ///
    /// The behaviour is the canon's, read off the prototype rather than
    /// invented: **one event is one step whichever way the platform sizes a
    /// notch** (the direction is read, the magnitude is not — that is what
    /// keeps a zoom from leaping on one mouse and crawling on another), the
    /// step is multiplicative, and the canvas point under the cursor stays
    /// under the cursor.
    fn wheel(&mut self, reading: &pinion_core::widgets::wheel::WheelReading) -> bool {
        let Some(state) = self.state.clone() else {
            return false;
        };
        let Some(direction) = reading.direction() else {
            // A phase-only marker or a horizontal-only trackpad event: nothing
            // to zoom by, and consuming it would take a horizontal scroll away
            // from a pane that could use it.
            return false;
        };
        let canvas = canvas_rect();
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, reading.at);
        // ★ Outside the canvas the wheel is not this gesture's — the canon
        // checks the same rectangle before it does anything (`if the cursor is
        // outside the viewport, return`), and declining here leaves the wheel
        // to the two side panes, which scroll.
        if !contains(canvas, px, py) {
            return false;
        }
        wheel_zoom(
            &state,
            direction,
            (
                f64::from(px) - f64::from(canvas.x),
                f64::from(py) - f64::from(canvas.y),
            ),
        )
    }

    /// R1703 §5.45 — and the screen SAYS so, which is what makes the router
    /// offer the event above at all and what `scene/wheel_intent` answers.
    ///
    /// ★★★★★ **Over the canvas, and only there.** §2 #7 makes this whole
    /// screen one `External`, so a declaration that ignored the point would
    /// have told the wire that a wheel over the palette zooms the graph — and
    /// the first thing this round's own measurement did, one minute after the
    /// wheel started working, was catch exactly that: the wire said `"zoom"` at
    /// `(64, 64)`, which is inside the palette, where [`Self::wheel`] declines
    /// and the pane behind it scrolls. The published answer was coarser than
    /// the behaviour, which is the drift this whole mechanism exists to
    /// prevent, and it took a *parameter on the trait method* to close rather
    /// than care here.
    fn wheel_intent(&self, at: (f32, f32)) -> Option<pinion_core::widgets::wheel::WheelIntent> {
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at);
        contains(canvas_rect(), px, py).then_some(pinion_core::widgets::wheel::WheelIntent::Zoom)
    }

    fn pointer_move(&mut self, at: PointerReading) {
        let Some(state) = self.state.clone() else {
            return;
        };
        // ★ R1656 — the fraction is of the LIVE surface, and this multiplies by
        // the live surface.
        //
        // It multiplied by the design size until a person reported that nodes
        // stop clicking after a maximise. `External::pointer_move` hands a
        // fraction of the widget's post-layout rect and does not hand the rect,
        // so a consumer that wants pixels has to find the basis somewhere else
        // — and `window_size()` reads `use_viewport_size`, which needs a
        // reactive scope. There is none inside a pointer callback, so it fell
        // through to the design constants and every coordinate arrived scaled
        // by opening-size over current-size. Measured exactly: after a maximise
        // to 2494x1531 the app was told 0.5775x horizontally (1440/2494) and
        // 0.5880x vertically (900/1531), so a press aimed at the right-hand
        // inspector landed sixty pixels away and nothing under the cursor
        // answered.
        //
        // ★★★★★ R1714 — and the whole expression is the framework's now, not
        // just the basis. `layout_point` clamps, multiplies AND adds the
        // window's pan, which this screen has since it declared [`SHRINK`] as a
        // pan: below the comfortable size the window is a viewport onto a
        // bigger layout, so the pixel a fraction names in the window and the
        // pixel it names in the frame these rectangles are stated in differ by
        // however far the reader has panned. Measured with the offset ignored:
        // `scene/pointer_target` fell from 46 addressable rectangles to 28 and
        // eight became addressable at no point inside themselves.
        // ★★★★★ R1727 — and the rect is no longer missing. The comment above is
        // the one this round's type was built from: `pointer_move` handed a
        // fraction and not the rectangle, so a screen that wanted pixels had to
        // find a basis elsewhere, and the basis it found was wrong for two
        // rounds. `at.extent` is that rectangle, carried by the reading itself.
        // This screen keeps `layout_point` because it needs the PAN term too,
        // and the two agree — asserted rather than assumed, by
        // `r1727_the_readings_extent_is_the_surface_the_screen_was_told_about`.
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at.at);
        move_cursor(&state, px, py);
    }

    /// ★★★★★ R1700 §5.35 — what a press here addresses, for the framework to
    /// hold against what this screen painted here.
    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        // ★ R1714 — the framework asks in the frame the PAINT is published in,
        // and this screen answers in the frame its own rectangles are stated
        // in. The two are the same window until the window pans over the
        // layout, and then they differ by exactly the pan.
        let (x, y) = pinion_core::external::into_layout(VIEW_TAG, (x, y));
        self.state
            .as_ref()
            .map_or(PointerTarget::Unanswered, |s| Hit::at(s, x, y).target(s))
    }

    /// ★★★★★ R1700 §5.35 — the same question by name.
    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        self.state
            .as_ref()
            .map_or(PointerTarget::Unanswered, |s| Hit::of_tag(s, tag).target(s))
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

// ── The binding ─────────────────────────────────────────────────────────────

/// ★★★★★ R1724 — **the binding, public, because it is also a page.**
///
/// This screen is mounted as the Catalog destination of the analysis-tool
/// shell through `pinion_screen::Mount<NodeLabView>`, which is what makes the
/// tool one application rather than three executables. Nothing about the
/// screen changed to allow that: a mounted screen is the binding it already
/// was, and this line is the whole of the difference.
/// The card [`NodeLabView::pose`] poses on.
///
/// ★ It does **not** arrive carrying a `routing.mode` row, and the first draft
/// of the pose said it did. The row is one of the form's *addable* fields —
/// offered as a palette chip, put on the card by a press — so a pose that only
/// selected this card left the surface the specification is written against off
/// the screen, and the walk reported the section as never reproducing it. The
/// same fact is why R1732's own gate clicks the chip before it reads anything.
const POSE_CARD: &str = "P-01";

pub struct NodeLabView;

impl WidgetCore for NodeLabView {
    /// ★★ R1683 — the shared field's posture and caret, which the shell reads
    /// out of the painted scene and hands back to the view. The same contract
    /// the sibling node editor uses, so the field's own external stays the
    /// authority on what it holds and the view never guesses.
    type State = (TextFieldState, u32);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = LabOracle::new();
        oracle.attach(use_lab_state());
        Box::new(oracle)
    }

    /// ★★★ R1683 — the field's own external, mounted beside this screen's.
    ///
    /// `view_field` paints a container; the thing that HOLDS the text, owns
    /// focus, takes a keystroke and answers what it is doing is a separate
    /// external addressed by the same tag. Measured while wiring this: without
    /// it the field painted, the screen's `editing` slot said it was open, and
    /// every keystroke was refused — because the keymap forwards to an external
    /// that was not there. `blur_committing_field_extra` is the lifted
    /// commit-on-blur one the sibling node editor mounts, so a click away from
    /// the box does what a click away from a box does.
    fn create_extra_externals() -> Vec<pinion_core::widget_core::ExtraExternal> {
        vec![pinion_core::widgets::text_field::blur_committing_field_extra(EDIT_TAG)]
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    /// ★★★★★ R1911 — this screen's marks are addressed under `lab.`, not under
    /// its root tag; the root is one marker node. See
    /// [`pinion_core::WidgetCore::paint_stems`].
    fn paint_stems() -> Vec<&'static str> {
        vec![VIEW_TAG, "lab"]
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, EDIT_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, *frame)
    }

    /// ★★★ R1683 — the keystroke path, and it is the framework's keymap rather
    /// than another copy of one.
    ///
    /// `edit_field_keymap` is the lifted SSOT the data grid, the property grid
    /// and the sibling node editor already share; this is its FOURTH call site.
    /// Enter commits through the same verb the wire calls, Escape closes and
    /// leaves the value alone, and a named key this screen wants while the
    /// field is shut (Space to run, +/- to zoom) is deliberately NOT reached
    /// while it is open — a person typing a name must be able to type a space.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let state = use_lab_state();
        // ★★★★★ R1732 — an OPEN roster owns the keyboard, ahead of the focus
        // test below: it is drawn over the pane and a reader who opened it is
        // aiming at it. Shut, the control it belongs to opens on the same keys
        // it would then answer, so nothing has to be learned twice.
        if roster_key(&state, key) {
            return true;
        }
        if let Some(row) = focused.and_then(|tag| tag.strip_prefix("lab.form.control.")) {
            if state.picking.get().is_none() && Picker::opens(key) && chooses_one(&state, row) {
                open_roster(&state, row);
                // A letter that opened the roster also moves in it, so the
                // reader's first keystroke is not swallowed by the opening.
                if key.chars().count() == 1 && key != " " {
                    roster_key(&state, key);
                }
                return state.picking.get().is_some();
            }
        }
        if focused != Some(EDIT_TAG) {
            return false;
        }
        let kind = state
            .editing
            .get()
            .as_ref()
            .map_or(CellKind::Text, Editing::kind);
        edit_field_keymap(
            scene,
            EDIT_TAG,
            key,
            modifiers,
            kind,
            || {
                commit_edit(&state).ok();
            },
            || end_edit(&state),
        )
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-node-lab (R1651 §5.21 node graph lab)"
    }
}

impl WidgetA11y for NodeLabView {
    /// ★★★★★ R1691 — **the whole screen, not the parts somebody asked about.**
    ///
    /// Until this round the tree held the graph, the eight cards, the toolbar
    /// seats and the inspector's form rows: 35 nodes, of which **30** name a
    /// painted region, against **166** painted addressable regions — so **136**
    /// were unclassified. The palette, the icon rail, the canvas's frames and
    /// wires and pins, the launch gate, the gesture hint and the inspector's own
    /// chrome had no voice at all. That was not one omission — each of the three
    /// clusters that *did* have one had a round that asked for it (the cards at
    /// R1651, the toolbar at R1687), and nothing had ever asked about the rest.
    ///
    /// So it is answered by region, each from the data the painter uses, and the
    /// question "did anything get left out" is [`pinion_core::voice`]'s to ask
    /// rather than a reader's to notice.
    fn access_node(_state: &(TextFieldState, u32), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_lab_state();
        let verdict = state.verdict();
        let mut nodes = vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name(format!("{} node graph", spec::GRAPH_NAME))
                .with_value(AccessValue::Text(format!(
                    "{} nodes, {} links, {}",
                    state.cards().len(),
                    state.link_count(),
                    verdict.sentence(),
                ))),
        ];
        nodes.extend(appbar_access(&state));
        // ★★★★★ R1725 — and out of the TREE too, from the same one question the
        // paint asks. Measured at 6.11.1: a placed application window's menu
        // bar, tool bar and status bar all stay in the tree beside the host's,
        // so a reader is told the application has two of each — and there is no
        // API by which the guest could have known. This is the half that
        // measurement makes non-negotiable: omitting the paint alone would
        // leave a navigation a screen reader can walk to and a pointer cannot.
        if draws_own_rail() {
            nodes.extend(rail_access());
        }
        nodes.extend(palette_access(&state));
        nodes.extend(canvas_access(&state));
        nodes.extend(gate_access(&state));
        // ★★★★★ R1909 — the fault panel is DRAWN INSIDE the inspector and
        // announced from out here, which is a split the paint does not have:
        // the pane builds its body behind a closure, so folding it removes the
        // panel from the screen and left this announcement standing. The sweep
        // reported it as `lab.faults (ghost)` — a region a reader is told about
        // and a pointer cannot reach — the same failure R1887.1 found under a
        // folded palette.
        //
        // Asked of the SPECIFICATION rather than of `SidePanel::Inspector`
        // directly: `spec::PANES[3].holds` is where "the fault panel is inside
        // the inspector" is written down, and a condition spelled here would be
        // a second copy of that fact — free to disagree with the one the paint
        // gates read.
        if !in_folded_pane(spec::FAULT_PANEL.tag) {
            nodes.extend(fault_access(&state));
        }
        nodes.extend(toolbar_access(&state));
        nodes.extend(inspector_access(&state));
        nodes
    }
}

/// The application bar: what this screen is, and whether the graph is running.
///
/// ★ The running word is its own **live** status rather than part of the bar's
/// name: it changes without anybody moving focus, which is the one case where a
/// reader has to be told rather than asked to go and look.
fn appbar_access(state: &LabState) -> Vec<AccessNode> {
    // ★★★★★ R1822 — empty where this screen draws no bar. The paint, the
    // layout, the silence above and this tree all read one predicate, so a
    // reader cannot be offered a landmark for a strip that is not on screen.
    //
    // 🟥🟥🟥 ★★★★★ R1825 — **and empty is not enough.** The graph's name is
    // painted at `lab.toolbar.title`, which defers to this bar with a
    // `Silence::name_of("lab.appbar")`. R1822 dropped that deferral where the
    // bar is absent and stopped there, which moves the node from *wrongly
    // quiet* to **undecided** — a different fault, not a repair. Measured on
    // the running application: `lab.toolbar.title`, `voice: "unvoiced"`, the
    // one region at the lab destination the census could not decide.
    //
    // ⚠ Its test could not see that, and the reason is worth keeping: it
    // asserted `silence.is_some()` was false, and `is_some()` cannot tell
    // *declares a name* from *declares nothing*. Asserting the smallest thing
    // is right; asserting the wrong smallest thing passes for both answers.
    //
    // So where this screen draws no bar, the toolbar's copy of the name IS the
    // stop that says it, and says so here.
    if !draws_own_app_bar() {
        return vec![
            AccessNode::new("lab.toolbar.title", AriaRole::Group)
                .with_name(format!("node lab: {}", spec::GRAPH_NAME)),
        ];
    }
    let running = state.running.get();
    vec![
        AccessNode::new("lab.appbar", AriaRole::Group)
            .with_name(format!("node lab: {}", spec::GRAPH_NAME)),
        AccessNode::new("lab.appbar.state", AriaRole::Status)
            .with_name(if running { "running" } else { "stopped" })
            .with_live(AccessLive::Polite),
    ]
}

/// The icon rail, through the framework's own navigation landmark.
///
/// ★★ [`navigation_link_nodes`] rather than a hand-rolled landmark: this is its
/// fourth consumer, and a rail that built its own would be the divergence that
/// rule exists to prevent. What this round added to the substrate is the
/// **reason** a destination is inert — the rail's whole design is to show later
/// scope as visible-and-locked, and "unavailable" with no reason is exactly the
/// one bit the floor's accessibility layer carries.
fn rail_access() -> Vec<AccessNode> {
    let reasons: Vec<Option<Unavailable>> = spec::RAIL
        .iter()
        .map(|(_, requirement)| requirement.map(Unavailable::reserved))
        .collect();
    let tags: Vec<String> = spec::RAIL
        .iter()
        .map(|(name, _)| format!("lab.rail.{name}"))
        .collect();
    let links: Vec<NavLink<'_>> = spec::RAIL
        .iter()
        .enumerate()
        .map(|(i, (name, _))| NavLink {
            tag: &tags[i],
            label: name,
            state: if reasons[i].is_some() {
                RadioState::Disabled
            } else {
                RadioState::Idle
            },
            current: *name == spec::RAIL_ACTIVE,
            focused: false,
            unavailable: reasons[i].as_ref(),
        })
        .collect();
    navigation_link_nodes("lab.rail", "sections", &links)
}

/// The palette: the pane, a button per role, the pin legend, and the
/// determinism switch.
///
/// ★ The transports are the pane's **value** rather than five nodes saying one
/// word each. Their chips are a colour key, and what a reader who cannot see the
/// key loses is the membership of the set — so the set is announced once, where
/// it can be read in one breath, and each chip declares itself part of it.
fn palette_access(state: &LabState) -> Vec<AccessNode> {
    // ★★★★★ R1887.1 — **a folded panel announces what it paints and no more.**
    //
    // R1887's own closing audit found this the moment the sweep reached a
    // folded palette: twelve regions came back as GHOSTS — announced by the
    // accessibility tree, painted by nothing. A reader who never sees the
    // drawing was being offered eight roles, three pin kinds and a switch that
    // are not on the screen, which is worse than not being told at all: the
    // press that follows the announcement lands on a strip.
    //
    // ⇒ ★ **an announcement is a claim about the paint**, and a panel with two
    // paint branches needs two announcement branches.
    if SidePanel::Palette.at(state).folded {
        return side_panel_access(state, SidePanel::Palette);
    }
    let on = state.discovery.get();
    let mut nodes = vec![
        AccessNode::new("lab.palette", AriaRole::Group)
            .with_name(spec::PANES[1].title)
            .with_value(AccessValue::Text(format!(
                "{} roles; transports {}",
                spec::ROLES.len(),
                spec::PROTOCOLS.join(", "),
            ))),
    ];
    nodes.extend(side_panel_access(state, SidePanel::Palette));
    for role in spec::ROLES {
        nodes.push(
            AccessNode::new(format!("lab.palette.role.{}", role.name), AriaRole::Button)
                .with_name(format!("add a {} — {}", role.name, role.gist)),
        );
    }
    // The pin legend states what a pin can DO, which is not a fact about
    // colour: a reader who never sees the drawing still needs the vocabulary
    // its announcements use.
    for (kind, meaning) in spec::PIN_LEGEND {
        nodes.push(
            AccessNode::new(format!("lab.palette.pin.{kind}"), AriaRole::Group)
                .with_name(format!("{kind} pin — {meaning}")),
        );
    }
    nodes.push(
        AccessNode::new("lab.palette.discovery", AriaRole::Switch)
            .with_name("graph determinism")
            .with_state(AccessState {
                checked: Some(on),
                ..AccessState::default()
            })
            .with_value(AccessValue::Bool(on))
            .with_described_by(discovery_caption_tag()),
    );
    // The caption the switch points at. It is PAINTED, so it gets a node with
    // the words it paints — a `described_by` naming a region a reader can also
    // walk onto is the ordinary shape, and the words come from one place so the
    // description and the ink cannot disagree.
    nodes.push(
        AccessNode::new(discovery_caption_tag(), AriaRole::Status).with_name(discovery_caption(on)),
    );
    nodes
}

/// The canvas: the surface itself, the cards, the host frames, the wires, and
/// the pins a link is drawn between.
fn canvas_access(state: &LabState) -> Vec<AccessNode> {
    let selection = state.selection.get();
    let mut nodes = vec![
        AccessNode::new("lab.canvas", AriaRole::Group)
            .with_name("canvas")
            // ★★ R1706 — a canvas whose frame gesture selects six cards at once
            // is multi-selectable, and saying so is what makes the per-card
            // `aria-selected="false"` audible rather than noise: an assistive
            // technology announces "not selected" only where a set is possible.
            .with_multiselectable()
            .with_value(AccessValue::Text(format!(
                "{} cards, {} links, zoom {}%",
                state.cards().len(),
                state.link_count(),
                state.zoom.get(),
            ))),
    ];
    for node in state.cards() {
        let name = state.name_of(node);
        let role = state.role_of(node).unwrap_or(Role::Peer);
        let (inbound, outbound) = state.degree(node);
        let (collapsed, disabled) = card_switches(state, node);
        let mut card = AccessNode::new(format!("lab.node.{name}"), AriaRole::Group)
            .with_name(name.clone())
            // ★★ R1706 — `aria-selected` is MEMBERSHIP, and which member LEADS
            // is a second fact this row also has to carry: with six cards
            // outlined and one inspector open, a reader told only "selected"
            // six times cannot tell which card the panel is about.
            .with_selected(selection.contains(&node))
            .with_state(AccessState {
                disabled,
                ..AccessState::default()
            })
            .with_expanded(!collapsed);
        // ★★★ `aria-current`, and NOT `aria-focused` — which is a correction
        // the framework made rather than a preference. The first draft spelled
        // the leader with [`AccessNode::with_focused`], and the assembler
        // silently cleared it on every card: R1518 derives that flag from the
        // focus target the shell actually granted, precisely so a binding
        // cannot claim a focus nobody gave it. It was right to. Focus is where
        // the keyboard is; the leader of a selection is *the current item
        // within a set of related items*, which is what `aria-current` is for
        // and what stays true while the keyboard is somewhere else entirely.
        if selection.is_active(&node) {
            card = card.with_current(AriaCurrent::True);
        }
        nodes.push(card.with_value(AccessValue::Text(format!(
            "{}, {inbound} inbound, {outbound} outbound",
            role.name()
        ))));
    }
    for (frame, name) in frames_of(state) {
        let gist = spec::FRAMES
            .iter()
            .find(|f| f.name == name)
            .map_or("", |f| f.gist);
        nodes.push(
            AccessNode::new(format!("lab.frame.{name}"), AriaRole::Group)
                // R1692 — the tab's own words, with the kind in front. The
                // caption declares itself this node's name, so what a reader
                // hears has to CONTAIN what a reader sees.
                .with_name(format!("host {}", frame_caption(&name, gist)))
                .with_value(AccessValue::Text(format!(
                    "{} cards",
                    members_of(state, frame).len()
                ))),
        );
    }
    nodes.extend(wire_access(state));
    nodes.extend(link_chrome_access(state));
    nodes
}

/// Every wire, drawn and reported, and every pin one can be drawn from.
fn wire_access(state: &LabState) -> Vec<AccessNode> {
    let mut nodes = Vec::new();
    let selected = state.selected_link.get();
    let doc = state.doc.borrow();
    let Some(tree) = doc.tree(ROOT) else {
        return nodes;
    };
    for link in tree.links() {
        let from = state.name_of(link.from.node);
        let to = state.name_of(link.to.node);
        nodes.push(
            AccessNode::new(format!("lab.link.{}", link.id.0), AriaRole::Group)
                .with_name(format!("link {from} to {to}"))
                .with_selected(selected == Some(LinkPick::Authored(link.id))),
        );
    }
    // ★ A reported link is NOT in the graph, and its announcement has to say so
    // — the drawing says it with a dash rhythm and a warning colour, which is
    // the half a reader never receives.
    for seen in doc.observations(ROOT) {
        let from = state.name_of(seen.from.node);
        let to = state.name_of(seen.to.node);
        nodes.push(
            AccessNode::new(format!("lab.observed.{from}.{to}"), AriaRole::Group)
                .with_name(format!("reported link {from} to {to}, not authored"))
                .with_selected(selected == Some(LinkPick::Observed(seen.from, seen.to))),
        );
    }
    for node in state.cards() {
        let name = state.name_of(node);
        let Some(role) = state.role_of(node) else {
            continue;
        };
        // ★★★★★ R1928 — **what a pin is CALLED comes from the model**, and only
        // what it is FOR is this screen's.
        //
        // Until this round both halves were one hand-written sentence here, so
        // the model called the output port `dial` and this file wrote the word
        // `dial` again beside it — two spellings of one fact, the shape R1926
        // found in the colour table. Worse, the accept run's ports are named
        // for the ADDRESS each one listens on (`Item::label`, since R1681), and
        // none of that reached a reader who cannot see the canvas: every accept
        // pin on every card announced the same six words.
        //
        // `Document::port_label` is the one resolution, so the name a reader
        // hears is the name the model resolved — and when the kind answers
        // `Silent` there is no name to hear, which is a different sentence
        // rather than a missing one.
        nodes.push(
            AccessNode::new(format!("lab.pin.{name}.dial"), AriaRole::Button).with_name(
                pin_announcement(
                    state,
                    node,
                    PortRef::output(0),
                    &name,
                    "dial pin",
                    "drag to author a link",
                ),
            ),
        );
        if role.accepts() {
            nodes.push(
                AccessNode::new(format!("lab.pin.{name}.accept"), AriaRole::Button).with_name(
                    pin_announcement(
                        state,
                        node,
                        PortRef::input(0),
                        &name,
                        "accept pin",
                        "drop a link here",
                    ),
                ),
            );
        }
        // ★★★★★ R1914 — a member pin a split put on the frame is a thing on
        // the frame, so it is announced. A pin that is drawn and not announced
        // is a pin a reader who does not look at pixels cannot know exists —
        // and the split is exactly the gesture that makes new ones appear.
        for (side, word) in [(Side::Output, "dial"), (Side::Input, "accept")] {
            for (path, port) in member_pins(state, node, side) {
                nodes.push(
                    AccessNode::new(
                        format!("lab.pin.{name}.{}", pin_word(side, &path)),
                        AriaRole::Button,
                    )
                    .with_name(format!("{name} {word} pin — its {} half", port.name)),
                );
            }
        }
    }
    // ★★★★★ R1916 — and the DESCRIPTION a reader is being shown, wired to the
    // mark it belongs to through `aria-describedby`.
    //
    // The substrate's `describedby_region` owns the gating, so the link is
    // present exactly while the region is — a dangling reference to an absent
    // node is an AT defect rather than a style choice. Before this round the
    // assembled tool published ZERO nodes of this role across six pages while
    // the framework had carried the widget since R695; what it did not have was
    // a way to say *that mark over there has a sentence*.
    //
    // ★★★★★ R1918 — and the four steps around that call are the substrate's
    // now (`described::announce_description`), because R1918 needed FOUR more
    // screens to perform them. Finding the anchor, replacing rather than
    // duplicating it, and synthesising one when the screen publishes none are
    // decisions with one owner.
    if let Some(shown) = pin_description_shown(state) {
        pinion_widget_paint::described::announce_description(
            &mut nodes,
            &shown.0,
            TOOLTIP_TAG,
            &shown.1,
        );
    }
    nodes
}

/// The tag the description region is painted and announced under.
const TOOLTIP_TAG: &str = "lab.tip";

/// ★★★★★ R1916 — the sentences this screen's PINS carry, by paint tag.
///
/// Built from `Document::port_tooltip`, which is the substrate's one
/// composition of the type's half and the port's own half — so the sentence a
/// reader sees on the canvas and the one an agent reads over the wire are the
/// same derivation rather than two spellings.
///
/// ⚠ Only the ports the node actually DRAWS. A hidden port has no mark for a
/// pointer to rest on, so describing it would put a sentence in the register
/// that nothing can ever show — and a census counting described marks would
/// then count marks that are not there.
fn pin_descriptions(state: &LabState) -> Descriptions {
    let mut described = Descriptions::new();
    let doc = state.doc.borrow();
    for node in state.cards() {
        let name = state.name_of(node);
        let Some(seen) = doc.visible_ports(ROOT, node) else {
            continue;
        };
        for side in [Side::Output, Side::Input] {
            let drawn = match side {
                Side::Output => &seen.outputs,
                Side::Input => &seen.inputs,
            };
            for (index, (path, _)) in doc.resolved_ports(ROOT, node, side).into_iter().enumerate() {
                let index = u32::try_from(index).unwrap_or(u32::MAX);
                if !drawn.contains(&index) {
                    continue;
                }
                if let Some(tip) = doc.port_tooltip(ROOT, node, side, &path) {
                    described.describe(
                        format!("lab.pin.{name}.{}", pin_word(side, &path)),
                        tip.sentence(),
                    );
                }
            }
        }
    }
    described
}

/// ★★★★★ R1923 — **what each card says about itself, and who said it.**
///
/// The source travels with every sentence, which is the half the reference
/// cannot express: its node tooltip hands back a bare string, so a client there
/// cannot tell a note somebody wrote on THIS node from what nodes of that sort
/// are. An agent reading this row can — and so can an editor deciding whether
/// there is anything to clear.
fn notes_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    let rows: Vec<serde_json::Value> = state
        .cards()
        .into_iter()
        .map(|node| {
            let said = doc.description(ROOT, node);
            serde_json::json!({
                "node": state.name_of(node),
                "sentence": said.as_ref().map(|d| d.sentence.clone()),
                "source": said.as_ref().map(|d| d.source.wire_word()),
            })
        })
        .collect();
    serde_json::json!({ "nodes": rows })
}

/// ★★★★★ R1922 — **what this screen's graph would accept**, body by body.
///
/// The framework's `Document::admits` answers one placement; this publishes the
/// answer for every body kind this crate owns, because an agent deciding WHAT
/// to place needs the row before it places anything. Same shape and same reason
/// as R1920's `editable`, one axis over: that one is *may I edit this node*,
/// this one is *may I put this here at all*.
///
/// ★ And the refusal is REAL on this screen rather than deferred to a tripwire:
/// the lab's canvas is the root tree, and the root is the one tree nothing
/// instantiates, so an interface end is refused here and the walk drives it.
fn accepts_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    // The bodies this crate owns, named the way the wire names things. A kind
    // body needs an application kind, so it is represented by one the palette
    // can actually supply — anything else would be asking about a body no
    // caller here could place.
    let probes: Vec<(&str, NodeBody<LabNode>)> = vec![
        ("frame", NodeBody::Frame),
        (
            "interface-input",
            NodeBody::Interface(pinion_node_graph::InterfaceSide::Input),
        ),
        (
            "interface-output",
            NodeBody::Interface(pinion_node_graph::InterfaceSide::Output),
        ),
        ("group-of-this-tree", NodeBody::Group(ROOT)),
    ];
    let rows: Vec<serde_json::Value> = probes
        .into_iter()
        .map(|(word, body)| {
            let asked = doc.admits(ROOT, &body);
            serde_json::json!({
                "body": word,
                "verdict": if asked.is_ok() { "allowed" } else { "refused" },
                "because": asked.err().map(|why| why.to_string()),
            })
        })
        .collect();
    serde_json::json!({ "bodies": rows })
}

/// ★★★★★ R1928 — **what each card calls its own ports**, and who chose each
/// name.
///
/// One row per port of every card, on both sides, with the resolved name and
/// its source. `name` is `null` for a port the kind answers
/// [`Silent`](pinion_node_graph::PortName::Silent) for — deliberately unlabelled, which is a different
/// row from one whose name happens to be short.
///
/// Published because the pin's spoken sentence is now DERIVED from this, and a
/// walk that could see only the sentence could not say whether it agreed with
/// the model or merely resembled it.
fn port_names_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for node in state.cards() {
        let card = state.name_of(node);
        for side in [Side::Output, Side::Input] {
            for (index, held) in doc.port_labels(ROOT, node, side).into_iter().enumerate() {
                rows.push(serde_json::json!({
                    "card": card,
                    "side": if side == Side::Output { "dial" } else { "accept" },
                    "index": index,
                    "name": held.text,
                    "source": match held.source {
                        NameSource::Kind => "kind",
                        NameSource::Item => "item",
                        NameSource::Node => "node",
                    },
                }));
            }
        }
    }
    serde_json::json!({ "ports": rows })
}

/// ★★★★★ R1927 — **what is wrong with each card**, from the two places that can
/// answer, side by side.
///
/// `said` is the FRAMEWORK's — `Document::warning`, the kind's own judgement
/// about this node in this graph, carried verbatim. `problem` and `blocks` are
/// this screen's walk, which folds that answer in among its own findings.
///
/// Both, because they answer different questions and a reader has to be able to
/// tell them apart: a card can have a problem this screen found and the model
/// knows nothing about, and the mark on the canvas is about the second column
/// while the census rows this round closes are about the first. Publishing only
/// one would make the walk that checks the mark unable to say which it checked.
fn wrong_wire(state: &Rc<LabState>) -> serde_json::Value {
    let troubled = troubled_cards(state);
    let rows: Vec<serde_json::Value> = state
        .cards()
        .into_iter()
        .map(|node| {
            let said = state
                .doc
                .borrow()
                .warning(ROOT, node)
                .map(|held| held.sentence);
            let worst = troubled.get(&node).copied();
            serde_json::json!({
                "card": state.name_of(node),
                "said": said,
                "problem": worst.is_some(),
                "blocks": worst.unwrap_or(false),
            })
        })
        .collect();
    serde_json::json!({ "cards": rows })
}

/// A [`Tint`] as the six hex digits every other colour on this wire is written
/// with.
fn hex_of(tint: Tint) -> String {
    format!("#{:02X}{:02X}{:02X}", tint.r, tint.g, tint.b)
}

/// ★★★★★ R1926 — **what colour every socket type is, and what colour every pin
/// on the canvas takes from it.**
///
/// Two halves because they are two questions the reference keeps apart for a
/// reason its own signatures record: `types` is asked with a TYPE and no port,
/// which is what a legend or a type picker needs, and `pins` is asked with a
/// port. Here the second is DERIVED from the first, so a client reading either
/// gets the same answer — which is the whole difference from a screen that
/// keeps its own colour table, as this one did until this round.
///
/// A composite type publishes its `members` too: the reference's *secondary
/// pin type colour* is the second of those, and it has no third.
fn inks_wire(state: &Rc<LabState>) -> serde_json::Value {
    let types: Vec<serde_json::Value> = graph::Endpoint::all()
        .into_iter()
        .map(|ty| {
            let held = type_palette::<graph::LabNode>(&ty);
            serde_json::json!({
                "type": ty.wire_word(),
                "ink": held.own().map(hex_of),
                "members": held.members().iter().map(|held| held.map(hex_of))
                    .collect::<Vec<_>>(),
                "silent": held.is_silent(),
            })
        })
        .collect();
    let doc = state.doc.borrow();
    let mut pins = Vec::new();
    for node in state.cards() {
        for side in [Side::Output, Side::Input] {
            for (path, port) in doc.resolved_ports(ROOT, node, side) {
                let held = palette_of::<graph::LabNode>(&port.flow);
                pins.push(serde_json::json!({
                    "pin": format!("{}.{}", state.name_of(node), pin_word(side, &path)),
                    "member": path.depth() > 0,
                    "ink": held.own().map(hex_of),
                }));
            }
        }
    }
    serde_json::json!({ "types": types, "pins": pins })
}

/// ★★★★★ R1925 — the agent half of [`sections_wire`]: add, fold and remove a
/// section of this graph's own face.
///
/// A section is named on the wire by its header and by an id inside the
/// framework, and this is the one place the two meet.
fn section_command(state: &Rc<LabState>, word: &str, rest: &str) -> Result<String, InvokeError> {
    let named = |doc: &Document<LabNode>, name: &str| {
        doc.tree(ROOT)
            .map(pinion_node_graph::Tree::interface)
            .and_then(|face| {
                face.sections()
                    .iter()
                    .find(|held| held.name() == name)
                    .map(pinion_node_graph::Section::id)
            })
    };
    let mut doc = state.doc.borrow_mut();
    match word {
        "add" => {
            if rest.is_empty() {
                return Err(InvokeError::rejected("a section needs a header"));
            }
            if named(&doc, rest).is_some() {
                return Err(InvokeError::rejected(format!(
                    "this face already has a section called {rest:?}, and a header \
                     is how the wire names one"
                )));
            }
            doc.add_section(ROOT, rest)
                .map_err(|why| InvokeError::rejected(why.to_string()))?;
            Ok(rest.to_owned())
        }
        "fold" | "remove" => {
            let (name, tail) = rest.split_once(',').unwrap_or((rest, ""));
            let (name, tail) = (name.trim(), tail.trim());
            let held = named(&doc, name).ok_or_else(|| {
                InvokeError::rejected(format!("this face has no section called {name:?}"))
            })?;
            if word == "remove" {
                doc.remove_section(ROOT, held)
                    .map_err(|why| InvokeError::rejected(why.to_string()))?;
                return Ok(name.to_owned());
            }
            let folded = match tail {
                "on" => true,
                "off" => false,
                other => {
                    return Err(InvokeError::rejected(format!("{other:?} is not on / off")));
                }
            };
            doc.set_section_folded(ROOT, held, folded)
                .map_err(|why| InvokeError::rejected(why.to_string()))?;
            Ok(format!("{name},{tail}"))
        }
        other => Err(InvokeError::rejected(format!(
            "{other:?} is not a section command; they are {}",
            SECTION_COMMANDS.join(" / ")
        ))),
    }
}

/// ★★★★★ R1925 — **the sections this graph's own face is gathered into**, and
/// the framework's answer when this screen asks for a section switch.
///
/// A definition's face — the ports an instance of this graph would show — has
/// no pixels on the reference mock-up, and this round did not invent any: what
/// is not on the canon is not drawn here. What it does do is publish the
/// register, so an agent that wants to arrange that face can, and so the
/// question *may this screen make a section switch* has an answer instead of a
/// silence.
///
/// ★ The answer is **no, with a reason**, and the reason is a fact about this
/// application rather than about the framework: a section switch carries the
/// taxonomy's own two-state type, and this screen's taxonomy is locators —
/// [`Endpoint`](crate::graph::Endpoint) has no such member, so
/// `NodeKind::switch_type` is `None` here. Publishing that is the difference
/// between a capability an agent can rule out and one it has to discover by
/// failing.
fn sections_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    let interface = doc.tree(ROOT).map(pinion_node_graph::Tree::interface);
    let sections: Vec<serde_json::Value> = interface
        .map(|face| {
            face.sections()
                .iter()
                .map(|held| {
                    serde_json::json!({
                        "name": held.name(),
                        "folded": held.folded(),
                        "members": held.members().iter().map(ToString::to_string)
                            .collect::<Vec<_>>(),
                        "switch": held.switch(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // ★ Asked of the framework rather than re-derived here, which is the
    // difference between one rule and two: the screen must not carry its own
    // copy of *when a section may be switched*. The section named is whichever
    // this face has, or the first id an empty face would hand out — and that
    // choice cannot change the answer, because the taxonomy check comes ahead
    // of the section lookup for exactly this reason.
    let named = interface
        .and_then(|face| face.sections().first().map(pinion_node_graph::Section::id))
        .unwrap_or(pinion_node_graph::SectionId(0));
    let asked = doc.may_new_section_switch(ROOT, named);
    let switchable = <LabNode as pinion_node_graph::NodeKind>::switch_type().is_some();
    serde_json::json!({
        "sections": sections,
        "ports": interface.map_or(0, |face| face.inputs().len()),
        "switchable": switchable,
        "because": asked.err().map(|why| serde_json::json!({
            "word": why.wire_word(),
            "sentence": why.to_string(),
        })),
    })
}

/// ★★★★★ R1924 — for the picked wire's consuming end, every card on this
/// canvas and whether it would take it.
///
/// One row per card and not only the ones that refuse, because "which of these
/// will take it" and "why will that one not" are the same question asked of
/// different cards, and a reader that had to infer the yes from an absence
/// could not tell a card that accepts from a card the screen forgot.
///
/// `carried` says whether the wire is in a hand right now. The rows are the
/// same either way — a question about a picked link does not need the pointer
/// to be down — which is what lets an agent ask it without miming a drag.
fn rewire_wire(state: &Rc<LabState>) -> serde_json::Value {
    let picked = state.selected_link.get().and_then(LinkPick::authored);
    let Some(link) = picked else {
        return serde_json::json!({
            "picked": serde_json::Value::Null,
            "carried": false,
            "cards": [],
        });
    };
    let cards: Vec<NodeId> = state
        .doc
        .borrow()
        .tree(ROOT)
        .map(|t| t.nodes().map(|n| n.id).collect())
        .unwrap_or_default();
    let rows: Vec<serde_json::Value> = cards
        .into_iter()
        .map(|card| {
            let asked = landing_for(state, link, card);
            serde_json::json!({
                "card": state.name_of(card),
                "verdict": asked.word(),
                "because": asked.because(),
                // What the canvas is LIGHTING, so a client can check the
                // picture against the rule rather than trusting that they agree.
                "lit": state.rewire_targets.borrow().contains(&card),
            })
        })
        .collect();
    serde_json::json!({
        "picked": link.0,
        "carried": matches!(state.drag.get(), Some(Drag::Rewire { .. })),
        "cards": rows,
    })
}

/// ★★★★★ R1921 — `#rrggbb`, or the word for having no colour at all.
///
/// The absence is a WORD and not an empty string, because an empty argument is
/// what a caller sends by mistake and "take the colour away" is a thing they
/// mean on purpose. Refusing anything else is what keeps `tints` readable: a
/// silently-ignored malformed colour would leave the card its old one and
/// answer as though it had changed.
fn parse_tint(raw: &str) -> Result<Option<Tint>, InvokeError> {
    if raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(InvokeError::rejected(format!(
            "{raw:?} is not #rrggbb or \"none\""
        )));
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
    Ok(Some(Tint::rgb(channel(0), channel(2), channel(4))))
}

/// ★★★★★ R1921 — the colour a person gave this card, if any.
fn card_tint(state: &LabState, node: NodeId) -> Option<Tint> {
    state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|host| host.node(node))
        .and_then(|held| held.appearance.tint)
}

/// ★★★★★ R1921 — every card's authored colour and the faces it derives.
///
/// The faces are published beside the authored value rather than left for a
/// client to re-derive, because re-deriving them is exactly the duplication
/// `Faces` exists to remove: a second implementation of the contrast rule
/// would be free to disagree with the one the screen paints with.
fn tints_wire(state: &Rc<LabState>) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = state
        .cards()
        .into_iter()
        .map(|node| {
            let tint = card_tint(state, node);
            let faces = tint.map(Faces::of);
            serde_json::json!({
                "node": state.name_of(node),
                "tint": tint.map(|t| format!("#{:02x}{:02x}{:02x}", t.r, t.g, t.b)),
                "faces": faces.map(|f| serde_json::json!({
                    "title": format!("#{:02x}{:02x}{:02x}", f.title.r, f.title.g, f.title.b),
                    "body": format!("#{:02x}{:02x}{:02x}", f.body.r, f.body.g, f.body.b),
                    "comment": format!(
                        "#{:02x}{:02x}{:02x}", f.comment.r, f.comment.g, f.comment.b
                    ),
                    "title_text": format!(
                        "#{:02x}{:02x}{:02x}",
                        f.title_text.r, f.title_text.g, f.title_text.b
                    ),
                })),
            })
        })
        .collect();
    serde_json::json!({ "nodes": rows })
}

/// ★★★★★ R1920 — **what this screen would let an agent do, asked before it
/// does it.**
///
/// The framework's `Document::may` answers one edit at a time; this publishes
/// the answer for every card at once, because an agent deciding WHICH card to
/// act on needs the whole row, not a question per card. Each entry is the same
/// decision the verb itself will make — `delete_node` and `rename` route
/// through `may` inside the crate — so this cannot drift from what the screen
/// will actually do. That is the point of it: an agent can plan a destructive
/// edit without performing one to find out whether it was allowed.
///
/// ⚠ Every card on THIS screen is deletable today, and the walk asserts that
/// rather than hiding it: the lab builds no subgraphs, so the one refusal that
/// exists (a tree's own interface end) has nothing here to refuse. That is
/// `debt-the-assembled-tool-cannot-open-a-subgraph`, and the assertion is a
/// tripwire that goes red the day it is repaid.
fn editable_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for node in state.cards() {
        let asked = doc.may(ROOT, Act::Delete(node));
        rows.push(serde_json::json!({
            "node": state.name_of(node),
            "delete": if asked.is_ok() { "allowed" } else { "refused" },
            // The refusal's own sentence, not a word this screen invents for
            // it — a reader is told what would be lost, which is the half a
            // bare "refused" cannot carry.
            "because": asked.err().map(|why| why.to_string()),
        }));
    }
    serde_json::json!({ "nodes": rows })
}

/// ★★★★★ R1919 — the search's hits as data, each saying **where it is**.
///
/// `through` and `depth` are the halves neither reference publishes: both
/// descend to the hit and select it, and a caller there cannot ask how far away
/// it is before going. `because` keeps the two kinds of hit apart — a node a
/// person named, and one matched by the word its kind describes itself with.
fn found_wire(state: &Rc<LabState>) -> serde_json::Value {
    let doc = state.doc.borrow();
    serde_json::json!({
        "needle": state.searching.get(),
        "hits": found(state)
            .into_iter()
            .map(|hit| {
                serde_json::json!({
                    "node": state.name_of(hit.node),
                    "shown": hit.shown,
                    "because": hit.because.wire_word(),
                    "depth": hit.at.depth(),
                    // ★ The way IN, by the names a reader sees — and it is the
                    // EditPath's OWN breadcrumb rather than a second walk over
                    // the same path, so what an agent is told and what an
                    // editor would show cannot be two different routes.
                    "through": hit.at.breadcrumb(&doc),
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// ★★★★★ R1918 — the register as data, with the mark it is drawn under.
///
/// Published so a gate reads what this screen describes rather than spelling
/// the register a second time and comparing the screen against a copy of
/// itself. The other five pages of the assembled tool publish the same shape.
fn described_wire(state: &Rc<LabState>) -> serde_json::Value {
    let described = pin_descriptions(state);
    serde_json::json!({
        "region": TOOLTIP_TAG,
        "marks": described
            .tags()
            .map(|tag| serde_json::json!({
                "tag": tag,
                "sentence": described.of(tag).unwrap_or_default(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// ★★★★★ R1916 — the pin description a reader is being shown right now, as
/// `(tag, sentence)`.
///
/// The screen does NOT carry `hovered == tag` itself: it hands the substrate
/// where the reader's attention is and is handed back what to show. That is the
/// shape the debt this closes named as the thing to avoid — this tree has paid
/// for screens hand-rolling one state before.
fn pin_description_shown(state: &LabState) -> Option<(String, String)> {
    let described = pin_descriptions(state);
    let cursor = state.cursor.get();
    // ★★★★★ Where the pointer LAST WAS is not where it IS. A description shown
    // under a pointer that has since left the window is a sentence hanging over
    // a window nobody is pointing at, which is the state R1916 measured on the
    // running shell before this guard existed.
    let hovered = match Hit::at(state, cursor.0, cursor.1) {
        Hit::Pin { node, side, at } if state.pointer_inside.get() => Some(format!(
            "lab.pin.{}.{}",
            state.name_of(node),
            pin_word(side, &at)
        )),
        _ => None,
    };
    // ★★★★★ The KEYBOARD reader's half, and it is not decoration. The behaviour
    // canon this screen reproduces has ZERO key bindings, so reproducing it
    // exactly is how a keyboard affordance quietly stops existing — the debt
    // that opened this said so in as many words. The focus is the shell's, read
    // from the substrate rather than mirrored into a field here.
    let focused = pinion_core::focus_state::focused();
    let shown = described.shown(&Resting {
        hovered: hovered.as_deref(),
        focused: focused.as_deref(),
        dismissed: false,
    })?;
    Some((shown.tag.to_owned(), shown.sentence.to_owned()))
}

/// The launch gate panel and the gesture hint: the two things that float over
/// the canvas.
///
/// ★ The findings are a **list**, because that is what a reader navigates them
/// as: one at a time, with a position and a count. Folding them into the panel's
/// value would make four problems one paragraph.
/// The fault-injection panel's voice.
///
/// ★ A **list**, for the same reason the pre-launch check is one: a reader walks
/// the injectable faults one at a time, with a position and a count. And the
/// list is built from `fault_rows`, which is `fault_injection::injectable` over
/// this node's declared settings — so a field added to the declaration gains a
/// voice at the same moment it gains a row, without this function being edited.
fn fault_access(state: &LabState) -> Vec<AccessNode> {
    let rows = fault_rows(state);
    let names = &spec::FAULT_PANEL;
    let mut panel = AccessNode::new(names.tag, AriaRole::List)
        .with_name(format!(
            "fault injection - {} from this node's settings",
            rows.len()
        ))
        .with_value(AccessValue::Text(fault_scope_note()));
    for n in 0..rows.len() {
        panel = panel.with_child(names.row(n));
    }
    let mut nodes = vec![panel];
    for (n, row) in rows.iter().enumerate() {
        nodes.push(
            AccessNode::new(names.row(n), AriaRole::ListItem)
                .with_name(format!(
                    "{} {}: {} - {}, admitted by {}",
                    row.key,
                    row.kind.wire(),
                    if row.blocks() {
                        "blocks launch"
                    } else {
                        "warning"
                    },
                    row.applies.map_or("form", Applies::wire),
                    row.admitted_by,
                ))
                .with_value(AccessValue::Text(row.value.clone()))
                .with_set_position(n, rows.len()),
        );
    }
    nodes
}

fn gate_access(state: &LabState) -> Vec<AccessNode> {
    let verdict = state.verdict();
    let (shown, hidden) = gate_shown(state);
    let mut gate = AccessNode::new("lab.gate", AriaRole::List)
        .with_name("pre-launch check")
        .with_value(AccessValue::Text(verdict.sentence()));
    for n in 0..shown.len() {
        gate = gate.with_child(format!("lab.gate.line.{n}"));
    }
    let mut nodes = vec![gate];
    for (n, (blocks, sentence)) in shown.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("lab.gate.line.{n}"), AriaRole::ListItem)
                .with_name(format!(
                    "{}: {sentence}",
                    if *blocks { "blocks launch" } else { "warning" }
                ))
                .with_set_position(n, shown.len()),
        );
    }
    if hidden > 0 {
        nodes.push(
            AccessNode::new("lab.gate.more", AriaRole::Status)
                .with_name(format!("{hidden} more the panel has no room for")),
        );
    }
    // The reset seats. `reset_seats` is the same list the hit test resolves
    // against, so a scope that gains an affordance gains a voice with it.
    for (scope, _) in reset_seats(state) {
        nodes.push(
            AccessNode::new(format!("lab.reset.{}", scope.wire()), AriaRole::Button)
                .with_name(format!("reset the {}", scope.wire())),
        );
    }
    nodes.push(
        AccessNode::new("lab.hint", AriaRole::Status).with_name(
            spec::GESTURES
                .iter()
                .map(|(g, what)| format!("{g} = {what}"))
                .collect::<Vec<_>>()
                .join("; "),
        ),
    );
    // ★★★★★ R1691 — **the toast, and the sweep is what found it.** It is the
    // one place several of this screen's operations report what they did (the
    // export, the script, the reset that put something back), so a reader who
    // is not told it appeared is not told the operation happened at all.
    //
    // It exists only after an act, so the census at boot could not see it and
    // no round had ever looked.
    //
    // ★★★★★ R1719 — the urgency was `Assertive`, flat, and R1691's reason for
    // that ("it is the answer to something the person just did") is an argument
    // for exactly the half of what this screen says that ISN'T. Measured by
    // driving it: `selected R-01` interrupted a screen reader. It comes off the
    // tone now, so a confirmation waits and a refusal cuts in, and neither is a
    // constant anybody can get half right.
    if let Some(said) = state.toast.showing() {
        nodes.push(
            AccessNode::new("lab.toast", AriaRole::Status)
                .with_name(said.sentence())
                .with_live(AccessLive::for_urgency(said.urgency())),
        );
    }
    nodes
}

/// The canvas toolbar: its seats, and the counts beside them.
///
/// ★★★★ R1687 — **the toolbar was silent, all of it.** The graph, the cards and
/// the form rows had announced themselves since R1651, and the strip holding the
/// launch control had no node at all — so a screen reader could be told the
/// gate's verdict (it is in the root group's value) and not be told there was a
/// button for it.
///
/// Found by adding two seats to that strip and asking whether they were named.
/// They were not, and neither was anything beside them. The list is therefore
/// the WHOLE cluster and not the two that round put there: a gate placed where
/// the last defect was finds only the last defect (R1684.2), and a seventh seat
/// added later fails the demo's roster check rather than going quiet.
fn toolbar_access(state: &LabState) -> Vec<AccessNode> {
    let mut nodes = vec![
        AccessNode::new("lab.toolbar", AriaRole::Toolbar).with_name("canvas"),
        AccessNode::new("lab.toolbar.meta", AriaRole::Status)
            .with_name(format!(
                "{} nodes, {} links",
                state.cards().len(),
                state.link_count()
            ))
            .with_live(AccessLive::Polite),
    ];
    for (tag, name) in toolbar_seat_names(state) {
        nodes.push(AccessNode::new(tag, AriaRole::Button).with_name(name));
    }
    nodes
}

/// The inspector: the pane, the chrome that says which card is selected, the
/// three acts on that card, and the form rows.
fn inspector_access(state: &LabState) -> Vec<AccessNode> {
    // ★★★★★ R1909 — **the inspector's half of R1887.1's repair**, and it is a
    // latent defect coming due rather than a new one.
    //
    // That round found twelve GHOSTS under a folded palette — regions the
    // accessibility tree announced and nothing painted — and wrote the rule
    // down: *an announcement is a claim about the paint, and a panel with two
    // paint branches needs two announcement branches.* It then built the
    // branch for ONE of the two panels. Nothing was wrong yet, because no
    // specification opened a panel folded and the sweep folded only the
    // palette; the inspector's missing branch was a defect with a date, and
    // R1909 — the round that declares this pane opens folded — is the date.
    //
    // Measured here: ghosts under a folded inspector — `lab.faults` and the
    // form's `add` chips among them — and no count, because the pane's own
    // `inspector_body_w()` PANICKED partway through the census. A form row's
    // rectangle is derived from a body that is not there, so the sweep never
    // reached the end of the list it was building.
    //
    // ⇒ ★★★★★ *a rule stated for a population and applied to one member of it
    // is not applied* — the same shape as the measurement this round overturned
    // one file over, at the other end of the same campaign.
    if SidePanel::Inspector.at(state).folded {
        let mut folded =
            vec![AccessNode::new("lab.inspector", AriaRole::Group).with_name(spec::PANES[3].title)];
        folded.extend(side_panel_access(state, SidePanel::Inspector));
        return folded;
    }
    let mut nodes =
        vec![AccessNode::new("lab.inspector", AriaRole::Group).with_name(spec::PANES[3].title)];
    nodes.extend(side_panel_access(state, SidePanel::Inspector));
    if let Some(node) = state.active_card() {
        let name = state.name_of(node);
        nodes.push(
            AccessNode::new("lab.inspector.id", AriaRole::Heading)
                .with_name(name.clone())
                .with_level(2),
        );
        nodes.push(
            AccessNode::new("lab.inspector.role", AriaRole::Status)
                .with_name(identity_caption(state, node)),
        );
        nodes.push(
            AccessNode::new("lab.inspector.degree", AriaRole::Status)
                .with_name(degree_caption(state, node)),
        );
        // ★★ R1706 — the same sentence the chip paints, from the same call.
        // A reader who cannot see six outlined cards has no other way to learn
        // that the panel is one of six, and the count is the only place the
        // screen says so in words.
        nodes.push(
            AccessNode::new("lab.inspector.selcount", AriaRole::Status)
                .with_name(selection_caption(state)),
        );
        // ★ The seat's name is the word it PAINTS, from the same call — a
        // toggle whose button reads "expand" and whose announcement reads
        // "collapse" is a control that does the opposite of what a reader is
        // told, and two derivations of one word is how that happens.
        let (collapsed, disabled) = card_switches(state, node);
        let pins_away = pins_are_away(state, node);
        for act in NodeAct::ALL {
            nodes.push(
                AccessNode::new(act.tag(), AriaRole::Button).with_name(format!(
                    "{} {name}",
                    act.word(collapsed, disabled, pins_away)
                )),
            );
        }
        nodes.push(
            AccessNode::new("lab.inspector.rename", AriaRole::Button)
                .with_name(format!("rename {name}")),
        );
    }
    // ★ The shut box, announced exactly while it is PAINTED. The open editor is
    // its own external with its own node, so a row here in both states would be
    // a name for a region that is not on screen — which the census calls a
    // ghost, and which it is what caught the condition being needed at all.
    if !matches!(
        state.editing.get(),
        Some(Editing::Name(_) | Editing::Key(_))
    ) {
        nodes.push(
            AccessNode::new("lab.inspector.name", AriaRole::TextInput)
                .with_name("a name for this card, or a configuration path to add"),
        );
    }
    nodes.push(
        AccessNode::new("lab.inspector.addkey", AriaRole::Button)
            .with_name("add a field by typing its key"),
    );
    nodes.push(
        AccessNode::new("lab.inspector.reach", AriaRole::Status)
            .with_name(reach_caption())
            .with_live(AccessLive::Polite),
    );
    if let Some(form) = selected_form(state) {
        // The note under the form: which keys reach a running node, or which
        // edits are waiting for a restart. It is a WARNING about what a person
        // just did, so it is live — a reader who edits a restart-scoped key and
        // is told nothing has been told the edit took effect.
        nodes.push(
            AccessNode::new("lab.inspector.note", AriaRole::Status)
                .with_name(restart_note(&form))
                .with_live(AccessLive::Polite),
        );
        let geometry = inspector_geometry(state);
        nodes.extend(row_access_nodes("lab.form", &form, &geometry));
    }
    nodes
}

/// Every pressable seat of the canvas toolbar, and what it announces as.
///
/// ★ The run seat's name carries the gate, because that is what the control
/// *does* right now: "run" and "run blocked" are two different offers and a
/// reader who is told only "run" has been told the button will start the graph.
/// It is the same sentence the label paints, from the same two facts.
///
/// ★ R1688 — a rendering of [`toolbar_seats`], not a second list.
fn toolbar_seat_names(state: &LabState) -> Vec<(&'static str, String)> {
    toolbar_seats(state)
        .into_iter()
        .map(|seat| (seat.tag, seat.name))
        .collect()
}

impl WidgetView for NodeLabView {
    type Renderer = HelloNodeLabRenderer;

    /// ★★★ R1684 — **a press inside the open box puts the caret where the
    /// pointer landed**, through the framework's own hit test.
    ///
    /// R1683 wrote that this happened already, because the field is a real
    /// external and an external owns its rectangle. **Measured this round, and
    /// it was not true**: every press on this screen is routed to the ONE root
    /// external that does the screen's own hit test — that is R1655's
    /// invariant, and the field's external is a focus owner and a keystroke
    /// sink, not a second pointer target. Removing the arm that makes the
    /// screen stand aside inside the open box turned a typed `a9` back into
    /// the row's stored `a1`, which is the measurement; and nothing in process
    /// could have asked, because there is no external there to compete with.
    ///
    /// So the screen forwards deliberately, and without these two hooks the
    /// box could be typed into and never clicked into — no caret placement, no
    /// selection sweep, on the only text entry the screen has.
    ///
    /// `hit_tag` is this screen's own root everywhere, so the question the
    /// sibling text-field bindings settle with a tag comparison is settled
    /// here by the geometry the paint published — the same rectangle
    /// `Hit::at` stands aside for, so the two cannot disagree about where
    /// the box is.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        _hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        let byte = field_byte_at(state.0, scene, focused, x, y)?;
        let edit = use_text_edit_state(EDIT_TAG);
        if extend {
            let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
            edit.set_selection(anchor, byte);
            Some(anchor)
        } else {
            edit.set_caret(byte);
            Some(byte)
        }
    }

    /// A drag inside the box sweeps a selection, from the byte the press
    /// pinned (R1684).
    ///
    /// The other half of the same hit test: without it a press positions the
    /// caret and a drag from it selects nothing, which is a box that behaves
    /// like a text field until somebody tries to select a word in it.
    fn select_drag_to_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(byte) = field_byte_at(state.0, scene, focused, x, y) else {
            return false;
        };
        let edit = use_text_edit_state(EDIT_TAG);
        let before = (edit.caret(), edit.selection_anchor());
        edit.set_selection(anchor, byte);
        before != (edit.caret(), edit.selection_anchor())
    }

    /// The window OPENS at the design size — the one the specification's
    /// rectangles were measured against — and can be dragged to any size from
    /// there, down to the floor `SHRINK` declares.
    ///
    /// ★ R1654 — it was `Fixed`, which pins the OS-resize FLOOR at the open
    /// size: the window could be enlarged and never shrunk. Together with the
    /// pane rectangles being constants, that made the screen the one size it
    /// was written at. Both halves had to move — a layout that follows the
    /// window is no use if the window cannot be resized.
    ///
    /// ★★★★★ R1712 — and the floor is no longer `MIN_W` x `MIN_H`. It is
    /// derived from `SHRINK`, the same value `window_size` clamps against,
    /// so this binding has nowhere to write a second minimum.
    ///
    /// ⚠ **R1821 — "the same value" needs one more word now.** `window_size`
    /// clamps against `comfortable_size`, which is the floor evaluated for the
    /// chrome this screen is actually drawing rather than for the chrome the
    /// window policy assumes. The two are equal wherever this binding applies,
    /// because a screen with a window of its own draws all of its own chrome and
    /// the two calls are the same call — so the sentence above is still true
    /// HERE, and would stop being true if it were read as a claim about the
    /// mounted screen. Which is the point: this binding is the window's, and a
    /// mounted page has no window to floor.
    ///
    /// ⚠ R1821 wrote that as *`SHRINK`'s comfortable width less whatever chrome
    /// the host draws*, and R1822 removed the subtraction it described — one
    /// round later, because the same form on the height axis over-charged a
    /// mounted page for rail seats it does not have.
    ///
    /// ★★★★★ R1714 — and what the window does below that floor is no longer to
    /// clip. `SHRINK` declares a PAN, so the framework wraps this screen in a
    /// viewport onto its own layout and the app bar's right end and the
    /// inspector are one gesture away rather than gone. That is why there is no
    /// list of what the band gives up any more: it gives nothing up.
    ///
    /// ★ R1713 re-measured the band with a predicate that can see a mark inside
    /// a sliced pane: **24 pixels**, not 119 and not 30. See `FLOOR_W` for the
    /// three answers and why the first two were wrong.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }

    /// ★★★★★ R1861 — the gesture hint, which is the one thing this screen puts
    /// where a host's floating overlay lands.
    ///
    /// **Derived from `hint_rect` rather than written down**, so the strip
    /// cannot move without this moving with it — the defect a declaration beside
    /// a painter always eventually has. Offset into the region because the
    /// strip's own rectangle is in this screen's coordinates and the host asks
    /// in the window's.
    ///
    /// Measured before this existed: the host's toast covered the top 6 pixels
    /// of the run inside this strip, and a reader reported it.
    fn keeps_clear(region: Rect) -> Option<Rect> {
        let hint = hint_rect();
        Some(Rect::new(
            region.x + hint.x,
            region.y + hint.y,
            hint.w,
            hint.h,
        ))
    }

    /// ★★★★★ R1742 — the verdict this screen has always computed, answered
    /// where the application it is a page of can reach it.
    ///
    /// R1732 wrote `docs/analyzer-inspector-spec.json` and compared the painted
    /// inspector against it inside a unit test of this binary. R1738 then
    /// measured that this was the one section of the assembled tool that had a
    /// written specification and published no verdict about it — not failing a
    /// check, absent from the population. See `judge` for the decision that
    /// made publishing possible: what a screen says about a surface a session
    /// has not put on screen.
    fn conformance() -> Option<pinion_core::conformance::DocumentReport> {
        Some(judge::conformance())
    }

    /// ★★★★★ R1808 — **two, and the reason is this screen's specification, not
    /// its size.**
    ///
    /// `docs/analyzer-inspector-spec.json` names an `enum_row` surface and an
    /// `enum_roster` surface, and the roster **is** that row's open state: the
    /// row is specified with its roster shut, because a roster standing over it
    /// is a part of the row that is not always there, and the roster is
    /// specified open, because a shut one has no parts at all. So the two
    /// exclude each other and no frame can carry both.
    ///
    /// R1732 already knew this — its own gate reads one surface, opens the
    /// roster, and reads the other. What it could not do was tell a HOST, so an
    /// assembled application walking its sections reported this one as never
    /// reproducing its specification and was right to. This is that knowledge
    /// moved to where a host can ask for it.
    fn poses() -> usize {
        2
    }

    /// Pose 0 selects a card that carries the enum row, with the roster shut;
    /// pose 1 opens that row's roster over it.
    ///
    /// Through the same state the pointer path moves, so a pose cannot put the
    /// screen somewhere a reader could not reach.
    /// ★ It **refuses loudly** when it cannot reach the state, rather than
    /// leaving the screen where it was. A pose that quietly does nothing looks
    /// exactly like a screen that failed to reproduce its specification, and
    /// the round that built this spent two runs on that ambiguity.
    ///
    /// ⚠ It poses through this screen's **state**, where R1732's own gate
    /// drives the same two states through **presses** at painted coordinates.
    /// That is a deliberate difference and not a duplication: R1732 is asserting
    /// that a hand can get there, and this is asserting what the screen looks
    /// like once it has. Posing by pointer would make every host that mounts
    /// this screen depend on where its chips are drawn.
    fn pose(nth: usize) {
        let state = use_lab_state();
        // Select the card AND put the specified row on it. The row is not there
        // by default: it is one of the form's `addable` fields, offered as a
        // palette chip, so a pose that only selected would leave the surface the
        // specification is written against off the screen — which is what the
        // walk reported before this was measured.
        let carry_the_row = || {
            let node = state
                .node_of(POSE_CARD)
                .unwrap_or_else(|| panic!("the opening graph has no `{POSE_CARD}` to select"));
            state.selection.set(Selection::one(node));
            // Through the same function the press arm calls, so a pose cannot
            // reach a state a reader could not.
            let _ = amend(&state, node, |form| form.add(spec::ENUM_KEY));
        };
        match nth {
            0 => {
                state.picking.set(None);
                carry_the_row();
            }
            1 => {
                carry_the_row();
                open_roster(&state, spec::ENUM_KEY);
                assert!(
                    state.picking.get().is_some(),
                    "pose 1 must open the `{}` roster and it did not",
                    spec::ENUM_KEY
                );
            }
            _ => {}
        }
    }
}

/// Run the lab as an application of its own.
///
/// ★ R1724 — the four lines the standalone binary is, kept here so the binary
/// and the mounted page name the same binding. `src/main.rs` calls this.
pub fn run() {
    pinion_shell::run::<NodeLabView>();
}

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;
