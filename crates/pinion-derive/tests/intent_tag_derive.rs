//! Fixture tests for `#[derive(IntentTag)]`. Covers the supported v0
//! variant shapes (unit, bool, int, float, string) plus the
//! `schema()` and `from_intent()` outputs the macro generates.

use pinion_core::external::IntrospectValue;
use pinion_core::intent::{Intent, IntentTag};
use pinion_derive::IntentTag;

#[derive(Debug, PartialEq, IntentTag)]
enum ButtonIntentFixture {
    #[tag("save_btn.click")]
    Click,
    #[tag("save_btn.toggle")]
    Toggle(bool),
    #[tag("counter.set")]
    Set(i64),
    #[tag("slider.value")]
    Value(f64),
    #[tag("input.label")]
    Label(String),
}

#[test]
fn schema_lists_all_variants_with_payload_type_names() {
    assert_eq!(
        ButtonIntentFixture::schema(),
        &[
            ("save_btn.click", "void"),
            ("save_btn.toggle", "bool"),
            ("counter.set", "int"),
            ("slider.value", "float"),
            ("input.label", "string"),
        ],
    );
}

#[test]
fn const_tag_returns_per_variant_label() {
    assert_eq!(ButtonIntentFixture::Click.const_tag(), "save_btn.click");
    assert_eq!(
        ButtonIntentFixture::Toggle(true).const_tag(),
        "save_btn.toggle",
    );
    assert_eq!(ButtonIntentFixture::Set(7).const_tag(), "counter.set");
    assert_eq!(ButtonIntentFixture::Value(1.5).const_tag(), "slider.value",);
    assert_eq!(
        ButtonIntentFixture::Label("hi".to_string()).const_tag(),
        "input.label",
    );
}

#[test]
fn from_intent_round_trips_unit_variant() {
    let intent = Intent::new_static("save_btn.click", IntrospectValue::Null);
    assert_eq!(
        ButtonIntentFixture::from_intent(&intent),
        Some(ButtonIntentFixture::Click),
    );
}

#[test]
fn from_intent_round_trips_bool_int_float_string_payloads() {
    let bool_in = Intent::new_static("save_btn.toggle", IntrospectValue::Bool(true));
    assert_eq!(
        ButtonIntentFixture::from_intent(&bool_in),
        Some(ButtonIntentFixture::Toggle(true)),
    );

    let int_in = Intent::new_static("counter.set", IntrospectValue::Int(42));
    assert_eq!(
        ButtonIntentFixture::from_intent(&int_in),
        Some(ButtonIntentFixture::Set(42)),
    );

    let float_in = Intent::new_static("slider.value", IntrospectValue::Float(1.5));
    assert_eq!(
        ButtonIntentFixture::from_intent(&float_in),
        Some(ButtonIntentFixture::Value(1.5)),
    );

    let text_in = Intent::new_static("input.label", IntrospectValue::Text("hello".to_string()));
    assert_eq!(
        ButtonIntentFixture::from_intent(&text_in),
        Some(ButtonIntentFixture::Label("hello".to_string())),
    );
}

#[test]
fn from_intent_rejects_payload_type_mismatch() {
    let bad = Intent::new_static("counter.set", IntrospectValue::Bool(true));
    assert!(ButtonIntentFixture::from_intent(&bad).is_none());
}

#[test]
fn from_intent_rejects_unknown_tag() {
    let bad = Intent::new_static("ghost.path", IntrospectValue::Null);
    assert!(ButtonIntentFixture::from_intent(&bad).is_none());
}

#[test]
fn unit_variant_rejects_non_null_payload() {
    let bad = Intent::new_static("save_btn.click", IntrospectValue::Int(0));
    assert!(ButtonIntentFixture::from_intent(&bad).is_none());
}
