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

/// Where every tag in the painted scene ended up, and every text run with it.
struct Painted {
    /// Tag -> the rectangle the layout pass gave it, window-absolute.
    tags: BTreeMap<String, Rect>,
    /// Every text run: its content, its rectangle, and the tag of its nearest
    /// tagged ancestor. Runs carry no tag of their own, which is exactly why
    /// they were the thing R1649 painted in a stack down the left edge with a
    /// 118-assertion demo watching.
    runs: Vec<(String, Rect, Option<String>)>,
}

impl Painted {
    /// Run the real pipeline and index what came out of it.
    fn of(scene: &Scene) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
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
        Self { tags, runs }
    }
}

/// Paint the screen as the window would: the view function, then the layout
/// pass, at whatever size the shell has published.
fn painted_at(state: &std::rc::Rc<LabState>, size: (u32, u32)) -> Painted {
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
    let shot = Painted::of(&scene);
    assert!(
        std::rc::Rc::ptr_eq(state, &use_lab_state()),
        "the sweep must drive the state the view reads"
    );
    shot
}

/// Paint the screen as the window would, at the design size.
fn painted(state: &std::rc::Rc<LabState>) -> Painted {
    painted_at(state, (WIN_W, WIN_H))
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

/// (1) FORWARD — every element the specification declares is painted.
fn assert_forward(when: &str, state: &LabState, shot: &Painted) {
    let want = declared_tags(state);
    let absent: Vec<&String> = want
        .iter()
        .filter(|tag| !shot.tags.contains_key(*tag))
        .collect();
    assert!(
        absent.is_empty(),
        "{when}: the specification declares {} element(s) the screen does not \
         paint: {absent:?}",
        absent.len()
    );
}

/// (3) REACHABLE — every painted control answers for itself when pressed at the
/// centre of the rectangle it was painted in. Returns how many were probed.
fn assert_reachable(when: &str, state: &LabState, shot: &Painted) -> usize {
    let probes: Vec<(&String, String)> = shot
        .tags
        .iter()
        .filter_map(|(tag, _)| must_answer(tag).map(|want| (tag, want)))
        .collect();
    assert!(
        probes.len() >= 55,
        "{when}: only {} painted control(s) — a screen that stops painting \
         controls must fail here, not report a smaller number",
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
fn assert_contained(when: &str, shot: &Painted) {
    let mut escaped = Vec::new();
    for (tag, rect) in &shot.tags {
        if let Some(pane) = owning_pane(tag)
            && !inside(pane, *rect)
        {
            escaped.push((tag, *rect, pane));
        }
    }
    assert!(
        escaped.is_empty(),
        "{when}: {} mark(s) are painted outside the pane their address puts \
         them in: {escaped:?}",
        escaped.len()
    );
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

/// The one sweep, over every state.
#[test]
fn r1653_the_painted_screen_is_the_specification_in_every_state() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_lab_state();
        let mut probed_total = 0;
        let mut richest = 0;
        for (when, mutate) in STATES {
            mutate(&state);
            let shot = painted(&state);
            assert_forward(when, &state, &shot);
            let probed = assert_reachable(when, &state, &shot);
            probed_total += probed;
            richest = richest.max(probed);
            assert_contained(when, &shot);
            assert_disjoint(when, &shot);
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
        for size in [(WIN_W, WIN_H), (1920, 1200), (900, 620)] {
            let shot = painted_at(&state, size);
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
        let narrow = painted_at(&state, (900, 620)).tags["lab.canvas"];
        let wide = painted_at(&state, (1920, 1200)).tags["lab.canvas"];
        assert!(
            wide.w > narrow.w && wide.h > narrow.h,
            "the canvas takes the room the window gained: {narrow:?} -> {wide:?}"
        );
    });
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
