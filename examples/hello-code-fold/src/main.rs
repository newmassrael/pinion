//! `hello-code-fold` — R933 §5.22 §5.36 demo of the `TextField` widget's
//! code-folding surface.
//!
//! ## What it is (and is not)
//!
//! This binding is a **keyboard-interactive fold navigator** over a
//! read-only buffer (R955): the gutter shows code lines + fold chevrons, and
//! while it owns shell focus `ArrowUp` / `ArrowDown` move a current-line
//! cursor over the *visible* lines (stepping over a collapsed block, not into
//! its hidden interior) and `Enter` / `Space` fold / unfold the region at the
//! cursor. The current line is highlighted (the read-only viewer paints no
//! caret glyph — the line band is the cursor affordance). The text itself is
//! not editable here — typing / insertion is the `hello-textarea` axis; this
//! viewer exercises the R933 fold *substrate* with a cursor on top.
//!
//! The fold set + caret live on the reactive
//! [`TextEditState`](pinion_core::widgets::text_edit::TextEditState) (which
//! *is* fully editable, and tracks folds across edits — see its unit tests).
//! Both paths drive the same state: an AI agent folds / unfolds / moves the
//! caret purely over the wire (`toggle-fold` / `fold-all` / `unfold-all` /
//! `caret`), the §2 #2 "RPC headless is the primary path", and the keyboard
//! calls the same `TextEditState` methods — they converge on one mutation.
//!
//! **Deferred** (honest): pointer click-to-fold on a gutter chevron — the
//! fold rows are not composite-routable click targets yet (it needs a
//! `TextFieldExternal` click→toggle-fold path, the R954 `SelectItems`-send
//! analog).
//!
//! ## What it demonstrates
//!
//! The foldable regions are **derived from the buffer** (each bracket block
//! spanning ≥ 2 logical lines, found by the same R926 bracket scan reused
//! as `match_forward`), not declared by hand. Collapsing a region hides its
//! interior lines and leaves the opener line with a `…` placeholder.
//! Because the regions re-derive on read (the `find_matches` /
//! `matching_bracket` lineage), the rendered gutter, the `scene/snapshot`
//! paint, and the `scene/<tag>/external/fold_regions` RPC all read one
//! derivation and never disagree.
//!
//! Folding is **view state**, not document content: like the data widgets'
//! sort / filter / group, it is deliberately outside the undo journal
//! (Ctrl+Z reverses edits, never a fold). Collapsing a region that contains
//! the caret reanchors the caret to the opener line so a fold never strands
//! it on a hidden line.
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state + caret byte
//!   offset; the text + fold set live on the reactive `TextEditState`
//!   reached via `use_text_edit_state(TF_TAG)`.
//! - The folded code panel is rendered binding-local (one row per *visible*
//!   logical line, hidden lines skipped) rather than through the generic
//!   single-line `tf_paint::view_field`, because folding is a multi-line
//!   view transform `view_field` (one shaped layout) cannot express. This
//!   ~40-line view body is the seed of a future shared code-panel substrate
//!   — the moment a 2nd code-editor binding needs gutter + fold rendering it
//!   becomes the 2nd consumer and must lift ([[abstraction-needs-second-consumer]]).
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
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
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

/// R955 §5.22 §5.36 — the **logical** line (0-based) the caret byte offset
/// `caret` sits on: the count of `\n`s before it. The fold view is a
/// per-logical-line transform (not a parley single-block), so navigation
/// reasons in logical lines, computed from the text — no shaped layout.
fn caret_line(text: &str, caret: usize) -> usize {
    let c = caret.min(text.len());
    text.get(..c).map_or(0, |s| s.matches('\n').count())
}

/// R955 §5.22 — the byte offset where logical `line` starts (the position
/// after the `line`-th `\n`); clamps to the text end past the last line.
fn line_start_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    text.len()
}

/// R955 §5.22 §5.36 — the next **visible** logical line from `cur` in the
/// `down` direction, skipping lines hidden inside a collapsed region
/// ([`TextEditState::is_line_hidden`]); `None` at the visible end. Arrow
/// navigation steps over a collapsed block rather than into its hidden
/// interior — the editor's "fold acts like one line" rule.
fn next_visible_line(edit: &TextEditState, cur: usize, total: usize, down: bool) -> Option<usize> {
    let mut line = cur;
    loop {
        if down {
            line += 1;
            if line >= total {
                return None;
            }
        } else if line == 0 {
            return None;
        } else {
            line -= 1;
        }
        if !edit.is_line_hidden(line) {
            return Some(line);
        }
    }
}

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
fn view(state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let fg = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let st = use_text_edit_state(TF_TAG);
    let text = st.text();
    // R955 §5.22 — the keyboard cursor's logical line, highlighted as the
    // current line so a human sees where Arrow navigation / fold-toggle act
    // (the read-only viewer paints no caret glyph — the line band is the
    // cursor affordance). `state.1` is the caret byte offset.
    let cursor_line = caret_line(&text, state.1 as usize);
    // Derive the fold regions ONCE; the per-line hidden check reads this
    // `Vec` (O(regions)) instead of calling `st.is_line_hidden`, which would
    // re-run the O(text · openers) derivation for every line of the file.
    let regions = st.fold_regions();
    let hidden = |line: usize| {
        regions
            .iter()
            .any(|r| r.collapsed && line > r.start_line && line <= r.end_line)
    };

    let total = text.split('\n').count();
    let mut rows: Vec<Scene> = Vec::new();
    let mut visible = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        if hidden(i) {
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
        let mut row = ContainerNode::new(vec![gutter, code])
            .with_tag(format!("fold_row_{i}"))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_gap(GUTTER_GAP));
        if i == cursor_line {
            // Current-line band — a step lighter than the panel so the active
            // line reads as the keyboard cursor's position.
            row = row.with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)));
        }
        rows.push(Scene::Container(row));
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

    /// R955 §5.22 §5.36 — the viewer is a single Tab stop so the keyboard
    /// fold navigation reaches it.
    fn focusable_tags() -> Vec<&'static str> {
        vec![TF_TAG]
    }

    /// R955 §5.22 §5.36 — keyboard fold navigation. While the viewer owns
    /// shell focus: `ArrowUp` / `ArrowDown` move the cursor to the previous /
    /// next **visible** logical line (stepping over a collapsed block, not
    /// into its hidden interior), `Home` / `End` jump to the first / last
    /// visible line, and `Enter` / `Space` toggle the fold at the cursor line.
    /// Drives the reactive `TextEditState` directly (the `hello-textarea`
    /// pattern) — the same `caret` / `toggle-fold` an AI client drives over
    /// the §5.12 RPC plane, so the two paths converge on one mutation.
    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        let edit = use_text_edit_state(TF_TAG);
        let text = edit.text();
        let total = text.split('\n').count();
        let cur = caret_line(&text, edit.caret());
        match key {
            "ArrowDown" => {
                if let Some(next) = next_visible_line(&edit, cur, total, true) {
                    edit.set_caret(line_start_byte(&text, next));
                }
                true
            }
            "ArrowUp" => {
                if let Some(prev) = next_visible_line(&edit, cur, total, false) {
                    edit.set_caret(line_start_byte(&text, prev));
                }
                true
            }
            "Home" => {
                edit.set_caret(0);
                true
            }
            "End" => {
                let mut last = total.saturating_sub(1);
                while last > 0 && edit.is_line_hidden(last) {
                    last -= 1;
                }
                edit.set_caret(line_start_byte(&text, last));
                true
            }
            "Enter" | "Space" => {
                edit.toggle_fold(cur);
                true
            }
            _ => false,
        }
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

    use super::{caret_line, line_start_byte, view, FoldView, SEED_CODE, STATUS_TAG, TF_TAG};
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Modifiers, Scene, WidgetCore};

    /// A live fold-viewer scene (one composite `Scene::External` at `TF_TAG`)
    /// so `apply_key` walks the shell's exact topology. `apply_key` reads the
    /// reactive `TextEditState` (not the scene), so it shares state with the
    /// `use_text_edit_state(TF_TAG)` the test seeds in the same `Owner`.
    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(FoldView::create_external()).with_tag(TF_TAG))
    }

    fn press(scene: &mut Scene, key: &str) {
        assert!(
            FoldView::apply_key(scene, Some(TF_TAG), key, Modifiers::default()),
            "the fold viewer handles {key}",
        );
    }

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

    #[test]
    fn r955_arrows_move_the_cursor_line() {
        Owner::new().run(|| {
            let st = use_text_edit_state(TF_TAG);
            st.set_text(SEED_CODE.to_string());
            st.set_caret(0);
            let mut scene = scene_fixture();
            press(&mut scene, "ArrowDown");
            assert_eq!(caret_line(&st.text(), st.caret()), 1, "ArrowDown -> line 1");
            press(&mut scene, "ArrowDown");
            assert_eq!(caret_line(&st.text(), st.caret()), 2, "ArrowDown -> line 2");
            press(&mut scene, "ArrowUp");
            assert_eq!(caret_line(&st.text(), st.caret()), 1, "ArrowUp -> line 1");
            press(&mut scene, "End");
            assert_eq!(caret_line(&st.text(), st.caret()), 5, "End -> last line");
            press(&mut scene, "Home");
            assert_eq!(caret_line(&st.text(), st.caret()), 0, "Home -> first line");
        });
    }

    #[test]
    fn r955_enter_toggles_the_fold_at_the_cursor_line() {
        Owner::new().run(|| {
            let st = use_text_edit_state(TF_TAG);
            st.set_text(SEED_CODE.to_string());
            st.set_caret(0); // line 0 = the outer foldable opener
            let mut scene = scene_fixture();
            let collapsed_at0 = || st.fold_regions().iter().any(|r| r.start_line == 0 && r.collapsed);
            assert!(!collapsed_at0(), "the outer block is open at the start");
            press(&mut scene, "Enter");
            assert!(collapsed_at0(), "Enter folded the region at the cursor line");
            press(&mut scene, "Enter");
            assert!(!collapsed_at0(), "Enter again unfolded it");
        });
    }

    #[test]
    fn r955_arrow_down_steps_over_a_collapsed_block() {
        Owner::new().run(|| {
            let st = use_text_edit_state(TF_TAG);
            st.set_text(SEED_CODE.to_string());
            // Collapse the inner block (opens line 2, closes line 4): lines 3,4
            // go hidden, so from line 2 an ArrowDown lands on line 5, never on a
            // hidden interior line.
            assert!(st.toggle_fold(2));
            st.set_caret(line_start_byte(&st.text(), 2));
            let mut scene = scene_fixture();
            press(&mut scene, "ArrowDown");
            let line = caret_line(&st.text(), st.caret());
            assert_eq!(line, 5, "ArrowDown stepped over the collapsed inner block to line 5");
            assert!(!st.is_line_hidden(line), "the cursor never lands on a hidden line");
        });
    }
}
