//! §5.20 intent system — bidirectional symbolic channel from widgets to
//! the framework, mirror of the `scene/invoke` action channel.
//!
//! An [`Intent`] is a symbolic event drawn out of a widget after its
//! state machine advances: a tag plus a typed payload. The framework
//! collects intents from [`External`](crate::external::External)
//! impls via the §5.20 drain protocol (slice 3) and surfaces them
//! to AI agents through the `scene/intents` JSON-RPC method (slice 4).
//!
//! Authors normally describe intents via `#[derive(IntentTag)]` on an
//! enum (`pinion-derive` crate). For one-off cases — or fixtures
//! exercising the trait surface itself — a manual `impl` is supported.
//! The trait contract:
//!
//!   * [`IntentTag::const_tag`] — variant → tag string at zero cost.
//!   * [`IntentTag::from_intent`] — recover the typed enum from a
//!     wire-form [`Intent`]; `None` for tag/payload mismatch.
//!   * [`IntentTag::schema`] — static list of `(tag, payload_type)`
//!     pairs the type can emit.
//!
//! Tag convention is `<widget>.<kind>` (e.g. `save_btn.click`).
//! Payload types in the schema use the names `void` / `bool` / `int` /
//! `float` / `string`, parallel to the `IntrospectSchema` field
//! type-name strings.

use core::borrow::Borrow;
use std::borrow::Cow;

use crate::external::IntrospectValue;

/// Wire-form intent envelope drained from a widget.
///
/// `tag` is the symbolic discriminator (string, borrowed-or-owned to
/// keep zero-alloc emission viable when the tag is a `&'static str`).
/// `payload` carries the typed argument matching the variant in the
/// [`IntentTag`] schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Intent {
    pub tag: Cow<'static, str>,
    pub payload: IntrospectValue,
}

impl Intent {
    /// Construct an intent with a static tag and a payload.
    #[must_use]
    pub const fn new_static(tag: &'static str, payload: IntrospectValue) -> Self {
        Self {
            tag: Cow::Borrowed(tag),
            payload,
        }
    }

    /// Construct an intent with an owned tag string.
    #[must_use]
    pub fn new_owned(tag: String, payload: IntrospectValue) -> Self {
        Self {
            tag: Cow::Owned(tag),
            payload,
        }
    }

    /// Tag as a `&str`.
    #[must_use]
    pub fn tag_str(&self) -> &str {
        self.tag.borrow()
    }
}

/// Typed view of a widget's intent vocabulary.
///
/// Hand-implement when the variant shapes are exotic; otherwise use
/// `#[derive(IntentTag)]` from the `pinion-derive` crate.
pub trait IntentTag: Sized {
    /// Tag string for the current variant — `O(1)` lookup, no
    /// allocation.
    fn const_tag(&self) -> &'static str;

    /// Recover a typed enum value from a wire-form intent. `None` when
    /// the tag is unknown or the payload variant does not match the
    /// schema for that tag.
    fn from_intent(intent: &Intent) -> Option<Self>;

    /// Static map of `(tag, payload_type)` pairs this implementor can
    /// emit. Payload type names: `"void"`, `"bool"`, `"int"`,
    /// `"float"`, `"string"`.
    fn schema() -> &'static [(&'static str, &'static str)];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-rolled `IntentTag` impl — exercises the trait surface
    /// directly without depending on the `pinion-derive` macro. The
    /// derive crate has its own fixture tests; this guards the
    /// trait/Intent contract on its own.
    #[derive(Debug, PartialEq)]
    enum DemoIntent {
        Click,
        Toggle(bool),
        Label(String),
    }

    impl IntentTag for DemoIntent {
        fn const_tag(&self) -> &'static str {
            match self {
                DemoIntent::Click => "demo.click",
                DemoIntent::Toggle(_) => "demo.toggle",
                DemoIntent::Label(_) => "demo.label",
            }
        }

        fn from_intent(intent: &Intent) -> Option<Self> {
            match intent.tag_str() {
                "demo.click" => match &intent.payload {
                    IntrospectValue::Null => Some(DemoIntent::Click),
                    _ => None,
                },
                "demo.toggle" => match &intent.payload {
                    IntrospectValue::Bool(b) => Some(DemoIntent::Toggle(*b)),
                    _ => None,
                },
                "demo.label" => match &intent.payload {
                    IntrospectValue::Text(s) => Some(DemoIntent::Label(s.clone())),
                    _ => None,
                },
                _ => None,
            }
        }

        fn schema() -> &'static [(&'static str, &'static str)] {
            &[
                ("demo.click", "void"),
                ("demo.toggle", "bool"),
                ("demo.label", "string"),
            ]
        }
    }

    #[test]
    fn const_tag_returns_per_variant_label() {
        assert_eq!(DemoIntent::Click.const_tag(), "demo.click");
        assert_eq!(DemoIntent::Toggle(true).const_tag(), "demo.toggle");
        assert_eq!(
            DemoIntent::Label("x".to_string()).const_tag(),
            "demo.label",
        );
    }

    #[test]
    fn from_intent_round_trips_unit_variant() {
        let intent = Intent::new_static("demo.click", IntrospectValue::Null);
        assert_eq!(DemoIntent::from_intent(&intent), Some(DemoIntent::Click));
    }

    #[test]
    fn from_intent_round_trips_bool_variant() {
        let intent = Intent::new_static("demo.toggle", IntrospectValue::Bool(true));
        assert_eq!(
            DemoIntent::from_intent(&intent),
            Some(DemoIntent::Toggle(true)),
        );
    }

    #[test]
    fn from_intent_round_trips_string_variant() {
        let intent = Intent::new_static(
            "demo.label",
            IntrospectValue::Text("hi".to_string()),
        );
        assert_eq!(
            DemoIntent::from_intent(&intent),
            Some(DemoIntent::Label("hi".to_string())),
        );
    }

    #[test]
    fn from_intent_rejects_payload_type_mismatch() {
        let intent = Intent::new_static("demo.toggle", IntrospectValue::Int(1));
        assert!(DemoIntent::from_intent(&intent).is_none());
    }

    #[test]
    fn from_intent_rejects_unknown_tag() {
        let intent = Intent::new_static("ghost.tag", IntrospectValue::Null);
        assert!(DemoIntent::from_intent(&intent).is_none());
    }

    #[test]
    fn schema_lists_all_variants_with_payload_types() {
        assert_eq!(
            DemoIntent::schema(),
            &[
                ("demo.click", "void"),
                ("demo.toggle", "bool"),
                ("demo.label", "string"),
            ],
        );
    }

    #[test]
    fn owned_tag_constructor_preserves_payload() {
        let intent = Intent::new_owned(
            String::from("demo.click"),
            IntrospectValue::Null,
        );
        assert_eq!(intent.tag_str(), "demo.click");
        assert_eq!(intent.payload, IntrospectValue::Null);
    }
}
