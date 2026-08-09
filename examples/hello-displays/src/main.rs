// R838 §5.38 — example bindings tolerate looser doc-markdown lints than the
// substrate crates; the narrative carries many proper-noun identifiers.
#![allow(clippy::doc_markdown)]

//! `hello-displays` — R1576 §5.16 §5.41 — the **desk** the windows sit on.
//!
//! ## The problem this exists for
//!
//! Save a window layout on a two-monitor desk. Come back tomorrow with the
//! second monitor unplugged. Restore the layout. Where does the panel go?
//!
//! Every desktop toolkit gets this wrong the same way, and for the same reason:
//! a saved position is an **absolute** coordinate in a space the application
//! cannot describe. The usual save-geometry call writes exactly that into an
//! opaque byte blob, and the restore answers a bare `bool` with nowhere to say
//! that the window went somewhere else — which is why so many restored windows
//! open where no pixel is, unreachable without a keyboard shortcut most users
//! do not know.
//!
//! R1576's answer is that a place is stated **relative to a named display**.
//! [`WindowSpec::display`] plus a position is "40 logical pixels into that
//! monitor", which means the same thing after the desk is rearranged, and when
//! that monitor is gone the substitution onto the fallback is *named* rather
//! than silently performed.
//!
//! This binding is the forcing consumer. It declares two windows — a main one
//! and a `panel` — and its presets move the panel by writing its spec:
//!
//! | preset | display | what it proves |
//! |---|---|---|
//! | `primary` | whichever display the platform calls primary, read at apply time | the binding can NAME a display (`pinion_shell::use_displays`) |
//! | `external` | the literal id `external-4k`, which is not attached here | the **substitution**, reported by name |
//! | `absolute` | none — a bare desktop coordinate | the pre-R1576 reading, still exact |
//! | `far` | none, at `(9000, 40)` | a window declared where no pixel is |
//!
//! On a one-monitor machine — which is what CI has — `external` and `far` are
//! the interesting rows, and they are the two that are ordinarily impossible to
//! exercise without a monitor farm.
//!
//! ## R1610 — and where the panel sits in the stacking order
//!
//! A place has a second half a layout preset needs: not only *where* on the
//! desk, but *how far forward*. A monitoring tool's torn-off readout is
//! supposed to stay visible over the application being watched, and that is
//! [`WindowSpec::level`] — declared like the placement, changed at runtime like
//! the placement (`invoke("level", "always_on_top")`), and read back on
//! `scene/windows` like the placement.
//!
//! It carries the same honesty the placement does. `level` is what the binding
//! DECLARED; `level_outcome` is what the windowing system actually running did
//! with it, because on Wayland the answer is "nothing" — the core shell
//! protocol has no stacking request.
//!
//! ## R1617 — and which display each window is *actually* on
//!
//! Everything above is about the DECLARATION: where a preset asked the panel to
//! go, and what became of that request. A window also simply *is* somewhere —
//! a WM-placed window declares nothing at all, and a window the user drags has
//! moved without any declaration changing. That second question has two
//! answerers, and until this round only one of them was ever consulted.
//!
//! This framework derives a window's home from its live rectangle: the display
//! holding the largest share. The window system has its own opinion, and across
//! the four desktop backends underneath there are four rules for it — two
//! resolve by largest intersection, one caches an answer refreshed only when the
//! window's scale factor changes, and one reports the first compositor output
//! the surface entered, which is not geometric at all. One of them answers with
//! the *first enumerated* monitor for a window that is on no monitor, where this
//! framework answers "nowhere".
//!
//! So `scene/windows` now publishes `display_home` — `derived`, `platform`, and
//! the relation between them (`agreed` / `diverged` / `platform_silent` /
//! `derived_nowhere` / `nowhere`). The panel paints the same fact through
//! [`pinion_shell::use_window_home`], so the binding and the wire are one
//! derivation with two callers rather than two copies.
//!
//! ## What the usual desktop-toolkit shape cannot do
//!
//! 1. **A display has an address.** The conventional screen object has no id
//!    accessor and its name is platform text with no uniqueness guarantee, so a
//!    preset cannot name a monitor at all — the only handle is a pointer that
//!    dies when the screen is removed.
//! 2. **A rectangle can be resolved before it is used.** The conventional
//!    screen queries all take a *point*; `invoke("resolve", "x,y,w,h")` here
//!    answers with the home display, the pixels that are on a display, the
//!    pixels that are not, and where the window would have to move.
//! 3. **The substitution is in the answer.** `scene/windows` reports each
//!    window's `anchored` outcome — `on_declared` / `substituted` /
//!    `no_display` — with **both** ids when a display was swapped, where the
//!    conventional restore answers a bare `bool`.
//! 4. **The desk reaches the wire at all.** A screen list that lives in
//!    in-process C++ cannot be asked by anything outside the application.
//! 5. **The holes are stated.** `gap_free` says whether the arrangement fills
//!    its own bounding box — the fact that makes "inside the virtual desktop"
//!    and "on a display" different questions. Without it, code uses
//!    virtual-geometry containment as a visibility test and is wrong on every
//!    L-shaped desk.
//! 6. **A level is one value, and it is enumerable.** Encoded as two
//!    independent bits in a flags word, "on top and on bottom" is a value a
//!    program can build and each platform backend resolves differently, and
//!    "offer the user the window levels" is not a question the vocabulary can
//!    answer. `level_names` here answers it over the wire.
//! 7. **Changing it costs one call.** A flags-word encoding makes the change
//!    reparent and hide the window, and on one platform backend destroy and
//!    recreate it. Here the panel stays mapped.
//! 8. **A dropped declaration is reported.** A flags accessor returns what the
//!    program stored, so a stacking bit a platform backend never reads still
//!    reads back as set. `scene/windows` publishes `level_outcome` beside
//!    `level`.
//! 9. **Both answers to "which display is this window on" are readable.** The
//!    conventional shape has both too — a public accessor returning the platform
//!    plugin's stored answer, and a geometric resolver that is **private**, is
//!    consulted only when the application itself moves the window, and decides
//!    by CENTRE POINT rather than by largest share. Nothing there puts the two
//!    side by side, and an application cannot, because one of them is
//!    unreachable. `display_home` reports both and names the relation.
//! 10. **A silent platform is not agreement.** The backend accessor is an
//!     option, and a hidden window can get nothing from it. That is
//!     `platform_silent` here, distinct from `agreed` — folding them would
//!     manufacture a cross-check that never happened.
//!
//! ## The AI surface (§2 #7)
//!
//! Anything a human can conclude from this window, an agent can conclude from
//! the wire: how many displays there are, which is primary, whether the desk has
//! holes, where the panel is declared to be, which display that resolves to
//! today, and whether it was the one asked for.
//!
//! ## Verification
//!
//! `tools/demos/r1576_a_place_is_relative_to_a_display.py` drives all of it over
//! RPC, including the framework's own `scene/displays`.

use std::borrow::Cow;
use std::rc::Rc;
use std::sync::Arc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::display::{DisplayHome, DisplayId, DisplayRect, DisplayTopology};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::window_level::WindowLevel;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{
    DisplayHandle, SizeStrategy, WidgetView, WindowPlacement, WindowSpec, use_display_handle,
    use_window_home, vello_renderer_impl,
};

// pinion-forge codegen output: `pub struct HelloDisplaysRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloDisplaysRenderer, HelloDisplaysRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 460;
const PANEL_W: u32 = 320;
const PANEL_H: u32 = 220;

const VIEW_TAG: &str = "displays";
const PANEL_TAG: &str = "panel_view";
const THEME_TAG: &str = "app";
const STATE_KEY: &str = "hello-displays/state";

const MAIN_WINDOW: &str = "main";
const PANEL_WINDOW: &str = "panel";

const TITLE_FONT_PX: u32 = 17;
const BODY_FONT_PX: u32 = 12;
const ROW_H: u32 = 18;

/// The display id the `external` preset names. Deliberately not a plausible
/// platform name: it must be a display that is **not** attached on any machine
/// this runs on, because the substitution is the thing being demonstrated.
const ABSENT_DISPLAY: &str = "external-4k";

/// How far into its display the panel sits, in that display's logical pixels.
const PANEL_INSET: (i32, i32) = (48, 48);

/// The presets, in menu order. Each is a name and the placement it declares —
/// the whole of a layout preset, as data rather than as an opaque blob.
const PRESETS: &[(&str, PresetKind)] = &[
    ("primary", PresetKind::PrimaryDisplay),
    ("external", PresetKind::NamedDisplay(ABSENT_DISPLAY)),
    ("absolute", PresetKind::Absolute((120, 120))),
    ("far", PresetKind::Absolute((9000, 40))),
    ("unplaced", PresetKind::Unplaced),
];

/// What a preset declares.
///
/// [`PresetKind::PrimaryDisplay`] is resolved when the preset is applied rather
/// than when it is written, which is the point: a preset that captured today's
/// primary id would be as brittle as the absolute coordinate it replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresetKind {
    /// Whichever display the platform calls primary, at [`PANEL_INSET`].
    PrimaryDisplay,
    /// A display named by id, at [`PANEL_INSET`] — attached or not.
    NamedDisplay(&'static str),
    /// A bare virtual-desktop coordinate: the pre-R1576 reading.
    Absolute((i32, i32)),
    /// No placement at all — the window manager decides. Every pre-R1087
    /// window, and the state the panel boots in.
    Unplaced,
}

impl PresetKind {
    /// Turn this preset into a concrete spec change, against the desk the
    /// binding can see right now.
    fn apply_to(self, spec: WindowSpec, desk: &DisplayTopology) -> WindowSpec {
        match self {
            Self::PrimaryDisplay => match desk.primary().or_else(|| desk.fallback()) {
                Some(display) => spec
                    .with_display(display.id().clone())
                    .with_position(PANEL_INSET.0, PANEL_INSET.1),
                // A headless desk has no display to name, so the preset degrades
                // to an absolute placement rather than inventing an id that
                // would then be reported as substituted.
                None => spec.with_position(PANEL_INSET.0, PANEL_INSET.1),
            },
            Self::NamedDisplay(id) => spec
                .with_display(DisplayId::new(id))
                .with_position(PANEL_INSET.0, PANEL_INSET.1),
            // `with_placement` rather than `with_position`: an absolute
            // preset must CLEAR any display the previous preset declared,
            // otherwise the same numbers would keep meaning "into that
            // monitor" and the preset would be a different place than it says.
            Self::Absolute((x, y)) => spec.with_placement(Some(WindowPlacement {
                display: None,
                offset: (x, y),
            })),
            Self::Unplaced => spec.with_placement(None),
        }
    }
}

fn preset(name: &str) -> Option<PresetKind> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

// --- State --------------------------------------------------------------------

struct DesksState {
    /// The reactive window topology — the SSOT the shell reconciles the real OS
    /// windows against. Both windows are declared at boot; a preset REWRITES the
    /// panel's spec, which is what drives R1576's placement pass.
    windows: Signal<Vec<WindowSpec>>,
    applied: Signal<String>,
    last_event: Signal<String>,
    /// The desk handle, resolved once at wiring time and held — the
    /// off-thread-producer discipline `use_window_control_sink` documents, and
    /// what makes the desk readable from an `invoke` body, which runs outside
    /// any `Owner` scope of this binding's own.
    desk: Arc<DisplayHandle>,
}

impl DesksState {
    /// `desk` is handed in rather than resolved here: this runs inside an
    /// `Owner::cache` factory, and `use_display_handle` resolves a
    /// `ProviderSlot`, which re-enters the cache — the documented
    /// `[[owner-cache-no-nested-factory]]` rule, whose guard catches it at
    /// boot. Pre-resolve the dependent slot, then enter the outer cache.
    fn new(desk: Arc<DisplayHandle>) -> Self {
        Self {
            windows: Signal::new(vec![
                WindowSpec::new(
                    Cow::Borrowed(MAIN_WINDOW),
                    "hello-displays — the desk (R1576 §5.16)",
                    SizeStrategy::Fixed {
                        width: WIN_W,
                        height: WIN_H,
                    },
                ),
                // Boots UNPLACED: the window manager decides, `placement()` is
                // `None`, and `scene/windows` reports `anchored: null` — which
                // is a different fact from "placed, and it resolved to nothing".
                WindowSpec::new(
                    Cow::Borrowed(PANEL_WINDOW),
                    "hello-displays — panel",
                    SizeStrategy::Fixed {
                        width: PANEL_W,
                        height: PANEL_H,
                    },
                ),
            ]),
            applied: Signal::new("unplaced".to_string()),
            last_event: Signal::new("booted".to_string()),
            desk,
        }
    }

    fn desk(&self) -> DisplayTopology {
        self.desk.get()
    }

    fn panel_spec(&self) -> Option<WindowSpec> {
        self.windows
            .get()
            .into_iter()
            .find(|s| s.id.as_ref() == PANEL_WINDOW)
    }

    /// Apply a preset by rewriting the panel's spec in the window signal.
    ///
    /// One assignment: the shell's placement pass sees the changed placement,
    /// drives the real window, and `scene/windows` reports the resolution. The
    /// binding never computes a screen coordinate.
    fn apply(&self, name: &str) -> Result<String, InvokeError> {
        let kind = preset(name).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{name:?} is not a preset; known: {}",
                PRESETS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        let desk = self.desk();
        let mut specs = self.windows.get();
        let slot = specs
            .iter_mut()
            .find(|s| s.id.as_ref() == PANEL_WINDOW)
            .ok_or_else(|| InvokeError::rejected("the panel window is not declared"))?;
        *slot = kind.apply_to(slot.clone(), &desk);
        let summary = describe_placement(slot, &desk);
        self.windows.set(specs);
        // The canonical name, not the caller's copy of it.
        let canonical = PRESETS
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("unplaced", |(n, _)| *n);
        self.applied.set(canonical.to_string());
        self.last_event.set(format!("applied {canonical}"));
        Ok(summary)
    }

    /// R1610 — pin the panel above (or below) everything else, by writing its
    /// spec's level. The same one-assignment shape [`Self::apply`] has: the
    /// shell's level pass sees the change and drives the live OS window.
    ///
    /// This is the analyzer-tool gesture — "keep the readout over the app I am
    /// watching" — and it has to work on a window that is already open, which is
    /// why the axis is reconcilable rather than create-time.
    fn set_level(&self, name: &str) -> Result<String, InvokeError> {
        let level = WindowLevel::from_wire(name.trim()).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{name:?} is not a window level; known: {}",
                WindowLevel::ALL
                    .iter()
                    .map(|l| l.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        let mut specs = self.windows.get();
        let slot = specs
            .iter_mut()
            .find(|s| s.id.as_ref() == PANEL_WINDOW)
            .ok_or_else(|| InvokeError::rejected("the panel window is not declared"))?;
        *slot = slot.clone().with_level(level);
        self.windows.set(specs);
        self.last_event.set(format!("level {}", level.as_str()));
        Ok(level.as_str().to_string())
    }
}

/// R1617 — `kind:derived:platform` for one window's home, or `unstamped`.
///
/// A pure formatter over the answer, because the two readers here reach it
/// through different doors and must not therefore describe it differently: a
/// `view` runs under an `Owner` and calls [`use_window_home`], while the
/// oracle's `query` does not and reads the handle it resolved at wiring time.
/// One fact, one derivation, one description.
///
/// An answerer that named nothing contributes an empty field, which is a third
/// thing again from `unstamped` — nobody looked at all — and the two must stay
/// distinguishable.
fn describe_home(home: Option<DisplayHome>) -> String {
    home.map_or_else(
        || "unstamped".to_string(),
        |home| {
            let name = |id: Option<&DisplayId>| id.map_or("", DisplayId::as_str).to_string();
            format!(
                "{}:{}:{}",
                home.name(),
                name(home.derived()),
                name(home.platform()),
            )
        },
    )
}

/// One line describing where a spec's placement lands on `desk`.
///
/// Derived on every read from the declaration plus the desk, so it cannot
/// become a stale second copy of where the window is.
fn describe_placement(spec: &WindowSpec, desk: &DisplayTopology) -> String {
    let Some(placement) = spec.placement() else {
        return "unplaced (the window manager decides)".to_string();
    };
    let anchored = placement.resolve(desk);
    let at = anchored
        .at()
        .map_or_else(|| "nowhere".to_string(), |(x, y)| format!("{x},{y}"));
    let used = anchored
        .display()
        .map_or_else(|| "none".to_string(), |id| id.as_str().to_string());
    format!("{} on {used} at {at}", anchored.name())
}

fn use_desks_state() -> Rc<DesksState> {
    // Pre-resolved OUTSIDE the cache — see `DesksState::new`.
    let desk = use_display_handle();
    Owner::current()
        .expect("use_desks_state requires an active Owner scope")
        .cache(STATE_KEY, || DesksState::new(desk))
}

// --- The oracle (primary External) --------------------------------------------

/// Publishes the desk, the panel's declared placement, and what that placement
/// resolves to; owns the preset verb and the rectangle-resolution verb.
struct DeskOracle {
    state: Option<Rc<DesksState>>,
}

impl core::fmt::Debug for DeskOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeskOracle")
            .field("attached", &self.state.is_some())
            .finish()
    }
}

impl DeskOracle {
    /// R1564 §5.15 — the one sentence for "not wired to a model yet".
    const NO_STATE: &str = "this desk surface is not bound to a model yet";

    fn new() -> Self {
        Self { state: None }
    }

    fn attach_state(&mut self, state: Rc<DesksState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<DesksState>, InvokeError> {
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
}

impl ExternalIntrospect for DeskOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The desk, as the BINDING sees it — the same facts
                    // `scene/displays` publishes, read through `use_displays`
                    // rather than over the wire, so the two can be compared.
                    SchemaField::new("display_count", "int"),
                    SchemaField::new("display_ids", "string"),
                    SchemaField::new("primary_id", "string"),
                    SchemaField::new("fallback_id", "string"),
                    SchemaField::new("gap_free", "string"),
                    SchemaField::new("covered_px", "int"),
                    SchemaField::new("bounding_box", "string"),
                    // The panel's DECLARED placement and what it resolves to.
                    SchemaField::new("panel_display", "string"),
                    SchemaField::new("panel_offset", "string"),
                    SchemaField::new("panel_placement", "string"),
                    // R1610 — the panel's declared LEVEL and the names it can
                    // take. `level_names` is what makes a level picker
                    // buildable from the wire alone, where two bits inside a
                    // flags word carry no enumeration of themselves.
                    SchemaField::new("panel_level", "string"),
                    SchemaField::new("level_names", "string"),
                    // R1617 — which display each window is ACTUALLY on, per
                    // BOTH answerers, read in-process through
                    // `use_window_home`. The wire publishes the same fact on
                    // `scene/windows`, so a demo can hold the two together —
                    // and the conventional in-process API answers with the
                    // platform's opinion ALONE, with its own geometric
                    // derivation private.
                    SchemaField::new("panel_home", "string"),
                    SchemaField::new("main_home", "string"),
                    SchemaField::new("home_kinds", "string"),
                    SchemaField::new("applied_preset", "string"),
                    SchemaField::new("preset_names", "string"),
                    SchemaField::new("last_event", "string"),
                    // Verbs.
                    SchemaField::action("apply", "string"),
                    SchemaField::action("resolve", "string"),
                    SchemaField::action("level", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let desk = state.desk();
        let int = |v: u64| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        let text = |s: String| Some(IntrospectValue::Text(s));
        match path {
            "display_count" => int(u64::try_from(desk.len()).unwrap_or(u64::MAX)),
            "display_ids" => text(
                desk.iter()
                    .map(|d| d.id().as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            // The empty string is "no primary", a real state on a headless desk
            // — and one the conventional API models as a null screen pointer.
            "primary_id" => text(
                desk.primary()
                    .map(|d| d.id().as_str().to_string())
                    .unwrap_or_default(),
            ),
            "fallback_id" => text(
                desk.fallback()
                    .map(|d| d.id().as_str().to_string())
                    .unwrap_or_default(),
            ),
            "gap_free" => text(desk.is_gap_free().to_string()),
            "covered_px" => int(desk.covered_px()),
            "bounding_box" => text(desk.bounding_box().map_or_else(
                || "none".to_string(),
                |b| format!("{},{} {}x{}", b.x, b.y, b.w, b.h),
            )),
            "panel_display" => text(
                state
                    .panel_spec()
                    .and_then(|s| s.display.map(|d| d.as_str().to_string()))
                    .unwrap_or_default(),
            ),
            "panel_offset" => text(
                state
                    .panel_spec()
                    .and_then(|s| s.position)
                    .map_or_else(|| "none".to_string(), |(x, y)| format!("{x},{y}")),
            ),
            "panel_placement" => text(
                state
                    .panel_spec()
                    .map_or_else(|| "no panel".to_string(), |s| describe_placement(&s, &desk)),
            ),
            "panel_level" => text(
                state
                    .panel_spec()
                    .map_or_else(|| "no panel".to_string(), |s| s.level.as_str().to_string()),
            ),
            "level_names" => text(
                WindowLevel::ALL
                    .iter()
                    .map(|l| l.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            // R1617 — `kind:derived:platform`, with an empty field for an
            // answerer that named nothing, and `unstamped` when nobody looked
            // at all. Flattened into one string because this surface's values
            // are scalars; the wire carries the three as separate keys.
            "panel_home" => text(describe_home(state.desk.home_of(PANEL_WINDOW))),
            "main_home" => text(describe_home(state.desk.home_of(MAIN_WINDOW))),
            "home_kinds" => text(DisplayHome::KINDS.join(",")),
            "applied_preset" => text(state.applied.get()),
            "preset_names" => text(
                PRESETS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "last_event" => text(state.last_event.get()),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Every declared path here is a READ of a derived fact: the desk is the
        // platform's, and the panel's placement is written through `apply`,
        // which is a verb because it takes a preset NAME rather than a value.
        // R1566 — a path this surface declares is never reported unknown.
        if self.schema().fields.iter().any(|f| f.path == path) {
            return Err(InterveneError::ReadOnly);
        }
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "apply" => {
                let state = self.state()?.clone();
                let name = Self::text(&args)?;
                state.apply(name.trim()).map(IntrospectValue::Text)
            }
            "resolve" => {
                let state = self.state()?;
                let raw = Self::text(&args)?;
                let rect = parse_rect(&raw).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "malformed rectangle {raw:?} (expected \"x,y,w,h\")"
                    ))
                })?;
                let placement = state.desk().resolve(rect);
                // The same pure function `scene/displays` answers with, called
                // from the binding — so the demo can assert the two agree
                // rather than trusting that they would.
                Ok(IntrospectValue::Text(format!(
                    "home={} visible={} total={} offscreen={} suggestion={}",
                    placement
                        .home
                        .as_ref()
                        .map_or("none", pinion_core::display::DisplayId::as_str),
                    placement.visible_px,
                    placement.total_px,
                    placement.offscreen_px(),
                    placement
                        .suggestion
                        .map_or_else(|| "none".to_string(), |(x, y)| format!("{x},{y}")),
                )))
            }
            "level" => {
                let state = self.state()?.clone();
                let name = Self::text(&args)?;
                state.set_level(&name).map(IntrospectValue::Text)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for DeskOracle {
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
}

/// Parse the `"x,y,w,h"` argument shape `resolve` accepts.
fn parse_rect(raw: &str) -> Option<DisplayRect> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return None;
    }
    Some(DisplayRect::new(
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

// --- The view -----------------------------------------------------------------

fn line(text: impl Into<String>, y: u32, size: u32, fg: pinion_core::style::Color) -> Scene {
    Scene::Text(TextNode::styled(
        text,
        Rect::new(24, y, WIN_W - 48, size + 4),
        TextStyle::new().with_size_px(size).with_fg(fg),
    ))
}

fn view_main() -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_desks_state();
    let desk = state.desk();
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let mut children = vec![
        line(
            "A place is stated relative to a display, so a saved layout survives the desk changing",
            20,
            TITLE_FONT_PX,
            ink,
        ),
        line(
            format!(
                "{} display(s) · bounding box {} · {} px covered · gap-free {}",
                desk.len(),
                desk.bounding_box().map_or_else(
                    || "none".to_string(),
                    |b| format!("{},{} {}x{}", b.x, b.y, b.w, b.h)
                ),
                desk.covered_px(),
                desk.is_gap_free(),
            ),
            46,
            BODY_FONT_PX,
            muted,
        ),
    ];

    let mut y = 78;
    children.extend(display_rows(&desk, &mut y, ink, muted));
    children.extend(preset_rows(&applied_preset(), &mut y, ink, muted));
    children.extend(level_rows(
        state.panel_spec().map_or(WindowLevel::Normal, |s| s.level),
        &mut y,
        ink,
        muted,
    ));
    children.push(line(
        format!(
            "panel: {}",
            state.panel_spec().map_or_else(
                || "not declared".to_string(),
                |s| describe_placement(&s, &desk)
            )
        ),
        y,
        BODY_FONT_PX,
        ink,
    ));

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// The name of the preset last applied — read through the state hook so the two
/// row builders below stay pure functions of what they are given.
fn applied_preset() -> String {
    use_desks_state().applied.get()
}

/// One row per attached display, advancing `y` past them.
fn display_rows(
    desk: &DisplayTopology,
    y: &mut u32,
    ink: pinion_core::style::Color,
    muted: pinion_core::style::Color,
) -> Vec<Scene> {
    let mut rows = Vec::new();
    for display in desk.iter() {
        let (lw, lh) = display.logical_size();
        rows.push(line(
            format!(
                "{}{}  \"{}\"  {},{} {}x{} phys · {lw:.0}x{lh:.0} logical · scale {} · {}",
                if display.primary() { "* " } else { "  " },
                display.id(),
                display.label(),
                display.bounds().x,
                display.bounds().y,
                display.bounds().w,
                display.bounds().h,
                display.scale_factor(),
                display.refresh_mhz().map_or_else(
                    || "refresh unreported".to_string(),
                    |m| format!("{:.2} Hz", f64::from(m) / 1000.0)
                ),
            ),
            *y,
            BODY_FONT_PX,
            ink,
        ));
        *y += ROW_H;
    }
    if desk.is_empty() {
        rows.push(line(
            "no displays attached (a headless session — a state, not a failure)",
            *y,
            BODY_FONT_PX,
            muted,
        ));
        *y += ROW_H;
    }
    rows
}

/// One row per preset, marking the one last applied.
fn preset_rows(
    applied: &str,
    y: &mut u32,
    ink: pinion_core::style::Color,
    muted: pinion_core::style::Color,
) -> Vec<Scene> {
    let mut rows = Vec::new();
    *y += 14;
    rows.push(line("presets", *y, BODY_FONT_PX, muted));
    *y += ROW_H;
    for (name, kind) in PRESETS {
        let marker = if *name == applied { ">" } else { " " };
        let what = match kind {
            PresetKind::PrimaryDisplay => "the primary display, 48px in".to_string(),
            PresetKind::NamedDisplay(id) => format!("display \"{id}\", 48px in"),
            PresetKind::Absolute((x, y)) => format!("absolute desktop ({x}, {y})"),
            PresetKind::Unplaced => "unplaced — the window manager decides".to_string(),
        };
        rows.push(line(
            format!("{marker} {name}: {what}"),
            *y,
            BODY_FONT_PX,
            ink,
        ));
        *y += ROW_H;
    }
    *y += 14;
    rows
}

/// R1610 — one row per window level, marking the panel's declared one.
///
/// A level PICKER is the thing this row set is: three values a user chooses
/// between. Encoded as two independent bits inside a flags word carrying
/// twenty unrelated hints, the program cannot ask its own vocabulary what the
/// levels are — it has to know the two spellings and that they are mutually
/// exclusive, a rule each platform backend enforces differently.
fn level_rows(
    declared: WindowLevel,
    y: &mut u32,
    ink: pinion_core::style::Color,
    muted: pinion_core::style::Color,
) -> Vec<Scene> {
    let mut rows = Vec::new();
    rows.push(line("window level (panel)", *y, BODY_FONT_PX, muted));
    *y += ROW_H;
    for level in WindowLevel::ALL {
        let marker = if level == declared { ">" } else { " " };
        let what = match level {
            WindowLevel::AlwaysOnBottom => "below ordinary windows (a desktop widget)",
            WindowLevel::Normal => "ordinary stacking",
            WindowLevel::AlwaysOnTop => "above ordinary windows (a floating readout)",
        };
        rows.push(line(
            format!("{marker} {}: {what}", level.as_str()),
            *y,
            BODY_FONT_PX,
            ink,
        ));
        *y += ROW_H;
    }
    *y += 14;
    rows
}

fn view_panel() -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_desks_state();
    let desk = state.desk();
    Scene::Container(
        ContainerNode::new(vec![
            Scene::Text(TextNode::styled(
                "panel",
                Rect::new(20, 20, PANEL_W - 40, TITLE_FONT_PX + 4),
                TextStyle::new()
                    .with_size_px(TITLE_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )),
            Scene::Text(TextNode::styled(
                state.panel_spec().map_or_else(
                    || "not declared".to_string(),
                    |s| describe_placement(&s, &desk),
                ),
                Rect::new(20, 48, PANEL_W - 40, BODY_FONT_PX + 4),
                TextStyle::new()
                    .with_size_px(BODY_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )),
            // R1610 — the level the panel declares, painted where the placement
            // is, because they are the same kind of fact: a declaration whose
            // resolution the wire reports.
            Scene::Text(TextNode::styled(
                state.panel_spec().map_or_else(
                    || "no level".to_string(),
                    |s| format!("level: {}", s.level.as_str()),
                ),
                Rect::new(20, 48 + ROW_H, PANEL_W - 40, BODY_FONT_PX + 4),
                TextStyle::new()
                    .with_size_px(BODY_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )),
            // R1617 — and where the panel ACTUALLY is, per both answerers. A
            // `view` runs under an `Owner`, so this is the hook rather than the
            // held handle; the oracle beside it reads the same fact through the
            // handle because an `invoke` body does not. One fact, one
            // derivation, two callers.
            Scene::Text(TextNode::styled(
                format!("home: {}", describe_home(use_window_home(PANEL_WINDOW))),
                Rect::new(20, 48 + ROW_H * 2, PANEL_W - 40, BODY_FONT_PX + 4),
                TextStyle::new()
                    .with_size_px(BODY_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )),
        ])
        .with_tag(PANEL_TAG)
        .with_style(BoxStyle::filled(
            theme.resolve(ColorRole::SurfaceContainerHigh),
        ))
        .with_layout(LayoutStyle::new().with_size(Size::px(PANEL_W, PANEL_H))),
    )
}

struct DisplaysView;

impl WidgetCore for DisplaysView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = DeskOracle::new();
        oracle.attach_state(use_desks_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        view_main()
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-displays (R1576 §5.16 the desk a window sits on)"
    }
}

impl WidgetA11y for DisplaysView {
    /// The view is a WAI-ARIA `group` whose value text says what the window
    /// says — how many displays, which is primary, and where the panel is
    /// declared to be with what became of that declaration.
    ///
    /// The last part is the one the conventional shape cannot reach: a window's
    /// screen is known to the window object and to nothing an accessibility
    /// client can ask.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_desks_state();
        let desk = state.desk();
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Attached displays and the panel's declared place")
                .with_value(AccessValue::Text(format!(
                    "{} display(s), primary {}, gap-free {}; panel {}",
                    desk.len(),
                    desk.primary()
                        .map_or_else(|| "none".to_string(), |d| d.id().as_str().to_string()),
                    desk.is_gap_free(),
                    state.panel_spec().map_or_else(
                        || "not declared".to_string(),
                        |s| describe_placement(&s, &desk)
                    ),
                ))),
        ]
    }
}

impl WidgetView for DisplaysView {
    type Renderer = HelloDisplaysRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    /// R683 §5.16 §5.41 — the reactive window topology. A preset rewrites the
    /// panel's spec in this signal, and the shell's placement pass drives the
    /// real OS window: the binding never touches a winit handle, and never
    /// computes a screen coordinate.
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        Some(Rc::new(use_desks_state().windows.clone()))
    }

    fn view_for_window(window_id: &str, _state: Self::State, _frame: &Frame) -> Scene {
        if window_id == PANEL_WINDOW {
            view_panel()
        } else {
            view_main()
        }
    }
}

fn main() {
    pinion_shell::run::<DisplaysView>();
}

#[cfg(test)]
mod tests;
