// R833 §3 §5.53 §5.15 — own-rendered print dialog.
#![allow(rustdoc::private_intra_doc_links)]
//! R833 — pinion paints its **own** print dialog (no native GTK / Qt
//! print dialog, §5.53) and submits through the [`pinion_core::print`]
//! [`PrintBackend`] substrate: [`InMemoryPrintBackend`] for the headless
//! demo (this CI box has no CUPS destination), `CupsPrintBackend`
//! (`pinion-platform-print`) for real Linux printing.
//!
//! ## Composition
//!
//! The dialog is pure composition over existing widgets: four
//! [`ButtonExternal`]s (cycle printer / copies − / copies + / Print) over
//! a reactive [`PrintUiModel`] (`copies` + `selected` + the last receipt)
//! in [`Owner::cache`]. The Print button's reducer reads the model, builds
//! a [`PrintJob`], and submits it through the cached backend. The chosen
//! printer + submitted job are exposed for AI-first introspection through
//! a [`QueryOnlyIntrospect`] node (tag `job`):
//! `scene/query "/job/external/{printer_count,selected,copies,submit_count,
//! last_printer,last_copies,last_job}"`.
//!
//! ## AI clients
//!
//! `invoke("send", "…")` / `scene/click` drive the buttons; the submitted
//! job is then observable through the `job` query node — the whole
//! pick-printer → set-copies → Print → spool loop is RPC-driven and
//! RPC-verifiable, the own-renderer (§5.53) answer to the native print
//! dialog being headless-undriveable.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{
    External, IntrospectSchema, IntrospectValue, QueryOnlyIntrospect, QuerySource,
};
use pinion_core::intent::Intent;
use pinion_core::print::{InMemoryPrintBackend, PrintBackend, PrintJob};
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{intent_tag, Command, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{read_button_state, view_button, ButtonColors, ButtonStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPrintRenderer, HelloPrintRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

const PRINT_TAG: &str = "print";
const CYCLE_TAG: &str = "cycle";
const DEC_TAG: &str = "dec";
const INC_TAG: &str = "inc";
const JOB_TAG: &str = "job";

const PRINT_CLICK: &str = intent_tag!("print", "click");
const CYCLE_CLICK: &str = intent_tag!("cycle", "click");
const DEC_CLICK: &str = intent_tag!("dec", "click");
const INC_CLICK: &str = intent_tag!("inc", "click");

const BODY_FONT_PX: u32 = 15;
const STATUS_FONT_PX: u32 = 13;

/// The document this dialog prints (plain text this round; scene → PDF
/// page render is a future axis).
const DOCUMENT: &str = "Quarterly Report\nRevenue up 12% QoQ\nPrepared by pinion";

/// R833 §5.38 — the print dialog's reactive state: copies, the selected
/// printer index, and the last submitted receipt. Shared (one `Rc`) by
/// the view, the reducer, and the [`QueryOnlyIntrospect`] node.
#[derive(Debug)]
struct PrintUiModel {
    copies: Signal<u32>,
    selected: Signal<usize>,
    submit_count: Signal<u32>,
    last_printer: Signal<String>,
    last_copies: Signal<u32>,
    last_job: Signal<String>,
}

impl PrintUiModel {
    fn new() -> Self {
        Self {
            copies: Signal::new(1),
            selected: Signal::new(0),
            submit_count: Signal::new(0),
            last_printer: Signal::new(String::new()),
            last_copies: Signal::new(0),
            last_job: Signal::new(String::new()),
        }
    }
}

/// The introspection source: reads the printer roster from the backend
/// and the live dialog / receipt state from the model.
#[derive(Debug)]
struct PrintIntrospect {
    backend: Rc<InMemoryPrintBackend>,
    model: Rc<PrintUiModel>,
}

impl QuerySource for PrintIntrospect {
    fn introspect_schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("printer_count", "int"),
            ("selected", "int"),
            ("selected_id", "string"),
            ("copies", "int"),
            ("submit_count", "int"),
            ("last_printer", "string"),
            ("last_copies", "int"),
            ("last_job", "string"),
        ])
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        let printers = self.backend.enumerate_printers();
        let selected = self.model.selected.get();
        match path {
            "printer_count" => Some(IntrospectValue::Int(int_of(printers.len()))),
            "selected" => Some(IntrospectValue::Int(int_of(selected))),
            "selected_id" => Some(IntrospectValue::Text(
                printers.get(selected).map(|p| p.id.clone()).unwrap_or_default(),
            )),
            "copies" => Some(IntrospectValue::Int(i64::from(self.model.copies.get()))),
            "submit_count" => Some(IntrospectValue::Int(i64::from(self.model.submit_count.get()))),
            "last_printer" => Some(IntrospectValue::Text(self.model.last_printer.get())),
            "last_copies" => Some(IntrospectValue::Int(i64::from(self.model.last_copies.get()))),
            "last_job" => Some(IntrospectValue::Text(self.model.last_job.get())),
            _ => None,
        }
    }
}

fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn use_model() -> Rc<PrintUiModel> {
    let owner = Owner::current().expect("use_model requires an active Owner scope");
    owner.cache("hello_print.model", PrintUiModel::new)
}

fn use_backend() -> Rc<InMemoryPrintBackend> {
    let owner = Owner::current().expect("use_backend requires an active Owner scope");
    owner.cache("hello_print.backend", InMemoryPrintBackend::with_sample_printers)
}

/// Per-button interaction states captured for the paint.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PrintState {
    print: ButtonState,
    cycle: ButtonState,
    dec: ButtonState,
    inc: ButtonState,
}

impl Default for PrintState {
    fn default() -> Self {
        Self {
            print: ButtonState::Idle,
            cycle: ButtonState::Idle,
            dec: ButtonState::Idle,
            inc: ButtonState::Idle,
        }
    }
}

/// Compose one labelled button.
fn button(label: &str, tag: &'static str, state: ButtonState, colors: &ButtonColors) -> Scene {
    let style = ButtonStyle::m3_default(tag)
        .with_corner_radius(8)
        .with_padding(Rect::new(14, 8, 14, 8))
        .with_label_font_size_px(BODY_FONT_PX);
    view_button(label, state, 0.0, false, colors, &style)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: PrintState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let backend = use_backend();
    let model = use_model();
    let printers = backend.enumerate_printers();
    let selected = model.selected.get();
    let copies = model.copies.get();

    let title = Scene::Text(TextNode::styled(
        "Print",
        Rect::default(),
        TextStyle::new().with_size_px(22).with_fg(on_surface),
    ));

    // Printer roster: one row per destination, the selected one filled.
    let mut printer_rows: Vec<Scene> = Vec::with_capacity(printers.len());
    for (i, p) in printers.iter().enumerate() {
        let chosen = i == selected;
        let label = if p.is_default {
            format!("{}  (default)", p.name)
        } else {
            p.name.clone()
        };
        let row_fg = if chosen { theme.resolve(ColorRole::OnSurface) } else { on_surface_muted };
        let mut row = ContainerNode::new(vec![Scene::Text(TextNode::styled(
            &label,
            Rect::default(),
            TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(row_fg),
        ))])
        .with_layout(
            LayoutStyle::new()
                .with_padding(Rect::new(10, 6, 10, 6))
                .with_size(Size::auto().with_width(SizeValue::Px(WIN_W - 48))),
        );
        if chosen {
            row = row.with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)));
        }
        printer_rows.push(Scene::Container(row));
    }
    let printer_list = Scene::Container(
        ContainerNode::new(printer_rows).with_tag("printers").with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch)
                .with_gap(2),
        ),
    );

    let copies_label = Scene::Text(TextNode::styled(
        format!("Copies: {copies}"),
        Rect::default(),
        TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(on_surface),
    ));

    let tonal = ButtonColors::filled_tonal(&theme);
    let accent = ButtonColors::accent(&theme);
    let controls = Scene::Container(
        ContainerNode::new(vec![
            button("Next printer", CYCLE_TAG, state.cycle, &tonal),
            button("-", DEC_TAG, state.dec, &tonal),
            button("+", INC_TAG, state.inc, &tonal),
            button("Print", PRINT_TAG, state.print, &accent),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(10),
        ),
    );

    let status = if model.submit_count.get() > 0 {
        format!(
            "Sent {} cop{} to {} (job {})",
            model.last_copies.get(),
            if model.last_copies.get() == 1 { "y" } else { "ies" },
            model.last_printer.get(),
            model.last_job.get(),
        )
    } else {
        "No job submitted yet.".to_owned()
    };
    let status_line = Scene::Text(TextNode::styled(
        &status,
        Rect::default(),
        TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(on_surface_muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, printer_list, copies_label, controls, status_line])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_justify(JustifyContent::Start)
                    .with_gap(14)
                    .with_padding(Rect::new(24, 24, 24, 24)),
            ),
    )
}

struct PrintView;

impl WidgetCore for PrintView {
    type State = PrintState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(CYCLE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DEC_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(INC_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(
                JOB_TAG,
                Box::new(QueryOnlyIntrospect::new(Rc::new(PrintIntrospect {
                    backend: use_backend(),
                    model: use_model(),
                }))),
            ),
        ]
    }

    fn tag() -> &'static str {
        PRINT_TAG
    }

    fn read_state(scene: &Scene) -> PrintState {
        PrintState {
            print: read_button_state(scene, PRINT_TAG),
            cycle: read_button_state(scene, CYCLE_TAG),
            dec: read_button_state(scene, DEC_TAG),
            inc: read_button_state(scene, INC_TAG),
        }
    }

    fn view(state: PrintState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-print (R833 own-rendered print dialog)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // Forward the key to whichever button owns focus (container-aware:
        // the boot scene is a multi-external Container).
        let Some(tag) = focused else {
            return false;
        };
        let Some(node) = scene.find_external_with_tag_mut(tag) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke("key", IntrospectValue::Text(key.to_owned())),
            Ok(IntrospectValue::Bool(true))
        )
    }

    /// R833 §5.20 — the dialog reducer: cycle the selected printer, adjust
    /// the copy count, or (Print) build a [`PrintJob`] and submit it
    /// through the cached backend, recording the receipt in the model.
    fn update(_state: PrintState, intent: &Intent) -> Vec<Command> {
        let model = use_model();
        let backend = use_backend();
        let tag = intent.tag.as_ref();
        if tag == CYCLE_CLICK {
            let count = backend.enumerate_printers().len();
            if count > 0 {
                model.selected.set((model.selected.get() + 1) % count);
            }
        } else if tag == DEC_CLICK {
            model.copies.set(model.copies.get().saturating_sub(1).max(1));
        } else if tag == INC_CLICK {
            model.copies.set(model.copies.get() + 1);
        } else if tag == PRINT_CLICK {
            submit_current_job(&backend, &model);
        }
        Vec::new()
    }
}

/// Build the [`PrintJob`] from the model and submit it to the selected
/// destination, recording the receipt on success.
fn submit_current_job(backend: &InMemoryPrintBackend, model: &PrintUiModel) {
    let printers = backend.enumerate_printers();
    let Some(printer) = printers.get(model.selected.get()) else {
        return;
    };
    let job = PrintJob::new("hello-print document", DOCUMENT).with_copies(model.copies.get());
    if let Ok(receipt) = backend.submit(&printer.id, &job) {
        batch(|| {
            model.submit_count.set(model.submit_count.get() + 1);
            model.last_printer.set(receipt.printer_id);
            model.last_copies.set(receipt.copies);
            model.last_job.set(receipt.job_id);
        });
    }
}

impl WidgetA11y for PrintView {
    /// R833 §5.40 — a `group` (the dialog) owning one `button` per
    /// control. Names come from the painted labels via enrichment; the
    /// roster rows are presentational text this round (clickable a11y
    /// printer rows = additive carry).
    fn access_node(_state: &PrintState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_tag = <Self as WidgetCore>::tag();
        let buttons = [
            (CYCLE_TAG, "Next printer"),
            (DEC_TAG, "Decrease copies"),
            (INC_TAG, "Increase copies"),
            (PRINT_TAG, "Print"),
        ];
        let mut group = AccessNode::new("print_dialog", AriaRole::Group).with_name("Print");
        for (tag, _) in buttons {
            group = group.with_child(tag);
        }
        let mut nodes = vec![group];
        for (tag, name) in buttons {
            nodes.push(
                AccessNode::new(tag, AriaRole::Button).with_name(name).with_state(AccessState {
                    focused: focused == Some(tag) || (tag == group_tag && focused == Some(group_tag)),
                    ..AccessState::default()
                }),
            );
        }
        nodes
    }
}

impl WidgetView for PrintView {
    type Renderer = HelloPrintRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PrintView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_tags_are_dotted_full_form() {
        assert_eq!(PRINT_CLICK, "print.click");
        assert_eq!(CYCLE_CLICK, "cycle.click");
        assert_eq!(DEC_CLICK, "dec.click");
        assert_eq!(INC_CLICK, "inc.click");
    }

    #[test]
    fn access_node_emits_group_plus_four_buttons() {
        let nodes = PrintView::access_node(&PrintState::default(), None);
        assert_eq!(nodes.len(), 1 + 4);
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].children.len(), 4);
        assert!(nodes[1..].iter().all(|n| n.role == AriaRole::Button));
    }
}
