//! R670 §5.41 §5.40 — integration test for the TUI RPC ingress.
//!
//! Exercises [`pinion_tui::ShellCoreTui::dispatch_rpc`] against the
//! shared `ButtonFixture` test binding, covering the same wire arcs
//! the production crossterm shell forwards through:
//!
//! - `scene/snapshot` (read-only — returns the live scene)
//! - `scene/click` (input injection — deferred-input drain mutates
//!   cached state)
//! - `scene/key` named-key arc (e.g. Space → `KeyboardActivate`)
//! - `scene/key` character-key arc (R666 — single-codepoint string
//!   routes through `V::keybinding` first)
//! - `scene/invoke` (drives SCXML transitions directly via the
//!   `External::invoke` channel)
//! - `focus/get` + `focus/set` (programmatic focus through the
//!   substrate's `FocusManager` mirror)
//!
//! The §2 #6 GUI/TUI dual invariant requires identical wire-form
//! responses across both backends. Each assertion below has its
//! pinion-shell mirror in `crates/pinion-shell/src/` integration
//! tests (R51.83 / R51.196 / R666 / R51.73 axes); this file is the
//! TUI side of the same contract.

use pinion_core::test_fixtures::{ButtonFixture as TestButtonView, RepeatingButtonFixture};
use pinion_core::widgets::button::ButtonState;
use pinion_tui::ShellCoreTui;

/// R670 §5.41 §5.40 — substrate must respond to `scene/snapshot`
/// with the live scene. The default test binding is a `Button`
/// (initial state `Idle`); the snapshot includes the `External`
/// node `test_btn` the button externalises through.
#[test]
fn r670_dispatch_rpc_scene_snapshot_returns_json_with_external_node() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    // Prime the paint snapshot so `scene/snapshot` from: paint works
    // identically to the Vello side post-first-paint.
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"scene/snapshot","params":{"path":""}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/snapshot must produce a response");
    assert!(
        response.contains("\"jsonrpc\":\"2.0\""),
        "response shape: {response}",
    );
    assert!(response.contains("\"id\":1"), "id round-trips: {response}",);
    // Root scene IS the test_btn's External node (ButtonFixture's
    // create_external returns a Scene::External(node) with the
    // button tag); the snapshot must include the External tag in
    // its node shape.
    assert!(
        response.contains("external") || response.contains("test_btn"),
        "snapshot must include the button's External node: {response}",
    );
}

/// R670 §5.41 §5.49 §5.45 — `scene/click` enqueues a `Click`
/// `DeferredInput`; the substrate's post-dispatch drain (via the
/// R668 `drain_deferred_inputs` substrate) replays the cursor +
/// press + release sequence so the SCXML statechart transitions to
/// `Hover` (the Pressed → Hover edge fires on `pointer_up` against
/// the button rect).
#[test]
fn r670_dispatch_rpc_scene_click_drains_to_hover_state() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    assert_eq!(*core.cached_state(), ButtonState::Idle);

    // The ButtonFixture binding lays the button at pixel (0..32, 0..48);
    // click at (8, 8) lands inside the rect and arms the activation arc.
    let request =
        r#"{"jsonrpc":"2.0","id":2,"method":"scene/click","params":{"at":{"x":8.0,"y":8.0}}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/click must produce a response");
    assert!(response.contains("\"id\":2"), "id round-trips: {response}",);
    assert_eq!(
        *core.cached_state(),
        ButtonState::Hover,
        "post-drain state must land in Hover (Pressed → Hover edge on release)",
    );
}

/// R670 §5.41 §5.49 §5.45 — `scene/key` with a multi-char W3C key
/// name routes through the substrate's `dispatch_key` (named arc).
/// `Space` arms the Button's `KeyboardActivate` event — the SCXML
/// internal transition fires the `click` intent but leaves the
/// visible state unchanged (Idle → Idle), so the substrate observes
/// the intent through the dispatch tail without a visible flip.
#[test]
fn r670_dispatch_rpc_scene_key_named_space_fires_click_intent() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    assert_eq!(*core.cached_state(), ButtonState::Idle);

    let request = r#"{"jsonrpc":"2.0","id":3,"method":"scene/key","params":{"at":{"x":8.0,"y":8.0},"key":"Space"}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/key must produce a response");
    assert!(response.contains("\"id\":3"), "id round-trips: {response}",);
    // The deferred-input drain runs `cursor_moved(8, 8)` BEFORE the
    // named-key dispatch, so the cursor lands on the button rect →
    // Idle → Hover. The subsequent `Space` press fires the
    // `KeyboardActivate` event, which transitions internally (Hover
    // → Hover) and emits the `click` intent. End state must be Hover
    // because the cursor is still parked on the button.
    assert_eq!(*core.cached_state(), ButtonState::Hover);
}

/// R670 §5.41 §5.49 §5.45 / R666 — `scene/key` with a
/// single-codepoint string routes through the character-key arc.
/// The Button fixture's `keybinding` maps `'d'` → `Disable`, so the
/// SCXML statechart flips Idle → Disabled. Mirrors the R666
/// character-vs-named auto-discriminator on the TUI path.
#[test]
fn r670_dispatch_rpc_scene_key_character_d_disables_button() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    assert_eq!(*core.cached_state(), ButtonState::Idle);

    let request = r#"{"jsonrpc":"2.0","id":4,"method":"scene/key","params":{"at":{"x":8.0,"y":8.0},"key":"d"}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/key character arc must produce a response");
    assert!(response.contains("\"id\":4"), "id round-trips: {response}",);
    assert_eq!(
        *core.cached_state(),
        ButtonState::Disabled,
        "character-key 'd' routes through keybinding → Disable event → Disabled state",
    );
}

/// R670 §5.41 §5.40 — `scene/invoke` drives an `External::invoke`
/// call directly against the addressed widget. Sends `Disable` to
/// the button's External handle through the SCXML `send` channel;
/// the statechart transitions Idle → Disabled identically to the
/// keybinding arc above but through a different wire path.
#[test]
fn r670_dispatch_rpc_scene_invoke_send_disable_flips_state() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    assert_eq!(*core.cached_state(), ButtonState::Idle);

    // The ButtonFixture's root scene IS a `Scene::External(node)`,
    // so the v1 invoke path resolves to `/external/{action}`. R666
    // multi-External path syntax (`/<tag>/external/<action>`) only
    // applies to bindings with ExtraExternal children; single-
    // External bindings keep the canonical short form.
    let request = r#"{"jsonrpc":"2.0","id":5,"method":"scene/invoke","params":{"path":"/external/send","args":"Disable"}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/invoke must produce a response");
    assert!(response.contains("\"id\":5"), "id round-trips: {response}",);
    assert_eq!(
        *core.cached_state(),
        ButtonState::Disabled,
        "Disable event must transition the SCXML statechart through the invoke channel",
    );
}

/// R670 §5.41 §5.39 — `focus/get` reads the substrate's
/// `FocusManager`. The `ButtonFixture` binding paints a single
/// `.with_focusable(true)` node, so the R1020 scene-derived enumeration
/// seeds the tab order with the button tag — but `focus/get` reports
/// `None` until something focuses it (the `FocusManager` seeds the tab
/// order but does not auto-focus the first tag).
#[test]
fn r670_dispatch_rpc_focus_get_returns_none_on_fresh_substrate() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let request = r#"{"jsonrpc":"2.0","id":6,"method":"focus/get"}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("focus/get must produce a response");
    assert!(response.contains("\"id\":6"), "id round-trips: {response}",);
    // `focused_tag: null` shape — no widget has focus on a fresh substrate.
    assert!(
        response.contains("null") || response.contains("\"focused_tag\":null"),
        "focus/get on fresh substrate reports no focused tag: {response}",
    );
}

/// R670 §5.41 §5.39 — `focus/set` mutates the `FocusManager`
/// directly through the RPC channel. Sets focus to the button's
/// tag; the post-dispatch `focus_before != focus_after` arm fires
/// the `External::on_focus_change(true)` notification on the
/// button's External, mirroring the click-to-focus arc the Vello
/// side exercises.
#[test]
fn r670_dispatch_rpc_focus_set_targets_button_tag() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    assert_eq!(core.focus().focused(), None, "fresh focus is empty");

    let request = r#"{"jsonrpc":"2.0","id":7,"method":"focus/set","params":{"tag":"test_btn"}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("focus/set must produce a response");
    assert!(response.contains("\"id\":7"), "id round-trips: {response}",);
    assert_eq!(
        core.focus().focused(),
        Some("test_btn"),
        "focus/set must update the substrate's FocusManager",
    );
    // R1327 §5.39 §2 #6 (PR-53) — the focus a binding READS is published by the
    // `FocusManager` itself (the shared state owner), not re-derived per backend,
    // so the value and the equality-skip/self-heal behaviour are identical on both
    // render dispatch paths: one scene, ONE focus channel. R1335 owner-scoped it;
    // each backend does perform ONE setup step — `attach_owner(root_owner)` at boot
    // (asserted by construction: this test would read `None` if the TUI shell had
    // skipped it) — but the per-commit publish itself stays in the manager, so a
    // backend cannot drift the channel one mutation at a time.
    // Read the mirror inside the binding's root-owner scope, where a view fn / Effect reads it.
    assert_eq!(
        core.root_owner()
            .run(pinion_core::focus_state::focused)
            .as_deref(),
        Some("test_btn"),
        "the TUI backend publishes the focused tag to the binding too",
    );
}

/// R670 §5.41 §5.34 — `scene/click` mutating dispatch bumps the
/// §5.34 R40.4 OCC revision counter. AI clients pair this with
/// `propose_change` / `apply_preview` to detect concurrent mutation
/// during preview lifecycle. The TUI path must mirror the Vello
/// path's revision-bump semantics exactly so an AI client targeting
/// either backend gets the same wire-form OCC behaviour.
#[test]
fn r670_dispatch_rpc_mutating_call_bumps_revision_counter() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    let revision_before = core.revision();

    let request =
        r#"{"jsonrpc":"2.0","id":8,"method":"scene/click","params":{"at":{"x":8.0,"y":8.0}}}"#;
    let _ = core.dispatch_rpc(request);

    let revision_after = core.revision();
    assert!(
        revision_after > revision_before,
        "mutating dispatch must bump the OCC revision: before={revision_before} after={revision_after}",
    );
}

/// R984 §5.40 §2 #7 — `scene/access` on the TUI dumps the accessibility
/// tree, closing the §2 #6 dual-backend asymmetry. Pre-R984 the TUI had no
/// access producer, so the method answered `AccessTreeUnavailable` (the GUI
/// answered the enriched dump). The producer now runs the shared
/// `pinion_a11y::build_access_tree` SSOT, so the TUI returns the same
/// `{count, focus, nodes}` envelope shape. The atomic `ButtonFixture` carries
/// the default-empty a11y projection, so the node list is empty here — the
/// point is the WIRING (a success envelope, not the unavailable error); a
/// non-trivial tree is exercised by the `hello-textfield-tui` demo.
#[test]
fn r984_dispatch_rpc_scene_access_returns_tree_not_unavailable() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);

    let request = r#"{"jsonrpc":"2.0","id":9,"method":"scene/access","params":{}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/access must produce a response");
    assert!(response.contains("\"id\":9"), "id round-trips: {response}",);
    // The §2 #6 parity deliverable: the access producer is wired, so the dump
    // is a success envelope, NOT the pre-R984 AccessTreeUnavailable error.
    assert!(
        !response.contains("AccessTreeUnavailable"),
        "TUI must no longer report the access tree as unavailable: {response}",
    );
    assert!(
        response.contains("\"result\""),
        "scene/access must return a success result envelope: {response}",
    );
    // The envelope shape mirrors the GUI dump: count + nodes + focus. The atomic
    // ButtonFixture's a11y projection is empty, so count is 0 and focus is null
    // on a fresh substrate — assert the exact honest values, not just key
    // presence (R984.1 L5: the prior key-presence check could not tell a correct
    // envelope from one with wrong values).
    assert!(
        response.contains("\"count\":0"),
        "an atomic empty-a11y binding reports count 0: {response}",
    );
    assert!(
        response.contains("\"nodes\":[]"),
        "the empty tree serializes an empty node array: {response}",
    );
    assert!(
        response.contains("\"focus\":null"),
        "no focus on a fresh substrate -> focus is null: {response}",
    );
}

/// R984.1 §5.40 §2 #7 — the TUI access producer's BOUNDS-resolution composition
/// (the H1 coverage gap: the empty-`ButtonFixture` wiring test above never
/// resolved a single rect on the TUI). Over a REAL committed TUI paint scene, an
/// `AccessNode` tagged like a painted widget resolves to that widget's
/// cell-geometry rect through the same `pinion_runtime::rect_for_tag` the
/// producer binds — proving the TUI bounds path (not only the GUI's) actually
/// resolves geometry, the same `resolve_access_bounds` SSOT both shells share.
#[test]
fn r984_1_tui_access_bounds_resolve_over_a_real_paint_scene() {
    use pinion_a11y::{AccessNode, AriaRole, resolve_access_bounds};

    let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);

    // The button externalises through the `test_btn` tag; the view lays it at a
    // known rect (the same one the click tests hit at (8, 8)).
    let mut nodes = vec![AccessNode::new("test_btn", AriaRole::Button)];
    resolve_access_bounds(&mut nodes, |tag| pinion_runtime::rect_for_tag(&paint, tag));

    let bounds = nodes[0]
        .bounds
        .expect("the painted widget's rect resolves on the TUI via rect_for_tag");
    assert!(
        bounds.w > 0 && bounds.h > 0,
        "the resolved cell-geometry rect is non-empty: {bounds:?}",
    );
}

/// R670 §5.41 §5.40 — malformed JSON-RPC frames produce a wire-form
/// error response (per the JSON-RPC 2.0 spec) without panicking the
/// substrate. AI clients that send corrupt frames must observe the
/// error wire shape so they can retry — the TUI substrate must
/// not crash the entire TUI binary on a single bad frame.
#[test]
fn r670_dispatch_rpc_malformed_request_returns_error_response() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let request = "not valid json";
    let response = core
        .dispatch_rpc(request)
        .expect("malformed frame must produce an error response, not silence");
    assert!(
        response.contains("\"error\""),
        "malformed frame must produce a JSON-RPC error envelope: {response}",
    );
}

/// R885 §5.49 — `scene/input_state` on the TUI: `modifiers` is `null`
/// (crossterm delivers modifiers per-key-event only; the shell keeps
/// no absolute cache — the documented §2 #6 carry), while the
/// held-key chord cache is real (RPC-owned, R882) so a
/// `scene/key state:"down"` write reads back as `["Space"]`.
#[test]
fn r885_input_state_reports_null_modifiers_and_real_held_keys() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();

    let read = r#"{"jsonrpc":"2.0","id":1,"method":"scene/input_state","params":{}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""modifiers":null"#),
        "TUI keeps no absolute modifier cache; the wire must say so: {response}",
    );
    assert!(
        response.contains(r#""held_keys":[]"#),
        "no chord key held at boot: {response}",
    );

    let down = r#"{"jsonrpc":"2.0","id":2,"method":"scene/key","params":{"key":"Space","state":"down","at":{"x":1.0,"y":1.0}}}"#;
    let _ = core.dispatch_rpc(down).expect("write must respond");
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""held_keys":["Space"]"#),
        "scene/key down must read back through the held-key cache: {response}",
    );
}

/// R886.1 §5.49 — the TUI cursor leg of `scene/input_state` is real
/// (the session-review audit found it untested): a `scene/click`
/// injection moves the `DEFAULT_WINDOW` router's mouse position, and
/// the READ returns exactly that point.
#[test]
fn r886_1_input_state_cursor_follows_tui_click() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);

    let read = r#"{"jsonrpc":"2.0","id":1,"method":"scene/input_state","params":{}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""cursor":null"#),
        "no cursor event at boot: {response}",
    );

    let click =
        r#"{"jsonrpc":"2.0","id":2,"method":"scene/click","params":{"at":{"x":7.0,"y":3.0}}}"#;
    let _ = core.dispatch_rpc(click).expect("click must respond");
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""cursor":{"x":7.0,"y":3.0}"#),
        "cursor follows the click injection: {response}",
    );
}

/// R888 §5.49 §5.28 — `scene/pacing_state` on the TUI answers
/// `PacingStateUnavailable`: the terminal backend keeps no
/// frame-pacing clock (repaints are event-driven; `SetTargetFps`
/// drains as a wildcard no-op), so the READ peer must expose the
/// whole axis as absent — NOT alias it onto "default policy" — the
/// `modifiers: null` honesty precedent's whole-axis variant (§2 #6).
#[test]
fn r888_1_set_fps_write_is_unavailable_on_the_tui() {
    // R888.1 — write/read agree: the TUI cannot answer
    // `scene/pacing_state`, so it must not accept-and-drop a
    // `scene/set_fps` either (the R884 silent-no-op class).
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let write = r#"{"jsonrpc":"2.0","id":1,"method":"scene/set_fps","params":{"fps":30}}"#;
    let response = core.dispatch_rpc(write).expect("write must respond");
    assert!(
        response.contains(r#""error""#) && response.contains("PacingStateUnavailable"),
        "TUI must reject the pacing write with the shared token: {response}",
    );
}

#[test]
fn r888_pacing_state_is_unavailable_on_the_tui() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let read = r#"{"jsonrpc":"2.0","id":1,"method":"scene/pacing_state","params":{}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""error""#) && response.contains("PacingStateUnavailable"),
        "TUI has no pacing clock; the wire must say so: {response}",
    );
}

/// R889 §5.49 — the TUI gates the in-band `{window: "<id>"}` scope
/// through the same window-known predicate as the GUI
/// (`CoreShell::is_window_known`; the TUI registry holds exactly the
/// seeded `DEFAULT_WINDOW`). Pre-R889 the TUI never read the param:
/// a request scoped to ANY window id silently acted on the single
/// terminal window — the GUI's alias-to-primary smell in §2 #6
/// disguise.
#[test]
fn r889_unknown_window_scope_is_rejected_on_the_tui() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let read =
        r#"{"jsonrpc":"2.0","id":1,"method":"scene/input_state","params":{"window":"bogus"}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""code":-32602"#) && response.contains("unknown_window"),
        "bogus window scope must error, not alias onto the terminal window: {response}",
    );
    assert!(
        response.contains(r#""data":"bogus""#),
        "error data names the supplied id: {response}",
    );
}

#[test]
fn r889_main_window_scope_passes_the_tui_gate() {
    // `window: "main"` names the seeded DEFAULT_WINDOW — the gate
    // admits it and the dispatch proceeds exactly as without the
    // param (the TUI's single window IS main).
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let read =
        r#"{"jsonrpc":"2.0","id":1,"method":"scene/input_state","params":{"window":"main"}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""modifiers":null"#) && !response.contains("unknown_window"),
        "main scope passes the gate and answers normally: {response}",
    );
}

/// R890 §5.12 §2 #6 — `scene/layout {viewport: null}` projects the
/// router's stored paint scene with the canonical `"/0"` root prefix.
/// The retired per-commit TUI mirror built with a bare `""` prefix,
/// so the SAME node had different layout paths on the two backends —
/// and even between viewport:null and viewport-supplied reads on the
/// TUI itself. One projection home closes the divergence.
#[test]
fn r890_layout_viewport_null_projects_router_scene_with_canonical_paths() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    // Before any paint: honest NoLastPaintLayout (no mirror to leak).
    let read = r#"{"jsonrpc":"2.0","id":1,"method":"scene/layout","params":{}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains("NoLastPaintLayout"),
        "no paint yet -> honest absence: {response}",
    );
    // After the commit hand-off, the projection answers with the
    // canonical "/0" root path (GUI parity).
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    let read = r#"{"jsonrpc":"2.0","id":2,"method":"scene/layout","params":{}}"#;
    let response = core.dispatch_rpc(read).expect("read must respond");
    assert!(
        response.contains(r#""path":"/0""#),
        "root path is the canonical /0 wire shape: {response}",
    );
    assert!(
        !response.contains(r#""path":"""#),
        "the retired bare-prefix mirror shape must not reappear: {response}",
    );
}

/// R1344 §5.41 §5.12 §2 #2 — `scene/layout {viewport}` (the AI-primary,
/// never-painted path) returns MEASURED rects, not zeros.
///
/// `DispatchContext::paint_producer`'s contract requires the application to
/// run `compute_layout` inside the producer so the returned `Scene` carries
/// measured rects, and `layout_query` calls this arm "`dry_run` semantics".
/// Pre-R1344 the TUI producer returned a raw `V::view` result and got away
/// with it only because views authored their own rects. R1344 makes `rect` an
/// output, so an unlaid-out producer would hand every headless AI client an
/// all-zero rect tree — relocating the §2 #6 divergence this round closes onto
/// the §2 #2 primary path. Nothing covered this arm before; now it is pinned.
#[test]
fn r1344_scene_layout_viewport_returns_measured_rects_not_zeros() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    // NOTE: deliberately no `compute_paint_scene` / `update_paint_scene` —
    // this is the never-painted hypothetical-viewport arm.
    let request = r#"{"jsonrpc":"2.0","id":7,"method":"scene/layout","params":{"viewport":{"width":640,"height":384}}}"#;
    let response = core
        .dispatch_rpc(request)
        .expect("scene/layout must produce a response");
    assert!(response.contains("\"id\":7"), "id round-trips: {response}");
    assert!(
        !response.contains("\"error\""),
        "must not error: {response}",
    );
    // The fixture's root resolves against the supplied viewport. A raw
    // (unlaid-out) view reports the authored `Rect::new(0, 0, 32, 48)`; a
    // laid-out one fills the requested 640×384.
    assert!(
        response.contains(r#""rect":{"h":384,"w":640,"x":0,"y":0}"#),
        "the root must resolve to the requested viewport: {response}",
    );
}

/// R1549.5 §5.41 §5.35 §5.12 §2 #6 — `scene/auto_repeat` answers over the
/// TUI WIRE, not merely through the substrate accessor.
///
/// R1549.2 added the census and a test that called
/// `ShellCoreTui::auto_repeat_holds` directly, which left the dispatch
/// wiring unexercised: measured, removing
/// `ctx.with_auto_repeat_holds(...)` kept every pinion-tui test green
/// while the TUI wire silently answered `{"holds": []}` for every press
/// in flight. That is the fifth instance in one round of a guard landing
/// on a different arm from the capability, and the reason this file — the
/// §2 #6 wire-parity file — is where it belongs.
#[test]
fn r1549_5_scene_auto_repeat_answers_over_the_tui_wire() {
    let mut core: ShellCoreTui<RepeatingButtonFixture> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);

    let ask = r#"{"jsonrpc":"2.0","id":1,"method":"scene/auto_repeat","params":{}}"#;
    let idle = core
        .dispatch_rpc(ask)
        .expect("scene/auto_repeat must answer on the TUI");
    assert!(
        idle.contains(r#""holds":[]"#),
        "nothing held is an empty list, not an error: {idle}",
    );

    core.cursor_moved(8.0, 8.0);
    core.pointer_down();
    let held = core
        .dispatch_rpc(ask)
        .expect("scene/auto_repeat must answer while held");
    assert!(
        held.contains(r#""target":"test_btn""#),
        "the wire names the pressed target: {held}",
    );
    assert!(
        held.contains(r#""repeating":true"#),
        "and reports the declared cadence as live: {held}",
    );
    assert!(
        held.contains(r#""delay_secs""#),
        "with the cadence itself on the wire: {held}",
    );

    core.pointer_up();
    let released = core
        .dispatch_rpc(ask)
        .expect("scene/auto_repeat must answer after release");
    assert!(
        released.contains(r#""holds":[]"#),
        "the release empties the census: {released}",
    );
}

/// The negative control: an undeclared button's press is still REPORTED
/// over the wire, with `repeating` false — so the assertion above cannot
/// be satisfied by a backend that marks every press as repeating.
#[test]
fn r1549_5_the_tui_wire_reports_a_non_repeating_hold_as_a_hold() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);
    core.cursor_moved(8.0, 8.0);
    core.pointer_down();

    let held = core
        .dispatch_rpc(r#"{"jsonrpc":"2.0","id":1,"method":"scene/auto_repeat","params":{}}"#)
        .expect("answers");
    assert!(
        held.contains(r#""target":"test_btn""#),
        "the press IS in flight: {held}",
    );
    assert!(
        held.contains(r#""repeating":false"#),
        "it just declares no cadence: {held}",
    );
}

/// R1550 §5.41 §5.36 §5.7 §2 #6 — `scene/memory` answers over the TUI wire.
///
/// The arena census is a GUI-shaped fact in three of its four parts — the
/// paint-fragment cache holds `vello::Scene` encodings and the image caches
/// hold decoded RGBA8, neither of which a cell grid has — and that is exactly
/// the shape R1460 and R1549.2 both record as the §2 #6 trap: a method whose
/// GUI arm lands and whose terminal arm answers `Unavailable` forever, which
/// reads as "this backend has no memory" rather than "this backend has these
/// arenas".
///
/// A terminal holds one of them. `ShellCoreTui` keeps a `LayoutCache` even
/// though it never shapes (its measure arm lays text out on the cell grid),
/// and keeping it is not free: `LayoutCache` allocates its ghost index at
/// construction. So the row is present, priced, and — with nothing shaped —
/// exactly measured.
#[test]
fn r1550_scene_memory_answers_over_the_tui_wire() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    let paint = core.compute_paint_scene(80, 24);
    core.update_paint_scene(paint);

    let ask = r#"{"jsonrpc":"2.0","id":1,"method":"scene/memory","params":{}}"#;
    let response = core
        .dispatch_rpc(ask)
        .expect("scene/memory must answer on the TUI");

    assert!(
        response.contains(r#""arena":"text-shapes""#),
        "the shape cache is the arena a terminal holds: {response}",
    );
    assert!(
        !response.contains(r#""arena":"paint-fragments""#)
            && !response.contains(r#""arena":"images""#),
        "and the two it does not hold are ABSENT rather than zeroed — zero \
         bytes in an arena that exists and zero bytes in an arena that does \
         not are different facts: {response}",
    );
    assert!(
        response.contains(r#""window":null"#),
        "the shape cache is per shell, not per window: {response}",
    );
    assert!(
        !response.contains(r#""type":"parley::Layout""#),
        "this backend never shapes, so it holds no opaque LAYOUTS — the \
         derived basis is a fact about what is held, not a label on the \
         type: {response}",
    );
    assert!(
        response.contains(r#""type":"parley::LayoutContext""#)
            && response.contains(r#""basis":"partial""#),
        "it does hold parley's shaping scratch space, whose interior is as \
         opaque as a Layout's, and the row names it rather than rounding it \
         to zero: {response}",
    );
    assert!(
        !response.contains(r#""total_bytes":0"#),
        "and the arena is not free: a LayoutCache allocates its ghost index \
         at construction: {response}",
    );
    assert!(
        response.contains(r#""process_rss_bytes""#),
        "the process total rides beside the arenas on this backend too: \
         {response}",
    );
}

// ─── R1552 §5.7 §2 #6 PINION-PR83 — change streams on the terminal backend ───

/// A recording egress: keeps every frame written to it, so a test can assert
/// what the server said *unprompted*.
///
/// The TUI's production egress writes to stderr; what matters for the contract
/// is that a frame reaches the connection's writer, so the test substitutes its
/// own rather than parsing a stream it does not own.
#[derive(Default)]
struct RecordingEgress {
    frames: std::sync::Mutex<Vec<String>>,
}

impl pinion_rpc::RpcEgress for RecordingEgress {
    fn send_frame(&self, frame: String) -> bool {
        self.frames.lock().expect("egress lock").push(frame);
        true
    }
}

/// R1552 §5.7 §2 #6 — `scene/subscribe` works on the terminal backend, the
/// stream is delivered by the same `SceneRevision` observer the GUI installs,
/// and a stream is silent until the dispatch site has ARMED it.
///
/// The §2 #6 claim this round makes is that both backends answer the same wire
/// the same way. R1552's demo proves the GUI half against a live binary; this
/// is the TUI half, and without it the claim would rest on the wiring looking
/// right rather than on it having been run.
///
/// Arming is deliberately the *caller's* step, not the core's: it must happen
/// AFTER the subscribing frame's response has been written, and only the
/// caller knows when that is (`drain_rpc_into_substrate` on this backend,
/// `AppShell::dispatch_rpc` on the GUI). Driving the core directly, as this
/// test does, is therefore what makes the un-armed window observable — so the
/// test asserts the property rather than working around it.
#[test]
fn r1552_a_terminal_connection_is_written_to_unprompted() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    // Two handles to ONE egress: the typed one the test reads, and the trait
    // object the dispatcher takes. An `Arc<RecordingEgress>` coerces to the
    // trait object on clone, so there is no downcast and no second sink.
    let recorder = std::sync::Arc::new(RecordingEgress::default());
    let egress: std::sync::Arc<dyn pinion_rpc::RpcEgress> = recorder.clone();
    let conn = pinion_rpc::ConnId::allocate();

    // Subscribing answers ONCE, like any other method.
    let response = core
        .dispatch_rpc_from(
            r#"{"jsonrpc":"2.0","id":1,"method":"scene/subscribe"}"#,
            Some((conn, &egress)),
        )
        .expect("scene/subscribe must produce a response");
    let opened: serde_json::Value = serde_json::from_str(&response).expect("valid JSON");
    let subscription = opened["result"]["subscription"]
        .as_u64()
        .expect("a subscription id");

    let delivered = |id: u64| -> Option<u64> {
        pinion_rpc::process_registry()
            .views()
            .subscriptions
            .iter()
            .find(|s| s.subscription == id)
            .map(|s| s.delivered_count)
    };

    // Un-armed: a scene advance reaches this stream with NOTHING. That is the
    // window a client would otherwise be told about an id it has not received.
    let _ = core.dispatch_rpc_from(
        r#"{"jsonrpc":"2.0","id":2,"method":"scene/tick","params":{"dt":0.016}}"#,
        Some((conn, &egress)),
    );
    assert_eq!(
        delivered(subscription),
        Some(0),
        "un-armed streams are silent"
    );
    assert!(
        recorder.frames.lock().expect("egress lock").is_empty(),
        "and nothing was written to the connection",
    );

    // What the drain does once the subscribing frame's response is on the wire.
    assert_eq!(pinion_rpc::process_registry().arm_pending(), 1);

    // Now the observer publishes: the advance already owed, named by revision.
    let _ = core.dispatch_rpc_from(
        r#"{"jsonrpc":"2.0","id":3,"method":"scene/tick","params":{"dt":0.016}}"#,
        Some((conn, &egress)),
    );
    assert_eq!(
        delivered(subscription),
        Some(1),
        "the advance reached the stream"
    );

    // And what reached the connection is a NOTIFICATION — the property the
    // whole wire form turns on, asserted on the terminal side too.
    let frames = recorder.frames.lock().expect("egress lock").clone();
    let note: serde_json::Value = serde_json::from_str(
        frames
            .last()
            .expect("the connection was written to unprompted"),
    )
    .expect("valid JSON");
    assert_eq!(note["method"], "scene/changed");
    assert_eq!(note["params"]["subscription"], subscription);
    assert_eq!(note["params"]["revision"], core.revision());
    assert!(note.get("id").is_none(), "a notification carries no id");

    // Clean up the process-wide registry so a later test in this binary starts
    // from the same state this one did.
    assert_eq!(pinion_rpc::process_registry().close_connection(conn), 1);
}
