//! ★★★★★ R1724 §5.16 §5.38 §5.40 §2 #7 — **a screen is a binding you can
//! arrive at.**
//!
//! # What was missing, measured
//!
//! The behaviour reference this project's analysis tool is modelled on is
//! **one application**: a shell with an app bar and a navigation rail, and the
//! rail switches between its sections. One class, one `view` field, and every
//! section shares the shell's palette, its floating panels, its presets and its
//! capture toggle.
//!
//! This tree assembles the same tool as **three executables**. Measured at this
//! round's close with `find examples/<pkg>/src -name '*.rs' | xargs wc -l` —
//! the command is quoted because a number in prose starts going stale the
//! moment it is written, and the three that were here first were taken
//! mid-round and were all wrong by the end: `hello-analyzer-shell` 13,854
//! lines, `hello-node-lab` 20,655, `hello-packet-view` 7,035 — and the shell's
//! own
//! rail says so out loud, because three of its seven seats are
//! [`Unavailable::elsewhere`](pinion_core::availability::Unavailable::elsewhere):
//! *built, shipping, and not here*. That arm exists because a destination in
//! this tree can be finished and still not be reachable, which is a sentence no
//! single-application product ever has to say.
//!
//! [`Destinations`](pinion_core::widgets::destination::Destinations) and
//! `pinion_widget_paint::pages::view_page_region` gave the model and the paint
//! halves of navigating in R1695. What neither gave is the half that makes the
//! seats openable: **what a destination's page is made of**, when the page is a
//! whole binding with its own externals, its own keymap, its own accessibility
//! tree and its own text caret.
//!
//! # The shape
//!
//! [`Screen`] is that: the dispatchable surface of a
//! [`WidgetView`](pinion_shell::WidgetView) with the window-level half removed,
//! as `&self` methods so a roster can hold several. [`Mount`] implements it for
//! any existing binding, so a screen is not a new kind of thing an author has
//! to write — it is the thing they already wrote, placed. [`ScreenRoster`]
//! pairs the destination roster with the screens behind its keys.
//!
//! ## No default methods, on purpose
//!
//! [`Screen`] declares every hook and defaults none of them. A hook added here
//! therefore stops [`Mount`] compiling until it forwards it, which is the only
//! mechanical guarantee available that mounting a binding does not silently
//! drop a behaviour the binding has. The direction that guarantee does *not*
//! cover — a hook added to `WidgetCore` / `WidgetA11y` / `WidgetView` and never
//! mirrored here — is covered by [`coverage`], a census that reads those three
//! traits' own source and fails when one of their hooks is neither mirrored nor
//! pinned as window-level with a reason.
//!
//! ## Against the reference toolkit's paged container
//!
//! Measured by building a probe against 6.11.1 and running it, rather than by
//! reading about it. R1695 measured *arriving*; these are the rows that only
//! appear once a page is a whole screen rather than a panel.
//!
//! | question | there | here |
//! |---|---|---|
//! | mounting an existing application window as a page | it stops being a window; its title survives and **is shown nowhere**; its status bar goes invisible — all silently | the binding is unchanged and the host publishes its title |
//! | the page that is not current, sent a press, a key and a wheel | **counted all three** | its externals are not in the state scene |
//! | the page that is not current, in the accessibility tree | reachable, **and its text field with it**, marked `invisible` | not built, so not in the tree |
//! | a page's own floating window when you navigate away | **left on screen** | the host's window set is the current screen's |
//! | state across leaving and returning | retained | retained — [`Mount`] keeps the latch and the binding keeps its owner scope |
//! | which page is current, published | the container's accessible value is empty | [`Destinations::wire`](pinion_core::widgets::destination::Destinations::wire) |
//! | arriving at a page that cannot be entered | arrives anyway | [`Detour`](pinion_core::widgets::destination::Detour), carrying the reason |
//!
//! The fourth row is the one that changes what an application can be. A section
//! that tears a panel off owns a window; leaving that section and finding its
//! panel still floating over a different section is not a small blemish, it is
//! the shell losing track of what it is showing. Here a screen's window set is
//! [`Screen::windows`], and the host's window set is the current screen's — so
//! leaving takes the panels with it and returning brings them back.
//!

pub mod conformance;
pub mod coverage;
pub mod journey;
pub mod layering;
mod mount;
mod roster;
pub mod tour;

use std::rc::Rc;

use pinion_a11y::{AccessAction, AccessFocus, AccessNode};
use pinion_core::command::Command;
use pinion_core::event::WheelDelta;
use pinion_core::input::{CompositionEvent, KeyPress, Modifiers};
use pinion_core::intent::Intent;
use pinion_core::reactive::Signal;
use pinion_core::scene::Rect;
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::{Frame, Scene};
use pinion_shell::{WindowPolicy, WindowSpec};

pub use conformance::{ApplicationConformance, SectionJudge, SectionRow, SectionStanding, Showing};
pub use journey::{JourneyConformance, JourneySection, JourneyStanding, SurfaceVisit};
pub use mount::Mount;
pub use roster::{RosterDefect, ScreenRoster, ScreenState};
pub use tour::{Tour, TourReport};

/// One destination's page, when the page is a whole binding.
///
/// Every method mirrors a hook of [`WidgetCore`](pinion_core::WidgetCore),
/// [`WidgetA11y`](pinion_a11y::WidgetA11y) or
/// [`WidgetView`](pinion_shell::WidgetView) that a *screen* can answer.
/// Window-level hooks — the renderer, the window's size strategy and shrink
/// policy, whether the application quits with its last window — are the host's
/// and are absent here by decision, recorded in [`coverage::WINDOW_LEVEL`].
///
/// # The two-phase state protocol
///
/// [`Screen::latch`] reads the screen's projection out of the state scene and
/// parks it; [`Screen::view`] and the `&self` hooks below paint and answer from
/// what was parked. This is not a shortcut around
/// [`WidgetCore::read_state`](pinion_core::WidgetCore::read_state) — it *is*
/// that contract, split across two calls because the host's own
/// `read_state`/`view` pair is where the scene stops being reachable: the
/// framework hands `view` a state and a frame, and a state erased across a
/// roster of differently-typed screens cannot travel in a `Copy` associated
/// type. The host calls `latch` before every frame, and [`ScreenRoster`] is the
/// only caller, so the ordering is a property of this crate rather than a rule
/// an author has to keep.
///
/// # Implementors
///
/// [`Mount`] for an existing binding, which is the expected route. Implementing
/// it directly is for a page that has no binding of its own and wants a
/// screen's *behaviour* — its own hit test, keymap, windows and accessibility
/// tree — which is why the trait is public.
///
/// ★★★★★ R1761 — it is **not** what a host's inline page needs in order to be
/// JUDGED, and this sentence said it was. A host paints a section's chrome
/// beside the page region rather than in it (measured: a layout bar above and a
/// palette panel to the right of a 1096×802 region), and a screen judges what it
/// paints — so the route recorded here would have produced a verdict that could
/// not reach a quarter of its own section. Publishing a verdict for a page the
/// host draws is [`SectionJudge`], registered through
/// [`ScreenRoster::judging`], which grants nothing else.
pub trait Screen {
    // --- identity -----------------------------------------------------------

    /// The screen's paint-root tag, and the tag its externals are keyed by.
    fn tag(&self) -> &'static str;

    /// What a reader calls the screen. The host publishes it as the window's
    /// title while this screen is the one showing — the row the reference
    /// toolkit keeps and shows nowhere.
    fn title(&self) -> &'static str;

    // --- specification ------------------------------------------------------

    /// ★★★★★ R1738 — the written specification this screen answers to, and how
    /// much of it the build reproduces, or `None` when it answers to none.
    ///
    /// **No default, deliberately, while the binding hook it mirrors has one.**
    /// The asymmetry is the decision: a hook required of every
    /// [`WidgetView`](pinion_shell::WidgetView) in this tree would be 225 edits
    /// that say nothing, while a screen written *directly* against this trait
    /// is a host's own inline page — the one place where "is this section
    /// judged" is a live question somebody should have to answer out loud.
    ///
    /// See [`conformance`] for what a host does with it and for the
    /// measurement that forced it.
    ///
    /// ★ R1742 — a report may say a surface is **not on screen** rather than
    /// absent, so a screen whose surfaces a session builds can answer honestly.
    /// It costs the report nothing: an away surface reproduces 0 and does not
    /// reconcile, so a section cannot pass by drawing less of itself.
    fn conformance(&self) -> Option<pinion_core::conformance::DocumentReport>;

    /// ★★★★★ R1808 — how many frames this screen needs to show all of what its
    /// specification describes, and how to put it into each.
    ///
    /// Defaulted here, unlike [`conformance`](Self::conformance) above, and for
    /// the opposite half of that decision's reason: *is this section judged* is
    /// a question a host's inline page must answer out loud, while *do my
    /// surfaces exclude each other* is answered `no` correctly for almost
    /// everything, and a required hook would be that many sites writing `1`.
    ///
    /// See [`WidgetView::poses`](pinion_shell::WidgetView::poses) for the
    /// measured case that forced it.
    fn poses(&self) -> usize {
        1
    }

    /// Put this screen into pose `nth`, counted from zero.
    fn pose(&self, nth: usize) {
        let _ = nth;
    }

    // --- state --------------------------------------------------------------

    /// Read this screen's projection out of `state_scene` and park it for the
    /// hooks below, returning a revision that changes exactly when the parked
    /// value does.
    ///
    /// The revision is what lets a host whose own state is otherwise constant
    /// still repaint when a mounted screen's state moves.
    fn latch(&self, state_scene: &Scene) -> u64;

    /// The parked projection as a line of log, for the same reason
    /// [`WidgetCore::fmt_state_log`](pinion_core::WidgetCore::fmt_state_log)
    /// exists: a host's own state says nothing about the screen it is showing.
    fn fmt_state_log(&self) -> String;

    // --- surfaces -----------------------------------------------------------

    /// Every external this screen needs live while it is the one showing — its
    /// primary surface first, then its extras.
    ///
    /// Flattened into one list because a host has no primary surface of its
    /// own: the surfaces of an application assembled from screens all belong to
    /// screens, and which one is "primary" is a fact about a binding that fills
    /// a window rather than about a page.
    fn externals(&self) -> Vec<ExtraExternal>;

    /// Rebuild whatever the screen recomputes once per frame.
    fn reconcile_frame(&self);

    // --- paint --------------------------------------------------------------

    /// The screen's scene, laid out in the extent the host granted.
    ///
    /// Reached only through [`ScreenRoster::page_scene`], which is what makes
    /// the grant ([`with_surface_extent`](pinion_core::external::with_surface_extent))
    /// structural: a page cannot be painted without the extent it was placed
    /// in being stated, so its hit test and its paint read one rectangle.
    fn view(&self, frame: &Frame) -> Scene;

    /// ★★★★★ What this screen needs, and what it concedes when it does not get
    /// it — the same declaration a binding makes about its own window.
    ///
    /// **Not a window-level fact, and the first mount is what proved it.** See
    /// [`coverage::WINDOW_LEVEL`] for the measurement: pinned as the window's,
    /// a screen placed in a region smaller than its layout minimum painted 51
    /// regions outside that rectangle and its inspector off the screen — while
    /// the screen itself had already declared
    /// [`Recourse::Pan`](pinion_core::shrink::Recourse::Pan) and nothing was
    /// listening. A region that shows a screen owes it the recourse it
    /// declared, exactly as a window does.
    fn shrink_policy(&self) -> Option<ShrinkPolicy>;

    /// ★★★★★ R1861 — **the part of `region` this screen has content in that a
    /// host's floating overlay must not cover.**
    ///
    /// The mirror of [`HostChrome`](pinion_core::chrome::HostChrome): that says
    /// what the place already provides, so the guest can leave it out; this says
    /// what the guest already occupies, so the place can put its overlay
    /// somewhere else. Both are facts about *the seam*, and until this existed
    /// only one direction of it could be stated.
    ///
    /// `None` for a screen with nothing an overlay would spoil — which is a
    /// declaration and not a default, exactly as `shrink_policy`'s `None` is.
    /// A screen that answers wrongly is caught by the paint rather than trusted:
    /// see `pinion_screen::layering::host_marks_over_guest_text`, which asks
    /// the frame whether anything of the guest's is covered.
    ///
    /// Measured on the analysis tool before this existed, at its shipping size:
    /// the host's toast covered the top 6 pixels of the node lab's gesture hint
    /// and the whole of two of the capture viewer's lane readouts.
    fn keeps_clear(&self, region: Rect) -> Option<Rect>;

    /// The screen's scene for a named window of its own.
    fn view_for_window(&self, window_id: &str, frame: &Frame) -> Scene;

    /// The windows this screen owns while it is showing.
    ///
    /// `Vec::new()` for a screen that lives entirely in the host's window,
    /// which is the ordinary case. A screen that tears panels off answers them
    /// here, and they leave with it.
    fn windows(&self) -> Vec<WindowSpec>;

    /// The reactive window set, when the screen's windows change at runtime.
    fn windows_signal(&self) -> Option<Rc<Signal<Vec<WindowSpec>>>>;

    /// The chrome and resize policy for one of the screen's windows.
    fn window_policy(&self, window_id: &str) -> WindowPolicy;

    /// Whether the screen refuses to let one of its windows close.
    fn window_close_requested(&self, window_id: &str) -> bool;

    // --- keyboard -----------------------------------------------------------

    /// The symbolic event name a key chord means to this screen, if any.
    ///
    /// A name rather than the binding's typed `Event`, which cannot cross a
    /// roster of differently-typed screens — and a name is what the wire and
    /// the state machine both consume anyway
    /// ([`WidgetCore::event_name`](pinion_core::WidgetCore::event_name) is the
    /// step every typed event takes next).
    fn keybinding(&self, key: &str) -> Option<&'static str>;

    /// Handle a key chord.
    fn apply_key(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool;

    /// Handle a key chord that may be an auto-repeat.
    fn apply_key_repeat(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
        repeat: bool,
    ) -> bool;

    /// Handle a key press with its full press record.
    fn apply_key_press(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        press: &KeyPress<'_>,
    ) -> bool;

    /// Forward a key to the focused external through the introspect channel.
    fn forward_key_to_external(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
    ) -> bool;

    /// Handle an IME composition event.
    fn apply_composition(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        event: &CompositionEvent,
    ) -> bool;

    // --- pointer ------------------------------------------------------------

    /// Handle a middle click.
    fn apply_middle_click(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        modifiers: Modifiers,
    ) -> bool;

    /// Handle a secondary (context) click at a window-local point.
    fn apply_secondary_click(&self, state_scene: &mut Scene, x: f32, y: f32) -> bool;

    /// Handle a wheel notch over `cursor`.
    fn apply_wheel(
        &self,
        paint_scene: &Scene,
        cursor: (f64, f64),
        delta: WheelDelta,
        modifiers: Modifiers,
    ) -> bool;

    /// Place the text caret for a press.
    fn position_caret_for_point(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
        hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize>;

    /// Extend a selection to a dragged-to point.
    fn select_drag_to_point(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool;

    /// The drop preview for a dock panel dragged over one of this screen's
    /// targets.
    fn dock_drop_preview(
        &self,
        source_panel: &str,
        target_tag: &str,
        panel_rect: Rect,
        x_rel: f32,
        y_rel: f32,
    ) -> Option<Scene>;

    /// The drag image for a label this screen is dragging.
    fn drag_image_style(&self, label: &str) -> Option<pinion_overlay::DragImageStyle>;

    // --- files --------------------------------------------------------------

    /// A file is hovering over one of this screen's windows.
    fn on_file_hover(&self, window_id: &str, path: &str) -> bool;

    /// The hover left without a drop.
    fn on_file_hover_cancel(&self, window_id: &str) -> bool;

    /// A file was dropped on one of this screen's windows.
    fn on_file_drop(&self, window_id: &str, path: &str) -> bool;

    // --- accessibility ------------------------------------------------------

    /// The screen's accessibility tree.
    fn access_node(&self, focused: Option<&str>) -> Vec<AccessNode>;

    /// The screen's accessibility tree for one of its own windows.
    fn access_node_for_window(&self, window_id: &str, focused: Option<&str>) -> Vec<AccessNode>;

    /// Where the cursor rests inside the focused composite.
    fn access_focus_target(&self, focused: Option<&str>) -> Option<AccessFocus>;

    /// An assistive technology's action on a composite child.
    fn access_child_invoke(
        &self,
        state_scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool;

    /// The focus ring this screen wants around `focused_tag`, or `None` to
    /// draw none.
    fn focus_ring_style(&self, focused_tag: &str) -> Option<pinion_overlay::FocusRingStyle>;

    /// Where the IME candidate window belongs.
    fn ime_caret_rect(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
    ) -> Option<pinion_text::CaretRect>;

    // --- intents ------------------------------------------------------------

    /// Turn an intent into the commands this screen wants dispatched.
    fn update(&self, intent: &Intent) -> Vec<Command>;
}
