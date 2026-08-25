//! R51.127 §5.41 — substrate-level test fixtures shared across the
//! `pinion-runtime` and `pinion-tui` test suites. Gated behind the
//! `test-fixtures` feature so the symbols never reach a production
//! binary, while still letting downstream `#[cfg(test)]` modules
//! re-export the fixtures through a `dev-dependencies` feature flag.
//!
//! The fixtures intentionally implement [`WidgetCore`] only — the
//! per-backend a11y / view-trait impls (e.g. `WidgetA11y`,
//! `WidgetViewTui`) stay in each backend's test module so the
//! `pinion-core` crate keeps its `pinion-a11y` / `pinion-tui` dep
//! direction empty (cycle invariant per [[r47-class-incident-
//! prevention]]).
//!
//! R55.G.22 §5.49 — also hosts
//! [`assert_widget_view_carries_tag`], the framework-level regression
//! primitive for the [[composite-paint-root-tag-convention]] (R55.G.17
//! §5.49). Nine widget example bindings carry an identical inline
//! assertion shape since R55.G.17 / G.18 / G.20; the helper extracts
//! that body into a single canonical entry point so a future widget
//! author pins the convention with one trait-bound call site, and so
//! the framework owns one place to evolve the assertion's error
//! message / hook list as the AT bounds attach contract grows.

// R1672 §5.32 §5.45 — the ink gate three screens run. Its own module because
// it is a HARNESS vocabulary rather than a widget stand-in: the check is
// `containment::escapes` and what a screen has to supply is the metric.
pub mod screen_ink;

// R1718 §5.12 — the gate over what a type SAYS. Its own module for the same
// reason `screen_ink` has one: it is a harness vocabulary rather than a widget
// stand-in, and the thing a caller supplies is the driving.
pub mod speech;

// R1774 §5.32 §5.45 — does the sweep reach BOTH sides of every clamp a screen
// has. Its own module for the reason the three around it have one: the rule is
// the framework's and the observables are the screen's. Screen C of the
// analysis tool carried this shape alone since R1669; the debt that recorded it
// observed the other two screens have guards nobody asks about.
pub mod clamp;

// R1731 §5.32 §5.40 — reading a specified surface back out of a PAINTED scene,
// for the same reason the two above are here. The second screen to compare its
// surfaces with a written specification would otherwise have carried a verbatim
// copy of the walk and the reading-order rule, and two screens reading a roster
// differently would disagree about the same defect.
pub mod surface;

// R1819 §5.32 §5.40 — every gesture a screen ADVERTISES does something, over a
// population that is never empty. Its own module for the reason the four above
// have one, and for a sharper one: this gate already existed TWICE and the two
// copies had drifted, while the third screen that needed it had none at all —
// so the honest next step was a third copy of a rule that was already differing
// from itself.
pub mod advertised;

// R1836 §5.32 §5.45 — a sweep's STRIDE against the window it must not step
// over, which is the half R1774 left. That module asks whether a swept state
// REACHED both sides of a guard; this one asks whether the sweep could have
// reached them at all. R1704 measured the difference: a 90 px stride across a
// 26 px window let the counterfactual for the WRONG spelling pass twice, and
// the repair stayed a comment at one call site.
pub mod sweep;

use std::borrow::Cow;

use crate::Frame;
use crate::cell_metric::CellMetric;
use crate::command::Command;
use crate::external::{External, IntrospectValue, StubExternal};
use crate::external::{InterveneError, InvokeError};
use crate::intent::Intent;
use crate::reactive::Owner;
use crate::scene::{
    BoxNode, ContainerNode, EffectNode, ExternalNode, ImageNode, ImmediateModeNode, PathCommand,
    PathNode, PathPoint, Rect, Scene, SceneNodeKind, ScrollNode, StubImmediateMode, TextGridNode,
    TextNode,
};
use crate::style::{BoxStyle, Color, PathStyle};
use crate::term_grid::{
    CellAttrs, CellWidth, CursorShape, GridBuffer, GridCursor, TermCell, TermColor,
};
use crate::widget_core::WidgetCore;
use crate::widgets::aria;
use crate::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};

/// R55.G.22 §5.49 — pin the composite paint-root tag convention.
///
/// Asserts that `V::view(state, frame)` returns a [`Scene`] which
/// contains a node tagged [`V::tag()`](WidgetCore::tag) somewhere
/// (depth-first walk via [`Scene::contains_tag`]). Pins the
/// [[composite-paint-root-tag-convention]] (R55.G.17 §5.49) per
/// widget binding — without the tag in the paint scene, AI-side
/// `scene/click` / `scene/key` / `scene/wheel` `{path: V::tag()}`
/// routing and `rect_for_tag` AT bounds attach both fail silently.
///
/// ## Why an `Owner::new()` wrap?
///
/// `WidgetCore::view` is sync and pure per §6.3 R51.27, but some
/// bindings observe [`Owner::current()`](crate::Owner::current)
/// inside the view fn — e.g. `examples/hello-button` registers a
/// hover-progress animation via [[oncecell-weak-self-pointer]] on
/// first paint. Calling the view without an active `Owner` would
/// panic outside the framework wrap; the helper installs a
/// throwaway `Owner::new().run(...)` scope so callers do not have to
/// remember which widget's view fn observes the current owner.
///
/// ## Usage
///
/// ```rust,no_run
/// # use pinion_core::test_fixtures::{assert_widget_view_carries_tag, ButtonFixture};
/// # use pinion_core::widgets::button::ButtonState;
/// # use pinion_core::Frame;
/// assert_widget_view_carries_tag::<ButtonFixture>(
///     ButtonState::Idle,
///     &Frame::default(),
/// );
/// ```
///
/// The `<V>` generic resolves both the view fn and the tag through
/// one trait-bound call site — adding a new widget pins the
/// convention with one line instead of replicating the 5-line
/// inline `assert!(scene.contains_tag(V::tag()), …)` block across
/// every example binding's test module.
///
/// # Panics
///
/// Panics if `V::view(state, frame)` returns a [`Scene`] that does
/// not contain a node tagged [`V::tag()`](WidgetCore::tag) anywhere
/// in its depth-first child / Scroll-content walk — that is exactly
/// the regression the helper exists to surface, so the panic is the
/// designed observable outcome.
pub fn assert_widget_view_carries_tag<V: WidgetCore>(state: V::State, frame: &Frame) {
    let owner = Owner::new();
    let scene = owner.run(|| V::view(state, frame));
    assert!(
        scene.contains_tag(V::tag()),
        "{} view must contain a node tagged {:?} (R55.G.17 §5.49 composite paint-root tag convention)",
        core::any::type_name::<V>(),
        V::tag(),
    );
}

/// R1360.2 — assert `V`'s view paints an **opaque window background**.
///
/// A `WidgetView`'s root [`Scene::Container`] fill is not decoration: it
/// becomes `RenderParams.base_color`, the surface clear for the whole
/// window (`pinion_runtime::paint_adapter::root_background`). Two defaults
/// compose into an invisible app:
///
/// * [`BoxStyle::default()`](crate::style::BoxStyle)'s fill is
///   `Color::default()` = `rgba(0, 0, 0, 0)` — so a root Container that
///   never calls `with_style` clears the window to **transparent black**,
///   which a compositor shows as black.
/// * [`TextStyle::new()`](crate::style::TextStyle)'s `fg_color` is
///   `rgb(0, 0, 0)` — so a `TextNode::new` row is **pure black**.
///
/// Black on black: the window renders nothing a human can see. R1360.1
/// shipped that bug in one binding, and the audit found two more already on
/// main (`hello-audio-rt`, `hello-audio-device`) — every pixel of their
/// live windows is `(0, 0, 0, α)`.
///
/// It survived because **the usual observations are blind to it**: a PNG
/// keeps alpha=0 (`encode_rgba8_png` writes `ColorType::Rgba`), so any
/// image *viewer* composites the window onto white and the black text reads
/// perfectly. Only sampling the pixel — or this assertion — sees it.
///
/// ## Usage
///
/// ```rust,no_run
/// # use pinion_core::test_fixtures::{assert_widget_view_paints_opaque_root, ButtonFixture};
/// # use pinion_core::widgets::button::ButtonState;
/// # use pinion_core::Frame;
/// assert_widget_view_paints_opaque_root::<ButtonFixture>(
///     ButtonState::Idle,
///     &Frame::default(),
/// );
/// ```
///
/// One line per binding, like
/// [`assert_widget_view_carries_tag`] — a per-binding regression test for a
/// defect whose cause is a shared default does not scale, and the 87 call
/// sites of that sibling are the evidence this shape does.
///
/// # Panics
///
/// Panics if the view's root is not a [`Scene::Container`] (any other root
/// clears the window to opaque black — legible only by luck, and never what
/// a binding means), or if that Container's fill is not fully opaque.
pub fn assert_widget_view_paints_opaque_root<V: WidgetCore>(state: V::State, frame: &Frame) {
    let owner = Owner::new();
    let scene = owner.run(|| V::view(state, frame));
    assert_scene_root_is_opaque(&scene, core::any::type_name::<V>());
}

/// R1470 §5.50 — the same R1360.2 check, made on a [`Scene`] the caller
/// already built.
///
/// [`assert_widget_view_paints_opaque_root`] is the one-liner for the common
/// case, but it can only reach a binding whose `view` is safe to *call* from a
/// test. `hello-audio-device`'s was not: its `view` resolves an `Owner::cache`
/// rig that opens a real cpal output device, so the opacity assertion opened
/// the developer's speakers — and on a host with no output device (every CI
/// runner) the binding's deliberate fail-loud path aborted the whole test
/// binary. Splitting the assertion from the *way the scene was obtained* lets
/// such a binding factor its scene into a pure builder, assert on that, and
/// keep the boot-time device policy untouched.
///
/// `who` names the subject in the panic message (a type name, a binding name).
///
/// # Panics
///
/// As [`assert_widget_view_paints_opaque_root`]: if `scene`'s root is not a
/// [`Scene::Container`], or if that Container's fill is not fully opaque.
pub fn assert_scene_root_is_opaque(scene: &Scene, who: &str) {
    let Scene::Container(root) = scene else {
        panic!(
            "{who}'s view must return a Scene::Container root — its fill is the \
             window's clear colour, and any other root variant clears to \
             opaque black (R1360.2)",
        );
    };
    assert_eq!(
        root.style.fill.a, 0xFF,
        "{who}'s view must fill its root with an OPAQUE colour: the root fill is \
         the window's clear colour, and the default (alpha 0) leaves the \
         window transparent — black on a compositor, under text whose own \
         default is black. Got {:?}. Fix: `.with_style(BoxStyle::filled(\
         theme.resolve(ColorRole::Surface)))` on the root. (R1360.2)",
        root.style.fill,
    );
}

/// R57.X.theme-fade §5.50 — advance `owner`'s registered animations by
/// one second of simulated wall-clock time (60 ticks of 1 / 60 s each)
/// so any in-flight spring settles to rest.
///
/// One second comfortably exceeds the `THEME_FADE_SPRING` Material 3
/// short4 (~200 ms) settle window the R57.X.theme-fade fade uses, and
/// also covers other paint-loop-driven [`Animation`]s
/// (`hello-button` hover progress, `caret_blink`, ...) at their
/// canonical settle horizons. After the call, the next
/// [`ThemeProvider::theme_animated`](crate::theme::ThemeProvider::theme_animated)
/// read returns the new target exactly via the at-rest snap path
/// (R585 §5.50), so widget cascade tests can assert exact equality
/// against palette field values without tolerance.
///
/// ## Usage pattern (R57.X.theme-fade widget cascade)
///
/// ```rust,no_run
/// # use pinion_core::reactive::Owner;
/// # use pinion_core::test_fixtures::settle_owner_animations;
/// let owner = Owner::new();
/// owner.run(|| {
///     // Register / re-target Animations against this Owner (e.g.
///     // flip the active ThemeMode so theme_animated() retargets
///     // its ThemeLinear spring toward the new palette).
/// });
/// settle_owner_animations(&owner);
/// owner.run(|| {
///     // Springs at rest — palette-cascade assertions can compare
///     // against `Theme::dark().surface` etc. via exact equality.
/// });
/// ```
///
/// The helper exists at the substrate level rather than as a
/// per-example boilerplate because the same five-line settle pattern
/// appears verbatim in every R57.X widget-binding test that swaps the
/// theme mid-test ([[substrate-incompleteness-signal]] — 9 sites
/// across `pinion-core::theme` substrate tests + 5 example bindings).
pub fn settle_owner_animations(owner: &Owner) {
    for _ in 0..60 {
        owner.tick_animations(1.0 / 60.0);
    }
}

/// R633 §5.7 §5.22 — substrate-level trait that routes a
/// `'static`-keyed cache binding through the appropriate per-widget
/// `use_*` hook. Lives in `pinion-core` (forward dep direction) so
/// `pinion-rpc` (and any other downstream crate that wires a per-tag
/// cache-binding fixture) consumes the abstraction instead of
/// defining it.
///
/// Pre-R633 the trait lived in `pinion-rpc::test_fixtures` and
/// impl'd for the `pinion-core` widget types via the orphan-rule
/// passport ("downstream trait + upstream type"). The arrangement
/// compiled but inverted the canonical dep direction:
/// `pinion-rpc` ↑ pinion-core via deps, yet `pinion-rpc`'s trait
/// reached *down* into core types. R633 flips the trait + impls
/// into `pinion-core::test_fixtures` (the `test-fixtures` feature
/// gate established in R51.127 §5.41) and `pinion-rpc` re-exports
/// or imports the trait by its substrate name.
///
/// Per the [[test-fixtures-feature-gate-pattern]] memory the
/// canonical Rust pattern for cross-crate test fixtures is:
///
/// 1. trait + impls live in the upstream crate behind a
///    `test-fixtures` (or `test-fixture`) feature
/// 2. downstream `dev-dependencies` activate the feature
/// 3. the trait + impls never reach a production binary
///
/// R633 lands all three: the trait is gated by `cfg(any(test,
/// feature = \"test-fixtures\"))` (via the `pub mod test_fixtures`
/// outer cfg in `lib.rs`); `pinion-rpc`'s `[dev-dependencies]`
/// activates the feature; consumers reach the trait via
/// `pinion_core::test_fixtures::BindableCacheSlot`.
pub trait BindableCacheSlot: Sized + 'static {
    /// Invoke the widget-specific `use_*` hook for `tag`. Must be
    /// called inside an active [`Owner::run`] scope — the helper
    /// fn [`bind_cache_slot`] wraps this contract so per-axis test
    /// sites collapse to a single line.
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self>;
}

impl BindableCacheSlot for crate::widgets::scroll::ScrollState {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::scroll::use_scroll_state(tag)
    }
}

impl BindableCacheSlot for crate::widgets::text_edit::TextEditState {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::text_edit::use_text_edit_state(tag)
    }
}

impl BindableCacheSlot for crate::theme::ThemeProvider {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::theme::use_theme(tag)
    }
}

impl BindableCacheSlot for crate::widgets::caret_blink::CaretBlink {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::caret_blink::use_caret_blink(tag)
    }
}

/// R633 §5.7 §5.22 — bind a substrate-introspection state slot under
/// `tag` on `owner`. Wraps [`BindableCacheSlot::use_in_scope`] in
/// [`Owner::run`] so each per-axis test site collapses to a single
/// line.
///
/// Generic over `S: BindableCacheSlot`. Call as
/// `bind_cache_slot::<ScrollState>(&owner, "list")` (or
/// `bind_cache_slot::<_>(&owner, "list")` when the return type is
/// inferred from the binding's later use).
#[must_use]
pub fn bind_cache_slot<S: BindableCacheSlot>(owner: &Owner, tag: &'static str) -> std::rc::Rc<S> {
    owner.run(|| S::use_in_scope(tag))
}

/// R1516 §5.2 — one node of every [`SceneNodeKind`], so a consumer that
/// must answer for *all* of them can be run over all of them.
///
/// The `match` is exhaustive: a kind added to the census arrives here as a
/// compile error, and until it is built no test that iterates
/// [`SceneNodeKind::ALL`] can be written to skip it. That is the point —
/// "a variant joined and nobody noticed" is the failure the census exists
/// to prevent, and a census whose own fixtures lagged behind it would
/// reproduce that failure one level up.
///
/// Lives here rather than in either caller because both `pinion-core`'s
/// census tests and `pinion-rpc`'s §2 #7 wire test need the same set, and
/// two copies of an exhaustive match are two places for a new kind to be
/// filled in lazily.
#[must_use]
pub fn scene_of_kind(kind: SceneNodeKind) -> Scene {
    let rect = Rect::new(1, 2, 30, 40);
    match kind {
        SceneNodeKind::Box => Scene::Box(BoxNode::new(rect, BoxStyle::default())),
        SceneNodeKind::Text => Scene::Text(TextNode::new("ab".to_string(), rect)),
        SceneNodeKind::Path => Scene::Path(PathNode::new(
            rect,
            vec![PathCommand::MoveTo(PathPoint::new(0.0, 0.0))],
            PathStyle::default(),
        )),
        SceneNodeKind::Image => Scene::Image(ImageNode::new("file:///x.png", rect)),
        SceneNodeKind::Container => Scene::Container(ContainerNode::new(vec![])),
        SceneNodeKind::Effect => Scene::Effect(EffectNode::new()),
        SceneNodeKind::External => Scene::External(ExternalNode::new(Box::new(StubExternal))),
        SceneNodeKind::Scroll => Scene::Scroll(ScrollNode::new(
            rect,
            Scene::Container(ContainerNode::new(vec![])),
        )),
        SceneNodeKind::ImmediateModeNode => Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), rect),
        ),
        SceneNodeKind::TextGrid => Scene::TextGrid(TextGridNode::new(CellMetric::DEFAULT)),
    }
}

/// R1615 — [`scene_of_kind`], with `tag` attached wherever the kind can carry
/// one.
///
/// Needed because a question asked *about a tag* — "why does this look like
/// that" — has to reach the node before it can decide the kind cannot answer.
/// The match is exhaustive for the same reason its sibling is: a new node kind
/// must state whether it is addressable by tag.
///
/// [`Scene::Effect`] is the one that comes back untagged, and that is not an
/// oversight here — an [`EffectNode`] has no tag field at all, so it is
/// unreachable by tag by construction. Callers derive that from
/// [`Scene::tag`] rather than hard-coding the exception.
#[must_use]
pub fn tagged_scene_of_kind(kind: SceneNodeKind, tag: &'static str) -> Scene {
    match scene_of_kind(kind) {
        Scene::Box(n) => Scene::Box(n.with_tag(tag)),
        Scene::Text(n) => Scene::Text(n.with_tag(tag)),
        Scene::Path(n) => Scene::Path(n.with_tag(tag)),
        Scene::Image(n) => Scene::Image(n.with_tag(tag)),
        Scene::Container(n) => Scene::Container(n.with_tag(tag)),
        Scene::External(n) => Scene::External(n.with_tag(tag)),
        Scene::Scroll(n) => Scene::Scroll(n.with_tag(tag)),
        Scene::ImmediateModeNode(n) => Scene::ImmediateModeNode(n.with_tag(tag)),
        Scene::TextGrid(n) => Scene::TextGrid(n.with_tag(tag)),
        // An `EffectNode` carries no tag field; see the doc above.
        effect @ Scene::Effect(_) => effect,
    }
}

/// R1618 — a node of `kind` that has been asked to publish ONE reason for its
/// own appearance, or `None` when nothing on that kind can be asked.
///
/// The independent statement of "how is this kind attributed", built by calling
/// each node type's real API rather than by reading
/// [`SceneNodeKind::marks_channel`](crate::scene::SceneNodeKind::marks_channel).
/// A kind that attributes POSITIONS is asked through the thing that carries its
/// content — a named [`StyleRun`](crate::scene::StyleRun) for text, a run over
/// the displayed buffer for a grid — and a kind that attributes ITSELF is asked
/// through `with_marks` over
/// [`domain::NODE`](crate::marks::domain::NODE). The DOMAIN that comes back is
/// therefore an observation, and a test can hold the declaration to it.
///
/// Exhaustive on purpose: a new node kind must say here how it is attributed,
/// or admit that it cannot be.
#[must_use]
pub fn marked_scene_of_kind(kind: SceneNodeKind, tag: &'static str) -> Option<Scene> {
    use crate::marks::{MarkSet, domain};
    let reason = |set: MarkSet| set.because("reason");
    Some(match tagged_scene_of_kind(kind, tag) {
        // Positional: the run is part of the CONTENT, so it is declared where
        // the content is and its indices count the content.
        Scene::Text(n) => {
            let end = u32::try_from(n.content.len()).unwrap_or(u32::MAX);
            Scene::Text(n.with_runs(vec![
                crate::scene::StyleRun::new(0, end, crate::style::TextStyle::new()).named("reason"),
            ]))
        }
        Scene::TextGrid(mut n) => {
            n.marks = Some(MarkSet::over(domain::BYTE).marking("reason", 0, 1));
            Scene::TextGrid(n)
        }
        // Whole-node: there is no interior, so the caller never picks a place.
        Scene::Box(n) => Scene::Box(n.with_marks(reason(MarkSet::whole()))),
        Scene::Path(n) => Scene::Path(n.with_marks(reason(MarkSet::whole()))),
        Scene::Image(n) => Scene::Image(n.with_marks(reason(MarkSet::whole()))),
        // Nothing to ask: these carry no attribution of their own.
        Scene::Container(_)
        | Scene::Scroll(_)
        | Scene::Effect(_)
        | Scene::External(_)
        | Scene::ImmediateModeNode(_) => return None,
    })
}

/// R1623 — one [`PathCommand`] of each
/// [`PathCommandKind`](crate::path_data::PathCommandKind), with every
/// argument set to a distinct value.
///
/// Exists so a consumer that must cover the whole path vocabulary — the
/// RPC wire, a backend census — can iterate
/// [`PathCommandKind::ALL`](crate::path_data::PathCommandKind::ALL)
/// instead of hand-listing the commands it remembers. The match is
/// exhaustive, so a new kind cannot reach those consumers untested: it
/// fails to compile here first.
///
/// The values are deliberately all different, so a serializer that
/// swaps two arguments is caught rather than reproducing a symmetric
/// fixture.
#[must_use]
pub fn path_command_of_kind(kind: crate::path_data::PathCommandKind) -> PathCommand {
    use crate::path_data::PathCommandKind;
    use crate::scene::{EllipticalArc, PathPoint};
    match kind {
        PathCommandKind::MoveTo => PathCommand::MoveTo(PathPoint::new(1.0, 2.0)),
        PathCommandKind::LineTo => PathCommand::LineTo(PathPoint::new(3.0, 4.0)),
        PathCommandKind::QuadTo => PathCommand::QuadTo {
            c: PathPoint::new(5.0, 6.0),
            end: PathPoint::new(7.0, 8.0),
        },
        PathCommandKind::CurveTo => PathCommand::CurveTo {
            c1: PathPoint::new(9.0, 10.0),
            c2: PathPoint::new(11.0, 12.0),
            end: PathPoint::new(13.0, 14.0),
        },
        PathCommandKind::ArcTo => PathCommand::ArcTo(EllipticalArc::new(
            15.0,
            16.0,
            17.0,
            true,
            false,
            PathPoint::new(18.0, 19.0),
        )),
        PathCommandKind::Close => PathCommand::Close,
    }
}

/// R1615 — a node of `kind` holding `child`, or `None` when that kind has no
/// child slot at all.
///
/// The structural fact "this kind contains other nodes", stated once by
/// construction rather than asserted as a list. A test that wants to know
/// whether a kind descends builds one of these and asks the walk, which is an
/// independent question from anything a kind *declares* about itself — the
/// distinction that matters when the two are supposed to agree.
///
/// Exhaustive, so a node kind with a new child slot has to be added here
/// instead of quietly answering `None`.
#[must_use]
pub fn scene_of_kind_containing(kind: SceneNodeKind, child: Scene) -> Option<Scene> {
    let rect = Rect::new(1, 2, 30, 40);
    match kind {
        SceneNodeKind::Container => Some(Scene::Container(ContainerNode::new(vec![child]))),
        SceneNodeKind::Scroll => Some(Scene::Scroll(ScrollNode::new(rect, child))),
        SceneNodeKind::Box
        | SceneNodeKind::Text
        | SceneNodeKind::Path
        | SceneNodeKind::Image
        | SceneNodeKind::Effect
        | SceneNodeKind::External
        | SceneNodeKind::ImmediateModeNode
        | SceneNodeKind::TextGrid => None,
    }
}

/// R1629 — a node of `kind` offered `children`, and how many of them it could
/// actually take.
///
/// The structural fact [`scene_of_kind_containing`] cannot state: a kind that
/// descends may hold a **list** it assembled, or exactly **one** subtree it
/// shows through a viewport, and that arity is the difference between a
/// composition and a window onto one. It is not a restatement of any
/// declaration — it is the arity of the node constructors themselves
/// ([`ContainerNode::new`] takes a `Vec`, [`ScrollNode::new`] takes a single
/// [`Scene`]), which is why a test can hold
/// [`SceneNodeKind::derives_channel`](crate::scene::SceneNodeKind::derives_channel)
/// to it.
///
/// Exhaustive, for [`scene_of_kind_containing`]'s reason.
#[must_use]
pub fn scene_of_kind_holding(kind: SceneNodeKind, children: Vec<Scene>) -> Option<Scene> {
    let rect = Rect::new(1, 2, 30, 40);
    match kind {
        SceneNodeKind::Container => Some(Scene::Container(ContainerNode::new(children))),
        // Takes one, by type. The rest are dropped because there is nowhere
        // for them to go, which is the fact being observed.
        SceneNodeKind::Scroll => children
            .into_iter()
            .next()
            .map(|one| Scene::Scroll(ScrollNode::new(rect, one))),
        SceneNodeKind::Box
        | SceneNodeKind::Text
        | SceneNodeKind::Path
        | SceneNodeKind::Image
        | SceneNodeKind::Effect
        | SceneNodeKind::External
        | SceneNodeKind::ImmediateModeNode
        | SceneNodeKind::TextGrid => None,
    }
}

/// Minimal Button binding for substrate-level tests.
///
/// Carries a [`ButtonExternal`] so the SCXML statechart stays
/// observable and intent-emitting. The view fn paints a 32×48-pixel
/// button rect tagged `test_btn` so the runtime hit-test router
/// resolves it.
///
/// The same fixture covers the TUI 4×3-cell footprint — the rect
/// `(0, 0, 32, 48)` lands inside the top-left cell of the buffer.
pub struct ButtonFixture;

impl WidgetCore for ButtonFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "test_btn"
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Ok(IntrospectValue::Text(name)) = intro.query("state")
        {
            return match name.as_str() {
                "Hover" => ButtonState::Hover,
                "Pressed" => ButtonState::Pressed,
                "Disabled" => ButtonState::Disabled,
                _ => ButtonState::Idle,
            };
        }
        ButtonState::Idle
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 32, 48),
            tag: Some(Cow::Borrowed("test_btn")),
            children: vec![Scene::Text(TextNode::default())],
            // (R1020 §5.39) Single keyboard focus stop — the scene-derived
            // enumeration collects "test_btn" (the pre-R1020 `focusable_tags()`
            // default `vec![tag()]` is retired).
            layout: crate::style::LayoutStyle::new().with_focusable(true),
            ..Default::default()
        })
    }

    fn event_name(event: Self::Event) -> &'static str {
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::PointerCancel => "PointerCancel",
            ButtonEvent::KeyboardActivate => "KeyboardActivate",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "Test"
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

/// R1569 §5.39 §2 #6 — a binding whose focused widget SHADOWS the window's
/// accelerator layers.
///
/// Its `keybinding` map claims the bare character `r`, and its External is a
/// [`KeySequenceEditExternal`](crate::widgets::key_sequence::KeySequenceEditExternal)
/// that records every chord while recording. So one dispatch of `"r"` has two
/// possible outcomes — the binding's `Record` event, or a recorded chord — and
/// WHICH one happens is exactly the precedence R1569 added.
///
/// Shared rather than per-backend because that is the point: the GUI and the
/// TUI resolve the shadow through one `CoreShell::accelerator_shadow`, and a
/// fixture each would let the two drift while both stayed green. R1569's own
/// counterfactual removed the TUI gate and every GUI assertion still passed —
/// which is how this fixture came to exist.
pub struct ShadowingFixture;

impl WidgetCore for ShadowingFixture {
    /// `(recorded_something, is_disabled)`.
    ///
    /// Both, because one alone cannot tell the two outcomes apart: the
    /// accelerator this fixture declares DISABLES the editor, so "nothing was
    /// recorded" is consistent with the keystroke having vanished. Observing
    /// the accelerator's own effect is what makes the baseline a control
    /// rather than an absence.
    type State = (bool, bool);
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        let mut editor = crate::widgets::key_sequence::KeySequenceEditExternal::new();
        editor.send(crate::widgets::key_sequence::KeySequenceEvent::Record);
        Box::new(editor)
    }

    fn tag() -> &'static str {
        "shadow_fixture"
    }

    fn read_state(scene: &Scene) -> Self::State {
        let Some(intro) = scene
            .find_external_with_tag(Self::tag())
            .and_then(|n| n.handle.introspect())
        else {
            return (false, false);
        };
        let recorded = matches!(
            intro.query("in_flight"),
            Ok(IntrospectValue::Text(ref run)) if !run.is_empty(),
        );
        let disabled = matches!(
            intro.query("state"),
            Ok(IntrospectValue::Text(ref name)) if name == "Disabled",
        );
        (recorded, disabled)
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 32, 48),
            tag: Some(Cow::Borrowed("shadow_fixture")),
            children: vec![Scene::Text(TextNode::default())],
            layout: crate::style::LayoutStyle::new().with_focusable(true),
            ..Default::default()
        })
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "Shadow"
    }

    /// The accelerator the editor must be able to take away.
    fn keybinding(key: &str) -> Option<Self::Event> {
        (key == "r").then_some(ButtonEvent::Disable)
    }

    /// Every key that survives the accelerator layers is a chord to record.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: crate::input::Modifiers,
    ) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        let chord = crate::accelerator::Chord::new(key, modifiers);
        scene
            .find_external_with_tag_mut(Self::tag())
            .and_then(|n| n.handle.introspect_mut())
            .is_some_and(|intro| {
                intro
                    .invoke("record", IntrospectValue::Text(chord.portable()))
                    .is_ok()
            })
    }
}

/// (R55.D.5 §5.45, lifted R884) `Owner::cache` key for the shared
/// [`ScrollState`](crate::widgets::scroll::ScrollState) the
/// [`ScrollbarMultiFixture`]'s extra scrollbar External attaches.
/// Tests resolve the same `Rc` via
/// `root_owner.run(|| use_scroll_state(MULTI_FIXTURE_SCROLL_KEY))`
/// to observe offsets after a dispatch.
pub const MULTI_FIXTURE_SCROLL_KEY: &str = "sb_state";

/// (R55.D.5 §5.45, lifted R884) Multi-External composition fixture:
/// [`ButtonFixture`] semantics plus a sibling
/// [`ScrollBarExternal`](crate::widgets::scrollbar::ScrollBarExternal)
/// tagged `"sb"`, so [`WidgetCore::create_extra_externals`] is
/// non-empty and the substrate's state scene composes as
/// `Scene::Container([primary, scrollbar])` instead of the bare
/// `Scene::External(primary)`.
///
/// Lifted out of `pinion-runtime::core_shell::tests` at R884 so all
/// three dispatch producers pin the Container-root invariant against
/// the same fixture: `CoreShell::forward` / `send_to_primary`
/// (pinion-runtime), `ShellCore::dispatch_intent` (pinion-shell) and
/// `ShellCoreTui::dispatch_intent` (pinion-tui) — the R884 bug class
/// was exactly "framework send silently no-ops on a Container root",
/// and a bare-External fixture cannot catch it.
/// R1549.2 §5.35 §5.38 §2 #6 — [`ButtonFixture`] whose button DECLARES a
/// press-and-hold repeat cadence (`AutoRepeat`, the toolkit
/// `setAutoRepeat`). Identical in every other respect,
/// so a test that swaps it in isolates exactly one variable: whether the
/// backend under test advances a held press.
///
/// It exists because R1549 landed the repeat on the Vello path only,
/// and nothing could catch that from the TUI side — every fixture's
/// button declared no cadence, so a backend that never ticked a hold and
/// one that ticked it correctly produced identical output.
///
/// The cadence is deliberately short (50 ms delay, 25 ms interval) so a
/// test crosses several thresholds in one injected tick without pretending
/// the DEFAULT is short; the default lives on `AutoRepeat::desktop`.
pub struct RepeatingButtonFixture;

impl RepeatingButtonFixture {
    /// The declared cadence — read by a test so its expected fire count
    /// comes from the same value the widget answers with, rather than a
    /// second copy of the numbers that can drift from it.
    #[must_use]
    pub fn repeat() -> crate::input::AutoRepeat {
        crate::input::AutoRepeat::new(0.050, 0.025)
    }
}

impl WidgetCore for RepeatingButtonFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new().with_auto_repeat(Self::repeat()))
    }

    fn tag() -> &'static str {
        <ButtonFixture as WidgetCore>::tag()
    }

    fn read_state(scene: &Scene) -> Self::State {
        <ButtonFixture as WidgetCore>::read_state(scene)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        <ButtonFixture as WidgetCore>::view(state, frame)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "repeating button fixture"
    }
}

pub struct ScrollbarMultiFixture;

impl WidgetCore for ScrollbarMultiFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        <ButtonFixture as WidgetCore>::create_external()
    }

    fn create_extra_externals() -> Vec<crate::widget_core::ExtraExternal> {
        let state = crate::widgets::scroll::use_scroll_state(MULTI_FIXTURE_SCROLL_KEY);
        state.set_max(0, 100);
        let bar = crate::widgets::scrollbar::ScrollBarExternal::new().attach_state(state);
        vec![crate::widget_core::ExtraExternal::new("sb", Box::new(bar))]
    }

    fn tag() -> &'static str {
        <ButtonFixture as WidgetCore>::tag()
    }

    fn read_state(scene: &Scene) -> Self::State {
        // Multi-External composition wraps the primary External in a
        // Container; walk to it by tag (R698.A §5.16 — state name
        // resolution through the WidgetStateName SSOT).
        scene
            .find_external_with_tag(<Self as WidgetCore>::tag())
            .and_then(|n| n.handle.introspect())
            .and_then(|i| i.query("state").ok())
            .map_or(ButtonState::Idle, |v| match v {
                IntrospectValue::Text(s) => {
                    <ButtonState as crate::WidgetStateName>::from_name_or_default(&s)
                }
                _ => ButtonState::Idle,
            })
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        <ButtonFixture as WidgetCore>::view(state, frame)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "MultiExternal"
    }
}

/// R887 §5.49 §5.53 — projected state for [`ContextMenuFixture`]:
/// the popup's open flag plus its anchor point, read straight off the
/// carried [`ContextMenuExternal`]'s `open` / `open_x` / `open_y`
/// query slots.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ContextMenuFixtureState {
    /// `query("open")` — whether the popup is showing.
    pub open: bool,
    /// `query("open_x")` / `query("open_y")` — the press anchor while
    /// open, `None` while closed.
    pub anchor: Option<(f32, f32)>,
}

/// R887 §5.49 §5.53 — secondary-click (right-click) producer fixture:
/// carries a real [`ContextMenuExternal`] and implements
/// [`WidgetCore::apply_secondary_click`] exactly the way the R772
/// `hello-contextmenu` binding does (`invoke("open_at", "<x>,<y>")`),
/// so every dispatch producer of the secondary-click arc pins the
/// same observable — the popup opens at the press point:
///
/// - `pinion-shell::substrate` — the `DeferredInput::SecondaryClick`
///   drain (`scene/click {button: "right"}`) and the winit
///   `MouseInput { button: Right }` path, both through
///   `secondary_click_for_window`.
/// - `pinion-tui::substrate` — the same drain plus the crossterm
///   `Down(Right)` arm, both through `ShellCoreTui::secondary_click`.
///
/// The walker is `find_external_with_tag` (root-shape-agnostic), not
/// a bare `Scene::External` root match — fixtures must stay valid if
/// a future variant composes extras around the primary (the R884
/// silent-drop class).
pub struct ContextMenuFixture;

impl WidgetCore for ContextMenuFixture {
    type State = ContextMenuFixtureState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(crate::widgets::context_menu::ContextMenuExternal::new(3))
    }

    fn tag() -> &'static str {
        "ctx_fixture"
    }

    fn read_state(scene: &Scene) -> Self::State {
        let open_query = |intro: &dyn crate::external::ExternalIntrospect, path: &str| {
            if let Ok(IntrospectValue::Float(v)) = intro.query(path) {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "anchor coords are window-local f32 values widened for the wire"
                )]
                Some(v as f32)
            } else {
                None
            }
        };
        scene
            .find_external_with_tag(Self::tag())
            .and_then(|n| n.handle.introspect())
            .map_or_else(ContextMenuFixtureState::default, |intro| {
                let open = matches!(intro.query("open"), Ok(IntrospectValue::Bool(true)));
                let anchor = match (open_query(intro, "open_x"), open_query(intro, "open_y")) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                };
                ContextMenuFixtureState { open, anchor }
            })
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 200, 120),
            tag: Some(Cow::Borrowed("ctx_fixture")),
            children: vec![Scene::Text(TextNode::default())],
            ..Default::default()
        })
    }

    fn apply_secondary_click(scene: &mut Scene, x: f32, y: f32) -> bool {
        let Some(node) = scene.find_external_with_tag_mut(Self::tag()) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke(
                "open_at",
                crate::widgets::context_menu::ContextMenuExternal::open_at_args(x, y),
            ),
            Ok(IntrospectValue::Bool(true))
        )
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "ContextMenuFixture"
    }
}

/// R1456 R1462 §5.39 — the invoker of [`ModalTailFixture`]: a background
/// control that opens the modal, and the tag the trap's automatic restore
/// aims at.
pub const MODAL_TAIL_TRIGGER: &str = "trigger";
/// R1462 §5.39 — a second background control. Proves the base enumeration
/// comes back *whole* on pop, and serves as the target of an explicit
/// focus request that competes with the automatic restore.
pub const MODAL_TAIL_OTHER_BG: &str = "other_bg";
/// R1456 §5.39 — the command menu's only member (modal A).
pub const MODAL_TAIL_MENU_ROW: &str = "menu_row";
/// R1456 §5.39 — the confirm dialog's only member (modal B).
pub const MODAL_TAIL_CONFIRM_OK: &str = "confirm_ok";

thread_local! {
    /// `(tag, focused)` in dispatch order, appended by every
    /// [`FocusArcRecorder`]. Thread-local, so parallel tests cannot see
    /// each other's arcs.
    static FOCUS_ARC_LOG: std::cell::RefCell<Vec<(&'static str, bool)>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

/// R1456 R1462 §5.39 — the focus arc dispatched to [`ModalTailFixture`]'s
/// externals since the last [`clear_focus_arc_log`], as `(tag, focused)`
/// pairs in order.
///
/// A `ButtonExternal` only keeps the latest posture as a flag, which cannot
/// distinguish "never notified" from "notified twice and back" — the exact
/// distinction the one-arc-per-dispatch-tail claim rests on. Hence a
/// recorder that keeps the sequence.
#[must_use]
pub fn focus_arc_log() -> Vec<(&'static str, bool)> {
    FOCUS_ARC_LOG.with_borrow(Clone::clone)
}

/// R1456 R1462 §5.39 — drop everything [`focus_arc_log`] would report, so
/// a test can assert on the arc of one specific dispatch.
pub fn clear_focus_arc_log() {
    FOCUS_ARC_LOG.with_borrow_mut(Vec::clear);
}

/// R1456 R1462 §5.39 — an [`External`] that records the focus arc the
/// substrate dispatches to it into [`focus_arc_log`].
#[derive(Debug)]
pub struct FocusArcRecorder(&'static str);

impl FocusArcRecorder {
    /// Record under `tag` — the paint tag the substrate addresses this
    /// external by.
    #[must_use]
    pub fn new(tag: &'static str) -> Self {
        Self(tag)
    }
}

impl External for FocusArcRecorder {
    fn backends(&self) -> crate::external::BackendSupport {
        crate::external::BackendSupport::new(
            &[
                crate::external::Backend::Gui,
                crate::external::Backend::Tui,
                crate::external::Backend::Rpc,
            ],
            crate::external::BackendFallback::Skip,
        )
    }
    fn repaint_ownership(&self) -> crate::external::RepaintOwner {
        crate::external::RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> crate::external::ThreadOwnership {
        crate::external::ThreadOwnership::UiThreadSync
    }
    fn on_focus_change(&mut self, focused: bool) {
        FOCUS_ARC_LOG.with_borrow_mut(|log| log.push((self.0, focused)));
    }
}

/// R1456 R1462 §5.39 — the dispatch-tail modal-focus fixture: four
/// focusable [`FocusArcRecorder`] externals standing in for the two
/// background controls and the two modals' members.
///
/// Two consumer field reports drive it, one per backend, because §2 #6
/// makes the modal drain a *mirrored* seam — a fix on one backend alone
/// would give GUI and TUI different focus from identical input:
///
/// - `pinion-shell::substrate::modal_tail_focus_tests` (R1456 handoff,
///   R1462 explicit-over-automatic precedence).
/// - `pinion-tui::substrate::modal_tail_focus_tests` (the mirror; before
///   R1462 the terminal drain had no modal test at all).
///
/// The view is a bare tagged [`Scene::Container`], deliberately: these
/// tests drive [`crate::modal_scope_request`] / [`crate::focus_request`]
/// against a real focus manager, and the enumeration is seeded by the test
/// rather than derived from paint, so the fixture must not paint focusable
/// nodes that would compete with it.
pub struct ModalTailFixture;

impl WidgetCore for ModalTailFixture {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(FocusArcRecorder::new(MODAL_TAIL_TRIGGER))
    }

    fn create_extra_externals() -> Vec<crate::widget_core::ExtraExternal> {
        vec![
            crate::widget_core::ExtraExternal::new(
                MODAL_TAIL_OTHER_BG,
                Box::new(FocusArcRecorder::new(MODAL_TAIL_OTHER_BG)),
            ),
            crate::widget_core::ExtraExternal::new(
                MODAL_TAIL_MENU_ROW,
                Box::new(FocusArcRecorder::new(MODAL_TAIL_MENU_ROW)),
            ),
            crate::widget_core::ExtraExternal::new(
                MODAL_TAIL_CONFIRM_OK,
                Box::new(FocusArcRecorder::new(MODAL_TAIL_CONFIRM_OK)),
            ),
        ]
    }

    fn tag() -> &'static str {
        MODAL_TAIL_TRIGGER
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view((): Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode::new(Vec::new()).with_tag(MODAL_TAIL_TRIGGER))
    }

    fn event_name((): Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "ModalTailFixture"
    }
}

/// R1715 §5.39 §5.35 §5.15 — the raw multi-button pointer sink's paint tag in
/// [`RawSinkFocusFixture`]: a terminal pane forwarding xterm mouse reports to
/// the program it hosts.
pub const RAW_FOCUS_PANE: &str = "raw_focus.pane";

thread_local! {
    /// R1715 — when set, [`RAW_FOCUS_OTHER`]'s control hands its focus on to
    /// this tag from inside `External::on_focus_change`.
    static RAW_FOCUS_REDIRECT: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
    /// R1715 — the same for [`RAW_FOCUS_PANE`]'s sink. Point the two at each
    /// other and the frame cannot settle, which is the case the resolution's
    /// pass bound exists to catch.
    static RAW_SINK_REDIRECT: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
    /// R1715 — how many times either widget was told it GAINED focus.
    ///
    /// This is what makes the resolution's pass bound observable. A bound that
    /// only has to terminate is satisfied by any finite number, so raising it
    /// from 8 to 200 changes nothing a test can see — measured, exactly that
    /// counterfactual passed. What the bound is actually for is that user code
    /// re-runs a SMALL number of times per frame, and this counter is that
    /// quantity.
    static RAW_FOCUS_GAINED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// R1715 §5.39 — make [`RAW_FOCUS_OTHER`]'s control hand its focus on from
/// inside `on_focus_change` (`None` restores the inert default).
///
/// The shape a container reaches for when it wants to give its focus to a
/// child: "I was told I have focus; the real target is over there." The
/// observer fires from inside the focus resolution, so honouring it requires
/// the resolution to reach a FIXED POINT rather than stop after one pass —
/// which is what R1715 made it do. Measured at R1715, none of the tree's 9
/// `on_focus_change` bodies writes a mailbox, so without a deliberate
/// exerciser the settle loop would be a mechanism nothing drives.
pub fn set_raw_focus_redirect(target: Option<&'static str>) {
    RAW_FOCUS_REDIRECT.set(target);
}

/// R1715 §5.39 — the [`RawFocusSink`] half of [`set_raw_focus_redirect`].
/// Arming both, pointed at each other, is the non-converging frame.
pub fn set_raw_sink_redirect(target: Option<&'static str>) {
    RAW_SINK_REDIRECT.set(target);
}

/// R1715 §5.39 — how many times a widget of [`RawSinkFocusFixture`] has been
/// told it GAINED focus since the last [`clear_raw_focus_edges`].
///
/// The resolution's pass bound is only a real bound if something reads it. A
/// test asserting this count against a literal is that reader: the frame's
/// user-code re-entry is bounded and SMALL, not merely finite.
#[must_use]
pub fn raw_focus_gained_count() -> usize {
    RAW_FOCUS_GAINED.get()
}

/// R1715 — record that a fixture widget was told it gained focus.
fn note_raw_focus_gained() {
    RAW_FOCUS_GAINED.set(RAW_FOCUS_GAINED.get() + 1);
}

/// R1715 §5.39 — the sibling control that holds the keyboard before the raw
/// edge lands, so a test reads a focus **move** rather than a ring that was
/// already where it wanted to be.
pub const RAW_FOCUS_OTHER: &str = "raw_focus.other";

/// R1715 §5.39 §5.35 §5.15 — an [`External`] that owns the raw multi-button
/// pointer stream and asks for the keyboard from inside
/// [`External::raw_pointer_button`].
///
/// The consumer shape (sprag PINION-PR89): a pane hosting a child program that
/// enabled xterm mouse reporting owns the raw stream, so
/// [`wants_raw_pointer_buttons`](External::wants_raw_pointer_buttons)
/// suppresses the GUI default for it — `click_to_focus` included. The focus
/// mailbox is therefore the ONLY channel such a widget has left to say "the
/// click that reached my child also gave me the keyboard".
///
/// It requests on **every** `(button, edge)` pair rather than the realistic
/// left-press alone: the shell seam routes all six pairs through ONE raw arm,
/// so a fixture answering a single pair would leave five arms unmeasured.
#[derive(Debug, Default)]
pub struct RawFocusSink;

thread_local! {
    /// R1715 — every raw edge [`RawFocusSink`] was handed, as
    /// `"<button>:<edge>"`. The child's own report: a fix that bought focus
    /// by *stealing* the click would empty this, and emptying it breaks every
    /// mouse-driven program running inside a pane.
    static RAW_FOCUS_EDGE_LOG: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// R1715 — every payload the LEGACY `send` wire delivered to
    /// [`RawFocusSink`]. It must stay EMPTY: a raw sink trades that wire for
    /// the raw stream, which is one of the four GUI defaults
    /// [`External::wants_raw_pointer_buttons`] suppresses for it. This is the
    /// observable the round's own counterfactual found nothing was reading —
    /// dropping the raw arm's `return` let both dispatches run, and every gate
    /// stayed green because focus happened to land on the same tag either way.
    static RAW_FOCUS_SEND_LOG: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// R1715 §5.35 §5.15 — the raw edges [`RawFocusSink`] received since the last
/// [`clear_raw_focus_edges`], as `"<button>:<edge>"` wire tokens.
#[must_use]
pub fn raw_focus_edges() -> Vec<String> {
    RAW_FOCUS_EDGE_LOG.with_borrow(Clone::clone)
}

/// R1715 §5.35 §5.15 — drop everything [`raw_focus_edges`] would report, so a
/// test can assert on the delivery of one specific dispatch.
pub fn clear_raw_focus_edges() {
    RAW_FOCUS_EDGE_LOG.with_borrow_mut(Vec::clear);
    RAW_FOCUS_SEND_LOG.with_borrow_mut(Vec::clear);
    RAW_FOCUS_GAINED.set(0);
}

/// R1715 §5.35 §5.15 — the payloads the LEGACY `PointerDown` / `PointerUp`
/// send wire delivered to [`RawFocusSink`] since the last
/// [`clear_raw_focus_edges`]. A correct raw arm leaves this **empty**: owning
/// the raw stream means the GUI default for this widget does not also run.
#[must_use]
pub fn raw_focus_legacy_sends() -> Vec<String> {
    RAW_FOCUS_SEND_LOG.with_borrow(Clone::clone)
}

impl External for RawFocusSink {
    fn backends(&self) -> crate::external::BackendSupport {
        crate::external::BackendSupport::new(
            &[
                crate::external::Backend::Gui,
                crate::external::Backend::Tui,
                crate::external::Backend::Rpc,
            ],
            crate::external::BackendFallback::Skip,
        )
    }
    fn repaint_ownership(&self) -> crate::external::RepaintOwner {
        crate::external::RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> crate::external::ThreadOwnership {
        crate::external::ThreadOwnership::UiThreadSync
    }
    fn wants_raw_pointer_buttons(&self) -> bool {
        true
    }
    fn raw_pointer_button(&mut self, event: crate::input::RawPointerButton) {
        RAW_FOCUS_EDGE_LOG.with_borrow_mut(|log| {
            log.push(format!(
                "{}:{}",
                event.button.as_wire_name(),
                event.edge.as_wire_name()
            ));
        });
        crate::focus_request::request(RAW_FOCUS_PANE);
    }
    fn on_focus_change(&mut self, focused: bool) {
        if !focused {
            return;
        }
        note_raw_focus_gained();
        if let Some(target) = RAW_SINK_REDIRECT.get() {
            crate::focus_request::request(target);
        }
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn crate::external::ExternalIntrospect> {
        Some(self)
    }
}

/// R1715 §5.35 §5.15 — the sink's `send` channel exists ONLY so a test can
/// prove nothing arrives on it. `dispatch_send` reaches a widget through
/// `introspect_mut().invoke("send", …)`, so a sink without this surface cannot
/// tell "the GUI default was suppressed" from "the GUI default ran and had
/// nowhere to land" — and that indistinguishability is what let the round's
/// CF-5 pass with the raw arm's `return` deleted.
impl crate::external::ExternalIntrospect for RawFocusSink {
    fn schema(&self) -> crate::external::IntrospectSchema {
        crate::external::IntrospectSchema::new(
            const { &[crate::external::SchemaField::new("send", "string")] },
        )
    }

    fn query(&self, _path: &str) -> Result<IntrospectValue, crate::external::ReadRefusal> {
        Err(crate::external::ReadRefusal::UnknownPath)
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let payload = match args {
            IntrospectValue::Text(t) => t,
            other => format!("{other:?}"),
        };
        RAW_FOCUS_SEND_LOG.with_borrow_mut(|log| log.push(payload));
        Ok(IntrospectValue::Null)
    }
}

/// R1715 §5.39 — the plain control beside [`RawFocusSink`]: an ordinary
/// focusable widget that takes focus through `click_to_focus` like any other.
///
/// Its `on_focus_change` is the fixture's **deliberate offender**, armed by
/// [`set_raw_focus_redirect`]: a container trying to hand its focus to a child
/// from inside the observer. Kept as its own type rather than folded into
/// [`FocusArcRecorder`], because that recorder is shared with the modal-tail
/// tests and an offender that leaked into them would break tests about a
/// different contract.
#[derive(Debug, Default)]
pub struct RawFocusControl;

impl External for RawFocusControl {
    fn backends(&self) -> crate::external::BackendSupport {
        crate::external::BackendSupport::new(
            &[
                crate::external::Backend::Gui,
                crate::external::Backend::Tui,
                crate::external::Backend::Rpc,
            ],
            crate::external::BackendFallback::Skip,
        )
    }
    fn repaint_ownership(&self) -> crate::external::RepaintOwner {
        crate::external::RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> crate::external::ThreadOwnership {
        crate::external::ThreadOwnership::UiThreadSync
    }
    fn on_focus_change(&mut self, focused: bool) {
        if !focused {
            return;
        }
        note_raw_focus_gained();
        if let Some(target) = RAW_FOCUS_REDIRECT.get() {
            crate::focus_request::request(target);
        }
    }
}

/// R1715.1 §5.16 §5.41 — the tag of [`NoPrimaryFixture`]'s only surface.
pub const NO_PRIMARY_PANEL: &str = "no_primary.panel";

/// R1715.1 (R1306 PR-51) §5.16 — a binding with NO primary surface: every
/// surface is a dynamic extra, the topology `hello-floating-chart` and
/// `hello-dock-chart` use.
///
/// Such a binding's [`WidgetCore::tag`] and [`WidgetCore::create_external`] are
/// `unreachable!()` BY DESIGN — `primary_surface()` returning `None` is the
/// declaration that they must never be reached. So any substrate site that
/// reads the binding's identity with a bare `V::tag()` instead of through
/// `primary_surface()` panics here and nowhere else.
///
/// It exists because R1714 did exactly that in the paint path and no gate saw
/// it: the topology had a dedicated example (`hello-no-primary`) whose four
/// tests never paint, so the panic surfaced only in an unrelated example that
/// happens to be no-primary AND paints — in CI, after the push. A shell gate
/// that paints this fixture is the layer the defect actually lives at.
pub struct NoPrimaryFixture;

impl WidgetCore for NoPrimaryFixture {
    type State = ();
    type Event = ();

    fn primary_surface() -> Option<crate::widget_core::PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("NoPrimaryFixture has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("NoPrimaryFixture has no primary surface — see primary_surface()")
    }

    fn create_extra_externals() -> Vec<crate::widget_core::ExtraExternal> {
        vec![crate::widget_core::ExtraExternal::new(
            NO_PRIMARY_PANEL,
            Box::new(StubExternal),
        )]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view((): Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            tag: Some(Cow::Borrowed(NO_PRIMARY_PANEL)),
            children: Vec::new(),
            layout: crate::style::LayoutStyle {
                size: crate::style::Size::px(80, 40),
                focusable: true,
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn event_name((): Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "NoPrimaryFixture"
    }
}

/// R1715 §5.39 §5.35 §5.15 — the raw-edge focus fixture: a [`RawFocusSink`]
/// beside a plain focusable control, both painted as hit-testable rects so a
/// test can drive a real pointer edge at either one.
///
/// Two consumer field reports drive it, one per backend, because §2 #6 makes
/// post-dispatch focus resolution a *mirrored* seam — a fix on one backend
/// alone would give GUI and TUI different focus from identical input:
///
/// - `pinion-shell/tests/raw_edge_resolves_its_focus.rs` (PINION-PR89).
/// - `pinion-tui/tests/raw_edge_resolves_its_focus.rs` (the mirror).
///
/// Unlike [`ModalTailFixture`] the view DOES paint focusable nodes: these
/// tests drive the seam through a real hit-test, so the enumeration has to be
/// the scene-derived one a click resolves against.
pub struct RawSinkFocusFixture;

impl WidgetCore for RawSinkFocusFixture {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RawFocusControl)
    }

    fn create_extra_externals() -> Vec<crate::widget_core::ExtraExternal> {
        vec![crate::widget_core::ExtraExternal::new(
            RAW_FOCUS_PANE,
            Box::new(RawFocusSink),
        )]
    }

    fn tag() -> &'static str {
        RAW_FOCUS_OTHER
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view((): Self::State, _frame: &Frame) -> Scene {
        // Two side-by-side 40x40 focus stops: the left half is the plain
        // control, the right half is the raw sink. A test seeds the cursor
        // inside one and the router resolves the edge to that tag.
        //
        // The geometry is DECLARED (flex row + fixed sizes), not written into
        // `rect` — the paint pass lowers `layout` through taffy and overwrites
        // whatever rects the view fn wrote, so a hand-placed rect on a node
        // whose style says `Auto` collapses to zero height and the hit-test
        // silently resolves to the root instead. Measured while building this
        // fixture: `raw_focus_edges()` came back EMPTY with every focus
        // assertion red, which reads exactly like the defect under test.
        let stop = |tag: &'static str| {
            Scene::Container(ContainerNode {
                tag: Some(Cow::Borrowed(tag)),
                children: Vec::new(),
                layout: crate::style::LayoutStyle {
                    size: crate::style::Size::px(40, 40),
                    flex_shrink: 0.0,
                    focusable: true,
                    ..Default::default()
                },
                ..Default::default()
            })
        };
        Scene::Container(ContainerNode {
            tag: Some(Cow::Borrowed("raw_focus.root")),
            children: vec![stop(RAW_FOCUS_OTHER), stop(RAW_FOCUS_PANE)],
            layout: crate::style::LayoutStyle {
                display: crate::style::Display::Flex,
                flex_direction: crate::style::FlexDirection::Row,
                size: crate::style::Size::px(80, 40),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn event_name((): Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "RawSinkFocusFixture"
    }
}

/// R51.167 §5.23 R27 — substrate-level reducer test fixture.
///
/// Reuses [`ButtonFixture`]'s External / paint / `read_state` /
/// `event_name` surface (the SCXML statechart and view geometry are
/// identical) but overrides [`WidgetCore::update`] to emit one
/// `echo.reply` [`Command`] per incoming [`Intent`]. Used by:
///
/// - `pinion-runtime::core_shell::tests` — R51.167 substrate API
///   `route_intent_through_update` assertions.
/// - `pinion-shell::substrate::tests` — R51.168 `dispatch_intent`
///   wires the reducer step BEFORE the SCXML invoke send.
/// - `pinion-tui::substrate::tests` — R51.168 TUI-side mirror.
///
/// Keeping the fixture in `pinion-core::test_fixtures` (rather than
/// duplicating it per backend) lets the three test sites assert
/// identical reducer behaviour without reimplementing the
/// `ButtonExternal` carrier each time.
pub struct EchoButtonFixture;

impl WidgetCore for EchoButtonFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "echo_btn"
    }

    fn read_state(scene: &Scene) -> Self::State {
        <ButtonFixture as WidgetCore>::read_state(scene)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        <ButtonFixture as WidgetCore>::view(state, frame)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "EchoBtn"
    }

    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        // R51.173 §5.23 R27 — by-value snapshot. The fixture
        // discards the snapshot (no state-dependent branching) and
        // emits one `echo.reply` per incoming Intent so the wiring
        // tests can count the queued commands deterministically.
        //
        // R51.177 §5.23 R27 — **test-only intentionally cascade-
        // unsafe**. Production reducers MUST match specific tags
        // (see `WidgetCore::update`'s "Cascade discipline" section)
        // because a wildcard-emit reducer paired with a handler
        // that echoes its intent through the SCXML send channel
        // forms an infinite loop. The substrate calls `update`
        // twice per cycle (R51.168 incoming + R51.169 drain), and
        // this fixture catches both — that asymmetry is what the
        // R51.168/169 wiring tests assert. Do NOT copy this body
        // into a widget binding.
        vec![Command::new_static(
            "echo.reply",
            IntrospectValue::Text(intent.tag_str().to_string()),
            42,
        )]
    }
}

/// R995 §5.41 §2 #6 — the wide cluster the cross-backend
/// [`text_grid_consistency_buffer`] places at `(0, 1)`. `\u{D55C}` is the
/// Hangul syllable "han" (East Asian Wide); kept as an escape per the
/// non-ASCII-source-literal rule (raw glyphs only in doc strings).
pub const TEXT_GRID_WIDE_HEAD: &str = "\u{D55C}";

/// R995 §5.41 §2 #6 — the **cross-backend cell-render facts** of one
/// [`crate::scene::TextGrid`] cell: the backend-neutral structural truths the
/// Vello ([`paint_text_grid`]) and TUI ([`paint_text_grid_inner`]) painters
/// must *both* honour for the §2 #6 GUI / TUI dual to stay consistent.
///
/// These are derived from the shared [`GridBuffer`] model alone (by
/// [`expected_text_grid_cell_facts`]) — *not* from either painter's helpers —
/// so the cross-consistency tests are non-tautological: each backend's
/// observable output is asserted to agree with the model, and therefore with
/// the other backend.
///
/// Colour is **not** a fact here: it is deliberately backend-resolved (the
/// Vello backend resolves every [`TermColor`] through pinion's
/// [`Palette`](crate::term_grid::Palette); the TUI backend hands indexed /
/// default colours to the host terminal). The cross-consistency contract is
/// **cell-structure identity, not pixel identity** — which cell inks a glyph,
/// reads reversed, and forms a wide span.
///
/// [`paint_text_grid`]: this is the Vello adapter fn in `pinion-runtime`.
/// [`paint_text_grid_inner`]: this is the TUI fn in `pinion-tui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextGridCellFacts {
    /// A **visible** glyph is inked here: a non-[`CellWidth::Trailer`],
    /// non-whitespace cluster that is not concealed by SGR 8 `hidden`. Vello
    /// suppresses the glyph paint for a hidden / trailer / blank cell; the TUI
    /// leaves a trailer as the terminal spill cell and tags a hidden cell with
    /// `Modifier::HIDDEN` so the terminal conceals it — both render no visible
    /// ink, observed per backend.
    pub inks_glyph: bool,
    /// The cell reads **reversed** (effective): SGR 7 `reverse` XOR a block
    /// cursor sitting on this cell. The TUI toggles `Modifier::REVERSED`; Vello
    /// swaps the effective fg / bg (and the block cursor inverts the cell), so a
    /// reversed cell under the block cursor cancels back to its original
    /// colours in both.
    pub reversed: bool,
    /// The cell's display-width role: [`CellWidth::Narrow`],
    /// [`CellWidth::Wide`] (a 2-column head), or [`CellWidth::Trailer`] (the
    /// continuation column with no independent glyph). Both backends span a
    /// wide head across two columns — Vello via the glyph + a 2-column
    /// background, the TUI via the head grapheme + the terminal spill cell.
    pub width: CellWidth,
}

/// R995 §5.41 §2 #6 — derive the backend-neutral [`TextGridCellFacts`] for
/// cell `(col, row)` straight from the [`GridBuffer`] model. An out-of-bounds
/// coordinate yields the all-false default.
///
/// This is the single source of truth the Vello and TUI cross-consistency
/// tests both assert against. It reads only the model (the cell's
/// `cluster` / `width` / `attrs` and the grid cursor) — it never calls a
/// painter helper — so it cannot drift into tautology with either backend.
#[must_use]
pub fn expected_text_grid_cell_facts(buf: &GridBuffer, col: u16, row: u16) -> TextGridCellFacts {
    let Some(cell) = buf.cell(col, row) else {
        return TextGridCellFacts::default();
    };
    let cursor = buf.cursor();
    // Only a BLOCK cursor inverts the whole cell in both backends; Bar /
    // Underline diverge (Vello draws a shaped beam, the TUI has no sub-cell
    // shape — a documented R1.6 residue), so the fixture uses a block cursor.
    let block_cursor_here = cursor.visible
        && cursor.shape == CursorShape::Block
        && cursor.col == col
        && cursor.row == row;
    let trailer = cell.width == CellWidth::Trailer;
    TextGridCellFacts {
        inks_glyph: !trailer && !cell.cluster.trim().is_empty() && !cell.attrs.hidden,
        reversed: cell.attrs.reverse ^ block_cursor_here,
        width: cell.width,
    }
}

/// R995 §5.41 §2 #6 — the canonical 4×3 [`GridBuffer`] both backends render in
/// their cross-consistency regression tests (`pinion-tui` exact-buffer, and
/// `pinion-shell` headless-GPU), so one buffer pins the §2 #6 GUI / TUI dual
/// instead of two drifting per-backend buffers.
///
/// It covers every structural case the §5.41 model carries:
///
/// - **SGR attrs** — bold / italic+underline / blink+strikethrough on ASCII
///   glyphs (row 0), so each backend maps the same [`CellAttrs`] to its own
///   target (`Modifier` bits vs `FontWeight` / geometric rules).
/// - **reverse** — `(2, 0)` carries SGR 7 (effective-reversed without a
///   cursor); `(0, 2)` is reversed by the block cursor instead.
/// - **wide + trailer** — `(0, 1)` is a wide [`TEXT_GRID_WIDE_HEAD`] head on a
///   distinct ANSI-blue bg; `(1, 1)` is its trailer (carrying the head bg
///   across both columns).
/// - **hidden / blank / whitespace** — `(2, 1)` conceals a white "E";
///   `(1, 2)` is a lone space; `(3, 1)` is a blank — none ink.
/// - **synthesised glyph class (R1181)** — `(3, 2)` is a box-drawing cross
///   `┼`; it inks in the model, so it pins the R1180 GUI-geometry / TUI-symbol
///   dual across §2 #6 (the GPU ink probe stays valid — its corner is bg).
/// - **cursor** — a visible [`CursorShape::Block`] cursor sits on `(0, 2)`.
///
/// `(0, 0)` is a high-contrast white-on-black "A" and `(2, 1)` a concealed
/// white "E" so the headless-GPU backend has a font-robust ink / no-ink pair
/// (bright pixels on black) per the [[pinion-text-layout-tests-system-font-
/// debt]] discipline, while the wide span is proven font-independently by its
/// two-column blue background.
#[must_use]
pub fn text_grid_consistency_buffer() -> GridBuffer {
    let e = CellAttrs::empty;
    let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
    let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
    let red = TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00));
    let teal = TermColor::Rgb(Color::rgb(0x12, 0x34, 0x56));
    // ANSI blue (#0000ee) — the wide head + trailer bg, distinct from every
    // neighbour so the GPU backend can prove the two-column span by colour.
    let blue = TermColor::Indexed(4);

    let head = TermCell::new(TEXT_GRID_WIDE_HEAD, TermColor::Indexed(3), blue).wide();

    GridBuffer::new(4, 3)
        .with_row(
            0,
            [
                // High-contrast bold "A" — the GPU backend's ink probe.
                TermCell::new("A", white, black).with_attrs(e().with_bold(true)),
                TermCell::new("B", red, TermColor::Default)
                    .with_attrs(e().with_italic(true).with_underline(true)),
                // SGR reverse, no cursor — effective-reversed on its own.
                TermCell::new("C", TermColor::Indexed(2), blue).with_attrs(e().with_reverse(true)),
                TermCell::new("D", TermColor::Default, TermColor::Default)
                    .with_attrs(e().with_blink(true).with_strikethrough(true)),
            ],
        )
        .with_row(
            1,
            [
                head.clone(),
                head.trailer(),
                // Hidden white "E" on teal — the GPU backend's no-ink probe.
                TermCell::new("E", white, teal).with_attrs(e().with_hidden(true)),
                TermCell::blank(),
            ],
        )
        .with_row(
            2,
            [
                // The block cursor sits here, inverting this otherwise-plain "F".
                TermCell::new("F", TermColor::Indexed(5), TermColor::Default),
                // A lone space — present, but inks nothing.
                TermCell::new(" ", TermColor::Default, TermColor::Indexed(6)),
                TermCell::new("G", TermColor::Default, TermColor::Default),
                // R1181 §2 #6 — a box-drawing cross. It inks_glyph in the model
                // (a lone non-blank codepoint), so both backends must render it:
                // the TUI writes the symbol, and the Vello arm synthesises the
                // R1180 line geometry. Pins the new synthesis class across the
                // GUI / TUI dual (the GPU `inks` probe stays valid — the cell
                // corner is background, only the centre cross inks).
                TermCell::new("\u{253C}", TermColor::Default, TermColor::Default),
            ],
        )
        .with_cursor(GridCursor::new(0, 2, CursorShape::Block, true))
}

/// R1564 §5.15 (PINION-PR82) — assert `result` is a refusal whose **stated
/// reason** mentions `needle`.
///
/// The assertion shape a `Rejected` test needs after R1564, and it exists
/// because the obvious mechanical rewrite is a regression. Before this round a
/// refusal carried nothing, so `assert_eq!(r, Err(Rejected))` was as strong as
/// an assertion about one could be. After it, the same line only compiles as
/// `matches!(r, Err(Rejected(_)))` — **strictly weaker than what it replaced**,
/// because it passes for a refusal that names something else entirely. Forty-odd
/// call sites rewritten that way would have quietly traded the round's whole
/// subject for a green build.
///
/// `needle` should be the *distinguishing* clause, not the widget prefix every
/// reason from that surface shares.
///
/// # Panics
///
/// When `result` is `Ok`, when it is a non-`Rejected` failure, or when the
/// stated reason does not contain `needle`.
pub fn assert_refused_saying<T: std::fmt::Debug>(result: &Result<T, InvokeError>, needle: &str) {
    match result {
        Ok(value) => panic!("expected a refusal saying {needle:?}, got Ok({value:?})"),
        Err(err) => {
            let Some(reason) = err.reason() else {
                panic!("expected a stated refusal saying {needle:?}, got {err:?}");
            };
            assert!(
                reason.as_str().contains(needle),
                "refusal did not say {needle:?}; it said {reason:?}",
            );
        }
    }
}

/// R1565 §5.15 (PINION-PR82) — the [`assert_refused_saying`] peer for the
/// **write-state** channel: assert `result` is a refusal whose stated reason
/// mentions `needle`.
///
/// Separate from its sibling rather than generic over the error type, and the
/// reason is the asymmetry it exists to keep visible: on this channel only
/// [`InterveneError::OutOfRange`] carries a reason, because it is the only arm
/// whose meaning the variant does not determine. A helper that took "any error
/// with a reason" would read as though `ReadOnly` might grow one, and a test
/// written against it would quietly pass on a refusal that named nothing.
///
/// # Panics
///
/// When `result` is `Ok`, when it is a reason-free failure (`UnknownPath` /
/// `TypeMismatch` / `ReadOnly`), or when the stated reason omits `needle`.
pub fn assert_out_of_range_saying<T: std::fmt::Debug>(
    result: &Result<T, InterveneError>,
    needle: &str,
) {
    match result {
        Ok(value) => {
            panic!("expected an out-of-range refusal saying {needle:?}, got Ok({value:?})")
        }
        Err(err) => {
            let Some(reason) = err.reason() else {
                panic!("expected a stated out-of-range refusal saying {needle:?}, got {err:?}");
            };
            assert!(
                reason.as_str().contains(needle),
                "refusal did not say {needle:?}; it said {reason:?}",
            );
        }
    }
}

#[cfg(test)]
mod r55_g22_tests {
    //! R55.G.22 §5.49 — `assert_widget_view_carries_tag` helper
    //! regression. Two arms:
    //!
    //! 1. Pass arm — [`ButtonFixture`] paints a Container tagged
    //!    `"test_btn"` matching [`ButtonFixture::tag()`], so the
    //!    helper must accept it without panicking.
    //! 2. Fail arm — `UntaggedFixture` paints a Container with **no**
    //!    tag (R55.G.19 §5.49 `contains_tag` returns `false`), so
    //!    the helper must panic with the convention-violation
    //!    message.
    //!
    //! The fail arm pins the helper's `assert!` arm against an
    //! accidental tautology refactor (e.g. swapping the assertion
    //! for an always-true predicate would let the fail-arm test
    //! catch it).
    use super::{ButtonFixture, assert_widget_view_carries_tag};
    use crate::Frame;
    use crate::external::External;
    use crate::scene::{ContainerNode, Rect, Scene, TextNode};
    use crate::widget_core::WidgetCore;
    use crate::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};

    #[test]
    fn pass_arm_button_fixture_view_carries_tag() {
        // R55.G.22 §5.49 — pass arm. ButtonFixture::view paints a
        // Container with tag="test_btn" matching ButtonFixture::tag(),
        // so the helper accepts it. Doubles as a usage smoke test
        // showing the trait-bound call site.
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Idle, &Frame::default());
    }

    /// Negative fixture for the R55.G.22 fail arm — paints a
    /// Container with **no** tag, so [`Scene::contains_tag`] returns
    /// `false` and the helper must panic.
    struct UntaggedFixture;

    impl WidgetCore for UntaggedFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }

        fn tag() -> &'static str {
            "untagged_fixture"
        }

        fn read_state(_: &Scene) -> Self::State {
            ButtonState::Idle
        }

        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            // Deliberately tagless — exercises the helper's panic
            // arm. Mirrors the R55.G.19 Scene::contains_tag "Effect
            // leaf / Container without tag" negative regression
            // arm.
            Scene::Container(ContainerNode {
                rect: Rect::new(0, 0, 32, 48),
                tag: None,
                children: vec![Scene::Text(TextNode::default())],
                ..Default::default()
            })
        }

        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }

        fn title() -> &'static str {
            "Untagged"
        }
    }

    #[test]
    #[should_panic(expected = "view must contain a node tagged \"untagged_fixture\"")]
    fn fail_arm_untagged_fixture_panics() {
        // R55.G.22 §5.49 — fail arm. UntaggedFixture's view paints
        // a Container with `tag: None`, so contains_tag returns
        // false and the helper panics with the convention-
        // violation message. The `#[should_panic(expected = …)]`
        // arm also pins the error message text so the convention
        // reference (R55.G.17 §5.49) stays user-visible.
        assert_widget_view_carries_tag::<UntaggedFixture>(ButtonState::Idle, &Frame::default());
    }

    #[test]
    fn pass_arm_returns_without_observable_side_effects() {
        // R55.G.22 §5.49 — the helper installs a throwaway
        // `Owner::new()` scope per call so widgets whose view fn
        // observes `Owner::current()` (hello-button hover
        // animation per R51.147 §5.28) can be exercised without
        // requiring callers to wrap manually. Repeated calls must
        // remain independent — verify by exercising the pass arm
        // twice in sequence.
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Idle, &Frame::default());
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Hover, &Frame::default());
    }
}

#[cfg(test)]
mod r995_text_grid_facts_tests {
    //! R995 §5.41 §2 #6 — pin the model-derived [`TextGridCellFacts`] of the
    //! shared [`text_grid_consistency_buffer`] by hand, so the cross-backend
    //! Vello / TUI tests assert against a verified truth (not a tautology with
    //! either painter). Each assertion below is an *independent* read of the
    //! model — "(2, 0) carries SGR reverse so it reads reversed", etc.
    use super::{TextGridCellFacts, expected_text_grid_cell_facts, text_grid_consistency_buffer};
    use crate::term_grid::CellWidth;

    fn facts(col: u16, row: u16) -> TextGridCellFacts {
        expected_text_grid_cell_facts(&text_grid_consistency_buffer(), col, row)
    }

    #[test]
    fn glyph_bearing_cells_ink_and_blanks_do_not() {
        // Plain / attributed ASCII glyphs ink; the blank, the lone space, the
        // trailer, and the concealed cell do not.
        assert!(facts(0, 0).inks_glyph, "(0,0) 'A' inks");
        assert!(facts(3, 0).inks_glyph, "(3,0) 'D' inks");
        assert!(facts(2, 2).inks_glyph, "(2,2) 'G' inks");
        assert!(!facts(3, 1).inks_glyph, "(3,1) blank does not ink");
        assert!(!facts(1, 2).inks_glyph, "(1,2) lone space does not ink");
        // R1181 — the box-drawing cross is a synthesised glyph class that must
        // ink in both backends (Vello geometry / TUI symbol).
        assert!(facts(3, 2).inks_glyph, "(3,2) box-drawing cross inks");
        // The wide head carries a glyph; its trailer does not.
        assert!(facts(0, 1).inks_glyph, "(0,1) wide head inks");
        assert!(!facts(1, 1).inks_glyph, "(1,1) trailer carries no glyph");
    }

    #[test]
    fn hidden_cell_inks_nothing_even_though_it_has_a_cluster() {
        // (2,1) is "E" with SGR 8 hidden — a non-blank cluster, but concealed,
        // so no *visible* ink (Vello suppresses the glyph; the TUI tags it
        // HIDDEN and the terminal conceals it).
        assert!(
            !facts(2, 1).inks_glyph,
            "(2,1) hidden 'E' inks nothing visible"
        );
    }

    #[test]
    fn reverse_comes_from_sgr_or_the_block_cursor() {
        // (2,0) carries SGR 7 reverse and no cursor → reversed.
        assert!(facts(2, 0).reversed, "(2,0) SGR reverse reads reversed");
        // (0,2) is plain "F" but the block cursor sits on it → reversed.
        assert!(facts(0, 2).reversed, "(0,2) block cursor inverts the cell");
        // A plain glyph with neither is not reversed.
        assert!(!facts(0, 0).reversed, "(0,0) plain 'A' is not reversed");
        assert!(!facts(2, 2).reversed, "(2,2) plain 'G' is not reversed");
    }

    #[test]
    fn wide_head_and_trailer_are_classified() {
        assert_eq!(facts(0, 1).width, CellWidth::Wide, "(0,1) is the wide head");
        assert_eq!(
            facts(1, 1).width,
            CellWidth::Trailer,
            "(1,1) is the trailer"
        );
        assert_eq!(facts(0, 0).width, CellWidth::Narrow, "(0,0) is narrow");
    }

    #[test]
    fn out_of_bounds_is_all_false() {
        assert_eq!(facts(4, 0), TextGridCellFacts::default(), "col past width");
        assert_eq!(facts(0, 3), TextGridCellFacts::default(), "row past height");
    }
}

/// R1599 — the gate under the standing rule *"bump `PERSISTED_SCHEMA_VERSION`
/// when the persistence schema changes"*.
///
/// # Why this exists
///
/// That rule lived only in prose — a line in a project checklist that no
/// commit gate read. R1597 reshaped a binding's whole persisted blob (six
/// parallel fields collapsed into one `Document`, every key changed) and the
/// version stayed put until an end-of-session audit noticed. Nothing before
/// that could notice: it compiles, and the tests pass, because **the symptom is
/// on a user's disk** — an old blob is read as a corrupt file rather than as an
/// old file, and a change that only *adds* fields with `#[serde(default)]` is
/// worse still, because the old blob loads silently and is then wrong.
///
/// R1582 already recorded the general form of the fix: a prose warning is not a
/// gate. This is the gate.
///
/// # How it forces the bump
///
/// `history` is an **append-only** ledger of `(version, digest)` pairs, where
/// the digest is over the serialized bytes of a representative value. The
/// assertion is fourfold, and it is the combination that has no way out:
///
/// 1. today's digest must equal the **last** entry's — so any shape change goes
///    red until the ledger is appended to;
/// 2. the last entry's version must equal the live constant — so the append
///    must state the version that is actually shipping;
/// 3. no version may appear twice — so the append cannot reuse the old number;
/// 4. no digest may appear twice — so a shape cannot be quietly reverted onto a
///    different version, and the ledger stays a real history.
///
/// Together: change the shape, and the only way back to green is a new pair
/// whose version is one nobody has used, which is the bump.
///
/// # Panics
///
/// With a message naming which of the four it was, and the digest to append.
pub fn assert_persisted_shape<T: serde::Serialize>(
    label: &str,
    live_version: u32,
    sample: &T,
    history: &[(u32, u64)],
) {
    let bytes = serde_json::to_vec(sample).expect("the persisted sample serializes");
    let digest = fnv1a64(&bytes);

    let (last_version, last_digest) = *history.last().unwrap_or_else(|| {
        panic!("{label}: the shape history is empty; append ({live_version}, {digest:#018x})")
    });

    let mut seen_versions = std::collections::BTreeSet::new();
    let mut seen_digests = std::collections::BTreeSet::new();
    for (version, entry) in history {
        assert!(
            seen_versions.insert(*version),
            "{label}: schema version {version} appears twice in the shape \
             history — a new shape needs a NEW version, which is the bump this \
             gate exists to force"
        );
        assert!(
            seen_digests.insert(*entry),
            "{label}: digest {entry:#018x} appears twice in the shape history — \
             two versions cannot have the same persisted shape"
        );
    }

    assert_eq!(
        last_version, live_version,
        "{label}: the newest shape-history entry is version {last_version} but \
         PERSISTED_SCHEMA_VERSION is {live_version} — they must move together"
    );
    assert_eq!(
        digest,
        last_digest,
        "{label}: the persisted shape CHANGED (now {digest:#018x}, history ends \
         at {last_digest:#018x}).\n\
         Bump PERSISTED_SCHEMA_VERSION to {}, then RUN THIS AGAIN and append \
         ({}, <the digest it prints then>) to the shape history — the digest \
         above was taken over a sample carrying the OLD version number, so \
         bumping moves it. R1647 followed this message literally and had to \
         come back for the second value.\n\
         An old blob on a user's disk cannot be read by this build; the version \
         is what tells them so instead of reporting a corrupt file.",
        live_version + 1,
        live_version + 1
    );
}

/// FNV-1a, 64-bit. A digest rather than a hash map's hasher because it must be
/// **stable across processes and releases** — `DefaultHasher` explicitly is
/// not, so a pinned value computed with it would rot silently.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod persisted_shape_gate {
    use super::assert_persisted_shape;

    /// The gate's own counterfactual: it must go RED for each of the four ways
    /// a shape change can be smuggled past a version. A gate nobody has watched
    /// fail is a gate nobody knows works.
    #[test]
    fn r1599_the_gate_catches_every_way_around_it() {
        let sample = ("shape", 1_u32);
        let digest = {
            let bytes = serde_json::to_vec(&sample).unwrap();
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in &bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            hash
        };

        // The honest case passes.
        assert_persisted_shape("t", 3, &sample, &[(3, digest)]);

        let red = |version: u32, history: &[(u32, u64)]| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_persisted_shape("t", version, &sample, history);
            }))
            .is_err()
        };
        assert!(
            red(3, &[(3, digest ^ 1)]),
            "a changed shape must go red -- this is the whole gate"
        );
        assert!(
            red(4, &[(3, digest)]),
            "a version that moved without the ledger must go red"
        );
        assert!(
            red(3, &[(3, digest ^ 1), (3, digest)]),
            "reusing a version number must go red: the append IS the bump"
        );
        assert!(
            red(3, &[(2, digest), (3, digest)]),
            "two versions with one shape must go red"
        );
        assert!(red(3, &[]), "an empty ledger must go red");
    }
}
