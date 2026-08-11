//! `scene/pointer_reach` — which of this screen a real pointer can drive, and
//! what is stealing the rest (R1650 §5.12 §5.35 §2 #2 §2 #7).
//!
//! # The two facts, only one of which was ever asserted
//!
//! Driving a control over the wire — `scene/click`, `scene/invoke`,
//! `scene/send` — calls the widget's handler. Driving it with a mouse asks the
//! §5.35 router to *find* that handler first: it resolves the deepest tagged
//! node under the cursor and looks the primary half of that tag up as an
//! `External`, and when the lookup fails it returns without a word. So "the
//! handler is right" and "the handler is reachable" are two claims, the wire
//! verbs prove only the first, and a screen can be **completely dead to a mouse**
//! with every scripted assertion green.
//!
//! That is not hypothetical here. R1649.1 measured it on a shell whose demo
//! made 118 assertions: every card, panel and palette row was tagged so it
//! could be addressed, each tag shadowed the window's one `External`, and no
//! press ever arrived. R1497 measured the same shape one widget down —
//! `hello-column-reorder` lost 100% of the clicks that landed on a header's own
//! centred label and none of the ones beside it.
//!
//! This method makes that question askable before a person finds out by
//! pressing something.
//!
//! # Against the toolkit 6.11
//!
//! There the analogous silence is *unaskable*, and for a structural reason:
//! every widget is itself an event target, so a press is never dropped for want
//! of a receiver — it is delivered to a child that ignores it, which looks
//! identical from outside and is a virtual function rather than data.
//! `WA_TransparentForMouseEvents` is readable one widget at a time through
//! `testAttribute`, nothing aggregates it, and `childAt()` answers *which*
//! widget is under a point but not whether that widget does anything with a
//! press. An external driver cannot ask the question at all; a developer
//! answers it with a debugger and a parent-chain walk.
//!
//! Here the whole answer is a pure function of the painted scene and the state
//! scene, so it is one round trip an agent takes *before* deciding a screen is
//! operable — and the row names both the thief and the victim, so the repair
//! is addressable rather than a search.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "deliverable": 1,
//!     "inert": 12,
//!     "shadows": [
//!       { "tag": "card.alpha", "path": "card.alpha", "shadowed": "shell.root" }
//!     ],
//!     "unreachable": [
//!       { "tag": "shell.root", "path": "", "blocked_by": "card.alpha" }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; it reads the last painted scene, so what it reports
//! is what the router is refusing right now.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/pointer_reach", "id": 1 }
//! ```
//!
//! `unreachable` empty is the operable answer, and it is the half worth
//! gating: a widget listed there cannot be pressed at the centre of its own
//! painted rect — the point `scene/click {path}` presses and the point a person
//! aims at. `shadows` is the wider census behind it, and a non-empty `shadows`
//! with an empty `unreachable` is a normal screen: a tagged header band or
//! scroll region intercepts presses in its own gaps while every widget still
//! answers where it lives. Reporting only the verdict would hide the drift that
//! produces one; reporting only the census would cry wolf.
//!
//! `shadows` empty is the stricter answer. The two counts beside it are what
//! keep an empty list from reading as coverage: `deliverable` is how many
//! painted tags a press actually arrives at, and `inert` is how many resolve to
//! nothing while taking nothing away — a legend swatch, a chart mark, a caption.
//! A surface reporting `deliverable: 0` has no widget on it at all, which an
//! empty `shadows` list alone would let a caller mistake for health.
//!
//! `path` is the address `scene/snapshot`, `scene/locate` and `scene/click`
//! all accept, so the offending node can be looked at in the same session.

use pinion_core::Scene;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One painted tag that swallows input a widget above it would have received.
#[derive(Debug, Clone, Serialize)]
pub struct ShadowEntry {
    /// The paint tag the router resolves at this node — including any
    /// composite `#n` half, because that is the spelling the router saw.
    pub tag: String,
    /// The node's address, `/`-joined the way `scene/locate` reports it.
    pub path: String,
    /// The tag of the nearest ancestor that **does** resolve to an `External`:
    /// the widget losing the press. Naming the victim is what makes the row
    /// actionable — the repair is a declaration on `tag`, and knowing what it
    /// was covering is how a caller decides whether that is the right repair.
    pub shadowed: String,
}

/// One widget that cannot be pressed at the centre of its own painted rect.
#[derive(Debug, Clone, Serialize)]
pub struct UnreachableEntry {
    /// The widget's paint tag.
    pub tag: String,
    /// Its address.
    pub path: String,
    /// What the router resolves there instead, or `null` when the centre hits
    /// nothing at all — off-window or zero-area, which is a different repair.
    pub blocked_by: Option<String>,
}

/// The `scene/pointer_reach` result.
#[derive(Debug, Clone, Serialize)]
pub struct PointerReachReport {
    /// Painted tags a press arrives at, if one can land on them.
    pub deliverable: usize,
    /// Painted tags that resolve to nothing and take nothing away.
    pub inert: usize,
    /// Every tag intercepting input a widget above it would have received —
    /// the census, in paint order.
    pub shadows: Vec<ShadowEntry>,
    /// The widgets that are not operable, in paint order — the verdict.
    pub unreachable: Vec<UnreachableEntry>,
}

/// R1650 §5.35 — compute the report from the painted scene and the state scene.
///
/// A binding that has not painted answers with an all-zero report rather than
/// an error: "nothing is unreachable" is true of a screen that does not exist
/// yet, and an error here would make the boot-time check impossible to run
/// before the first frame.
///
/// # Errors
///
/// Only [`RpcError::internal_error`], when the report fails to serialise —
/// which cannot happen for these three plain structs and is propagated rather
/// than unwrapped because a panic in a dispatcher takes the whole surface down.
pub fn handle_scene_pointer_reach(
    last_paint_scene: Option<&Scene>,
    state_scene: &Scene,
) -> Result<Value, RpcError> {
    let report = last_paint_scene.map_or(
        PointerReachReport {
            deliverable: 0,
            inert: 0,
            shadows: Vec::new(),
            unreachable: Vec::new(),
        },
        |paint| {
            let reach = pinion_runtime::pointer_reach(paint, state_scene);
            PointerReachReport {
                deliverable: reach.deliverable,
                inert: reach.inert,
                shadows: reach
                    .shadows
                    .into_iter()
                    .map(|s| ShadowEntry {
                        tag: s.tag,
                        path: s.path.join("/"),
                        shadowed: s.shadowed,
                    })
                    .collect(),
                unreachable: reach
                    .unreachable
                    .into_iter()
                    .map(|u| UnreachableEntry {
                        tag: u.tag,
                        path: u.path.join("/"),
                        blocked_by: u.blocked_by,
                    })
                    .collect(),
            }
        },
    );
    serde_json::to_value(report).map_err(RpcError::internal_error)
}
