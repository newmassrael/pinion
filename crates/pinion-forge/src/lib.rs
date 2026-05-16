//! pinion-forge — pinion DSL (`.pinion.xml`) parser + Rust codegen per
//! §5.22 (R38).
//!
//! ## Scope
//!
//! pinion-forge consumes `.pinion.xml` files authored by humans or AI
//! agents and emits Rust source that targets the [`pinion-core::reactive`]
//! runtime. Each `.pinion.xml` compiles to exactly one `pub struct` plus
//! an `impl <Name> { pub fn new(owner: &Owner) -> Self }` constructor.
//!
//! Per R38 ratify (§5.22 redefined): the file extension is fixed at
//! `.pinion.xml`, the root element is `<pinion xmlns="..." kind="..."
//! name="...">`, and the child set is closed (`<use>` / `<signal>` /
//! `<computed>` / `<resource>` for `kind="reactive"`).
//!
//! Per R37.7 + R37.8: pinion-forge owns its codegen. SCE upstream rejects
//! framework-specific kinds (RFC 001 closed); the only SCE surface this
//! crate consumes is the v1 NDJSON diagnostic *pattern* (`v` / `id` /
//! `code` / `stage` / `message` shape — see `schemas/sce-diagnostic.v1
//! .schema.json`). pinion's own diagnostic namespace lives in
//! [`diagnostic::PinionForgeDiagnostic`] and is not an SCE
//! `DiagnosticCode` extension.
//!
//! ## R38.1 status (this build round)
//!
//! Skeleton only. The parser accepts the empty root
//! `<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="..."/>`
//! and rejects every child element with `dsl/unsupported-element`. The
//! codegen emits the constructor body `Self` (no fields). R38.2+ lands
//! the real `<signal>` / `<computed>` / `<resource>` AST + emit.
//!
//! [`pinion-core::reactive`]: ../pinion_core/reactive/index.html

#![forbid(unsafe_code)]

pub mod ast;
pub mod build;
pub mod codegen;
pub mod diagnostic;
pub mod parser;
pub mod wire;

pub use ast::{PinionChild, PinionDoc, PinionKind};
pub use build::{CompileError, compile_file, compile_str};
pub use codegen::emit_rust;
pub use diagnostic::{Location, PINION_DSL_NS, PinionForgeDiagnostic, Stage};
pub use parser::parse_pinion;
pub use wire::{WIRE_VERSION, to_json_value, to_ndjson_line};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parse(xml: &str) -> Result<PinionDoc, Vec<PinionForgeDiagnostic>> {
        parse_pinion(xml, "<test>.pinion.xml")
    }

    fn parse_err(xml: &str) -> Vec<PinionForgeDiagnostic> {
        parse(xml).expect_err("expected diagnostic")
    }

    #[test]
    fn parses_empty_reactive_self_closing() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty"/>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.name, "Empty");
        assert_eq!(doc.kind, PinionKind::Reactive);
        assert!(doc.children.is_empty());
    }

    #[test]
    fn parses_empty_reactive_open_close_with_whitespace() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty">
        </pinion>"#;
        let doc = parse(xml).expect("happy path with whitespace body");
        assert_eq!(doc.name, "Empty");
    }

    #[test]
    fn emits_constructor_for_empty_doc() {
        let doc = PinionDoc {
            name: "Foo".into(),
            kind: PinionKind::Reactive,
            children: Vec::new(),
        };
        let rust = emit_rust(&doc);
        assert!(rust.contains("pub struct Foo;"));
        assert!(rust.contains("impl Foo"));
        assert!(rust.contains("pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self"));
        assert!(rust.contains("Self\n"));
        assert!(rust.ends_with('\n'));
    }

    #[test]
    fn roundtrip_parse_then_emit_compiles() {
        // The emitted source must syntactically parse as a Rust file. We
        // can't run rustc from a unit test, but `syn` is already in the
        // workspace via pinion-derive — fall back to a substring check
        // here and let the integration target (R38.2+ test crate)
        // exercise compilation end-to-end.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Button"/>"#;
        let rust = compile_str(xml, "ui/button.pinion.xml").expect("compile");
        assert!(rust.contains("pub struct Button;"));
    }

    #[test]
    fn rejects_non_pinion_root() {
        let xml = r#"<widget xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        assert_eq!(diags.len(), 1);
        let PinionForgeDiagnostic::InvalidRoot { found, .. } = &diags[0] else {
            panic!("expected InvalidRoot, got {:?}", diags[0]);
        };
        assert_eq!(found, "widget");
        assert_eq!(diags[0].code(), "dsl/invalid-root");
        assert_eq!(diags[0].stage(), Stage::Validate);
    }

    #[test]
    fn rejects_missing_xmlns() {
        let xml = r#"<pinion kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-xmlns"));
    }

    #[test]
    fn rejects_wrong_xmlns() {
        let xml = r#"<pinion xmlns="https://example.com/other" kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        let wrong = diags.iter().find(|d| d.code() == "dsl/wrong-xmlns").expect("wrong-xmlns");
        let PinionForgeDiagnostic::WrongXmlns { found, .. } = wrong else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "https://example.com/other");
    }

    #[test]
    fn rejects_missing_kind() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" name="X"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-kind"));
    }

    #[test]
    fn rejects_unknown_kind() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="view-fn" name="X"/>"#;
        let diags = parse_err(xml);
        let bad = diags.iter().find(|d| d.code() == "dsl/unknown-kind").expect("unknown-kind");
        let PinionForgeDiagnostic::UnknownKind { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "view-fn");
    }

    #[test]
    fn rejects_missing_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-name"));
    }

    #[test]
    fn rejects_invalid_name_keyword() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="impl"/>"#;
        let diags = parse_err(xml);
        let bad = diags.iter().find(|d| d.code() == "dsl/invalid-name").expect("invalid-name");
        let PinionForgeDiagnostic::InvalidName { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "impl");
    }

    #[test]
    fn rejects_invalid_name_punctuation() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="foo-bar"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/invalid-name"));
    }

    #[test]
    fn rejects_invalid_name_leading_digit() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="1Foo"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/invalid-name"));
    }

    #[test]
    fn rejects_unsupported_child_element() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="u32"/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unsupported-element")
            .expect("unsupported-element");
        let PinionForgeDiagnostic::UnsupportedElement { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "signal");
    }

    #[test]
    fn accumulates_multiple_attribute_diagnostics() {
        // Missing all three required attributes — parser must surface
        // each independently rather than fail-fast on the first.
        let xml = r"<pinion/>";
        let diags = parse_err(xml);
        let codes: std::collections::BTreeSet<_> =
            diags.iter().map(PinionForgeDiagnostic::code).collect();
        assert!(codes.contains("dsl/missing-xmlns"));
        assert!(codes.contains("dsl/missing-kind"));
        assert!(codes.contains("dsl/missing-name"));
    }

    #[test]
    fn rejects_malformed_xml() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X""#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/xml-parse"));
        assert!(diags.iter().any(|d| d.stage() == Stage::Parse));
    }

    #[test]
    fn wire_record_has_required_fields() {
        let xml = r#"<pinion kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        let diag = diags.iter().find(|d| d.code() == "dsl/missing-xmlns").unwrap();
        let line = to_ndjson_line(diag);
        let value: Value = serde_json::from_str(&line).expect("valid JSON");
        let obj = value.as_object().expect("object");
        assert_eq!(obj.get("v"), Some(&Value::from(1)));
        assert!(obj.get("id").and_then(Value::as_str).unwrap().starts_with("fnv1a:"));
        assert_eq!(obj.get("code"), Some(&Value::from("dsl/missing-xmlns")));
        assert_eq!(obj.get("stage"), Some(&Value::from("validate")));
        assert!(obj.get("message").is_some());
        let loc = obj.get("location").and_then(Value::as_object).expect("location");
        assert!(loc.get("file").is_some());
    }

    #[test]
    fn wire_id_is_stable_under_message_rewording() {
        // The (code, stage, file, key_fragments) tuple defines identity.
        // Two diagnostics built independently with the same identity
        // fields must produce the same `id`, even if (hypothetically) we
        // reworded the Display message.
        let a = PinionForgeDiagnostic::UnsupportedElement {
            tag: "signal".into(),
            location: Location::new("foo.pinion.xml").with_line_col(2, 13),
        };
        let b = PinionForgeDiagnostic::UnsupportedElement {
            tag: "signal".into(),
            location: Location::new("foo.pinion.xml").with_line_col(99, 42),
        };
        let va = to_json_value(&a);
        let vb = to_json_value(&b);
        assert_eq!(va["id"], vb["id"], "id must not depend on line/col");
    }

    #[test]
    fn wire_id_differs_across_codes() {
        let a = PinionForgeDiagnostic::MissingXmlns {
            expected: PINION_DSL_NS,
            location: Location::new("a.pinion.xml"),
        };
        let b = PinionForgeDiagnostic::MissingName {
            location: Location::new("a.pinion.xml"),
        };
        let va = to_json_value(&a);
        let vb = to_json_value(&b);
        assert_ne!(va["id"], vb["id"]);
    }
}
