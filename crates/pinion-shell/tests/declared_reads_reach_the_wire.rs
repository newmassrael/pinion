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
#[test]
fn r1637_a_declared_read_is_not_a_name_the_surface_dispatches() {
    use pinion_core::widgets::{
        badge::BadgeExternal, button::ButtonExternal, checkbox::CheckboxExternal,
        color_area::ColorAreaExternal, context_menu::ContextMenuExternal,
        disclosure::DisclosureExternal, disclosure_group::DisclosureGroupExternal,
        key_sequence::KeySequenceEditExternal, listbox_item::ListBoxItemExternal,
        pagination::PaginationExternal, radio::RadioExternal, range_slider::RangeSliderExternal,
        scrollbar::ScrollBarExternal, slider::SliderExternal, spin_button::SpinButtonExternal,
        text_field::TextFieldExternal, toggle::ToggleExternal, tooltip::TooltipExternal,
    };

    // (label, one boxed surface). Boxed per entry because each is a different
    // type and the walk only needs the trait.
    let surfaces: Vec<(&str, Box<dyn pinion_core::external::External>)> = vec![
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
    ];

    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for (label, handle) in surfaces {
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
