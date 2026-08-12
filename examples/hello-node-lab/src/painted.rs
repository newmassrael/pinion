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

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
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
}

impl Painted {
    /// Run the real pipeline and index what came out of it.
    fn of(scene: &Scene, window: (u32, u32)) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
        let mut reachable = BTreeMap::new();
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
                runs.push((text.content.clone(), rect, owner));
            }
        });
        Self {
            tags,
            runs,
            reachable,
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
    let mut scene = super::view((), Frame::default());
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
        "lab.toolbar.run".into(),
        "lab.gate".into(),
        "lab.gate.verdict".into(),
        "lab.hint".into(),
        "lab.hint.text".into(),
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
    match tag {
        "lab.toolbar.zoom.in" => Some("zoom:in".into()),
        "lab.toolbar.zoom.out" => Some("zoom:out".into()),
        "lab.toolbar.config" => Some("config".into()),
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

/// ★ R1656 — every painted mark is inside the box that owns it, asked of the
/// framework rather than of a helper here.
///
/// The property a person had to find by looking: at the size this screen opens
/// in, **seven of its eight node cards painted their last field row three to
/// five pixels below their own border**, and every check in this module was
/// green. Two reasons, and both are why this calls
/// [`pinion_core::containment::escapes`] instead of comparing rectangles
/// locally:
///
/// * `assert_contained` next door judges a run against its nearest **tagged
///   ancestor**, and the card's parts were the card's SIBLINGS. The owner it
///   resolved was the canvas, which is big enough to hold anything, so the
///   question was answered honestly about the wrong box.
/// * every rectangle in this module is the box the view *gave* a mark. The card
///   rows were inside their boxes. What left the card was the **ink**, and the
///   ink is not in the scene — it is a measurement.
///
/// So the check has to come from the framework, and the framework has to be
/// asked with a real metric. The stub used here is proportional to the string
/// rather than shaped, and that is deliberate: a shaped measurement makes this
/// gate green or red depending on which fonts the host has ([[zero-flake-policy]]).
/// The shaped answer is what `scene/containment` reports at boot to
/// `tools/rpc_verify.py`, on the machine that is actually painting.
fn assert_contained_ink(when: &str, scene: &Scene, size: (u32, u32)) -> usize {
    let (escapes, offscreen) = ink_escapes(scene, size);
    assert!(
        escapes.is_empty(),
        "{when}: {} painted mark(s) are outside the box that owns them — {:?}",
        escapes.len(),
        escapes
            .iter()
            .map(|e| (
                e.content.clone().or_else(|| e.tag.clone()),
                e.owner.clone(),
                e.over
            ))
            .take(6)
            .collect::<Vec<_>>()
    );
    offscreen
}

/// The escapes and how many of them were entirely off-window, without asserting.
///
/// Split out so a negative control can prove the metric MEASURES: a stand-in
/// that hands the box back reports every scene as clean, and a check that
/// cannot fail is indistinguishable from a screen with nothing wrong.
/// The ink stand-in both containment and reach are measured with.
///
/// One function because two stand-ins would drift, and a mark that is "inside
/// its box" under one measure and "out of reach" under another would be a
/// disagreement about the screen rather than about the two questions.
fn stand_in_ink(text: &pinion_core::scene::TextNode) -> (u32, u32) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a label is a handful of characters"
    )]
    let chars = text.content.chars().count() as u32;
    let px = text.style.font_size_px.max(1);
    let painted = if text.style.overflow.shortens() {
        text.rect.w.min(chars * px)
    } else {
        chars * px
    };
    (painted, text.rect.h)
}

fn ink_escapes(scene: &Scene, size: (u32, u32)) -> (Vec<pinion_core::containment::Escape>, usize) {
    let escapes = pinion_core::containment::escapes(scene, &mut |text| {
        // A monospace stand-in, wider per character than any face this screen
        // uses, so a box that passes here has room for a real one. Height is
        // the shaper's line box for the declared face, which is the number an
        // author most often gets wrong.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a label is a handful of characters"
        )]
        let chars = text.content.chars().count() as u32;
        let px = text.style.font_size_px.max(1);
        let painted = if text.style.overflow.shortens() {
            text.rect.w.min(chars * px)
        } else {
            chars * px
        };
        // ★ The WIDTH is stubbed and the HEIGHT is the laid-out one, and the
        // split is deliberate. "Is this string too long for its column" is a
        // question a font-independent stand-in can answer conservatively, and
        // it is the one an author gets wrong. "Is this line box tall enough for
        // this face" is answered by the layout pass using the real metrics of
        // the host's fonts, so re-deciding it here against a constant would
        // make this gate green or red depending on which fonts are installed
        // ([[zero-flake-policy]]) — and would demand that the flow layout agree
        // with a number this file made up. The shaped vertical answer is what
        // `scene/containment` reports at boot, on the machine that is painting.
        (painted, text.rect.h)
    });
    // Entirely below the window: nobody can see it, so it is the registered
    // scroll gap rather than a mark drawn over its neighbour. Partly visible
    // and escaping is still a defect — the grant is "invisible", not "low".
    let (offscreen, escapes): (Vec<_>, Vec<_>) = escapes
        .into_iter()
        .partition(|e| e.painted.y >= size.1 || e.painted.x >= size.0);
    (escapes, offscreen.len())
}

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
            let mut scene = super::view((), Frame::default());
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
        let mut scene = super::view((), Frame::default());
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
