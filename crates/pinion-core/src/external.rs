//! `External` primitive integration contract (§5.15, R16 slice 6).
//!
//! Eight-point contract ratified Round 7:
//!
//!   1. Backend support declaration — [`External::backends`]
//!   2. Repaint trigger ownership   — [`External::repaint_ownership`]
//!   3. Thread ownership            — [`External::thread_ownership`]
//!   4. Lifecycle event callbacks   — `on_mount` / `on_unmount` /
//!      `on_visibility_change` / `on_focus_change`
//!   5. Input forwarding policy     — [`External::handles_event`]
//!   6. DPI / resize notification   — `on_dpi_change` / `on_resize`
//!   7. Async state change channel  — [`External::poll_state`] (pull form)
//!   8. Symbolic introspection      — *opt-in*, lands as a separate
//!      sub-trait in a later slice
//!
//! Items 1-7 are mandatory; items 1-3 are required (no default), items
//! 4-7 ship sensible no-op defaults so authors only override what they
//! need.
//!
//! The trait is **dyn-safe** by construction (all methods take `&self`
//! or `&mut self`, no associated consts, no `Self`-returning methods).
//! This keeps `Box<dyn External>` available for heterogeneous storage
//! when the §5.15 scene-tree integration lands.
//!
//! `StubExternal` is a ref-impl: Gui-only, framework-driven repaint,
//! UI-thread synchronous. It exists to anchor the contract semantically
//! and to give tests/examples a baseline.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use serde_json::value::RawValue;

use crate::Event;
use crate::input::{GesturePhase, Modifiers, PointerKind, RawPointerButton};
use crate::intent::Intent;

thread_local! {
    /// Tag -> the size the framework last announced for that surface.
    static SURFACE_SIZES: RefCell<BTreeMap<String, (u32, u32)>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// ★★★ R1684.4 — **the size a surface was last laid out at, readable from
/// anywhere.**
///
/// # Why this exists
///
/// A widget could only learn its own size two ways, and both have a scope
/// attached. `use_viewport_size()` answers inside an [`Owner`] scope and
/// nowhere else; `External::on_resize` is handed the number once and then it is
/// the widget's problem to keep. The framework itself calls a widget's
/// `pointer_press`, `pointer_move`, `invoke` and `query` **outside** any owner
/// scope — so a screen that hit-tests its own surface has to remember the size
/// by hand, in a cache of its own, in every such screen.
///
/// That is not hypothetical. `hello-node-lab` carries three hand-written caches
/// of framework facts for exactly this reason (a scroll state, an edit buffer,
/// and — until this function existed — its window size), and the third was
/// found by a person maximising the window and reporting that the settings rows
/// had stopped selecting: the paint reflowed, the hit test went on using the
/// size the screen was designed at, and the error grew with distance from the
/// origin. The mature retained-mode toolkits this project is judged against have
/// no such split — a widget's own size is plain state there, answerable in every
/// callback.
///
/// So it is plain state here too. `announce_external_sizes` records every
/// surface it tells, on the same pass and from the same rectangle, so the size a
/// widget reads and the size its pointer fractions are fractions OF are one
/// derivation.
///
/// Answers `None` for a tag that has never been painted — which is the truthful
/// answer, and the reason a caller with a design size to fall back on must say
/// so itself rather than being handed a plausible number.
///
/// [`Owner`]: crate::reactive::Owner
#[must_use]
pub fn surface_size(tag: &str) -> Option<(u32, u32)> {
    SURFACE_SIZES.with(|sizes| sizes.borrow().get(tag).copied())
}

/// Record what the framework just announced to a surface — called by the layer
/// that calls [`External::on_resize`], never by a widget.
///
/// ★ Public because the announcer lives in `pinion-runtime`, one crate up. It
/// is the framework's own bookkeeping: a widget calling this would be inventing
/// a size rather than reading one.
pub fn record_surface_size(tag: &str, width: u32, height: u32) {
    SURFACE_SIZES.with(|sizes| {
        sizes
            .borrow_mut()
            .insert(tag.to_owned(), (width.max(1), height.max(1)));
    });
}

/// Forget a surface that is no longer painted, so a stale size cannot be read
/// back for something that is not on screen.
pub fn forget_surface_size(tag: &str) {
    SURFACE_SIZES.with(|sizes| {
        sizes.borrow_mut().remove(tag);
    });
}

/// ★★★★★ R1700 — **the size a surface lays itself out against, spelled once.**
///
/// # Why the framework owns the whole expression and not just the number
///
/// [`surface_size`] gave the *fact* in R1684.4. What it did not give is the
/// *policy* around it, and the policy is four lines of subtle case analysis:
/// take a tracked read inside a view so the view re-runs on a resize, take the
/// recorded announcement outside one, honour the layout's own floor, and fall
/// back to the design size only where nothing has been painted yet.
///
/// Three screens wrote those four lines separately. Measured at R1700, on the
/// three surfaces of one application:
///
/// | screen | what it answered off a view scope |
/// |---|---|
/// | node lab | the recorded surface size — correct |
/// | capture viewer | **the design constant** — wrong at every other size |
/// | shell | its own `Signal`, kept in the screen's state — correct, and a
/// |   | second spelling of what this function does |
///
/// The capture viewer's copy was the one that was wrong, and it was wrong in
/// the direction that hurts: its paint reflowed to the live window while its
/// hit test went on resolving against 1440x900, so **166 of the 166 painted
/// rectangles that moved under a resize stopped being pressable where they were
/// drawn**. A person reported it twice, and every gate was green both times,
/// because an in-process fixture paints and hit-tests inside one owner scope
/// where the two halves cannot disagree.
///
/// A policy re-derived per consumer has as many versions as consumers, and this
/// one had three versions and one defect. So it is one function.
///
/// # The floor is a parameter because it is the caller's fact
///
/// `floor` is the smallest size the caller's layout is *defined* at — below it a
/// layout that stops shrinking paints its design arrangement and clips, so that
/// is what the hit test must resolve against too. Passing it here rather than
/// clamping at the call site is the point: the paint half and the gesture half
/// then apply the same floor by construction, which is the class of defect this
/// function exists to remove.
///
/// # Why the two branches read two different quantities
///
/// Inside a view the WINDOW is the live fact and the surface's own rectangle is
/// not: the shell sets the viewport before the frame, while the rectangle comes
/// out of the layout this very view is feeding, so [`surface_size`] there is
/// last frame's answer and would lay the first frame after a resize out at the
/// old size. The viewport read is also the tracked one, which is what makes the
/// view re-run on the next resize at all.
///
/// Outside a view the viewport signal cannot be read — that is the whole
/// problem — and the surface's rectangle from the last painted frame is both
/// available and the *better* answer: it is the very rectangle a pointer
/// fraction is a fraction of, so a press and the layout it is resolved against
/// come from one derivation.
///
/// ★ Stated limit: for a surface that does not fill its window the in-view
/// branch answers the window, which is larger. Nothing can do better there —
/// during its own view a surface's rectangle has not been decided yet — and
/// none of this project's self-hit-testing screens is in that position. A
/// surface that is would read its rectangle off the previous frame instead, and
/// this function is where that would be decided rather than in three screens.
///
/// The mature retained-mode toolkits this project is judged against answer a
/// widget's own size from every callback with no scope attached (measured at
/// 6.11: a live `1200x700` read inside a press handler after a resize). This is
/// that property, plus the layout policy those toolkits leave to each widget.
///
/// # ★★★★★ R1711 — the floor is applied per AXIS, and it used not to be
///
/// The rule was "if either axis is under its floor, lay out at the design
/// size", which throws away the live extent of the axis that was *fine*. It is
/// not a corner case: measured on the node lab through the new
/// [`size_floor`](crate::size_floor) read, a window 1506 wide and 360 tall —
/// both extents individually reachable — lost **nine** marks, because dropping
/// under the floor on WIDTH also un-shortened the layout to its 900-pixel
/// design height and pushed the launch-gate panel out of a 360-tall window.
/// Narrowing a window made it lose content vertically.
///
/// Clamping each axis on its own is both simpler and the rule the shell already
/// applies to a window's own size
/// ([`SizeBounds`](crate::size_grant::SizeBounds), whose floor and ceiling are
/// per-axis for the same reason). What the two branches mean is unchanged:
/// below its floor a layout stops shrinking and the window clips, and the hit
/// test resolves against the same clamped extent because it calls this same
/// function.
///
/// "Nothing has painted yet" stays the one case that answers the design size,
/// and a zero on either axis is that case — R1006's `(0, 0)` is "viewport
/// unknown", and a window of no extent is not a size to lay anything out in.
#[must_use]
pub fn layout_size(tag: &str, floor: (u32, u32), design: (u32, u32)) -> (u32, u32) {
    let live = match crate::reactive::Owner::current() {
        Some(_) => Some(crate::reactive::use_viewport_size()),
        None => surface_size(tag),
    };
    match live {
        Some((w, h)) if w > 0 && h > 0 => (w.max(floor.0), h.max(floor.1)),
        _ => design,
    }
}

/// ★★★★★ R1714 — **the pixel a pointer fraction names, in the frame the
/// screen's layout is stated in.**
///
/// # The expression three screens were writing out by hand
///
/// [`External::pointer_move`] hands a *fraction* of the surface and not the
/// surface, so a screen that hit-tests its own rectangles has to find the basis
/// somewhere else and multiply. R1684.4 gave it the basis ([`surface_size`]);
/// what it did not give it is the multiplication, so the same three lines —
/// clamp, multiply, cast — are written at every point a screen turns a pointer
/// into pixels. Measured on the node lab alone: `pointer_move`, `wheel` and
/// `wheel_intent`, three copies in one file.
///
/// # And why it is no longer only a multiplication
///
/// A window whose policy declares [`Recourse::Pan`](crate::shrink::Recourse::Pan)
/// is a viewport onto a layout that is bigger than it, so the pixel a fraction
/// names in the WINDOW and the pixel it names in the LAYOUT differ by the pan.
/// Measured before this function existed: with the node lab panned 24 pixels,
/// `scene/pointer_target` — which asks the screen what a press inside each
/// painted rectangle addresses — went from 46 deliverable rectangles to 28, and
/// eight tagged rectangles became addressable at no point inside themselves.
/// The screen was hit-testing the layout with a window coordinate.
///
/// Adding the pan is one line, and the reason it is *this* line rather than one
/// in each screen is the reason [`layout_size`] is: a screen that forgets it
/// has a hit test that is right at one offset and wrong at every other, which
/// is a defect nothing but a person moving the window would find.
#[must_use]
pub fn layout_point(tag: &str, at: (f32, f32)) -> (u32, u32) {
    let (w, h) = surface_size(tag).unwrap_or((1, 1));
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window fraction times a window size is a pixel inside it"
    )]
    let pixel = |frac: f32, extent: u32| -> u32 { (frac.clamp(0.0, 1.0) * extent as f32) as u32 };
    into_layout(tag, (pixel(at.0, w), pixel(at.1, h)))
}

/// ★★★★★ R1714 — a point in the **window's** frame, in the frame the screen's
/// layout is stated in.
///
/// The other door to [`layout_point`]'s rule, for the callbacks that are handed
/// pixels rather than a fraction —
/// [`External::target_at`] is the one that made this necessary: the framework
/// asks it what a press at a painted rectangle's centre addresses, in the
/// coordinates the paint is published in, and a panned screen's paint is
/// published in the window's frame while its own rectangles are stated in the
/// layout's.
///
/// Measured with this missing while [`layout_point`] was already in place:
/// with the node lab panned 400 pixels, `scene/pointer_target` answered
/// **1** deliverable rectangle of 57 and called 26 unreachable — the pointer
/// path was right and the by-name path was not, which is precisely the split
/// that gate exists to find.
///
/// The identity for a screen that does not pan, so a caller can put it on every
/// such point without asking whether this screen is one.
#[must_use]
pub fn into_layout(tag: &str, at: (u32, u32)) -> (u32, u32) {
    let (pan_x, pan_y) = crate::shrink::window_pan(tag);
    (at.0.saturating_add(pan_x), at.1.saturating_add(pan_y))
}

/// ★★★★★ R1700 — what a surface says a press addresses, in the surface's own
/// vocabulary.
///
/// See [`External::target_at`] for the contract and for why the framework asks
/// the same question twice.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerTarget {
    /// This surface does not resolve presses to named things.
    ///
    /// The default, and deliberately **not** the same as [`Self::Nothing`]: a
    /// surface that cannot answer is not a surface that answered "nothing
    /// there". Collapsing the two would let a screen nobody checked read as a
    /// screen that checked out — the shape R1691 names "a total is satisfied by
    /// declaring everything silent".
    Unanswered,
    /// The surface answered, and nothing addressable is there.
    Nothing,
    /// What a press addresses, as the word this surface's own wire answers
    /// with. It is the surface's vocabulary and not a tag, because the two are
    /// not the same set: several painted rectangles can address one thing (a
    /// label inside its row) and one thing can be addressed with no rectangle
    /// of its own at all.
    Word(String),
}

impl PointerTarget {
    /// The word, where one was named.
    #[must_use]
    pub fn word(&self) -> Option<&str> {
        match self {
            Self::Word(word) => Some(word.as_str()),
            Self::Unanswered | Self::Nothing => None,
        }
    }

    /// Whether the surface answered at all — the census partition, so that
    /// "did not answer" and "answered nothing" stay two facts.
    #[must_use]
    pub fn answered(&self) -> bool {
        !matches!(self, Self::Unanswered)
    }
}

/// Render backends an `External` may declare support for (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    /// GPU-backed window per §5.9 trait-based Renderer.
    Gui,
    /// Text terminal rendering per §5.9 dual backend.
    Tui,
    /// JSON-RPC symbolic surface per §5.7 §5.12.
    Rpc,
}

/// What the framework should do when a scene targets a backend the
/// `External` does not support (§5.15 item 1 fallback policy).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendFallback {
    /// Reject the scene at composition time — per §5.15 caveat,
    /// non-conforming `External` should not silently break.
    Reject,
    /// Skip the `External` (renders as an empty placeholder); useful
    /// for optional content like a video viewport when running headless.
    Skip,
}

/// Backend support declaration (§5.15 item 1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSupport {
    /// Backends the `External` can dispatch into. Order is not
    /// significant; uniqueness is the implementor's responsibility.
    pub supported: &'static [Backend],
    /// Policy for unsupported backends.
    pub fallback: BackendFallback,
}

impl BackendSupport {
    #[must_use]
    pub const fn new(supported: &'static [Backend], fallback: BackendFallback) -> Self {
        Self {
            supported,
            fallback,
        }
    }

    /// Returns `true` when this `External` declares support for `backend`.
    #[must_use]
    pub fn supports(&self, backend: Backend) -> bool {
        self.supported.contains(&backend)
    }
}

/// Who drives repaint scheduling for an `External` (§5.15 item 2).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintOwner {
    /// Framework decides when to repaint (layout-driven; default for
    /// static content like styled boxes embedding an SVG).
    Framework,
    /// `External` owns its render loop (game viewport, video player);
    /// the framework just composes the resulting surface.
    External,
}

/// Where the `External` performs its work (§5.15 item 3).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadOwnership {
    /// `External` runs synchronously on the UI thread.
    UiThreadSync,
    /// `External` owns a worker thread; framework communicates via a
    /// sync channel. State pushes use [`External::poll_state`] today;
    /// push-form variant lands when §6.3 async boundary settles.
    OwnThread,
}

/// Opaque state-update payload from an `External` to the framework
/// (§5.15 item 7). The concrete schema is settled in a later slice —
/// today this is a marker so the contract surface stays stable.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct StateUpdate;

// ---------------------------------------------------------------------------
// §5.15 item 8 — Optional symbolic introspection (opt-in sub-trait).
// ---------------------------------------------------------------------------

/// R1353 §5.12 §2 #2 — where a **parametric** path's argument is allowed to
/// come from, so a client can enumerate valid arguments instead of guessing
/// them.
///
/// The domain is expressed as a *reference to another path on the same
/// surface*, never as a literal bound. A surface's shape is live (a grid gains
/// columns, a tree collapses), so a literal `0..8` baked into a schema would be
/// stale the moment the model changed — and a schema that lies is worse than one
/// that says nothing. Pointing at `cols` instead means the bound is always read
/// fresh, from the one place that owns it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgDomain {
    /// Valid arguments are the indices `0..query(<count_path>)` — the argument
    /// is an offset into a sequence whose length the surface publishes.
    /// (`width.<col>` with `IndexOf("cols")`: read `cols`, then `width.0` …
    /// `width.<cols-1>` are exactly the answerable paths.)
    IndexOf(&'static str),
    /// Valid arguments are the values the surface lists at `<values_path>` —
    /// the argument is a key, not an offset (a panel id, a voice id).
    ValuesOf(&'static str),
    /// R1638 — the argument's valid values are this **closed literal set**.
    ///
    /// The one case the "never a literal bound" rule above does not cover, and
    /// the distinction is worth stating precisely because the rule is otherwise
    /// right. That rule is about bounds derived from the **model**: a column
    /// count, a row set, a voice list. Those are live, so a literal baked into a
    /// schema is stale the moment the model moves.
    ///
    /// A **verb vocabulary is not a fact about the model** — it is a fact about
    /// the code, it changes only when the code changes, and the schema is code,
    /// so the two move in the same commit. `set_voice_policy` accepts exactly
    /// the two spellings `VoicePolicy::from_wire` admits and no model can add a
    /// third.
    ///
    /// **Tie the set to its definition rather than hand-writing it.** The
    /// in-tree form is a `const WIRE_NAMES: [&str; Self::ARMS]` beside the enum,
    /// whose length comes from `#[derive(VariantCensus)]`, so a new variant is a
    /// build failure rather than a silently short list — R1630's ratchet, which
    /// exists because a hand-written vocabulary is a census disconnected from
    /// its definition. A bare literal here would be exactly that census.
    ///
    /// The reference constrains an argument this way only when it is a C++
    /// enum-typed parameter (its meta-enum); this constrains any argument,
    /// including a string one.
    OneOf(&'static [&'static str]),
    /// R1642 — a closed vocabulary where **the value chosen decides what the
    /// rest of the call looks like**: each [`ArgCase`] names one admissible
    /// value and the arguments that value brings with it.
    ///
    /// # The shape this exists for
    ///
    /// `arrange` is spelled `pass:axis:tail`, and the third segment is a closed
    /// edge vocabulary after `align`, an integer after `stack`, and **absent**
    /// after `distribute` and `straighten`. Flattened
    /// into one argument list that can only be described as
    /// `{type: "string", domain: open, optional: true}` — which is what R1638
    /// declared, and which admits three calls the surface refuses
    /// (`align:horizontal` with the segment elided, `align:horizontal:17`,
    /// `stack:horizontal:start`) plus one it accepts and silently ignores
    /// (`distribute:horizontal:start`). Silence would have been honest; that
    /// declaration was not silent, it was wrong, which is the error direction
    /// R1602 records as the expensive one.
    ///
    /// A flat list cannot be made right here, because the dependency is not a
    /// missing detail but a different *arity*: `item`'s `add:in:1[:label]` and
    /// `move:in:2:0` do not have the same number of segments, and no single
    /// positional list describes both.
    ///
    /// # Why the case table hangs off the discriminant
    ///
    /// Because "which values may I pass" and "what does that value then
    /// require" are one fact, and a declaration that splits them lets them
    /// drift — the argument R1593 made for links, where legality and conversion
    /// became one declaration precisely so "may I draw this" and "what arrives
    /// if I do" could not disagree. Reading a domain of `OneOfWith` tells a
    /// client both halves at once, in one place, with nothing to correlate.
    ///
    /// # Composition rule
    ///
    /// A case's [`then`](ArgCase::then) arguments come **after every argument
    /// the field declares**, in the case's own order — so a field's `args` hold
    /// what every case shares and the case holds what only it takes.
    /// `arrange` declares `[pass, axis]` and `align` brings `edge`, which
    /// expands to `pass:axis:edge`. At most one argument per field may carry
    /// this domain, since two would leave the append order ambiguous; a
    /// discriminant may not be [`optional`](SchemaArg::optional), since a case
    /// cannot be selected by an absent value.
    /// `r1642_a_discriminant_is_singular_and_required` and
    /// `r1642_declared_case_arguments_do_not_shadow` hold every declaration in
    /// the workspace to all of it.
    ///
    /// # Where the reference stands
    ///
    /// The meta-object cannot express this at all: a parameter list is
    /// generated from one C++ signature, so a conditional argument has to
    /// become *separate methods* — and the toolkit does exactly that, spelling
    /// the eleven alignment commands R1631 folded into parameters as eleven
    /// names. Its one concession is `Cloned`, the synthetic shorter overloads a
    /// default argument generates, which enumerates arities but cannot say that
    /// two of them belong to different values of the same parameter. So a
    /// client there discovers eleven unrelated verbs; here it discovers one
    /// verb and its four cases, which is the same information plus the fact
    /// that they are one operation.
    OneOfWith(&'static [ArgCase]),
    /// The surface publishes nothing a client can enumerate the argument from.
    ///
    /// **Worth suspicion at every use, and common enough to matter**: this is
    /// the majority variant today, and each one is a client still guessing. Two
    /// honest reasons to reach for it, both real in-tree:
    ///
    /// * the bound exists but is **not expressible** — `datepicker`'s
    ///   `state.<day>` runs `1..=days` (one-based, inclusive), which
    ///   [`IndexOf`](Self::IndexOf)'s `0..count` cannot state, so claiming it
    ///   would be false;
    /// * the bound exists but lives on **another surface** — `hello-tree-grid`'s
    ///   `cell_at.<pos>.<col>` bounds `pos` by a visible-row count that belongs
    ///   to the tree-state external, not to this one.
    ///
    /// What it must NOT be is a default. `Open` on a surface that publishes the
    /// count three lines up is an *affirmative false statement* — it tells an
    /// agent there is nothing to know, which is worse than the pre-R1353 silence
    /// it replaced, because now it carries a schema's authority.
    Open,
}

impl ArgDomain {
    /// The `$schema` wire form of this domain.
    ///
    /// Rendered **here**, in the crate that defines the enum, rather than in
    /// `pinion-rpc` where `$schema` is assembled. `ArgDomain` is
    /// `#[non_exhaustive]`, so a match in any other crate needs a `_` arm — and
    /// a `_` arm would render a future variant as some silent placeholder,
    /// telling clients "unconstrained" about a domain that constrains. Inside
    /// the defining crate the match is exhaustive, so adding a variant fails to
    /// compile here until its wire form is decided. The type owns its wire form;
    /// the transport only forwards it.
    #[must_use]
    pub fn to_wire(self) -> serde_json::Value {
        match self {
            Self::IndexOf(count_path) => {
                serde_json::json!({ "kind": "index_of", "count_path": count_path })
            }
            Self::ValuesOf(values_path) => {
                serde_json::json!({ "kind": "values_of", "values_path": values_path })
            }
            Self::OneOf(values) => serde_json::json!({ "kind": "one_of", "values": values }),
            Self::OneOfWith(cases) => serde_json::json!({
                "kind": "one_of_with",
                "cases": cases.iter().map(ArgCase::to_wire).collect::<Vec<_>>(),
            }),
            Self::Open => serde_json::json!({ "kind": "open" }),
        }
    }

    /// The case table — non-empty for [`OneOfWith`](Self::OneOfWith) alone, and
    /// empty for every other domain, which is what "this argument decides
    /// nothing about the rest of the call" means.
    ///
    /// Matched exhaustively rather than with a `_` arm even though this is the
    /// defining crate, so a future domain that also carries cases has to be
    /// listed here instead of silently answering "none" — the same reasoning
    /// [`to_wire`](Self::to_wire) gives for its own match, applied to the
    /// question a gate asks.
    #[must_use]
    pub const fn cases(self) -> &'static [ArgCase] {
        match self {
            Self::OneOfWith(cases) => cases,
            Self::OneOf(_) | Self::IndexOf(_) | Self::ValuesOf(_) | Self::Open => &[],
        }
    }
}

/// R1642 — one admissible value of a discriminant argument, together with the
/// arguments choosing it brings.
///
/// See [`ArgDomain::OneOfWith`] for what this is for, where the case's
/// arguments sit in the call, and what the reference can and cannot say.
///
/// A struct rather than a `(&str, &[SchemaArg])` pair for the reason
/// [`SchemaField`] is not the pair it replaced: the pair has no room for the
/// next dimension, and this one is already short of an obvious one — a case has
/// no place to say what it *returns*, though `arrange`'s four passes answer
/// four differently-shaped reports. Named fields with a `const` constructor mean
/// that lands additively.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgCase {
    /// The discriminant value that selects this case — one member of the closed
    /// vocabulary, in the same spelling the surface's own parser admits.
    pub value: &'static str,
    /// The arguments this case adds, in wire order, **after** every argument the
    /// field declares. Empty when choosing this value adds nothing, which is a
    /// claim (`distribute` takes no tail) rather than silence.
    pub then: &'static [SchemaArg],
}

impl ArgCase {
    /// The fill value for a fixed-size array a `const fn` builds before
    /// overwriting every slot — the same role, and the same hazard, as
    /// [`SchemaField::EMPTY`]. An empty value matches no discriminant, so a slot
    /// left un-overwritten is a visibly blank case rather than a plausible one.
    pub const EMPTY: Self = Self::new("", &[]);

    /// One case: the value, and what choosing it adds.
    #[must_use]
    pub const fn new(value: &'static str, then: &'static [SchemaArg]) -> Self {
        Self { value, then }
    }

    /// The `$schema` wire form — rendered here for the reason
    /// [`ArgDomain::to_wire`] gives.
    #[must_use]
    pub fn to_wire(&self) -> serde_json::Value {
        serde_json::json!({
            "value": self.value,
            "then": self.then.iter().map(SchemaArg::to_wire).collect::<Vec<_>>(),
        })
    }

    /// Whether this case adds exactly `count` arguments, the last of them
    /// optional iff `optional`.
    ///
    /// A `const fn` because it exists to be called from a `const` assertion at a
    /// declaration site, where the case table is composed by hand out of a
    /// mapping the owning crate answers. `ArrangePass::tail()` says whether a
    /// pass reads a trailing segment and `ArrangeTail::required()` says whether
    /// it may be elided; the `SchemaArg` that states those two facts on the wire
    /// has to be spelled beside the schema, because a model crate that must not
    /// depend on the framework cannot name one. That leaves one fact written in
    /// two places — the exact drift this round is repairing one level up — so the
    /// site asserts the agreement at compile time and a changed `required()`
    /// becomes a build failure instead of a schema that lies.
    ///
    /// `count` is a length rather than a shape because that is all a caller can
    /// check without naming argument types it does not own; the *names* and
    /// domains are checked over the wire by the round's conformance demo, which
    /// drives every call the declaration admits.
    #[must_use]
    pub const fn adds(&self, count: usize, optional: bool) -> bool {
        if self.then.len() != count {
            return false;
        }
        count == 0 || self.then[count - 1].optional == optional
    }
}

/// R1353 §5.12 §2 #2 — the argument a **parametric** [`SchemaField`] takes.
///
/// See [`SchemaField::parametric`] for what parametric means and why it must be
/// declared rather than left implicit in the `query` impl.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaArg {
    /// What the argument means, for a human and for an AI's prompt: `"col"`,
    /// `"pos"`, `"id"`. Not a type — [`Self::ty`] is.
    pub name: &'static str,
    /// The argument's type tag, in the same vocabulary as [`SchemaField::ty`]
    /// (`"int"`, `"string"`).
    pub ty: &'static str,
    /// Where the answerable arguments come from.
    pub domain: ArgDomain,
    /// R1638 — the call is well-formed without this argument.
    ///
    /// Only meaningful on the action channel: a parametric READ's placeholders
    /// are all required by the template's own shape (a missing segment makes a
    /// different path, or none). On an action it is the difference between
    /// `invoke("send", "3:PointerUp")` and `invoke("send", "3:PointerUp::l")`,
    /// which the send wire elides only from the END — so an optional argument
    /// may not precede a required one, and
    /// `r1638_optional_arguments_are_a_suffix` holds every declaration in the
    /// workspace to that.
    pub optional: bool,
}

impl SchemaArg {
    /// An index argument bounded by the sequence length at `count_path`
    /// (`ArgDomain::IndexOf`) — the overwhelmingly common shape.
    #[must_use]
    pub const fn index(name: &'static str, count_path: &'static str) -> Self {
        Self {
            name,
            ty: "int",
            domain: ArgDomain::IndexOf(count_path),
            optional: false,
        }
    }

    /// A key argument drawn from the values listed at `values_path`
    /// ([`ArgDomain::ValuesOf`]).
    #[must_use]
    pub const fn key(name: &'static str, ty: &'static str, values_path: &'static str) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::ValuesOf(values_path),
            optional: false,
        }
    }

    /// R1638 — an argument drawn from a closed literal vocabulary
    /// ([`ArgDomain::OneOf`]). Pass a `const` tied to the definition that owns
    /// the set, never a literal spelled at the call site.
    #[must_use]
    pub const fn one_of(
        name: &'static str,
        ty: &'static str,
        values: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::OneOf(values),
            optional: false,
        }
    }

    /// An argument the surface does not constrain ([`ArgDomain::Open`]).
    #[must_use]
    pub const fn open(name: &'static str, ty: &'static str) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::Open,
            optional: false,
        }
    }

    /// R1639 — the argument a **bare-event `send`** takes: one statechart
    /// event name, from that widget's own closed vocabulary.
    ///
    /// Eleven widgets in this tree spell `send` that way — a plain
    /// `invoke("send", "PointerDown")` decoded by
    /// [`require_event`](crate::widget_core::require_event) — and R1638 could
    /// describe none of them, because the set of names is per widget and was
    /// reachable only as a runtime `Vec`. `#[derive(WidgetEventName)]` now
    /// emits it as `DRIVABLE_NAMES`, projected from the very const `from_name`
    /// gates on, so what a client discovers IS what the parser admits.
    ///
    /// A named helper rather than a whole `SchemaField` constructor because a
    /// `const fn` cannot build the `&'static [SchemaArg]` a field needs out of
    /// its own parameter — a function argument is not promotable. The call site
    /// writes the one-element slice in a `const` block, which is where the
    /// promotion is legal:
    ///
    /// ```ignore
    /// SchemaField::action_with(
    ///     "send",
    ///     "string",
    ///     ArgForm::Scalar,
    ///     const { &[SchemaArg::event(&ButtonEvent::DRIVABLE_NAMES)] },
    /// )
    /// ```
    #[must_use]
    pub const fn event(names: &'static [&'static str]) -> Self {
        Self::one_of("event", "string", names)
    }

    /// R1638 — the same argument, marked as one a well-formed call may omit.
    ///
    /// A builder rather than a fourth constructor because optionality is
    /// orthogonal to where the values come from: every domain can be optional,
    /// and three more constructors would be the product of two axes spelled out.
    #[must_use]
    pub const fn optional(self) -> Self {
        Self {
            optional: true,
            ..self
        }
    }

    /// R1642 — the discriminant argument of a conditional call: a closed
    /// vocabulary in which each value brings its own trailing arguments
    /// ([`ArgDomain::OneOfWith`]).
    ///
    /// Pass a `const` derived from the definition that owns the mapping, never a
    /// table spelled at the call site — R1630's ratchet, and here it buys more
    /// than a length check: the *cases* are the mapping, so a hand-written table
    /// can disagree with the dispatcher about what a value implies, not merely
    /// about how many values there are.
    #[must_use]
    pub const fn one_of_with(
        name: &'static str,
        ty: &'static str,
        cases: &'static [ArgCase],
    ) -> Self {
        Self {
            name,
            ty,
            domain: ArgDomain::OneOfWith(cases),
            optional: false,
        }
    }

    /// The `$schema` wire form of this argument.
    ///
    /// R1642 moved this out of `pinion-rpc`, where it was a free function beside
    /// `$schema`'s assembly. Two reasons, and the second is the forcing one:
    /// this struct is `#[non_exhaustive]`, so a renderer in another crate cannot
    /// be held to covering a field added later — and `ArgDomain::OneOfWith`
    /// makes the form **recursive**, so [`ArgDomain::to_wire`] must be able to
    /// render an argument, which it can only do from here. The transport
    /// forwards; the types render themselves.
    #[must_use]
    pub fn to_wire(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("name".to_owned(), serde_json::Value::from(self.name));
        obj.insert("type".to_owned(), serde_json::Value::from(self.ty));
        obj.insert("domain".to_owned(), self.domain.to_wire());
        // Present only when true, for the same reason `channel` is: the absent
        // key is the common case and a reader that predates it must keep seeing
        // the shape it knew.
        if self.optional {
            obj.insert("optional".to_owned(), serde_json::Value::Bool(true));
        }
        serde_json::Value::Object(obj)
    }
}

/// R1638 §5.12 §2 #2 — **how** a declared field's arguments are carried, and
/// whether it has said.
///
/// [`SchemaArg`] says what an argument means and where its values come from. It
/// cannot say where the argument *goes*, and until R1638 nothing did: a read's
/// arguments ride the path, an action's ride `scene/invoke`'s `args`, and the
/// action channel carries them three different ways in this tree alone. A
/// client handed `{"path": "arrange", "args": [{"name": "axis"}, …]}` with no
/// form has to guess between a bare string, a JSON object and a delimited
/// command — and R1352 already measured what a guess about the argument channel
/// costs.
///
/// # Why silence is an arm rather than an absent field
///
/// [`Undeclared`](Self::Undeclared) is the default, and it is the honest state
/// of most of this workspace: 487 actions are declared and the overwhelming
/// majority take an argument no declaration describes. Making an empty `args`
/// mean "takes nothing" would have converted every one of those into an
/// affirmative false statement — the error direction R1602 records as the
/// expensive one, because a wrong `have` inflates silently while a wrong
/// `absent` self-corrects the moment somebody reaches for it.
///
/// So an action that has not said publishes no `args` at all, and one that has
/// said publishes both the form and the arguments. [`Nullary`](Self::Nullary) is
/// how a surface says "takes nothing" *affirmatively*, which is a different
/// claim from having not said.
///
/// **The reference distinguishes these two by construction and cannot express
/// the first**: a meta-method's parameter list is generated from the signature,
/// so `parameterCount() == 0` is always a claim and "undeclared" is not a state
/// it has. It is a state a hand-written declaration very much has, and pretending
/// otherwise is what R1637 spent a round undoing on the neighbouring axis.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArgForm {
    /// The field has not described its arguments. No `args` reaches the wire,
    /// so a client learns nothing rather than something false.
    #[default]
    Undeclared,
    /// The arguments ride the **path**, in the template's placeholder order —
    /// the read channel's only shape ([`SchemaField::parametric`]). A scalar
    /// read declares this with an empty `args`, which is the affirmative "reads
    /// as spelled".
    Path,
    /// The action takes no argument. `scene/invoke` may still be handed
    /// `null`; anything else is the caller's mistake.
    Nullary,
    /// The single declared argument **is** the whole `args` value
    /// (`invoke("stop", 7)`).
    Scalar,
    /// The arguments are the members of a JSON object, keyed by
    /// [`SchemaArg::name`] (`invoke("arrange", {"axis": "horizontal", …})`).
    Object,
    /// The arguments are the segments of a delimited string, in declared order
    /// (`invoke("send", "3:PointerUp::l")`, `invoke("item", "add:in:1")`).
    ///
    /// A form rather than a spelled-out template because the delimiter is what
    /// a client needs to split on, and the segment meanings are already the
    /// `args` list. Trailing segments may be elided when the arguments that
    /// carry them are [`optional`](SchemaArg::optional) — which is exactly the
    /// send wire's rule, stated once instead of per widget.
    Delimited(char),
}

impl ArgForm {
    /// The `$schema` wire form, or `None` for [`Undeclared`](Self::Undeclared)
    /// — which publishes nothing at all rather than the word "undeclared",
    /// because a reader that does not know this key sees the shape it always
    /// saw and a reader that does can tell silence from a claim by the key's
    /// absence.
    ///
    /// Rendered here rather than in `pinion-rpc` for the reason
    /// [`ArgDomain::to_wire`] gives: this enum is `#[non_exhaustive]`, so a
    /// match anywhere else needs a `_` arm that would quietly render a future
    /// form as some existing one.
    #[must_use]
    pub fn to_wire(self) -> Option<serde_json::Value> {
        match self {
            Self::Undeclared => None,
            Self::Path => Some(serde_json::json!({ "kind": "path" })),
            Self::Nullary => Some(serde_json::json!({ "kind": "nullary" })),
            Self::Scalar => Some(serde_json::json!({ "kind": "scalar" })),
            Self::Object => Some(serde_json::json!({ "kind": "object" })),
            Self::Delimited(sep) => {
                Some(serde_json::json!({ "kind": "delimited", "separator": sep }))
            }
        }
    }
}

/// R1642 — a way a field's [`ArgDomain::OneOfWith`] declaration is malformed, as
/// reported by [`SchemaField::conditional_defect`].
///
/// Each arm is a way the *client's* call-assembly would become ambiguous or
/// impossible, not a matter of taste — which is why they are refusals rather
/// than lint notes. A declaration is the only thing an agent has; one that
/// cannot be followed unambiguously is worse than none, because it carries a
/// schema's authority.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalDefect {
    /// Two arguments both carry a case table, so where each case's added
    /// arguments go is undefined: the rule appends them after the field's own
    /// arguments, and two appenders have no order between them.
    TwoDiscriminants {
        /// The first argument found carrying a case table.
        first: &'static str,
        /// The second.
        second: &'static str,
    },
    /// The discriminant is [`optional`](SchemaArg::optional). A case is selected
    /// by a value, so an absent value selects none, and the arguments that
    /// follow would be unaccountable.
    OptionalDiscriminant(&'static str),
    /// A case's added argument reuses a name the field already declares, or one
    /// an earlier argument of the same case does — so a client keying arguments
    /// by name (the object form) could not tell them apart.
    ShadowedName {
        /// The case that adds it.
        case: &'static str,
        /// The name already in use.
        name: &'static str,
    },
    /// Expanding this case puts an optional argument before a required one,
    /// which the delimited form cannot represent (see
    /// `r1638_optional_arguments_are_a_suffix`). The commonest way in is a case
    /// whose added argument is required while the field's own trailing argument
    /// is optional.
    OptionalNotASuffix {
        /// The case whose expansion breaks the rule.
        case: &'static str,
    },
    /// A case's added argument carries a case table of its own.
    ///
    /// The append rule extends to a second level without ambiguity, so this is
    /// not forbidden because it could not be defined — it is forbidden because
    /// nothing in this tree needs it and an unexercised wire shape is a claim no
    /// client has been held to. A round that needs nesting removes this arm and
    /// gains a consumer in the same commit; publishing it first would be the
    /// dead-capability shape R1641 recorded, where a mapping was advertised
    /// through a channel nothing carried.
    NestedDiscriminant {
        /// The outer case.
        case: &'static str,
        /// The argument inside it that carries a second case table.
        name: &'static str,
    },
}

/// R1504 — which channel a declared path belongs to: something a client
/// **reads** (`scene/query`), or something it **calls** (`scene/invoke`).
///
/// R1501 declared both kinds in one list and had no way to say which was
/// which, so the only thing that could tell them apart was a hand-written list
/// of names in a test — fifteen of them, and this round was about to add a
/// sixteenth. A declaration that cannot state the difference makes every
/// consumer of it restate the difference.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SchemaChannel {
    /// Answered by `query` at the declared path.
    #[default]
    Read,
    /// Called by `invoke` under the declared name. Not readable: probing it
    /// with `query` is expected to answer nothing.
    Invoke,
}

/// One declared member of an [`ExternalIntrospect`] surface: a path, the type of
/// the value it reads, and — R1353 — whether it takes an **argument**.
///
/// # Why this is a struct and not the `(path, type)` pair it replaced
///
/// (R1353 §2 #2, the PR-61 root cause.) The pair could not say that a path is
/// parametric. `("width", "int")` and `("total", "int")` rendered *identically*
/// through `$schema`, yet `total` is read as-is while `width` must be spelled
/// `width.<col>` — the argument rides the path (see
/// [`ExternalIntrospect::query`]). The arity lived only in the `query` impl's
/// `strip_prefix`, so the one surface an agent is *supposed* to discover the
/// contract from could not express it. Agents had to guess, and the guess failed
/// in both directions: `query("width")` answered `UnknownIntrospectPath` for a
/// path `$schema` had just advertised, while `query("width.999")` answered with a
/// plausible wrong number.
///
/// That is not a documentation problem. §2 #2 makes RPC the AI's *primary* path,
/// so a contract the surface cannot state is a contract that does not exist. A
/// consumer read the argument-free `query` signature, concluded a parameterized
/// read was impossible, routed it through `invoke` instead — and bought a ~30Hz
/// livelock that burned a core at idle, because `invoke` is a mutation and bumps
/// the scene revision its own `waitFor` was parked on.
///
/// A struct rather than a richer *string* (`"width.<col>"`, the URI-template
/// shape): the pair is already short of a second dimension — nothing here
/// distinguishes a readable value from an `invoke` action, so `float_policy` and
/// `set_float_policy` both render as bare `string`. A template string would fix
/// arity and have to be redone for that. Const constructors + defaulted fields
/// mean the next dimension lands additively.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaField {
    /// The declared path, as the wire **template**: a scalar spells itself
    /// (`"total"`), a parametric family spells its placeholders inline
    /// (`"width.<col>"`, `"cell.<row>.<col>"`, `"voice.<id>.gain"`).
    ///
    /// The template — rather than a bare stem plus trailing args — because an
    /// argument is not always trailing: `voice.<id>.gain` carries a literal
    /// *after* its argument, and a stem model cannot say that. It also happens
    /// to be the form the workspace's schemas were already hand-spelling before
    /// R1353 gave the arguments types; those strings were right, they were just
    /// unaccompanied.
    ///
    /// [`literal_prefix`](Self::literal_prefix) recovers the exact string a
    /// `query` impl strips.
    pub path: &'static str,
    /// Type tag of the value read at this path.
    pub ty: &'static str,
    /// The arguments the path takes, in wire order — empty for a plain scalar.
    ///
    /// A slice rather than an `Option<SchemaArg>` because a family can be keyed
    /// by more than one argument: `Table` reads a cell as `cell.<row>.<col>`.
    /// Modelling one argument would have forced that field back to a hand-spelled
    /// template string — the exact under-declaration this type exists to end.
    pub args: &'static [SchemaArg],
    /// R1504 — read path or invoke channel. [`new`](Self::new) and
    /// [`parametric`](Self::parametric) declare reads; [`action`](Self::action)
    /// declares a call.
    pub channel: SchemaChannel,
    /// R1638 — how [`args`](Self::args) are carried, or
    /// [`Undeclared`](ArgForm::Undeclared) when this field has not said.
    ///
    /// The pair is read together: `args` alone cannot be interpreted, so a
    /// consumer that finds `Undeclared` must treat `args` as absent rather than
    /// as empty. The wire enforces that by rendering neither.
    pub form: ArgForm,
}

impl SchemaField {
    /// A placeholder for `const fn` schema COMPOSITION — the fill value for a
    /// fixed-size array a `const fn` builds before overwriting every slot
    /// (`hello-audio-device` concatenates the RT surface's fields with its own
    /// this way, so a field added upstream cannot silently go missing
    /// downstream).
    ///
    /// It exists because `#[non_exhaustive]` denies other crates the struct
    /// literal such a composer would otherwise use, and a composer cannot invent
    /// a placeholder from a name it does not have. Never declare it: an empty
    /// path matches no query, so a slot left un-overwritten shows up as a blank
    /// row in `$schema` rather than as something plausible.
    pub const EMPTY: Self = Self::new("", "");

    /// A plain scalar path: read it as spelled, no argument.
    #[must_use]
    pub const fn new(path: &'static str, ty: &'static str) -> Self {
        Self {
            path,
            ty,
            args: &[],
            channel: SchemaChannel::Read,
            // A scalar read HAS declared its arity: zero, riding the path.
            form: ArgForm::Path,
        }
    }

    /// R1504 — an `invoke` channel rather than a readable path. Declared in the
    /// same list because it is part of the same surface, but marked, so a
    /// caller auditing "does every declared path answer?" can skip these
    /// without being handed a list of names to maintain.
    #[must_use]
    pub const fn action(path: &'static str, ty: &'static str) -> Self {
        Self {
            path,
            ty,
            args: &[],
            channel: SchemaChannel::Invoke,
            // R1638 — silence, not "takes nothing". See `ArgForm::Undeclared`
            // for why the two must stay distinguishable, and `action_with` for
            // how a surface says either one.
            form: ArgForm::Undeclared,
        }
    }

    /// R1638 — an `invoke` channel that **says what it takes**: the form the
    /// arguments arrive in, and one [`SchemaArg`] per argument in wire order.
    ///
    /// The peer of [`action`](Self::action), which stays silent. Both are
    /// truthful; this one is useful. A surface that takes nothing says so with
    /// [`ArgForm::Nullary`] and an empty slice, which is a claim rather than the
    /// absence of one — see [`ArgForm`] for why that distinction is load-bearing
    /// in a tree whose declarations are hand-written.
    ///
    /// # What this gives that the reference cannot
    ///
    /// A meta-method publishes each parameter's **name and type**, which is
    /// where the toolkit stops. A [`SchemaArg`] additionally carries its
    /// [`domain`](SchemaArg::domain) — *where the answerable values come from* —
    /// so `set_voice_gain`'s `id` says "the ids listed at `voices`" instead of
    /// only "int", and an agent enumerates a valid call instead of guessing one.
    /// The reference's nearest equivalent constrains C++ enum-typed parameters
    /// alone; this constrains any argument.
    ///
    /// ```
    /// # use pinion_core::external::{ArgForm, SchemaArg, SchemaField};
    /// const ARRANGE: SchemaField = SchemaField::action_with(
    ///     "arrange",
    ///     "string",
    ///     ArgForm::Object,
    ///     const {
    ///         &[
    ///             SchemaArg::open("axis", "string"),
    ///             SchemaArg::open("gap", "int").optional(),
    ///         ]
    ///     },
    /// );
    /// assert_eq!(ARRANGE.form, ArgForm::Object);
    /// ```
    #[must_use]
    pub const fn action_with(
        path: &'static str,
        ty: &'static str,
        form: ArgForm,
        args: &'static [SchemaArg],
    ) -> Self {
        Self {
            path,
            ty,
            args,
            channel: SchemaChannel::Invoke,
            form,
        }
    }

    /// R1638 — the composite pointer channel every widget declares, with the
    /// send wire's grammar attached: `"<key>:<Event>[:<mods>[:<buttons>]]"`.
    ///
    /// `returns` because the widgets differ there and only there — a button
    /// answers its new state name, a toggle answers a formatted pair, several
    /// answer nothing — while the *argument* grammar is one thing owned by
    /// [`split_send_payload`](crate::composite_tag::split_send_payload). The
    /// sites spell this instead of restating four arguments each, which is the
    /// same reason the parser is not copied either: the wire has grown a segment
    /// three times, and a per-site copy would have gone stale on each.
    ///
    /// # Six surfaces accept a shorthand this does not describe
    ///
    /// R1639 — `datepicker`, `disclosure_group`, `listbox`, `radio_group`,
    /// `table` and `text_field` decode a composite payload AND fall back to a
    /// bare event name, so they take either. `ArgForm` has no "one or the
    /// other" arm and deliberately gains none here: what a declaration states
    /// is **what a client should send**, and a shorthand the surface also
    /// happens to accept is a convenience rather than a contract. Declaring the
    /// composite form is the choice that stays true if the shorthand is ever
    /// retired, and it is the same call `pinion_audio`'s `play` made for its
    /// bare-string form. The alternative — a union arm — is left to
    /// [[debt-send-is-four-grammars-under-one-name]], which is also where the
    /// question of whether these four grammars should share a name lives.
    ///
    /// A surface whose `send` is ONLY a bare event uses
    /// [`SchemaArg::event`] instead.
    #[must_use]
    pub const fn send(returns: &'static str) -> Self {
        Self::action_with(
            "send",
            returns,
            ArgForm::Delimited(crate::composite_tag::SEND_SEPARATOR),
            crate::composite_tag::SEND_ARGS,
        )
    }

    /// A **parametric** path: `path` is the wire template with a `<name>`
    /// placeholder per entry of `args`, in order (`"width.<col>"`,
    /// `"cell.<row>.<col>"`, `"voice.<id>.gain"`).
    ///
    /// `args` is a slice even for the overwhelmingly common single-argument case:
    /// a `const fn` cannot build a `&'static` slice out of a by-value parameter,
    /// so a one-arg convenience constructor could not delegate here and would be
    /// a second, drifting definition of the same thing.
    ///
    /// The template's placeholders and `args` must agree in name and order. A
    /// `const fn` cannot parse the string to check that here, so it is enforced
    /// by `r1353_1_every_real_declaration_matches_its_template`, which scans the
    /// workspace's SOURCE for every real `parametric` call — a runtime test
    /// cannot see a declaration in a crate this one does not link against, and
    /// R1353's first cut cited a test that only checked its own fixtures.
    #[must_use]
    pub const fn parametric(
        path: &'static str,
        ty: &'static str,
        args: &'static [SchemaArg],
    ) -> Self {
        Self {
            path,
            ty,
            args,
            channel: SchemaChannel::Read,
            form: ArgForm::Path,
        }
    }

    /// R1501 — `a` followed by `b`, composed at **compile time**, so a consumer
    /// that layers its own paths over another surface's declares its own and
    /// borrows the rest instead of hand-copying them.
    ///
    /// The hand-copy is what fails. `hello-column-reorder` restated ~40 of
    /// [`ColumnLayout`](crate::widgets::column_layout::ColumnLayout)'s paths in
    /// its own literal, and three consecutive rounds that added a path upstream
    /// left the copy behind — measured, five of them, all answering and none
    /// discoverable. This is the shape [`EMPTY`](Self::EMPTY) was introduced
    /// for; `hello-audio-device` had already written the loop by hand, and
    /// R1501 is its third caller, which is what lifts it here.
    ///
    /// `N` comes from the call site's own type, which is where the two lengths
    /// are known:
    ///
    /// ```
    /// # use pinion_core::external::SchemaField;
    /// const OWN: [SchemaField; 1] = [SchemaField::new("labels", "json")];
    /// const BASE: &[SchemaField] = &[SchemaField::new("order", "json")];
    /// static ALL: [SchemaField; OWN.len() + BASE.len()] =
    ///     SchemaField::concat(&OWN, BASE);
    /// assert_eq!(ALL[1].path, "order");
    /// ```
    ///
    /// # Panics
    ///
    /// When `N` is smaller than `a.len() + b.len()`. In the `const` context
    /// this exists for that is a compile error, not a runtime one — which is
    /// the point: a length that stops matching its inputs cannot ship.
    #[must_use]
    pub const fn concat<const N: usize>(a: &[Self], b: &[Self]) -> [Self; N] {
        // `EMPTY` is the fill, and every slot is overwritten below; an
        // un-overwritten one would render as a blank row rather than as
        // something plausible (see `EMPTY`).
        let mut out = [Self::EMPTY; N];
        let mut i = 0;
        while i < a.len() {
            out[i] = a[i];
            i += 1;
        }
        let mut j = 0;
        while j < b.len() {
            out[i + j] = b[j];
            j += 1;
        }
        out
    }

    /// The literal prefix before this field's first argument — exactly the
    /// string a `query` impl's `strip_prefix` matches (`"width.<col>"` →
    /// `"width."`, `"state:<id>"` → `"state:"`). Equal to [`path`](Self::path)
    /// for a scalar.
    ///
    /// Verbatim, with the separator: the separator is whatever the author wrote
    /// (`.` almost everywhere, `:` in `hello-input-chip`), and this type does not
    /// get to assume one. R1353's first cut trimmed `'.'` specifically, which
    /// made the same accessor answer `"width"` for a dotted template and
    /// `"state:"` for a colon one — and its doc, which claimed to return what
    /// `strip_prefix` matches, was then true only for the case it did not trim.
    #[must_use]
    pub fn literal_prefix(&self) -> &'static str {
        match self.path.find('<') {
            Some(i) => &self.path[..i],
            None => self.path,
        }
    }

    /// Does `probe` address this field? An exact hit for a scalar; for a
    /// parametric family, a probe that matches the template's literal segments.
    ///
    /// The membership question every caller actually means — a parametric family
    /// is addressed by its members, never by its template, so a bare
    /// `fields.iter().any(|f| f.path == probe)` answers "no such path" for
    /// `width.0`, a path the surface answers perfectly well. That mistake is the
    /// §2 #7 lie [`read_only_or_unknown`] exists to prevent, so it routes here.
    ///
    /// **Ownership only — never validity.** Whether an argument is well-formed,
    /// in range, or non-empty is the `query` impl's call and no one else's:
    /// `width.zzz` belongs to `width` and is malformed, not unknown, and the
    /// surface says which by answering
    /// [`ReadRefusal::QueryTypeMismatch`] rather than by being unreachable.
    ///
    /// R1667 — this used to make exactly one exception, requiring a **non-empty**
    /// run in each placeholder, and that exception is why the doc above was true
    /// of `width.zzz` and false of `width.`. Emptiness is a property of the
    /// argument, not of the address, so the two malformed probes were being
    /// answered along different axes; and the exception was not load-bearing for
    /// the parse, which is greedy-to-the-next-literal and therefore stays
    /// deterministic with empty runs (`cell..2` is `row=""`, `col="2"`, one
    /// reading). What it did instead was decide, on behalf of all 106 declared
    /// families at once, that no family may have a meaningful empty member —
    /// while [`SchemaArg`] exists precisely so a family can state its own
    /// argument's domain. A consumer that publishes `find.` as "a search for
    /// nothing" was told its own declared family had no such address.
    #[must_use]
    pub fn addresses(&self, probe: &str) -> bool {
        // R1638 — only a PATH-form field's arguments are part of its address.
        // An action's are carried by `scene/invoke`, so its path is exact
        // however many it declares; keying this off `args.is_empty()` (as it did
        // before actions could declare any) would have started template-matching
        // `arrange` the moment it said what it takes.
        if self.form != ArgForm::Path || self.args.is_empty() {
            return self.path == probe;
        }
        // Walk the template's literal segments across `probe`, requiring a
        // non-empty run wherever a placeholder sits.
        let mut rest = probe;
        let mut tmpl = self.path;
        let mut first = true;
        while let Some(open) = tmpl.find('<') {
            let literal = &tmpl[..open];
            let Some(after) = rest.strip_prefix(literal) else {
                return false;
            };
            if !first && literal.is_empty() {
                // Two placeholders with no literal between them cannot be
                // delimited; such a template is malformed, not matchable.
                return false;
            }
            rest = after;
            let Some(close) = tmpl[open..].find('>') else {
                return false;
            };
            tmpl = &tmpl[open + close + 1..];
            // The argument runs until the template's next literal (or the end).
            let next_lit_end = tmpl.find('<').unwrap_or(tmpl.len());
            let next_lit = &tmpl[..next_lit_end];
            let arg_len = if next_lit.is_empty() {
                rest.len()
            } else {
                match rest.find(next_lit) {
                    Some(i) => i,
                    None => return false,
                }
            };
            // R1667 — an empty run is a member with an empty argument, not a
            // non-member. See the doc above for why this is not the matcher's
            // call to make.
            rest = &rest[arg_len..];
            first = false;
        }
        rest == tmpl
    }

    /// R1642 — the first way this field's conditional declaration is malformed,
    /// or `None` when a client can follow it unambiguously.
    ///
    /// One definition of the composition rule stated in [`ArgDomain::OneOfWith`],
    /// because three callers ask it and three copies would drift: a
    /// `pinion-core` unit test that drives every arm against a fixture built to
    /// violate it (a checker nobody has seen fail is a checker nobody has
    /// tested), the workspace declaration walk, and each surface that declares a
    /// conditional verb, whose own test module holds its real declaration to it.
    ///
    /// Answers `None` for the overwhelming majority of fields, which declare no
    /// discriminant at all — the rules are about a shape most declarations do not
    /// have, so a caller reporting "no defect" over a population with no
    /// inhabitants is reporting nothing. Callers should state the inhabitant
    /// count beside the verdict; [`declares_cases`](Self::declares_cases) is how.
    #[must_use]
    pub fn conditional_defect(&self) -> Option<ConditionalDefect> {
        let mut discriminant: Option<&SchemaArg> = None;
        for arg in self.args {
            if arg.domain.cases().is_empty() {
                continue;
            }
            if let Some(first) = discriminant {
                return Some(ConditionalDefect::TwoDiscriminants {
                    first: first.name,
                    second: arg.name,
                });
            }
            if arg.optional {
                return Some(ConditionalDefect::OptionalDiscriminant(arg.name));
            }
            discriminant = Some(arg);
        }
        let discriminant = discriminant?;
        for case in discriminant.domain.cases() {
            for added in case.then {
                if !added.domain.cases().is_empty() {
                    return Some(ConditionalDefect::NestedDiscriminant {
                        case: case.value,
                        name: added.name,
                    });
                }
                if self.args.iter().any(|a| a.name == added.name)
                    || case.then.iter().filter(|a| a.name == added.name).count() > 1
                {
                    return Some(ConditionalDefect::ShadowedName {
                        case: case.value,
                        name: added.name,
                    });
                }
            }
            // The expansion is the field's own arguments followed by this case's,
            // which is the order a client sends and therefore the order the
            // optional-suffix rule has to hold of.
            let optional: Vec<bool> = self
                .args
                .iter()
                .chain(case.then)
                .map(|a| a.optional)
                .collect();
            if let Some(i) = optional.iter().position(|o| *o)
                && !optional[i..].iter().all(|o| *o)
            {
                return Some(ConditionalDefect::OptionalNotASuffix { case: case.value });
            }
        }
        None
    }

    /// Whether this field declares a case table at all — the denominator a
    /// caller of [`conditional_defect`](Self::conditional_defect) should report,
    /// so "no defect" cannot be read as coverage over a population of zero.
    #[must_use]
    pub fn declares_cases(&self) -> bool {
        self.args.iter().any(|a| !a.domain.cases().is_empty())
    }
}

/// Schema declaring which paths an [`ExternalIntrospect`] exposes.
///
/// R1353: a static slice of [`SchemaField`]s — a path, its type, and whether it
/// takes an argument. Future expansion (a structured `Type` enum, read-vs-action
/// kind, units of measure) lands via `#[non_exhaustive]` + defaulted const
/// constructors — additive only.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectSchema {
    /// Declared fields.
    ///
    /// R1501 — the sentence that stood here said authors are responsible for
    /// keeping this in sync with `query` / `intervene`, and that mismatches
    /// surface as test failures. Measured over the real wire against
    /// `hello-column-reorder`, they do not: five surfaces answered that
    /// `$schema` never mentioned (`stretch_last_section`,
    /// `effective_resize_modes`, `effective_resize_mode.<logical>`,
    /// `resize_contents_precision`, `reset_default_section_size`), each one
    /// added by a round that edited the answering module and not the
    /// hand-copied list downstream of it. Nothing failed, because nothing was
    /// checking this direction — [`SchemaField::addresses`] audits the
    /// declarations that exist, and an omission declares nothing to audit.
    ///
    /// So the responsibility is not the author's to remember: a surface
    /// declares the paths it answers, and a consumer *composes* that
    /// declaration with [`SchemaField::concat`] rather than restating it. A
    /// widget that gates its own dispatch on its own list
    /// (`ColumnLayout::query`) cannot grow an undeclared path at all — the arm
    /// is unreachable until it is declared, so the round that adds it finds out
    /// in its own tests instead of shipping a surface no client can discover.
    pub fields: &'static [SchemaField],
}

impl IntrospectSchema {
    #[must_use]
    pub const fn new(fields: &'static [SchemaField]) -> Self {
        Self { fields }
    }

    /// The field addressing `probe`, if any — exact for a scalar, stem-matched
    /// for a parametric family ([`SchemaField::addresses`]).
    #[must_use]
    pub fn field_for(&self, probe: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.addresses(probe))
    }
}

/// R1480 §5.15 — JSON text a producer has **already encoded**, carried to
/// the wire without a `serde_json::Value` DOM in between.
///
/// [`IntrospectValue::Json`] is a DOM. An [`ExternalIntrospect::query`]
/// answering with anything larger than a scalar builds a tree of maps and
/// vectors that the JSON-RPC envelope then walks again to produce text.
/// Neither end wants the tree: the producer holds a `Serialize` type, the
/// consumer receives bytes. The tree exists only because the channel's
/// type demanded one. `RawJson` widens the channel — the producer
/// serializes once and the envelope splices the result.
///
/// **The bytes are the value.** Two `RawJson`s are equal iff their text is
/// identical, so `{"a":1,"b":2}` and `{"b":2,"a":1}` differ here although
/// their `Value` projections do not. That is the contract the type exists
/// to keep: a raw answer promises a particular encoding, and an equality
/// that looked past the encoding would compare something this type does
/// not carry.
///
/// **Serialization is `serde_json`-specific.** [`RawValue`] asks its
/// serializer for verbatim splicing through a private token; any other
/// `Serializer` would see a struct named by that token. pinion's only
/// response serializer is `serde_json`, so the requirement holds by
/// construction — but a future non-JSON transport must go through
/// [`Self::to_value`] rather than serialize a `RawJson` directly.
#[derive(Debug, Clone)]
pub struct RawJson(Box<RawValue>);

impl RawJson {
    /// Encode `value` straight to JSON text — one serialization pass, no
    /// intermediate DOM. Validity needs no check because the encoder
    /// produced the text.
    ///
    /// # Errors
    ///
    /// Whatever `T`'s `Serialize` impl reports: a custom impl that errors,
    /// or a map with non-string keys (the same inputs
    /// `serde_json::to_value` rejects).
    pub fn encode<T: ?Sized + serde::Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        serde_json::value::to_raw_value(value).map(Self)
    }
    /// Adopt JSON text from an untrusted source. The text is parsed for
    /// validity — and discarded, not retained as a DOM — so malformed
    /// JSON is rejected here rather than corrupting a wire frame.
    ///
    /// # Errors
    ///
    /// `json` is not a single well-formed JSON value.
    pub fn parse(json: String) -> Result<Self, serde_json::Error> {
        RawValue::from_string(json).map(Self)
    }

    /// The JSON text, exactly as the producer wrote it.
    #[must_use]
    pub fn get(&self) -> &str {
        self.0.get()
    }

    /// Materialize the DOM this type exists to avoid. For contexts that
    /// genuinely need one — a raw answer nested inside a larger `Value`
    /// the envelope is assembling (`scene/snapshot`, `scene/dry_run`),
    /// where the enclosing tree has to be walked regardless.
    ///
    /// # Errors
    ///
    /// The text is valid JSON that `Value` cannot represent — in
    /// practice only a number outside `f64` range, which `Value` has no
    /// slot for.
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(self.0.get())
    }
}

impl PartialEq for RawJson {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}

impl serde::Serialize for RawJson {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Opaque value payload for `query` / `intervene`. Scalar variants
/// cover the JSON-RPC primitive surface; `Json` carries arbitrary
/// structured payloads (objects, arrays, mixed scalars) for callers
/// that round-trip through `serde_json::Value` — used by the §5.22
/// reactive bridge for `Signal<T>` where `T` is a struct or sequence
/// (R37.6 #11 extension); `Raw` carries the same structured payloads
/// for producers that already hold the encoding (R1480).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum IntrospectValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Json(serde_json::Value),
    /// R1480 §5.15 — an answer whose JSON text the producer already has.
    /// Reaches the `scene/query` / `scene/invoke` result verbatim; nested
    /// introspection contexts materialize it (see [`RawJson::to_value`]).
    Raw(RawJson),
}

impl IntrospectValue {
    /// R51.155 §5.15 — extract a `bool` payload. Returns `Some(b)`
    /// only when the variant is [`Self::Bool`]; every other variant
    /// (including `Json(serde_json::Value::Bool(_))`) returns `None`
    /// so the typed-extraction path stays unambiguous.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract an `i64` payload. Returns `None` for
    /// non-[`Self::Int`] variants; numeric coercions (`Float → i64`,
    /// `Json::Number → i64`) are intentional opt-outs.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract an `i64` payload narrowed to `i32`.
    /// Returns `None` when the variant is not [`Self::Int`] or when
    /// the stored value falls outside the `i32` range — narrowing
    /// failures are surfaced rather than silently truncated.
    /// Convenient for the common composite-widget index path
    /// (`focused_index` / `selected_index` introspect slots return
    /// non-negative `Int`).
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        self.as_i64().and_then(|v| i32::try_from(v).ok())
    }

    /// R51.155 §5.15 — extract an `i64` payload narrowed to `usize`.
    /// Returns `None` for non-[`Self::Int`] variants and for negative
    /// integers (which can't be a `usize`).
    #[must_use]
    pub fn as_usize(&self) -> Option<usize> {
        self.as_i64().and_then(|v| usize::try_from(v).ok())
    }

    /// R51.155 §5.15 — extract a `f64` payload. Returns `None` for
    /// non-[`Self::Float`] variants; integer-to-float coercion is an
    /// intentional opt-out.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// R51.155 §5.15 — extract a `f64` payload narrowed to `f32`.
    /// Returns `None` for non-[`Self::Float`] variants; the `f64 →
    /// f32` narrowing is a documented truncation (precision loss for
    /// values past f32's representable range, NaN passes through).
    /// Encapsulates the previous per-call-site
    /// `#[allow(clippy::cast_possible_truncation)]` lints that
    /// hello-slider*/hello-slider-vertical sprinkled around their
    /// `IntrospectValue::Float(v) => v as f32` matches.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(|v| v as f32)
    }

    /// R51.155 §5.15 — extract a `&str` payload. Returns `None` for
    /// non-[`Self::Text`] variants; `Json::String` is opt-out (the
    /// JSON path goes through `as_json`).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// R51.155 §5.15 — `true` iff the variant is [`Self::Null`].
    /// Diagnostic helper paired with the typed accessors above.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// R1276 §5.15 — construct a [`Self::Json`] payload from any
    /// serializable value: the canonical way an
    /// [`ExternalIntrospect::query`] returns a structured (object / array)
    /// result. Lifts the `Json(serde_json::to_value(..).unwrap_or(Null))`
    /// idiom the narrative / place-map / audio introspection surfaces each
    /// hand-rolled (Rule-of-Three). A serialization failure — which the
    /// derived `Serialize` impls this is used with cannot produce —
    /// degrades to [`Self::Null`] rather than panicking.
    #[must_use]
    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        Self::Json(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))
    }

    /// R1480 §5.15 — construct a [`Self::Raw`] payload: the same call
    /// shape as [`Self::json`] with the DOM removed. Prefer it whenever
    /// the answer is larger than a scalar and the caller is a `query` /
    /// `invoke` result, which is where the raw text survives to the wire.
    ///
    /// Degrades to [`Self::Null`] on a serialization failure — the same
    /// inputs [`Self::json`] refuses, and the same JSON `null` on the
    /// wire (that one keeps a `Json` wrapper around its null; past the
    /// envelope the two are indistinguishable). The variant differs on
    /// purpose: [`Self::as_raw`] then reports `None`, so a caller can
    /// still tell the encoding did not happen. A producer that wants the
    /// error itself builds with [`RawJson::encode`].
    #[must_use]
    pub fn raw<T: serde::Serialize>(value: &T) -> Self {
        RawJson::encode(value).map_or(Self::Null, Self::Raw)
    }

    /// R1480 §5.15 — borrow a [`Self::Raw`] payload. Returns `None` for
    /// every other variant, including a `Json` holding the same document:
    /// the typed accessors do not coerce across variants.
    #[must_use]
    pub fn as_raw(&self) -> Option<&RawJson> {
        match self {
            Self::Raw(r) => Some(r),
            _ => None,
        }
    }
}

/// R826 §5.12 — the shared `<axis>.<pos>` introspect projection. Resolve a
/// visual position (`rest`, the part after a matched `<axis>.` prefix)
/// through `get`, then `project` the looked-up item to an
/// [`IntrospectValue`]. An out-of-range or unparseable position — or a
/// `project` that itself yields it — reports [`IntrospectValue::Null`]
/// (present-but-empty), never absence: the §5.12 convention every
/// position-indexed introspect path shares.
///
/// The single source of truth for that convention, lifted at R826 from the
/// three byte-identical copies the sort proxies
/// ([`source_at_value`](crate::widgets::order_memo)), the tree filter
/// (`id_value`), and the tree view (`row_at`) had each grown — each now a
/// thin `(get, project)` binding over this primitive, so the out-of-range
/// policy lives in exactly one place.
pub(crate) fn at_index<T>(
    rest: &str,
    get: impl Fn(usize) -> Option<T>,
    project: impl Fn(T) -> IntrospectValue,
) -> IntrospectValue {
    rest.parse::<usize>()
        .ok()
        .and_then(get)
        .map_or(IntrospectValue::Null, project)
}

/// R742 §5.51 — typed drag-and-drop payload. Produced by a drag source
/// via [`External::begin_drag`] and carried by the router's drag session
/// until the matching drop, mirroring the
/// [`Intent`] wire form (a `kind` tag plus an
/// [`IntrospectValue`]) so the in-flight drag is introspectable as
/// scene-as-data (§2 #7) and a future cross-widget drop target can match
/// on `kind` before interpreting `value`.
#[derive(Debug, Clone, PartialEq)]
pub struct DragPayload {
    /// Discriminator naming what is being dragged (e.g. `"dnd-row"`,
    /// `"dock-panel"`, `"tab"`). A drop target matches on this before
    /// reading `value`. `Cow` so a static-string source pays no
    /// allocation while a runtime-built kind is still expressible.
    pub kind: Cow<'static, str>,
    /// The dragged datum — typically the source item's stable id or
    /// index, addressed the same way an [`Intent`] payload is.
    pub value: IntrospectValue,
}

/// R742 §5.51 — the live drop location the router resolves under the
/// cursor during a drag and feeds back to the drag source via
/// [`External::drag_to`] / [`External::drag_release`].
///
/// `tag` is the full paint tag directly under the cursor — a composite
/// `widget#sub` when the hovered region is a sub-element (the reorder
/// row / dock panel / tab the cursor is over). `x_rel` / `y_rel` are the
/// cursor position normalised over that tag's post-layout rect, in
/// `0.0`..`1.0` because `tag` is the region the cursor is over — the
/// normalisation is NOT itself clamped, so a value outside that range
/// would mean the cursor had left the rect. The source coordinator
/// classifies before / after / centre from these without re-reading
/// layout — the generalisation of the dock resolver's edge-vs-centre
/// zone test.
#[derive(Debug, Clone, PartialEq)]
pub struct DropPoint {
    /// Full paint tag under the cursor (possibly composite `widget#sub`).
    pub tag: String,
    /// Cursor X normalised over `tag`'s rect (`0.0` left .. `1.0` right).
    pub x_rel: f32,
    /// Cursor Y normalised over `tag`'s rect (`0.0` top .. `1.0` bottom).
    pub y_rel: f32,
}

/// (R1156 §5.51) Reserved [`DropPoint::tag`] the cross-window drop resolution returns when
/// the cursor lands in the OUTER PERIMETER band of the drop surface (within
/// [`OUTER_DOCK_MARGIN`] of the window content's edge) instead of over an inner panel. A dock
/// consumer reads it as a FULL-SPAN outer dock at the edge the `x_rel` / `y_rel`
/// (normalised over the WHOLE surface here, not a panel) is nearest — the
/// container-edge / "outer dock guide" gesture (VS Code edge zones, the
/// toolkit ADS outer dock areas). The leading `NUL` makes it a sentinel no real
/// paint tag can collide with.
pub const OUTER_DOCK_ZONE_TAG: &str = "\u{0}outer-dock-zone";

/// (R1205 §5.51 §5.39) Tag the dock walker
/// ([`view_dock_surface`](../../pinion_widget_paint/dock/fn.view_dock_surface.html))
/// stamps on the container wrapping its whole workspace subtree, so the laid-out
/// rect of that wrapper IS the DOCK AREA — the region the reorganizer manages,
/// wherever the composing view places it (below a client-side chrome strip, below
/// a fixed toolbar / menu, inside a split, …). The one SSOT for "where is the
/// dock area" ([`Scene::dock_surface_rect`](crate::scene::Scene::dock_surface_rect)):
/// the same-window OUTER dock band (`InputRouter::resolve_own_outer_dock`) and the
/// cross-window redock preview both read this rect, so they agree on the dock area
/// with ZERO wiring — no per-window chrome-height scalar to stamp (R1202/R1203's
/// `dock_area_top_inset` / `inset_below_chrome`, a top-only approximation blind to a
/// toolbar, were retired for this rect). The leading `NUL` makes it a sentinel no
/// real user paint tag can collide with, and it is a structural ANCESTOR of the
/// tagged splitter / panel that fills it, so `resolve_hover_tag`'s deepest-first
/// walk never resolves to it.
pub const DOCK_SURFACE_TAG: &str = "\u{0}dock-surface";

/// (R1156 §5.51) How far INSIDE / outside the drop surface's perimeter the cursor
/// may sit and still classify as an OUTER full-span dock (logical px). The
/// outermost band of this width maps to the container edge; the interior past it
/// maps to per-panel inner zones — so a drop at the very top of the dock area is a
/// full-width row, while a drop between two panels splits just those panels.
///
/// ★LIVE-TUNE (R1167, HW-gated): this is a fixed absolute band. The user found it
/// "too thin" cross-window; the R1167 same-window outer dock
/// (`InputRouter::resolve_own_outer_dock`) reaches it from inside, so it is now
/// reachable at this width, but the FEEL of the width is the user's `:0` call. A
/// fixed widen is NOT scale-safe (a band wider than a small floater's half makes
/// its whole area outer), so a future tune is likely a fraction-of-dimension band
/// (`min(MARGIN, frac * dim)`), not a bigger constant — deferred to live feedback.
pub const OUTER_DOCK_MARGIN: f64 = 32.0;

/// (R1081 §5.51; R1167 SSOT-lift to core) The [`DragPayload::kind`] discriminator a
/// dock-panel drag (a panel header OR a tab) carries. Lives in core so BOTH the
/// producing widget (`pinion_widget_paint::dock`, which re-exports it) AND the
/// consuming runtime router (which gates the same-window OUTER-dock override on it
/// — a non-dock drag like the outliner tree reparent must NOT get the dock
/// sentinel) name the one string. The drag-kind is wire vocabulary shared across
/// the producer/consumer boundary, so its canon home is the shared crate
/// (the [[wire-vocab-canon-pin-not-fold]] pattern), like [`OUTER_DOCK_ZONE_TAG`].
pub const DOCK_PANEL_DRAG_KIND: &str = "dock-panel";

/// R1667 §5.15 §2 #7 — why a surface declined to answer
/// [`ExternalIntrospect::query`]. The read channel's peer of
/// [`InterveneError`].
///
/// # Why a read needs a reason at all
///
/// It did not have one. `query` answered `Option<IntrospectValue>`, and the
/// transport turned every `None` into the single word `UnknownIntrospectPath` —
/// so "the schema does not declare that name", "the family is declared and index
/// 999 addresses nothing", and "the argument is not an integer" reached a client
/// as **byte-identical** answers. The surface knew which of the three it meant
/// (its own `parse` and bounds check are what produced the `None`) and had no
/// way to say so. `dispatch::query_error_reason` even carried the sentence that
/// documented the gap: *"a READ cannot be refused by a producer: `query`
/// answers `Option`, so every failure here is the transport's own
/// classification."*
///
/// The write channel never had that problem — [`InterveneError`] has said
/// `UnknownPath` / `TypeMismatch` / `ReadOnly` / `OutOfRange` since R51, and
/// R1565 gave the last of those the producer's own sentence. The read channel
/// was the asymmetric one ([[wire-form-read-write-symmetry]]), and a consumer
/// paid for it: sprag publishes three distinct facts about a parametric family
/// (a member with an argument, a member with an **empty** argument, and a bare
/// stem that is no address at all) and could deliver only two, because two of
/// its three answers were the same bytes.
///
/// # Which arm to answer
///
/// * [`UnknownPath`](Self::UnknownPath) — the schema does not declare this
///   path. The transport's declaration gate normally answers this before the
///   surface is consulted at all (R1637), so a surface reaches for it only when
///   it is driven directly, in-process, past that gate.
/// * [`NoSuchMember`](Self::NoSuchMember) — the path belongs to a declared
///   family and the argument names nothing that exists. **Carries the
///   surface's own sentence**, for the reason
///   [`InterveneError::OutOfRange`] does: the variant says a member is missing
///   and cannot say which members are present, so a client holding it alone
///   knows only that it guessed wrong. Build it with
///   [`no_such_member`](Self::no_such_member).
/// * [`QueryTypeMismatch`](Self::QueryTypeMismatch) — the path belongs to a
///   declared family and the argument is not the declared type (`width.zzz`
///   where `col` is an int, and — since R1667 — `width.` where it is empty).
///   No sentence: `$schema` publishes the argument's type, so a sentence here
///   would restate what the client can already read.
/// * [`Unavailable`](Self::Unavailable) — the path is declared, the call is
///   well formed, and **this instance** cannot answer it: a `TextField` with no
///   `TextEditState` attached has a `caret`, in the schema and not in the
///   object. The read peer of [`InvokeError::Rejected`], and it exists
///   for the same reason: the caller did nothing wrong, so telling it to fix
///   its call sends it to rewrite something that was already right.
///
/// Answering a **value** is still available for "that position holds nothing":
/// [`IntrospectValue::Null`] is an `Ok`, and the surfaces routed through
/// `at_index` use it. This enum is for the cases where there is no value to
/// give, not for empty ones — and `text_field`'s doc has drawn exactly that
/// line since R56.1.f.3, promising clients they could "distinguish *no state
/// bound* from *no selection*". Both were `None` when it said so.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadRefusal {
    /// The schema does not declare this path.
    UnknownPath,
    /// A declared family, and the argument addresses nothing. Carries the
    /// surface's sentence naming what would have worked.
    NoSuchMember(RefusalReason),
    /// A declared family, and the argument is not the declared type — including
    /// an argument that is **empty** (`width.`), which R1667 made the surface's
    /// call rather than the matcher's. See [`SchemaField::addresses`].
    QueryTypeMismatch,
    /// Declared, well addressed, and unanswerable by this instance — it holds
    /// no state to read. Carries the surface's sentence, because "which state,
    /// and how would a client attach it" is the whole of what makes this
    /// actionable. Build it with [`unavailable`](Self::unavailable).
    Unavailable(RefusalReason),
}

impl ReadRefusal {
    /// Refuse a read because the argument addresses no existing member,
    /// **stating what does**.
    ///
    /// ```
    /// # use pinion_core::external::ReadRefusal;
    /// let err = ReadRefusal::no_such_member(format!("row {} is outside 0..{}", 99, 12));
    /// assert!(err.reason().is_some_and(|why| why.as_str().contains("0..12")));
    /// ```
    #[must_use]
    pub fn no_such_member(reason: impl Into<RefusalReason>) -> Self {
        Self::NoSuchMember(reason.into())
    }

    /// Refuse a read because this instance holds nothing to read it from,
    /// **saying what is missing**.
    ///
    /// ```
    /// # use pinion_core::external::ReadRefusal;
    /// let err = ReadRefusal::unavailable("no TextEditState is attached to this field");
    /// assert!(err.reason().is_some_and(|why| why.as_str().contains("TextEditState")));
    /// ```
    #[must_use]
    pub fn unavailable(reason: impl Into<RefusalReason>) -> Self {
        Self::Unavailable(reason.into())
    }

    /// The surface's sentence, when this is a refusal that carries one.
    ///
    /// `None` for the two arms whose variant fully determines their meaning —
    /// the same split [`InterveneError::reason`] makes, and for the same
    /// reason: a sentence beside a self-explaining variant is prose a client
    /// would have to parse to learn nothing.
    #[must_use]
    pub fn reason(&self) -> Option<&RefusalReason> {
        match self {
            Self::NoSuchMember(reason) | Self::Unavailable(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Failure modes for [`ExternalIntrospect::intervene`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterveneError {
    /// Path is not declared in the schema.
    UnknownPath,
    /// Path exists but the value variant does not match the slot type.
    /// Use this for "the JSON was a String when an Int was expected",
    /// not for "the Int was outside the slot's accepted range" — the
    /// latter is [`OutOfRange`](Self::OutOfRange).
    TypeMismatch,
    /// Path exists and the type matches but the slot is read-only.
    ReadOnly,
    /// R51.91 §5.40 — path exists and the value variant matches the
    /// slot type, but the value itself falls outside the accepted
    /// range. Composite widgets that address sub-elements by index
    /// (`RadioGroup::selected_index` / `focused_index`,
    /// future `ListBox::selected_index` / `TabBar::active_index`)
    /// raise this for negative integers and indices `>= count`. Slot
    /// types with continuous-value clamping (`Slider::value`) prefer
    /// internal clamping over rejection and do not raise this.
    ///
    /// R1565 — carries the producer's own [`RefusalReason`], for the reason
    /// [`InvokeError::Rejected`] does: **the variant does not determine the
    /// range**. "Out of range" is the one arm of this enum whose meaning is
    /// incomplete without the surface saying which range, and a client holding
    /// it knows only that its value was wrong, not what a right one would be.
    /// Build it with [`out_of_range`](Self::out_of_range).
    OutOfRange(RefusalReason),
}

impl InterveneError {
    /// R1565 §5.15 (PINION-PR82) — refuse a write because the value is outside
    /// the slot's range, **stating the range**.
    ///
    /// ```
    /// # use pinion_core::external::InterveneError;
    /// let err = InterveneError::out_of_range(format!("row {} is outside 0..{}", 99, 12));
    /// assert!(err.reason().is_some_and(|why| why.as_str().contains("0..12")));
    /// ```
    #[must_use]
    pub fn out_of_range(reason: impl Into<RefusalReason>) -> Self {
        Self::OutOfRange(reason.into())
    }

    /// R1565 §5.15 — the producer's sentence, when this is a failure that
    /// carries one.
    ///
    /// `None` for [`UnknownPath`](Self::UnknownPath),
    /// [`TypeMismatch`](Self::TypeMismatch) and [`ReadOnly`](Self::ReadOnly),
    /// and that asymmetry with [`OutOfRange`](Self::OutOfRange) is the design
    /// rather than a gap left in it. Each of those three is **fully determined
    /// by its variant**: there is one way for a path to be undeclared, one way
    /// for a `String` to arrive where an `Int` belongs, and one way for a slot
    /// to be unwritable. `OutOfRange` is the arm with a fact behind it that the
    /// variant cannot hold — the range — which is exactly the shape
    /// [`InvokeError::Rejected`] had, and the reason it is the only arm here
    /// that gained a payload.
    ///
    /// (The honest test of that claim: PINION-PR82's complaint was a producer
    /// knowing *which* of several facts and unable to say. For a read-only slot
    /// there is only one fact. For an out-of-range value there are as many as
    /// there are slots.)
    #[must_use]
    pub fn reason(&self) -> Option<&RefusalReason> {
        match self {
            Self::OutOfRange(reason) => Some(reason),
            _ => None,
        }
    }
}

/// R1564 §5.15 §2 #2 (PINION-PR82) — the sentence a producer attaches when it
/// refuses to fire an action: what an operator reads, and what an agent reasons
/// about.
///
/// # Why a refusal has to carry one
///
/// [`InvokeError::Rejected`] used to be a payload-free variant, so a producer
/// that knew *exactly* why it was refusing had nowhere to say it. The cost was
/// measured downstream rather than argued: over sprag's fifteen reachable CLI
/// failure paths, **six** print a list of causes joined by `or` — not because
/// the consumer is lazy, but because the daemon's own handler knew which one it
/// was and the wire had no slot for the answer. `sprag_host::workspace::
/// report_agent` refuses in exactly two places, "no detector installed" and "no
/// pane with that id", and the two demand completely different operator
/// actions; they arrived fused, as the string `InvokeRejected`.
///
/// The variant's own doc had already conceded the point — it listed
/// "preconditions unmet, statechart in a forbidding state, etc.", which is a
/// set, not a reason. This type is where that set collapses back to the member
/// the producer actually observed.
///
/// # What a good reason says
///
/// It names the **thing** and the **fact about it**, in the vocabulary the
/// caller used: `"no pane 999 on this host"`, not `"precondition failed"`. It
/// is prose for a human and for a model, never a discriminator — a consumer
/// that needs to branch reads the JSON-RPC error *code* (see
/// `pinion_rpc::ACTION_REFUSED`), which is exactly why that code was split out
/// of `-32602 Invalid params` in the same round. Matching on this text is the
/// thing this type exists to stop, not to enable.
///
/// # Past the toolkit
///
/// The toolkit's floor here is the absence of a channel: `invokeMethod` answers `bool`, `trigger()`
/// answers `void`, and a abstract button that declines a click reports nothing at
/// all. There is a toolkit API a refused action can put a sentence into,
/// so nothing here is parity — the shape is chosen ([[the
/// toolkit-is-the-floor-not-the-target]]).
///
/// `Cow` rather than `String` because the overwhelming majority of in-tree
/// reasons are fixed sentences known at compile time, and a refusal on a hot
/// decode path should not allocate to say so; a runtime reason that interpolates
/// the offending value ([`InvokeError::rejected`] takes both) is the case that
/// pays.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefusalReason(Cow<'static, str>);

impl RefusalReason {
    /// A fixed sentence, known at compile time — no allocation.
    #[must_use]
    pub const fn stated(reason: &'static str) -> Self {
        Self(Cow::Borrowed(reason))
    }

    /// The sentence, for rendering onto a wire or into a log.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The sentence as a `Cow`, so a static reason reaches the wire without an
    /// allocation the borrow already avoided.
    #[must_use]
    pub fn into_cow(self) -> Cow<'static, str> {
        self.0
    }
}

impl From<&'static str> for RefusalReason {
    fn from(reason: &'static str) -> Self {
        Self(Cow::Borrowed(reason))
    }
}

impl From<String> for RefusalReason {
    fn from(reason: String) -> Self {
        Self(Cow::Owned(reason))
    }
}

impl From<Cow<'static, str>> for RefusalReason {
    fn from(reason: Cow<'static, str>) -> Self {
        Self(reason)
    }
}

impl std::fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Failure modes for [`ExternalIntrospect::invoke`] (R17 bidirectional
/// RPC spec round — symbolic action channel, third leg of the
/// query / intervene / invoke triad).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// Path is not declared as an action in the schema.
    UnknownPath,
    /// Args variant does not match the action's declared argument
    /// type.
    TypeMismatch,
    /// Path exists and args type matches, but the action refused to
    /// fire. Distinct from `TypeMismatch` because retrying with
    /// different args may succeed.
    ///
    /// R1564 — carries the producer's own [`RefusalReason`]. Build it with
    /// [`rejected`](Self::rejected) rather than the tuple constructor, so a
    /// `&'static str` and a `format!` reason are written the same way.
    ///
    /// The reason is **required by the type**: there is no arm of this variant
    /// that refuses anonymously, and that is deliberate. An optional reason is
    /// a reason nobody supplies — the same argument
    /// [`ArgDomain::Open`]'s doc makes about a domain that must never be a
    /// default. The cost is that every producer states something; the benefit
    /// is that no operator ever reads a refusal that names nothing.
    Rejected(RefusalReason),
}

impl InvokeError {
    /// R1564 §5.15 — refuse to fire, stating why.
    ///
    /// Takes `&'static str`, `String` or `Cow<'static, str>`, so a fixed
    /// sentence costs no allocation and an interpolated one
    /// (`format!("no pane {id}")`) needs no ceremony:
    ///
    /// ```
    /// # use pinion_core::external::InvokeError;
    /// let id = 999;
    /// let fixed = InvokeError::rejected("the detector is not installed");
    /// let interpolated = InvokeError::rejected(format!("no pane {id} on this host"));
    /// assert_ne!(fixed, interpolated);
    /// ```
    #[must_use]
    pub fn rejected(reason: impl Into<RefusalReason>) -> Self {
        Self::Rejected(reason.into())
    }

    /// R1564 §5.15 — the producer's sentence, when this is a refusal that
    /// carries one. `None` for [`UnknownPath`](Self::UnknownPath) /
    /// [`TypeMismatch`](Self::TypeMismatch), whose meaning is the variant
    /// itself and whose wire word the transport owns.
    #[must_use]
    pub fn reason(&self) -> Option<&RefusalReason> {
        match self {
            Self::Rejected(reason) => Some(reason),
            _ => None,
        }
    }
}

/// ★★★★★ R1699 — **what a person reads when an action refuses.**
///
/// `Debug` is what eight call sites across three screens were using to put a
/// refusal in front of somebody, and `Debug` is Rust syntax: the dashboard's
/// palette announced
/// `refused: Rejected(RefusalReason("\"topology\" is reserved for requirement
/// 12 and this release does not place it"))` to a person who had asked to place
/// a widget. Found by looking at a demo's own output rather than by any check,
/// because every check reads the typed value and the typed value was right.
///
/// The two anonymous arms render as sentences too rather than as their variant
/// names, because a consumer that has to special-case them will not: it will
/// fall back to `Debug` and reintroduce exactly this.
impl fmt::Display for InvokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPath => f.write_str("that is not an action this widget offers"),
            Self::TypeMismatch => f.write_str("that is not the kind of value this action takes"),
            Self::Rejected(reason) => f.write_str(reason.as_str()),
        }
    }
}

/// Opt-in symbolic introspection (§5.15 item 8). An `External` exposes
/// this sub-trait by overriding [`External::introspect`] /
/// [`External::introspect_mut`] to return `Some(self)`.
///
/// The triad of operations (R17 bidirectional RPC spec round):
///   * [`schema`](Self::schema): declare which paths exist.
///   * [`query`](Self::query): read a value at a path (`&self`).
///   * [`intervene`](Self::intervene): write a value to a slot
///     (`&mut self`, returns `()`).
///   * [`invoke`](Self::invoke): trigger an action with args
///     (`&mut self`, returns `IntrospectValue`).
///
/// The split: `intervene` writes a *slot* (idempotent assignment),
/// `invoke` calls an *action* (event-shaped, may return a computed
/// value such as the resulting state). Schemas may declare a path as
/// either a state slot (write via intervene) or an action (call via
/// invoke); §5.3 DSL settles whether the schema distinguishes them
/// explicitly.
///
/// Designed dyn-safe (all methods take `&self` or `&mut self`,
/// no associated items, no `Self`-returning methods) so the framework
/// can hold `&dyn ExternalIntrospect` for path-driven dispatch under
/// the §5.12 `query` / `snapshot` / `rewind` / `invoke` RPC methods.
pub trait ExternalIntrospect {
    /// Schema of introspectable state.
    fn schema(&self) -> IntrospectSchema;

    /// Read the value at `path`, or say why not.
    ///
    /// R1667 §5.15 — this answered `Option<IntrospectValue>` until the read
    /// channel was given a reason to state. See [`ReadRefusal`] for which arm a
    /// surface owes each caller, and for the three facts that used to arrive as
    /// one word.
    ///
    /// # Errors
    ///
    /// Returns [`ReadRefusal`] per the variants there.
    ///
    /// # A read CAN take an argument: encode it in the path
    ///
    /// (R1352 §5.12 §2 #2 PR-61) There is no `args` parameter here, and that
    /// reads at a glance like "a parameterized read is impossible" — a
    /// consumer who reached exactly that conclusion from this signature
    /// routed their offset-bearing read through [`invoke`](Self::invoke)
    /// instead, and paid for it (see below). It is not impossible. The
    /// **argument rides the path**, and the workspace does this widely:
    ///
    /// * `width.<col>` — [`ColumnWidthExternal`](crate::widgets::column_widths)
    /// * `id_at.<pos>` / `level_at.<pos>` — [`tree_nav`](crate::widgets::tree_nav)
    /// * `name.<idx>` / `is_dir.<idx>` — [`file_browser`](crate::widgets::file_browser)
    /// * `state.<idx>` / `expanded.<idx>` — [`disclosure_group`](crate::widgets::disclosure_group)
    ///
    /// An impl serves one by matching the prefix before its exact-path arm:
    ///
    /// ```ignore
    /// fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
    ///     if let Some(rest) = path.strip_prefix("width.") {
    ///         let col: usize = rest.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?;
    ///         if col >= self.col_count() {
    ///             return Err(ReadRefusal::no_such_member(
    ///                 format!("column {col} is outside 0..{}", self.col_count()),
    ///             ));
    ///         }
    ///         return Ok(IntrospectValue::Int(self.width(col).into()));
    ///     }
    ///     match path { /* argument-free paths */ _ => Err(ReadRefusal::UnknownPath) }
    /// }
    /// ```
    ///
    /// The two refusals in that body are the point of the signature: before
    /// R1667 both were `None`, so a client that asked for `width.zzz` and a
    /// client that asked for `width.999` were told the same thing, and neither
    /// was told the truth.
    ///
    /// Declare the family in [`schema`](Self::schema) with
    /// [`SchemaField::parametric`], whose path is the wire TEMPLATE
    /// (`"width.<col>"`) — see the next section. `query("width")` with no index
    /// correctly resolves to `None`, and a snapshot skips parametric families by
    /// declaration, so a family never pollutes one.
    ///
    /// ## The schema SAYS a path is parametric — do not make a client guess
    ///
    /// (R1352 found this hole; R1353 closed it.) A parametric family is declared
    /// with [`SchemaField::parametric`], which carries the wire template and a
    /// typed [`SchemaArg`] per placeholder — so `$schema` renders
    /// `{"path": "width.<col>", "type": "int", "args": [{"name": "col",
    /// "type": "int", "domain": {"kind": "index_of", "count_path": "cols"}}]}`
    /// where a scalar renders a bare `{"path": "total", "type": "int"}`.
    /// A client reads the arity, the argument's type, and where its valid values
    /// come from, instead of inferring any of it.
    ///
    /// It briefly did not. `("width", "int")` and `("total", "int")` rendered
    /// identically, so an agent had to guess that one of them took an argument —
    /// and the guess failed quietly, because an out-of-range `width.999` answered
    /// with the min clamp rather than an error. That hole is why a consumer
    /// reading this signature concluded a parameterized read was impossible at
    /// all: the convention was undiscoverable from the surface built to reveal
    /// it. Both halves are addressed — the declaration states the contract, and
    /// an out-of-range read no longer fabricates a value.
    ///
    /// The fix went into [`IntrospectSchema`], **not** into an `args` parameter
    /// here: that would fork the read surface in two and orphan every family
    /// listed above.
    ///
    /// ## Two things it does NOT settle
    ///
    /// **The separator has no escape.** An argument is delimited by `.`, and
    /// nothing escapes a `.` *inside* an argument. So a key that contains one is
    /// not addressable: `voice.<id>.gain` cannot express the id `"x.gain"`
    /// (matching takes the first trailing literal, so `voice.x.gain.gain` matches
    /// nothing), and a template's final argument swallows the rest, so
    /// `cell.<row>.<col>` reads `cell.1.2.3` as `col = "2.3"` — owned by the
    /// field, malformed, [`ReadRefusal::QueryTypeMismatch`] from `query`. This is
    /// safe today only because every argument in the workspace is an integer
    /// index or a dot-free id. A family keyed by a filename or a dotted path
    /// needs an escaping rule first; declaring one without it would be a promise
    /// the wire cannot keep.
    ///
    /// **Out-of-range has two spellings**, both honest and both in the tree:
    /// [`ReadRefusal::NoSuchMember`] — "there is no such member, and here is the
    /// range that has some" — from the surfaces that guard the index explicitly
    /// (`column_widths`, `listbox`, `radio_group`, `disclosure_group`, `table`,
    /// `file_browser`, `row_style`, and any other that bounds before reading),
    /// and `Ok(Null)` — "that position holds nothing" — from everything routed
    /// through `at_index`, which `map_or`s a missing element to `Null`
    /// (`tree_nav`, `tree_filter`, `grid_sort`, `view_order`, `group_order`,
    /// `row_search`, …). Treat neither list as exhaustive; the rule, not the
    /// roster, is what holds: **neither fabricates**, and that is the property
    /// `r1353_declared_domains_hold_on_real_widgets` enforces.
    ///
    /// R1667 did not unify them, and the choice is now a stated one rather than
    /// an artifact: before, both spellings were `Option` arms and the difference
    /// between "absent" and "empty" was only visible to a reader of the impl.
    /// A refusal and a `Null` are now different *types* of answer, so a surface
    /// picks its spelling on the wire, deliberately, at each site.
    ///
    /// # Why this matters more than it looks
    ///
    /// `scene/query` is classified `MethodOcc::Read`, so it does **not** bump
    /// the scene revision. `scene/invoke` is `Mutate` and does. A read
    /// disguised as an invoke therefore *broadcasts a state change on every
    /// read* — and a client that also parks on `scene/waitFor` (which waits on
    /// that revision) wakes its own waiter with its own read. That loop was
    /// measured at ~30Hz: a core burnt at idle, ~4 CPU-hours on a day-old
    /// instance, and a wedged socket. Splitting the concept into extra
    /// argument-free paths to dodge it (one slot for "live", one for
    /// "historical") only moves the damage into the binding's wire vocabulary
    /// — one concept, two addresses, forever.
    ///
    /// So: a read that needs an argument is a `query` with the argument in the
    /// path. It is a read, and it stays free.
    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal>;

    /// Write `value` to `path`. Errors when the path is unknown, the
    /// value does not match the slot type, or the slot is read-only.
    ///
    /// # Errors
    ///
    /// Returns [`InterveneError`] per the variants above.
    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError>;

    /// Trigger the action at `path` with `args`, returning a typed
    /// result value (e.g. the new state after a state-machine
    /// transition). Default impl returns `Err(InvokeError::UnknownPath)`
    /// so existing `External` impls remain valid without opting in to
    /// the action channel.
    ///
    /// # Errors
    ///
    /// Returns [`InvokeError`] per the variants above.
    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Err(InvokeError::UnknownPath)
    }
}

/// R1407 §5.35 §5.22 — the payload a `Ctrl`/`Cmd`+C copy chord would write for
/// an introspectable external, or `None` when the key is not the copy chord OR
/// the external has nothing to copy (an empty or non-`Text` `query_field`). The
/// caller performs the OS side effect with its own clipboard tag:
///
/// ```ignore
/// if let Some(payload) = selection_copy_payload(intro, key, modifiers, FIELD) {
///     use_app_clipboard(TAG).copy(payload);
///     return true;
/// }
/// ```
///
/// `query_field` is the **AI-first peer**: the SAME serialization an
/// out-of-process `query <field>` reads, so a keyboard copy and an AI client
/// share one path. Copy is a *read* plus an OS side effect (CQS): this queries
/// the payload through the `&self` [`query`](ExternalIntrospect::query) — it
/// never mutates the external — and returns the string; the OS write (a binding's
/// `use_app_clipboard(TAG).copy(payload)`) stays at the call site.
///
/// Returning the payload rather than writing it here is forced by layering, not
/// preference: the platform clipboard (`pinion-platform-clipboard`) depends on
/// `pinion-core`, so a core helper that performed the write would invert that
/// edge into a dependency cycle. The split also keeps the decision unit-testable
/// **without** a clipboard, so the real OS clipboard is never raced from a test.
///
/// The `Ctrl`-OR-`Cmd`-not-`Alt` gate mirrors the `text_field` chord decode: on
/// layouts where `AltGr` is `Ctrl`+`Alt`,
/// [`command_key`](Modifiers::command_key) is `true` while the keypress composes
/// a character, so without the `!alt_key` guard `AltGr`+C would misfire the copy
/// and swallow the char (R1223). The key match is case-insensitive so
/// `Shift`+`Ctrl`+`C` (platforms deliver `"C"`) still copies.
///
/// This lifts the byte-identical copy chord+query its three consumers had grown
/// (`hello-cell-select` R1222, `hello-data-grid` R1372, `hello-hex-dump` R1407).
/// Each consumer's divergent part — *what* to serialize — stays in its own
/// `query` (the field name it passes), not here. The `text_field` widget's own
/// `Ctrl`+C is a distinct consumer class (an attached clipboard +
/// `TextEditState::selection_text`, not an introspect query) and deliberately
/// does NOT route through this helper.
#[must_use]
pub fn selection_copy_payload(
    intro: &dyn ExternalIntrospect,
    key: &str,
    modifiers: Modifiers,
    query_field: &str,
) -> Option<String> {
    if !(modifiers.command_key() && !modifiers.alt_key() && key.eq_ignore_ascii_case("c")) {
        return None;
    }
    match intro.query(query_field) {
        Ok(IntrospectValue::Text(payload)) => Some(payload),
        _ => None,
    }
}

/// R738 §5.35 / R786 §5.35 — the rect a captured widget's cursor is
/// normalized against (returned by [`External::capture_normalize`]). One
/// exhaustive decision rather than the bool + `Option` pair it replaced, so a
/// widget cannot simultaneously request its primary *and* a named tag (an
/// illegal state the precedence rule used to resolve silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureNormalize<'a> {
    /// The grabbed (sub-)tag's own rect — the default. Correct for single-tag
    /// capture widgets and composites whose drag value is sub-region-relative
    /// (a dock panel tear-off measured against the grabbed header).
    Target,
    /// The primary half of the composite tag (`primary#sub` → `primary`). The
    /// range slider grabs a thumb sub-tag but its value spans the track.
    Primary,
    /// An explicitly named element's rect — for a drag whose reference is
    /// neither the grabbed tag nor its primary (column resize → grid viewport).
    Tag(&'a str),
}

/// R1101 §5.15 §5.51 — the live-drag context the router forwards to a drag
/// SOURCE on every move ([`External::drag_to_at`]) and the matching release
/// ([`External::drag_release_at`]). One struct rather than the widening
/// positional-argument ladder the `_at` pair had grown into (3 → 4 → … args):
/// the [`CaptureNormalize`] precedent applied to forwarding data. A new
/// dimension of drag context is a new field here, NOT a new positional arg
/// every `External` impl and call site must re-thread — so adding one leaves
/// the `drag_to_at` / `drag_release_at` SIGNATURE (and thus every drag source's
/// impl) untouched; only the router and the test builders that *construct* a
/// `DragUpdate` change.
///
/// Every field is data the **router alone** holds — a source `External` only
/// ever sees rect-relative coordinates — so the router is the single producer
/// and the source consumes. In particular [`became_drag`](Self::became_drag) is
/// the router's click-vs-drag verdict: a drag source MUST read it rather than
/// re-derive "is this press a drag yet" from its own distance tracking. A
/// private re-derivation drifts from the router's SSOT because it cannot see
/// the real press point (only the router's `press_gestures` does); it can only
/// approximate the origin with the first post-move sample, so it latches later
/// and at a different distance (the R1097 → R1101 `detach_latch` clearance).
///
/// Not `#[non_exhaustive]`: the router (pinion-runtime) and the drag-source
/// tests construct it with a struct literal across crate boundaries, exactly
/// as they construct [`DropPoint`] — the established precedent for a
/// cross-crate-constructed drag data carrier. `#[non_exhaustive]` would forbid
/// that literal and force a positional constructor, relocating the very
/// arg-ladder this struct removes.
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdate<'a> {
    /// The drop location currently under the cursor (the full tag + cursor
    /// normalised over its rect), or `None` over no tagged region — the
    /// rect-relative signal [`drag_to`](External::drag_to) already carried.
    pub over: Option<DropPoint>,
    /// The absolute **window-logical** cursor `(x, y)` the router holds (the
    /// same frame `scene/layout` reports). The only live pointer signal once
    /// `over` goes `None` (the cursor escaped every tag — the tear-off case).
    pub cursor: (f64, f64),
    /// The spec id of the window whose drop target `over` resolved in, when the
    /// cursor escaped THIS (the source's) window into ANOTHER and the shell
    /// resolved the drop there (`pinion_runtime::resolve_cross_window_drop`).
    /// `None` = the own / source window (every same-window drag). The shell
    /// composes this; the per-window router stays cross-window-blind, so the
    /// window dimension rides the `_at` path only.
    pub over_window: Option<&'a str>,
    /// (R1107 §5.16 §5.41 §5.51) The spec id of the window the drag is happening
    /// IN — the window whose router is driving this gesture, so `cursor` is in
    /// THIS window's logical frame. The symmetric peer of `over_window` (the
    /// TARGET window) at the DATA layer: `over_window` names where the drop
    /// lands, `source_window` names where the cursor is measured. (Their
    /// POPULATION is asymmetric: `over_window` is per-gesture transient session
    /// state the shell composes and pushes onto the router; `source_window` is
    /// the router's OWN permanent window identity, stamped once at the
    /// per-window dispatch seam — the router stays cross-window-blind for
    /// resolution but knows which window it IS.) A tear-off follow needs it to convert
    /// the window-logical cursor to a DESKTOP position via the SOURCE window's
    /// outer origin — re-dragging an already-floating panel's header reports a
    /// cursor in that floating window's frame, not the main window's, so a
    /// binding that assumed `"main"` placed the follower wrong. `None` = the
    /// router did not record its window id (a single-window / pre-R1107 path);
    /// the binding falls back to its primary window. The router fills it from
    /// its own window id, which the shell sets at the per-window dispatch choke.
    pub source_window: Option<&'a str>,
    /// The router's click-vs-drag verdict for the in-flight press: `true` once
    /// it strayed past `DRAG_CLICK_THRESHOLD_PX` *from the real press point*.
    /// The single SSOT the router owns (`press_gestures`) — a drag source reads
    /// it to decide "is this a drag yet" (a dock panel tearing off) instead of
    /// opening its own latch, which would drift.
    pub became_drag: bool,
    /// (R1117 §5.15 §5.51) The window-logical cursor at the PRESS that opened
    /// this drag session — constant for the gesture. `cursor` is the LIVE
    /// position; `press_cursor` is where the grab started. A grab-offset drag
    /// (relocate a borderless floater by its title bar, keeping the grabbed
    /// point under the cursor) computes `cursor - press_cursor` — the
    /// displacement since the grab. Without it a source could only anchor on the
    /// first MOVE sample, which already lags the press by the first motion delta
    /// (a constant mis-grab, ~1px for a real mouse but systematically wrong). The
    /// router fills it from the cursor it held when `begin_drag` opened the
    /// session, so it is exact regardless of how far the first move strays.
    pub press_cursor: (f64, f64),
}

/// The 8-point integration contract (§5.15). Items 1-3 are required;
/// items 4-7 have no-op defaults so authors override selectively.
///
/// `Debug` is a super-trait so `Box<dyn External>` participates in the
/// scene tree's `#[derive(Debug)]` machinery (§5.2 `ExternalNode`).
pub trait External: core::fmt::Debug {
    // --- 1. Backend support declaration ---

    /// Which backends this `External` dispatches into, and the policy
    /// for unsupported ones.
    fn backends(&self) -> BackendSupport;

    // --- 2. Repaint trigger ownership ---

    /// Whether the framework drives repaints (layout cadence) or the
    /// `External` owns its own render loop.
    fn repaint_ownership(&self) -> RepaintOwner;

    // --- 3. Thread ownership ---

    /// UI-thread synchronous, or `External`-owned worker thread.
    fn thread_ownership(&self) -> ThreadOwnership;

    // --- 4. Lifecycle event callbacks ---

    fn on_mount(&mut self) {}
    fn on_unmount(&mut self) {}
    fn on_visibility_change(&mut self, _visible: bool) {}
    fn on_focus_change(&mut self, _focused: bool) {}

    /// R1185 §5.16 §5.35 — re-project declarative policy from a freshly
    /// rebuilt descriptor onto this preserved live handle during an
    /// external-set reconcile.
    ///
    /// [`CoreShell::reconcile_externals`](../../pinion_runtime/core_shell/struct.CoreShell.html#method.reconcile_externals)
    /// keeps the LIVE `External` node for every surviving tag — preserving
    /// the in-flight gesture / session state (a drag capture, an id-minting
    /// counter, a lifecycle chart) that a blind rebuild would reset — and
    /// drops the freshly built handle. That is right for in-flight state,
    /// but it also freezes any **declarative policy** the factory computes
    /// as a reactive projection of binding state: a dock panel's
    /// `movable` / `floatable` lock that must flip when the last docked
    /// pane may no longer tear off is recomputed by the factory on every
    /// reconcile, yet the fresh value never reaches the live node, so the
    /// flag stays frozen at first construction. The reconcile hands the
    /// live handle the `fresh` descriptor here so a widget can copy its
    /// reactive-but-declarative fields across while retaining its own
    /// in-flight state.
    ///
    /// `fresh` is the just-built handle for the SAME tag (the same concrete
    /// type as this `&mut self`). Read its declared policy through the
    /// introspection channel already provided for exactly this cross-`dyn`
    /// read ([`introspect`](Self::introspect) +
    /// [`ExternalIntrospect::query`]) — no downcast required. Called on
    /// every reconcile pass for every surviving surface, so keep it to a
    /// few field copies, and do NOT touch in-flight state (that is the
    /// state the reconcile preserved this live node to keep).
    ///
    /// Default: ignore `fresh`, keep every field as-is — the pre-R1185
    /// behaviour, so an `External` with no reactive policy (and every
    /// existing impl) is unaffected.
    fn reconcile_from(&mut self, _fresh: &dyn External) {}

    // --- 5. Input forwarding policy ---

    /// Return `true` to claim the event (framework does *not* forward
    /// it further); `false` lets the framework process normally. Default
    /// is `false` — the framework forwards every event.
    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    /// R51.34 §5.15 + §5.35 — pointer-capture opt-in. When `true`, the
    /// framework's [`InputRouter`](crate#) keeps the cursor lock on this widget across
    /// the `pointer_down` → `pointer_up` span even when the cursor strays outside the widget rect
    /// (Material / `SwiftUI` / the toolkit gesture-recognizer convention) — `cursor_moved`
    /// forwards the cursor to the widget and **suppresses the `PointerLeave` that hover
    /// re-resolution would otherwise fire** for any stray, so a small jitter
    /// during the press cannot cancel it.
    ///
    /// R741 §5.35: button-like widgets (Button / Toggle / Checkbox /
    /// Radio) override this to `true` so a real-mouse click is robust to
    /// the sub-pixel jitter between press and release (before R741 they
    /// defaulted `false`, so a 1px stray fired `PointerLeave → Idle` and
    /// the click silently cancelled — the canonical toolkits all capture
    /// to avoid exactly this). They pair it with
    /// [`cancel_on_release_off_target`](Self::cancel_on_release_off_target)
    /// `= true` so a *deliberate* slide-off-and-release still cancels.
    ///
    /// Drag-aware widgets (Slider in R51.35, drag-to-resize, range
    /// pickers) override to `true` with the default
    /// `cancel_on_release_off_target = false` (release commits the value
    /// wherever the cursor ended). The router still dispatches
    /// `PointerDown` / `PointerUp` symbolic events to the widget via
    /// `ExternalIntrospect::invoke("send", ...)`; the difference is
    /// purely in the cursor-leave handling.
    fn wants_pointer_capture(&self) -> bool {
        false
    }

    /// R1405 §5.35 — opt into receiving [`pointer_move`](Self::pointer_move)
    /// on **plain hover** (no button held), not only under a capture-lock.
    ///
    /// By default the router forwards `pointer_move` to a widget only while it
    /// holds pointer capture (a drag); a free hover delivers only the
    /// `Enter` / `Leave` boundary events, never the intra-widget position. A
    /// widget that must react to *where inside it* the pointer is on hover —
    /// a `TextGrid` lighting the OSC-8 link cell under the cursor, a chart
    /// crosshair, a map tooltip — returns `true`, and the router then also
    /// forwards each hover move's position (the same rel-coord
    /// `pointer_move(x_rel, y_rel)` the capture path uses). Independent of
    /// [`wants_pointer_capture`](Self::wants_pointer_capture): a widget can
    /// want hover positions without capturing the press.
    fn wants_hover_move(&self) -> bool {
        false
    }

    /// R1416 §5.35 §5.15 — opt into the **raw multi-button pointer stream**:
    /// receive [`raw_pointer_button`](Self::raw_pointer_button) for EVERY mouse
    /// button (left / middle / right) on BOTH the press and release edge, each
    /// carrying the held modifiers, with the button identified.
    ///
    /// The default pipeline routes only the LEFT button to a widget (as the
    /// `send`-wire `PointerDown` / `PointerUp`), and its right / middle presses
    /// drive GUI *default actions* instead — a right press opens the
    /// own-renderer context menu, a middle press-release pastes the PRIMARY
    /// selection, a middle drag pans. A widget that IS the pointer authority
    /// for its region — a terminal pane forwarding xterm mouse reports, a game
    /// viewport, a remote-desktop surface — needs the raw edges, not those
    /// GUI interpretations. Returning `true` makes the router deliver each
    /// button edge to this widget through
    /// [`raw_pointer_button`](Self::raw_pointer_button) and **suppress the GUI
    /// default** for it (no context menu, no paste, no pan, no legacy
    /// `PointerDown` / `PointerUp` send wire).
    ///
    /// The suppression is scoped to THIS widget: every other widget keeps the
    /// standard button semantics (left = focus / select, middle = PRIMARY
    /// paste, right = context menu). Only the widget that owns the raw stream
    /// trades them for the raw edges — the W3C model, where a listener that
    /// handles `mousedown` / `contextmenu` opts out of the browser's default.
    ///
    /// R1715 — and it is scoped to this widget's GUI *defaults*, the four
    /// listed above. The dispatch's own post-processing is not one of them: a
    /// raw sink is user code like any other widget body, so a
    /// [`focus_request`](crate::focus_request) it writes from
    /// [`raw_pointer_button`](Self::raw_pointer_button) is resolved before the
    /// next paint exactly as one written from `invoke` would be. Losing the
    /// GUI default is in fact why it needs that mailbox at all — click-to-focus
    /// is one of the things being suppressed, so a pane that wants the keyboard
    /// on its own click has no other channel to ask (PINION-PR89).
    ///
    /// Independent of, and usually paired with,
    /// [`wants_hover_move`](Self::wants_hover_move) (or
    /// [`wants_pointer_capture`](Self::wants_pointer_capture)): those forward
    /// the cursor POSITION a raw sink correlates each button edge against;
    /// this forwards the button EDGES. A raw sink wants both.
    fn wants_raw_pointer_buttons(&self) -> bool {
        false
    }

    /// R1416 §5.35 §5.15 — deliver one raw mouse-button edge to a widget that
    /// opted into the multi-button stream via
    /// [`wants_raw_pointer_buttons`](Self::wants_raw_pointer_buttons).
    ///
    /// Called for each left / middle / right press and release while this
    /// widget is the pointer target (the hover target under the cursor, or its
    /// captured target while it holds capture). The [`RawPointerButton`] carries
    /// the button, the press/release edge, and the modifiers held at that edge;
    /// the cursor POSITION arrives separately via
    /// [`pointer_move`](Self::pointer_move) (see the type docs). Default no-op,
    /// so a widget that does not opt in never sees this.
    fn raw_pointer_button(&mut self, _event: RawPointerButton) {}

    /// R741 §5.35 — release-position policy for a captured widget.
    /// Consulted only when [`wants_pointer_capture`](Self::wants_pointer_capture)
    /// is `true`. On `pointer_up`, the router checks whether the cursor
    /// is still over this widget:
    ///
    /// * `false` (default, drag widgets) — release always dispatches
    ///   `PointerUp` (the drag commits its value wherever the cursor
    ///   ended; a Slider released past the track edge still commits the
    ///   clamped value).
    /// * `true` (button-like widgets) — release **over** the widget
    ///   dispatches `PointerUp` (activate); release **off** the widget
    ///   dispatches `PointerLeave` (cancel). This is the standard
    ///   button "press, slide off to abort, release off = no-op"
    ///   gesture, made reachable now that capture suppresses the
    ///   mid-press leave.
    fn cancel_on_release_off_target(&self) -> bool {
        false
    }

    /// R1549 §5.35 §5.38 — press-and-hold **auto-repeat** declaration: the
    /// cadence at which a press the user keeps holding re-activates this
    /// widget, or `None` (default) for a widget that fires once per press.
    /// The toolkit `setAutoRepeat` /
    /// `setAccelerated`, asked of the widget rather than
    /// configured on it.
    ///
    /// # It is asked, not stored — and that is what makes a runaway repeat
    /// unrepresentable
    ///
    /// The router re-asks on **every** frame of a hold, so this is a
    /// *level* read of the widget's own state, never an edge the router
    /// latched at press time. A widget answers `Some` only while it is
    /// genuinely held:
    ///
    /// ```ignore
    /// fn auto_repeat(&self) -> Option<AutoRepeat> {
    ///     (self.inc.state() == ButtonState::Pressed).then(AutoRepeat::desktop)
    /// }
    /// ```
    ///
    /// so a press that slid off the target (its statechart already left `Pressed` on
    /// `PointerLeave`), a widget that disabled itself mid-hold, or one whose value hit
    /// its bound all stop repeating with no un-arming code anywhere. The
    /// toolkit keeps a basic timer that a missed release / hide / disable path
    /// can leave running — the classic runaway-spinbox bug class — because
    /// *arming* and *being pressed* are two facts there that have to be kept
    /// in agreement.
    ///
    /// A `None` answer mid-hold also **resets** the router's ramp, so
    /// sliding off a button and back on restarts from the delay rather
    /// than resuming at speed (the toolkit's `mouseMoveEvent` does the same).
    ///
    /// # Which sub-region
    ///
    /// Composite widgets answer for the sub-region they recorded as
    /// pressed: `PointerDown` reached the widget before the router can
    /// ask, exactly as [`begin_drag`](Self::begin_drag) relies on. The
    /// widget is therefore the authority on its own sub-regions, and the
    /// router never needs to parse a composite tag to decide a cadence.
    ///
    /// # What a repeat actually does
    ///
    /// The router re-dispatches the widget's own activation arc (`PointerUp` then `PointerDown`)
    /// — the toolkit's `released(); clicked(); pressed();` in statechart vocabulary. There is no separate
    /// "repeat" event, so a repeat cannot mean anything different from a
    /// click, and no widget has to grow an SCXML transition to be repeatable.
    fn auto_repeat(&self) -> Option<crate::input::AutoRepeat> {
        None
    }

    /// R1569 §5.39 §5.20 — while **focused**, does this widget claim `chord`
    /// ahead of the window's accelerator layers?
    ///
    /// The layers are the §5.20 mnemonic map (R1543, <kbd>Alt</kbd>+char) and
    /// the binding's [`WidgetCore::keybinding`](crate::WidgetCore::keybinding)
    /// character map. Both fire from anywhere in the window regardless of
    /// focus, which is what an accelerator *is* — and which is wrong for the
    /// widget the user is typing into. Answering `true` routes the key
    /// straight to `apply_key` instead, exactly as if no accelerator had been
    /// declared for it.
    ///
    /// The toolkit spells this `ShortcutOverride`, an event offered to the
    /// focus widget which it `accept()`s to claim the key. The capability is
    /// the toolkit's floor; the shape is chosen ([[the
    /// toolkit-is-the-floor-not-the-target]]), and the choice is that this is
    /// a **question**, for two reasons:
    ///
    /// 1. The toolkit's override must be accepted on *every* press, so a widget that
    ///    handles a key in `keyPressEvent` and forgets the `ShortcutOverride`
    ///    arm in `event()` loses exactly the presses that collide with a
    ///    shortcut — invisible until someone adds the colliding shortcut, in
    ///    another file. A widget cannot be asked too late here.
    /// 2. A question can be evaluated **without a keystroke**, so
    ///    `scene/accelerators` can publish which declared accelerators are
    ///    live right now and which are shadowed and by whom. The toolkit cannot: the
    ///    event is transient and shortcut map is private.
    ///
    /// # Per chord, deliberately
    ///
    /// The right answer differs by chord for the same widget.
    /// [`TextFieldExternal`](crate::widgets::text_field::TextFieldExternal) claims a bare `d`
    /// (it is text — the toolkit's line edit does the same) but **not**
    /// <kbd>Alt</kbd>+<kbd>F</kbd>, so mnemonics keep working while typing.
    /// [`KeySequenceEditExternal`](crate::widgets::key_sequence::KeySequenceEditExternal) claims
    /// everything while recording, because recording a chord means recording
    /// *that* chord.
    ///
    /// # Only the focused widget is asked
    ///
    /// The toolkit's scope too (`ShortcutOverride` goes to the focus widget), and it is what
    /// bounds the mechanism: at most one widget can shadow, and it is the one
    /// the user is typing into. An unfocused widget is never consulted, so a
    /// `true` here can never make a window's accelerators mysteriously inert.
    ///
    /// Default `false` — the accelerator layers keep the precedence they had
    /// before R1569, so no existing widget changes behaviour by omission.
    fn shadows_accelerator(&self, _chord: &crate::accelerator::Chord) -> bool {
        false
    }

    /// R880 §5.35 §5.49 — opt-in for the **bare** (non-composite) send wire
    /// to carry the R781 held-modifier token. When `true`, a background
    /// dispatch with a non-empty modifier state reaches `invoke("send", ...)`
    /// as the three-segment wire `":<EventName>:<token>"` — the key segment
    /// is *empty* (a background press has no sub-target), so
    /// [`split_send_payload`](crate::composite_tag::split_send_payload)
    /// decodes it through the exact same `:` grammar SSOT as a composite
    /// send, with `""` as the key. An empty modifier state always emits the
    /// plain `"<EventName>"` back-compat wire.
    ///
    /// Default `false`, because the bare payload doubles as the SCXML event
    /// name for the entire statechart-driven catalogue — an un-gated wire
    /// change would turn every `Ctrl`+click on a plain widget into an
    /// unmatchable `":PointerUp:c"` statechart event. Only a coordinator
    /// that decodes its own send wire (and needs background-release
    /// modifiers — e.g. a `Ctrl`/`Shift` marquee) should opt in, exactly as
    /// [`wants_pointer_capture`](Self::wants_pointer_capture) gates the
    /// capture machinery.
    fn wants_bare_send_modifiers(&self) -> bool {
        false
    }

    /// R738 §5.35 / R786 §5.35 — which post-layout rect the framework's
    /// [`InputRouter`](crate#) normalizes the dragged cursor against while this
    /// widget holds capture. One exhaustive [`CaptureNormalize`] decision —
    /// `Target` (default), `Primary`, or `Tag(name)` — so a widget cannot ask
    /// for two rects at once (the bool + `Option` pair this replaced could).
    ///
    /// - [`CaptureNormalize::Target`] (default): the grabbed (sub-)tag's own
    ///   rect — correct for a single-tag capture widget and for a composite
    ///   whose value is sub-region-relative (a dock panel's tear-off fraction).
    /// - [`CaptureNormalize::Primary`]: the primary half of the composite tag —
    ///   the dual-thumb range slider tags thumbs `range#low` / `range#high` but
    ///   the value maps across the whole track, so it normalizes against the
    ///   primary (track) rect instead of the ~18px thumb.
    /// - [`CaptureNormalize::Tag`]: an explicitly named element's rect — the
    ///   column-resize handle's drag is a **pixel** delta needing a rect whose
    ///   width is **stable across the drag**; the grabbed cell resizes under it,
    ///   so the handle names the grid viewport (which does not resize when a
    ///   column does), exactly as the splitter normalizes against its stable
    ///   pane container, not the moving handle.
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Target
    }

    /// R51.34 §5.15 + §5.35 — pointer-move forward during drag. The
    /// framework's [`InputRouter`](crate#) calls this whenever the
    /// cursor moves while this widget holds capture (i.e. after a
    /// `pointer_down` on a `wants_pointer_capture` = true widget and
    /// before the matching `pointer_up`). `x_rel` / `y_rel` are
    /// normalised over the widget's post-layout rect: `0.0` is the
    /// left / top edge, `1.0` is the right / bottom edge.
    ///
    /// Coordinates may exceed `[0.0, 1.0]` (or be negative) when the
    /// cursor strays outside the rect under capture lock — the
    /// implementor decides whether to clamp (Slider does, since its
    /// value is `0.0..=1.0` normalised) or extrapolate (a future
    /// fling / overscroll gesture might not).
    ///
    /// Default no-op; widgets that need cursor-position state
    /// override. Non-drag widgets must not override — capture-lock
    /// without `pointer_move` is a valid stance (e.g. a future
    /// long-press widget that only cares about the dwell time, not
    /// the cursor X).
    fn pointer_move(&mut self, _x_rel: f32, _y_rel: f32) {}

    /// ★★★★★ R1700 §5.15 §5.35 §2 #7 — **what a press at this point
    /// addresses**, in this surface's own vocabulary. Half of the pair the
    /// framework needs to check a self-hit-testing surface against its own
    /// paint; [`target_of_tag`](Self::target_of_tag) is the other half.
    ///
    /// `x` / `y` are in this surface's OWN pixels — the space
    /// [`pointer_move`](Self::pointer_move)'s fractions resolve into, origin at
    /// the surface's top-left.
    ///
    /// # Why the framework asks
    ///
    /// §2 #7 makes a screen ONE `External` so an agent can query it, which puts
    /// the hit test in the screen's own code. The framework therefore knows
    /// every rectangle the screen painted and nothing about what pressing one
    /// would do, so its pointer guarantee (`scene/pointer_reach`) stops at the
    /// surface boundary. Measured at R1700 on the analyser's capture viewer:
    /// the framework vouched for **1 of the 291 tagged rectangles on screen**,
    /// and the other 290 were on the screen's honour. That is how a screen
    /// shipped in which every rectangle that moved under a resize had stopped
    /// being pressable where it was drawn — reported by a person twice, by a
    /// gate never.
    ///
    /// # Why two questions and not one
    ///
    /// A single "what tag is here" would need the surface to keep a map from
    /// its own vocabulary back to the tags it painted, and an inverse written
    /// by hand drifts from the thing it inverts (R1699 met exactly that and
    /// wrote the gate before the function). So the surface answers the SAME
    /// question two ways it already knows how to answer — geometrically, and by
    /// name — and the framework holds the two against the paint.
    /// `scene/pointer_target` runs the comparison over every painted rectangle
    /// in one pass, and its verdicts are `pointer_reach`'s vocabulary one level
    /// down: deliverable, inert, unreachable.
    ///
    /// # The contract
    ///
    /// The word must be the one this surface's wire answers with for the same
    /// press — the two are the same fact and a second spelling of it would make
    /// the check compare a surface with itself.
    ///
    /// [`PointerTarget::Nothing`] is a real answer: the gap between two
    /// controls addresses nothing, and that is correct.
    /// [`PointerTarget::Unanswered`] — the default — means this surface does
    /// not resolve presses to names at all, and keeps it out of the census
    /// rather than counting it as clean.
    ///
    /// # What the floor does here
    ///
    /// Nothing. The mature retained-mode toolkits this project is judged
    /// against have no equivalent: measured at 6.11, offscreen, a self-painting
    /// widget's eight painted marks are invisible to the framework's point
    /// lookup, which answers null; the scene-graph point lookup trusts an
    /// item's *declared* shape and finds nothing where a paint drew outside it;
    /// and no member enumerates what a widget painted at all, because the only
    /// framework-held record of a paint there is pixels, which carry no
    /// identity. Introspection-from-paint is what makes the comparison possible
    /// here.
    ///
    /// Default [`PointerTarget::Unanswered`]; a surface that hit-tests itself
    /// overrides.
    fn target_at(&self, _x: u32, _y: u32) -> PointerTarget {
        PointerTarget::Unanswered
    }

    /// ★★★★★ R1700 §5.15 §5.35 — **what the thing this surface painted under
    /// `tag` addresses**, in the same vocabulary
    /// [`target_at`](Self::target_at) answers in.
    ///
    /// The by-name half of the pair. A surface that already resolves a tag to
    /// something — for a keyboard activation, which names a thing rather than a
    /// pixel — implements this in one line over that.
    ///
    /// [`PointerTarget::Nothing`] for a rectangle that addresses nothing is the
    /// right answer and a common one: captions, rules and badges are painted
    /// and are not pressed. The framework then tolerates
    /// [`target_at`](Self::target_at) naming whatever the decoration sits on
    /// top of, which is the honest reading of a label inside its row.
    ///
    /// What it must NOT do is answer `Nothing` for something that IS
    /// addressable, because that turns a rectangle nobody can press into a
    /// rectangle nobody checks. The census publishes how many of a surface's
    /// painted rectangles are addressable, so under-answering shows up as a
    /// number rather than as a pass.
    ///
    /// Default [`PointerTarget::Unanswered`]; overridden together with
    /// [`target_at`](Self::target_at) or not at all.
    fn target_of_tag(&self, _tag: &str) -> PointerTarget {
        PointerTarget::Unanswered
    }

    /// R1423 §5.35 §5.15 — the current pointer PRESSURE for this widget, the
    /// W3C `PointerEvent.pressure` / the toolkit `pressure()` peer: a normalised `0.0..=1.0` force, `0.0` when no
    /// pressure is reported (a plain mouse, or a lifted pen). Forwarded
    /// alongside each [`pointer_move`](Self::pointer_move) (pressure travels WITH
    /// position, the W3C `pointermove` model) AND on a standalone pressure change (a pen
    /// pressing harder in place), so a pressure-aware surface — an ink brush
    /// whose width tracks force, a DCC viewport, a velocity-sensitive control
    /// — reads the live force without a separate device query.
    ///
    /// The native source is the platform pen / touch force (winit
    /// `Touch::force`, normalised); the AI-first source is the `scene/pointer_pressure`
    /// RPC (§2 #2), so the value is drivable and introspectable headless — a
    /// tablet is not required to exercise a pressure-reactive widget.
    ///
    /// Default no-op; only a widget that reacts to force overrides. A mouse
    /// reports `0.0` (the toolkit gives a mouse no tablet event either —
    /// pressure is a pen/touch axis, not a synthesised mouse-button level).
    fn pointer_pressure(&mut self, _pressure: f32) {}

    /// R1429 §5.35 §5.15 — the current pointer TILT for this widget, the W3C
    /// `PointerEvent.tiltX` / `tiltY` / the toolkit `xTilt()` / `yTilt()` peer: the pen's lean off the surface
    /// normal, in DEGREES, each axis `-90.0..=90.0`. `tilt_x` is the lean in the device X-Z
    /// plane (positive = the pen top tilts toward +X / screen right); `tilt_y` in
    /// the Y-Z plane (positive = the pen top tilts toward +Y / screen bottom).
    /// `(0.0, 0.0)` is a pen held perpendicular, and what a plain mouse reports (a
    /// mouse has no tilt, exactly as it has no pressure). Forwarded alongside
    /// each [`pointer_move`](Self::pointer_move) (tilt travels WITH position, the W3C `pointermove`
    /// model) AND on a standalone tilt change (a pen leaning in place), so a
    /// tilt-aware surface — a calligraphy nib whose stroke shape follows the
    /// lean, a DCC viewport — reads the live angle without a separate device
    /// query.
    ///
    /// winit 0.30 exposes no tablet-tilt axis, so the sole driver is the
    /// `scene/pointer_tilt` RPC (§2 #2, the AI-first primary path): the value is
    /// drivable and introspectable headless, no tablet required. A future winit
    /// tablet API, or a platform bridge, would populate it natively — the same
    /// place [`pointer_pressure`](Self::pointer_pressure) reads winit
    /// `Touch::force` today.
    ///
    /// Default no-op; only a widget that reacts to lean overrides.
    fn pointer_tilt(&mut self, _tilt_x: f32, _tilt_y: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer TWIST for this widget, the W3C
    /// `PointerEvent.twist` / the toolkit `rotation()` peer: the barrel rotation of an art pen about its
    /// own axis, in DEGREES clockwise, normalised `0.0..=360.0` (`0.0` = a plain pen /
    /// mouse, which has no barrel to turn). Forwarded WITH position like the
    /// tilt / pressure axes, so a twist-aware surface — a calligraphic nib
    /// whose broad edge follows the barrel, a pattern brush whose stamp
    /// rotates — reads the live angle.
    ///
    /// The sole driver is the `scene/pointer_twist` RPC (§2 #2): winit 0.30
    /// exposes no barrel-rotation axis, so the value is drivable / introspectable
    /// headless, no art pen required. Default no-op.
    fn pointer_twist(&mut self, _twist: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer TANGENTIAL PRESSURE for this
    /// widget, the W3C `PointerEvent.tangentialPressure` / the toolkit `tangentialPressure()` peer: the airbrush finger-wheel
    /// position, normalised `-1.0..=1.0` (`0.0` = the wheel's neutral rest, and what a
    /// plain pen / mouse reports — it has no wheel). Forwarded WITH position
    /// like the other axes, so an airbrush-aware surface reads the live wheel
    /// without a device query.
    ///
    /// The sole driver is the `scene/pointer_tangential_pressure` RPC (§2 #2):
    /// winit 0.30 exposes no finger-wheel axis. Default no-op.
    fn pointer_tangential_pressure(&mut self, _tangential: f32) {}

    /// R1430 §5.35 §5.15 — the current pointer HEIGHT for this widget, the
    /// toolkit `z()` peer: the pen's distance ABOVE the tablet surface while it
    /// hovers, `0.0` at contact and rising as the pen lifts (device units,
    /// non-negative — there is no W3C `PointerEvent` equivalent, so this is the
    /// toolkit-parity axis). Forwarded WITH position like the other axes, so a
    /// hover-height-aware surface — a preview that fades as the pen lifts, a
    /// depth-cued brush cursor — reads the live distance.
    ///
    /// The sole driver is the `scene/pointer_height` RPC (§2 #2): winit 0.30
    /// exposes no hover-distance axis. Default no-op.
    fn pointer_height(&mut self, _height: f32) {}

    /// R1431 §5.35 §5.15 — the DEVICE that produced the current pointer stream
    /// for this widget, the W3C `PointerEvent.pointerType` / the toolkit `pointerType()` peer: [`PointerKind::Mouse`] / `Pen` / `Eraser`
    /// / `Touch`. `Mouse` is the default — what a plain pointer reports. The `Eraser`
    /// variant is the stylus's eraser end (a toolkit distinction W3C folds
    /// into `"pen"`), so an eraser-aware surface — a paint canvas that flips to
    /// erase when the pen is inverted — reads the device without a query.
    /// Forwarded WITH position like the scalar axes.
    ///
    /// The sole driver is the `scene/pointer_type` RPC (§2 #2): winit 0.30 does
    /// not classify the pointer device. Default no-op.
    fn pointer_kind(&mut self, _kind: PointerKind) {}

    /// R1432 §5.35 §5.15 — a native PINCH (magnify) gesture over this widget,
    /// the toolkit native gesture event `ZoomNativeGesture` / macOS `magnify:` / W3C wheel-with-`Ctrl`
    /// peer: a two-finger trackpad pinch a viewport reads to zoom without a
    /// wheel or a button chord.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect the
    ///   [`wheel`](Self::wheel) / [`pointer_move`](Self::pointer_move) hooks use
    ///   (a pinch has no position of its own; it targets whatever the cursor
    ///   hovers, so a zoom anchored at the cursor and a drag share one basis).
    /// * `magnification` — the INCREMENTAL scale delta for this event (winit's
    ///   `PinchGesture::delta` / the macOS `magnification`): positive zooms in,
    ///   negative zooms out, `0.0` at rest. It is a per-event increment, not an
    ///   absolute factor, so a surface accumulates it across the
    ///   [`Begin`](GesturePhase::Begin)`..`[`End`](GesturePhase::End) arc
    ///   (each event does `scale *= 1.0 + magnification`) and drops the
    ///   accumulator on [`Cancel`](GesturePhase::Cancel).
    /// * `phase` — the gesture lifecycle ([`GesturePhase`]) bracketing the arc.
    /// * `modifiers` — the held keyboard modifiers (the same out-of-band cache
    ///   the wheel reads), so a `Shift`-constrained or `Alt`-fine zoom is one
    ///   hook.
    ///
    /// Return `true` to consume the gesture, `false` (default) to decline — the same
    /// consume contract as [`wheel`](Self::wheel), though a pinch has no `Scene::Scroll`
    /// default action to fall through to (the toolkit delivers a native
    /// gesture only to the widget under the cursor, with no scroll fallback),
    /// so declining is simply a no-op.
    ///
    /// The native source is the platform trackpad (winit
    /// `WindowEvent::PinchGesture`, macOS / iOS only); the AI-first source is
    /// the `scene/pinch_gesture` RPC (§2 #2), so a zoom-reactive viewport is
    /// drivable and introspectable headless with no trackpad. Default no-op;
    /// only a widget that zooms overrides.
    fn pinch_gesture(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _magnification: f64,
        _phase: GesturePhase,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R1433 §5.35 §5.15 — native two-finger ROTATION gesture, the
    /// [`pinch_gesture`](Self::pinch_gesture) sibling with rotation in place of scale. The
    /// toolkit native gesture event `RotateNativeGesture` / macOS `rotateWithEvent:` peer: a trackpad twist an
    /// editor reads to rotate a canvas / gizmo without a modifier chord.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect the
    ///   [`pinch_gesture`](Self::pinch_gesture) / [`wheel`](Self::wheel) hooks
    ///   use (a rotation has no position of its own; it targets whatever the
    ///   cursor hovers, so a twist anchored at the cursor and a drag share one
    ///   basis).
    /// * `rotation` — the INCREMENTAL rotation delta for this event, in
    ///   **degrees** (winit's `RotationGesture::delta` / the macOS
    ///   `rotation` / the toolkit's `RotateNativeGesture` value, all degrees): winit's
    ///   sign convention is positive = **counter-clockwise**, negative =
    ///   clockwise, `0.0` at rest. It is a per-event increment, not an absolute
    ///   angle, so a surface accumulates it across the
    ///   [`Begin`](GesturePhase::Begin)`..`[`End`](GesturePhase::End) arc
    ///   (each event does `angle += rotation`) and drops the accumulator on
    ///   [`Cancel`](GesturePhase::Cancel).
    /// * `phase` — the gesture lifecycle ([`GesturePhase`]) bracketing the arc.
    /// * `modifiers` — the held keyboard modifiers (the same out-of-band cache
    ///   the pinch / wheel read), so a `Shift`-snap-to-15° or `Alt`-fine twist
    ///   is one hook.
    ///
    /// Return `true` to consume the gesture, `false` (default) to decline — the same
    /// consume contract as [`pinch_gesture`](Self::pinch_gesture), and like a pinch a
    /// rotation has no default action to fall through to (the toolkit delivers
    /// a native gesture only to the widget under the cursor, with no
    /// fallback), so declining is simply a no-op.
    ///
    /// The native source is the platform trackpad (winit
    /// `WindowEvent::RotationGesture`, macOS / iOS only); the AI-first source is
    /// the `scene/rotation_gesture` RPC (§2 #2), so a rotation-reactive gizmo is
    /// drivable and introspectable headless with no trackpad. Default no-op;
    /// only a widget that rotates overrides.
    fn rotation_gesture(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _rotation: f64,
        _phase: GesturePhase,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R1434 §5.35 §5.15 — native N-finger PAN gesture, the
    /// [`pinch_gesture`](Self::pinch_gesture) /
    /// [`rotation_gesture`](Self::rotation_gesture) sibling with a
    /// **two-dimensional** delta in place of a single scalar. The toolkit
    /// native gesture event `PanNativeGesture` / winit
    /// `WindowEvent::PanGesture` peer: the trackpad / touch pan a map or a
    /// canvas reads to translate its content by direct manipulation.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect the
    ///   [`pinch_gesture`](Self::pinch_gesture) / [`wheel`](Self::wheel) hooks
    ///   use (a pan has no position of its own; it targets whatever the cursor
    ///   hovers, so a pan and a drag share one coordinate basis).
    /// * `delta_x` / `delta_y` — the INCREMENTAL pan for this event, in
    ///   **logical pixels** (the framework converts winit's physical
    ///   `PhysicalPosition` delta at the boundary, exactly as it does for a
    ///   pixel wheel). The sign is the platform's raw finger movement —
    ///   positive `delta_x` = the fingers moved right, positive `delta_y` =
    ///   down — deliberately NOT sign-flipped the way [`wheel`](Self::wheel) is:
    ///   a wheel is a *scroll command* (W3C: positive `dy` reveals content
    ///   below), a native pan is *direct manipulation* (the content follows the
    ///   fingers). A surface accumulates the deltas across the
    ///   [`Begin`](GesturePhase::Begin)`..`[`End`](GesturePhase::End) arc
    ///   (`offset += delta`) and drops the accumulator on
    ///   [`Cancel`](GesturePhase::Cancel).
    /// * `phase` — the gesture lifecycle ([`GesturePhase`]) bracketing the arc.
    /// * `modifiers` — the held keyboard modifiers (the same out-of-band cache
    ///   the pinch / rotation / wheel read), so a `Shift`-axis-locked or
    ///   `Alt`-fine pan is one hook.
    ///
    /// Return `true` to consume the gesture, `false` (default) to decline — the same
    /// consume contract as [`pinch_gesture`](Self::pinch_gesture); like every native
    /// gesture a pan has no default action to fall through to (the toolkit
    /// delivers it only to the widget under the cursor), so declining is
    /// simply a no-op. Note this is the NATIVE gesture, not the framework's
    /// drag-to-pan: a held pointer drag stays on the pointer hooks.
    ///
    /// The native source is the platform trackpad / touchscreen (winit
    /// `WindowEvent::PanGesture`, iOS only); the AI-first source is the
    /// `scene/pan_gesture` RPC (§2 #2), so a pannable viewport is drivable and
    /// introspectable headless with no trackpad. Default no-op; only a widget
    /// that translates its content overrides.
    fn pan_gesture(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _delta_x: f32,
        _delta_y: f32,
        _phase: GesturePhase,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R1435 §5.35 §5.15 — native SMART-ZOOM gesture, the last member of the
    /// native-gesture family and the one shaped unlike the others: the toolkit
    /// native gesture event `SmartZoomNativeGesture` / macOS `smartMagnifyWithEvent:` / winit `WindowEvent::DoubleTapGesture` peer — a two-finger
    /// double tap that a document view reads to zoom the *object* under the
    /// cursor to fit, and to restore when tapped again.
    ///
    /// * `x_rel` / `y_rel` — the cursor normalised over the SAME rect the
    ///   [`pinch_gesture`](Self::pinch_gesture) / [`wheel`](Self::wheel) hooks
    ///   use. This is the whole payload, and it is not incidental: the "smart"
    ///   in smart-zoom is that the anchor SELECTS what to fit (the paragraph,
    ///   the cell, the node under the finger), so a widget that ignores it has
    ///   implemented a plain zoom toggle, not this gesture.
    /// * `modifiers` — the held keyboard modifiers (the same out-of-band cache
    ///   the other gestures read).
    ///
    /// **DISCRETE, not an arc**: unlike [`pinch_gesture`](Self::pinch_gesture) /
    /// [`rotation_gesture`](Self::rotation_gesture) / [`pan_gesture`](Self::pan_gesture)
    /// there is no [`GesturePhase`] and no incremental delta — the platform
    /// reports one completed toggle, so there is nothing to accumulate and
    /// nothing a `Cancel` could discard. Each call is one committed state
    /// change; a widget toggles rather than integrates.
    ///
    /// Return `true` to consume the gesture, `false` (default) to decline — the
    /// same consume contract as the sibling gestures, with no default action to
    /// fall through to. Note this is NOT the pointer double-click (`scene/double_click`,
    /// two press/release cycles): that is a mouse-button event with a click
    /// count, this is a trackpad gesture with no button at all.
    ///
    /// The native source is the platform trackpad (winit
    /// `WindowEvent::DoubleTapGesture`, macOS 10.8+ / iOS); the AI-first source
    /// is the `scene/smart_zoom_gesture` RPC (§2 #2), so a fit-to-view surface is
    /// drivable and introspectable headless with no trackpad. Default no-op;
    /// only a widget with a zoom-to-fit notion overrides.
    fn smart_zoom_gesture(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
        false
    }

    /// R877 §5.15 §5.49 — wheel-input forward (the §5.15 item-5 input-
    /// forwarding leg the pointer hooks left open). The framework's
    /// [`InputRouter`](crate#) offers a wheel event to the `External`
    /// resolved from the tag under the cursor *before* falling back to
    /// the `Scene::Scroll` dispatch — the W3C model where a wheel
    /// listener on the event path may consume (`preventDefault`) ahead
    /// of the scroll default action; the offered widget can be an
    /// ancestor of a deeper `Scroll` (the canvas-hijack pattern), and
    /// declining preserves the inner scroll chain.
    ///
    /// The whole event arrives as one [`WheelReading`] — where the cursor is
    /// (normalised over the SAME rect
    /// [`capture_normalize`](Self::capture_normalize) selects for
    /// [`pointer_move`](Self::pointer_move), so a zoom anchored under the
    /// cursor and a drag share one coordinate basis), the delta in logical
    /// pixels on the W3C sign convention, the gesture
    /// [phase](crate::input::GesturePhase), and the held modifiers.
    ///
    /// Return `true` to consume the event (the router stops — no scroll
    /// dispatch); `false` to decline, letting the wheel fall through to the
    /// nearest [`Scene::Scroll`] ancestor exactly as if this widget were not
    /// there.
    ///
    /// ## R1703 — the declaration is the precondition
    ///
    /// This hook is **only called on a widget whose
    /// [`wheel_intent`](Self::wheel_intent) is `Some`**. A widget that answers
    /// `None` is never offered the event, so "what a wheel does here" cannot
    /// drift from what a wheel here does: removing the declaration removes the
    /// behaviour, and the wire's answer (`scene/wheel_intent`) is the same
    /// value the router routed by.
    ///
    /// The default is therefore not "decline" but *unreachable* — a widget
    /// with no declaration never arrives, and one with a declaration owes an
    /// implementation.
    ///
    /// [`Scene::Scroll`]: crate::Scene::Scroll
    /// [`WheelReading`]: crate::widgets::wheel::WheelReading
    fn wheel(&mut self, _reading: &crate::widgets::wheel::WheelReading) -> bool {
        false
    }

    /// R1703 §5.45 §5.15 — **what a wheel at this point over this widget
    /// does**, or `None` when a wheel there is not this widget's business and
    /// belongs to whatever scrolls behind it.
    ///
    /// `at` is normalised over the same basis
    /// [`wheel`](Self::wheel) receives, and it is a parameter for a reason this
    /// round measured rather than anticipated: the first draft asked the
    /// surface with no point, and on a screen — which §2 #7 makes ONE
    /// `External` — the wire then answered "a wheel here zooms" over the
    /// palette, where the screen's own handler declines. The published answer
    /// was coarser than the behaviour it claimed to be, which is the exact
    /// drift this method exists to make impossible. A widget whose whole rect
    /// means one thing ignores the argument; a screen reads it.
    ///
    /// The reference toolkit has no such question. Measured at 6.11.1 by
    /// building a probe and running it: over the four widget classes that
    /// answer a wheel there, 309 introspectable properties and 172
    /// introspectable methods contain **zero** naming the wheel, so the only
    /// way to learn that a control will eat your scroll is to scroll and find
    /// out. The same probe measured what that costs: a **closed, unfocused**
    /// combo box in a form changes its value on a wheel aimed at the form.
    ///
    /// Declaring it makes three things true that a hand-written `wheelEvent`
    /// cannot:
    ///
    /// * **the router uses it** — no declaration, no offer (see
    ///   [`wheel`](Self::wheel)), so the answer and the behaviour are one fact;
    /// * **the wire publishes it** — `scene/wheel_intent` answers for the
    ///   surface under a point, so an agent, a test or an audit of a form can
    ///   ask *before* moving anything;
    /// * **it can change with the widget's state** — a combo box declares a
    ///   step only while its list is open, which is the form hazard above
    ///   answered by construction rather than by every consumer installing an
    ///   event filter.
    fn wheel_intent(&self, _at: (f32, f32)) -> Option<crate::widgets::wheel::WheelIntent> {
        None
    }

    // --- 5b. Drag-and-drop source / coordinator (R742 §5.51) ---

    /// R742 §5.51 — drag-source hook. The framework's
    /// [`InputRouter`](crate#) calls this immediately after it dispatches
    /// `PointerDown` to this widget. Returning `Some(payload)` **starts a
    /// drag session**: the router pins this pointer's hover (so the
    /// statechart sees no spurious `PointerLeave` mid-drag, exactly like
    /// capture) and, on every subsequent cursor move, resolves the drop
    /// location under the *absolute* cursor and forwards it back to this
    /// widget via [`drag_to_at`](Self::drag_to_at) (the rect-relative
    /// [`DropPoint`] **plus** the absolute window-logical cursor; its default
    /// delegates to the cursor-less [`drag_to`](Self::drag_to)), then once
    /// via [`drag_release_at`](Self::drag_release_at) on the matching
    /// `pointer_up`.
    ///
    /// Why the *source* receives the updates, not the hovered target: an
    /// `External` only ever sees rect-relative coordinates and the router
    /// routes the whole press → release gesture to the pressed widget, so
    /// no widget can resolve "what is under the cursor" on its own — only
    /// the router holds the absolute cursor plus the full paint layout.
    /// The router does the hit-test and hands the resolved [`DropPoint`]
    /// to the coordinator that started the drag. Every drop candidate for
    /// the in-tree reorder consumers (reorder list, tab bar, dock, tree)
    /// belongs to that one coordinator, so the source *is* the resolver —
    /// the pointer-driven generalisation of the invoke-driven dock
    /// `resolve_dock_drop`. A future cross-widget drop (palette → canvas)
    /// adds a target-side hook without changing this source contract.
    ///
    /// Called on `&self`: arming is observation of state the matching
    /// `PointerDown` already recorded (which sub-region was pressed), not
    /// a mutation. Default `None` — the widget is not a drag source and
    /// no session starts, so every pre-R742 `External` is unaffected.
    fn begin_drag(&self) -> Option<DragPayload> {
        None
    }

    /// (R1348 §5.51 PR-57) Does this drag source ACCEPT the synthetic
    /// [`OUTER_DOCK_ZONE_TAG`] perimeter zone the router is about to CLAIM at
    /// `point`? `false` ⇒ the router does not claim it and the plain inner
    /// hit-test applies instead, exactly as in the band's interior — so the
    /// widget UNDER the perimeter keeps its own drop bands. Default `true`
    /// (claim as before), so every pre-R1348 `External` is unaffected: the
    /// router only ASKS on the same-window override, which is gated on
    /// [`DOCK_PANEL_DRAG_KIND`]. (That gate is not a claim that a non-dock drag
    /// never meets the sentinel — the CROSS-window perimeter resolver is
    /// drag-kind-BLIND and can put [`OUTER_DOCK_ZONE_TAG`] in a non-dock drag's
    /// `over`. It is unvetoable today; see the carry on
    /// `InputRouter::resolve_drag_own_over`.)
    ///
    /// **Why the SOURCE answers.** Whether a perimeter zone is worth claiming is
    /// a question about the source's own model (for a dock: "does an outer band
    /// at that edge reach any arrangement an inner split does not?"), which the
    /// router cannot answer — it holds geometry, not topology, and the crate that
    /// owns the topology is its SIBLING, not its dependency. This is the
    /// [`begin_drag`](Self::begin_drag) contract carried one step further: the
    /// source *is* the resolver, so the source also decides which synthetic
    /// targets are offered to it. The router asks BEFORE stealing the hit-test,
    /// so a zone the source would only reject cannot be claimed in the first
    /// place.
    ///
    /// **The invariant this closes.** R1201 declared the VS Code / the toolkit
    /// ADS rule — *an outer drop indicator is offered only when the outcome
    /// differs* — but enforced it one layer too LATE, at RESOLVE (`resolve_drop_checked` mapped a
    /// redundant perimeter drop to a stay-put `SnapBack`). The claim still happened,
    /// so the outcome died while the CLAIM survived: the band previewed
    /// nothing, did nothing, and masked the split bands of the panel beneath
    /// it — a dead strip. A source that answers this the same way it resolves
    /// makes "claimed but inert" unrepresentable, rather than merely unwanted.
    /// Implementors MUST therefore answer with the SAME predicate their `drag_release`
    /// resolves with, so claim and outcome cannot drift.
    fn accepts_outer_dock(&self, _payload: &DragPayload, _point: &DropPoint) -> bool {
        true
    }

    /// R742 §5.51 — live drag update. Called on every cursor move while a
    /// session this widget started via [`begin_drag`](Self::begin_drag)
    /// is in flight. `over` is the drop location currently under the
    /// cursor, or `None` when the cursor is over no tagged region. The
    /// widget updates its drop-preview state (e.g. the insertion index a
    /// reorder list highlights) — typically by writing a shared
    /// `Rc<Signal<_>>` the view fn also reads, so the highlight
    /// re-renders reactively. Default no-op.
    fn drag_to(&mut self, _payload: &DragPayload, _over: Option<DropPoint>) {}

    /// R742 §5.51 — drop commit. Called once on `pointer_up` with the
    /// final drop location (`None` when released over no tagged region).
    /// The widget applies the move / reorder and clears its drop-preview
    /// state. The router *also* dispatches the normal `PointerUp` to the
    /// source afterwards, so a press-release-in-place (no real drag) still
    /// reaches the statechart as a click. Default no-op.
    fn drag_release(&mut self, _payload: &DragPayload, _over: Option<DropPoint>) {}

    /// R1093 §5.15 §5.51 — live drag update WITH the full [`DragUpdate`]
    /// context. The §5.15 input-forwarding ENRICHMENT (not a break):
    /// [`drag_to`](Self::drag_to) forwards only the rect-relative [`DropPoint`]
    /// — which is `None` the moment the cursor escapes every tagged region,
    /// exactly the dock tear-off case — so a coordinator that must place
    /// something **at the cursor** (a floating window that follows the pointer)
    /// had no way to read where the cursor actually is, whether the press has
    /// become a drag, or which window the cursor escaped into. The router calls
    /// THIS method on every move with all of that ([`DragUpdate::cursor`] /
    /// [`became_drag`](DragUpdate::became_drag) / [`over_window`](DragUpdate::over_window));
    /// its default delegates to [`drag_to`](Self::drag_to) with just the
    /// rect-relative `over`, so every pre-R1093 drag source is bit-identical and
    /// no widget receives both calls. Override this **instead of** `drag_to` to
    /// receive the context. (R1101 collapsed the former positional `cursor` /
    /// `over_window` args into [`DragUpdate`]; see that struct for why a drag
    /// source consumes [`became_drag`](DragUpdate::became_drag) rather than
    /// re-deriving it.)
    fn drag_to_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_to(payload, update.over.clone());
    }

    /// R1093 §5.15 §5.51 — drop commit WITH the full [`DragUpdate`] context,
    /// the release sibling of [`drag_to_at`](Self::drag_to_at). The router calls
    /// this once on `pointer_up`; its default delegates to
    /// [`drag_release`](Self::drag_release) with just `over` so pre-R1093
    /// sources are unaffected. A coordinator that opens a floating window where
    /// the drag was released reads [`DragUpdate::cursor`] here (in the SOURCE
    /// window's logical frame; converting to a desktop position additionally
    /// needs the source window's outer position, which the shell owns) and
    /// [`over_window`](DragUpdate::over_window) to redock into another window.
    fn drag_release_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_release(payload, update.over.clone());
    }

    /// R937.1 §5.51 — drag ABORT. Called once when the OS revokes an
    /// in-flight drag this widget started (winit `TouchPhase::Cancelled` —
    /// a system gesture, phone call, app-switcher, edge-swipe), the
    /// drag-session counterpart of [`pointer_cancel`]. Unlike
    /// [`drag_release`](Self::drag_release), the widget must **discard** the
    /// gesture (NO move / reorder applied) and clear its drop-preview state —
    /// a cancel is "the drag never happened", so committing the last preview
    /// would be wrong. Default no-op (a widget with no preview / arm state to
    /// clear needs nothing). The router clears the drag session regardless,
    /// so even a no-op impl can no longer leave a dangling session.
    ///
    /// [`pointer_cancel`]: ../../pinion_runtime/input/struct.InputRouter.html#method.pointer_cancel
    fn drag_cancel(&mut self, _payload: &DragPayload) {}

    // --- 6. DPI / resize notification ---

    fn on_dpi_change(&mut self, _scale: f32) {}

    /// The surface this widget was laid out at changed size.
    ///
    /// ★★★ R1684.4 — **the framework records this for you; see
    /// [`surface_size`].** Implement it when the size has to be acted on
    /// (recomputing a cached layout, telling a game loop); do NOT implement it
    /// merely to remember the number, because remembering it by hand is what
    /// this arm's history is made of.
    fn on_resize(&mut self, _width: u32, _height: u32) {}

    // --- 7. Async state change channel (pull form) ---

    /// Poll for a state change pushed by the `External`. Default
    /// returns `None`. Push-form (channel-based) lands when §6.3 async
    /// boundary is settled at the runtime crate edge.
    fn poll_state(&mut self) -> Option<StateUpdate> {
        None
    }

    // --- §5.20 intent channel (R18; complements item 7's state-update
    //     poll with a symbolic event stream). ---

    /// Drain any pending [`Intent`]s into `sink`. Default no-op so
    /// existing `External` authors are unaffected. Implementors that
    /// emit intents (e.g. a button whose state machine just clicked)
    /// override this to flush their internal queue.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(Intent)) {}

    /// Return `true` when this `External` has pending intents the
    /// runtime should drain on the current frame. Default `false`.
    /// Used to skip the [`drain_intents`](Self::drain_intents) virtual
    /// call when there is nothing to harvest.
    fn is_dirty(&self) -> bool {
        false
    }

    // --- 8. Optional symbolic introspection (opt-in per §5.15 caveat) ---

    /// Surface the [`ExternalIntrospect`] view of this `External`, when
    /// the author opts in. Default returns `None`; override with
    /// `Some(self)` after `impl ExternalIntrospect for YourType`.
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        None
    }

    /// Mutable counterpart to [`introspect`](Self::introspect), used by
    /// the §5.12 `rewind` and `dry_run` paths.
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        None
    }
}

/// R810.1 §5.38 §5.12 — a reactive state holder that exposes a
/// **read-only** introspection view of itself: its declared schema and
/// the live value at each path. Implement this on a widget's reactive
/// holder (e.g. `ModalState`, `SnackbarTimer`) and wrap it in a
/// [`QueryOnlyIntrospect`] to get a query-only RPC node — no hand-rolled
/// `External` boilerplate, no second source of truth.
///
/// The "read-only" contract is enforced by [`QueryOnlyIntrospect`], not
/// here: any path in `introspect_schema` is
/// refused on `intervene` with [`InterveneError::ReadOnly`]. This is the
/// right shape when the state is *driver-coupled* — a modal's open flag
/// moves with its focus-trap, a snackbar's countdown is advanced by the
/// animation driver — so a raw rewind would desync it. Mutations go
/// through the holder's own methods (a reducer / action), never the wire.
/// R834 §5.12 — widen a `usize` count to the `i64` an
/// [`IntrospectValue::Int`] slot carries, saturating at [`i64::MAX`]
/// rather than wrapping or panicking. The single SSOT for the
/// introspect-count-widening decision (was hand-rolled in `widgets::table`
/// + two example bindings).
#[must_use]
pub fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

pub trait QuerySource {
    /// The declared query paths and their type-name tags. Usually a
    /// `'static` slice independent of `self` (the schema is fixed per
    /// type), kept in lockstep with [`introspect_query`](Self::introspect_query).
    fn introspect_schema(&self) -> IntrospectSchema;

    /// The live value at `path`, or why not.
    ///
    /// R1667 — this answered `Option` and reached the wire through
    /// [`QueryOnlyIntrospect`], so every surface built on it inherited the read
    /// channel's one-bit refusal. Widening the adapter alone would not have
    /// helped: the adapter would have had to *invent* a reason, which is the
    /// same defect wearing the fix's clothes.
    ///
    /// # Errors
    ///
    /// Returns [`ReadRefusal`] per the variants there.
    fn introspect_query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal>;
}

/// R847 §5.15 §5.40 — emit the [`External`] skeleton shared by every
/// **display-config proxy coordinator** (the sort/filter proxy
/// [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal),
/// the grid-sort proxy
/// [`GridSortExternal`](crate::widgets::grid_sort::GridSortExternal), the
/// group-by proxy
/// [`GroupOrderExternal`](crate::widgets::group_order::GroupOrderExternal)): a
/// `Gui`+`Rpc` backend, framework-owned repaint, UI-thread external that emits
/// **no §5.20 intent** (its state is a shared reactive holder whose `Signal`
/// writes already repaint every subscriber, so it is never independently dirty)
/// and surfaces that state through [`ExternalIntrospect`] only.
///
/// These five methods were byte-identical across all three proxies (the R847
/// audit's Rule-of-Three); this macro is their SSOT, so a change to (say)
/// [`thread_ownership`](External::thread_ownership) cannot silently diverge
/// between them. The type must implement [`ExternalIntrospect`] (the macro's
/// `introspect` / `introspect_mut` return `Some(self)`); each proxy keeps its
/// own `ExternalIntrospect` impl (the part that genuinely differs).
///
/// R858 — exported (`pub use` below) so a `Gui`+`Rpc` config-holder External in
/// an *example* (e.g. `hello-tree-filter`'s `TreeSortExternal`) reaches the same
/// SSOT instead of hand-rolling the skeleton:
/// `pinion_core::external::query_proxy_external_impl!(MyExternal);`.
#[macro_export]
macro_rules! query_proxy_external_impl {
    ($t:ty) => {
        impl $crate::external::External for $t {
            fn backends(&self) -> $crate::external::BackendSupport {
                $crate::external::BackendSupport::new(
                    &[
                        $crate::external::Backend::Gui,
                        $crate::external::Backend::Rpc,
                    ],
                    $crate::external::BackendFallback::Skip,
                )
            }
            fn repaint_ownership(&self) -> $crate::external::RepaintOwner {
                $crate::external::RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> $crate::external::ThreadOwnership {
                $crate::external::ThreadOwnership::UiThreadSync
            }
            fn introspect(&self) -> Option<&dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn introspect_mut(&mut self) -> Option<&mut dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
        }
    };
}
pub use query_proxy_external_impl;

/// The `intervene` error for a path with **no writable slot**: `ReadOnly` when
/// `schema` declares it (a real field, just not writable), `UnknownPath` when it
/// does not.
///
/// The §2 #7 distinction between "you cannot write this" and "this does not
/// exist" is worth keeping honest — an agent that gets `UnknownPath` for a field
/// it can plainly `query` learns something false about the surface. Every
/// read-only External needs exactly this rule, so it lives here rather than being
/// re-typed: `QueryOnlyIntrospect` below and `pinion-audio`'s RT External (whose
/// own copy admitted, in its docstring, to mirroring this one) both route through
/// it.
/// (R1353) Parametric members resolve through [`SchemaField::addresses`], so
/// `width.0` reports `ReadOnly` like the family it belongs to. Matching the
/// declared path exactly would have answered `UnknownPath` for it — telling an
/// agent that a path it can plainly `query` does not exist, which is the precise
/// lie this function was written to prevent.
///
/// # R1566 — the dispatcher now backstops this, and this still earns its place
///
/// The wire refusal is derived from the declaration at the RPC boundary, so a
/// surface that never calls this no longer publishes the lie. What that
/// derivation cannot do is the parametric case: it sees only `UnknownPath` and
/// cannot tell "this shape is not mine" from "this shape is mine and its
/// ARGUMENT addresses nothing", and a declared family may be writable
/// (`voice.<id>.gain` is), so concluding `ReadOnly` there would be a fresh
/// false statement. Called from an impl, after that impl's own arms, the
/// question is already settled — which is why a family member resolves
/// correctly here and is deliberately left alone out there.
#[must_use]
pub fn read_only_or_unknown(schema: &IntrospectSchema, path: &str) -> InterveneError {
    if schema.field_for(path).is_some() {
        InterveneError::ReadOnly
    } else {
        InterveneError::UnknownPath
    }
}

/// Move an inner `External`'s pending §5.20 [`Intent`]s into `sink`.
///
/// For the **wrapper** shape: an External that delegates to an inner one has to
/// forward the intents the inner queued, or they are simply lost. Written inline
/// that is not one line but three, because `src` and `sink` are usually two fields
/// of the same wrapper and the borrow checker needs them destructured apart:
///
/// ```ignore
/// let result = self.inner.invoke(path, args);
/// let Self { inner, pending_intents, .. } = &mut *self;   // <- only to split the borrow
/// inner.drain_intents(&mut |intent| pending_intents.push(intent));
/// ```
///
/// Taking the two as separate arguments splits the borrow at the call site, so
/// the dance disappears:
///
/// ```ignore
/// let result = self.inner.invoke(path, args);
/// forward_intents(&mut self.inner, &mut self.pending_intents);
/// ```
///
/// Lifted at the fourth byte-identical copy (three example bindings).
pub fn forward_intents(src: &mut impl External, sink: &mut Vec<Intent>) {
    src.drain_intents(&mut |intent| sink.push(intent));
}

/// R1276 §5.15 — the `External` skeleton for a **read-write introspection**
/// node: RPC-only (paints nothing — the binding's `view` is the paint
/// scene), framework repaint, UI-thread sync, and it drains a §5.20 intent
/// channel. The sibling of [`query_proxy_external_impl`] for nodes that also
/// `invoke` / `intervene` and emit intents (a cursor walk, an audio
/// controller), rather than being purely query-only.
///
/// The type must have a `pending_intents: Vec<Intent>` field (the macro's
/// `drain_intents` / `is_dirty` read it) and its own [`ExternalIntrospect`]
/// impl (the part that genuinely differs). This is the SSOT for the
/// otherwise byte-identical skeleton that `pinion-narrative`'s
/// `NarrativeExternal` and `pinion-audio`'s `AudioEngineExternal` had each
/// hand-rolled (the Rule-of-Three lift; the read-only case has
/// [`QueryOnlyIntrospect`]).
///
/// # Declaring thread ownership (§5.15 item 3)
///
/// The one-argument form declares [`ThreadOwnership::UiThreadSync`], which is
/// right for the common case — an External the framework calls straight from the
/// UI thread. An External that **owns a real OS thread** and is spoken to over a
/// channel (an audio device callback, say) must say so, or it lies about a
/// mandatory §5.15 item:
///
/// ```ignore
/// pinion_core::intent_query_external_impl!(DeviceAudioExternal, OwnThread);
/// ```
///
/// The name is any [`ThreadOwnership`] variant.
#[macro_export]
macro_rules! intent_query_external_impl {
    ($t:ty) => {
        $crate::intent_query_external_impl!($t, UiThreadSync);
    };
    ($t:ty, $thread_ownership:ident) => {
        impl $crate::external::External for $t {
            fn backends(&self) -> $crate::external::BackendSupport {
                $crate::external::BackendSupport::new(
                    &[$crate::external::Backend::Rpc],
                    $crate::external::BackendFallback::Skip,
                )
            }
            fn repaint_ownership(&self) -> $crate::external::RepaintOwner {
                $crate::external::RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> $crate::external::ThreadOwnership {
                $crate::external::ThreadOwnership::$thread_ownership
            }
            fn introspect(&self) -> Option<&dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn introspect_mut(&mut self) -> Option<&mut dyn $crate::external::ExternalIntrospect> {
                Some(self)
            }
            fn drain_intents(&mut self, sink: &mut dyn FnMut($crate::intent::Intent)) {
                for intent in self.pending_intents.drain(..) {
                    sink(intent);
                }
            }
            fn is_dirty(&self) -> bool {
                !self.pending_intents.is_empty()
            }
        }
    };
}
pub use intent_query_external_impl;

/// R810.1 §5.38 §5.12 — the generic **query-only** introspection
/// `External`: a node that paints nothing (RPC backend only), handles no
/// events, and forwards `schema` / `query` to its [`QuerySource`] while
/// refusing every `intervene` (read-only). It lifts the byte-identical
/// `External` boilerplate that `ModalIntrospect` (R795) and
/// `SnackbarIntrospect` (R810) had each hand-rolled — the
/// [[abstraction-needs-second-consumer]] payoff, made now rather than at
/// the 3rd consumer because pinion's AI-introspection thesis guarantees
/// every transient widget grows one of these. Bindings register it via a
/// thin `*_introspection_extra(tag, state)` helper
/// (`ExtraExternal::new(tag, Box::new(QueryOnlyIntrospect::new(state)))`).
#[derive(Debug)]
pub struct QueryOnlyIntrospect<S> {
    source: Rc<S>,
}

impl<S> QueryOnlyIntrospect<S> {
    /// Wrap a shared [`QuerySource`] as its query-only introspection
    /// node. The `Rc` is cloned, so the view / driver / this node all
    /// report the same live state.
    #[must_use]
    pub fn new(source: Rc<S>) -> Self {
        Self { source }
    }
}

impl<S: QuerySource + core::fmt::Debug + 'static> External for QueryOnlyIntrospect<S> {
    /// RPC-only: the node carries no pixels (the binding paints the real
    /// surface), so the visual backends skip it while §5.12 `query` still
    /// routes through it.
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl<S: QuerySource + core::fmt::Debug + 'static> ExternalIntrospect for QueryOnlyIntrospect<S> {
    fn schema(&self) -> IntrospectSchema {
        self.source.introspect_schema()
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        self.source.introspect_query(path)
    }

    /// Every declared slot is read-only; an undeclared path is
    /// `UnknownPath`. The schema is the single source of "which paths
    /// exist", so a slot can never drift between `query` and `intervene`.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(read_only_or_unknown(&self.source.introspect_schema(), path))
    }
}

/// Reference no-op `External`: Gui only, framework-driven repaint,
/// UI-thread synchronous. Useful as a baseline for tests and as a
/// minimal example for new `External` authors.
#[derive(Debug, Clone, Copy, Default)]
pub struct StubExternal;

impl StubExternal {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

/// Reference `External` opting *in* to symbolic introspection (§5.15
/// item 8). Exposes a single `count: int` slot via the [`ExternalIntrospect`]
/// trait; useful as a worked example and as a fixture for the §5.12
/// `query` / `rewind` RPC methods once they wire up.
///
/// Additionally demonstrates the §5.20 intent channel: every successful
/// `intervene` write enqueues a `"counted.changed"` intent carrying the
/// new value, which `drain_intents` / `is_dirty` flush into the runtime
/// queue. Keeps the existing fixture role intact (no `Copy` removal
/// breaks any test using `Box<dyn External>` storage).
#[derive(Debug, Clone, Default)]
pub struct CountedExternal {
    pub count: i64,
    pending_intents: Vec<Intent>,
}

impl CountedExternal {
    #[must_use]
    pub const fn new(count: i64) -> Self {
        Self {
            count,
            pending_intents: Vec::new(),
        }
    }
}

impl External for CountedExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending_intents.drain(..) {
            sink(intent);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending_intents.is_empty()
    }
}

impl ExternalIntrospect for CountedExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    // R1637 — the fixture's action, declared. It answered over
                    // the wire from R17 to R1636 while `$schema` never
                    // mentioned it, which is the defect the transport now makes
                    // unreachable rather than merely documentable.
                    SchemaField::action("increment", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "count" => Ok(IntrospectValue::Int(self.count)),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" => match value {
                IntrospectValue::Int(n) => {
                    self.count = n;
                    self.pending_intents.push(Intent::new_static(
                        "counted.changed",
                        IntrospectValue::Int(n),
                    ));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Action: add the integer arg to the running count and
            // return the new total. Demos the §5.15 invoke triad with
            // a minimal mutating action that returns a computed value.
            "increment" => match args {
                IntrospectValue::Int(delta) => {
                    self.count = self.count.saturating_add(delta);
                    Ok(IntrospectValue::Int(self.count))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::WindowEvent;

    /// ★★★★★ R1714 — and its sibling had none either, which two counterfactuals
    /// said before a person did.
    ///
    /// [`layout_point`] and [`into_layout`] carry the whole expression a
    /// self-hit-testing screen used to write out by hand, and the round that
    /// moved it there checked it through three screens and a demo. Breaking the
    /// direction of the pan (`+` to `-`) and swapping the two axes both left the
    /// entire `pinion-core` and `pinion-rpc` suites green — the R1712 shape
    /// exactly: the function that assembles the answer had no test, only the
    /// things reading its output did.
    ///
    /// The fixture discriminates on purpose. The surface is **not square**, the
    /// pan is **not symmetric**, and the fraction is **not a half**, so a swap of
    /// either pair changes the answer. A fixture built from equal numbers is one
    /// that cannot tell two answers apart, which is this session's other
    /// recurring finding.
    #[test]
    fn r1714_a_pointer_fraction_becomes_a_pixel_in_the_layouts_frame() {
        let tag = "r1714.layout_point";
        record_surface_size(tag, 800, 400);
        crate::shrink::forget_pan(tag);
        assert_eq!(
            layout_point(tag, (0.25, 0.75)),
            (200, 300),
            "a fraction of each axis times that axis's extent",
        );
        assert_eq!(
            into_layout(tag, (200, 300)),
            (200, 300),
            "and a screen with no pan is the identity",
        );

        let pan = crate::shrink::pan_state(tag);
        pan.set_max(120, 30);
        pan.scroll_to(120, 30);
        assert_eq!(crate::shrink::window_pan(tag), (120, 30));
        assert_eq!(
            layout_point(tag, (0.25, 0.75)),
            (320, 330),
            "the pan is ADDED — a window pixel is that much further into the layout",
        );
        assert_eq!(
            into_layout(tag, (200, 300)),
            (320, 330),
            "and the pixel door says the same thing as the fraction door",
        );
        // ★ The clamp is on the fraction, not on the answer: a fraction outside
        // `0..=1` is a pointer the framework never sends, and rounding it into
        // range is a smaller lie than multiplying by it.
        assert_eq!(layout_point(tag, (-1.0, 2.0)), (120, 430));
        crate::shrink::forget_pan(tag);
        forget_surface_size(tag);
    }

    /// ★★★★★ R1711 — the policy [`layout_size`] exists to spell once had, until
    /// this round, **no test at all**: three screens were the only thing
    /// exercising it, and the case that mattered needs a window under the floor
    /// on one axis and over it on the other, which none of them drove.
    ///
    /// The no-scope branch is the one under test because it is the one a unit
    /// test can drive honestly — the in-view branch reads the same value from
    /// the viewport signal and applies the same clamp below it.
    #[test]
    fn r1711_a_floor_is_applied_per_axis_and_not_all_or_nothing() {
        let tag = "r1711.layout_size";
        let floor = (1625, 360);
        let design = (1625, 900);
        record_surface_size(tag, 1900, 1200);
        assert_eq!(
            layout_size(tag, floor, design),
            (1900, 1200),
            "a window above the floor on both axes lays out at its own size",
        );
        record_surface_size(tag, 1506, 360);
        assert_eq!(
            layout_size(tag, floor, design),
            (1625, 360),
            "under the floor on WIDTH only: the height stays live. Before R1711 \
             this answered the design size (1625, 900) and pushed 900 pixels of \
             content into a 360-pixel window — nine marks out of reach, measured",
        );
        record_surface_size(tag, 1900, 200);
        assert_eq!(
            layout_size(tag, floor, design),
            (1900, 360),
            "and the mirror image: under the floor on height only",
        );
        record_surface_size(tag, 100, 100);
        assert_eq!(
            layout_size(tag, floor, design),
            floor,
            "under both floors, the layout stops shrinking and the window clips",
        );
        forget_surface_size(tag);
        assert_eq!(
            layout_size(tag, floor, design),
            design,
            "a surface nothing has painted answers the design size",
        );
        // The other spelling of "nothing has painted": inside a view scope the
        // viewport signal answers R1006's `(0, 0)` until the shell seeds it,
        // and a window of no extent is not a size to lay anything out in.
        let owner = crate::reactive::Owner::new();
        assert_eq!(
            owner.run(|| layout_size(tag, floor, design)),
            design,
            "R1006's `(0, 0)` is 'viewport unknown', which is the design size",
        );
    }

    /// R1353 §2 #2 — a declared arity that does not match the `query` impl is
    /// worse than no declaration: it is a confident lie on the one surface an
    /// agent is told to trust. These pin the two directions the declaration can
    /// be wrong, on a real widget.
    #[test]
    fn r1353_declared_arity_matches_the_real_query_impl() {
        use crate::widgets::column_widths::{ColumnWidthExternal, ColumnWidths};
        use std::rc::Rc;

        let ext = ColumnWidthExternal::new(Rc::new(ColumnWidths::new(vec![100, 200, 300])));
        let schema = ext.schema();

        // Direction 1: a PARAMETRIC declaration must not be answerable bare, and
        // must be answerable with an argument. (`width` declared parametric but
        // secretly readable as a scalar would make the arg a lie.)
        let width = schema
            .fields
            .iter()
            .find(|f| f.path == "width.<col>")
            .expect("width is declared");
        let [arg] = width.args else {
            panic!("width declares exactly one argument");
        };
        assert_eq!(width.path, "width.<col>", "the declared wire template");
        assert!(
            ext.query("width").is_err(),
            "a parametric stem must not answer bare",
        );
        assert!(ext.query("width.0").is_ok(), "…but answers with an arg");

        // Direction 2: the declared DOMAIN must be true. `IndexOf("cols")`
        // promises that `cols` is readable and that exactly `0..cols` answer —
        // the promise a client plans against instead of probing.
        let ArgDomain::IndexOf(count_path) = arg.domain else {
            panic!("width's domain is IndexOf");
        };
        let Ok(IntrospectValue::Int(cols)) = ext.query(count_path) else {
            panic!("the declared count_path {count_path:?} must itself be readable");
        };
        assert_eq!(cols, 3);
        for col in 0..cols {
            assert!(
                ext.query(&format!("width.{col}")).is_ok(),
                "every index below the declared count answers ({col})",
            );
        }
        assert!(
            ext.query(&format!("width.{cols}")).is_err(),
            "and the first index at/above it does NOT — an out-of-range read that \
             answered would be the fabricated value R1353 removed",
        );

        // Direction 3: a SCALAR declaration must answer bare. (`total` declared
        // scalar but secretly needing an arg is the original defect inverted.)
        assert!(
            schema
                .fields
                .iter()
                .find(|f| f.path == "total")
                .expect("total is declared")
                .args
                .is_empty(),
        );
        assert!(ext.query("total").is_ok(), "a scalar answers bare");
    }

    /// R1501 — a binding's `External` wrapper, reduced to what the audit needs:
    /// it declares a surface's schema and forwards the reads. Every real
    /// consumer of [`ColumnLayout`](crate::widgets::column_layout::ColumnLayout)
    /// is this plus a paint.
    struct LayoutProbe(crate::widgets::column_layout::ColumnLayout);

    impl ExternalIntrospect for LayoutProbe {
        fn schema(&self) -> IntrospectSchema {
            crate::widgets::column_layout::ColumnLayout::SCHEMA
        }
        fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
            self.0.query(path)
        }
        fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
            self.0.intervene(path, &value)
        }
    }

    /// R1353.1 §2 #2 — run a parametric widget's DECLARED arity against its real
    /// `query` impl.
    ///
    /// **Coverage is the widgets listed at the bottom, not all of them.** 32
    /// files declare a `parametric` field; this audits the ones `pinion-core` can
    /// construct, and a widget in another crate (`pinion-widget-paint`,
    /// `pinion-audio`, every example) cannot be reached from here at all. R1353's
    /// first cut claimed "every parametric widget" while auditing six — a test
    /// that names a coverage it does not have is worse than no test, because it
    /// answers the question "is this checked?" with a lie.
    ///
    /// What IS workspace-wide is the static half:
    /// `r1353_1_every_real_declaration_matches_its_template` scans every real
    /// declaration's source. This is the dynamic half — it needs a live widget,
    /// so it goes as far as linkage allows. Adding widget N+1 here is a manual
    /// line; the honest fix is a per-crate audit each crate runs over its own
    /// externals, which nothing has forced yet.
    #[test]
    fn r1353_declared_domains_hold_on_real_widgets() {
        use crate::widgets::column_widths::{ColumnWidthExternal, ColumnWidths};
        use crate::widgets::disclosure_group::DisclosureGroupExternal;
        use crate::widgets::listbox::ListBoxExternal;
        use crate::widgets::pagination::PaginationExternal;
        use crate::widgets::radio_group::RadioGroupExternal;
        use crate::widgets::row_search::{RowSearchExternal, RowSearchState};
        use crate::widgets::row_style::{RowStyleExternal, RowStyleState};
        use crate::widgets::table::TableExternal;
        use crate::widgets::toolbar::{ToolItem, ToolbarExternal};
        use std::rc::Rc;

        /// Assert every declared field of `ext` tells the truth about itself.
        fn audit(label: &str, ext: &dyn ExternalIntrospect) {
            for f in ext.schema().fields {
                if f.args.is_empty() {
                    // A scalar promises it reads as spelled. (`send` / action
                    // slots legitimately read `None` — they are write channels —
                    // so absence alone is not a defect here; the claim under test
                    // is only that a scalar never needs an argument.)
                    assert!(
                        !f.path.contains('<'),
                        "{label}: {:?} spells a placeholder but declares no args",
                        f.path,
                    );
                    continue;
                }
                // NOTE: R1353's first cut also asserted "a parametric field's
                // stem is not readable". It never caught anything: `Table`
                // legitimately declares BOTH a scalar `selected` ("is anything
                // selected") and a family `selected.<row>`, so the check needed a
                // special-case to pass at all, and against a verbatim
                // `literal_prefix` ("selected.") it is vacuous — that never reads.
                // Removed rather than kept as reassurance. The three checks below
                // are the ones that pin a real promise.
                for a in f.args {
                    let ArgDomain::IndexOf(count_path) = a.domain else {
                        // `ValuesOf` / `Open` are audited by their own surfaces;
                        // only `IndexOf` makes a bound claim this test can check.
                        continue;
                    };
                    // The promise: `count_path` is itself readable, and it is an
                    // int. A domain pointing at a path that does not exist is a
                    // dead end a client cannot follow.
                    let Ok(IntrospectValue::Int(n)) = ext.query(count_path) else {
                        panic!(
                            "{label}: {:?} declares domain IndexOf({count_path:?}), \
                             but that path does not read as an int",
                            f.path,
                        );
                    };
                    // A single-arg family is fully checkable: every index below
                    // the count answers, and the first one at it does not.
                    if f.args.len() == 1 && n > 0 {
                        let inside = f.path.replace(&format!("<{}>", a.name), "0");
                        assert!(
                            ext.query(&inside).is_ok(),
                            "{label}: {inside:?} is inside the declared domain but \
                             does not answer",
                        );
                        // Outside the declared domain, a read must not produce a
                        // VALUE. Several spellings of that are in the tree and all
                        // are honest: a refusal from the surfaces that guard the
                        // index explicitly, and `Ok(Null)` from everything routed
                        // through `at_index`. (See `ExternalIntrospect::query`; the
                        // rosters there are examples, not an exhaustive list.) The
                        // invariant that matters is none of those; it is that
                        // nothing plausible comes back. `width.999` answering `40`
                        // — a real-looking width for a column that does not exist —
                        // is what R1353 removed, and it is what this catches.
                        //
                        // R1667 — the refusal side is now a family of words rather
                        // than one, so this matches on `Err(_)` instead of naming
                        // an arm. Naming one would make the census a statement
                        // about WHICH refusal each surface picked, which is the
                        // surface's call and not this invariant's business.
                        let outside = f.path.replace(&format!("<{}>", a.name), &n.to_string());
                        let answer = ext.query(&outside);
                        assert!(
                            matches!(answer, Err(_) | Ok(IntrospectValue::Null)),
                            "{label}: {outside:?} is OUTSIDE the declared domain \
                             (count={n}) yet answered {answer:?} — a client that \
                             trusts the declaration cannot tell that apart from a \
                             real value",
                        );
                    }
                }
            }
        }

        audit(
            "column_widths",
            &ColumnWidthExternal::new(Rc::new(ColumnWidths::new(vec![100, 200, 300]))),
        );
        audit("listbox", &ListBoxExternal::new(4));
        audit("radio_group", &RadioGroupExternal::new(3));
        audit("disclosure_group", &DisclosureGroupExternal::new(3));
        audit(
            "row_search",
            &RowSearchExternal::new(Rc::new(RowSearchState::new(
                2,
                vec![vec!["a".into(), "b".into()], vec!["c".into(), "d".into()]],
            ))),
        );
        audit(
            "toolbar",
            &ToolbarExternal::new(vec![ToolItem::Command, ToolItem::Toggle]),
        );
        audit("pagination", &PaginationExternal::new(4, 0));
        // R1501 — reachable from here for the first time because the header's
        // declaration moved OUT of `hello-column-reorder` and into the module
        // that answers it. While it lived in an example, neither half of this
        // audit could see it: this one links only what `pinion-core` links, and
        // the static scan reads `parametric(` call sites, which is precisely
        // what `logical_index_at.<x>` was not — it was declared a scalar while
        // spelling a placeholder, alongside five paths never declared at all.
        audit(
            "column_layout",
            &LayoutProbe(crate::widgets::column_layout::ColumnLayout::new(vec![
                150, 90, 100,
            ])),
        );
        // `with_source` is what makes `match.<row>` / `tint.<row>` answerable at
        // all, so the audit must use it — a bare `RowStyleExternal` has
        // `row_count = 0` and the domain check would skip.
        audit(
            "row_style",
            &RowStyleExternal::new(Rc::new(RowStyleState::new()))
                .with_source(3, |_row| vec!["a".to_string()]),
        );
        audit(
            "table",
            &TableExternal::new(
                vec!["a".into(), "b".into()],
                vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
            ),
        );
    }

    /// R1353.1 — the template's `<placeholders>` must match `args` in name and
    /// order **at every real declaration in the workspace**, not just the
    /// fixtures below.
    ///
    /// A source-text scan, for the same reason
    /// `widgets::commit`'s literal guard is one: no runtime assertion can see a
    /// declaration in a crate this one does not depend on, and a `const fn`
    /// cannot parse its own path string. R1353's first cut asserted this about
    /// four literals it wrote itself while `SchemaField::parametric`'s doc
    /// claimed the invariant was "enforced" — a green test named for a coverage
    /// it did not have, which is worse than no test. The invariant the whole
    /// model rests on (that `"cell.<row>.<col>"` really does take `row` then
    /// `col`) now gets checked where it is actually written.
    #[test]
    fn r1353_1_every_real_declaration_matches_its_template() {
        fn placeholders(t: &str) -> Vec<String> {
            let mut out = Vec::new();
            let mut rest = t;
            while let Some(o) = rest.find('<') {
                rest = &rest[o + 1..];
                let Some(c) = rest.find('>') else { break };
                out.push(rest[..c].to_string());
                rest = &rest[c + 1..];
            }
            out
        }
        // `parametric("<template>", "<ty>", const { &[SchemaArg::<k>("<name>", ..), ..] })`
        // — matched textually across the workspace, including crates and examples
        // that pinion-core cannot link against.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root is two levels above this crate");
        let mut checked = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if !matches!(name.as_str(), "target" | "vendor" | ".git") {
                        stack.push(p);
                    }
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let Ok(src) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for (i, _) in src.match_indices("SchemaField::parametric(") {
                    let tail = &src[i..];
                    // The declaration ends at its closing `)`, which the nested
                    // `const { &[..] }` makes the LAST one before the next
                    // declaration — bound the window at the next `SchemaField::`
                    // to stay inside this literal.
                    let end = tail[1..]
                        .find("SchemaField::")
                        .map_or(tail.len(), |n| n + 1);
                    let win = &tail[..end];
                    let Some(t0) = win.find('"') else { continue };
                    let Some(t1) = win[t0 + 1..].find('"') else {
                        continue;
                    };
                    let template = &win[t0 + 1..t0 + 1 + t1];
                    let names: Vec<String> = win
                        .match_indices("SchemaArg::")
                        .filter_map(|(j, _)| {
                            let a = &win[j..];
                            let s = a.find('"')?;
                            let e = a[s + 1..].find('"')?;
                            Some(a[s + 1..s + 1 + e].to_string())
                        })
                        .collect();
                    checked += 1;
                    if placeholders(template) != names {
                        offenders.push(format!(
                            "{}: template {template:?} declares {:?} but its args are {names:?}",
                            p.file_name().unwrap_or_default().to_string_lossy(),
                            placeholders(template),
                        ));
                    }
                }
            }
        }
        assert!(
            checked > 100,
            "the scan must actually reach the workspace's declarations; saw {checked}",
        );
        assert!(
            offenders.is_empty(),
            "{} declaration(s) whose template and args disagree:\n{}",
            offenders.len(),
            offenders.join("\n"),
        );
    }

    /// R1353 — the template's `<placeholders>` must match `args` in name and
    /// order. Fixture-level companion to
    /// `r1353_1_every_real_declaration_matches_its_template`: this one pins the
    /// SHAPES the rule must handle (scalar, one arg, two args, an infix arg),
    /// which a scan over today's declarations cannot guarantee stay represented.
    #[test]
    fn r1353_every_declared_template_matches_its_args() {
        fn placeholders(template: &str) -> Vec<&str> {
            let mut out = Vec::new();
            let mut rest = template;
            while let Some(open) = rest.find('<') {
                rest = &rest[open + 1..];
                let close = rest.find('>').expect("an unclosed <placeholder>");
                out.push(&rest[..close]);
                rest = &rest[close + 1..];
            }
            out
        }
        let check = |f: &SchemaField| {
            let names: Vec<&str> = f.args.iter().map(|a| a.name).collect();
            assert_eq!(
                placeholders(f.path),
                names,
                "template {:?} and its declared args disagree",
                f.path,
            );
        };
        check(&SchemaField::new("total", "int"));
        check(&SchemaField::parametric(
            "width.<col>",
            "int",
            const { &[SchemaArg::index("col", "cols")] },
        ));
        check(&SchemaField::parametric(
            "cell.<row>.<col>",
            "string",
            const {
                &[
                    SchemaArg::index("row", "rows"),
                    SchemaArg::index("col", "cols"),
                ]
            },
        ));
        // The infix shape: a literal AFTER the argument. This is why `path` holds
        // the template rather than a stem plus trailing args.
        check(&SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        ));
    }

    /// R1353.1 — `literal_prefix` returns exactly what a `query` impl strips,
    /// for every template shape and **every separator**.
    ///
    /// The separator is the author's choice, not this type's: almost everything
    /// uses `.`, `hello-input-chip` uses `:`. R1353's first cut trimmed `'.'`
    /// specifically, so the same accessor answered `"width"` for one template and
    /// `"state:"` for the other — and claimed, in its doc, to return the string
    /// `strip_prefix` matches, which was then true only where it had not
    /// trimmed. Verbatim is the only answer that is true for both.
    #[test]
    fn r1353_1_literal_prefix_is_what_a_query_impl_strips() {
        // A scalar is its own prefix.
        assert_eq!(SchemaField::new("total", "int").literal_prefix(), "total");
        // `column_widths::query` does `strip_prefix("width.")`.
        assert_eq!(
            SchemaField::parametric(
                "width.<col>",
                "int",
                const { &[SchemaArg::open("col", "int")] }
            )
            .literal_prefix(),
            "width.",
        );
        // `hello-input-chip::query` does `strip_prefix("state:")` — a different
        // separator, and the accessor must not assume one.
        assert_eq!(
            SchemaField::parametric(
                "state:<id>",
                "string",
                const { &[SchemaArg::open("id", "int")] }
            )
            .literal_prefix(),
            "state:",
        );
        // An infix template's prefix stops at its FIRST argument.
        assert_eq!(
            SchemaField::parametric(
                "voice.<id>.gain",
                "float",
                const { &[SchemaArg::open("id", "int")] }
            )
            .literal_prefix(),
            "voice.",
        );
    }

    /// R1353 — an INFIX argument (a literal after the placeholder) matches only
    /// when the trailing literal is present. `voice.3` is not `voice.3.gain`.
    #[test]
    fn r1353_addresses_handles_an_infix_argument() {
        let f = SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        );
        assert!(f.addresses("voice.3.gain"));
        assert!(
            f.addresses("voice.abc.gain"),
            "well-formedness is query's job"
        );
        assert!(!f.addresses("voice.3"), "the trailing literal is required");
        assert!(
            f.addresses("voice..gain"),
            "R1667 — an EMPTY argument is a member with an empty argument; \
             emptiness is well-formedness, and well-formedness is query's job",
        );
        assert!(!f.addresses("voice.3.pan"), "a DIFFERENT field's template");
    }

    /// R1353 — the separator has no escape, pinned as MEASURED behaviour rather
    /// than left as a sentence in a doc.
    ///
    /// An argument is delimited by `.` and nothing escapes a `.` inside one, so
    /// a key containing the separator is not addressable. These cases are the
    /// boundary of what the convention can express; if someone later adds an
    /// escaping rule, this test is the list of answers that must change (and
    /// `ExternalIntrospect::query`'s "does NOT settle" section is the prose to
    /// update with it).
    #[test]
    fn r1353_an_argument_cannot_contain_the_separator() {
        let infix = SchemaField::parametric(
            "voice.<id>.gain",
            "float",
            const { &[SchemaArg::key("id", "int", "voices")] },
        );
        // A dot-free id is fine — including one that spells the trailing literal.
        assert!(infix.addresses("voice.3.gain"));
        assert!(infix.addresses("voice.gain.gain"), "id = \"gain\"");
        // …but the id "x.gain" is UNREACHABLE: matching binds the argument at the
        // FIRST trailing literal, so this addresses nothing rather than silently
        // binding a different id than the caller meant.
        assert!(
            !infix.addresses("voice.x.gain.gain"),
            "an id containing the separator is not addressable",
        );

        // A template's FINAL argument has no trailing literal, so it takes the
        // rest — `col` binds to "2.3". The field owns the path (that is what
        // `addresses` answers); `query` then rejects it as malformed. Owned and
        // malformed is deliberately distinct from unknown: the agent asked this
        // field a bad question, it did not ask a question of nothing.
        let two = SchemaField::parametric(
            "cell.<row>.<col>",
            "string",
            const {
                &[
                    SchemaArg::index("row", "rows"),
                    SchemaArg::index("col", "cols"),
                ]
            },
        );
        assert!(two.addresses("cell.1.2"));
        assert!(
            two.addresses("cell.1.2.3"),
            "the final argument takes the rest"
        );
        assert!(
            !two.addresses("cell.1"),
            "a MISSING argument is not a member — the template's trailing \
             literal never matched, so this is a different address, not a \
             malformed one",
        );
        // R1667 — an argument that is present and empty is the field's, and the
        // parse stays unambiguous either way: greedy-to-the-next-literal gives
        // each of these exactly one reading.
        assert!(two.addresses("cell.1."), "col = \"\"");
        assert!(two.addresses("cell..2"), "row = \"\"");
        assert!(two.addresses("cell.."), "both empty");
    }

    /// R1353 — `addresses` is the membership question, so a parametric family is
    /// addressed by its MEMBERS. Pinned because `read_only_or_unknown` routes
    /// through it: matching the stem exactly would tell an agent that `width.0`
    /// — a path it can plainly read — does not exist.
    #[test]
    fn r1353_addresses_matches_members_not_the_bare_stem() {
        let scalar = SchemaField::new("total", "int");
        assert!(scalar.addresses("total"));
        assert!(!scalar.addresses("total.0"), "a scalar has no members");

        let param = SchemaField::parametric(
            "width.<col>",
            "int",
            const { &[SchemaArg::index("col", "cols")] },
        );
        assert!(param.addresses("width.0"));
        assert!(param.addresses("width.12"));
        assert!(
            !param.addresses("width"),
            "the bare stem addresses nothing — it is not a readable path",
        );
        assert!(
            param.addresses("width."),
            "R1667 — the declared family owns its empty member; whether that \
             member has a value is `query`'s answer, not the matcher's",
        );
        assert!(
            !param.addresses("widths"),
            "a longer path that merely shares the prefix is a DIFFERENT field \
             (column_widths declares both `width` and `widths`)",
        );
    }

    #[test]
    fn r1353_read_only_or_unknown_sees_parametric_members() {
        let schema = IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("total", "int"),
                    SchemaField::parametric(
                        "width.<col>",
                        "int",
                        const { &[SchemaArg::index("col", "cols")] },
                    ),
                ]
            },
        );
        assert_eq!(
            read_only_or_unknown(&schema, "width.0"),
            InterveneError::ReadOnly,
            "a real, readable member is read-only — not 'no such path'",
        );
        assert_eq!(
            read_only_or_unknown(&schema, "total"),
            InterveneError::ReadOnly,
        );
        assert_eq!(
            read_only_or_unknown(&schema, "nope"),
            InterveneError::UnknownPath,
        );
    }

    #[test]
    fn stub_declares_gui_only_with_skip_fallback() {
        let stub = StubExternal::new();
        let support = stub.backends();
        assert!(support.supports(Backend::Gui));
        assert!(!support.supports(Backend::Tui));
        assert!(!support.supports(Backend::Rpc));
        assert_eq!(support.fallback, BackendFallback::Skip);
    }

    #[test]
    fn stub_uses_framework_repaint_and_ui_thread() {
        let stub = StubExternal::new();
        assert_eq!(stub.repaint_ownership(), RepaintOwner::Framework);
        assert_eq!(stub.thread_ownership(), ThreadOwnership::UiThreadSync);
    }

    #[test]
    fn stub_does_not_claim_any_event() {
        let stub = StubExternal::new();
        let event = Event::Window(WindowEvent::Close);
        assert!(!stub.handles_event(&event));
    }

    #[test]
    fn stub_lifecycle_and_dpi_callbacks_are_noop() {
        let mut stub = StubExternal::new();
        stub.on_mount();
        stub.on_visibility_change(true);
        stub.on_focus_change(false);
        stub.on_dpi_change(2.0);
        stub.on_resize(800, 600);
        stub.on_unmount();
    }

    #[test]
    fn stub_poll_state_is_none() {
        let mut stub = StubExternal::new();
        assert!(stub.poll_state().is_none());
    }

    #[test]
    fn drag_at_methods_default_delegate_to_cursorless() {
        use std::cell::Cell;
        // R1093 — a drag source that overrides ONLY the pre-R1093
        // cursor-less hooks. The additive `drag_to_at`/`drag_release_at`
        // defaults must route into them, so every existing source stays
        // bit-identical (the cursor is simply dropped) and no source ever
        // receives both the `_at` call AND the cursor-less one.
        #[derive(Debug)]
        struct RecordingSource {
            to_calls: Cell<u32>,
            release_calls: Cell<u32>,
        }
        impl External for RecordingSource {
            fn backends(&self) -> BackendSupport {
                BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
            }
            fn repaint_ownership(&self) -> RepaintOwner {
                RepaintOwner::Framework
            }
            fn thread_ownership(&self) -> ThreadOwnership {
                ThreadOwnership::UiThreadSync
            }
            fn drag_to(&mut self, _p: &DragPayload, _o: Option<DropPoint>) {
                self.to_calls.set(self.to_calls.get() + 1);
            }
            fn drag_release(&mut self, _p: &DragPayload, _o: Option<DropPoint>) {
                self.release_calls.set(self.release_calls.get() + 1);
            }
        }
        let mut src = RecordingSource {
            to_calls: Cell::new(0),
            release_calls: Cell::new(0),
        };
        let payload = DragPayload {
            kind: Cow::Borrowed("dock-panel"),
            value: IntrospectValue::Text("inspector".to_owned()),
        };
        let to_update = DragUpdate {
            over: None,
            cursor: (12.0, 34.0),
            over_window: None,
            source_window: None,
            became_drag: false,
            press_cursor: (12.0, 34.0),
        };
        let release_update = DragUpdate {
            over: None,
            cursor: (56.0, 78.0),
            over_window: None,
            source_window: None,
            became_drag: false,
            press_cursor: (56.0, 78.0),
        };
        src.drag_to_at(&payload, &to_update);
        src.drag_release_at(&payload, &release_update);
        assert_eq!(
            src.to_calls.get(),
            1,
            "drag_to_at default must delegate to drag_to"
        );
        assert_eq!(
            src.release_calls.get(),
            1,
            "drag_release_at default must delegate to drag_release"
        );
    }

    #[test]
    fn stub_does_not_want_pointer_capture() {
        let stub = StubExternal::new();
        assert!(!stub.wants_pointer_capture());
    }

    #[test]
    fn stub_pointer_move_default_is_noop() {
        let mut stub = StubExternal::new();
        // Default impl drops both coords — exercising it is the
        // assertion that the trait signature remains dyn-safe and
        // the no-op body compiles for the StubExternal baseline.
        stub.pointer_move(0.5, 0.5);
        stub.pointer_move(-0.1, 1.3);
    }

    #[test]
    fn stub_is_not_a_drag_source() {
        // R742 §5.51 — the default `begin_drag` returns `None`, so the
        // router never starts a session for a non-DnD widget. `drag_to`
        // / `drag_release` are no-op defaults exercised here so the
        // additive trait surface compiles for the StubExternal baseline
        // and stays dyn-safe.
        let mut stub = StubExternal::new();
        assert!(stub.begin_drag().is_none());
        let payload = DragPayload {
            kind: Cow::Borrowed("dnd-row"),
            value: IntrospectValue::Int(0),
        };
        let over = DropPoint {
            tag: "dnd#1".to_string(),
            x_rel: 0.5,
            y_rel: 0.25,
        };
        stub.drag_to(&payload, Some(over.clone()));
        stub.drag_to(&payload, None);
        stub.drag_release(&payload, Some(over));
        stub.drag_release(&payload, None);
    }

    #[test]
    fn trait_is_dyn_safe() {
        // Compile-time guard: any future change that loses dyn-safety
        // (associated consts, Self-returning methods, etc.) breaks this.
        let _: Box<dyn External> = Box::new(StubExternal::new());
    }

    #[test]
    fn stub_opts_out_of_introspection() {
        let stub = StubExternal::new();
        assert!(stub.introspect().is_none());
        let mut stub_mut = StubExternal::new();
        assert!(stub_mut.introspect_mut().is_none());
    }

    #[test]
    fn counted_opts_in_to_introspection() {
        let counted = CountedExternal::new(7);
        let introspect = counted.introspect().expect("opt-in declared");
        assert_eq!(introspect.query("count"), Ok(IntrospectValue::Int(7)),);
        assert!(introspect.query("missing").is_err());
    }

    /// R1638 — an ACTION's path is exact however many arguments it declares.
    ///
    /// `addresses` used to read "has arguments" as "is a template", which was
    /// true while only a parametric read could have any. An action carries its
    /// arguments on `scene/invoke`, so a name that merely LOOKS like a template
    /// is still one exact path — and getting this wrong would make
    /// `scene/invoke cell.3` resolve to a field the surface never dispatches.
    #[test]
    fn r1638_an_actions_path_is_exact_however_many_arguments_it_takes() {
        const LOOKS_PARAMETRIC: SchemaField = SchemaField::action_with(
            "cell.<row>",
            "null",
            ArgForm::Object,
            const { &[SchemaArg::open("row", "int")] },
        );
        assert!(LOOKS_PARAMETRIC.addresses("cell.<row>"), "spelled exactly");
        assert!(
            !LOOKS_PARAMETRIC.addresses("cell.3"),
            "an action is not addressed by a member of a family it does not have",
        );
        // The read channel's peer, unchanged: a template IS addressed by its
        // members and not by itself.
        let real: SchemaField = SchemaField::parametric(
            "cell.<row>",
            "int",
            const { &[SchemaArg::open("row", "int")] },
        );
        assert!(real.addresses("cell.3"));
        assert!(real.addresses("cell."), "R1667 — its empty member, too");
    }

    #[test]
    fn counted_schema_lists_count_field() {
        let counted = CountedExternal::new(0);
        let schema = counted.schema();
        assert_eq!(
            schema.fields,
            &[
                SchemaField::new("count", "int"),
                SchemaField::action("increment", "int"),
            ],
        );
    }

    #[test]
    fn intervene_updates_value() {
        let mut counted = CountedExternal::new(0);
        let introspect = counted.introspect_mut().expect("opt-in declared");
        introspect
            .intervene("count", IntrospectValue::Int(42))
            .expect("matching type");
        assert_eq!(counted.count, 42);
    }

    #[test]
    fn intervene_rejects_type_mismatch() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Bool(true))
            .unwrap_err();
        assert_eq!(err, InterveneError::TypeMismatch);
    }

    #[test]
    fn intervene_rejects_unknown_path() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .introspect_mut()
            .unwrap()
            .intervene("ghost", IntrospectValue::Int(1))
            .unwrap_err();
        assert_eq!(err, InterveneError::UnknownPath);
    }

    #[test]
    fn introspect_sub_trait_is_dyn_safe() {
        let counted = CountedExternal::new(0);
        let _: &dyn ExternalIntrospect = &counted;
    }

    #[test]
    fn counted_invoke_increment_returns_new_total() {
        let mut counted = CountedExternal::new(10);
        let out = counted
            .invoke("increment", IntrospectValue::Int(5))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(15));
        assert_eq!(counted.count, 15);
    }

    #[test]
    fn counted_invoke_increment_with_wrong_type_is_type_mismatch() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .invoke("increment", IntrospectValue::Text("nope".to_string()))
            .unwrap_err();
        assert_eq!(err, InvokeError::TypeMismatch);
    }

    #[test]
    fn counted_invoke_unknown_path_is_unknown_path() {
        let mut counted = CountedExternal::new(0);
        let err = counted
            .invoke("ghost", IntrospectValue::Int(1))
            .unwrap_err();
        assert_eq!(err, InvokeError::UnknownPath);
    }

    #[test]
    fn stub_is_dirty_default_is_false() {
        // §5.20 default contract: an External that doesn't opt in to
        // the intent channel reports clean — `walk_scene_and_drain`
        // can skip the drain virtual call.
        let stub = StubExternal::new();
        assert!(!stub.is_dirty());
    }

    #[test]
    fn stub_drain_intents_default_is_noop() {
        // Default `drain_intents` must not emit even when the runtime
        // calls it anyway. Guards against accidental drain-through-
        // unrelated-state-changes.
        let mut stub = StubExternal::new();
        let mut harvested = Vec::new();
        stub.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn counted_intervene_marks_dirty_and_drains_intent() {
        let mut counted = CountedExternal::new(0);
        assert!(!counted.is_dirty());
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(7))
            .unwrap();
        assert!(counted.is_dirty());
        let mut harvested: Vec<Intent> = Vec::new();
        counted.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "counted.changed");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(7));
        assert!(!counted.is_dirty());
    }

    #[test]
    fn counted_multiple_intervenes_accumulate_intents() {
        // Each successful intervene pushes one intent; drain flushes
        // them in insertion order so subscribers observe the same
        // sequence the state actually traversed.
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(1))
            .unwrap();
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(2))
            .unwrap();
        let mut harvested: Vec<Intent> = Vec::new();
        counted.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 2);
        assert_eq!(harvested[0].payload, IntrospectValue::Int(1));
        assert_eq!(harvested[1].payload, IntrospectValue::Int(2));
    }

    #[test]
    fn counted_failed_intervene_does_not_mark_dirty() {
        let mut counted = CountedExternal::new(0);
        let _ = counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Bool(true));
        assert!(!counted.is_dirty());
    }

    #[test]
    fn stub_invoke_default_is_unknown_path() {
        // `StubExternal` does not opt in to invoke beyond the default
        // impl — assertion guards against accidental future override
        // that would change the contract.
        //
        // StubExternal doesn't implement ExternalIntrospect; reach the
        // default via the trait-bound dispatch path by constructing an
        // ad-hoc impl-of-the-trait. Item definitions are hoisted before
        // any `let` to keep clippy::items_after_statements clean.
        struct NullIntrospect;
        impl ExternalIntrospect for NullIntrospect {
            fn schema(&self) -> IntrospectSchema {
                IntrospectSchema::new(const { &[] })
            }
            fn query(&self, _: &str) -> Result<IntrospectValue, ReadRefusal> {
                Err(ReadRefusal::UnknownPath)
            }
            fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
                Err(InterveneError::UnknownPath)
            }
            // invoke uses default impl
        }
        let mut stub = StubExternal::new();
        let mut null = NullIntrospect;
        let err = null.invoke("anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::UnknownPath);
        // Silence the unused stub binding.
        let _ = &mut stub;
    }

    // ───────────────────────────────────────────────────────────────
    // R51.155 §5.15 — IntrospectValue typed accessors.
    // ───────────────────────────────────────────────────────────────

    #[test]
    fn as_bool_extracts_only_bool_variant() {
        assert_eq!(IntrospectValue::Bool(true).as_bool(), Some(true));
        assert_eq!(IntrospectValue::Bool(false).as_bool(), Some(false));
        assert_eq!(IntrospectValue::Null.as_bool(), None);
        assert_eq!(IntrospectValue::Int(1).as_bool(), None);
        assert_eq!(IntrospectValue::Float(1.0).as_bool(), None);
        assert_eq!(IntrospectValue::Text("true".into()).as_bool(), None);
    }

    #[test]
    fn as_i64_extracts_only_int_variant() {
        assert_eq!(IntrospectValue::Int(42).as_i64(), Some(42));
        assert_eq!(IntrospectValue::Int(-1).as_i64(), Some(-1));
        assert_eq!(IntrospectValue::Float(1.0).as_i64(), None);
        assert_eq!(IntrospectValue::Null.as_i64(), None);
        assert_eq!(IntrospectValue::Bool(true).as_i64(), None);
    }

    #[test]
    fn as_i32_narrows_in_range_int() {
        assert_eq!(IntrospectValue::Int(42).as_i32(), Some(42));
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MAX)).as_i32(),
            Some(i32::MAX),
        );
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MIN)).as_i32(),
            Some(i32::MIN),
        );
    }

    #[test]
    fn as_i32_rejects_out_of_range_int() {
        assert_eq!(
            IntrospectValue::Int(i64::from(i32::MAX) + 1).as_i32(),
            None,
            "narrowing failure surfaces as None, not silent truncation",
        );
        assert_eq!(IntrospectValue::Int(i64::from(i32::MIN) - 1).as_i32(), None,);
    }

    #[test]
    fn as_usize_rejects_negative() {
        assert_eq!(IntrospectValue::Int(0).as_usize(), Some(0));
        assert_eq!(IntrospectValue::Int(42).as_usize(), Some(42));
        assert_eq!(
            IntrospectValue::Int(-1).as_usize(),
            None,
            "negative ints can't be usize",
        );
    }

    #[test]
    fn as_f64_extracts_only_float_variant() {
        assert_eq!(IntrospectValue::Float(1.5).as_f64(), Some(1.5));
        assert_eq!(IntrospectValue::Float(0.0).as_f64(), Some(0.0));
        assert_eq!(IntrospectValue::Int(1).as_f64(), None);
        assert_eq!(IntrospectValue::Null.as_f64(), None);
    }

    #[test]
    fn as_f32_narrows_with_documented_truncation() {
        assert_eq!(
            IntrospectValue::Float(0.5).as_f32().map(f32::to_bits),
            Some(0.5_f32.to_bits()),
        );
        // f64 → f32 truncation: pi in f64 vs f32.
        let pi = IntrospectValue::Float(std::f64::consts::PI).as_f32();
        assert!(pi.is_some());
        // Round-trip precision lost — but the truncation is documented.
        let pi_f32 = pi.unwrap();
        let diff = (pi_f32 - std::f32::consts::PI).abs();
        assert!(diff < 1e-6, "as_f32 truncation lands close to f32 const");
    }

    #[test]
    fn as_f32_returns_none_for_non_float_variants() {
        assert_eq!(IntrospectValue::Int(1).as_f32(), None);
        assert_eq!(IntrospectValue::Null.as_f32(), None);
        assert_eq!(IntrospectValue::Bool(true).as_f32(), None);
    }

    #[test]
    fn as_str_extracts_only_text_variant() {
        assert_eq!(
            IntrospectValue::Text("hello".to_string()).as_str(),
            Some("hello"),
        );
        assert_eq!(IntrospectValue::Null.as_str(), None);
        assert_eq!(IntrospectValue::Int(1).as_str(), None);
    }

    #[test]
    fn is_null_distinguishes_null_only() {
        assert!(IntrospectValue::Null.is_null());
        assert!(!IntrospectValue::Bool(false).is_null());
        assert!(!IntrospectValue::Int(0).is_null());
        assert!(!IntrospectValue::Float(0.0).is_null());
        assert!(!IntrospectValue::Text(String::new()).is_null());
    }
}

#[cfg(test)]
mod selection_copy_payload_tests {
    //! R1407 §5.35 §5.22 — the lifted `Ctrl`/`Cmd`+C copy chord + query. Pure:
    //! it returns the string a copy would write (or `None`) and never touches a
    //! clipboard, so every path — including `AltGr` safety — is asserted without
    //! racing the real OS clipboard.
    use super::{
        ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, ReadRefusal,
        selection_copy_payload,
    };
    use crate::input::Modifiers;

    /// A minimal introspectable whose `"sel"` field returns a fixed payload (or
    /// `None` when the range is empty), a non-`Text` `"count"`, and no other
    /// path — the two shapes the copy must accept and reject.
    struct FakeSelection(Option<&'static str>);

    impl ExternalIntrospect for FakeSelection {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
            match path {
                "sel" => self
                    .0
                    .map(|s| IntrospectValue::Text(s.to_owned()))
                    .ok_or(ReadRefusal::UnknownPath),
                "count" => Ok(IntrospectValue::Int(3)),
                _ => Err(ReadRefusal::UnknownPath),
            }
        }
        fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn ctrl_c_yields_the_queried_payload() {
        let ext = FakeSelection(Some("50494e01"));
        assert_eq!(
            selection_copy_payload(&ext, "c", ctrl(), "sel"),
            Some("50494e01".to_owned()),
            "the chord returns the field's serialization",
        );
    }

    #[test]
    fn cmd_shift_c_still_matches_the_uppercase_key() {
        // The platform delivers "C" when Shift is held, and Cmd (meta) is the
        // macOS command key; the case-insensitive match must still fire.
        let ext = FakeSelection(Some("ab"));
        let mods = Modifiers {
            meta: true,
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            selection_copy_payload(&ext, "C", mods, "sel"),
            Some("ab".to_owned()),
        );
    }

    #[test]
    fn a_non_chord_key_yields_nothing() {
        let ext = FakeSelection(Some("x"));
        // A bare "c" (no modifier) is a literal, not a copy.
        assert_eq!(
            selection_copy_payload(&ext, "c", Modifiers::default(), "sel"),
            None,
        );
        // Ctrl held but the wrong letter.
        assert_eq!(selection_copy_payload(&ext, "v", ctrl(), "sel"), None);
    }

    #[test]
    fn altgr_c_does_not_misfire_the_copy() {
        // AltGr = Ctrl+Alt: command_key() is true but the key is composing a
        // character, so the copy must NOT fire and swallow it (R1223).
        let ext = FakeSelection(Some("x"));
        let altgr = Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(selection_copy_payload(&ext, "c", altgr, "sel"), None);
    }

    #[test]
    fn the_chord_with_nothing_selected_yields_nothing() {
        // A copy chord whose field is empty returns None, so the caller leaves
        // the key unhandled and it can bubble to an application shortcut.
        let ext = FakeSelection(None);
        assert_eq!(selection_copy_payload(&ext, "c", ctrl(), "sel"), None);
    }

    #[test]
    fn a_non_text_payload_yields_nothing() {
        // A field resolving to a non-Text value is skipped (a copy needs a
        // string) and the key falls through.
        let ext = FakeSelection(Some("x"));
        assert_eq!(selection_copy_payload(&ext, "c", ctrl(), "count"), None);
    }
}

// R1480 §5.15 — `RawJson`: the bytes a producer already has.
#[cfg(test)]
mod raw_json_tests {
    use super::{IntrospectValue, RawJson};

    /// Declared z → m → a; sorted order is a → m → z. The gap between the
    /// two is what every test below reads.
    #[derive(serde::Serialize)]
    struct Doc {
        z: u8,
        m: &'static str,
        a: u8,
    }

    const DOC: Doc = Doc {
        z: 1,
        m: "mid",
        a: 9,
    };

    #[test]
    fn r1480_encode_keeps_the_producers_own_encoding() {
        assert_eq!(
            RawJson::encode(&DOC)
                .expect("a derived Serialize cannot fail")
                .get(),
            r#"{"z":1,"m":"mid","a":9}"#,
            "the text must be what the producer wrote, not a re-rendering",
        );
    }

    #[test]
    fn r1480_a_dom_round_trip_would_have_reordered_it() {
        // The premise the wire tests rely on, asserted rather than assumed:
        // going through a `Value` really does change the text. Without this,
        // a serde_json built with `preserve_order` would silently turn every
        // key-order witness in this round into a vacuous pass.
        let via_dom = serde_json::to_string(&serde_json::to_value(DOC).expect("to_value"))
            .expect("to_string");
        assert_eq!(via_dom, r#"{"a":9,"m":"mid","z":1}"#);
        assert_ne!(
            via_dom,
            RawJson::encode(&DOC).expect("encode").get(),
            "if these ever agree the DOM/raw witness has lost its teeth",
        );
    }

    #[test]
    fn r1480_parse_rejects_text_that_is_not_json() {
        assert!(RawJson::parse("{\"unterminated\":".to_owned()).is_err());
        assert!(RawJson::parse("not json at all".to_owned()).is_err());
        // Two values are not one value: a frame spliced from this would be
        // malformed downstream of the splice, where nothing could report it.
        assert!(RawJson::parse("{} {}".to_owned()).is_err());
    }

    #[test]
    fn r1480_parse_keeps_the_text_it_accepted() {
        let raw = RawJson::parse(r#"{"b":2,  "a":1}"#.to_owned()).expect("valid JSON");
        assert_eq!(raw.get(), r#"{"b":2,  "a":1}"#);
    }

    #[test]
    fn r1480_equality_is_textual_because_the_text_is_the_payload() {
        let one = RawJson::parse(r#"{"a":1,"b":2}"#.to_owned()).expect("valid");
        let other = RawJson::parse(r#"{"b":2,"a":1}"#.to_owned()).expect("valid");
        assert_ne!(one, other, "different encodings are different RawJsons");
        assert_eq!(
            one.to_value().expect("value"),
            other.to_value().expect("value"),
            "…of the same JSON value — which is why the wire may pick either",
        );
    }

    #[test]
    fn r1480_to_value_materializes_the_tree_the_type_exists_to_avoid() {
        assert_eq!(
            RawJson::encode(&DOC)
                .expect("encode")
                .to_value()
                .expect("valid JSON parses"),
            serde_json::json!({"z":1,"m":"mid","a":9}),
        );
    }

    #[test]
    fn r1480_introspect_raw_mirrors_introspect_json() {
        // Same call shape, same degradation policy, different channel.
        let raw = IntrospectValue::raw(&DOC);
        assert_eq!(
            raw.as_raw().expect("Raw variant").get(),
            r#"{"z":1,"m":"mid","a":9}"#,
        );

        // A map with non-string keys is the input both encoders reject.
        // Neither fabricates an answer; both land on a null-valued payload
        // the envelope renders identically. They differ in variant, and
        // that difference is useful: `as_raw` reports the failure, which
        // `json`'s `Json(Null)` cannot.
        let unencodable = std::collections::BTreeMap::from([(vec![1_u8, 2], "x")]);
        assert_eq!(
            IntrospectValue::json(&unencodable),
            IntrospectValue::Json(serde_json::Value::Null),
        );
        let degraded = IntrospectValue::raw(&unencodable);
        assert_eq!(degraded, IntrospectValue::Null);
        assert!(
            degraded.as_raw().is_none(),
            "a failed encode must not read back as an encoded answer",
        );
    }

    #[test]
    fn r1480_as_raw_does_not_coerce_across_variants() {
        assert!(
            IntrospectValue::json(&DOC).as_raw().is_none(),
            "a Json holding the same document is still not a Raw",
        );
        assert!(IntrospectValue::Null.as_raw().is_none());
    }
}

// R1642 §5.12 §2 #2 — a conditional argument: the discriminant's case table.
#[cfg(test)]
mod conditional_argument_tests {
    use super::{ArgCase, ArgDomain, ArgForm, ConditionalDefect, SchemaArg, SchemaField};

    /// The legal shape every fixture below is one step away from: a discriminant
    /// whose first case adds an argument and whose second adds none.
    const LEGAL_CASES: &[ArgCase] = &[
        ArgCase::new("one", &[SchemaArg::open("extra", "int")]),
        ArgCase::new("two", &[]),
    ];
    /// A case adding an argument the field already declares.
    const SHADOWING: &[ArgCase] = &[ArgCase::new("one", &[SchemaArg::open("common", "int")])];
    /// A case adding an argument that carries a case table of its own.
    const NESTED: &[ArgCase] = &[ArgCase::new(
        "one",
        &[SchemaArg::one_of_with("deep", "string", LEGAL_CASES)],
    )];

    /// A delimited action taking `args`, so each fixture differs from the
    /// control in exactly one respect and a refusal is attributable.
    fn verb(args: &'static [SchemaArg]) -> SchemaField {
        SchemaField::action_with("verb", "string", ArgForm::Delimited(':'), args)
    }

    /// R1642 — the shape the refusals below are each one step away from.
    ///
    /// A control, and a load-bearing one: every test in this group asserts a
    /// refusal, and a checker that refused everything would pass all of them.
    #[test]
    fn r1642_a_well_formed_conditional_declaration_is_accepted() {
        let legal = verb(
            const {
                &[
                    SchemaArg::one_of_with("kind", "string", LEGAL_CASES),
                    SchemaArg::open("common", "int"),
                ]
            },
        );
        assert_eq!(legal.conditional_defect(), None, "the control is legal");
        assert!(legal.declares_cases(), "and it is an inhabitant");
    }

    /// R1642 — a field with no case table is not this rule's business.
    ///
    /// The arm that makes "no defect" over a catalog an honest answer rather
    /// than a vacuous one: most declarations have no discriminant at all, so a
    /// caller must be able to tell "checked and clean" from "nothing to check",
    /// which is what `declares_cases` is for.
    #[test]
    fn r1642_a_declaration_without_cases_is_not_an_inhabitant() {
        let plain = SchemaField::action_with(
            "verb",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::one_of("kind", "string", &["one", "two"])] },
        );
        assert_eq!(plain.conditional_defect(), None);
        assert!(!plain.declares_cases());
    }

    /// R1642 — two case tables leave the append order undefined.
    #[test]
    fn r1642_two_discriminants_are_refused() {
        let two = verb(
            const {
                &[
                    SchemaArg::one_of_with("kind", "string", LEGAL_CASES),
                    SchemaArg::one_of_with("also", "string", LEGAL_CASES),
                ]
            },
        );
        assert_eq!(
            two.conditional_defect(),
            Some(ConditionalDefect::TwoDiscriminants {
                first: "kind",
                second: "also",
            }),
        );
    }

    /// R1642 — an absent value selects no case, so the arguments that follow it
    /// would be unaccountable.
    #[test]
    fn r1642_an_optional_discriminant_is_refused() {
        let optional =
            verb(const { &[SchemaArg::one_of_with("kind", "string", LEGAL_CASES).optional()] });
        assert_eq!(
            optional.conditional_defect(),
            Some(ConditionalDefect::OptionalDiscriminant("kind")),
        );
    }

    /// R1642 — a case may not add a name the field already uses: a client keying
    /// arguments by name could not tell the two apart.
    #[test]
    fn r1642_a_shadowed_argument_name_is_refused() {
        let shadow = verb(
            const {
                &[
                    SchemaArg::one_of_with("kind", "string", SHADOWING),
                    SchemaArg::open("common", "int"),
                ]
            },
        );
        assert_eq!(
            shadow.conditional_defect(),
            Some(ConditionalDefect::ShadowedName {
                case: "one",
                name: "common",
            }),
        );
    }

    /// R1642 — the optional-suffix rule holds of the EXPANSION, not only of the
    /// field's own arguments.
    ///
    /// The commonest real way in: the field's trailing argument is optional and a
    /// case adds a required one after it, so the delimited payload would have to
    /// leave a hole in the middle — which `"3::l"` already proved it cannot
    /// (R1638). The flat check on `args` alone passes this declaration, which is
    /// why the rule had to be restated over expansions when cases arrived.
    #[test]
    fn r1642_an_expansion_that_puts_an_optional_before_a_required_is_refused() {
        let hole = verb(
            const {
                &[
                    SchemaArg::one_of_with("kind", "string", LEGAL_CASES),
                    SchemaArg::open("common", "int").optional(),
                ]
            },
        );
        assert_eq!(
            hole.conditional_defect(),
            Some(ConditionalDefect::OptionalNotASuffix { case: "one" }),
        );
    }

    /// R1642 — a second level of cases is refused while nothing needs one.
    ///
    /// Not because the append rule breaks down, but because an unexercised wire
    /// shape is a claim no client has been held to. See
    /// [`ConditionalDefect::NestedDiscriminant`].
    #[test]
    fn r1642_a_nested_case_table_is_refused() {
        let nested = verb(const { &[SchemaArg::one_of_with("kind", "string", NESTED)] });
        assert_eq!(
            nested.conditional_defect(),
            Some(ConditionalDefect::NestedDiscriminant {
                case: "one",
                name: "deep",
            }),
        );
    }

    /// R1642 — the case table reaches the wire whole, arguments and all.
    ///
    /// The recursion is the point: a case's arguments are `SchemaArg`s, so
    /// rendering a domain means rendering an argument, which is why both wire
    /// forms live in this crate. A renderer that stopped at the case's name would
    /// publish the vocabulary and drop precisely the half this round adds.
    #[test]
    fn r1642_a_case_table_survives_the_wire_form() {
        const CASES: &[ArgCase] = &[
            ArgCase::new(
                "align",
                &[SchemaArg::one_of("edge", "string", &["start", "end"])],
            ),
            ArgCase::new("distribute", &[]),
        ];
        let wire = ArgDomain::OneOfWith(CASES).to_wire();
        assert_eq!(wire["kind"], "one_of_with");
        let cases = wire["cases"].as_array().expect("an array of cases");
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0]["value"], "align");
        let then = cases[0]["then"]
            .as_array()
            .expect("align adds one argument");
        assert_eq!(then.len(), 1);
        assert_eq!(then[0]["name"], "edge");
        assert_eq!(then[0]["domain"]["kind"], "one_of");
        assert_eq!(then[0]["domain"]["values"][1], "end");
        assert_eq!(
            cases[1]["then"].as_array().map(Vec::len),
            Some(0),
            "an empty `then` is a claim: choosing this adds nothing",
        );
        // And the closed vocabulary is still readable as one, so a client that
        // only wants the legal values does not have to understand cases.
        let values: Vec<&str> = ArgDomain::OneOfWith(CASES)
            .cases()
            .iter()
            .map(|c| c.value)
            .collect();
        assert_eq!(values, vec!["align", "distribute"]);
        assert!(
            ArgDomain::OneOf(&["a"]).cases().is_empty(),
            "and a plain closed set decides nothing about the rest of the call",
        );
    }
}
