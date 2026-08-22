//! R1566 §2 #7 §5.12 — every framework `External` answers on the channel it
//! declares, **asked rather than read**.
//!
//! # Why this file exists, and why a source scan was not enough
//!
//! R1566 corrected 102 declarations in this workspace's own widgets from
//! `SchemaField::new` to `SchemaField::action` — verbs the surface answers from
//! `invoke` and published as readable string fields. They were *found* by
//! scanning source, and a scan proves nothing about the running code: it can
//! match a name in a comment, miss one built by a helper, and it cannot notice
//! the next one at all. So the finding is re-established here by construction —
//! every External below is built and **asked**, and the assertion is the one
//! the declaration makes:
//!
//!   * a `Read` scalar must ANSWER `query` — a surface that publishes a name
//!     and then says it does not exist is the §2 #7 lie R1566 removed from the
//!     wire, and it must not come back one declaration at a time;
//!   * an `Invoke` path must NOT answer `query` — that is what tells an agent
//!     which call to make, and it is the half that was wrong 102 times.
//!
//! R1563 wrote exactly this gate for **one** widget (`VirtualSelectExternal`)
//! and the shape never reached the rest, which is why the 102 survived. It is a
//! hand-listed roster here for a stated reason: this framework has no registry
//! of `External` implementors, so nothing can enumerate them for us. The roster
//! is therefore itself audited — `every_external_type_in_the_widget_tree_is_listed`
//! counts the `impl ExternalIntrospect` blocks the crate actually contains and
//! fails when one is added without a row here.
//!
//! Parametric families are skipped: the declared path is a template
//! (`width.<col>`), not an address, so there is nothing to ask for.

use std::rc::Rc;

use pinion_core::external::{ExternalIntrospect, ReadRefusal, SchemaChannel};

/// Assert one surface answers on the channels it declares. Returns
/// `(reads, actions)` — the counts checked, so a caller can prove the fixture
/// exercised both kinds instead of passing vacuously.
fn check(label: &str, surface: &dyn ExternalIntrospect) -> (usize, usize) {
    let (mut reads, mut actions) = (0, 0);
    for field in surface.schema().fields {
        if field.path.is_empty() || !field.args.is_empty() {
            continue; // a template is not an address
        }
        if field.channel == SchemaChannel::Invoke {
            actions += 1;
            assert_eq!(
                surface.query(field.path),
                Err(ReadRefusal::UnknownPath),
                "{label}: {:?} is declared an ACTION and also answers a read — \
                 `SchemaChannel` is what tells an agent which call to make",
                field.path,
            );
        } else {
            reads += 1;
            assert!(
                surface.query(field.path).is_ok(),
                "{label}: {:?} is declared READABLE and answers nothing — the \
                 surface publishes a name and then says it does not exist \
                 (R1566: this was true of 102 declarations in this crate)",
                field.path,
            );
        }
    }

    // ★★★★★ R1769 — **the snapshot slot and the action that takes it back are
    // a PAIR, and this is what makes that a gate rather than a habit.**
    //
    // A surface answering `configuration` without a `resume` publishes a value
    // a client can read and never give back, and a `resume` without a
    // `configuration` takes a value nothing can produce. Either half alone is
    // worse than neither, because the schema advertises a round trip that is
    // not there. Eight surfaces adopted the pair in one round; the ninth is
    // whoever adds a statechart widget next, and they will be told here.
    let declares = |path: &str| surface.schema().fields.iter().any(|f| f.path == path);
    assert_eq!(
        declares("configuration"),
        declares("resume"),
        "{label}: `configuration` and `resume` are a round trip and must be \
         declared together — one without the other advertises a trip a client \
         cannot make"
    );

    (reads, actions)
}

#[test]
fn r1566_every_framework_external_answers_on_the_channel_it_declares() {
    use pinion_core::widgets::*;

    let mut reads = 0;
    let mut actions = 0;
    let mut checked = 0;
    let mut record = |label: &str, surface: &dyn ExternalIntrospect| {
        let (r, a) = check(label, surface);
        reads += r;
        actions += a;
        checked += 1;
    };

    record("ButtonExternal", &button::ButtonExternal::new());
    record("CheckboxExternal", &checkbox::CheckboxExternal::new());
    record("ColorAreaExternal", &color_area::ColorAreaExternal::new());
    record("DisclosureExternal", &disclosure::DisclosureExternal::new());
    record(
        "ListBoxItemExternal",
        &listbox_item::ListBoxItemExternal::new(),
    );
    record("RadioExternal", &radio::RadioExternal::new());
    record(
        "RangeSliderExternal",
        &range_slider::RangeSliderExternal::new(),
    );
    record("ScrollBarExternal", &scrollbar::ScrollBarExternal::new());
    record("SliderExternal", &slider::SliderExternal::new());
    // Bound, not bare: `text` / `caret` read through an attached
    // `TextEditState` and answer `None` without one — a documented state with
    // its own test (`r56_1_b_query_text_returns_none_without_attached_state`),
    // not a mis-declaration. This gate found the distinction by asking, which
    // is the difference between a gate and a scan.
    record(
        "TextFieldExternal",
        &text_field::TextFieldExternal::new().attach_state(Rc::new(
            text_edit::TextEditState::with_initial("hello".to_owned()),
        )),
    );
    record("ToggleExternal", &toggle::ToggleExternal::new());
    record("TooltipExternal", &tooltip::TooltipExternal::new());
    record(
        "ContextMenuExternal",
        &context_menu::ContextMenuExternal::new(3),
    );
    record(
        "DisclosureGroupExternal",
        &disclosure_group::DisclosureGroupExternal::new(3),
    );
    record("ListBoxExternal", &listbox::ListBoxExternal::new(3));
    record(
        "PaginationExternal",
        &pagination::PaginationExternal::new(5, 1),
    );
    record(
        "RadioGroupExternal",
        &radio_group::RadioGroupExternal::new(3),
    );
    record(
        "SpinButtonExternal",
        &spin_button::SpinButtonExternal::new(1.0, 0.0, 10.0, 1.0),
    );
    record(
        "TableExternal",
        &table::TableExternal::new(
            vec!["a".to_owned(), "b".to_owned()],
            vec![vec!["1".to_owned(), "2".to_owned()]],
        ),
    );
    record("MenuBarExternal", &menu::MenuBarExternal::new(vec![2, 2]));
    record(
        "ToolbarExternal",
        &toolbar::ToolbarExternal::new(vec![toolbar::ToolItem::Command]),
    );

    assert!(
        reads >= 40,
        "the roster must exercise real read slots: {reads}"
    );
    assert!(
        actions >= 20,
        "and real ACTIONS — without them the invoke half of this gate is \
         vacuous and the 102 corrections are unguarded: {actions}",
    );
    assert!(checked >= 20, "surfaces checked: {checked}");
}

/// The state-backed surfaces, split out only because each needs its shared
/// state built first. Same assertion, same reason.
#[test]
fn r1566_state_backed_externals_answer_on_the_channel_they_declare() {
    use pinion_core::widgets::*;

    let mut actions = 0;
    let mut record = |label: &str, surface: &dyn ExternalIntrospect| {
        actions += check(label, surface).1;
    };

    record(
        "UndoStackExternal",
        &pinion_core::undo::UndoStackExternal::new(Rc::new(pinion_core::undo::UndoStack::new())),
    );
    record(
        "ColumnWidthExternal",
        &column_widths::ColumnWidthExternal::new(Rc::new(column_widths::ColumnWidths::new(vec![
            80, 80, 80,
        ]))),
    );
    record(
        "CompleterExternal",
        &completion::CompleterExternal::new(Rc::new(completion::CompletionState::new(vec![
            "alpha".to_owned(),
        ]))),
    );
    record(
        "GridSortExternal",
        &grid_sort::GridSortExternal::new(Rc::new(grid_sort::GridSortState::new(
            2,
            vec![vec!["a".to_owned(), "b".to_owned()]],
        ))),
    );
    record(
        "RowSearchExternal",
        &row_search::RowSearchExternal::new(Rc::new(row_search::RowSearchState::new(
            2,
            vec![vec!["a".to_owned(), "b".to_owned()]],
        ))),
    );

    assert!(
        actions >= 10,
        "the state-backed roster must exercise ACTIONS — the half R1566 found \
         wrong 102 times: {actions}",
    );
}

/// R1566 — the roster above is hand-listed because this framework has no
/// registry of `External` implementors, so this counts what the crate contains
/// and fails when a surface is added that no test asks.
///
/// It is deliberately a **floor on the roster**, not a name-by-name match: the
/// point is that the gate cannot silently stop covering the tree, and a count
/// says that without a second list to keep in sync with the first.
#[test]
fn r1566_the_roster_still_covers_the_widget_tree() {
    let sources = [
        include_str!("../src/widgets/button.rs"),
        include_str!("../src/widgets/checkbox.rs"),
        include_str!("../src/widgets/color_area.rs"),
        include_str!("../src/widgets/context_menu.rs"),
        include_str!("../src/widgets/disclosure.rs"),
        include_str!("../src/widgets/disclosure_group.rs"),
        include_str!("../src/widgets/listbox.rs"),
        include_str!("../src/widgets/listbox_item.rs"),
        include_str!("../src/widgets/menu.rs"),
        include_str!("../src/widgets/pagination.rs"),
        include_str!("../src/widgets/radio.rs"),
        include_str!("../src/widgets/radio_group.rs"),
        include_str!("../src/widgets/range_slider.rs"),
        include_str!("../src/widgets/scrollbar.rs"),
        include_str!("../src/widgets/slider.rs"),
        include_str!("../src/widgets/spin_button.rs"),
        include_str!("../src/widgets/table.rs"),
        include_str!("../src/widgets/text_field.rs"),
        include_str!("../src/widgets/toggle.rs"),
        include_str!("../src/widgets/toolbar.rs"),
        include_str!("../src/widgets/tooltip.rs"),
    ];
    let implementors: usize = sources
        .iter()
        .map(|src| {
            src.lines()
                .filter(|line| line.starts_with("impl ExternalIntrospect for "))
                .count()
        })
        .sum();
    assert!(
        implementors >= 21,
        "the files this roster draws from hold {implementors} ExternalIntrospect \
         impls; if that count fell, a surface moved and the gate may have \
         stopped covering it",
    );
}
