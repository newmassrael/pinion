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
/// R1640 — **every** widget external in `pinion_core::widgets`, boxed to the
/// trait, with a minimal model where the type needs one.
///
/// One list, because three tests walk it and a second copy would let them drift
/// into checking different populations while reporting the same coverage. The
/// list is held to the source by
/// `r1640_the_catalog_is_every_widget_external`, so a widget added later cannot
/// slip past these checks by simply not being written down — which is how it
/// stood until R1640: the walk built the nineteen types whose constructors took
/// nothing, and the other twenty were invisible to it AND to any reader, since
/// nothing stated the denominator.
///
/// The models are deliberately tiny and meaningless. Nothing here asserts on
/// behaviour; what is under test is each surface's DECLARATION, which is a
/// property of the type rather than of the data it holds.
// A list of thirty-nine constructors is thirty-nine lines of data, and the
// lint's ceiling is about control flow. Splitting it would put the
// completeness this file gates across two places, which is the drift the
// gate exists to prevent.
#[allow(clippy::too_many_lines)]
fn catalog() -> Vec<(&'static str, Box<dyn pinion_core::external::External>)> {
    use pinion_core::directory::InMemoryDirectory;
    use pinion_core::widgets::{
        badge::BadgeExternal,
        button::{ButtonExternal, ButtonState, ButtonStateSnapshot},
        checkbox::CheckboxExternal,
        color_area::ColorAreaExternal,
        column_widths::{ColumnResizeExternal, ColumnWidthExternal, ColumnWidths},
        completion::{CompleterExternal, CompletionState},
        context_menu::ContextMenuExternal,
        datepicker::DatePickerExternal,
        disclosure::DisclosureExternal,
        disclosure_group::DisclosureGroupExternal,
        file_browser::{DirectoryExternal, DirectoryState},
        grid_sort::{GridSortExternal, GridSortState},
        group_order::{GroupOrderExternal, GroupOrderState},
        key_sequence::KeySequenceEditExternal,
        listbox::ListBoxExternal,
        listbox_item::ListBoxItemExternal,
        menu::MenuBarExternal,
        pagination::PaginationExternal,
        progress_bar::ProgressBarExternal,
        radio::RadioExternal,
        radio_group::RadioGroupExternal,
        range_slider::RangeSliderExternal,
        row_dissect::{RowDissectionExternal, RowDissectionState},
        row_search::{RowSearchExternal, RowSearchState},
        row_style::{RowStyleExternal, RowStyleState},
        scroll::ScrollState,
        scrollbar::ScrollBarExternal,
        slider::SliderExternal,
        spin_button::SpinButtonExternal,
        table::TableExternal,
        text_field::TextFieldExternal,
        toggle::{ToggleExternal, ToggleState, ToggleStateSnapshot},
        toolbar::{ToolItem, ToolbarExternal},
        tooltip::TooltipExternal,
        tree_filter::{TreeFilterExternal, TreeFilterState},
        view_order::{ViewOrderState, ViewSortFilterExternal},
        virtual_select::VirtualSelectExternal,
    };
    use std::rc::Rc;

    let cells = || vec![vec!["a".to_owned()], vec!["b".to_owned()]];
    let widths = || Rc::new(ColumnWidths::new(vec![80, 80]));
    vec![
        ("BadgeExternal", ext_scene(BadgeExternal::new())),
        ("ButtonExternal", ext_scene(ButtonExternal::new())),
        (
            "ButtonStateSnapshot",
            ext_scene(ButtonStateSnapshot::new(ButtonState::Idle)),
        ),
        ("CheckboxExternal", ext_scene(CheckboxExternal::new())),
        ("ColorAreaExternal", ext_scene(ColorAreaExternal::new())),
        (
            "ColumnResizeExternal",
            ext_scene(ColumnResizeExternal::new(
                widths(),
                0,
                Rc::new(ScrollState::new()),
                "grid",
            )),
        ),
        (
            "ColumnWidthExternal",
            ext_scene(ColumnWidthExternal::new(widths())),
        ),
        (
            "CompleterExternal",
            ext_scene(CompleterExternal::new(Rc::new(CompletionState::new(vec![
                "alpha".to_owned(),
            ])))),
        ),
        (
            "ContextMenuExternal",
            ext_scene(ContextMenuExternal::new(3)),
        ),
        (
            "DatePickerExternal",
            ext_scene(DatePickerExternal::new(2026, 8, None)),
        ),
        (
            "DirectoryExternal",
            ext_scene(DirectoryExternal::new(Rc::new(DirectoryState::new(
                Rc::new(InMemoryDirectory::new()),
                "/",
            )))),
        ),
        ("DisclosureExternal", ext_scene(DisclosureExternal::new())),
        (
            "DisclosureGroupExternal",
            ext_scene(DisclosureGroupExternal::new(3)),
        ),
        (
            "GridSortExternal",
            ext_scene(GridSortExternal::new(Rc::new(GridSortState::new(
                1,
                cells(),
            )))),
        ),
        (
            "GroupOrderExternal",
            ext_scene(GroupOrderExternal::new(Rc::new(GroupOrderState::new(
                vec![0, 1],
                vec!["g0".to_owned(), "g1".to_owned()],
            )))),
        ),
        (
            "KeySequenceEditExternal",
            ext_scene(KeySequenceEditExternal::new()),
        ),
        ("ListBoxExternal", ext_scene(ListBoxExternal::new(3))),
        ("ListBoxItemExternal", ext_scene(ListBoxItemExternal::new())),
        (
            "MenuBarExternal",
            ext_scene(MenuBarExternal::new(vec![2, 2])),
        ),
        (
            "PaginationExternal",
            ext_scene(PaginationExternal::new(9, 0)),
        ),
        ("ProgressBarExternal", ext_scene(ProgressBarExternal::new())),
        ("RadioExternal", ext_scene(RadioExternal::new())),
        ("RadioGroupExternal", ext_scene(RadioGroupExternal::new(3))),
        ("RangeSliderExternal", ext_scene(RangeSliderExternal::new())),
        (
            "RowDissectionExternal",
            ext_scene(RowDissectionExternal::new(Rc::new(
                RowDissectionState::new(vec![serde_json::json!({"a": 1})]),
            ))),
        ),
        (
            "RowSearchExternal",
            ext_scene(RowSearchExternal::new(Rc::new(RowSearchState::new(
                1,
                cells(),
            )))),
        ),
        (
            "RowStyleExternal",
            ext_scene(RowStyleExternal::new(Rc::new(RowStyleState::default()))),
        ),
        ("ScrollBarExternal", ext_scene(ScrollBarExternal::new())),
        ("SliderExternal", ext_scene(SliderExternal::new())),
        (
            "SpinButtonExternal",
            ext_scene(SpinButtonExternal::new(1.0, 0.0, 10.0, 1.0)),
        ),
        (
            "TableExternal",
            ext_scene(TableExternal::new(vec!["h".to_owned()], cells())),
        ),
        ("TextFieldExternal", ext_scene(TextFieldExternal::new())),
        ("ToggleExternal", ext_scene(ToggleExternal::new())),
        (
            "ToggleStateSnapshot",
            ext_scene(ToggleStateSnapshot::new(ToggleState::Idle, false)),
        ),
        (
            "ToolbarExternal",
            ext_scene(ToolbarExternal::new(vec![ToolItem::Command])),
        ),
        ("TooltipExternal", ext_scene(TooltipExternal::new())),
        (
            "TreeFilterExternal",
            ext_scene(TreeFilterExternal::new(Rc::new(TreeFilterState::new(
                Box::new(|_| Vec::new()),
            )))),
        ),
        (
            "ViewSortFilterExternal",
            ext_scene(ViewSortFilterExternal::new(Rc::new(ViewOrderState::new(
                vec!["k".to_owned()],
                vec![0],
            )))),
        ),
        (
            "VirtualSelectExternal",
            ext_scene(VirtualSelectExternal::new(3)),
        ),
    ]
}

#[test]
fn r1637_a_declared_read_is_not_a_name_the_surface_dispatches() {
    /// How a surface answered — the variant alone, because the CONTROL below
    /// compares answers to each other rather than to a fixed expectation.
    fn shape(answer: &Result<IntrospectValue, InvokeError>) -> u8 {
        match answer {
            Err(InvokeError::UnknownPath) => 0,
            Err(InvokeError::TypeMismatch) => 1,
            Err(InvokeError::Rejected(_)) => 2,
            Err(_) => 3,
            Ok(_) => 4,
        }
    }

    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for (label, handle) in catalog() {
        let mut handle = handle;
        let intro = handle
            .introspect_mut()
            .expect("every entry opts into introspection");
        // R1640 — the NEGATIVE CONTROL, and the walk is worthless without it.
        // The first version asked only "does `invoke` answer something other
        // than UnknownPath", which reads a UNIFORM refusal as recognition:
        // `ButtonStateSnapshot` and `ToggleStateSnapshot` decline every name
        // with one stated sentence ("this surface is a read-only copy"), and
        // three of their declared reads were reported as mis-declared channels
        // on the walk's first widened run. A surface that answers a name it has
        // never heard of the same way it answers a declared one is not telling
        // us anything about either.
        let control = shape(&intro.invoke("$__no_such_action__", IntrospectValue::Null));
        let reads: Vec<&'static str> = intro
            .schema()
            .fields
            .iter()
            .filter(|f| f.channel == SchemaChannel::Read && f.args.is_empty())
            .map(|f| f.path)
            .collect();
        for path in reads {
            checked += 1;
            if shape(&intro.invoke(path, IntrospectValue::Null)) != control {
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

/// R1642 — every conditional declaration in the walked catalog can be followed.
///
/// The rule itself is [`SchemaField::conditional_defect`], stated once in the
/// crate that owns the types; this applies it to the population the walk can
/// reach.
///
/// # This gate has no inhabitants today, and says so
///
/// No `pinion_core::widgets` surface declares a case table — the two verbs that
/// do (`arrange`, `item`) live in `hello-node-groups`, which the walk does not
/// reach ([[debt-the-declaration-walk-reaches-one-crate]]); their own test module
/// applies the same predicate, and the round's demo drives them over the wire.
/// So the count is asserted rather than the absence of defects: R1640's finding
/// was that a gate which does not state its denominator reads as coverage, and
/// a gate reporting "no defect" over zero fields would be the purest form of
/// that. When a catalog widget gains a conditional verb, the count moves and
/// this check starts discriminating without being edited.
#[test]
fn r1642_conditional_declarations_can_be_followed() {
    let mut checked = 0_usize;
    let mut inhabitants: Vec<String> = Vec::new();
    let mut wrong: Vec<String> = Vec::new();
    let mut surfaces: Vec<(&str, Box<dyn pinion_core::external::External>)> =
        vec![("dock_panel", Box::new(DockPanelExternal::new("a")))];
    surfaces.extend(catalog());
    for (label, handle) in surfaces {
        let intro = handle.introspect().expect("opts into introspection");
        for field in intro.schema().fields {
            checked += 1;
            if field.declares_cases() {
                inhabitants.push(format!("{label}.{}", field.path));
            }
            if let Some(defect) = field.conditional_defect() {
                wrong.push(format!("{label}.{}: {defect:?}", field.path));
            }
        }
    }
    assert!(checked > 100, "the walk must be non-trivial, saw {checked}");
    assert!(
        wrong.is_empty(),
        "a declaration cannot be followed: {wrong:?}"
    );
    assert_eq!(
        inhabitants.len(),
        0,
        "the walked catalog declares no case table; when one does, this count is \
         the population the check above ran against: {inhabitants:?}",
    );
}

/// R1639 — no `send` in the widget catalog is left saying nothing.
///
/// `send` is the most-declared action in the tree and, until R1638/R1639, the
/// least described: sixty-seven surfaces published the name and none published
/// its grammar. The two grammars a catalog widget can use are now both
/// expressible — the composite pointer wire (delimited, four segments) and the
/// bare statechart event (scalar, from that widget's own closed vocabulary) —
/// so a silent one here is an omission rather than an honest absence.
///
/// Deliberately scoped to the catalog this test can build, and deliberately NOT
/// asserting WHICH form: that is the surface's fact, and a test that pinned it
/// would have to restate the decoder's choice, which is the copy
/// `composite_tag::SEND_ARGS` exists to avoid.
#[test]
fn r1639_no_catalog_send_is_left_undeclared() {
    use pinion_core::external::{ArgDomain, ArgForm};

    let mut silent: Vec<String> = Vec::new();
    let mut seen = 0_usize;
    for (label, handle) in catalog() {
        let intro = handle.introspect().expect("opts into introspection");
        for field in intro.schema().fields.iter().filter(|f| f.path == "send") {
            seen += 1;
            match field.form {
                ArgForm::Undeclared => silent.push(label.to_owned()),
                // A bare-event send must name a NON-EMPTY vocabulary: an empty
                // one is the promise that cannot be kept, and it is what a
                // widget whose chart drives nothing externally would publish.
                ArgForm::Scalar => {
                    let ArgDomain::OneOf(values) = field.args[0].domain else {
                        silent.push(format!("{label} (scalar, no vocabulary)"));
                        continue;
                    };
                    assert!(!values.is_empty(), "{label}: an empty vocabulary");
                }
                _ => {}
            }
        }
    }
    assert!(
        seen >= 10,
        "the walk must find real send actions, saw {seen}"
    );
    assert!(
        silent.is_empty(),
        "these declare `send` and do not say what it takes: {silent:?}",
    );
}

/// R1640 — the catalog **is** every `ExternalIntrospect` in
/// `pinion_core::widgets`, and a widget added later cannot slip past these
/// checks by not being written down.
///
/// Before this the walk built the nineteen types whose constructors took
/// nothing, and the denominator was stated nowhere — so "the catalog passes"
/// read as coverage while half the catalog was invisible. Widening it to all
/// thirty-nine found a real mis-declared channel (`ColumnResizeExternal`'s
/// `send`) in the first run, which is the same hit rate the walk had on its
/// very first run against dock.
///
/// # Why this reads the source, and how it avoids the census trap
///
/// Rust cannot enumerate the impls of a trait at runtime, so the denominator
/// has to come from the text. This session burned three times on text censuses
/// that read the wrong thing (a comment, a tuple pattern, a first match only),
/// so the rule here is R1605's: **every occurrence must account for itself**.
/// The scan does not count — it collects names, and every name must be either
/// in the catalog or in `NOT_A_WIDGET_SURFACE` with a reason. A mis-parse
/// therefore surfaces as an unaccounted NAME, which a reader can act on,
/// rather than as a number that happens to match.
#[test]
fn r1640_the_catalog_is_every_widget_external() {
    use std::collections::BTreeSet;

    /// Types that impl the trait in that directory and are deliberately absent.
    /// Empty today, and kept as the place a future exclusion must state itself
    /// rather than being silently dropped from the list.
    const NOT_A_WIDGET_SURFACE: &[(&str, &str)] = &[];

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pinion-core/src/widgets");
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut files = 0_usize;
    for entry in std::fs::read_dir(&dir).expect("the widget directory is readable") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        files += 1;
        let src = std::fs::read_to_string(&path).expect("a readable widget module");
        for line in src.lines() {
            // Only a top-level impl, which is what an addressable surface is:
            // an indented one is inside a test module's fixture.
            if let Some(rest) = line.strip_prefix("impl ExternalIntrospect for ") {
                found.insert(rest.trim_end_matches(" {").trim().to_owned());
            }
        }
    }
    assert!(
        files > 30,
        "the scan must see the real directory, saw {files}"
    );
    assert!(
        found.len() > 30,
        "and the real impls in it, saw {}",
        found.len()
    );

    // The catalog's labels ARE the type names, so this is an exact set
    // comparison. An earlier draft matched a snake_case label against the type
    // by lowercasing and dropping underscores, and four entries that WERE in
    // the catalog came back unaccounted (`row_dissect` against
    // `RowDissectionExternal`) — a fuzzy comparison inside a completeness gate
    // reports the gate's own spelling as a gap, which is the one failure a
    // completeness gate must not have.
    let built: BTreeSet<&str> = catalog().into_iter().map(|(name, _)| name).collect();
    // R1640 — an exclusion is the ONLY way a surface leaves this walk, so it is
    // not free: it must name a reason, and the reason must be about that
    // surface rather than a placeholder. A counterfactual that added a
    // reason-less exclusion passed against the first draft, which made the
    // gate's own escape hatch the cheapest way through it.
    for (name, reason) in NOT_A_WIDGET_SURFACE {
        assert!(
            reason.len() > 20,
            "{name} is excluded from the walk with no stated reason: {reason:?}",
        );
        assert!(
            found.contains(*name),
            "{name} is excluded and is not in the source — a stale exclusion \
             silently shrinks the denominator",
        );
    }
    let excused: BTreeSet<&str> = NOT_A_WIDGET_SURFACE.iter().map(|(n, _)| *n).collect();

    let unaccounted: Vec<&String> = found
        .iter()
        .filter(|name| !built.contains(name.as_str()) && !excused.contains(name.as_str()))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "these widget surfaces are in the source and in neither the catalog nor \
         the stated exclusions: {unaccounted:?}",
    );
}
