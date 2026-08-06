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
