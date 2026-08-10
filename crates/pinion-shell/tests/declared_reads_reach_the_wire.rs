//! R1637 §2 #2 §2 #7 — a path a surface DECLARES on the read channel is a path
//! `scene/query` answers.
//!
//! R1637 made the declaration a precondition of both channels, so `$schema` and
//! the wire can no longer disagree in the direction that used to hurt (a name
//! answered and never published). This closes the other direction for a real
//! widget: a declaration that stops being written stops being readable, and
//! until now nothing would have said so.
//!
//! It is here rather than in `pinion-widget-paint` because that crate does not
//! depend on `pinion-rpc` — the two only meet at the shell. That distance is
//! exactly why the gap existed: every `lifecycle` assertion in the tree calls
//! `ExternalIntrospect::query` **directly**, which does not pass through the
//! transport and therefore cannot see the declaration at all
//! ([[debt-in-process-dispatch-bypasses-the-declaration]]). R1637's own
//! counterfactual found it: deleting dock's `lifecycle` declaration left
//! `cargo test` entirely green.

use pinion_core::Scene;
use pinion_core::external::{ExternalIntrospect, IntrospectValue, InvokeError, SchemaChannel};
use pinion_core::scene::ExternalNode;
use pinion_widget_paint::dock::DockPanelExternal;

/// Every argument-free read the panel declares answers over the real
/// `scene/query` dispatcher — the declaration and the wire, compared.
///
/// Parametric families are skipped by declaration, not by name: their template
/// is not a readable address (`cell.<row>` addresses nothing), which is the
/// same reason `scene/snapshot` omits them.
#[test]
fn r1637_every_declared_read_on_a_real_widget_answers_over_the_wire() {
    let panel = DockPanelExternal::new("a");
    let declared: Vec<&'static str> = panel
        .schema()
        .fields
        .iter()
        .filter(|f| f.channel == SchemaChannel::Read && f.args.is_empty())
        .map(|f| f.path)
        .collect();
    assert!(
        declared.len() > 5,
        "the fixture must have a real contract to check: {declared:?}"
    );
    assert!(
        declared.contains(&"lifecycle"),
        "the path whose own doc calls it the §2 #7 surface: {declared:?}"
    );

    let scene = Scene::External(ExternalNode::new(Box::new(DockPanelExternal::new("a"))));
    // Collected rather than asserted one at a time: the first run of this test
    // found three mis-declared paths, and a per-path assert would have reported
    // one, been fixed, and reported the next. A contract check should say how
    // far the contract is from the surface, not where the walk happened to stop.
    let refused: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|path| pinion_rpc::query(&scene, &format!("/external/{path}")).is_err())
        .collect();
    assert!(
        refused.is_empty(),
        "declared readable, and the wire refuses them: {refused:?}",
    );

    // And the same surface's declared ACTIONS are refused by the read channel
    // rather than answered — the channel split, checked on a real widget rather
    // than only on a fixture.
    let actions: Vec<&'static str> = panel
        .schema()
        .fields
        .iter()
        .filter(|f| f.channel == SchemaChannel::Invoke)
        .map(|f| f.path)
        .collect();
    assert!(!actions.is_empty(), "this panel drives through invoke");
    for path in &actions {
        assert_eq!(
            pinion_rpc::query(&scene, &format!("/external/{path}")).unwrap_err(),
            pinion_rpc::QueryError::PathIsAnAction,
            "{path:?} is an action, and a reader is told so",
        );
    }

    // The lifecycle value itself, so a declaration that survives while the
    // answer rots is still caught.
    assert_eq!(
        pinion_rpc::query(&scene, "/external/lifecycle").unwrap(),
        IntrospectValue::Text("Docked".to_owned()),
    );
}

/// Across the widget catalog: a field declared on the READ channel is not one
/// the surface's own `invoke` recognises.
///
/// # Why this oracle and not "the read answers"
///
/// The obvious check — every declared read answers `scene/query` — cannot tell
/// a mis-declared channel from an **unbound** surface. `TextFieldExternal::new()`
/// answers `None` for `text` / `caret` / `selection` and thirteen more, and its
/// own doc says why: "`None` when no handle is attached; the AI client treats
/// that as widget not bound to reactive state". Written that way this test
/// reported all fourteen as defects on its first run, which is a fixture that
/// cannot discriminate rather than a finding.
///
/// Asking the surface's DISPATCH instead is exact and needs no binding: if a
/// name declared readable is one `invoke` knows, the declaration names the
/// wrong channel — whatever state the widget is in. The call goes to the trait
/// directly on purpose, because the transport gate now answers
/// `PathIsAReadSlot` from the declaration itself and would agree with any
/// declaration, right or wrong.
///
/// Scoped to the externals a test can construct with no model. That is a stated
/// limit, not a coverage claim: a surface needing a `Signal`, a row model or a
/// tag is absent here and its declaration is unchecked.
/// Every widget external a test can build with no model, boxed to the trait.
///
/// One list, because two tests now walk it and a second copy would let them
/// drift into checking different populations while reporting the same coverage.
fn catalog() -> Vec<(&'static str, Box<dyn pinion_core::external::External>)> {
    use pinion_core::widgets::{
        badge::BadgeExternal, button::ButtonExternal, checkbox::CheckboxExternal,
        color_area::ColorAreaExternal, context_menu::ContextMenuExternal,
        disclosure::DisclosureExternal, disclosure_group::DisclosureGroupExternal,
        key_sequence::KeySequenceEditExternal, listbox_item::ListBoxItemExternal,
        pagination::PaginationExternal, radio::RadioExternal, range_slider::RangeSliderExternal,
        scrollbar::ScrollBarExternal, slider::SliderExternal, spin_button::SpinButtonExternal,
        text_field::TextFieldExternal, toggle::ToggleExternal, tooltip::TooltipExternal,
    };
    vec![
        ("badge", ext_scene(BadgeExternal::new())),
        ("button", ext_scene(ButtonExternal::new())),
        ("checkbox", ext_scene(CheckboxExternal::new())),
        ("color_area", ext_scene(ColorAreaExternal::new())),
        ("context_menu", ext_scene(ContextMenuExternal::new(3))),
        ("disclosure", ext_scene(DisclosureExternal::new())),
        (
            "disclosure_group",
            ext_scene(DisclosureGroupExternal::new(3)),
        ),
        ("key_sequence", ext_scene(KeySequenceEditExternal::new())),
        ("listbox_item", ext_scene(ListBoxItemExternal::new())),
        ("pagination", ext_scene(PaginationExternal::new(9, 0))),
        ("radio", ext_scene(RadioExternal::new())),
        ("range_slider", ext_scene(RangeSliderExternal::new())),
        ("scrollbar", ext_scene(ScrollBarExternal::new())),
        ("slider", ext_scene(SliderExternal::new())),
        (
            "spin_button",
            ext_scene(SpinButtonExternal::new(1.0, 0.0, 10.0, 1.0)),
        ),
        ("text_field", ext_scene(TextFieldExternal::new())),
        ("toggle", ext_scene(ToggleExternal::new())),
        ("tooltip", ext_scene(TooltipExternal::new())),
    ]
}

#[test]
fn r1637_a_declared_read_is_not_a_name_the_surface_dispatches() {
    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for (label, handle) in catalog() {
        let mut handle = handle;
        let intro = handle
            .introspect_mut()
            .expect("every entry opts into introspection");
        let reads: Vec<&'static str> = intro
            .schema()
            .fields
            .iter()
            .filter(|f| f.channel == SchemaChannel::Read && f.args.is_empty())
            .map(|f| f.path)
            .collect();
        for path in reads {
            checked += 1;
            if !matches!(
                intro.invoke(path, IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            ) {
                wrong.push(format!("{label}.{path}"));
            }
        }
    }
    assert!(checked > 60, "the walk must be non-trivial, saw {checked}");
    assert!(
        wrong.is_empty(),
        "declared on the read channel, and the surface's own `invoke` knows \
         them: {wrong:?}",
    );
}

fn ext_scene<E: pinion_core::external::External + 'static>(
    ext: E,
) -> Box<dyn pinion_core::external::External> {
    Box::new(ext)
}

/// R1638 — an **optional** argument may not precede a required one.
///
/// The rule is forced by the only form that can elide anything: a delimited
/// payload drops trailing segments, so a gap in the middle would be
/// unrepresentable — `"3::l"` cannot say "no event, buttons `l`" because the
/// decoder counts positions. An object form does not need the rule, and obeying
/// it there costs nothing while keeping one statement true of every form.
///
/// Checked over every surface this test can construct, plus the send grammar
/// itself, which is the declaration the rule was written for and the one every
/// composite widget points at.
#[test]
fn r1638_optional_arguments_are_a_suffix() {
    use pinion_core::composite_tag::SEND_ARGS;

    fn suffix_ok(args: &[pinion_core::external::SchemaArg]) -> bool {
        let first_optional = args.iter().position(|a| a.optional);
        match first_optional {
            None => true,
            Some(i) => args[i..].iter().all(|a| a.optional),
        }
    }

    assert!(suffix_ok(SEND_ARGS), "the send grammar: {SEND_ARGS:?}");

    let mut checked = 0_usize;
    let mut wrong: Vec<String> = Vec::new();
    let mut surfaces: Vec<(&str, Box<dyn pinion_core::external::External>)> =
        vec![("dock_panel", Box::new(DockPanelExternal::new("a")))];
    surfaces.extend(catalog());
    for (label, handle) in surfaces {
        let intro = handle.introspect().expect("opts into introspection");
        for field in intro.schema().fields {
            checked += 1;
            if !suffix_ok(field.args) {
                wrong.push(format!("{label}.{}", field.path));
            }
        }
    }
    assert!(checked > 100, "the walk must be non-trivial, saw {checked}");
    assert!(
        wrong.is_empty(),
        "an optional argument precedes a required one: {wrong:?}",
    );
}
