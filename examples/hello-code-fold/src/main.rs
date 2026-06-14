//! `hello-code-fold` — R933 §5.22 §5.36 code-folding consumer for the
//! `TextField` widget's fold surface.
//!
//! ## What it demonstrates
//!
//! The depth slice that turns a code editor into a *foldable* one: the
//! foldable regions are **derived from the buffer** (each bracket block
//! spanning ≥ 2 logical lines, found by the same R926 bracket scan reused
//! as `match_forward`), not declared by hand. Collapsing a region hides
//! its interior lines and leaves the opener line with a `…` placeholder —
//! the VS Code / Sublime default. Because the fold set lives on the
//! reactive
//! [`TextEditState`](pinion_core::widgets::text_edit::TextEditState) and
//! the regions re-derive on read (the `find_matches` / `matching_bracket`
//! lineage), the rendered gutter, the `scene/snapshot` paint, and the
//! `scene/<tag>/external/fold_regions` RPC all read one derivation and an
//! AI agent folds / unfolds purely over the wire (`toggle-fold` /
//! `fold-all` / `unfold-all`).
//!
//! Folding is **view state**, not document content: like the data
//! widgets' sort / filter / group, it is deliberately outside the undo
//! journal (the Qt / Unreal convention — Ctrl+Z reverses edits, never a
//! fold). Collapsing a region that contains the caret reanchors the caret
//! to the opener line so a fold never strands it on a hidden line.
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state + caret byte
//!   offset; the text + fold set live on the reactive `TextEditState`
//!   reached via `use_text_edit_state(TF_TAG)`.
//! - The folded code panel is rendered binding-local (one row per *visible*
//!   logical line, hidden lines skipped) rather than through the generic
//!   single-line `tf_paint::view_field`, because folding is a multi-line
//!   code-editor view transform.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p hello-code-fold
//! python3 tools/demos/r933_code_fold.py
//! ```

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
// R657 §5.16 §5.38 — lifted TextField substrate: state-scene introspect
// read + the textbox ARIA node SSOT (the only two helpers this fold view
// borrows; the folded panel itself is rendered binding-local).
use pinion_widget_paint::text_field as tf_paint;

// pinion-forge codegen output. Defines `pub struct HelloCodeFoldRenderer`
// + async `new<W>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>` can
// construct + render + resize it.
vello_renderer_impl!(HelloCodeFoldRenderer, HelloCodeFoldRendererError);

/// Tag for the code-editor widget — the `WidgetCore::tag` (paint-root +
/// input-router hit-test target) and the `use_text_edit_state` cache key,
/// so `create_external` and the view fn resolve to the same reactive
/// `Rc<TextEditState>`.
const TF_TAG: &str = "code_editor";

/// Status line tag so the demo reads "{n}/{m} lines visible, {k} folded"
/// as scene-as-data (`find_by_tag`).
const STATUS_TAG: &str = "fold_status";

/// [`ThemeProvider`] cache key — the `"app"` gallery convention.
const THEME_TAG: &str = "app";

const WIN_W: u32 = 520;
const WIN_H: u32 = 340;

const TITLE_FONT_PX: u32 = 18;
const CODE_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 14;
const LINE_GAP: u32 = 2;
const GUTTER_GAP: u32 = 12;

// R933 §5.36 — fold-affordance glyphs. Per the repo non-ASCII-in-source
// rule each is a named const with a `\u{..}` escape (the doc names the
// glyph the escape encodes).
/// `\u{25be}` (▾) — an expanded foldable block (collapse to `▸`).
const CHEVRON_EXPANDED: &str = "\u{25be}";
/// `\u{25b8}` (▸) — a collapsed block (expand to `▾`).
const CHEVRON_COLLAPSED: &str = "\u{25b8}";
/// A space — gutter filler on a non-foldable line, keeping the number
/// column aligned with the chevron column.
const CHEVRON_NONE: &str = " ";
/// `\u{2026}` (…) — the placeholder appended to a collapsed block's
/// opener line.
const FOLD_ELLIPSIS: &str = "\u{2026}";

/// R933 §5.36 — the seeded code. Two nested brace blocks: the `fn` body
/// (opener line 0, closer line 5) and the `if` body (opener line 2, closer
/// line 4); the same-line `()` calls are not foldable (one line each).
const SEED_CODE: &str =
    "fn main() {\n    let x = 1;\n    if x > 0 {\n        log(x);\n    }\n}";

/// view-fn (§6.3): pure-ish sync mapping `(state, frame) -> Scene`. The
/// reactive reads inside `use_text_edit_state` / `use_theme` subscribe, so
/// the same reactive store state yields the same `Scene`.
///
/// Layout (top-to-bottom, centered): title, the folded code panel (one row
/// per *visible* logical line — gutter chevron + 1-based number + the code,
/// a `…` appended to a collapsed opener), and the status line.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors WidgetCore::view (&Frame)"
)]
fn view(_state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let fg = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let st = use_text_edit_state(TF_TAG);
    let text = st.text();
    let regions = st.fold_regions();

    let total = text.split('\n').count();
    let mut rows: Vec<Scene> = Vec::new();
    let mut visible = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        if st.is_line_hidden(i) {
            continue;
        }
        visible += 1;
        // A foldable region opening on this line drives the chevron + the
        // collapsed-line placeholder.
        let opener = regions.iter().find(|r| r.start_line == i);
        let chevron = match opener {
            Some(r) if r.collapsed => CHEVRON_COLLAPSED,
            Some(_) => CHEVRON_EXPANDED,
            None => CHEVRON_NONE,
        };
        let gutter = Scene::Text(TextNode::styled(
            format!("{chevron} {:>2}", i + 1),
            Rect::default(),
            TextStyle::new().with_size_px(CODE_FONT_PX).with_fg(muted),
        ));
        let code_text = if opener.is_some_and(|r| r.collapsed) {
            format!("{line} {FOLD_ELLIPSIS}")
        } else {
            line.to_string()
        };
        let code = Scene::Text(TextNode::styled(
            code_text,
            Rect::default(),
            TextStyle::new().with_size_px(CODE_FONT_PX).with_fg(fg),
        ));
        rows.push(Scene::Container(
            ContainerNode::new(vec![gutter, code])
                .with_tag(format!("fold_row_{i}"))
                .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_gap(GUTTER_GAP)),
        ));
    }

    let code_panel = Scene::Container(
        ContainerNode::new(rows)
            // R55.G.17 — the paint scene must carry `WidgetCore::tag` so
            // `scene/<tag>` routing + `rect_for_tag` resolve.
            .with_tag(TF_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(LINE_GAP)),
    );

    let collapsed = regions.iter().filter(|r| r.collapsed).count();
    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            format!("{visible}/{total} lines visible, {collapsed} folded"),
            Rect::default(),
            TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(muted),
        ))])
        .with_tag(STATUS_TAG),
    );

    let title = Scene::Text(TextNode::styled(
        "Code Folding",
        Rect::default(),
        TextStyle::new().with_size_px(TITLE_FONT_PX).with_fg(fg),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, code_panel, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding for the code-folding editor. State shape mirrors
/// the other `TextField` bindings; the text + fold set are reactive on the
/// `TextEditState` the External attaches and the view fn reads.
struct FoldView;

impl WidgetCore for FoldView {
    type State = (TextFieldState, u32);
    type Event = TextFieldEvent;

    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(TF_TAG);
        if text_state.text().is_empty() {
            text_state.set_text(SEED_CODE.to_string());
        }
        Box::new(TextFieldExternal::new().attach_state(text_state))
    }

    fn tag() -> &'static str {
        TF_TAG
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, TF_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-code-fold (R933 §5.22 §5.36)"
    }
}

impl WidgetA11y for FoldView {
    /// ARIA `textbox` node carrying the live (unfolded) buffer — folding is
    /// a presentation transform, so the AT contract reports the whole
    /// document. Routed through the lifted `tf_paint::text_field_a11y_node`
    /// SSOT every text-field binding shares.
    fn access_node(state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret) = state;
        let text = use_text_edit_state(TF_TAG).text();
        let tag = <Self as WidgetCore>::tag();
        vec![tf_paint::text_field_a11y_node(tag, text, *interaction, focused == Some(tag))]
    }
}

impl WidgetView for FoldView {
    type Renderer = HelloCodeFoldRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<FoldView>();
}

#[cfg(test)]
mod tests {
    //! R933 §5.36 — binding-level regression: paint-root tag presence +
    //! the folded view actually drops the hidden rows.

    use super::{view, SEED_CODE, STATUS_TAG, TF_TAG};
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene};

    #[test]
    fn r933_view_carries_paint_root_and_status_tags() {
        Owner::new().run(|| {
            use_text_edit_state(TF_TAG).set_text(SEED_CODE.to_string());
            let scene: Scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag(TF_TAG), "R55.G.17 paint-root tag");
            assert!(scene.contains_tag(STATUS_TAG), "status line tag");
            // All six logical lines visible when nothing is folded.
            assert!(scene.contains_tag("fold_row_0"));
            assert!(scene.contains_tag("fold_row_5"));
        });
    }

    #[test]
    fn r933_collapsed_view_drops_hidden_rows() {
        Owner::new().run(|| {
            let st = use_text_edit_state(TF_TAG);
            st.set_text(SEED_CODE.to_string());
            // Collapse the outer block (opens on line 0) — interior rows go.
            assert!(st.toggle_fold(0));
            let scene: Scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag("fold_row_0"), "opener row stays visible");
            assert!(!scene.contains_tag("fold_row_1"), "interior row hidden");
            assert!(!scene.contains_tag("fold_row_3"), "interior row hidden");
            assert!(!scene.contains_tag("fold_row_5"), "closer row hidden");
        });
    }
}
