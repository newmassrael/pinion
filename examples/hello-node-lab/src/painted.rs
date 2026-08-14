//! R1653 — **the integration test: the screen the pipeline paints, against the
//! specification it is supposed to reproduce.**
//!
//! `tests.rs` next door asserts the *model* against [`crate::spec`] and the
//! geometry helpers against each other. Neither of them renders anything. The
//! only thing that had ever compared the reference screen to the pixels was
//! `tools/demos/r1651_the_node_lab_matches_the_reference.py`, which needs a
//! live process, a wire and a python interpreter, and which no local gate runs.
//!
//! That gap has a measured history. Three consecutive rounds shipped a screen
//! whose paint disagreed with its own hit test — R1648's cards drawn at double
//! their offset, R1651.1's rail seats drawn in window coordinates while the
//! press was resolved in pane coordinates, R1652.1's six-element list drawn on
//! top of the field below it — and in all three cases every test in `tests.rs`
//! passed, because every one of them asks the geometry *helper* where a control
//! is. The helper was right each time. The painter was not.
//!
//! So this module runs the real pipeline — `view()` then
//! [`pinion_runtime::compute_layout`], the same two stages the window runs
//! before handing a scene to the rasteriser — and asks the resulting scene
//! where things ended up, through [`pinion_core::NodeVisit::absolute_rect`]. It
//! then holds the screen to five properties, at every state in [`STATES`]
//! rather than only at the one the specification describes:
//!
//! 1. **Forward** — every element the specification declares is painted, with a
//!    rectangle somebody can see.
//! 2. **Backward** — every painted tag belongs to a declared family, and the
//!    families whose size the specification fixes are that size.
//! 3. **Reachable** — every painted control answers for *itself* when pressed
//!    at the centre of the rectangle it was painted in.
//! 4. **Contained** — every painted mark lies inside the pane its address puts
//!    it in, and every text run inside the node that owns it.
//! 5. **Disjoint** — no two controls of the settings form, and no two text runs
//!    of one widget, are painted on top of each other.
//! 6. **Clipped** — a pan that carries the graph past the canvas edge stops
//!    painting it rather than painting it somewhere else.
//!
//! ★ The population of each check is derived from the scene or from
//! [`crate::spec`], never written out here. R1651.1 is why: a hand-written
//! population makes "40 controls pass" read as coverage when it is a sample,
//! and the three controls that round's list did not name were all broken.
//!
//! # The box is the promise, and R1654 is what makes it keepable
//!
//! Every rectangle here is the box the view **gave** a run, not the extent of
//! its glyphs, so a string wider than its box would smear over the row below
//! and this module would still report the screen as clean. R1653 wrote that
//! down as a gap because a run could not be told to elide; R1654 implemented
//! the arm that had been declared since R47.5 and never honoured, and the sweep
//! now holds the screen to the rule that closes it: **every run declares a
//! policy that shortens it.**
//!
//! That is asserted rather than measured, and deliberately: a measurement
//! depends on which fonts the host has, so a gate built on one is green here
//! and red on a machine with different metrics. The declaration is
//! font-independent, and `pinion-text`'s own tests are where "an eliding arm
//! produces something that fits" is proven.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Frame, Scene};

use super::{
    Hit, LabState, Role, WIN_H, WIN_W, canvas_rect, inspector_rect, palette_rect, rail_rect, spec,
    toolbar_rect, use_lab_state,
};

/// The states the screen is swept in.
///
/// ★ R1652.1's defect is the reason this is a list rather than one call: the
/// specification describes the screen *as it opens*, that is the only state
/// anybody had ever checked, and it is the state nobody works in. A list field
/// with one element cannot overflow the row below it; with six it did, and the
/// row below became unreachable.
///
/// Each entry mutates the live state and names what it is for. They compose —
/// state *n* is state *n-1* plus one edit — because that is what a session with
/// the tool looks like, and because a screen that only survives being reset is
/// not a screen anybody can use.
/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&std::rc::Rc<LabState>));

const STATES: &[SweptState] = &[
    ("as it opens", |_| {}),
    ("with a node added from the palette", |state| {
        super::add_node(state, Role::Responder);
    }),
    ("with a list field grown to six elements", |state| {
        for _ in 0..5 {
            super::add_element(state, "listen.endpoints");
        }
    }),
    ("with another node selected", |state| {
        let store = state.node_of("S-01").expect("the opening graph has it");
        state.selected.set(Some(store));
    }),
    ("with the launch gate closed by a bad value", |state| {
        let router = state.node_of("R-01").expect("the opening graph has it");
        state.selected.set(Some(router));
        super::set_and_sync(state, "transport.link.tx.batch_size", "70000");
    }),
    ("zoomed out", |state| {
        state.zoom.set(super::ZOOM_MIN);
    }),
    (
        "panned, by the gesture the hint strip advertises",
        |state| {
            // Driven through the app's own drag handler rather than by writing the
            // pan: a state a test invents can be one the screen cannot reach, and
            // the defect this sweep found here — a leftward pan underflowing the
            // pane-local conversion — only counts if a mouse can produce it.
            state.zoom.set(spec::OPENING_ZOOM);
            let canvas = super::canvas_rect();
            let start = (canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
            super::move_cursor(state, start.0, start.1);
            super::press(state);
            super::move_cursor(state, start.0 - 30, start.1 + 20);
            super::release(state);
        },
    ),
    ("running", |state| {
        state.running.set(true);
    }),
    // ★★ R1679 — a card the PALETTE added, then moved. The state the layout
    // predicate was blind to: its population was the specification's node list,
    // so a card that is not in the specification could be dragged anywhere and
    // the screen reported its layout unchanged (measured: [502,476] to
    // [562,512], `changed.layout` false). Every earlier swept state adds a card
    // or drags one, and none did both — which is why nothing saw it.
    ("with a card the palette added, then dragged", |state| {
        // The view first, through the screen's own operation: the states
        // are cumulative and two before this one move the view, so without
        // it the card is off the viewport and there is nothing to press.
        // "A person pressed home, then moved a card they had added" is a
        // session; a state reached by assignment is not.
        super::ResetScope::View.apply(state);
        // ★ SELF-CONTAINED, and that is not a detail: these states have two
        // consumers with opposite assumptions. The painted sweeps apply
        // them cumulatively — state n is state n-1 plus one edit — while
        // R1677's operation gate and R1679's affordance gate reset the
        // screen before each one, because their rows have to be
        // independent. A state that leaned on an earlier one for the card
        // it drags passed the first three and panicked in the fourth.
        let added_card = |state: &std::rc::Rc<LabState>| {
            state.cards().into_iter().find(|n| {
                let name = state.name_of(*n);
                !spec::NODES.iter().any(|want| want.id == name)
            })
        };
        if added_card(state).is_none() {
            super::add_node(state, Role::Publisher);
        }
        let node = added_card(state).expect("a card was added, so one is here");
        // ★ The aim comes from the PAINTED tag. `card_rect` answers in the
        // world surface's coordinates and `move_cursor` takes the window's;
        // the first draft passed one to the other, the drag silently went
        // nowhere, and the counterfactual for the very defect this state
        // exists for PASSED.
        let shot = painted(state);
        let seat = *shot
            .tags
            .get(&format!("lab.node.{}", state.name_of(node)))
            .expect("the added card is painted with the view back home");
        let (px, py) = centre(seat);
        super::move_cursor(state, px, py);
        super::press(state);
        super::move_cursor(state, px + 60, py + 36);
        super::release(state);
    }),
];

/// The window sizes the screen is swept at.
///
/// ★ R1656 — **size was never an axis here, and that is why five defects got
/// out.** R1654 wrote down the reason in its own words — "every check ran the
/// layout at `WIN_W x WIN_H`, so the assumption and the defect were the same
/// number" — and then left the axis at one entry. A person reported the next
/// two: text outside the node cards, and nodes that stop clicking after a
/// maximise. Eight states times one size is not eight cases; it is one case
/// visited eight ways.
///
/// The opening size is first because the specification describes it. The others
/// are the two a person actually produces: a maximised window on this display,
/// and a window dragged down to the floor the screen declares.
const SIZES: &[(&str, (u32, u32))] = &[
    ("at the size it opens in", (WIN_W, WIN_H)),
    ("maximised", (2494, 1531)),
    ("at its declared floor", (super::MIN_W, super::MIN_H)),
];

/// Where every tag in the painted scene ended up, and every text run with it.
struct Painted {
    /// Tag -> the rectangle the layout pass gave it, window-absolute.
    tags: BTreeMap<String, Rect>,
    /// Every text run: its content, its rectangle, and the tag of its nearest
    /// tagged ancestor. Runs carry no tag of their own, which is exactly why
    /// they were the thing R1649 painted in a stack down the left edge with a
    /// 118-assertion demo watching.
    runs: Vec<(String, Rect, Option<String>)>,
    /// ★ R1662 — tags that are NOT on screen and that a scroll offset would
    /// bring into view, with the offset that does it.
    ///
    /// The panes scroll now, so "painted" stopped being the whole question: a
    /// control below the fold of a scrolling pane is absent from
    /// [`Self::tags`] and is not missing — the reader scrolls to it. Kept
    /// beside the painted set rather than folded into it, because the two are
    /// different facts and a check that wants "on screen" must still be able
    /// to ask for exactly that.
    reachable: BTreeMap<String, (String, (i32, i32))>,
    /// ★ R1690 — what a run that carries a tag of its OWN says.
    ///
    /// Separate from [`Self::runs`] rather than folded into its owner column,
    /// and the reason is that folding it would quietly disarm the containment
    /// check: that check asks whether a run lies inside the box of the widget
    /// that owns it, and a run whose owner was itself would satisfy it for
    /// free. The owner column keeps meaning "the nearest tagged ancestor"; this
    /// answers the different question of what a named run reads.
    said: BTreeMap<String, String>,
}

impl Painted {
    /// Run the real pipeline and index what came out of it.
    fn of(scene: &Scene, window: (u32, u32)) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
        let mut reachable = BTreeMap::new();
        let mut said = BTreeMap::new();
        for out in pinion_core::reach::out_of_sight(scene, window, &mut stand_in_ink) {
            if let (Some(tag), pinion_core::reach::Reach::Scrollable { to }) = (out.tag, out.reach)
            {
                reachable.insert(tag, (out.viewport.name, to));
            }
        }
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                // Clipped entirely away: painted nowhere, so it is not painted.
                return;
            };
            if let Some(tag) = visit.node.tag() {
                tags.entry(tag.to_owned()).or_insert(rect);
            }
            if let Scene::Text(text) = visit.node {
                let owner = visit
                    .ancestors
                    .iter()
                    .rev()
                    .find_map(|a| a.tag())
                    .map(str::to_owned);
                if let Some(own) = visit.node.tag() {
                    said.insert(own.to_owned(), text.content.clone());
                }
                runs.push((text.content.clone(), rect, owner));
            }
        });
        Self {
            tags,
            runs,
            reachable,
            said,
        }
    }
}

/// Paint the screen as the window would: the view function, then the layout
/// pass, at whatever size the shell has published.
fn painted_at(state: &std::rc::Rc<LabState>, size: (u32, u32)) -> (Painted, Scene) {
    // ★ R1654 — publishing the size the way the shell does, so the sweep can
    // ask the screen to lay out at a size other than the one it was designed
    // for. Every earlier check here passed `WIN_W, WIN_H` to the layout pass,
    // which ASSUMES the thing that was wrong: the screen ignored the window and
    // painted its design size into the corner of it.
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view((TextFieldState::Idle, 0), Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
    let shot = Painted::of(&scene, size);
    assert!(
        std::rc::Rc::ptr_eq(state, &use_lab_state()),
        "the sweep must drive the state the view reads"
    );
    (shot, scene)
}

/// The index and the scene it came from.
///
/// ★ R1656 — the SCENE is handed back too, because the containment property
/// cannot be asked of the index: an index of rectangles has lost the parent
/// chain, and "inside the box that owns it" is a question about that chain.
fn painted_and_scene(state: &std::rc::Rc<LabState>, size: (u32, u32)) -> (Painted, Scene) {
    painted_at(state, size)
}

/// Paint the screen as the window would, at the design size.
fn painted(state: &std::rc::Rc<LabState>) -> Painted {
    painted_at(state, (WIN_W, WIN_H)).0
}

/// The centre of a rectangle, which is where a press is aimed.
const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.w <= outer.x + outer.w
        && inner.y + inner.h <= outer.y + outer.h
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Every tag the specification says the screen has to paint.
///
/// Derived from [`crate::spec`], so a row added to the specification is a row
/// this demands on screen without anybody editing this function.
fn declared_tags(state: &LabState) -> Vec<String> {
    let mut want: Vec<String> = vec![
        super::VIEW_TAG.to_owned(),
        "lab.appbar".into(),
        "lab.toolbar".into(),
        "lab.toolbar.title".into(),
        "lab.toolbar.meta".into(),
        "lab.toolbar.gate".into(),
        "lab.toolbar.zoom".into(),
        // ★ R1688 — the fit seat, demanded like every other toolbar control.
        "lab.toolbar.fit".into(),
        "lab.toolbar.run".into(),
        "lab.gate".into(),
        "lab.gate.verdict".into(),
        "lab.hint".into(),
        "lab.hint.text".into(),
        // ★★★ R1690 — the reach meter, demanded UNCONDITIONALLY. It is a fact
        // about the palette rather than about the selection, so a screen with
        // nothing selected still has to carry it — and the states below include
        // exactly that one, which is what makes the placement checkable rather
        // than asserted.
        "lab.inspector.reach".into(),
        "lab.inspector.reach.text".into(),
    ];
    for pane in spec::PANES {
        want.push(pane.tag.to_owned());
        if let Some(body) = pane.body {
            want.push(body.to_owned());
        }
    }
    for (seat, _) in spec::RAIL {
        want.push(format!("lab.rail.{seat}"));
    }
    for role in spec::ROLES {
        want.push(format!("lab.palette.role.{}", role.name));
        want.push(format!("lab.palette.swatch.{}", role.name));
    }
    for (kind, _) in spec::PIN_LEGEND {
        want.push(format!("lab.palette.pin.{kind}"));
    }
    for word in spec::PROTOCOLS {
        want.push(format!("lab.palette.protocol.{word}"));
    }
    for frame in spec::FRAMES {
        want.push(format!("lab.frame.{}", frame.name));
        want.push(format!("lab.frame.{}.name", frame.name));
    }
    // ★ R1678 — the reset affordances, derived from the same list that decides
    // which are painted. The gated four are demanded exactly when their scope
    // has something to put back, so this is not "the family may or may not be
    // there": at any moment the specification names precisely the buttons that
    // must exist, and the backward check names any other as invented.
    want.push("lab.reset.view".to_owned());
    for scope in super::changed_scopes(state) {
        want.push(format!("lab.reset.{}", scope.wire()));
    }
    // ★★★ R1688 — the toast, demanded exactly when the screen has said
    // something. Conditional for the same reason the four gated resets are: a
    // message region that was always there would be an empty box on the canvas,
    // and the reference paints its own only while there is something in it.
    if !state.toast.get().trim().is_empty() {
        want.push("lab.toast".to_owned());
        want.push("lab.toast.dot".to_owned());
        want.push("lab.toast.text".to_owned());
    }
    // The cards are the ones the model holds, not the ones the specification
    // opened with — R1651's real-mouse defect was a canvas that drew the
    // specification's list and therefore could never show an added node.
    for node in state.cards() {
        let id = state.name_of(node);
        want.push(format!("lab.node.{id}"));
        want.push(format!("lab.node.{id}.id"));
        want.push(format!("lab.node.{id}.badge"));
        want.push(format!("lab.pin.{id}.dial"));
    }
    if let Some(form) = super::selected_form(state) {
        for field in form.fields() {
            if field.hidden() {
                continue;
            }
            want.push(format!("lab.form.control.{}", field.key()));
            want.push(format!("lab.form.applies.{}", field.key()));
            // ★★ R1686 — every row that is shown offers to be taken away, so
            // the census demands one seat per shown row rather than one
            // somewhere. The reference draws it on every row it does not
            // derive, and every row here is authored.
            want.push(format!("lab.form.remove.{}", field.key()));
        }
        for field in form.addable() {
            want.push(format!("lab.form.add.{}", field.key()));
        }
    }
    want
}

/// What a press at this tag's centre has to answer, read off the tag alone.
///
/// The inverse of the painter's own naming scheme, and deliberately not a call
/// into the hit test: the tag is what the *painter* wrote and the answer is
/// what the *geometry* resolves, so agreeing is a claim about two derivations
/// meeting rather than a tautology.
///
/// `None` means the tag names something that is not pressable — a pane, a
/// label, a wire — and is not probed.
fn must_answer(tag: &str) -> Option<String> {
    for (prefix, verb) in [
        ("lab.rail.", "rail"),
        ("lab.palette.role.", "role"),
        ("lab.form.add.", "add"),
        ("lab.form.control.", "field"),
        // ★ R1686 — `remove:<key>`, spelled like `add:` and `field:` rather
        // than like the control's parts: it is a seat of the ROW, not an
        // affordance inside the control, and the geometry publishes it apart
        // from `parts` for the same reason.
        ("lab.form.remove.", "remove"),
    ] {
        if let Some(rest) = tag.strip_prefix(prefix) {
            return Some(format!("{verb}:{rest}"));
        }
    }
    for family in ["option", "step", "toggle", "item"] {
        if let Some(rest) = tag.strip_prefix(&format!("lab.form.{family}.")) {
            return Some(format!("{family}.{rest}"));
        }
    }
    if let Some(rest) = tag.strip_prefix("lab.pin.") {
        let (node, side) = rest.rsplit_once('.')?;
        return Some(format!("pin:{node}:{side}"));
    }
    if let Some(rest) = tag.strip_prefix("lab.node.")
        && !rest.contains('.')
    {
        return Some(format!("node:{rest}"));
    }
    // ★★ R1681.3 — the picked link's own seats. Declared here BECAUSE the card
    // sweeps now excuse whatever this chrome covers: an exception that took
    // reachability away without demanding it back would be a way to pass by
    // painting an affordance nobody can press.
    if let Some(rest) = tag.strip_prefix("lab.link.endpoint.")
        && !rest.contains('.')
    {
        return Some(format!("link:endpoint:{rest}"));
    }
    match tag {
        // ★★ R1682 — the node's-life seats. Declared for the reason R1681.3
        // wrote one line up: an affordance that is painted and not demanded
        // back is one this whole module can pass over while nobody can press
        // it. These sit in the inspector's scrolling body, so the demand is
        // also what checks that the seat and its press agree once the pane has
        // moved.
        "lab.inspector.collapse" => Some("card:collapse".into()),
        "lab.inspector.disable" => Some("card:disable".into()),
        "lab.inspector.delete" => Some("card:delete_node".into()),
        // R1683 — the one field's two seats. The field itself is an external
        // with its own hit target and is deliberately not demanded here.
        "lab.inspector.rename" => Some("card:rename".into()),
        "lab.inspector.addkey" => Some("card:addkey".into()),
        "lab.link.act" => Some("link:act".into()),
        "lab.toolbar.zoom.in" => Some("zoom:in".into()),
        "lab.toolbar.zoom.out" => Some("zoom:out".into()),
        // ★★ R1688 — the read-out is the view reset now, so it is a control and
        // is demanded back like one. It had no entry here while it was a seat
        // captioned `home`, which is the hole R1681.3 wrote down: an affordance
        // that is painted and not demanded is one this whole module passes over
        // while nobody can press it.
        "lab.reset.view" => Some("reset:view".into()),
        "lab.toolbar.fit" => Some("fit".into()),
        "lab.toolbar.gate" => Some("problem".into()),
        "lab.toolbar.config" => Some("config".into()),
        "lab.toolbar.script" => Some("script".into()),
        "lab.toolbar.run" => Some("run".into()),
        "lab.palette.discovery" => Some("discovery".into()),
        _ => None,
    }
}

/// Both answers are about the same settings-form row.
///
/// A control that holds affordances INSIDE it — a list's element rows, a
/// stepper's arrows, a checkbox — legitimately answers with the affordance
/// under the cursor rather than with the row. What must never happen is an
/// answer naming a *different* row, which is what R1651.1's three defects and
/// R1652.1's overlap all did.
fn same_row(want: &str, got: &str) -> bool {
    let Some(key) = want.strip_prefix("field:") else {
        return false;
    };
    ["option.", "step.", "toggle.", "item."]
        .iter()
        .filter_map(|f| got.strip_prefix(f))
        .any(|rest| {
            // `item.listen.endpoints.3` is the fourth element of the row keyed
            // `listen.endpoints`, and `.add` is that row's "one more" button.
            // Those two are the whole vocabulary a list control puts inside
            // itself; anything else after the key is a different row, and
            // "the same row plus anything" would accept a neighbour whose key
            // merely starts the same.
            rest == key
                || rest
                    .strip_prefix(key)
                    .and_then(|tail| tail.strip_prefix('.'))
                    .is_some_and(|tail| {
                        tail == "add"
                            || (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()))
                    })
        })
}

/// The pane a tag's mark has to be painted inside.
fn owning_pane(tag: &str) -> Option<Rect> {
    if tag.starts_with("lab.rail") {
        return Some(rail_rect());
    }
    if tag.starts_with("lab.palette") {
        return Some(palette_rect());
    }
    if tag.starts_with("lab.toolbar") {
        return Some(toolbar_rect());
    }
    if tag.starts_with("lab.inspector") || tag.starts_with("lab.form") {
        return Some(inspector_rect());
    }
    if tag.starts_with("lab.node.")
        || tag.starts_with("lab.frame.")
        || tag.starts_with("lab.link.")
        || tag.starts_with("lab.gate")
        || tag.starts_with("lab.hint")
        || tag.starts_with("lab.pin.")
    {
        return Some(canvas_rect());
    }
    None
}

/// Whether a tag names something drawn INSIDE the canvas viewport, as opposed
/// to chrome around it. Graph content can legitimately be off-screen; chrome
/// cannot.
fn is_graph_content(tag: &str) -> bool {
    tag.starts_with("lab.node.")
        || tag.starts_with("lab.frame.")
        || tag.starts_with("lab.link.")
        || tag.starts_with("lab.pin.")
        || tag.starts_with("lab.gate")
}

/// (1) FORWARD — every element the specification declares is painted.
///
/// ★ R1656 — with SIZE an axis, "painted" has to mean what it always meant on
/// the canvas: the canvas is a viewport onto a world larger than itself, so a
/// node outside it is not missing, it is off-screen, and R1653 already granted
/// that for the panned state. Adding a floor-sized window made the same thing
/// true without a pan — the first run of the size axis reported 28 absences,
/// all of them graph content the small canvas cannot show at once — so the
/// grant is stated by CATEGORY here rather than by state.
///
/// The chrome is held to the stricter rule at every size, and that is the half
/// worth holding: a toolbar, a palette or an inspector that stops being painted
/// because the window got small is a screen that lost a control, not a screen
/// showing part of a diagram.
fn assert_forward(when: &str, state: &LabState, shot: &Painted, size: (u32, u32)) -> usize {
    let want = declared_tags(state);
    // Smaller than the size the graph was laid out for: the viewport shows part
    // of the world. Bigger is not excused — a larger window shows MORE, so a
    // node missing there is missing.
    let graph_fits = size.0 >= WIN_W && size.1 >= WIN_H;
    let absent: Vec<&String> = want
        .iter()
        .filter(|tag| !shot.tags.contains_key(*tag))
        // ★ R1662 — a control the pane has scrolled past is not a control the
        // screen lost: the reader reaches it, and the offset that does is in
        // the report. Excused only when the derivation SAYS so — an element
        // that is neither painted nor reachable is still a failure, which is
        // the half this grant must not swallow.
        .filter(|tag| !shot.reachable.contains_key(*tag))
        .filter(|tag| {
            // Graph content is excused only when the canvas is too small to
            // hold the world — never when it has the room and simply did not
            // paint the node.
            graph_fits || !is_graph_content(tag)
        })
        .collect();
    assert!(
        absent.is_empty(),
        "{when}: the specification declares {} element(s) the screen does not \
         paint: {absent:?}",
        absent.len()
    );
    // How many the check actually demanded — a floor on THIS is what keeps a
    // grant (like the off-screen one above) from quietly emptying the check.
    want.iter()
        .filter(|tag| graph_fits || !is_graph_content(tag))
        .count()
}

/// (3) REACHABLE — every painted control answers for itself when pressed at the
/// centre of the rectangle it was painted in. Returns how many were probed.
fn assert_reachable(when: &str, state: &LabState, shot: &Painted, size: (u32, u32)) -> usize {
    let probes: Vec<(&String, String)> = shot
        .tags
        .iter()
        .filter_map(|(tag, _)| must_answer(tag).map(|want| (tag, want)))
        // ★ R1656 — a control painted past the window's own edge is excused
        // here, and that is the framework's own distinction rather than a new
        // one: `scene/pointer_reach` fails a widget COVERED by another node
        // (one declaration fixes it) and only REPORTS one that is off-window
        // (the repair is a scroll region, which is a layout decision). The
        // floor size is where this bites — the inspector's form grows with the
        // list it shows and the pane does not scroll, so past some length its
        // last chips are below the window whatever the floor is set to.
        // Registered, with the measurement, as
        // [[debt-the-node-lab-panes-do-not-scroll]].
        .filter(|(_, _)| true)
        .filter(|(tag, _)| {
            let r = shot.tags[*tag];
            r.x + r.w <= size.0 && r.y + r.h <= size.1
        })
        .collect();
    // ★ R1662 — the floor counts the controls the screen HAS, on screen or one
    // scroll away, rather than the ones that happen to fit this window. At the
    // declared floor the side panes show a fraction of their content and the
    // rest is reachable; counting only the painted ones would have this floor
    // fall with the window size, which is the shape of a check that empties
    // itself. A control that is neither painted nor reachable is counted by
    // neither term, so the floor still bites when one disappears.
    let scrolled = shot
        .reachable
        .keys()
        .filter(|tag| must_answer(tag).is_some())
        .count();
    assert!(
        probes.len() + scrolled >= 40,
        "{when}: only {} control(s) — {} painted and {scrolled} one scroll \
         away. A screen that stops painting controls must fail here, not \
         report a smaller number",
        probes.len() + scrolled,
        probes.len()
    );
    let mut unreachable = Vec::new();
    for (tag, want) in &probes {
        let rect = shot.tags[*tag];
        let (px, py) = centre(rect);
        // ★★ R1681.3 — the picked link's own chrome legitimately covers what it
        // is drawn over: it is an affordance the person summoned, the reference
        // draws it in the same place, and the seats themselves are probed here
        // too (see `must_answer`), so this excuses nothing it does not replace
        // with something reachable.
        //
        // ★★★ NOT for the chrome's OWN tags, and a counterfactual is what said
        // so: a first draft excused every probe the chrome covered, which
        // includes the chrome, so making the `delete` seat unpressable left
        // this sweep green. An exception that swallows the thing it is
        // exchanged for is not an exception, it is a hole.
        if !tag.starts_with("lab.link.") && super::chrome_covers(state, px, py) {
            continue;
        }
        let got = Hit::at(state, px, py).word(state);
        if &got != want && !same_row(want, &got) {
            unreachable.push((tag, want.clone(), got, rect));
        }
    }
    assert!(
        unreachable.is_empty(),
        "{when}: {} of {} painted control(s) are drawn where a press does not \
         reach them: {unreachable:?}",
        unreachable.len(),
        probes.len()
    );
    probes.len()
}

/// (4) CONTAINED — a mark outside its own pane is a mark the reader sees
/// somewhere its address says it is not.
fn assert_contained(when: &str, shot: &Painted, size: (u32, u32)) -> usize {
    let mut escaped = Vec::new();
    let mut below = 0usize;
    for (tag, rect) in &shot.tags {
        if let Some(pane) = owning_pane(tag)
            && !inside(pane, *rect)
        {
            // ★ R1656 — the SAME distinction `assert_reachable` makes, for the
            // same reason: a mark past the bottom of the window is content the
            // pane has no room for (the repair is a scroll region — see
            // [[debt-the-node-lab-panes-do-not-scroll]]), while a mark outside
            // its pane SIDEWAYS is drawn over the pane next door, which is the
            // defect the size axis found three of on its first run.
            if rect.y >= size.1 || rect.y + rect.h > size.1 {
                below += 1;
                continue;
            }
            escaped.push((tag, *rect, pane));
        }
    }
    assert!(
        escaped.is_empty(),
        "{when}: {} mark(s) are painted outside the pane their address puts \
         them in: {escaped:?}",
        escaped.len()
    );
    below
}

/// (5) DISJOINT — the settings form's controls do not overlap.
fn assert_disjoint(when: &str, shot: &Painted) {
    let controls: Vec<(&String, Rect)> = shot
        .tags
        .iter()
        .filter(|(tag, _)| tag.starts_with("lab.form.control."))
        .map(|(tag, rect)| (tag, *rect))
        .collect();
    let mut collisions = Vec::new();
    for (i, (a_tag, a)) in controls.iter().enumerate() {
        for (b_tag, b) in &controls[i + 1..] {
            if overlaps(*a, *b) {
                collisions.push((a_tag, *a, b_tag, *b));
            }
        }
    }
    assert!(
        collisions.is_empty(),
        "{when}: {} pair(s) of settings-form controls are painted on top of \
         each other: {collisions:?}",
        collisions.len()
    );
}

/// ★ R1656 — a card added from the palette lands where nothing already is.
///
/// The placement was the canvas centre unconditionally, and two cards answering
/// for the same pixels is a screen where a press is a coin toss. Nothing
/// asserted it until a counterfactual disabled the free-spot search and every
/// test stayed green: the swept states add ONE node, and one node on an
/// eight-node graph happens to miss.
#[test]
fn r1656_cards_added_from_the_palette_do_not_cover_each_other() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        for _ in 0..6 {
            super::add_node(&state, Role::Responder);
        }
        let placed: Vec<(String, Rect)> = state
            .cards()
            .into_iter()
            .filter_map(|n| super::card_rect(&state, n).map(|r| (state.name_of(n), r)))
            .collect();
        assert!(
            placed.len() >= 14,
            "six added to the opening graph: {}",
            placed.len()
        );
        let mut covering = Vec::new();
        for (i, (a_name, a)) in placed.iter().enumerate() {
            for (b_name, b) in &placed[i + 1..] {
                if overlaps(*a, *b) {
                    covering.push((a_name.clone(), b_name.clone()));
                }
            }
        }
        assert!(
            covering.is_empty(),
            "{} pair(s) of cards are painted on top of each other: {covering:?}",
            covering.len()
        );
    });
}

/// ★ R1656 — the containment check can FAIL, proven against a scene built to
/// fail it.
///
/// A counterfactual that replaced the ink metric with the run's own box — which
/// is exactly the blindness this whole round is about, since the box is the
/// promise — went unnoticed: the screen is clean, so a check with no power and a
/// check with nothing to find are indistinguishable. A negative control is the
/// difference, and it belongs beside the sweep rather than in the framework's
/// own tests, because what is being proven here is that THIS caller wired the
/// metric to something that measures.
#[test]
fn r1656_the_containment_check_reports_a_scene_built_to_break_it() {
    let card = Rect::new(0, 0, 40, 20);
    let mut inner = pinion_core::scene::ContainerNode::new(vec![Scene::Text(
        pinion_core::scene::TextNode::styled(
            "a string far wider than forty pixels",
            Rect::new(0, 0, 40, 12),
            pinion_core::style::TextStyle::new().with_size_px(11),
        ),
    )]);
    inner.rect = card;
    inner.tag = Some("card".into());
    let scene = Scene::Container(inner);
    let (escapes, _) = ink_escapes(&scene, (WIN_W, WIN_H));
    assert!(
        !escapes.is_empty(),
        "the ink metric this sweep hands in must MEASURE — a stand-in that \
         returns the box back reports every screen as clean, and that is the \
         defect this round exists to make visible"
    );
}

/// ★★★ R1681.3 — what a press over the picked link's chrome reaches is what a
/// person SEES there.
///
/// The two orders have to agree and nothing made them. `Hit::at` resolves the
/// chrome before everything else on the canvas, and the chrome was painted with
/// the wires — before the cards — so a card drawn afterwards covered it while a
/// press over that card answered `link:act`. A screen showing one thing and
/// answering another is this example's oldest defect class, and it was found by
/// LOOKING AT THE RUNNING APP rather than by any check here.
///
/// Asserted as an order over the painted tags rather than by sampling pixels,
/// because "later wins" is the rule the renderer actually follows and a sample
/// only reaches the overlaps that happen to exist today.
#[test]
fn r1681_the_picked_links_chrome_paints_over_what_a_press_over_it_reaches() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let (_, scene) = painted_at(&state, (WIN_W, WIN_H));
        let mut order: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag() {
                order.push(tag.to_owned());
            }
        });
        let last_card = order
            .iter()
            .rposition(|tag| tag.starts_with("lab.node.") && !tag[9..].contains('.'))
            .expect("cards are painted");
        for seat in ["lab.link.label", "lab.link.act"] {
            let at = order
                .iter()
                .position(|tag| tag == seat)
                .unwrap_or_else(|| panic!("{seat} is painted on the opening screen"));
            assert!(
                at > last_card,
                "★ {seat} is painted at {at}, BEFORE the last card at \
                 {last_card} — so a card covers it while a press over that \
                 card answers the seat"
            );
        }
    });
}

/// ★★ R1681.2 — a reported link is drawn in a RHYTHM a drawn one is not.
///
/// The claim R1681 got backwards, which is why this exists. It recorded that
/// this screen's wire primitive "carries no dash pattern" and reached for
/// colour alone — but `Stroke` has carried `dash` since R1575, and the sibling
/// screen that draws these same two layers already spells a reported link
/// `Dash::DOTTED`. A limit invented in place of a substrate reached for, and
/// nothing on this screen could tell the difference. This is what tells.
///
/// Colour alone is not enough for the same reason the sibling gives: the two
/// layers mean different things, and a reader who cannot distinguish a claim
/// about the world from a decision somebody made is reading the wrong diagram.
#[test]
fn r1681_a_reported_link_is_drawn_in_a_rhythm_a_drawn_one_is_not() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let (_, scene) = painted_at(&state, (WIN_W, WIN_H));
        let mut rhythms: Vec<(String, bool)> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Scene::Path(path) = visit.node {
                let Some(tag) = path.tag.as_deref() else {
                    return;
                };
                if tag.starts_with("lab.link.") || tag.starts_with("lab.observed.") {
                    let dashed = path.style.stroke.and_then(|s| s.dash).is_some();
                    rhythms.push((tag.to_owned(), dashed));
                }
            }
        });
        let drawn: Vec<&(String, bool)> = rhythms
            .iter()
            .filter(|(tag, _)| tag.starts_with("lab.link."))
            .collect();
        let reported: Vec<&(String, bool)> = rhythms
            .iter()
            .filter(|(tag, _)| tag.starts_with("lab.observed."))
            .collect();
        assert!(
            !drawn.is_empty() && !reported.is_empty(),
            "both layers are on the opening screen: {rhythms:?}"
        );
        assert!(
            drawn.iter().all(|(_, dashed)| !dashed),
            "a link somebody DREW is solid: {drawn:?}"
        );
        assert!(
            reported.iter().all(|(_, dashed)| *dashed),
            "★ and one a source only REPORTED is not: {reported:?}"
        );
    });
}

/// ★ R1656 — a card's text is part of the diagram, so it shrinks with it.
///
/// Written because a counterfactual that stopped the canvas font scaling was
/// **not caught by anything**: the card's box is derived from its rows, so it
/// simply grew, and at the zoom where a bigger card would have covered its
/// neighbour the level-of-detail rule had already dropped the rows. A property
/// nothing asserts is a property the next round can delete.
#[test]
fn r1656_a_cards_text_shrinks_with_the_diagram() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let face_at = |zoom: u32| {
            state.zoom.set(zoom);
            let (_, scene) = painted_at(&state, (WIN_W, WIN_H));
            let mut smallest = u32::MAX;
            scene.for_each_node(&mut |visit| {
                if let Scene::Text(t) = visit.node {
                    // The cards live in the canvas world, which is the only
                    // subtree that scales; chrome keeps its face.
                    if visit.absolute_rect().is_some() && t.style.font_size_px > 0 {
                        smallest = smallest.min(t.style.font_size_px);
                    }
                }
            });
            smallest
        };
        let big = face_at(200);
        let small = face_at(50);
        assert!(
            small < big,
            "the smallest face on the screen is {small}px at 50% and {big}px at              200% — a card is part of the diagram, not chrome over it, so its              text has to scale with it"
        );
        state.zoom.set(spec::OPENING_ZOOM);
    });
}

/// ★★ R1672 — the ink gate comes from the CRATE now
/// ([`pinion_core::test_fixtures::screen_ink`]), and this screen is one of its
/// three consumers.
///
/// It was written here at R1656 for a property a person had to find by looking:
/// at the size this screen opens in, **seven of its eight node cards painted
/// their last field row three to five pixels below their own border**, and
/// every check in this module was green. Two reasons, and both are why the
/// check is [`pinion_core::containment::escapes`] rather than a rectangle
/// comparison local to this file:
///
/// * [`assert_contained`] next door judges a run against its nearest **tagged
///   ancestor**, and the card's parts were the card's SIBLINGS. The owner it
///   resolved was the canvas, which is big enough to hold anything, so the
///   question was answered honestly about the wrong box.
/// * every rectangle in this module is the box the view *gave* a mark. The card
///   rows were inside their boxes. What left the card was the **ink**, and the
///   ink is not in the scene — it is a measurement.
///
/// What moved at R1672 is the *metric*, because there were three mechanical
/// copies of it — two in this file — and screen B held one without ever running
/// the check, so a counterfactual that put its panes over their panels'
/// outlines was caught by nothing.
///
/// ★ There is no allowance, and that is a finding. The channel learned the
/// border-box / content-box distinction this round and surfaced six escapes
/// here. A first pass closed four and named the other two in an
/// `OUTLINE_ALLOWANCE`; the round then closed those as well — the published
/// part rectangles in `pinion_widget_paint::config_form` (a crate, so every
/// consumer followed) and this screen's own toolbar pill. An exception
/// mechanism holding the empty set is a place for the next escape to be filed
/// instead of fixed, so it is gone.
use pinion_core::test_fixtures::screen_ink::{assert_contained_ink, ink_escapes, stand_in_ink};

/// The one sweep, over every state.
#[test]
fn r1653_the_painted_screen_is_the_specification_in_every_state() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut probed_total = 0;
        let mut richest = 0;
        let mut demanded_min = usize::MAX;
        let mut below_total = 0usize;
        for (when, mutate) in STATES {
            mutate(&state);
            for (how_big, size) in SIZES {
                let label = format!("{when}, {how_big}");
                let (shot, scene) = painted_and_scene(&state, *size);
                let demanded = assert_forward(&label, &state, &shot, *size);
                demanded_min = demanded_min.min(demanded);
                let probed = assert_reachable(&label, &state, &shot, *size);
                probed_total += probed;
                richest = richest.max(probed);
                below_total += assert_contained(&label, &shot, *size);
                assert_disjoint(&label, &shot);
                below_total += assert_contained_ink(&label, &scene, *size);
            }
        }
        // ★ Floors on the SWEEP, not only on each state — because a state that
        // is deleted takes its own assertions with it. A counterfactual that
        // removed the grown-list state left every remaining assertion green,
        // which made that state decoration; these are what it trips.
        assert!(
            STATES.len() >= 8,
            "the sweep visits {} state(s)",
            STATES.len()
        );
        // ★ R1656 — a floor on the SIZE axis for the reason there is one on the
        // state axis: an axis with one entry looks like coverage and is a
        // constant. This one had one entry for 1,656 rounds.
        // ★ R1656 — what the two grants above actually cost, as a number, so a
        // grant cannot quietly grow into a hole. `demanded_min` is the
        // smallest population the forward check still demanded at any size,
        // and `below_total` is how many marks fell past the window bottom
        // across the whole sweep — the registered scroll gap, measured rather
        // than assumed ([[debt-the-node-lab-panes-do-not-scroll]]).
        assert!(
            demanded_min >= 30,
            "the leanest size still demanded {demanded_min} element(s) — a \
             grant that leaves the forward check with almost nothing to prove \
             is a hole, not an exemption"
        );
        assert!(
            below_total <= 40,
            "{below_total} mark(s) fell past the window bottom across the \
             sweep; the panes not scrolling is a registered gap and this is \
             its ratchet"
        );
        assert!(
            SIZES.len() >= 3,
            "the sweep visits {} size(s) — the opening size, a maximised \
             window and the declared floor are three different screens",
            SIZES.len()
        );
        assert!(
            SIZES.iter().any(|(_, (w, _))| *w > WIN_W),
            "one of the sizes has to be BIGGER than the design size, which is \
             the case a person reported"
        );
        assert!(
            richest >= 68,
            "the busiest state painted {richest} control(s) — the screen is at \
             its fullest when a list field has been grown, and a sweep that \
             never grows one has stopped visiting the state R1652.1's defect \
             lived in"
        );
        assert!(
            probed_total >= 510,
            "the sweep probed {probed_total} control(s) across {} states",
            STATES.len()
        );
    });
}

/// (2) BACKWARD — the screen invented nothing.
///
/// Every painted tag has to be accounted for by the specification or by a named
/// structural family, and the families the specification sizes have to be that
/// size. A forward-only check cannot see chrome nobody declared, and a check
/// whose families are open-ended cannot see a family that lost half its
/// members.
#[test]
fn r1653_the_painted_screen_invented_nothing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let shot = painted(&state);
        let declared: BTreeSet<String> = declared_tags(&state).into_iter().collect();

        // family prefix -> how many members the specification fixes it at, or
        // `None` where the count is a function of the live model rather than of
        // the specification.
        let families: &[(&str, Option<usize>)] = &[
            ("lab.rail.", Some(spec::RAIL.len())),
            ("lab.palette.role.", Some(spec::ROLES.len())),
            ("lab.palette.swatch.", Some(spec::ROLES.len())),
            ("lab.palette.pin.", Some(spec::PIN_LEGEND.len())),
            ("lab.palette.protocol.", Some(spec::PROTOCOLS.len())),
            ("lab.link.", None),
            ("lab.gate", None),
            ("lab.node.", None),
            ("lab.pin.", None),
            ("lab.frame.", None),
            ("lab.form.", None),
            ("lab.inspector.", None),
            ("lab.toolbar.", None),
            ("lab.appbar.", None),
            ("lab.hint.", None),
            ("lab.palette.discovery", None),
        ];
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut unaccounted = Vec::new();
        for tag in shot.tags.keys() {
            if declared.contains(tag) {
                if let Some((prefix, _)) = families.iter().find(|(p, _)| tag.starts_with(p)) {
                    *counts.entry(prefix).or_default() += 1;
                }
                continue;
            }
            match families.iter().find(|(prefix, _)| tag.starts_with(prefix)) {
                Some((prefix, _)) => *counts.entry(prefix).or_default() += 1,
                None => unaccounted.push(tag),
            }
        }
        assert!(
            unaccounted.is_empty(),
            "the screen paints {} tag(s) the specification does not declare and \
             no family accounts for: {unaccounted:?}",
            unaccounted.len()
        );
        for (prefix, fixed) in families {
            if let Some(fixed) = fixed {
                assert_eq!(
                    counts.get(prefix).copied().unwrap_or_default(),
                    *fixed,
                    "★ {prefix} is a family the specification sizes — a member \
                     that stops being painted has to fail here, and a count \
                     nobody pins is a count nobody notices"
                );
            }
        }
    });
}

/// ★ R1649's defect class, as a property rather than as a memory: a text run
/// carries no tag, so every tag-keyed assertion is blind to it, and the round
/// that stacked every card's text down the left edge of the window had 118
/// wire assertions passing while it did so.
///
/// The rule: a run is painted inside the thing that owns it. That is checkable
/// only because [`pinion_core::NodeVisit`] hands the ancestor chain and the
/// window-absolute rectangle to the same visitor — the reference toolkits this
/// is judged against cannot answer it at all, because a run inside a control is
/// not an object there and has no geometry to ask for.
#[test]
fn r1653_every_text_run_is_painted_inside_what_owns_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let shot = painted(&state);
        assert!(
            shot.runs.len() > 60,
            "the screen has text: {}",
            shot.runs.len()
        );
        let mut escaped = Vec::new();
        for (content, rect, owner_tag) in &shot.runs {
            let Some(owner_tag) = owner_tag else {
                escaped.push((content.clone(), *rect, "<no tagged ancestor>".to_owned()));
                continue;
            };
            let Some(owner_rect) = shot.tags.get(owner_tag) else {
                continue;
            };
            if !inside(*owner_rect, *rect) {
                escaped.push((content.clone(), *rect, owner_tag.clone()));
            }
        }
        assert!(
            escaped.is_empty(),
            "{} text run(s) are painted outside the node that owns them: {escaped:?}",
            escaped.len()
        );
    });
}

/// ★ R1653 — the canvas is a viewport, and a pan that carries the graph past
/// its edge stops painting it rather than painting it somewhere else.
///
/// Stated as a property because it arrived as a surprise: the first sweep here
/// panned and *crashed*, because the screen added the pan to every rectangle
/// and converted the result back to pane-local coordinates in `u32`. A pan to
/// the left underflowed that subtraction — a panic in a debug build, a
/// coordinate near four billion in a release one — on a gesture the hint strip
/// advertises. The screen also had no clip at all, so a pan the other way
/// painted the graph over the palette and the inspector.
#[test]
fn r1653_a_pan_past_the_edge_stops_painting_the_graph() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let visible = |shot: &Painted| {
            spec::NODES
                .iter()
                .filter(|n| shot.tags.contains_key(&format!("lab.node.{}", n.id)))
                .count()
        };
        assert_eq!(
            visible(&painted(&state)),
            spec::NODES.len(),
            "every declared node is on screen to begin with"
        );

        // Far enough left that the whole graph is past the viewport.
        state.pan.set((-3_000, 0));
        let shot = painted(&state);
        assert_eq!(visible(&shot), 0, "and the pan carried all of it away");
        let escaped: Vec<(&String, Rect)> = shot
            .tags
            .iter()
            .filter(|(tag, _)| tag.starts_with("lab.node.") || tag.starts_with("lab.frame."))
            .map(|(tag, rect)| (tag, *rect))
            .collect();
        assert!(
            escaped.is_empty(),
            "★ nothing was painted outside the canvas instead — before the \
             viewport existed this is where the graph landed on top of the \
             palette: {escaped:?}"
        );

        // And the chrome that floats over the canvas does not pan with it.
        for tag in ["lab.gate", "lab.hint", "lab.toolbar.title"] {
            assert!(shot.tags.contains_key(tag), "{tag} is chrome, not content");
        }

        // Panning back brings every one of them back, at its original place.
        state.pan.set((0, 0));
        assert_eq!(visible(&painted(&state)), spec::NODES.len());
    });
}

/// ★ R1654 — the screen fills the window it is given, at every size.
///
/// Reported from the running window: enlarging it left the content in the
/// top-left corner and the rest of the surface black. The screen had its design
/// size written into every pane rectangle, and — the reason no test here saw
/// it — every check ran the layout pass at that same size, so the assumption
/// and the defect were the same number.
///
/// Three sizes, and the properties are relations rather than pixels: the panes
/// tile the window with no gap and no overlap, the fixed-width ones keep their
/// width, and the canvas takes what is left.
#[test]
fn r1654_the_screen_fills_whatever_window_it_is_given() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        // ★ R1656 — the smallest entry is the DECLARED floor rather than a
        // number picked here. It was `(900, 620)`, which R1656 measured to be
        // below what this screen can paint; the layout clamps at the floor, so
        // the assertion was comparing the window it asked for against the
        // window it got and would fail for a reason that is not a defect.
        for size in [(WIN_W, WIN_H), (1920, 1200), (super::MIN_W, super::MIN_H)] {
            let shot = painted_at(&state, size).0;
            let rail = shot.tags["lab.rail"];
            let palette = shot.tags["lab.palette"];
            let canvas = shot.tags["lab.canvas"];
            let inspector = shot.tags["lab.inspector"];
            let appbar = shot.tags["lab.appbar"];

            assert_eq!(appbar.w, size.0, "{size:?}: the bar spans the window");
            assert_eq!(rail.x, 0, "{size:?}");
            assert_eq!(rail.x + rail.w, palette.x, "{size:?}: no gap at the rail");
            assert_eq!(
                palette.x + palette.w,
                canvas.x,
                "{size:?}: no gap at the palette"
            );
            assert_eq!(
                canvas.x + canvas.w,
                inspector.x,
                "{size:?}: the canvas ends where the inspector starts"
            );
            assert_eq!(
                inspector.x + inspector.w,
                size.0,
                "{size:?}: and the inspector reaches the right edge"
            );
            for (name, pane) in [
                ("rail", rail),
                ("palette", palette),
                ("inspector", inspector),
            ] {
                assert_eq!(
                    pane.y + pane.h,
                    size.1,
                    "{size:?}: {name} reaches the bottom"
                );
            }
            assert_eq!(palette.w, super::PALETTE_W, "{size:?}: fixed width");
            assert_eq!(inspector.w, super::INSP_W, "{size:?}: fixed width");
        }
        // And the canvas is the one that absorbs the difference.
        let narrow = painted_at(&state, (900, 620)).0.tags["lab.canvas"];
        let wide = painted_at(&state, (1920, 1200)).0.tags["lab.canvas"];
        assert!(
            wide.w > narrow.w && wide.h > narrow.h,
            "the canvas takes the room the window gained: {narrow:?} -> {wide:?}"
        );
    });
}

/// ★ R1655 — **every card, in every state, answers a press at the centre of the
/// rectangle it was painted in, and the drag that follows moves THAT card.**
///
/// Reported from the running window: "sometimes a node presses and sometimes it
/// does not". A press that lands on the wrong thing is invisible to every check
/// here that asks about one state or one card — R1653's sweep probes a control
/// per painted TAG, which asks whether the address answers, and this asks the
/// harder question: after the press, did the thing that moved turn out to be
/// the thing under the cursor?
///
/// The population is derived — every card the model holds, at every state the
/// sweep visits — so a card that becomes unpressable in one state cannot hide
/// behind the others.
#[test]
fn r1655_every_card_presses_and_drags_in_every_state() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut checked = 0;
        let mut wrong = Vec::new();
        for (when, mutate) in STATES {
            mutate(&state);
            let shot = painted(&state);
            for node in state.cards() {
                let name = state.name_of(node);
                let Some(rect) = shot.tags.get(&format!("lab.node.{name}")).copied() else {
                    // Panned out of view is a legitimate absence; the clipping
                    // property is asserted elsewhere.
                    continue;
                };
                let (px, py) = centre(rect);
                checked += 1;

                // (1) the press resolves to the card it was painted in.
                let answered = Hit::at(&state, px, py);
                if answered != Hit::Node(node) {
                    wrong.push(format!(
                        "{when}: pressing {name} at its painted centre ({px},{py}) \
                         answered {}",
                        answered.word(&state)
                    ));
                    continue;
                }

                // (2) the drag that follows moves THAT card and no other.
                let before = positions(&state);
                super::move_cursor(&state, px, py);
                super::press(&state);
                super::move_cursor(&state, px + 9, py + 7);
                super::release(&state);
                let after = positions(&state);
                let moved: Vec<&String> = before
                    .iter()
                    .filter(|(k, v)| after.get(*k) != Some(v))
                    .map(|(k, _)| k)
                    .collect();
                if moved != vec![&name] {
                    wrong.push(format!("{when}: dragging {name} moved {moved:?}"));
                }
            }
        }
        assert!(checked >= 60, "the sweep pressed {checked} card(s)");
        assert!(
            wrong.is_empty(),
            "{} of {checked} press-and-drag(s) reached the wrong thing:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
    });
}

/// ★ R1655 — the same question over the WHOLE card, not only its centre.
///
/// "Sometimes a node presses and sometimes it does not" is not a question about
/// the middle of a card: a centre probe answers for one pixel out of a few
/// thousand, and a press lands wherever the hand was. This samples a grid over
/// every card in every state and reports what each point actually resolves to.
#[test]
fn r1655_a_press_anywhere_on_a_card_reaches_that_card() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        let mut sampled = 0;
        for (when, mutate) in STATES {
            mutate(&state);
            let shot = painted(&state);
            for node in state.cards() {
                let name = state.name_of(node);
                let Some(rect) = shot.tags.get(&format!("lab.node.{name}")).copied() else {
                    continue;
                };
                for i in 0..6u32 {
                    for j in 0..6u32 {
                        let px = rect.x + 1 + (rect.w.saturating_sub(2)) * i / 5;
                        let py = rect.y + 1 + (rect.h.saturating_sub(2)) * j / 5;
                        // ★★ R1681.3 — a point the PICKED LINK'S OWN CHROME
                        // covers is not a hole in this card. It is an
                        // affordance the person summoned by picking that link,
                        // drawn where the reference draws it, and a screen that
                        // moved it off the wire to keep this sweep quiet put it
                        // 240px from the wire it annotates — measured on the
                        // running app. The exception is DERIVED from the same
                        // function that paints it, so a chrome that wandered
                        // anywhere else is still caught here.
                        if super::chrome_covers(&state, px, py) {
                            continue;
                        }
                        sampled += 1;
                        let got = Hit::at(&state, px, py);
                        if got == Hit::Node(node) {
                            continue;
                        }
                        let kind = match got {
                            Hit::Pin { node: p, dial } if p == node => {
                                format!("its own {} pin", if dial { "dial" } else { "accept" })
                            }
                            Hit::Pin { node: p, .. } => {
                                format!("ANOTHER node's pin ({})", state.name_of(p))
                            }
                            Hit::Node(other) => {
                                format!("ANOTHER card ({})", state.name_of(other))
                            }
                            other => format!("{} [{when}]", other.word(&state)),
                        };
                        *tally.entry(kind).or_default() += 1;
                    }
                }
            }
        }
        assert!(sampled > 1500, "sampled {sampled} point(s)");
        let stray: Vec<(&String, &usize)> = tally
            .iter()
            .filter(|(k, _)| !k.starts_with("its own"))
            .collect();
        assert!(
            stray.is_empty(),
            "of {sampled} points on a card, these reached something else: {stray:?}"
        );
    });
}

/// ★ R1655 — every tag but the root is pointer-transparent, at every state.
///
/// The R1649.1 class, which is the one that produces "sometimes it presses and
/// sometimes it does not": the §5.35 router resolves a hit target by
/// hit-testing the paint scene for the DEEPEST TAGGED node under the cursor and
/// then looking up an `External` carrying that tag. Every tag here is an
/// address and there is exactly one `External` — the root — so a tagged child
/// that is not transparent makes the lookup fail and the router forwards
/// NOTHING. Wherever that child is painted, the screen is dead to a hand, and
/// everywhere else it works.
///
/// The sibling shell has carried this test since R1649.1 and this screen never
/// had one, which is why a round could add a tagged node and nothing would say.
#[test]
fn r1655_every_tag_but_the_root_is_pointer_transparent() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut opaque = Vec::new();
        let mut tagged = 0;
        for (when, mutate) in STATES {
            mutate(&state);
            let mut scene = super::view((TextFieldState::Idle, 0), Frame::default());
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
            let mut walk = vec![(&scene, true)];
            while let Some((node, is_root)) = walk.pop() {
                if let Some(tag) = node.tag() {
                    tagged += 1;
                    if is_root {
                        assert_eq!(tag, super::VIEW_TAG, "the root carries the External's tag");
                        assert!(
                            !node.is_pointer_transparent(),
                            "the ROOT must stay opaque, or there is no hit target at all"
                        );
                    } else if !node.is_pointer_transparent() {
                        opaque.push(format!("{when}: {tag}"));
                    }
                }
                for child in node.child_nodes() {
                    walk.push((child, false));
                }
            }
        }
        assert!(tagged > 400, "the screen tags plenty to check: {tagged}");
        assert!(
            opaque.is_empty(),
            "{} tagged node(s) are NOT pointer-transparent, so the router \
             resolves them as the hit target, finds no External with that tag, \
             and forwards nothing — the screen is dead to a real mouse wherever \
             they are painted: {opaque:?}",
            opaque.len()
        );
    });
}

/// Every card's world position, by name.
fn positions(state: &LabState) -> BTreeMap<String, (i32, i32)> {
    state
        .cards()
        .into_iter()
        .filter_map(|n| {
            state
                .doc
                .borrow()
                .tree(super::ROOT)
                .and_then(|t| t.node(n).map(|s| (state.name_of(n), (s.x, s.y))))
        })
        .collect()
}

/// ★ R1654 — every run on this screen declares a policy for not fitting.
///
/// The rule that makes "the box is the promise" keepable. A run is given an
/// exact rectangle so it does not flow (R1653), which fixes its WIDTH too — and
/// the strings here are user data: an endpoint, a key expression, a node
/// identifier. Without a shortening policy the ones that outgrow their box wrap
/// to a second line that lands on the row below, which is what two rounds of
/// this screen shipped and what no box-measuring check can see.
///
/// Font-independent on purpose. Asserting a measured width would make this gate
/// depend on the host's font metrics, and a gate that is green on one machine
/// and red on another is the flake this project does not accept.
#[test]
fn r1654_every_painted_run_declares_what_happens_when_it_does_not_fit() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut scene = super::view((TextFieldState::Idle, 0), Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
        let _ = &state;
        let mut unguarded = Vec::new();
        let mut checked = 0;
        scene.for_each_node(&mut |visit| {
            let Scene::Text(text) = visit.node else {
                return;
            };
            checked += 1;
            if !text.style.overflow.shortens() {
                unguarded.push((text.content.clone(), text.style.overflow));
            }
        });
        assert!(checked > 60, "the screen paints text: {checked} run(s)");
        assert!(
            unguarded.is_empty(),
            "{} run(s) have an exact box and no policy for outgrowing it: {unguarded:?}",
            unguarded.len()
        );
    });
}

/// ★ The other half of a run's placement: two runs that share an owner must not
/// be painted on top of each other.
///
/// Containment alone cannot see this, and it is the failure a fixed rectangle
/// invites. Giving a run an exact box — which is what stops it flowing — also
/// fixes its *width*, so a string wider than the box the author guessed wraps to
/// a second line and that line lands on the run below. The result reads as a
/// smear rather than as a missing element, so nothing downstream notices: the
/// tag is present, the rectangle is inside its pane, and the words are
/// illegible.
#[test]
fn r1653_no_two_text_runs_are_painted_on_top_of_each_other() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let shot = painted(&state);
        let mut by_owner: BTreeMap<String, Vec<(&String, Rect)>> = BTreeMap::new();
        for (content, rect, owner_tag) in &shot.runs {
            by_owner
                .entry(owner_tag.clone().unwrap_or_default())
                .or_default()
                .push((content, *rect));
        }
        let mut smeared = Vec::new();
        for (owner_tag, runs) in &by_owner {
            for (i, (a_text, a)) in runs.iter().enumerate() {
                for (b_text, b) in &runs[i + 1..] {
                    if overlaps(*a, *b) {
                        smeared.push((owner_tag, a_text, *a, b_text, *b));
                    }
                }
            }
        }
        assert!(
            smeared.is_empty(),
            "{} pair(s) of text runs are painted over each other: {smeared:?}",
            smeared.len()
        );
    });
}

/// ★★ R1662 — nothing this screen paints is out of the reader's reach.
///
/// The property in one sentence: for every painted mark, either it is on screen
/// or some offset the enclosing viewport can take brings it there. A mark that
/// fails both is `lost` — the reader cannot get to it by any gesture, and until
/// this test existed nothing on this screen could tell that apart from a mark
/// that is merely scrolled away.
///
/// ★ Why this is a different question from the containment sweep beside it.
/// That one asks whether a mark stayed inside the box that owns it, and it
/// **forgives** a mark painted entirely below the window — it partitions those
/// off as "the registered scroll gap". That grant is exactly where the two
/// panes' overflow was hiding: it was not a scroll gap, because neither pane
/// scrolled. The verdict is now derived rather than granted, and the grant's
/// name has to be earned by a pane that really does scroll.
///
/// ★ The population is derived twice over — every state, at every size — and
/// the counts are asserted, so a screen that paints nothing cannot report clean.
#[test]
fn r1662_every_mark_is_shown_or_reachable_in_every_state_and_size() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut marks = 0usize;
        let mut scrollable = 0usize;
        let mut lost: Vec<String> = Vec::new();
        for (when, mutate) in STATES {
            mutate(&state);
            for (how, size) in SIZES {
                let (_, scene) = painted_and_scene(&state, *size);
                let mut counted = 0usize;
                scene.for_each_node(&mut |_| counted += 1);
                marks += counted;
                for out in pinion_core::reach::out_of_sight(&scene, *size, &mut stand_in_ink) {
                    match out.reach {
                        pinion_core::reach::Reach::Scrollable { .. } => scrollable += 1,
                        pinion_core::reach::Reach::Lost { short_by } => lost.push(format!(
                            "{when} {how}: {} is past {} by {short_by:?} \
                             (viewport {}x{}, content {}x{}, range {:?})",
                            out.tag
                                .clone()
                                .or_else(|| out.content.clone())
                                .unwrap_or_else(|| out.path.join("/")),
                            out.viewport.name,
                            out.viewport.size.0,
                            out.viewport.size.1,
                            out.viewport.content.0,
                            out.viewport.content.1,
                            out.viewport.max,
                        )),
                    }
                }
            }
        }
        assert!(
            marks > 1_000,
            "the sweep examined {marks} mark(s) — an empty screen reports clean"
        );
        assert!(
            lost.is_empty(),
            "{} mark(s) no gesture can bring into view (of {marks} examined, \
             {scrollable} merely scrolled away):\n  {}",
            lost.len(),
            lost.iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    });
}

/// ★ R1662 — the panes the specification says scroll, do.
///
/// A companion to the property above rather than a duplicate of it: that one
/// would still pass if a pane stopped scrolling and its content happened to
/// fit. This reads the specification's own column and asks the painted scene
/// whether the body is there and is a scroll node with a range that came from
/// its content.
/// R1669 — this screen's reserved rail seats are DECLARED with their booking,
/// exactly as screen C's are.
///
/// The debt this closes is that two screens of one tool spelled one concept two
/// ways. R1668 gave the framework a channel for *why* a region is inert and
/// screen C adopted it; this rail kept the bare `locked: bool` it had when
/// there was nowhere to put a reason, so its two seats were grey and mute —
/// absent from `scene/disabled`, and announced to a screen reader as ordinary
/// destinations that simply do not respond.
///
/// The assertion is the same one screen C carries, deliberately: a law two
/// screens hold identically is a law, and one screen holding it is a habit.
#[test]
fn r1669_every_reserved_rail_seat_is_declared_with_its_booking() {
    use pinion_core::availability::{Recourse, UnavailableKind};

    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let (_, mut scene) = painted_and_scene(&state, (super::WIN_W, super::WIN_H));
        // The cascade is what the window runs after layout; a sweep that
        // skipped it would ask "is this seat inert" of a scene where nothing
        // had resolved.
        pinion_core::scene_disabled::resolve_disabled(&mut scene);
        let census: std::collections::BTreeMap<String, _> =
            pinion_core::scene_disabled::disabled_census(&scene)
                .into_iter()
                .map(|row| (row.tag.clone(), row))
                .collect();

        let mut reserved = 0;
        for (name, booking) in spec::RAIL {
            let tag = format!("lab.rail.{name}");
            match booking {
                Some(why) => {
                    reserved += 1;
                    let row = census.get(&tag).unwrap_or_else(|| {
                        panic!("the {name} seat is reserved and the screen paints it live")
                    });
                    assert_eq!(row.reason.kind(), UnavailableKind::Reserved);
                    assert_eq!(
                        row.reason.detail(),
                        *why,
                        "the {name} seat reports a booking the specification does not state",
                    );
                    assert_eq!(row.reason.recourse(), Recourse::AwaitRelease);
                }
                None => assert!(
                    !census.contains_key(&tag),
                    "the {name} seat is open and the screen paints it inert",
                ),
            }
        }
        assert_eq!(reserved, 2, "the reference locks two seats on this rail");
        // And nothing ELSE on this screen is inert, which is the direction that
        // catches a region declared unavailable by accident.
        let unexpected: Vec<&String> = census
            .keys()
            .filter(|t| !t.starts_with("lab.rail."))
            .collect();
        assert!(
            unexpected.is_empty(),
            "this screen paints inert regions nobody declared reserved: {unexpected:?}",
        );
    });
}

#[test]
fn r1662_the_panes_the_specification_says_scroll_are_scroll_panes() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        // The floor size is the one that makes the side panes overflow, which
        // is the state the property is about.
        let (_, scene) = painted_and_scene(&state, (super::MIN_W, super::MIN_H));
        let mut found: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Scene::Scroll(node) = visit.node
                && let Some(tag) = node.tag.as_deref()
            {
                found.push(tag.to_owned());
                assert!(
                    node.state.is_some(),
                    "{tag} has no state, so the wheel writes nowhere"
                );
            }
        });
        for pane in spec::PANES {
            let Some(body) = pane.body else { continue };
            assert!(
                found.iter().any(|t| t == body),
                "{} declares a scrolling body {body} and the screen paints no \
                 scroll node with that tag — found {found:?}",
                pane.tag
            );
        }
    });
}

/// ★★ R1662 — the offset the report publishes actually works.
///
/// The property above is a derivation: it says a scroll offset *exists* that
/// brings each mark into view. This drives that offset through the pane's own
/// scroll state, repaints, and then presses the control at the centre it landed
/// in — so the claim is settled by the screen rather than by the arithmetic
/// that made it. A published offset nobody has ever scrolled to is the kind of
/// answer that is right until the day the pane's frame changes.
///
/// Run at the declared floor, because that is the size where the side panes
/// have more content than window.
#[test]
fn r1662_a_control_one_scroll_away_is_pressable_after_that_scroll() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let floor = (super::MIN_W, super::MIN_H);
        let mut probed = 0usize;
        let mut wrong: Vec<String> = Vec::new();
        for (when, mutate) in STATES {
            mutate(&state);
            let (shot, _) = painted_and_scene(&state, floor);
            let away: Vec<(String, String, (i32, i32))> = shot
                .reachable
                .iter()
                .filter(|(tag, _)| must_answer(tag).is_some())
                .map(|(tag, (scroller, to))| (tag.clone(), scroller.clone(), *to))
                .collect();
            for (tag, scroller, to) in away {
                let Some(scroll) = super::pane_scroll(&state, &scroller) else {
                    // A control off the WINDOW rather than off a pane is the
                    // other property's business; it cannot be scrolled to.
                    continue;
                };
                let before = scroll.offset();
                scroll.scroll_to(to.0, to.1);
                let (after, _) = painted_and_scene(&state, floor);
                probed += 1;
                match after.tags.get(&tag) {
                    None => wrong.push(format!(
                        "{when}: scrolling {scroller} to {to:?} did not bring \
                         {tag} onto the screen"
                    )),
                    Some(rect) => {
                        let (px, py) = centre(*rect);
                        let want = must_answer(&tag).expect("filtered above");
                        let got = Hit::at(&state, px, py).word(&state);
                        // The same equivalence the design-size probe uses: a
                        // control that holds affordances inside it answers with
                        // the one under the cursor, and a list control's centre
                        // IS an element chip.
                        if got != want && !same_row(&want, &got) {
                            wrong.push(format!(
                                "{when}: after scrolling {scroller} to {to:?}, \
                                 pressing {tag} at ({px},{py}) answered {got} \
                                 and not {want}"
                            ));
                        }
                    }
                }
                scroll.scroll_to(before.0, before.1);
            }
        }
        assert!(
            probed >= 20,
            "only {probed} control(s) were scrolled to — at the declared floor \
             the side panes hold more than they show, so this must not be small"
        );
        assert!(
            wrong.is_empty(),
            "{} of {probed} published offset(s) did not deliver:\n  {}",
            wrong.len(),
            wrong
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    });
}

// ─────────────────────────────────────────────────────────────────
// R1677 — the operations, which a census of the painted screen cannot see
// ─────────────────────────────────────────────────────────────────

/// The gesture that causes one operation, joined to [`spec::OPERATIONS`] by the
/// specification's own word for it.
///
/// A function rather than a value because a gesture *is* a sequence — put the
/// cursor here, press, travel there, release — and none of that is something
/// the specification table can hold. What the table holds is *whether* one
/// exists; what lives here is the one that does. The gate asserts the two agree
/// in both directions, so a declared gesture with no driver and a driver for an
/// undeclared one are both failures.
///
/// ★ Every driver aims at a rectangle READ OUT OF THE PAINTED SCENE and goes
/// through this screen's own pointer entry points. Two rules, and each has a
/// round behind it: R1653 found three consecutive rounds of defects that every
/// test missed because the tests asked the geometry *helper* where a control
/// was and the helper was right each time, so the aim comes from the paint; and
/// the `panned` swept state records why the press is a press — a state a test
/// reaches by assignment can be one no mouse can produce, and an operation
/// "caused" that way proves nothing about whether a person can cause it.
type OperationDriver = (&'static str, fn(&std::rc::Rc<LabState>, &Painted));

const OPERATION_GESTURES: &[OperationDriver] = &[
    ("add a node", |state, shot| {
        press_tag(state, shot, "lab.palette.role.Responder");
    }),
    ("move a node", |state, shot| {
        drag_tag(state, shot, "lab.node.P-03", (40, 24));
    }),
    ("re-parent a node between frames", |state, shot| {
        // Onto the other host's frame, which is where a drop changes whose
        // machine the node starts on.
        drag_between(state, shot, "lab.node.P-03", "lab.frame.host-b");
    }),
    ("move a frame and its members", |state, shot| {
        drag_tag(state, shot, "lab.frame.host-b.name", (30, 0));
    }),
    // ★★ R1682 — the node's own life. Each picks the card with the pointer
    // first, because the seats act on the SELECTED card and a driver that set
    // the selection directly would be proving the buttons work against a state
    // no mouse can produce — the rule R1681 wrote next door for the link seats.
    ("delete a node", |state, shot| {
        press_tag(state, shot, "lab.node.P-03");
        press_tag(state, &painted(state), "lab.inspector.delete");
    }),
    ("collapse a node", |state, shot| {
        press_tag(state, shot, "lab.node.P-03");
        press_tag(state, &painted(state), "lab.inspector.collapse");
    }),
    ("disable a node", |state, shot| {
        press_tag(state, shot, "lab.node.P-03");
        press_tag(state, &painted(state), "lab.inspector.disable");
    }),
    // ★★★ R1683 — the two the one text field answers. Each opens it with the
    // pointer, types through the framework's own buffer and applies with the
    // same seat, which is the path a PERSON takes; the wire's `edit`/`type`/
    // `apply` trio drives the same three steps.
    ("rename a node", |state, shot| {
        press_tag(state, shot, "lab.node.P-03");
        press_tag(state, &painted(state), "lab.inspector.rename");
        type_into(state, "edge-01");
        press_tag(state, &painted(state), "lab.inspector.rename");
    }),
    ("add a field by typing its key", |state, shot| {
        let _ = shot;
        press_tag(state, &painted(state), "lab.inspector.addkey");
        type_into(state, "transport.unicast.lowlatency");
        press_tag(state, &painted(state), "lab.inspector.rename");
        assert!(
            state.editing.get().is_none(),
            "the apply shut the field, or the operation did not finish"
        );
    }),
    ("add a field from the catalogue", |state, shot| {
        // ★★ R1690 — the key is read off the operation table's own argument,
        // not written here. This driver named a key that the option surface
        // then made precise, and the two failed apart — the R1688 lesson, that
        // a gate holding its own copy of a list measures the screen of the day
        // it was written, in a place small enough to have looked harmless.
        press_tag(state, shot, &format!("lab.form.add.{}", catalogue_key()));
    }),
    ("edit a field", |state, shot| {
        // Growing a list field by one element: an edit a person performs with
        // the pointer alone. NOT the integer stepper, which the gate's first
        // run showed is already at its field's ceiling on the opening screen —
        // a driver that clamps causes nothing and would have read as a defect.
        press_tag(state, shot, "lab.form.item.listen.endpoints.add");
    }),
    // ★★ R1686 — the seat at the trailing edge of a row's key line. This row
    // carried `gesture: false` since R1677 as the table's own record that the
    // wire could take a row out and nothing on the screen could.
    ("remove a field", |state, shot| {
        press_tag(state, shot, "lab.form.remove.control.permissions");
    }),
    // ★★★ R1684 — the launch gate, closed the way a PERSON closes it. The
    // stepper cannot: it clamps at the field's ceiling, which is right, and is
    // why this row read `gesture: false` while the value that closes the gate
    // is one past that ceiling. Pressing the middle of the row's control — left
    // of the stepper, where the value's text is — opens the one field over it,
    // and what is typed is stored as typed and reported on its row.
    ("validate", |state, shot| {
        press_tag(state, shot, "lab.form.control.transport.link.tx.batch_size");
        type_into(state, "70000");
        press_tag(state, &painted(state), "lab.inspector.rename");
    }),
    ("author a link", |state, shot| {
        drag_between(state, shot, "lab.pin.S-01.dial", "lab.pin.P-02.accept");
    }),
    // ★★ R1681 — a link's life. The act seat is painted on the PICKED link, so
    // each of these picks one with the pointer first: pressing the wire is how
    // a person selects it, and a driver that set the selection directly would
    // be proving the button works against a state no mouse can produce.
    ("delete a link", |state, shot| {
        press_wire(state, shot, "Q-01", "R-01");
        press_tag(state, &painted(state), "lab.link.act");
    }),
    ("rewire a link", |state, shot| {
        // Off the accept pin it lands on, onto another node's. Pressing an
        // accept pin PICKS UP the wire that arrived there — the reference's
        // rule and every node editor's — so this is one drag, not a delete
        // followed by a draw.
        let _ = shot;
        drag_between(
            state,
            &painted(state),
            "lab.pin.R-01.accept",
            "lab.pin.P-03.accept",
        );
    }),
    ("select a link endpoint", |state, shot| {
        // The precondition grew the selected node's listen list, so the seats
        // are painted; the second one is the endpoint the link did not take.
        let _ = shot;
        press_tag(state, &painted(state), "lab.link.endpoint.1");
    }),
    ("adopt an observed link", |state, shot| {
        press_wire(state, shot, "P-01", "P-02");
        press_tag(state, &painted(state), "lab.link.act");
    }),
    ("pan", |state, _| {
        let canvas = canvas_rect();
        let from = (canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
        drag_from(state, from, (-30, 20));
    }),
    ("zoom", |state, shot| {
        press_tag(state, shot, "lab.toolbar.zoom.in");
    }),
    ("toggle discovery", |state, shot| {
        press_tag(state, shot, "lab.palette.discovery");
    }),
    // ★ R1678 — the five resets. Four are painted only once their scope has
    // something to put back, which is why every one of them declares a `needs`:
    // the gate causes that first, and the button the driver aims at is painted
    // BECAUSE it did. A driver that found its tag missing would panic here
    // rather than pass quietly, which is the point.
    ("reset the node set", |state, shot| {
        press_tag(state, shot, "lab.reset.nodes");
    }),
    ("reset the layout", |state, shot| {
        press_tag(state, shot, "lab.reset.layout");
    }),
    ("reset the fields", |state, shot| {
        press_tag(state, shot, "lab.reset.fields");
    }),
    ("reset the links", |state, shot| {
        press_tag(state, shot, "lab.reset.links");
    }),
    ("reset the view", |state, shot| {
        press_tag(state, shot, "lab.reset.view");
    }),
    // ★★ R1687 — what leaves the screen, from the two seats the reference puts
    // side by side. Both are unconditional: a graph always has a plan, even an
    // empty one, and an affordance that came and went would make "can I export
    // this" a thing a person has to discover by looking.
    ("export the configuration", |state, shot| {
        press_tag(state, shot, "lab.toolbar.config");
    }),
    ("produce the launch script", |state, shot| {
        press_tag(state, shot, "lab.toolbar.script");
    }),
    // ★★ R1688 — the view's last two. The fit is the pill's trailing seat; the
    // jump is the LAUNCH CHIP, which was on screen saying the verdict and doing
    // nothing until this round. Neither declares a `needs`: the opening graph
    // does not fit the opening zoom, and the first finding is not on the card
    // the screen opens with — both of which are properties of this screen's own
    // specification, asserted in `tests.rs` rather than assumed here.
    ("fit the graph to the view", |state, shot| {
        press_tag(state, shot, "lab.toolbar.fit");
    }),
    ("go to the first problem", |state, shot| {
        press_tag(state, shot, "lab.toolbar.gate");
    }),
];

/// Press the middle of the wire running between two nodes (R1681).
///
/// ★ A wire is a PATH, and a path's painted rectangle is its bounding box —
/// most of the canvas, with its centre nowhere near the stroke. So the aim is
/// still read out of the paint, but from the two things that do have
/// rectangles: the pins the wire runs between. Their midpoint is the point the
/// screen's own chord resolver answers with this link, which is what "a person
/// can press this wire" means.
fn press_wire(state: &std::rc::Rc<LabState>, shot: &Painted, from: &str, to: &str) {
    let seat = |tag: String| -> (u32, u32) {
        centre(
            *shot
                .tags
                .get(&tag)
                .unwrap_or_else(|| panic!("{tag} is painted, so the wire has an end there")),
        )
    };
    let a = seat(format!("lab.pin.{from}.dial"));
    let b = seat(format!("lab.pin.{to}.accept"));
    let at = (u32::midpoint(a.0, b.0), u32::midpoint(a.1, b.1));
    super::move_cursor(state, at.0, at.1);
    super::press(state);
    super::release(state);
}

/// Press and release at the centre of a painted tag.
fn press_tag(state: &std::rc::Rc<LabState>, shot: &Painted, tag: &str) {
    let rect = *shot
        .tags
        .get(tag)
        .unwrap_or_else(|| panic!("{tag} is painted, so a person can aim at it"));
    let at = centre(rect);
    super::move_cursor(state, at.0, at.1);
    super::press(state);
    super::release(state);
}

/// Press at a painted tag's centre and travel by a delta before releasing.
fn drag_tag(state: &std::rc::Rc<LabState>, shot: &Painted, tag: &str, by: (i32, i32)) {
    let rect = *shot
        .tags
        .get(tag)
        .unwrap_or_else(|| panic!("{tag} is painted, so a person can aim at it"));
    drag_from(state, centre(rect), by);
}

/// ★★★ R1683 — put text in the open field, and the LIMIT of what this can
/// drive, stated rather than glossed.
///
/// A character reaches the buffer through `edit_field_keymap`, which forwards
/// to the field's own **external** — and that external is mounted by the SHELL
/// (`WidgetCore::create_extra_externals`), which this harness does not run: it
/// builds a scene by calling the view directly. Measured while wiring this,
/// because the first draft asserted the keystroke path here and got a refusal
/// with the field painted and `editing` open.
///
/// So the split is honest rather than convenient. What is this SCREEN's is
/// driven here with the pointer — the seat opens the field, the seat applies
/// it, the value lands — and the keystroke path is the framework's, driven end
/// to end in `r1683_a_name_is_typed.py` where a real shell has mounted the
/// external. A driver that claimed the keystroke here would be claiming
/// coverage of a path it cannot reach, which is the shape R1682's own passing
/// counterfactual had.
fn type_into(state: &std::rc::Rc<LabState>, text: &str) {
    state.buffer.set_text(text.to_owned());
}

/// Press at one painted tag's centre and release at another's.
fn drag_between(state: &std::rc::Rc<LabState>, shot: &Painted, from: &str, to: &str) {
    let a = centre(
        *shot
            .tags
            .get(from)
            .unwrap_or_else(|| panic!("{from} is painted")),
    );
    let b = centre(
        *shot
            .tags
            .get(to)
            .unwrap_or_else(|| panic!("{to} is painted")),
    );
    super::move_cursor(state, a.0, a.1);
    super::press(state);
    super::move_cursor(state, b.0, b.1);
    super::release(state);
}

/// The whole gesture from a point, by a signed delta.
fn drag_from(state: &std::rc::Rc<LabState>, from: (u32, u32), by: (i32, i32)) {
    super::move_cursor(state, from.0, from.1);
    super::press(state);
    let to = (
        u32::try_from(i64::from(from.0) + i64::from(by.0)).unwrap_or(0),
        u32::try_from(i64::from(from.1) + i64::from(by.1)).unwrap_or(0),
    );
    super::move_cursor(state, to.0, to.1);
    super::release(state);
}

/// ★★ R1678 — bring the screen to the state an operation needs before it can
/// be caused at all.
///
/// Reached the way a person reaches it: by causing the earlier operation the
/// specification names, preferring its GESTURE where it has one. A setup that
/// wrote the state directly would let a reset be "proven" against a state no
/// session can produce — the rule the swept states next door already state, and
/// the reason `needs` names an operation rather than describing a condition.
///
/// Panics rather than skipping on a `needs` this table cannot satisfy: an
/// unreachable precondition would silently stop exercising the operation that
/// declared it, which is a gate quietly covering less than it says.
fn reach_precondition(op: &spec::OperationSpec, state: &std::rc::Rc<LabState>) {
    let Some(earlier) = op.needs else { return };
    let earlier = spec::OPERATIONS
        .iter()
        .find(|o| o.name == earlier)
        .unwrap_or_else(|| {
            panic!(
                "{:?} needs {earlier:?}, which this table does not hold",
                op.name
            )
        });
    if let Some((_, drive)) = OPERATION_GESTURES.iter().find(|(n, _)| *n == earlier.name) {
        let shot = painted(state);
        drive(state, &shot);
        return;
    }
    let (verb, arg) = earlier.verb.unwrap_or_else(|| {
        panic!(
            "{:?} needs {:?}, which has no way in at all",
            op.name, earlier.name
        )
    });
    let mut oracle = super::LabOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    oracle
        .invoke(verb, IntrospectValue::Text(arg.to_owned()))
        .unwrap_or_else(|why| panic!("{:?}'s precondition refused: {why:?}", op.name));
}

/// Read one witness slot through the screen's OWN wire surface.
///
/// Not a second reader written here: the operation gate's whole claim is that
/// driving an operation changes something an agent can observe, and "an agent"
/// means this exact code path. A helper that reached into `LabState` instead
/// would be asserting that the state moved, which is a weaker statement and the
/// one R1653 caught three rounds of defects hiding behind.
fn witness(state: &std::rc::Rc<LabState>, slot: &str) -> String {
    let mut oracle = super::LabOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    match oracle.query(slot) {
        Ok(value) => format!("{value:?}"),
        // A refusal is a reading too — "no node is selected" is a state the
        // witness legitimately passes through, and an operation that moves the
        // screen out of it has changed the answer.
        Err(refusal) => format!("refused: {refusal:?}"),
    }
}

/// ★★★ R1677 — **every way this screen says an operation can be caused causes
/// it**, and the ways it says are the reference's own list.
///
/// This is the check the rest of this module cannot perform. Everything above
/// compares the painted scene with [`spec`], which can only ever find drift in
/// something that is *drawn*. An operation the screen does not answer draws
/// nothing, so a census of the paint is blind to it by construction — and the
/// measurement is that sixteen of the reference's thirty operations were absent
/// while every check here was green.
///
/// Three assertions, and each catches a different way this can go wrong:
///
/// 1. **The two tables agree.** Every operation declaring a gesture has a
///    driver and every driver names a declared operation. A `gesture: true`
///    with nothing behind it is the exact failure mode of the hint strip that
///    advertised a wheel zoom no wheel produced.
/// 2. **A declared way works.** For each operation, from a fresh screen, the
///    witness before and after must DIFFER — separately for the verb and for
///    the gesture, because the defects a person reports on this screen live
///    precisely between those two columns. Every user report this screen has
///    collected had the shape "the wire does it and the pointer does not".
/// 3. **The absent set is exactly the declared one.** An operation the table
///    says is missing that turns out to work fails just as loudly as one that
///    is claimed and does not — a stale declaration is how a gate stops
///    measuring without anybody noticing.
///
/// The count of absent operations is a RATCHET rather than a target: this
/// screen genuinely cannot do sixteen of the thirty today, and a gate that
/// simply failed would have to be switched off. It fails when the number grows,
/// and it fails when the number shrinks without the table being updated, so the
/// only way past it is to move a row.
#[test]
fn r1677_every_declared_way_of_causing_an_operation_causes_it() {
    let owner = Owner::new();
    owner.run(|| {
        let declared: BTreeSet<&str> = spec::OPERATIONS
            .iter()
            .filter(|op| op.gesture)
            .map(|op| op.name)
            .collect();
        let driven: BTreeSet<&str> = OPERATION_GESTURES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            declared, driven,
            "★ the specification and the drivers name different operations — \
             a gesture declared with nothing behind it is what put a wheel on \
             the hint strip that no wheel answers"
        );
        let names: BTreeSet<&str> = spec::OPERATIONS.iter().map(|op| op.name).collect();
        assert_eq!(
            names.len(),
            spec::OPERATIONS.len(),
            "the operations are named uniquely, or the join above is ambiguous"
        );

        let mut inert = Vec::new();
        let mut exercised = 0;

        for op in spec::OPERATIONS {
            if let Some((verb, arg)) = op.verb {
                super::reset_lab_state();
                let state = use_lab_state();
                reach_precondition(op, &state);
                let before = witness(&state, op.witness);
                let mut oracle = super::LabOracle::new();
                oracle.attach(std::rc::Rc::clone(&state));
                let answer = oracle.invoke(verb, IntrospectValue::Text((*arg).to_owned()));
                let after = witness(&state, op.witness);
                exercised += 1;
                if answer.is_err() {
                    inert.push(format!(
                        "{:?}: the wire refused `{verb} {arg}` ({answer:?})",
                        op.name
                    ));
                } else if before == after {
                    inert.push(format!(
                        "{:?}: `{verb} {arg}` was accepted and `{}` did not move",
                        op.name, op.witness
                    ));
                }
            }
            if let Some((_, drive)) = OPERATION_GESTURES.iter().find(|(n, _)| *n == op.name) {
                super::reset_lab_state();
                let state = use_lab_state();
                reach_precondition(op, &state);
                let shot = painted(&state);
                let before = witness(&state, op.witness);
                drive(&state, &shot);
                let after = witness(&state, op.witness);
                exercised += 1;
                if before == after {
                    inert.push(format!(
                        "{:?}: the gesture ran and `{}` did not move — this is \
                         the column a wire-driven test cannot see",
                        op.name, op.witness
                    ));
                }
            }
        }

        assert!(
            inert.is_empty(),
            "{} of {exercised} declared way(s) of causing an operation caused \
             nothing:\n  {}",
            inert.len(),
            inert.join("\n  ")
        );

        // The ratchet, printed whether it fires or not: the number is the
        // measurement this gate exists to keep honest.
        let absent: Vec<&str> = spec::OPERATIONS
            .iter()
            .filter(|op| op.verb.is_none() && !op.gesture)
            .map(|op| op.name)
            .collect();
        assert_eq!(
            absent.len(),
            ABSENT_OPERATIONS,
            "★ this screen answers {} of the reference's {} operations and the \
             ratchet says {}. Growing is a regression; shrinking means the \
             table moved and this number has to move with it:\n  {}",
            spec::OPERATIONS.len() - absent.len(),
            spec::OPERATIONS.len(),
            spec::OPERATIONS.len() - ABSENT_OPERATIONS,
            absent.join("\n  ")
        );
    });
}

/// Run one of this screen's own actions, or say why it refused.
fn act(state: &std::rc::Rc<LabState>, verb: &str, arg: &str) -> Result<String, String> {
    let mut oracle = super::LabOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    oracle
        .invoke(verb, IntrospectValue::Text(arg.to_owned()))
        .map(|value| format!("{value:?}"))
        .map_err(|why| format!("{why:?}"))
}

/// ★★★★★ R1689 — **what a save carries is declared, and what it carries comes
/// back.**
///
/// The reference publishes four self-censuses and this is the fourth: a
/// partition of its own state into carried and deliberately volatile, with
/// whatever falls outside reported. `spec::OPERATIONS` mirrors the first of the
/// four one-for-one and R1688 finished it; this is the one that had no mirror,
/// and the cluster it belongs to — saving, opening, importing, clearing — is
/// **not on the reference's own operation list at all**. A census taken over a
/// declared list is complete against that list and blind to everything the list
/// leaves out, which is R1688's finding one level up.
///
/// Three assertions, and the third is where this beats the meter it copies:
///
/// 1. **The partition covers exactly what the operations move**, in both
///    directions. A slot an operation moves that nobody classified is a save
///    with a hole in it; a classification for a slot nothing moves is a rule
///    about a fact that does not exist. Counting one way lets the first hide.
/// 2. **A carried slot comes back.** Drive the operation, take the archive,
///    open it on a screen that has just started, and the slot must read what it
///    read after the operation. The reference's meter asks only whether the key
///    is *classified* — a key can be listed as carried and still not return.
/// 3. **A volatile slot does NOT come back.** The same run asserts the other
///    half, so "we deliberately do not keep this" is checked rather than
///    asserted. Without it, classifying everything as volatile would pass.
///
/// ★★ And the discriminating set is checked too: an operation whose witness
/// reads the same after it as on a screen that has just started proves nothing
/// about a round trip, and five of the thirty are exactly that by nature (a
/// reset puts its scope back to how it opened). So each row is still round
/// tripped — a wrong one still fails — and the gate additionally requires every
/// classified slot to have at least one operation that genuinely moves it away
/// from the opening reading. Otherwise this whole check could pass while
/// proving nothing, which is the failure R1681.1 and R1644.1 both wrote down.
#[test]
fn r1689_what_a_save_carries_is_declared_and_comes_back() {
    let owner = Owner::new();
    owner.run(|| {
        let moved: BTreeSet<&str> = spec::OPERATIONS.iter().map(|op| op.witness).collect();
        let classified: BTreeSet<&str> = spec::KEPT.iter().map(|k| k.witness).collect();
        assert_eq!(
            moved, classified,
            "★ every slot an operation moves is classified, and nothing else is"
        );
        assert_eq!(
            classified.len(),
            spec::KEPT.len(),
            "each slot is classified once"
        );

        let mut broken = Vec::new();
        let mut discriminating: BTreeSet<&str> = BTreeSet::new();

        for op in spec::OPERATIONS {
            let Some((_, drive)) = OPERATION_GESTURES.iter().find(|(n, _)| *n == op.name) else {
                continue;
            };
            let keeps = spec::KEPT
                .iter()
                .find(|k| k.witness == op.witness)
                .expect("the partition covers every witness, asserted above");

            super::reset_lab_state();
            let state = use_lab_state();
            let opening = witness(&state, op.witness);
            reach_precondition(op, &state);
            let shot = painted(&state);
            drive(&state, &shot);
            let after = witness(&state, op.witness);
            if after != opening {
                discriminating.insert(op.witness);
            }
            act(&state, "save_graph", "")
                .unwrap_or_else(|why| panic!("{:?}: the save refused: {why}", op.name));
            assert!(
                witness(&state, "stored").len() > 2,
                "{:?}: the save wrote nothing to read back",
                op.name
            );

            // ★★★★★ **The two halves have to be asked on DIFFERENT screens, and
            // finding that out cost a counterfactual.** The first draft asked
            // both on a screen that had just started, and for a volatile slot
            // that cannot fail: the slot is at its opening value there because
            // nothing ever set it, so "the load did not bring it back" is true
            // whatever the load does. Deleting the line that clears the
            // artifacts passed the whole gate. The volatile claim is about a
            // screen that HAS the value — opening a graph must leave it at the
            // opening reading rather than carrying a stale one — so it is asked
            // where the value is.
            act(&state, "open_graph", "").unwrap_or_else(|why| {
                panic!(
                    "{:?}: opening the save on the same screen refused: {why}",
                    op.name
                )
            });
            let same = witness(&state, op.witness);

            super::reset_lab_state();
            let fresh = use_lab_state();
            act(&fresh, "open_graph", "")
                .unwrap_or_else(|why| panic!("{:?}: opening the save refused: {why}", op.name));
            let restored = witness(&fresh, op.witness);

            match keeps.keeps {
                spec::Keeps::Saved if restored != after => broken.push(format!(
                    "{:?}: `{}` is declared SAVED ({}) and came back as {restored} \
                     instead of {after}",
                    op.name, op.witness, keeps.why
                )),
                spec::Keeps::Volatile if same != opening => broken.push(format!(
                    "{:?}: `{}` is declared VOLATILE ({}) and a load left it at \
                     {same} instead of the opening {opening}",
                    op.name, op.witness, keeps.why
                )),
                _ => {}
            }
        }

        assert!(
            broken.is_empty(),
            "{} slot(s) did not survive a save the way this screen declares:\n  {}",
            broken.len(),
            broken.join("\n  ")
        );
        assert_eq!(
            discriminating, classified,
            "★ every classified slot needs at least ONE operation that moves it \
             away from the opening reading — a round trip whose two readings are \
             equal to begin with cannot tell a save that works from one that \
             does nothing"
        );
    });
}

/// ★★★ R1689 — **the reasons an archive refuses are four, and they are four
/// different sentences on this screen too.**
///
/// The value of the substrate's reading is only real if the screen passes it
/// on. A screen that caught every refusal and said "could not open" would have
/// the `bool` back with extra steps.
#[test]
fn r1689_the_screen_says_which_of_four_things_stopped_a_load() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let good = super::persist::graph_text(&state);
        assert!(
            good.contains("\"revision\": 1") && good.contains("\"Router\""),
            "the screen writes its own graph out, revision and taxonomy included"
        );
        let untouched = witness(&state, "nodes");

        let mut whys = Vec::new();
        for (what, text) in [
            ("nothing saved", String::new()),
            ("not a saved graph", "definitely not one".to_owned()),
            (
                "another revision",
                good.replace("\"revision\": 1", "\"revision\": 77"),
            ),
            (
                "a role this build does not have",
                good.replace("\"Router\"", "\"Wormhole\""),
            ),
        ] {
            let why = match act(&state, "open_graph", &text) {
                Ok(said) => panic!("{what} was ACCEPTED: {said}"),
                Err(why) => why,
            };
            whys.push((what, why));
        }
        assert_eq!(
            witness(&state, "nodes"),
            untouched,
            "and every refusal left the graph alone"
        );
        let distinct: BTreeSet<&String> = whys.iter().map(|(_, why)| why).collect();
        assert_eq!(
            distinct.len(),
            whys.len(),
            "★ four refusals, four sentences — this is the `bool` this round \
             replaced, seen from the screen: {whys:?}"
        );
    });
}

/// ★★ R1682 — a node's-life seat answers a press aimed where it is PAINTED,
/// with the pane scrolled.
///
/// The inspector's body scrolls, so a seat whose window rectangle forgot the
/// offset would be right only at zero — R1662's defect in a new place. The
/// rectangle here comes from the layout pass, and the question goes to the hit
/// test, so the two derivations have to meet.
///
/// ★★★ **It is in this module for a reason worth writing down.** The first
/// draft of this check lived next door in `tests.rs`, where the only way to get
/// a rectangle was to call `node_act_seat` — the function under test — and aim
/// the press at its centre. A counterfactual that removed the scroll offset
/// from that function passed the whole suite, because both sides of the
/// assertion moved together. An assertion that compares a function with itself
/// reads exactly like coverage. R1681.1 wrote the same sentence one round ago.
#[test]
fn r1682_a_node_life_seat_is_pressable_where_it_is_painted() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let seats = ["collapse", "disable", "delete"];
        // The FLOOR size, so the inspector's body genuinely overflows and the
        // offset the layout keeps is one it could reach. Forcing `set_max`
        // instead does not survive: `scroll_pane` measures its own content on
        // every pass and clamps an offset the content cannot justify, so the
        // paint used the offset, the state was put back to zero, and the two
        // rectangles disagreed for a reason that was the fixture's and not the
        // screen's. Measured while writing this.
        let floor = (super::MIN_W, super::MIN_H);
        let mut scrolled = 0;

        for want in [0, 40] {
            painted_at(&state, floor);
            let room = state.inspector_scroll.max().1;
            let to = want.min(room);
            state.inspector_scroll.scroll_to(0, to);
            let (shot, _) = painted_at(&state, floor);
            let live = state.inspector_scroll.offset().1;
            scrolled = scrolled.max(live);
            for seat in seats {
                let tag = format!("lab.inspector.{seat}");
                let rect = *shot
                    .tags
                    .get(&tag)
                    .unwrap_or_else(|| panic!("{tag} is painted at offset {live}"));
                let (px, py) = centre(rect);
                assert_eq!(
                    super::Hit::at(&state, px, py).word(&state),
                    must_answer(&tag).expect("declared"),
                    "★ pressing {tag} at the centre of where the LAYOUT put it \
                     ({rect:?}), with the pane at offset {live}"
                );
            }
        }
        assert!(
            scrolled > 0,
            "★ the pane never actually moved, so this only ever checked the \
             unscrolled case — which is the case a missing offset term passes"
        );
    });
}

/// ★★★ R1683 — the screen and the painter hold ONE buffer, on a later frame.
///
/// This is the question the debt that opened this round said to answer with a
/// test before designing anything. `use_text_edit_state` resolves through
/// `Owner::current().cache(tag, ..)`; the screen takes its reference when it is
/// built and the painter takes one on every frame, and those are the same
/// object only while the owner outlives the frame. If it were ever per-frame
/// the two would diverge silently — the field would paint one buffer and every
/// commit would read another — so it is asserted rather than assumed.
#[test]
fn r1683_the_screen_and_the_painter_hold_one_buffer() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        state.buffer.set_text("typed".to_owned());

        for pass in 0..3 {
            painted(&state);
            assert!(
                std::rc::Rc::ptr_eq(&state.buffer, &use_text_edit_state(super::EDIT_TAG)),
                "★ pass {pass}: the screen's buffer and the painter's are the \
                 same object"
            );
            assert_eq!(
                state.buffer.text(),
                "typed",
                "and painting does not reset what it holds"
            );
        }
    });
}

/// How many of the reference's operations this screen cannot do at all.
///
/// A measurement, not a target — see the gate above for why it is a ratchet.
/// It moves DOWN when a row of [`spec::OPERATIONS`] gains a verb or a gesture,
/// and the gate refuses either direction of drift.
///
/// ★ **Eighteen, and the hand comparison that preceded this gate said sixteen.**
/// Worth keeping the discrepancy visible rather than quietly adopting one
/// number: that comparison was a three-way reading — answered, partly answered,
/// absent — and "partly" is a judgement. This is binary and mechanical: an
/// operation is absent when there is no verb AND no gesture, which is a
/// property of the table a machine settles. Two rows the reading called partly
/// answered have neither way in at all — the configuration export says how many
/// keys there are without exporting them, and the launch script does not exist
/// — so a prose judgement had been carrying them as half-present. The gate
/// disagreeing with the reading that motivated it is the gate doing its job.
///
/// ★★ R1682 took it from nine to five: a node's own life — delete, rename,
/// collapse, disable — was absent as a cluster, exactly as a link's second half
/// was before R1681. Three of the four gained both columns; the rename gained
/// only its verb, because it needs a name TYPED and this screen has no text
/// entry anywhere. That is one axis and not one omission — the same absence
/// answers for the form's text rows and for "add a field by typing its key" —
/// so it is registered as an axis rather than bolted on here for one caller.
/// ★★★ R1683 took it from five to four, and what moved was an AXIS: this screen
/// had no text entry anywhere, so every operation needing a value typed was
/// pointer-unreachable together. One field — the framework's own — with a
/// target answers the rename's gesture and the key row at once, and gives the
/// form's text rows somewhere to go next.
///
/// ★★★★★ **R1688 takes it to zero, which is the first time this number has
/// been able to be a claim about the whole table rather than a countdown.** The
/// last two were the view's: framing the graph, and going to the first thing
/// wrong with it. Neither was hard and both had been absent for eleven rounds,
/// which is the argument for the column existing — a census of what is ON the
/// screen is blind by construction to what the screen cannot DO, and these two
/// paint nothing when they are missing.
///
/// ★★ Zero is not the end of the axis. The population is the reference's own
/// declaration of what a graph editor must do, and that declaration is one
/// tool's; the gate that matters from here is the one above this line, which
/// asks that every declared way of causing an operation actually causes it.
const ABSENT_OPERATIONS: usize = 0;

/// ★★★ R1679 — **the affordance is painted exactly when pressing it would do
/// something**, judged by DOING it rather than by asking the predicate.
///
/// This closes the hole R1678's own counterfactual found. `changed_scopes`
/// decides which reset buttons are painted, and `declared_tags` reads the same
/// function to decide which the specification demands — so a version of it that
/// always answered yes moved the paint and the specification together and all
/// thirty-six checks here passed. Only the demo caught it, from outside, and a
/// demo is CI-only.
///
/// The repair is not a second copy of the predicate. It is to stop consulting
/// the predicate at all: in each swept state, for each gated scope, take the
/// witness, apply the reset, take it again — and assert the button was painted
/// **iff** the value moved. A `changed_scopes` that lies in either direction
/// fails here, and nothing in this test knows what it answers.
///
/// Both directions are real defects. A button painted over a scope with nothing
/// to put back does nothing when pressed, which is a control that lies; a scope
/// with something to put back and no button strands the change behind a wire
/// call no person can make.
#[test]
fn r1679_a_reset_affordance_is_painted_exactly_when_it_would_do_something() {
    let owner = Owner::new();
    owner.run(|| {
        let gated: Vec<super::ResetScope> = super::ResetScope::ALL
            .into_iter()
            .filter(|scope| scope.gated())
            .collect();
        assert!(
            !gated.is_empty(),
            "no scope is conditional, so this test asserts nothing"
        );
        let mut checked = 0;
        let mut wrong = Vec::new();

        for (when, mutate) in STATES {
            for scope in &gated {
                // Each scope is judged on its OWN screen: applying one reset
                // changes what the next has to put back, and a state where the
                // earlier resets already ran is not the state being described.
                super::reset_lab_state();
                let state = use_lab_state();
                mutate(&state);
                let shot = painted(&state);
                let tag = format!("lab.reset.{}", scope.wire());
                let is_painted = shot.tags.contains_key(&tag);

                let reads = scope_witness(*scope);
                let before = witness(&state, reads);
                scope.apply(&state);
                let moved = witness(&state, reads) != before;
                checked += 1;

                if is_painted != moved {
                    wrong.push(format!(
                        "{when}: `{}` is {} and pressing it would {}",
                        scope.wire(),
                        if is_painted { "painted" } else { "absent" },
                        if moved {
                            "change the screen"
                        } else {
                            "do nothing"
                        },
                    ));
                }
            }
        }

        assert_eq!(
            checked,
            STATES.len() * gated.len(),
            "every scope is judged in every swept state"
        );
        assert!(
            wrong.is_empty(),
            "{} of {checked} reset affordance(s) disagree with what pressing \
             them would do:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
    });
}

/// ★★★ R1684 — **pressing the middle of a control does something**, for every
/// control the settings form paints, in every swept state.
///
/// This is the check that was missing when a person reported "the text box does
/// nothing". The sweep already asked, at
/// `r1651_every_control_the_screen_paints_is_hit_at_the_centre_it_paints_in`,
/// whether a press RESOLVES to the control it landed on — and it did: the hit
/// test answered `field:<key>`, the wire agreed, and the release handler ended
/// in `_ => {}`. Resolution is not action. The question this asks is the next
/// one and the only one a person can feel: **and then what changed?**
///
/// Three details, each of which the round it came from paid for:
///
/// * The aim is the rectangle the PAINT gave the control, never
///   [`crate::inspector_geometry`] — R1653's three rounds of defects all hid
///   behind a test that asked the geometry helper where a control was.
/// * The screen is rebuilt for every control, because one press changes what
///   the next one would do — a stepper moves a value the row below derives from
///   and an editor opened over one row covers another.
/// * "Something changed" is read through the screen's own wire over BOTH the
///   form and the field, because a press on a text row's control is supposed to
///   open the editor and a press on a switch is supposed to flip it. Asking
///   only about the form would call the first one dead.
///
/// The population is every `lab.form.control.*` tag in the painted scene, so a
/// row this test does not know about cannot escape it.
#[test]
fn r1684_the_centre_of_every_control_answers_a_press() {
    let owner = Owner::new();
    owner.run(|| {
        let mut dead = Vec::new();
        let mut checked = 0;

        for (when, mutate) in STATES {
            super::reset_lab_state();
            let survey = use_lab_state();
            mutate(&survey);
            let controls: Vec<String> = painted(&survey)
                .tags
                .keys()
                .filter_map(|tag| tag.strip_prefix("lab.form.control.").map(str::to_owned))
                .collect();
            assert!(
                !controls.is_empty(),
                "{when}: the settings form paints no control at all"
            );

            for key in controls {
                super::reset_lab_state();
                let state = use_lab_state();
                mutate(&state);
                // Re-read the rectangle on this screen rather than trusting the
                // survey's: the two are built the same way, and asserting they
                // agree is not this test's job.
                let shot = painted(&state);
                let Some(rect) = shot.tags.get(&format!("lab.form.control.{key}")).copied() else {
                    dead.push(format!(
                        "{when}: {key} was painted on the survey and not on its own screen"
                    ));
                    continue;
                };
                let at = centre(rect);
                let before = (witness(&state, "form"), witness(&state, "editing"));
                press_at(&state, at);
                let after = (witness(&state, "form"), witness(&state, "editing"));
                checked += 1;
                if before == after {
                    dead.push(format!(
                        "{when}: pressing the middle of {key}'s control ({rect:?}, \
                         which the screen resolves to {:?}) changed nothing",
                        Hit::at(&state, at.0, at.1).word(&state)
                    ));
                }
            }
        }

        assert!(
            dead.is_empty(),
            "{} of {checked} control(s) do nothing when pressed in the middle — \
             the shape of the defect a person reported as \"the text box does \
             not do anything\":\n  {}",
            dead.len(),
            dead.join("\n  ")
        );
    });
}

/// ★★★ R1684 — **the field is painted on the thing it is editing, holding what
/// that thing holds, and applying puts it there.**
///
/// The gate above says a press changes something. This says it changes the
/// RIGHT something, and it exists because the three ways this can go wrong are
/// all silent: a field painted at the head of the pane while the press landed
/// halfway down it looks like a screen that ignored the press; a field seeded
/// from the wrong row looks like a value that changed by itself; a commit that
/// writes the row instead of the element quietly eats the neighbours.
///
/// Every rectangle is read out of the painted scene — the field's own container
/// tag and the form painter's — so this compares two paints rather than a
/// helper with itself.
#[test]
fn r1684_the_field_stands_on_the_row_it_edits() {
    let owner = Owner::new();
    owner.run(|| {
        let mut wrong = Vec::new();
        let mut opened = 0;

        for (when, mutate) in STATES {
            super::reset_lab_state();
            let survey = use_lab_state();
            mutate(&survey);
            let seats: Vec<(String, String)> = painted(&survey)
                .tags
                .keys()
                .filter_map(|tag| {
                    // A row's own control, and — for a list — each element's
                    // row, which is a target in its own right.
                    tag.strip_prefix("lab.form.control.")
                        .map(|key| (key.to_owned(), tag.clone()))
                })
                .chain(painted(&survey).tags.keys().filter_map(|tag| {
                    let part = tag.strip_prefix("lab.form.item.")?;
                    let (key, n) = part.rsplit_once('.')?;
                    n.parse::<usize>().ok()?;
                    Some((key.to_owned(), tag.clone()))
                }))
                .collect();

            for (key, tag) in seats {
                super::reset_lab_state();
                let state = use_lab_state();
                mutate(&state);
                let shot = painted(&state);
                let Some(aim) = shot.tags.get(&tag).copied() else {
                    continue;
                };
                let at = centre(aim);
                press_at(&state, at);
                let Some(target) = state.editing.get() else {
                    // A switch flips instead of opening, which the gate above
                    // has already established changes something.
                    continue;
                };
                opened += 1;

                // 1. Where it is: over the seat the field SAYS it opened on,
                //    which is not always the tag that was aimed at — the
                //    centre of a list's control is one of its element rows, and
                //    the field standing on that element is the right answer.
                //    Derived from the target rather than from the aim, so a
                //    field that opened on the wrong thing cannot pass by being
                //    painted where the press was.
                let after = painted(&state);
                let want_tag = seat_tag(&target.wire());
                let box_rect = after.tags.get(super::EDIT_TAG).copied();
                let seat = after.tags.get(&want_tag).copied();
                // ★★★ And the seat it opened on is the one that was UNDER THE
                // CURSOR. Without this the check is self-comparing: it asks
                // where the field is and derives where it should be from the
                // field's own answer, so a press on element three that opened
                // element zero passes — measured, by a counterfactual that did
                // exactly that and was not caught.
                if seat.is_some_and(|seat| {
                    !(at.0 >= seat.x
                        && at.0 < seat.x + seat.w
                        && at.1 >= seat.y
                        && at.1 < seat.y + seat.h)
                }) {
                    wrong.push(format!(
                        "{when}: pressing {tag} at {at:?} opened {want_tag}, \
                         which is at {seat:?} — that is not what was under the \
                         cursor"
                    ));
                }
                match (box_rect, seat) {
                    (Some(field), Some(seat)) => {
                        // Pinned to the seat's top-left and its width; a field
                        // is one line tall, so a seat taller than a line (a
                        // whole list) is allowed to be taller than the box.
                        if (field.x, field.y, field.w) != (seat.x, seat.y, seat.w)
                            || field.h > seat.h
                        {
                            wrong.push(format!(
                                "{when}: pressing {tag} opened the field on \
                                 {want_tag} at {field:?} and that seat is at \
                                 {seat:?} — a person types where they pressed"
                            ));
                        }
                    }
                    (field, seat) => wrong.push(format!(
                        "{when}: pressing {tag} opened the field on {want_tag}; \
                         the field is at {field:?} and the seat at {seat:?}"
                    )),
                }

                // 2 and 3: what the box holds when it opens, and what applying
                // it does — including that an element's neighbours do not move.
                wrong.extend(
                    seed_and_commit_faults(&state, &target.wire(), &key)
                        .into_iter()
                        .map(|fault| format!("{when}: over {tag}, {fault}")),
                );
            }
        }

        assert!(opened > 0, "no row on this screen opened the field at all");
        assert!(
            wrong.is_empty(),
            "{} fault(s) over {opened} opened field(s):\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
    });
}

/// ★★★ R1684 — **leaving the field applies what is in it**, whether the next
/// press is another row or another card.
///
/// Found by LOOKING at the running screen rather than by any check here: a
/// value typed into one row and abandoned by pressing the next row vanished
/// without a word. The house style is commit-on-blur — R1683 mounted the
/// blur-committing external for exactly this — and it cannot fire, because
/// that external never sees a press. So the screen does it, and this is what
/// says so.
#[test]
fn r1684_leaving_the_field_applies_what_is_in_it() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let press_centre = |tag: &str| press_centre_of(&state, tag);
        let row_value = |key: &str| {
            super::selected_form(&state)
                .and_then(|form| form.field(key).map(|f| f.value().to_owned()))
                .unwrap_or_default()
        };
        press_centre("lab.form.control.id");
        state.buffer.set_text("typed-and-left".to_owned());
        press_centre("lab.form.control.transport.link.tx.batch_size");
        assert_eq!(
            row_value("id"),
            "typed-and-left",
            "★ pressing another row APPLIED what was in the box — the house \
             style is commit-on-blur, and the field's own external never sees \
             the press that would trigger it"
        );
        assert_eq!(
            state.editing.get().map(|what| what.wire()).as_deref(),
            Some("value:transport.link.tx.batch_size"),
            "and the field moved to the row that was pressed"
        );

        // ★★★ A REFUSED commit refuses the move. Without this the switch would
        // destroy the very thing the refusal was about — the person typed a
        // name that is already taken, and pressing anywhere else would throw it
        // away rather than let them fix it.
        super::reset_lab_state();
        let state = use_lab_state();
        press_centre_of(&state, "lab.inspector.rename");
        assert_eq!(
            state.editing.get().map(|what| what.wire()).as_deref(),
            Some("name"),
            "the seat opened the box on the card's name"
        );
        state.buffer.set_text("S-01".to_owned());
        press_centre_of(&state, "lab.form.control.id");
        assert_eq!(
            state.editing.get().map(|what| what.wire()).as_deref(),
            Some("name"),
            "★ the box did not move — a name another card holds was refused"
        );
        assert_eq!(
            state.buffer.text(),
            "S-01",
            "and it still holds what was typed, to be edited rather than retyped"
        );
        assert!(
            state.toast.get().contains("already"),
            "the refusal reached the toast: {:?}",
            state.toast.get()
        );
    });
}

/// ★★★ R1684 — **picking another card shuts the field.**
///
/// Written because the counterfactual that removed this passed the whole
/// suite: the field is placed over the INSPECTED card's row, so a selection
/// that moved without shutting it would leave a box standing on a form that is
/// no longer underneath it — and the commit would still land on the card the
/// box was opened over, correctly, which is what makes the wrongness quiet.
#[test]
fn r1684_picking_another_card_shuts_the_field() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let shot = painted(&state);
        let row = *shot
            .tags
            .get("lab.form.control.id")
            .expect("the opening card has a text row");
        press_at(&state, centre(row));
        assert!(
            state.editing.get().is_some(),
            "the row opened the field, or this asserts nothing"
        );
        let elsewhere = *painted(&state)
            .tags
            .get("lab.node.S-01")
            .expect("another card is on the canvas");
        press_at(&state, centre(elsewhere));
        assert_eq!(
            state.selected.get(),
            state.node_of("S-01"),
            "the press picked the other card"
        );
        assert!(
            state.editing.get().is_none(),
            "★ and the field shut with it — it was standing over a row of a \
             form that is not on the screen any more"
        );
    });
}

/// ★★★★★ R1686 — **taking a row away shuts the field that was standing on it**,
/// and does not apply what was in it.
///
/// Written because the round's counterfactual PASSED: breaking the shut left
/// every Rust gate green, and only the demo noticed. That is the recorded
/// pattern — a passed counterfactual is a finding — and the gap was this
/// screen's, not the harness's: `editing` is a plain signal and needs no
/// external to observe, so there was no reason for the question to live only in
/// a subprocess.
///
/// Both halves matter and they pull in opposite directions. R1684's rule is
/// that leaving the field APPLIES what is in it, so a press somewhere else must
/// not silently discard a value; but the one place that rule cannot hold is the
/// row being taken away, because applying to a row about to vanish writes to
/// nothing. The reference tool draws the same line — it drops the edit in the
/// same act that hides the row.
#[test]
fn r1686_taking_a_row_away_shuts_the_field_standing_on_it() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let key = "id";
        let opened_as = super::selected_form(&state)
            .and_then(|form| form.field(key).map(|f| f.value().to_owned()))
            .expect("the opening card has that row");

        let row = *painted(&state)
            .tags
            .get(&format!("lab.form.control.{key}"))
            .expect("the row is painted");
        press_at(&state, centre(row));
        assert!(
            state.editing.get().is_some(),
            "the row opened the field, or this asserts nothing"
        );
        state.buffer.set_text("something else".to_owned());

        let seat = *painted(&state)
            .tags
            .get(&format!("lab.form.remove.{key}"))
            .expect("and the row offers to be taken away");
        press_at(&state, centre(seat));

        assert!(
            super::selected_form(&state).is_some_and(|form| form.field(key).is_none()),
            "the seat took the row away"
        );
        assert!(
            state.editing.get().is_none(),
            "★★ and the field shut with it — a box standing over a row that is \
             gone is a box aimed at nothing, and its target names that row"
        );
        // Put the row back and read what came with it: the half-typed text must
        // NOT have been applied on the way out.
        let mut forms = state.forms.borrow_mut();
        if let Some(form) = forms.get_mut(&state.selected.get().expect("a card")) {
            form.add(key).expect("the catalogue holds an opening row");
        }
        drop(forms);
        assert_eq!(
            super::selected_form(&state)
                .and_then(|form| form.field(key).map(|f| f.value().to_owned())),
            Some(opened_as),
            "★ what was in the field was dropped, not written to the row on \
             its way out"
        );
    });
}

/// Put the cursor at a point and click there — the three events a click is.
///
/// ★ R1686 — lifted, not invented: the round's own third-consumer grep found
/// **six** copies of this triple with nothing between them, which is well past
/// the threshold at which a mechanical repeat becomes a helper. A drag is not
/// one of them and stays written out, because its move happens BETWEEN the
/// press and the release and that ordering is the thing under test.
fn press_at(state: &std::rc::Rc<super::LabState>, at: (u32, u32)) {
    super::move_cursor(state, at.0, at.1);
    super::press(state);
    super::release(state);
}

/// ★★★ R1684 — **the caret a click lands on is the caret the field paints**,
/// round-tripped byte by byte.
///
/// The click-to-caret hook resolves a point to a byte by RE-SHAPING the text,
/// so it and the painter have to agree about the shaping — same font size, same
/// padding, same width. They do because both go through `edit_field_style`, and
/// this is what makes that load-bearing rather than tidy: a hit test shaped at a
/// different size answers a byte a character or two away from the glyph under
/// the cursor, which feels like a field that ignores where you clicked.
///
/// Font-independent by construction: it does not assert *which* byte an x
/// resolves to — that depends on the host's fonts — but that the forward map
/// (byte to caret rectangle, which is what the screen draws) and the backward
/// map (point to byte, which is what a press uses) are inverses. Two different
/// stylings cannot both be, whatever fonts are installed.
#[test]
fn r1684_a_click_resolves_to_the_byte_whose_caret_is_under_it() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        // A row with something long enough in it that the bytes are spread
        // across the box rather than piled at its left edge.
        super::set_and_sync(&state, "listen.endpoints", "tcp/0.0.0.0:7447");
        let shot = painted(&state);
        let at = centre(
            *shot
                .tags
                .get("lab.form.control.listen.endpoints")
                .expect("the row is painted"),
        );
        press_at(&state, at);
        assert!(state.editing.get().is_some(), "the press opened the field");

        let scene = painted_at(&state, (WIN_W, WIN_H)).1;
        let rect =
            pinion_shell::rect_for_tag(&scene, super::EDIT_TAG).expect("the open field is painted");
        let theme = pinion_core::theme::use_theme(super::THEME_TAG).theme_animated();
        let style = super::edit_field_style(rect);
        let text = state.buffer.text();
        assert!(text.len() > 4, "the buffer holds the row's value: {text:?}");

        let mut wrong = Vec::new();
        for byte in 0..=text.len() {
            if !text.is_char_boundary(byte) {
                continue;
            }
            let caret = pinion_widget_paint::text_field::ime_caret_rect_for(
                super::EDIT_TAG,
                TextFieldState::Focused,
                u32::try_from(byte).unwrap_or(0),
                rect,
                &theme,
                &style,
            );
            // Aim just right of the caret's own stem, inside the glyph that
            // follows it, and ask the press path which byte that is.
            #[allow(clippy::cast_precision_loss, reason = "small viewport coords")]
            let (x, y) = (caret.x + caret.width / 2.0, caret.y + caret.height / 2.0);
            let back =
                super::field_byte_at(TextFieldState::Focused, &scene, Some(super::EDIT_TAG), x, y);
            if back != Some(byte) {
                wrong.push(format!(
                    "byte {byte} is painted with its caret at ({x}, {y}) and a \
                     press there resolves to {back:?}"
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} byte(s) do not round-trip between the caret the field \
             PAINTS and the byte a press RESOLVES — the two are shaped \
             differently:\n  {}",
            wrong.len(),
            text.len() + 1,
            wrong.join("\n  ")
        );
    });
}

/// ★★ R1684 — **the wire opens the field on a row it read back, and refuses
/// every spelling that names nothing.**
///
/// The target grammar (`value:<key>`, `value:<key>[<n>]`) is what an agent
/// reads out of `editing.target` and hands back to `edit`, so the two spellings
/// have to be one. And each refusal is a separate arm — a key the form does not
/// hold, an element the row does not have, an index that is not a number, a
/// bracket that never shuts — because a grammar that accepted any of them would
/// open a box over nothing and answer as though it had worked.
///
/// Local rather than only in the demo: a wire refusal needs no window, and the
/// counterfactual that removed the element check was caught by nothing here.
#[test]
fn r1684_the_wire_opens_the_row_it_names_and_refuses_the_rest() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let mut oracle = super::LabOracle::new();
        oracle.attach(std::rc::Rc::clone(&state));
        let mut edit = |what: &str| oracle.invoke("edit", IntrospectValue::Text(what.to_owned()));

        assert!(edit("value:listen.endpoints").is_ok(), "a row it holds");
        assert_eq!(
            state.editing.get().map(|what| what.wire()).as_deref(),
            Some("value:listen.endpoints"),
            "★ and it reads back in the spelling it was asked for"
        );
        assert!(
            edit("value:listen.endpoints[0]").is_ok(),
            "an element it holds"
        );
        assert_eq!(
            state.editing.get().map(|what| what.wire()).as_deref(),
            Some("value:listen.endpoints[0]"),
        );

        for (spelled, expected) in [
            ("value:not.a.row", "is not a row"),
            ("value:listen.endpoints[99]", "has no element"),
            ("value:listen.endpoints[x]", "not an element number"),
            ("value:listen.endpoints[0", "never shuts it"),
            ("neither", "is not a thing this screen edits"),
        ] {
            let refusal = edit(spelled).expect_err(&format!("{spelled:?} names nothing"));
            let said = format!("{refusal:?}");
            assert!(
                said.contains(expected),
                "{spelled:?} was refused as {said} and should say {expected:?}"
            );
        }
    });
}

/// Press the middle of a painted tag, aiming from the paint on the frame the
/// press is made on.
fn press_centre_of(state: &std::rc::Rc<LabState>, tag: &str) {
    let rect = *painted(state)
        .tags
        .get(tag)
        .unwrap_or_else(|| panic!("{tag} is painted, so a person can aim at it"));
    let (px, py) = centre(rect);
    super::move_cursor(state, px, py);
    super::press(state);
    super::release(state);
}

/// Which element of a list row a wire target names, or `None` for the row's own
/// value.
fn element_of(target: &str) -> Option<usize> {
    target
        .rsplit_once('[')?
        .1
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// What the open field HOLDS and what applying it DOES, as a list of faults
/// (R1684).
///
/// Split out of the sweep because the two questions are about the value rather
/// than about the geometry, and because the sweep that asks them for every row
/// in every state is otherwise one function doing three things.
///
/// Applies a probe value rather than the one already there: a commit that
/// wrote nothing would be indistinguishable from a commit that wrote the same
/// thing back.
fn seed_and_commit_faults(state: &std::rc::Rc<LabState>, target: &str, key: &str) -> Vec<String> {
    use pinion_core::widgets::config_form::FieldType;
    const PROBE: &str = "tcp/0.0.0.0:7999";

    let value = |state: &std::rc::Rc<LabState>| -> String {
        super::selected_form(state)
            .and_then(|form| form.field(key).map(|f| f.value().to_owned()))
            .unwrap_or_default()
    };
    let mut faults = Vec::new();
    let held = value(state);
    let element = element_of(target);
    let want = match element {
        Some(n) => FieldType::elements(&held)
            .nth(n)
            .map(str::to_owned)
            .unwrap_or_default(),
        None => held.clone(),
    };
    if state.buffer.text() != want {
        faults.push(format!(
            "the box holds {:?} and the thing it is editing holds {want:?}",
            state.buffer.text()
        ));
    }

    let before: Vec<String> = FieldType::elements(&held).map(str::to_owned).collect();
    state.buffer.set_text(PROBE.to_owned());
    super::commit_edit(state).ok();
    let now = value(state);
    let landed: Vec<String> = FieldType::elements(&now).map(str::to_owned).collect();
    match element {
        Some(n) => {
            if landed.get(n).map(String::as_str) != Some(PROBE) {
                faults.push(format!(
                    "applying did not put the text in element {n}: {landed:?}"
                ));
            }
            let others = |list: &[String]| -> Vec<String> {
                list.iter()
                    .enumerate()
                    .filter(|(at, _)| *at != n)
                    .map(|(_, e)| e.clone())
                    .collect()
            };
            if others(&landed) != others(&before) {
                faults.push(format!(
                    "applying moved its NEIGHBOURS: {:?} became {:?}",
                    others(&before),
                    others(&landed)
                ));
            }
        }
        None => {
            if now != PROBE {
                faults.push(format!("applying left {key} holding {now:?}"));
            }
        }
    }
    faults
}

/// ★★★ R1684.3 — **the hit test answers where the paint put things when it is
/// asked from OUTSIDE an owner scope**, which is the only place production ever
/// asks it.
///
/// Reported by a person: maximise the window and the settings rows stop
/// selecting. Measured through the wire: after a resize the paint puts a form
/// control at x≈2339 and `point` refuses it as "outside the 1440x900 window".
/// `window_size()` reads the live viewport through a hook that only answers
/// inside an owner scope, and every pointer handler and every wire action on
/// this screen runs outside one — so they were laying out against the DESIGN
/// size while the paint used the live one.
///
/// ★★ **The whole size axis of the sweep next door was void for the hit test,
/// and this is why it could not see this.** Those checks run inside
/// `owner.run(...)`, so both the paint and the hit test resolve the hook and
/// agree by construction — the fixture makes the two facts one, which is the
/// exact shape R1682's passing counterfactual had. So this test deliberately
/// leaves the scope before it presses, and that is the whole point of it.
#[test]
fn r1684_3_a_press_lands_where_the_paint_put_it_after_a_resize() {
    let owner = Owner::new();
    // Paint at a maximised size INSIDE the scope, then ANNOUNCE it the way the
    // shell does — `announce_external_sizes` calls `External::on_resize` with
    // the tag's painted rectangle after every paint (R1656). Announcing it here
    // rather than having the paint record it on the way past is what makes this
    // drive the framework's contract instead of a side effect.
    let (state, shot) = owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let shot = painted_at(&state, SIZES[1].1).0;
        pinion_core::external::record_surface_size(super::VIEW_TAG, SIZES[1].1.0, SIZES[1].1.1);
        (state, shot)
    });

    // ★ And ask from outside it, the way a pointer handler and the wire do.
    assert!(
        Owner::current().is_none(),
        "this test's whole claim is that the question is asked with no scope"
    );
    let mut wrong = Vec::new();
    let mut probed = 0;
    for (tag, rect) in &shot.tags {
        let Some(want) = must_answer(tag) else {
            continue;
        };
        let (px, py) = centre(*rect);
        probed += 1;
        let word = Hit::at(&state, px, py).word(&state);
        // A centre may land on something smaller painted over it — a list's
        // first element row sits in the middle of its control. Excused only
        // when that something is itself painted at the point, which is asked of
        // the scene rather than assumed (the rule the corner check states).
        let nested = word != want
            && shot.tags.iter().any(|(other, other_rect)| {
                must_answer(other).as_deref() == Some(word.as_str())
                    && px >= other_rect.x
                    && px < other_rect.x + other_rect.w
                    && py >= other_rect.y
                    && py < other_rect.y + other_rect.h
            });
        if word != want && !nested {
            wrong.push(format!(
                "{tag} is painted at {rect:?} and a press at its centre \
                 ({px},{py}) answers {word:?}, not {want:?}"
            ));
        }
    }

    assert!(probed > 0, "nothing pressable was painted");
    assert!(
        wrong.is_empty(),
        "{} of {probed} control(s) are unreachable once the window has been \
         resized — the paint reflowed and the hit test did not:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// The tag of the seat a field target names, in the form painter's vocabulary.
///
/// The inverse of the target grammar the wire reads back — `value:<key>` is a
/// row's control and `value:<key>[<n>]` is one of a list's element rows — so a
/// check can ask the paint where the thing being edited is without knowing how
/// the screen lays a form out.
fn seat_tag(target: &str) -> String {
    let named = target.strip_prefix("value:").unwrap_or(target);
    match named.split_once('[') {
        Some((key, rest)) => format!("lab.form.item.{key}.{}", rest.trim_end_matches(']')),
        None => format!("lab.form.control.{named}"),
    }
}

/// ★★★ R1684 — **a control answers at its EDGES, not only in the middle.**
///
/// Written because of what unifying the inspector's two translations exposed:
/// the form's window geometry had been short by the panel's frame — one pixel,
/// up and left — since the pane learned to scroll, and nothing could see it.
/// Nothing could see it because every check aims at a centre, and a centre
/// absorbs any error smaller than half the control.
///
/// So this aims at the corners. A press one pixel inside a painted control must
/// still be answered by the row that control belongs to; with the paint and the
/// hit test derived from different arithmetic, the corner is the first place
/// they disagree.
#[test]
fn r1684_a_control_answers_at_its_edges_not_only_its_middle() {
    let owner = Owner::new();
    owner.run(|| {
        let mut wrong = Vec::new();
        let mut probed = 0;

        for (when, mutate) in STATES {
            super::reset_lab_state();
            let state = use_lab_state();
            mutate(&state);
            let shot = painted(&state);
            // ★ The population is every pressable tag the screen declares, not
            // one family. The defect this was written for was found in the
            // settings form; nothing makes that pane special, and a gate that
            // only watches where the last defect was is a gate that finds the
            // last defect.
            let demanded: Vec<(&String, Rect, String)> = shot
                .tags
                .iter()
                .filter_map(|(tag, rect)| must_answer(tag).map(|want| (tag, *rect, want)))
                .filter(|(_, rect, _)| rect.w > 0 && rect.h > 0)
                .collect();
            assert!(!demanded.is_empty(), "{when}: nothing pressable is painted");

            for (tag, rect, want) in &demanded {
                for (corner, (px, py)) in [
                    ("top left", (rect.x, rect.y)),
                    ("top right", (rect.x + rect.w - 1, rect.y)),
                    ("bottom left", (rect.x, rect.y + rect.h - 1)),
                    ("bottom right", (rect.x + rect.w - 1, rect.y + rect.h - 1)),
                ] {
                    probed += 1;
                    let word = Hit::at(&state, px, py).word(&state);
                    if word == *want {
                        continue;
                    }
                    // ★★ A corner may legitimately land inside a SMALLER
                    // affordance painted over this one — a stepper at the end
                    // of a number row, a pin on the edge of a card, the picked
                    // link's chrome. That is only an excuse when the thing
                    // answering is itself painted THERE, which is asked of the
                    // scene rather than assumed: an answer naming something
                    // that is somewhere else is exactly the drift this looks
                    // for.
                    let nested = demanded.iter().any(|(other, other_rect, other_want)| {
                        other != tag
                            && *other_want == word
                            && px >= other_rect.x
                            && px < other_rect.x + other_rect.w
                            && py >= other_rect.y
                            && py < other_rect.y + other_rect.h
                    });
                    if !nested {
                        wrong.push(format!(
                            "{when}: the {corner} of {tag} ({rect:?}) is answered \
                             as {word:?}, and nothing painted there answers that"
                        ));
                    }
                }
            }
        }

        assert!(probed > 0, "nothing was probed");
        assert!(
            wrong.is_empty(),
            "{} of {probed} corner(s) are answered by something that is not \
             painted there — the shape a paint/hit-test drift takes before it \
             is big enough to see:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
    });
}

/// ★★★★★ R1690 — **the launch gate's panel is bounded by the canvas, and says
/// what it has no room for.**
///
/// Its height was a function of how many problems the graph has, which is
/// unbounded: enough of them placed the box above the top of the canvas, where
/// the pane-local conversion underflowed and the screen panicked. It had been
/// one bad graph away for the affordance's whole life, and what hid it is that
/// nothing on this screen could produce many problems at once — until the
/// identifier's declared shape was enforced and three of the opening graph's
/// own values turned out to be unparseable. Three extra lines were enough.
///
/// Driven at the floor window size rather than the design size, because that is
/// where the bound is reachable with a graph a test can build: a demo in a
/// full-size window would need thirty-five problems to get there, and one that
/// only checked the design size is what let this sit.
///
/// The **counted** half is the load-bearing one. A panel that stopped at the
/// edge and dropped the rest would be worse than the crash: the launch verdict
/// is derived from all of the problems, so a reader would see a gate closed for
/// reasons the screen is not showing.
#[test]
fn r1690_the_gate_panel_is_bounded_by_the_canvas_and_counts_what_it_hides() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        // Every card holding a value its own shape refuses, which is the
        // cheapest way to make many problems at once.
        for node in state.cards() {
            state.selected.set(Some(node));
            super::set_and_sync(&state, "id", "zz");
        }
        let problems = state.gate_lines().len();
        assert!(problems >= 8, "the fixture has to make many: {problems}");

        let floor = (super::MIN_W, super::MIN_H);
        let (shot, _) = painted_and_scene(&state, floor);
        let canvas = *shot.tags.get("lab.gate").expect("the panel is painted");
        let pane = super::canvas_rect();
        assert!(
            canvas.y >= pane.y && canvas.y + canvas.h <= pane.y + pane.h,
            "the panel stands inside the canvas with {problems} problem(s): \
             {canvas:?} vs {pane:?}",
        );

        // What it could not show is counted rather than dropped.
        let (shown, hidden) = super::gate_shown(&state);
        assert_eq!(
            shown.len() + hidden,
            problems,
            "every problem is either shown or counted",
        );
        assert!(
            hidden > 0,
            "★ the fixture has to REACH the bound, or this test is about a \
             panel that happened to fit: {problems} problem(s), {} shown at \
             {floor:?}",
            shown.len(),
        );
        let said = shot
            .said
            .get("lab.gate.more")
            .expect("the panel says how many it is not showing");
        assert!(
            said.contains(&hidden.to_string()),
            "and says the number: {said:?}",
        );

        // ★ The counter-assertion, and the one that keeps this from passing on
        // a screen that simply refuses to grow: with FEW problems the panel
        // shows all of them and says nothing about hiding any.
        super::reset_lab_state();
        let state = use_lab_state();
        let (few, none) = super::gate_shown(&state);
        assert_eq!(none, 0, "the opening graph fits");
        assert_eq!(few.len(), state.gate_lines().len());
        assert!(
            !painted(&state).tags.contains_key("lab.gate.more"),
            "and the panel does not claim to be hiding anything",
        );
    });
}

/// The catalogue key the operation table drives "add a field" with.
///
/// One place, read by the wire driver and by the gesture driver both, so the
/// two cannot come to disagree about which chip the operation is about.
fn catalogue_key() -> &'static str {
    spec::OPERATIONS
        .iter()
        .find(|op| op.name == "add a field from the catalogue")
        .and_then(|op| op.verb)
        .map(|(_, key)| key)
        .expect("the operation table declares how a field is added")
}

/// What the run tagged `tag` reads on the painted screen.
fn run_under(shot: &Painted, tag: &str) -> String {
    shot.said
        .get(tag)
        .cloned()
        .unwrap_or_else(|| panic!("{tag} is a painted run"))
}

/// ★★★★★ R1690 — **the reach meter, on the painted screen and on the wire.**
///
/// The reference tool publishes four self-censuses. Two of them this screen
/// already mirrors — the operation table and the save partition — and these are
/// the other two: how much of the option surface the palette can author, and
/// how much of that surface's string half has a shape.
///
/// The gate is an integration one on purpose. A meter is trivially right in a
/// unit test and wrong on screen in the two ways this file exists to catch: the
/// number can be painted from a different computation than the one the wire
/// answers, and the pill can be painted somewhere nobody can see. So this drives
/// the real pipeline, reads the run's own text out of the laid-out scene, and
/// holds it against the wire slot the same screen publishes.
///
/// # Where the teeth are
///
/// Green here would be worth little if the number could not move, so the
/// assertions are chosen to be falsifiable rather than merely true:
///
/// * the painted label EQUALS the wire's two fractions — a rendering against
///   its source, so a second computation behind the pill fails,
/// * the surface is strictly larger than the palette, so the fractions are not
///   `n/n` and a coverage regression has somewhere to fall to,
/// * the meter is painted with **nothing selected**, which is the state a
///   palette fact has to survive and the one a selection-derived meter dies in,
/// * `sound` is true, which is this round's regression gate: it went false the
///   moment the identifier was offered as free text, and the substrate's own
///   test proves that column can fire.
#[test]
fn r1690_the_reach_meter_says_the_same_thing_on_screen_and_on_the_wire() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();

        let wire = witness(&state, "reach");
        let strings = witness(&state, "strings");
        let painted_text = run_under(&painted(&state), "lab.inspector.reach.text");

        // The two fractions the wire answers, in the spelling the pill shows.
        let fields = json_field(&wire, "sections");
        let leaves = json_field(&wire, "leaves");
        let pinned = json_field(&strings, "pinned");
        assert_eq!(
            painted_text,
            format!("sections {fields} · leaves {leaves} · strings {pinned}"),
            "the pill is a rendering of the wire's numbers, not a second count",
        );

        // The surface is bigger than the palette, so the meter has room to
        // report a loss. A `n/n` meter is a decoration.
        let (hit, total) = split_fraction(&leaves);
        assert!(
            hit < total,
            "leaves {leaves}: a surface the palette already covers whole cannot \
             show a regression",
        );
        assert!(
            hit > 0,
            "leaves {leaves}: nor can one it does not reach at all"
        );
        let (roots_hit, roots_total) = split_fraction(&fields);
        assert!(roots_hit < roots_total && roots_hit > 0, "fields {fields}");

        // ★ The string half uses all three classes, read off the wire rather
        // than off the table, so a screen that published a summary of a
        // different census would fail here.
        for class in ["choices", "formats", "free"] {
            assert!(
                strings.contains(&format!("\\\"{class}\\\":[\\\"")),
                "the string census publishes a non-empty {class}: {strings}",
            );
        }

        // ★★★ This round's regression gate, and it covers all three ways a
        // palette can be unsound: a key offered at a shape the target refuses
        // (what this screen did with its node identifier), a key the surface
        // does not declare, and a key naming a SECTION — which three of this
        // screen's own chips did, each composing a string where a subtree
        // belongs, with every coverage count reading them as covered.
        assert!(
            wire.contains("\\\"sound\\\":true"),
            "every chip names a leaf, at the shape the surface declares: {wire}",
        );

        // ★★ And it is painted with nothing selected, because it is a fact
        // about the tool. A meter derived from the selection would vanish here.
        state.selected.set(None);
        let empty = painted(&state);
        assert!(
            empty.tags.contains_key("lab.inspector.reach"),
            "the meter stands with no card selected",
        );
        assert_eq!(
            run_under(&empty, "lab.inspector.reach.text"),
            painted_text,
            "and says the same thing, because nothing about it is the card's",
        );
    });
}

/// ★★★★★ R1690 — **the number falls on its own when the palette narrows.**
///
/// The property a coverage meter is worth having only if it has, and the one
/// that cannot be checked by reading the code: the figure is recomputed from
/// the catalogue every time it is asked, so dropping a chip lowers it with
/// nobody editing anything.
///
/// Driven through the screen's own remove-a-field path rather than by building
/// a narrower form, so what is being measured is the tool a person is using.
/// The interesting half is the direction it does NOT move: taking a row off the
/// screen leaves the reach alone, because the chip offers it back — reach is a
/// property of the palette, and a meter that fell when somebody tidied their
/// inspector would be reporting the session.
#[test]
fn r1690_reach_follows_the_palette_and_not_the_screen() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let before = run_under(&painted(&state), "lab.inspector.reach.text");

        // Take a row out through the affordance a person presses.
        press_centre_of(&state, "lab.form.remove.transport.link.tx.batch_size");
        assert!(
            super::selected_form(&state)
                .is_some_and(|form| form.field("transport.link.tx.batch_size").is_none()),
            "the row really left the screen",
        );
        assert_eq!(
            run_under(&painted(&state), "lab.inspector.reach.text"),
            before,
            "★ and the reach did not move: the chip offers the key back, so the \
             tool can still author it",
        );

        // The other direction, with the same fixture the meter reads: a
        // catalogue that never held the key reports one leaf and one section
        // fewer, with nothing recording a score anywhere.
        let full = super::palette_reach();
        let narrowed = {
            let forms: Vec<_> = Role::ALL
                .iter()
                .map(|role| super::form_for(spec::SELECTED_NODE, *role))
                .collect();
            let catalogue: Vec<_> = forms
                .iter()
                .flat_map(|form| form.fields().iter().chain(form.addable()))
                .filter(|field| field.key() != "qos.priority")
                .map(|field| (field.key(), field.shape()))
                .collect();
            crate::settings::reach(&catalogue)
        };
        assert_eq!(narrowed.leaf_hit(), full.leaf_hit() - 1);
        assert_eq!(
            narrowed.root_hit(),
            full.root_hit() - 1,
            "and the section it was the only leaf of goes too",
        );
        assert!(
            narrowed.leaves_missing.iter().any(|p| p == "qos.priority"),
            "the report names it: {:?}",
            narrowed.leaves_missing,
        );
    });
}

/// The value of a string field of a `Text(..)`-wrapped JSON slot.
///
/// The witness spelling is `Text("{...}")` with the quotes escaped, which is
/// what an agent reading the wire sees; parsing it back out here rather than
/// reaching into the state keeps this gate on the same path an agent is on.
fn json_field(witness: &str, key: &str) -> String {
    let needle = format!("\\\"{key}\\\":\\\"");
    let start = witness
        .find(&needle)
        .unwrap_or_else(|| panic!("{witness} carries {key}"))
        + needle.len();
    let rest = &witness[start..];
    let end = rest.find('\\').unwrap_or(rest.len());
    rest[..end].to_owned()
}

/// `"3/7"` as `(3, 7)`.
fn split_fraction(text: &str) -> (u32, u32) {
    let (hit, total) = text
        .split_once('/')
        .unwrap_or_else(|| panic!("{text} is a fraction"));
    (
        hit.parse().expect("a count"),
        total.parse().expect("a count"),
    )
}

// ── R1691: what a reader is told this screen has ────────────────────────────

/// The voice census of the screen as it is painted right now.
///
/// ★ Built from the SAME two producers the window uses — `view` + layout for the
/// scene, `NodeLabView::access_node` for the tree — so this asks the question of
/// what a screen reader would actually receive, not of a model of it.
fn voice_of(state: &std::rc::Rc<LabState>, size: (u32, u32)) -> pinion_core::voice::VoiceCensus {
    use pinion_a11y::WidgetA11y;

    let (_, scene) = painted_and_scene(state, size);
    let mut nodes = super::NodeLabView::access_node(&(TextFieldState::Idle, 0), None);
    // ★★★★★ R1692 — the shell's own enrichment, run here for the first time.
    // A widget's `access_node` leaves the name `None`; the name a reader hears
    // is resolved from the PAINT SCENE after layout, on WAI-ARIA 1.2's own
    // name-computation precedence. Until this gate judged names it did not, and
    // reading the tree without this step asks a question about a tree nobody
    // receives — every node would be nameless by construction.
    let derived = pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
    // ★ And it finds nothing to do, which is a claim rather than an accident:
    // this screen names every node it announces, at the site that builds it. A
    // counterfactual is why this line is asserted instead of assumed — deleting
    // the enrichment left every gate green, because there was nothing for it to
    // fill. If that ever stops being true the name a reader hears starts coming
    // from the paint scene, and this gate would have gone on judging names the
    // production path resolves differently.
    assert_eq!(
        derived, 0,
        "the screen left {derived} node(s) to be named from the paint scene",
    );
    // ★ The framework's own derivations, not second ones. What counts as a
    // reference, and what an announcement is, are rules the wire and this gate
    // must agree about; this gate had a hand copy of the first until the round's
    // third-consumer grep.
    let announced = pinion_a11y::announcements(&nodes);
    let referenced = pinion_a11y::referenced_tags(&nodes);
    pinion_core::voice::voice_census(&scene, &announced, &referenced)
}

/// The tags a [`spec::VoiceSpec`] row stands for, expanded from the table its
/// population names.
///
/// ★ Expanded rather than listed, so a ninth role or a sixth field is demanded
/// by this gate the moment it is added to the specification — the property
/// R1651.1 wrote down after a hand-written population reported a sample as
/// coverage.
fn voice_population(tag: &str, population: spec::Population) -> Vec<String> {
    let fill = |member: &str| tag.replace("{}", member);
    match population {
        spec::Population::One => vec![tag.to_owned()],
        spec::Population::Roles => spec::ROLES.iter().map(|r| fill(r.name)).collect(),
        spec::Population::Rail => spec::RAIL.iter().map(|(n, _)| fill(n)).collect(),
        spec::Population::Nodes => spec::NODES.iter().map(|n| fill(n.id)).collect(),
        // A link is addressed by the identifier it was minted with, and the
        // opening graph mints them in the specification's own order.
        spec::Population::Links => (0..spec::LINKS.len())
            .map(|i| fill(&i.to_string()))
            .collect(),
        spec::Population::Fields => spec::FIELDS.iter().map(|f| fill(f.key)).collect(),
        spec::Population::Protocols => spec::PROTOCOLS.iter().map(|p| fill(p)).collect(),
        spec::Population::PinKinds => spec::PIN_LEGEND.iter().map(|(k, _)| fill(k)).collect(),
    }
}

/// ★★★★★ R1691 — **every addressable region of this screen is classified**, and
/// the split between what speaks and what is deliberately quiet is the
/// specification's rather than whatever the last round happened to leave.
///
/// Two properties, and the second is what makes the first mean anything:
///
/// 1. Nothing is unclassified. A painted, addressable region with no
///    accessibility node and no declared reason is a region a reader is never
///    told about and no author chose that — measured the day this was written,
///    **131 of this screen's 166**.
/// 2. The split is the one [`spec::VOICES`] and [`spec::SILENCES`] declare. A
///    total census is satisfiable by declaring everything silent, so totality
///    alone would let the whole finding be answered by writing it off.
///
/// Swept over every state, because a region that speaks as the screen opens and
/// goes quiet once a card is added is exactly the failure a one-state check
/// cannot see.
#[test]
fn r1691_every_addressable_region_is_classified_in_every_state() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        for (when, mutate) in STATES {
            mutate(&state);
            // ★ Both sizes. The floor is where the side panes overflow and their
            // content is reached only by scrolling, and "painted where nobody
            // can see it" and "painted with no voice" are two different ways to
            // be unreachable — a screen could close one by opening the other.
            for size in [(WIN_W, WIN_H), (super::MIN_W, super::MIN_H)] {
                let census = voice_of(&state, size);
                let holes: Vec<String> = census
                    .defects()
                    .map(|row| format!("{} ({})", row.tag, row.voice.name()))
                    .collect();
                assert!(
                    holes.is_empty(),
                    "{when} at {size:?}: regions a reader is not told about and \
                     nobody declared quiet: {holes:?}",
                );
                // Not vacuous: a screen that painted nothing addressable would
                // pass the line above.
                assert!(
                    census.count(pinion_core::voice::Voice::Announced) >= 100,
                    "{when} at {size:?}: only {} regions announce — the census \
                     is passing on an empty population",
                    census.count(pinion_core::voice::Voice::Announced),
                );
            }
        }
    });
}

/// The declared split, driven: what owes a voice speaks, and what owes a
/// silence is quiet **with the class the specification names**.
#[test]
fn r1691_the_screen_speaks_and_is_quiet_exactly_where_the_specification_says() {
    use pinion_a11y::WidgetA11y;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let census = voice_of(&state, (WIN_W, WIN_H));
        let rows: BTreeMap<&str, &pinion_core::voice::VoiceNode> =
            census.nodes.iter().map(|n| (n.tag.as_str(), n)).collect();
        let nodes = super::NodeLabView::access_node(&(TextFieldState::Idle, 0), None);
        let roles: BTreeMap<&str, &'static str> = nodes
            .iter()
            .map(|n| (n.tag.as_str(), n.role.aria_name()))
            .collect();

        let mut spoken = 0;
        for want in spec::VOICES {
            for tag in voice_population(want.tag, want.population) {
                let row = rows.get(tag.as_str()).unwrap_or_else(|| {
                    panic!("the specification says {tag} is on screen and nothing paints it")
                });
                assert_eq!(
                    row.voice,
                    pinion_core::voice::Voice::Announced,
                    "{tag} owes a reader a voice and is {}",
                    row.voice.name(),
                );
                // ★ And carries no silence. A voice wins over a declaration in
                // the census — the tree is what a reader receives — so a
                // `quiet()` left on a region that also speaks would be a dead
                // declaration nothing else can fail on.
                assert!(
                    row.silence.is_none(),
                    "{tag} speaks AND declares a silence, so the declaration is \
                     a claim nobody acts on",
                );
                // The role is what a reader is told they can DO, so a control
                // announced as the wrong kind is worse than one poorly named.
                // An empty column means the role is the shape's — checked
                // against the field's own type word below.
                if !want.role.is_empty() {
                    assert_eq!(
                        roles.get(tag.as_str()).copied(),
                        Some(want.role),
                        "{tag} announces as the wrong kind",
                    );
                }
                spoken += 1;
            }
        }
        assert!(spoken >= 60, "the declared population is {spoken}");

        let mut quiet = 0;
        for (tag, population, kind) in spec::SILENCES {
            for tag in voice_population(tag, *population) {
                let row = rows.get(tag.as_str()).unwrap_or_else(|| {
                    panic!("the specification declares {tag} quiet and nothing paints it")
                });
                assert_eq!(
                    row.voice,
                    pinion_core::voice::Voice::Silent,
                    "{tag} is declared quiet and the census calls it {}",
                    row.voice.name(),
                );
                assert_eq!(
                    row.silence.as_ref().map(|s| s.kind().name()),
                    Some(*kind),
                    "{tag} is quiet for a reason the specification does not state",
                );
                quiet += 1;
            }
        }
        assert!(quiet >= 25, "the declared silences are {quiet}");
    });
}

/// ★★★ A control announced as the wrong kind tells a reader to do something the
/// control cannot do, and the form had exactly that for its whole life: a
/// boolean row was a **text box** to a screen reader.
///
/// The population is [`spec::FIELDS`]' own type words, so a field whose shape
/// changes moves this without anybody editing it.
#[test]
fn r1691_a_rows_control_announces_the_kind_its_shape_is() {
    use pinion_a11y::WidgetA11y;

    let owner = Owner::new();
    owner.run(|| {
        let _state = use_lab_state();
        let nodes = super::NodeLabView::access_node(&(TextFieldState::Idle, 0), None);
        let roles: BTreeMap<&str, &'static str> = nodes
            .iter()
            .map(|n| (n.tag.as_str(), n.role.aria_name()))
            .collect();
        for field in spec::FIELDS {
            let want = match field.ty {
                "int" => "spinbutton",
                "perm" => "group",
                "address[]" => "list",
                // `id` is a formatted string and `text` is free text; both are
                // typed into.
                _ => "textbox",
            };
            let tag = format!("lab.form.control.{}", field.key);
            assert_eq!(
                roles.get(tag.as_str()).copied(),
                Some(want),
                "{} is typed {} and announces as the wrong kind",
                field.key,
                field.ty,
            );
        }
        // ★★ And every affordance INSIDE a control, which is where the roles
        // are decided per shape and where a fallback would otherwise let a
        // wrong one pass unseen: a named node satisfies the census whatever it
        // calls itself.
        let want_part = [
            ("lab.form.step.transport.link.tx.batch_size.up", "button"),
            ("lab.form.step.transport.link.tx.batch_size.down", "button"),
            ("lab.form.item.listen.endpoints.0", "textbox"),
            ("lab.form.item.listen.endpoints.add", "button"),
            // A `perm` field takes any subset, so its options are independent
            // checkboxes rather than a radio set — the distinction that tells a
            // reader whether picking one un-picks another.
            ("lab.form.option.control.permissions.read", "checkbox"),
            ("lab.form.option.control.permissions.write", "checkbox"),
        ];
        for (tag, want) in want_part {
            assert_eq!(
                roles.get(tag).copied(),
                Some(want),
                "{tag} announces as the wrong kind",
            );
        }
    });
}

/// The slot that moves when a scope is put back — read off the operation table
/// rather than restated here, so a scope whose witness changes moves this too.
fn scope_witness(scope: super::ResetScope) -> &'static str {
    let name = match scope {
        super::ResetScope::Nodes => "reset the node set",
        super::ResetScope::Layout => "reset the layout",
        super::ResetScope::Fields => "reset the fields",
        super::ResetScope::Links => "reset the links",
        super::ResetScope::View => "reset the view",
    };
    spec::OPERATIONS
        .iter()
        .find(|op| op.name == name)
        .unwrap_or_else(|| panic!("the operation table holds {name:?}"))
        .witness
}
