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
    int_of, External, IntrospectSchema, IntrospectValue, QueryOnlyIntrospect, QuerySource,
};
use pinion_core::intent::Intent;
use pinion_core::print::{
    InMemoryPrintBackend, PrintBackend, PrintError, PrintJob, PrintReceipt, PrinterInfo,
};
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle, Size,
    SizeValue, TextStyle,
};
use pinion_pdf::{render_scene, PageSize};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{intent_tag, Command, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_platform_print::{cups_available, CupsPrintBackend};
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

/// The focusable / rovable buttons, in keyboard-roving order (R834: the
/// `.with_focusable(true)` Tab-stop + `activate_or_rove` keyboard model).
const BUTTON_TAGS: [&str; 4] = [CYCLE_TAG, DEC_TAG, INC_TAG, PRINT_TAG];

const PRINT_CLICK: &str = intent_tag!("print", "click");
const CYCLE_CLICK: &str = intent_tag!("cycle", "click");
const DEC_CLICK: &str = intent_tag!("dec", "click");
const INC_CLICK: &str = intent_tag!("inc", "click");

const BODY_FONT_PX: u32 = 15;
const STATUS_FONT_PX: u32 = 13;

/// The printed document's title + body lines. R911 — the dialog now
/// prints the *rendered* document (a vector PDF), not a plain-text blob:
/// `document_scene` lays these out as an explicitly-positioned scene and
/// [`render_print_pdf`] walks it through the §5.53 own-renderer
/// `pinion-pdf` pipeline. The page is US Letter (the in-memory backend
/// records the PDF bytes; a real `lp` destination auto-detects PDF).
const DOC_TITLE: &str = "Quarterly Report";
const DOC_BODY: [&str; 3] = [
    "Revenue up 12% QoQ",
    "Operating margin steady at 18%",
    "Prepared by pinion",
];

/// The printed page size (US Letter, portrait).
const DOC_PAGE: PageSize = PageSize::LETTER;
/// Left/top margin for the document body, in points.
const DOC_MARGIN: u32 = 54;

/// Build the print document as an explicitly-positioned [`Scene`] (no
/// layout pass needed — a print document is hand-placed, the textbook
/// PDF-generator approach): a bold title, an accent rule, then the body
/// lines stacked at a fixed line height.
fn document_scene() -> Scene {
    let ink = Color::rgb(0x11, 0x11, 0x11);
    let accent = Color::rgb(0x2f, 0x6f, 0xed);
    let body_w = DOC_PAGE.width_pt - DOC_MARGIN * 2;

    let mut children: Vec<Scene> = Vec::with_capacity(DOC_BODY.len() + 2);
    children.push(Scene::Text(TextNode::styled(
        DOC_TITLE,
        Rect::new(DOC_MARGIN, DOC_MARGIN, body_w, 28),
        TextStyle::new().with_size_px(24).with_weight(FontWeight::BOLD).with_fg(ink),
    )));
    // An accent rule under the title.
    children.push(Scene::Box(BoxNode::filled(
        Rect::new(DOC_MARGIN, DOC_MARGIN + 38, body_w, 2),
        accent,
    )));
    let mut y = DOC_MARGIN + 58;
    for line in DOC_BODY {
        children.push(Scene::Text(TextNode::styled(
            line,
            Rect::new(DOC_MARGIN, y, body_w, 18),
            TextStyle::new().with_size_px(14).with_fg(ink),
        )));
        y += 26;
    }
    Scene::Container(ContainerNode::new(children))
}

/// Render the print document to a PDF document string through the §5.53
/// own-renderer `pinion-pdf` pipeline — the 2nd consumer of the R908
/// renderer substrate.
fn render_print_pdf() -> String {
    render_scene(&document_scene(), DOC_PAGE).document
}

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
    /// R911 — the rendered PDF document of the last submitted job (the
    /// own-renderer print payload), exposed for AI-first verification.
    last_content: Signal<String>,
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
            last_content: Signal::new(String::new()),
        }
    }
}

/// The introspection source: reads the printer roster from the backend
/// and the live dialog / receipt state from the model.
#[derive(Debug)]
struct PrintIntrospect {
    backend: Rc<AppPrintBackend>,
    model: Rc<PrintUiModel>,
}

impl QuerySource for PrintIntrospect {
    fn introspect_schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("backend_kind", "string"),
            ("printer_count", "int"),
            ("selected", "int"),
            ("selected_id", "string"),
            ("copies", "int"),
            ("submit_count", "int"),
            ("last_printer", "string"),
            ("last_copies", "int"),
            ("last_job", "string"),
            ("last_content", "string"),
        ])
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        let printers = self.backend.enumerate_printers();
        let selected = self.model.selected.get();
        match path {
            "backend_kind" => Some(IntrospectValue::Text(self.backend.kind.to_owned())),
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
            "last_content" => Some(IntrospectValue::Text(self.model.last_content.get())),
            _ => None,
        }
    }
}

fn use_model() -> Rc<PrintUiModel> {
    let owner = Owner::current().expect("use_model requires an active Owner scope");
    owner.cache("hello_print.model", PrintUiModel::new)
}

/// R834 — the runtime-selected print backend (mirrors todomvc's
/// `AppStorage(Box<dyn Storage>)`): the real [`CupsPrintBackend`] when
/// CUPS has destinations, else the in-memory sample backend. Boxed behind
/// a newtype so [`Owner::cache`] stores one concrete type.
#[derive(Debug)]
struct AppPrintBackend {
    inner: Box<dyn PrintBackend>,
    /// `"cups"` or `"memory"` — surfaced through the introspect node so an
    /// AI client knows which path is live.
    kind: &'static str,
}

impl PrintBackend for AppPrintBackend {
    fn enumerate_printers(&self) -> Vec<PrinterInfo> {
        self.inner.enumerate_printers()
    }

    fn submit(&self, printer_id: &str, job: &PrintJob) -> Result<PrintReceipt, PrintError> {
        self.inner.submit(printer_id, job)
    }
}

/// Select the print backend: the real CUPS bridge when the scheduler is
/// reachable AND has at least one destination, else the in-memory sample
/// backend (this CI box has no destination). `PINION_PRINT_BACKEND=cups|
/// memory` forces a choice (the demo forces `memory` for determinism —
/// mirrors todomvc's `PINION_STORAGE_DIR` override).
fn build_print_backend() -> AppPrintBackend {
    let forced = std::env::var("PINION_PRINT_BACKEND").ok();
    match forced.as_deref() {
        Some("cups") => AppPrintBackend { inner: Box::new(CupsPrintBackend::new()), kind: "cups" },
        Some("memory") => AppPrintBackend {
            inner: Box::new(InMemoryPrintBackend::with_sample_printers()),
            kind: "memory",
        },
        _ => {
            let cups = CupsPrintBackend::new();
            if cups_available() && !cups.enumerate_printers().is_empty() {
                AppPrintBackend { inner: Box::new(cups), kind: "cups" }
            } else {
                AppPrintBackend {
                    inner: Box::new(InMemoryPrintBackend::with_sample_printers()),
                    kind: "memory",
                }
            }
        }
    }
}

fn use_backend() -> Rc<AppPrintBackend> {
    let owner = Owner::current().expect("use_backend requires an active Owner scope");
    owner.cache("hello_print.backend", build_print_backend)
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
        .with_label_font_size_px(BODY_FONT_PX)
        .with_focusable(true);
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

    /// R834 — Space / Enter activates the focused button; Arrow / Home /
    /// End rove between them — the shared
    /// [`pinion_core::focus_request::activate_or_rove`] SSOT (as
    /// hello-fab). The previous hand-rolled `invoke("key", …)` was dead
    /// code: `ButtonExternal` only handles the `"send"` action, so it
    /// never activated anything.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::focus_request::activate_or_rove(scene, &BUTTON_TAGS, focused, key)
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
fn submit_current_job(backend: &AppPrintBackend, model: &PrintUiModel) {
    let printers = backend.enumerate_printers();
    let Some(printer) = printers.get(model.selected.get()) else {
        return;
    };
    // R911 — render the document to a vector PDF (the §5.53 own-renderer
    // payload) and spool that, instead of a plain-text blob.
    let pdf = render_print_pdf();
    let job = PrintJob::new("hello-print document", pdf.clone()).with_copies(model.copies.get());
    if let Ok(receipt) = backend.submit(&printer.id, &job) {
        batch(|| {
            model.submit_count.set(model.submit_count.get() + 1);
            model.last_printer.set(receipt.printer_id);
            model.last_copies.set(receipt.copies);
            model.last_job.set(receipt.job_id);
            model.last_content.set(pdf);
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

    #[test]
    fn r911_print_payload_is_a_pdf_carrying_the_document() {
        // The print job content is now the own-rendered PDF, not text.
        let pdf = render_print_pdf();
        assert!(pdf.starts_with("%PDF-1.7"), "print payload is a PDF document");
        assert!(pdf.trim_end().ends_with("%%EOF"), "PDF trailer present");
        // US Letter MediaBox.
        assert!(pdf.contains("/MediaBox [0 0 612 792]"), "Letter page box");
        // The title + a body line are embedded as text-show operators.
        assert!(pdf.contains(&format!("({DOC_TITLE}) Tj")), "title embedded");
        assert!(pdf.contains(&format!("({}) Tj", DOC_BODY[0])), "first body line embedded");
        // The accent rule fills a rect.
        assert!(pdf.contains("re\nf\n"), "accent rule filled");
    }

    #[test]
    fn r911_submit_records_the_rendered_pdf() {
        let owner = Owner::new();
        owner.run(|| {
            let model = use_model();
            let backend = AppPrintBackend {
                inner: Box::new(InMemoryPrintBackend::with_sample_printers()),
                kind: "memory",
            };
            submit_current_job(&backend, &model);
            assert_eq!(model.submit_count.get(), 1);
            let content = model.last_content.get();
            assert!(content.starts_with("%PDF-1.7"), "recorded content is the rendered PDF");
            assert!(content.contains(&format!("({DOC_TITLE}) Tj")), "PDF carries the title");
        });
    }
}
