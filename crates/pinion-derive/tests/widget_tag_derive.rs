//! R650 §5.16 — substrate-only fixture for `#[derive(WidgetTag)]`.
//!
//! Pins the `PascalCase` → `snake_case` converter contract (including
//! the digit-as-lowercase-letter convention `DesignButtonM3 →
//! design_button_m3`), the round-trip between `as_tag` and `from_tag`,
//! and the unknown-tag rejection. R650 walked back the single-tag
//! binding adoption (`hello-button` + `design-button-m3` Tags enums)
//! per [[abstraction-needs-second-consumer]]; the substrate itself
//! stays land per [[textbook-long-term-correct]] for the composite
//! widget consumer. Without this fixture the substrate would be
//! untested until that 2nd consumer appears, so the round-trip pins
//! live here.
//!
//! Cross-ref: R644 originally placed these assertions inside the
//! `hello-button` + `design-button-m3` test modules; that placement
//! coupled substrate test coverage to binding adoption, which made
//! the R650 walk-back leave the trait wholly untested. This file is
//! the correct home — the derive macro is what is being verified, so
//! the test lives next to the derive macro.

use pinion_core::WidgetTag;
use pinion_derive::WidgetTag;

#[derive(Copy, Clone, Eq, PartialEq, Debug, WidgetTag)]
enum SingleTag {
    MainBtn,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, WidgetTag)]
enum MultiTag {
    MainBtn,
    ScrollBar,
    DesignButtonM3,
}

#[test]
fn single_variant_round_trip() {
    assert_eq!(SingleTag::MainBtn.as_tag(), "main_btn");
    assert_eq!(SingleTag::from_tag("main_btn"), Some(SingleTag::MainBtn));
    assert_eq!(SingleTag::from_tag("unknown"), None);
}

#[test]
fn pascal_to_snake_case_basic() {
    assert_eq!(MultiTag::MainBtn.as_tag(), "main_btn");
    assert_eq!(MultiTag::ScrollBar.as_tag(), "scroll_bar");
}

#[test]
fn pascal_to_snake_case_digit_no_underscore() {
    // ASCII digits are treated as lowercase letters per the converter
    // contract — `M3` becomes `m3`, NOT `m_3`. A regression here would
    // silently rename design-button-m3 binding's wire-tag.
    assert_eq!(MultiTag::DesignButtonM3.as_tag(), "design_button_m3");
}

#[test]
fn from_tag_inverse_for_every_variant() {
    assert_eq!(MultiTag::from_tag("main_btn"), Some(MultiTag::MainBtn));
    assert_eq!(MultiTag::from_tag("scroll_bar"), Some(MultiTag::ScrollBar));
    assert_eq!(
        MultiTag::from_tag("design_button_m3"),
        Some(MultiTag::DesignButtonM3),
    );
}

#[test]
fn from_tag_rejects_unknown() {
    assert_eq!(MultiTag::from_tag(""), None);
    assert_eq!(MultiTag::from_tag("nope"), None);
    // PascalCase input is wire-form invalid — only the snake_case
    // output of `as_tag` is a legitimate input to `from_tag`.
    assert_eq!(MultiTag::from_tag("MainBtn"), None);
}
