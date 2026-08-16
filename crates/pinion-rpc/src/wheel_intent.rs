//! `scene/wheel_intent` — **what a wheel over this point would do**, asked
//! before turning it.
//!
//! # Why this method exists
//!
//! A wheel is the only pointer gesture whose meaning is entirely local policy.
//! The same motion over the same pixel scrolls a page, steps a value, flips a
//! tab or zooms a canvas, and the event says nothing about which. Every toolkit
//! resolves it the same way — the widget under the cursor overrides a wheel
//! handler — and every toolkit therefore makes the behaviour discoverable only
//! by trying it.
//!
//! Measured on the reference toolkit at 6.11.1, by building a probe and running
//! it offscreen rather than by reading its documentation: over the four widget
//! classes that answer a wheel there, **309 introspectable properties and 172
//! introspectable methods contain zero** naming the wheel. The same probe
//! measured what the silence costs — a **closed, unfocused** combo box sitting
//! in a form steps its value on a wheel aimed at the form (index 1 → 2 with no
//! focus, 2 → 3 with it), and nothing in that widget's interface lets the form
//! find out beforehand, ask it to stop, or even report afterwards which of the
//! things under the cursor ate the scroll.
//!
//! # What makes the answer trustworthy
//!
//! Not that the surface is honest. The value published here is **the same value
//! the router routes by**: since R1703 a widget is offered
//! [`External::wheel`] only when its
//! [`wheel_intent`](pinion_core::external::External::wheel_intent) is `Some`,
//! so a declaration that lies about doing nothing removes the behaviour, and a
//! behaviour with no declaration is never reached. There is one fact, read two
//! ways, rather than a description maintained beside an implementation.
//!
//! # Shape
//!
//! With no parameters it is a census: every painted `External`, and what it
//! says. That is what a boot gate and a form audit want — *which controls on
//! this screen will take a wheel away from the page*, a question the reference
//! cannot pose at all.
//!
//! With `at: {x, y}` (or `path: "<tag>"`, resolved to that tag's painted
//! rectangle centre) it answers for one point, **including the fall-through**:
//! when no surface there declares an intent the wheel reaches the nearest
//! attached [`Scene::Scroll`] ancestor, and the answer says whether one is
//! there. So the whole question — "what happens if I turn the wheel here" — has
//! one call, rather than being split between what a widget might do and what
//! the framework would then do instead.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/wheel_intent",
//!   "params": { "at": { "x": 820.0, "y": 470.0 } }, "id": 1 }
//! ```
//!
//! [`External::wheel`]: pinion_core::external::External::wheel
//! [`Scene::Scroll`]: pinion_core::Scene

use pinion_core::scene::{Rect, Scene};
use pinion_core::widgets::wheel::WheelIntent;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// The point a surface's census row is asked about: its painted rectangle's
/// centre.
///
/// A wheel intent is a question about a POINT — R1703 measured why, one minute
/// after the first wheel worked: a screen is one `External` (§2 #7), so a
/// surface-wide answer said "a wheel here zooms" over a palette where the
/// screen declines. The per-surface census therefore has to name where it
/// asked, and the middle is the representative point: for a catalog widget the
/// surface IS the control, and for a screen it says what the middle of the
/// screen does. A caller wanting a specific control asks with `at` / `path`.
fn centre(rect: Rect) -> (f64, f64) {
    (
        f64::from(rect.x) + f64::from(rect.w) / 2.0,
        f64::from(rect.y) + f64::from(rect.h) / 2.0,
    )
}

/// What one surface says a wheel over it does.
#[derive(Debug, Clone, Serialize)]
pub struct WheelIntentRow {
    /// The painted `External` this row sampled.
    pub surface: String,
    /// The tag the router actually resolves at [`asked_x`](Self::asked_x) /
    /// [`asked_y`](Self::asked_y) — usually [`surface`](Self::surface) itself,
    /// and something else when another surface is painted over that point.
    ///
    /// ★ Published because the first run of this census produced exactly that
    /// case and it read as a contradiction: an open combo box paints its
    /// options over its dismiss barrier, so the row labelled `combo_barrier`
    /// carried the option list's answer. The row was right and its label was
    /// incomplete — the wheel's own version of R1700's `covering`, and the
    /// honest fix is to say who answered rather than to sample somewhere else.
    pub answered_by: Option<String>,
    /// The point the row is about — the surface's painted centre. Published
    /// rather than implied, because an intent is a fact about a POINT and a row
    /// that did not say where it asked would be the coarse answer this method
    /// was built to stop giving.
    pub asked_x: f64,
    /// The point the row is about.
    pub asked_y: f64,
    /// The declared intent's wire word (`"step"` / `"zoom"`), or `null` for a
    /// surface that declares none there and therefore never receives a wheel.
    pub intent: Option<&'static str>,
    /// What one notch moves for a stepping intent (`"item"` / `"value"`),
    /// `null` otherwise.
    pub unit: Option<&'static str>,
}

/// The answer for one point.
#[derive(Debug, Clone, Serialize)]
pub struct WheelIntentAt {
    /// The window-logical point the answer is about.
    pub x: f64,
    /// The window-logical point the answer is about.
    pub y: f64,
    /// The deepest painted surface covering it, or `null` when none does.
    pub surface: Option<String>,
    /// That surface's declared intent, or `null`.
    pub intent: Option<&'static str>,
    /// The stepping unit, or `null`.
    pub unit: Option<&'static str>,
    /// Where the wheel goes when no surface here declares an intent:
    /// `"scroll"` when an attached scroll container covers the point,
    /// `"nothing"` when the wheel would be dropped. `null` when a surface DID
    /// declare one, since then nothing falls through.
    pub falls_through_to: Option<&'static str>,
}

/// The `scene/wheel_intent` result.
#[derive(Debug, Clone, Serialize)]
pub struct WheelIntentReport {
    /// One row per painted `External`, in scene order.
    pub surfaces: Vec<WheelIntentRow>,
    /// How many of them declare an intent — the number a form audit reads.
    pub declared: usize,
    /// The per-point answer, when the call carried `at` / `path`.
    pub at: Option<WheelIntentAt>,
}

fn words(intent: Option<WheelIntent>) -> (Option<&'static str>, Option<&'static str>) {
    (
        intent.map(WheelIntent::as_str),
        intent
            .and_then(WheelIntent::unit)
            .map(pinion_core::widgets::wheel::StepUnit::as_str),
    )
}

/// Every painted `External`, paired with the point its row is about.
fn painted_surfaces(paint: &Scene, state_scene: &Scene) -> Vec<(String, Rect)> {
    let painted = paint.absolute_rects_by_tag();
    let mut found = Vec::new();
    state_scene.for_each_node(&mut |visit| {
        if let Scene::External(node) = visit.node
            && let Some(tag) = node.tag.as_deref()
            && let Some(rect) = painted.get(tag)
            && rect.w > 0
            && rect.h > 0
        {
            found.push((tag.to_owned(), *rect));
        }
    });
    found
}

/// R1703 §5.45 §5.15 — compute the report.
///
/// A screen that has not painted answers an empty census rather than an error,
/// like [`crate::pointer_target`]: "no surface declares anything" is true of a
/// screen that does not exist yet, and an error would make a boot-time check
/// impossible to run before the first frame.
///
/// # Errors
///
/// Only [`RpcError::internal_error`], when the report fails to serialise —
/// propagated rather than unwrapped because a panic in a dispatcher takes the
/// whole surface down.
pub fn handle_scene_wheel_intent(
    last_paint_scene: Option<&Scene>,
    state_scene: &Scene,
    at: Option<(f64, f64)>,
) -> Result<Value, RpcError> {
    let report = last_paint_scene.map_or_else(
        || WheelIntentReport {
            surfaces: Vec::new(),
            declared: 0,
            at: None,
        },
        |paint| build(paint, state_scene, at),
    );
    serde_json::to_value(&report).map_err(|e| RpcError::internal_error(e.to_string()))
}

fn build(paint: &Scene, state_scene: &Scene, at: Option<(f64, f64)>) -> WheelIntentReport {
    let ask = |point: (f64, f64)| pinion_runtime::wheel_intent_at(paint, state_scene, point);
    let surfaces: Vec<WheelIntentRow> = painted_surfaces(paint, state_scene)
        .into_iter()
        .map(|(tag, rect)| {
            // Asked THROUGH the router's resolution at the surface's middle,
            // not read off the node: the surface a point resolves to is the
            // router's answer, and this method's whole claim is that it
            // publishes the value the dispatch uses.
            let (x, y) = centre(rect);
            let hit = ask((x, y));
            let (intent, unit) = words(hit.as_ref().and_then(|(_, i)| *i));
            WheelIntentRow {
                surface: tag,
                answered_by: hit.map(|(who, _)| who),
                asked_x: x,
                asked_y: y,
                intent,
                unit,
            }
        })
        .collect();
    let declared = surfaces.iter().filter(|r| r.intent.is_some()).count();
    let at = at.map(|(x, y)| {
        let hit = ask((x, y));
        let (intent, unit) = words(hit.as_ref().and_then(|(_, i)| *i));
        let falls_through_to = intent.is_none().then(|| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a window-logical coordinate the scroll lookup takes \
                          as a pixel index; a negative or absurd value simply \
                          covers nothing"
            )]
            let (px, py) = (x.max(0.0) as u32, y.max(0.0) as u32);
            if paint.scroll_state_at(px, py).is_some() {
                "scroll"
            } else {
                "nothing"
            }
        });
        WheelIntentAt {
            x,
            y,
            surface: hit.map(|(tag, _)| tag),
            intent,
            unit,
            falls_through_to,
        }
    });
    WheelIntentReport {
        surfaces,
        declared,
        at,
    }
}
