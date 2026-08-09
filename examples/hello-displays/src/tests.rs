//! R1576 §5.16 §5.41 — the preset logic against **fabricated** desks.
//!
//! The demo drives the real binding on whatever monitors the host has, which is
//! one on CI and one here. These tests are where the multi-monitor cases live,
//! because a `DisplayTopology` is a value: a two-panel desk, a high-DPI panel
//! and a headless session are all arguments rather than hardware.
//!
//! What they pin is the part of R1576 this binding owns — that a preset is a
//! *declaration* and applying one only ever rewrites a `WindowSpec`.

use pinion_core::display::{DisplayId, DisplayInfo, DisplayRect, DisplayTopology};

use super::{
    ABSENT_DISPLAY, PANEL_INSET, PANEL_WINDOW, PRESETS, PresetKind, describe_placement, parse_rect,
    preset,
};
use pinion_shell::{SizeStrategy, WindowSpec};

fn panel() -> WindowSpec {
    WindowSpec::new(
        std::borrow::Cow::Borrowed(PANEL_WINDOW),
        "panel",
        SizeStrategy::Fixed {
            width: 320,
            height: 220,
        },
    )
}

/// Two 1000x1000 panels side by side, the RIGHT one primary — so a test that
/// confused "primary" with "first enumerated" fails here.
fn two_panels() -> DisplayTopology {
    DisplayTopology::new(vec![
        DisplayInfo::new("left", DisplayRect::new(0, 0, 1000, 1000)),
        DisplayInfo::new("right", DisplayRect::new(1000, 0, 1000, 1000)).as_primary(),
    ])
}

#[test]
fn r1576_every_preset_is_reachable_by_name() {
    for (name, kind) in PRESETS {
        assert_eq!(preset(name), Some(*kind), "{name} resolves");
    }
    assert_eq!(preset("no-such-preset"), None);
}

#[test]
fn r1576_the_primary_preset_names_the_primary_not_the_first() {
    let spec = PresetKind::PrimaryDisplay.apply_to(panel(), &two_panels());
    assert_eq!(
        spec.display.as_ref().map(DisplayId::as_str),
        Some("right"),
        "the preset resolves the PRIMARY at apply time"
    );
    assert_eq!(spec.position, Some(PANEL_INSET));
    // And the place it declares is on that display, not on the desktop origin.
    let desk = two_panels();
    let anchored = spec.placement().expect("placed").resolve(&desk);
    assert!(anchored.is_declared());
    assert_eq!(anchored.at(), Some((1048, 48)));
}

#[test]
fn r1576_a_preset_naming_an_absent_display_keeps_the_name_and_is_substituted() {
    let spec = PresetKind::NamedDisplay(ABSENT_DISPLAY).apply_to(panel(), &two_panels());
    assert_eq!(
        spec.display.as_ref().map(DisplayId::as_str),
        Some(ABSENT_DISPLAY),
        "the DECLARATION keeps the name it was given — a preset that silently \
         rewrote itself could never be corrected"
    );
    let desk = two_panels();
    let anchored = spec.placement().expect("placed").resolve(&desk);
    assert!(!anchored.is_declared());
    assert_eq!(anchored.name(), "substituted");
    assert_eq!(
        anchored.display().map(DisplayId::as_str),
        Some("right"),
        "and it lands on the FALLBACK, which is the primary"
    );
}

#[test]
fn r1576_an_absolute_preset_clears_the_display_the_previous_one_declared() {
    // The defect this exists to prevent: `with_position` alone would leave the
    // display behind, so `(120, 120)` would keep meaning "into that monitor"
    // and the preset would be a different place than it says.
    let placed = PresetKind::NamedDisplay("right").apply_to(panel(), &two_panels());
    assert!(placed.display.is_some());
    let absolute = PresetKind::Absolute((120, 120)).apply_to(placed, &two_panels());
    assert_eq!(absolute.display, None, "the display is cleared");
    assert_eq!(absolute.position, Some((120, 120)));
}

#[test]
fn r1576_the_unplaced_preset_clears_both_fields() {
    let placed = PresetKind::NamedDisplay("right").apply_to(panel(), &two_panels());
    let unplaced = PresetKind::Unplaced.apply_to(placed, &two_panels());
    assert_eq!(unplaced.display, None);
    assert_eq!(unplaced.position, None);
    assert!(
        unplaced.placement().is_none(),
        "declaring NO place is a state the type can hold, and it is what a \
         window-manager-placed window is"
    );
}

#[test]
fn r1576_the_primary_preset_degrades_to_absolute_on_a_headless_desk() {
    // Naming a display that does not exist would then be reported as
    // `substituted`, which would be a lie: nothing was substituted, there was
    // nothing to name.
    let spec = PresetKind::PrimaryDisplay.apply_to(panel(), &DisplayTopology::empty());
    assert_eq!(spec.display, None);
    assert_eq!(spec.position, Some(PANEL_INSET));
}

#[test]
fn r1576_a_relative_offset_scales_with_its_display() {
    let desk = DisplayTopology::new(vec![
        DisplayInfo::new("hidpi", DisplayRect::new(0, 0, 2000, 2000))
            .with_scale(2.0)
            .as_primary(),
    ]);
    let spec = PresetKind::PrimaryDisplay.apply_to(panel(), &desk);
    assert_eq!(
        spec.placement().expect("placed").resolve(&desk).at(),
        Some((96, 96)),
        "48 LOGICAL pixels in is 96 physical on a 2x panel — one preset, one \
         visible distance, on monitors of different densities"
    );
}

#[test]
fn r1576_the_description_names_the_outcome_and_the_display_used() {
    let desk = two_panels();
    assert_eq!(
        describe_placement(&panel(), &desk),
        "unplaced (the window manager decides)"
    );
    let on = PresetKind::NamedDisplay("left").apply_to(panel(), &desk);
    assert_eq!(
        describe_placement(&on, &desk),
        "on_declared on left at 48,48"
    );
    let gone = PresetKind::NamedDisplay(ABSENT_DISPLAY).apply_to(panel(), &desk);
    assert_eq!(
        describe_placement(&gone, &desk),
        "substituted on right at 1048,48"
    );
    // A headless desk has no place at all, and says so rather than reporting
    // a coordinate it made up.
    assert_eq!(
        describe_placement(&gone, &DisplayTopology::empty()),
        "no_display on none at nowhere"
    );
}

// ---- R1610 §5.16 §5.41 — the panel's window LEVEL ----

#[test]
fn r1610_a_level_is_declared_on_the_spec_and_nothing_else_moves() {
    // The claim the round rests on: pinning a panel is a spec write, not a
    // reach for a window handle — and it must not disturb the placement, which
    // is the other declaration living on the same struct.
    use pinion_core::window_level::WindowLevel;
    let placed = PresetKind::NamedDisplay("left").apply_to(panel(), &two_panels());
    let pinned = placed.clone().with_level(WindowLevel::AlwaysOnTop);
    assert_eq!(pinned.level, WindowLevel::AlwaysOnTop);
    assert_eq!(pinned.display, placed.display, "the display is untouched");
    assert_eq!(pinned.position, placed.position, "the offset is untouched");
    assert_eq!(pinned.title, placed.title);
    assert_eq!(
        describe_placement(&pinned, &two_panels()),
        describe_placement(&placed, &two_panels()),
        "a level change cannot move a window",
    );
}

#[test]
fn r1610_a_preset_does_not_disturb_the_level() {
    // The mirror image, and the one that would catch a preset rebuilding the
    // spec from scratch: applying a placement preset to a PINNED panel must
    // leave it pinned. A monitoring readout that quietly dropped behind the
    // app when the user moved it to the other monitor is the bug.
    use pinion_core::window_level::WindowLevel;
    let pinned = panel().with_level(WindowLevel::AlwaysOnTop);
    for (_, kind) in PRESETS {
        let moved = kind.apply_to(pinned.clone(), &two_panels());
        assert_eq!(
            moved.level,
            WindowLevel::AlwaysOnTop,
            "{kind:?} must not touch the level",
        );
    }
}

/// R1610 — the state object, against a fabricated desk.
///
/// Everything above tests a PURE function. Two counterfactuals found what that
/// leaves uncovered: deleting the signal write from `set_level`, and making
/// `apply` reset the level, both left this suite green, because nothing here
/// exercised `DesksState`'s own verbs — the demo was the only thing that did,
/// and a demo does not run under `cargo test`. `Signal::new` needs no `Owner`
/// scope and `DisplayHandle` is constructible, so the state object is a value a
/// test can hold, and there was never a reason for the gap.
fn state_on(desk: DisplayTopology) -> super::DesksState {
    let handle = std::sync::Arc::new(pinion_shell::DisplayHandle::new());
    handle.set(desk);
    super::DesksState::new(handle)
}

#[test]
fn r1610_set_level_writes_the_declaration_the_shell_reconciles() {
    use pinion_core::window_level::WindowLevel;
    let state = state_on(two_panels());
    assert_eq!(
        state.panel_spec().expect("declared").level,
        WindowLevel::Normal
    );

    let echoed = state.set_level("always_on_top").expect("a known level");
    assert_eq!(
        echoed, "always_on_top",
        "the verb echoes the canonical name"
    );
    assert_eq!(
        state.panel_spec().expect("declared").level,
        WindowLevel::AlwaysOnTop,
        "the verb must write the SIGNAL — the shell's level pass reads nothing \
         else, so a verb that only computed would pin nothing",
    );
    // Un-pinning is the same path, and it has to reach the signal too.
    state.set_level("normal").expect("a known level");
    assert_eq!(
        state.panel_spec().expect("declared").level,
        WindowLevel::Normal,
    );
}

#[test]
fn r1610_set_level_refuses_an_unknown_spelling_and_changes_nothing() {
    use pinion_core::window_level::WindowLevel;
    let state = state_on(two_panels());
    state.set_level("always_on_top").expect("a known level");
    let err = state.set_level("sideways").expect_err("not a level");
    assert!(
        format!("{err:?}").contains("is not a window level"),
        "the refusal says what it refused: {err:?}",
    );
    assert_eq!(
        state.panel_spec().expect("declared").level,
        WindowLevel::AlwaysOnTop,
        "a refused verb leaves the declaration exactly as it was",
    );
}

#[test]
fn r1610_applying_a_placement_preset_leaves_the_level_alone() {
    // The counterfactual that found this one added `.with_level(Normal)` to
    // `apply`, and every test passed: a monitoring readout that quietly drops
    // behind the watched application when the user moves it to the other
    // monitor is the bug, and nothing could see it.
    use pinion_core::window_level::WindowLevel;
    let state = state_on(two_panels());
    state.set_level("always_on_top").expect("a known level");
    for (name, _) in PRESETS {
        state
            .apply(name)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(
            state.panel_spec().expect("declared").level,
            WindowLevel::AlwaysOnTop,
            "preset {name} must not touch the level",
        );
    }
}

#[test]
fn r1610_a_window_boots_at_the_normal_level() {
    use pinion_core::window_level::WindowLevel;
    assert_eq!(
        panel().level,
        WindowLevel::Normal,
        "every pre-R1610 window is byte-identical",
    );
}

#[test]
fn r1576_the_rectangle_argument_is_parsed_strictly() {
    assert_eq!(
        parse_rect("10,20,30,40"),
        Some(DisplayRect::new(10, 20, 30, 40))
    );
    assert_eq!(
        parse_rect(" -10 , -20 , 30 , 40 "),
        Some(DisplayRect::new(-10, -20, 30, 40)),
        "a negative origin is ordinary — a display can be left of the primary"
    );
    for malformed in ["", "1,2,3", "1,2,3,4,5", "a,b,c,d", "1,2,-3,4"] {
        assert_eq!(parse_rect(malformed), None, "{malformed:?} is refused");
    }
}

// ---- R1617 §5.16 §5.41 §2 #7 — the home the binding reads ----

#[test]
fn r1617_the_description_tells_three_kinds_of_nothing_apart() {
    use super::describe_home;
    use pinion_core::display::DisplayHome;

    // Nobody looked at all.
    assert_eq!(describe_home(None), "unstamped");
    // Somebody looked and neither answerer named a display. Distinct from the
    // above, and a demo that could not tell them apart would pass on a shell
    // that had stopped stamping entirely.
    assert_eq!(
        describe_home(Some(DisplayHome::between(None, None))),
        "nowhere::",
    );
    // We answered, the window system did not. Also distinct — silence is not
    // concurrence, so this must not read like agreement.
    assert_eq!(
        describe_home(Some(DisplayHome::between(
            Some(DisplayId::new("left")),
            None
        ))),
        "platform_silent:left:",
    );
}

#[test]
fn r1617_a_divergence_is_described_with_both_names() {
    use super::describe_home;
    use pinion_core::display::DisplayHome;
    assert_eq!(
        describe_home(Some(DisplayHome::between(
            Some(DisplayId::new("right")),
            Some(DisplayId::new("left")),
        ))),
        "diverged:right:left",
        "both answers survive the description — picking one would be this \
         binding inventing a rule over the platform's",
    );
    assert_eq!(
        describe_home(Some(DisplayHome::between(
            Some(DisplayId::new("right")),
            Some(DisplayId::new("right")),
        ))),
        "agreed:right:right",
    );
}

#[test]
fn r1617_the_binding_reads_the_home_through_the_handle_it_holds() {
    use super::{MAIN_WINDOW, describe_home};
    // The oracle's read path: a held handle, because an `invoke` / `query` body
    // runs outside any `Owner` scope of this binding's own. The `view` uses the
    // hook instead, and both funnel into one formatter.
    let handle = std::sync::Arc::new(pinion_shell::DisplayHandle::new());
    handle.set(two_panels());
    assert_eq!(
        describe_home(handle.home_of(PANEL_WINDOW)),
        "unstamped",
        "a desk without a window rectangle claims nothing",
    );
    handle.set_homes(vec![
        (
            PANEL_WINDOW.to_owned(),
            // Straddling the seam with 300 of its 400 columns on the right.
            DisplayRect::new(900, 0, 400, 100),
            Some(DisplayId::new("right")),
        ),
        (
            MAIN_WINDOW.to_owned(),
            DisplayRect::new(10, 10, 200, 200),
            Some(DisplayId::new("right")),
        ),
    ]);
    assert_eq!(
        describe_home(handle.home_of(PANEL_WINDOW)),
        "agreed:right:right"
    );
    assert_eq!(
        describe_home(handle.home_of(MAIN_WINDOW)),
        "diverged:left:right",
        "wholly on the left panel while the window system names the right one",
    );
}
