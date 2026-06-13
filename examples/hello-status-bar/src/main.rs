//! `hello-status-bar` — R913 §5.38 §5.22 editor status bar.
//!
//! ## What this demonstrates
//!
//! The bottom chrome of an editor shell (VS Code / `JetBrains` status bar):
//! a reactive cursor-position segment (`Ln L, Col C`), clickable
//! language-mode and encoding segments that cycle on click, and a
//! message area. It rounds out the self-hosted-editor shell alongside
//! the R912 command palette and the R909/R910 inspector.
//!
//! - A primary [`StatusBarExternal`] holds the cursor position, the
//!   mode / encoding indices, and a message (the reactive-holder
//!   pattern; the cursor position is what a real editor would feed in
//!   from its buffer).
//! - Clickable segments are tagged `statusbar#mode` / `statusbar#encoding`
//!   — a click routes through the R51.42 `#`-split to the External `send`
//!   wire, which cycles the **named** segment on the activation edge
//!   (a named-action send, the toolbar-button cousin of the indexed
//!   row-select wire R910/R912 lifted into `send_activation_index`).
//!
//! ## §2 AI-first
//!
//! `intervene line` / `col` / `message` feed the bar; `invoke cycle_mode`
//! / `cycle_encoding` (or a click) advance the pickers; `query position`
//! / `mode` / `encoding` / `message` read it back — all scene-as-data.
//!
//! ## Verification
//!
//! `tools/demos/r913_status_bar.py` drives it over RPC: set the cursor →
//! the position segment reflects it; cycle the mode (RPC + click) → it
//! advances and wraps; set a message → it shows. Deterministic.

use std::rc::Rc;

use pinion_core::composite_tag::send_activation_key;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloStatusBarRenderer, HelloStatusBarRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 120;

const STATUS_TAG: &str = "statusbar";
const THEME_TAG: &str = "app";

const SEG_FONT_PX: u32 = 13;
const BAR_H: u32 = 26;

/// Language modes the mode segment cycles through (a click advances,
/// wrapping). The roster a code editor would populate from its language
/// registry.
const MODES: [&str; 5] = ["Plain Text", "Rust", "Python", "Markdown", "JSON"];

/// Text encodings the encoding segment cycles through.
const ENCODINGS: [&str; 3] = ["UTF-8", "UTF-16 LE", "Latin-1"];

// ─── Shared reactive state ────────────────────────────────────────

fn use_line() -> Rc<Signal<u32>> {
    Owner::current().expect("Owner").cache("statusbar.line", || Signal::new(1u32))
}
fn use_col() -> Rc<Signal<u32>> {
    Owner::current().expect("Owner").cache("statusbar.col", || Signal::new(1u32))
}
fn use_mode() -> Rc<Signal<usize>> {
    Owner::current().expect("Owner").cache("statusbar.mode", || Signal::new(1usize)) // Rust
}
fn use_encoding() -> Rc<Signal<usize>> {
    Owner::current().expect("Owner").cache("statusbar.encoding", || Signal::new(0usize))
}
fn use_message() -> Rc<Signal<String>> {
    Owner::current().expect("Owner").cache("statusbar.message", || Signal::new(String::new()))
}

// ─── External ─────────────────────────────────────────────────────

struct StatusBarExternal {
    line: Rc<Signal<u32>>,
    col: Rc<Signal<u32>>,
    mode: Rc<Signal<usize>>,
    encoding: Rc<Signal<usize>>,
    message: Rc<Signal<String>>,
}

impl StatusBarExternal {
    fn cycle_mode(&self) {
        self.mode.set((self.mode.get() + 1) % MODES.len());
    }
    fn cycle_encoding(&self) {
        self.encoding.set((self.encoding.get() + 1) % ENCODINGS.len());
    }
    fn position(&self) -> String {
        format!("Ln {}, Col {}", self.line.get(), self.col.get())
    }
}

impl core::fmt::Debug for StatusBarExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StatusBarExternal").field("position", &self.position()).finish_non_exhaustive()
    }
}

impl External for StatusBarExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for StatusBarExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("line", "int"),
            ("col", "int"),
            ("position", "string"),
            ("mode", "string"),
            ("mode_index", "int"),
            ("encoding", "string"),
            ("message", "string"),
            ("cycle_mode", "json"),
            ("cycle_encoding", "json"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "line" => Some(IntrospectValue::Int(i64::from(self.line.get()))),
            "col" => Some(IntrospectValue::Int(i64::from(self.col.get()))),
            "position" => Some(IntrospectValue::Text(self.position())),
            "mode" => Some(IntrospectValue::Text(MODES[self.mode.get() % MODES.len()].to_owned())),
            "mode_index" => Some(IntrospectValue::Int(i64::try_from(self.mode.get()).ok()?)),
            "encoding" => {
                Some(IntrospectValue::Text(ENCODINGS[self.encoding.get() % ENCODINGS.len()].to_owned()))
            }
            "message" => Some(IntrospectValue::Text(self.message.get())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "line" => match value {
                IntrospectValue::Int(n) => {
                    self.line.set(u32::try_from(n).map_err(|_| InterveneError::OutOfRange)?.max(1));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "col" => match value {
                IntrospectValue::Int(n) => {
                    self.col.set(u32::try_from(n).map_err(|_| InterveneError::OutOfRange)?.max(1));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "message" => match value {
                IntrospectValue::Text(text) => {
                    self.message.set(text);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "position" | "mode" | "encoding" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            "cycle_mode" => {
                self.cycle_mode();
                Ok(IntrospectValue::Text(MODES[self.mode.get()].to_owned()))
            }
            "cycle_encoding" => {
                self.cycle_encoding();
                Ok(IntrospectValue::Text(ENCODINGS[self.encoding.get()].to_owned()))
            }
            // Named-segment send wire: a click on `statusbar#mode` /
            // `statusbar#encoding` cycles that segment on the activation
            // edge (the named-action cousin of the indexed row-select
            // `send_activation_index`; the key here is a segment name,
            // not a numeric index).
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    match send_activation_key(payload) {
                        Some("mode") => self.cycle_mode(),
                        Some("encoding") => self.cycle_encoding(),
                        _ => {}
                    }
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn make_status_bar_external() -> StatusBarExternal {
    StatusBarExternal {
        line: use_line(),
        col: use_col(),
        mode: use_mode(),
        encoding: use_encoding(),
        message: use_message(),
    }
}

// ─── View ─────────────────────────────────────────────────────────

/// One status-bar segment: a label in a pill. `tag` `Some(name)` makes
/// it clickable (composite `statusbar#<name>` → the `send` wire).
fn segment(label: String, name: Option<&str>, fg: Color, fill: Color) -> Scene {
    let mut node = ContainerNode::new(vec![Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(SEG_FONT_PX).with_fg(fg),
    ))])
    .with_style(BoxStyle::filled(fill).with_corner_radius(4))
    .with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_align_items(AlignItems::Center)
            .with_justify(JustifyContent::Center)
            .with_size(Size::px(0, BAR_H))
            .with_padding(Rect::new(8, 0, 8, 0)),
    );
    if let Some(name) = name {
        node = node.with_tag(format!("{STATUS_TAG}#{name}"));
    }
    Scene::Container(node)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_mode_index: usize, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_accent = Color::rgb(0xff, 0xff, 0xff);
    let accent = theme.resolve(ColorRole::Accent);
    let surface = theme.resolve(ColorRole::Surface);
    let bar_bg = theme.resolve(ColorRole::SurfaceContainerHighest);
    let seg_fg = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let position = format!("Ln {}, Col {}", use_line().get(), use_col().get());
    let mode = MODES[use_mode().get() % MODES.len()];
    let encoding = ENCODINGS[use_encoding().get() % ENCODINGS.len()];
    let message = use_message().get();

    // Left: cursor position. Middle: message (flex spacer). Right:
    // clickable mode + encoding pills.
    let pos_seg = segment(position, None, seg_fg, Color::TRANSPARENT);
    let msg_seg = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            if message.is_empty() { "Ready".to_owned() } else { message },
            Rect::default(),
            TextStyle::new().with_size_px(SEG_FONT_PX).with_fg(muted),
        ))])
        .with_tag("statusbar_message")
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_flex_grow(1.0)
                .with_padding(Rect::new(8, 0, 8, 0)),
        ),
    );
    let mode_seg = segment(mode.to_owned(), Some("mode"), on_accent, accent);
    let enc_seg = segment(encoding.to_owned(), Some("encoding"), seg_fg, bar_bg);

    let bar = Scene::Container(
        ContainerNode::new(vec![pos_seg, msg_seg, mode_seg, enc_seg])
            .with_tag(STATUS_TAG)
            .with_style(BoxStyle::filled(bar_bg))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_size(Size::px(WIN_W, BAR_H + 8))
                    .with_padding(Rect::new(8, 4, 8, 4)),
            ),
    );

    // The bar sits at the bottom of a surface (an editor body above it).
    Scene::Container(
        ContainerNode::new(vec![bar])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::End)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── Binding ──────────────────────────────────────────────────────

#[widget(
    tag = "statusbar",
    state = usize,
    event = (),
    title = "pinion hello-status-bar (R913 §5.38 editor status bar)",
    renderer = HelloStatusBarRenderer,
    initial_size = (WIN_W, WIN_H),
    external = make_status_bar_external,
    role = Status,
)]
struct StatusBarView;

impl StatusBarView {
    fn read_state(scene: &Scene) -> usize {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Int(n)) = intro.query("mode_index") {
                    return usize::try_from(n).unwrap_or(0);
                }
            }
        }
        0
    }

    fn view(state: usize, frame: Frame) -> Scene {
        view(state, &frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }
}

fn main() {
    pinion_shell::run::<StatusBarView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> StatusBarExternal {
        Owner::new().run(make_status_bar_external)
    }

    #[test]
    fn r913_position_formats_line_and_col() {
        let mut e = ext();
        e.intervene("line", IntrospectValue::Int(42)).unwrap();
        e.intervene("col", IntrospectValue::Int(7)).unwrap();
        assert_eq!(e.query("position"), Some(IntrospectValue::Text("Ln 42, Col 7".to_owned())));
    }

    #[test]
    fn r913_line_col_floor_at_one() {
        let mut e = ext();
        assert_eq!(e.intervene("line", IntrospectValue::Int(0)), Ok(()));
        assert_eq!(e.query("line"), Some(IntrospectValue::Int(1)), "line floors at 1");
        assert!(e.intervene("line", IntrospectValue::Int(-3)).is_err(), "negative rejected");
    }

    #[test]
    fn r913_cycle_mode_advances_and_wraps() {
        let mut e = ext();
        // Boot mode = Rust (index 1).
        assert_eq!(e.query("mode"), Some(IntrospectValue::Text("Rust".to_owned())));
        for _ in 0..MODES.len() {
            e.invoke("cycle_mode", IntrospectValue::Null).unwrap();
        }
        // A full cycle returns to Rust (wrap).
        assert_eq!(e.query("mode"), Some(IntrospectValue::Text("Rust".to_owned())));
    }

    #[test]
    fn r913_cycle_encoding_advances() {
        let mut e = ext();
        assert_eq!(e.query("encoding"), Some(IntrospectValue::Text("UTF-8".to_owned())));
        e.invoke("cycle_encoding", IntrospectValue::Null).unwrap();
        assert_eq!(e.query("encoding"), Some(IntrospectValue::Text("UTF-16 LE".to_owned())));
    }

    #[test]
    fn r913_named_send_cycles_the_clicked_segment() {
        let mut e = ext();
        // Hover does not cycle; the PointerUp activation edge does.
        e.invoke("send", IntrospectValue::Text("mode:PointerEnter".to_owned())).unwrap();
        assert_eq!(e.query("mode"), Some(IntrospectValue::Text("Rust".to_owned())));
        e.invoke("send", IntrospectValue::Text("mode:PointerUp".to_owned())).unwrap();
        assert_eq!(e.query("mode"), Some(IntrospectValue::Text("Python".to_owned())));
        // The encoding segment is independent.
        e.invoke("send", IntrospectValue::Text("encoding:PointerUp".to_owned())).unwrap();
        assert_eq!(e.query("encoding"), Some(IntrospectValue::Text("UTF-16 LE".to_owned())));
    }

    #[test]
    fn r913_message_round_trips() {
        let mut e = ext();
        e.intervene("message", IntrospectValue::Text("Saved 3 files".to_owned())).unwrap();
        assert_eq!(e.query("message"), Some(IntrospectValue::Text("Saved 3 files".to_owned())));
    }

    #[test]
    fn r913_read_only_axes_reject_intervene() {
        let mut e = ext();
        assert_eq!(e.intervene("mode", IntrospectValue::Text("x".to_owned())), Err(InterveneError::ReadOnly));
        assert_eq!(e.intervene("position", IntrospectValue::Text("x".to_owned())), Err(InterveneError::ReadOnly));
    }

    fn has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| has_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn view_shows_position_and_mode() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            use_line().set(10);
            use_col().set(3);
            view(0, &Frame::new())
        });
        assert!(has_text(&scene, "Ln 10, Col 3"), "position rendered");
        assert!(has_text(&scene, "Rust"), "mode segment rendered");
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<StatusBarView>(0, &Frame::new());
    }

    #[test]
    fn status_bar_root_reports_status_role() {
        let nodes = <StatusBarView as WidgetA11y>::access_node(&0, None);
        assert_eq!(nodes[0].role, AriaRole::Status);
        assert_eq!(nodes[0].tag, STATUS_TAG);
    }
}
