//! `hello-textarea` — R764 §5.22 §5.36 multi-line `TextField` (textarea)
//! consumer.
//!
//! ## What it demonstrates
//!
//! The multi-line text-editing substrate landed in R764 on top of the
//! existing single-line `TextField` (R56.1.b.1) + click-caret (R762) +
//! pointer-selection (R763) stack. The same [`TextFieldExternal`] +
//! [`TextEditState`](pinion_core::widgets::text_edit::TextEditState) +
//! [`tf_paint`] substrate drives both — a textarea is a `TextField`
//! whose content carries `\n` and whose [`TextFieldStyle::m3_multiline`]
//! style top-aligns a `rows`-tall box. The pieces R764 adds:
//!
//! - **Per-line selection bands** — [`tf_paint::view_field`] decomposes
//!   a selection range into one rect per visual line
//!   (`pinion_text::selection_rects_for_range` → parley
//!   `Selection::geometry`), so a drag / Shift-select that spans hard
//!   line breaks paints the partial-first / full-middle / partial-last
//!   band shape a text editor draws.
//! - **Vertical caret navigation** — `ArrowUp` / `ArrowDown` resolve the
//!   adjacent-line byte via
//!   [`tf_paint::byte_for_field_vertical_move`]
//!   (`pinion_text::byte_offset_for_line_move` → parley
//!   `Selection::move_lines`). This is geometry-dependent so it runs in
//!   the binding's `apply_key` (which holds the layout cache), not on
//!   the geometry-free `TextEditState` path the horizontal arrows use.
//! - **`Enter` inserts a newline** — the one binding-level divergence
//!   from a single-line field (where `Enter` would submit / be ignored).
//!
//! Click-to-position, drag-select, and Shift-click all reuse the R762 /
//! R763 hooks unchanged — `byte_for_field_point` hit-tests against the
//! multi-line layout (parley `Cursor::from_point` already picks the line
//! by `y`), so a click on line 3 lands on line 3 with no extra work.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p hello-textarea
//! ```
//!
//! Tab in → caret blinks. Type → text inserts; `Enter` starts a new
//! line. `ArrowUp` / `ArrowDown` move between lines (hold `Shift` to
//! select); drag across lines to select a multi-line band.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
use pinion_widget_paint::text_field as tf_paint;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTextAreaRenderer, HelloTextAreaRendererError);

/// Paint-root + input-router + reactive-cache tag for the textarea.
const TA_TAG: &str = "main_textarea";
/// Shared [`ThemeProvider`] cache key (matches the gallery `"app"`
/// convention so a host binding shares one provider).
const THEME_TAG: &str = "app";
/// Visible rows for the textarea box.
const TA_ROWS: u32 = 5;

const WIN_W: u32 = 480;
const WIN_H: u32 = 320;

const TITLE_FONT_SIZE_PX: u32 = 18;
const STATUS_FONT_SIZE_PX: u32 = 12;
const ROW_GAP: u32 = 16;

/// R764 §5.22 — single source of truth for the textarea's
/// [`TextFieldStyle`]. The view fn, the pointer hooks, and the
/// vertical-nav `apply_key` arm all shape against this *identical*
/// style so the painted Layout and the hit-tested / line-moved Layout
/// stay one cache entry (the R762.1 `field_shaping` SSOT discipline).
fn ta_style() -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle::m3_multiline(TA_ROWS)
}

/// R56.2.b §5.22 — `Sized` wrapper around `Box<dyn Clipboard>` so the
/// [`use_clipboard`] hook parks either an [`ArboardClipboard`] or an
/// [`InMemoryClipboard`] in one `Owner::cache` slot (mirror of the
/// hello-textfield pattern).
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
}

/// `Owner::cache`-keyed clipboard hook — platform `arboard` with an
/// in-memory fallback when the platform daemon is unreachable.
fn use_clipboard(key: &'static str) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("use_clipboard requires an active Owner scope")
        .cache(key, || {
            AppClipboard(match ArboardClipboard::try_new() {
                Ok(arboard) => Box::new(arboard) as Box<dyn Clipboard>,
                Err(e) => {
                    eprintln!(
                        "hello-textarea: ArboardClipboard init failed ({e}); \
                         falling back to InMemoryClipboard",
                    );
                    Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>
                }
            })
        });
    cb
}

/// View: title + the multi-line field + a status line mirroring the
/// live `(state, caret, selection)` so the AI side verifies the same
/// data the visible field renders via `scene/query`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors hello-textfield"
)]
fn view(state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let (interaction, caret_byte) = state;
    let theme = use_theme(THEME_TAG).theme_animated();

    let field = tf_paint::view_field(
        TA_TAG,
        interaction,
        caret_byte,
        &theme,
        &ta_style(),
        "Multi-line text input",
    );

    let title = Scene::Text(TextNode::styled(
        "TextArea",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let text_state = use_text_edit_state(TA_TAG);
    let text = text_state.text();
    let lines = text.split('\n').count();
    let selection = match text_state.selection_range() {
        Some((start, end)) => format!(" | sel={start}..{end}"),
        None => String::new(),
    };
    let status_str = format!(
        "{} | caret={} | lines={}{}",
        interaction.as_name(),
        caret_byte,
        lines,
        selection,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, field, status])
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

struct TextAreaView;

impl WidgetCore for TextAreaView {
    type State = (TextFieldState, u32);
    type Event = TextFieldEvent;

    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(TA_TAG);
        // Seed multi-line content so the textarea reads as multi-line at
        // boot (parley breaks on the explicit `\n`).
        if text_state.text().is_empty() {
            text_state.set_text("first line\nsecond line\nthird line".to_owned());
            text_state.set_caret(0);
        }
        let blink = use_caret_blink(TA_TAG);
        let clipboard = use_clipboard(TA_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
        )
    }

    fn tag() -> &'static str {
        TA_TAG
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, TA_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-textarea (R764 §5.22)"
    }

    /// R764 §5.22 — multi-line key handling. Three keys diverge from the
    /// single-line field; everything else forwards to the External's
    /// modifier-aware `invoke("key", ...)` channel exactly like
    /// hello-textfield.
    ///
    /// - `Enter` inserts a newline (a single-line field would submit).
    /// - `ArrowUp` / `ArrowDown` move the caret one visual line, holding
    ///   the horizontal position, via the layout-geometry helper
    ///   ([`tf_paint::byte_for_field_vertical_move`]). Shift extends the
    ///   selection from the retained anchor. These run here (not on the
    ///   `TextEditState` `apply_key` path) because vertical movement
    ///   needs the shaped layout the binding's cache holds.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        match key {
            "Enter" => {
                use_text_edit_state(TA_TAG).insert("\n");
                return true;
            }
            "ArrowUp" | "ArrowDown" => {
                let (interaction, _caret) = Self::read_state(scene);
                let theme = use_theme(THEME_TAG).theme_animated();
                let delta = if key == "ArrowUp" { -1 } else { 1 };
                let new_byte = tf_paint::byte_for_field_vertical_move(
                    TA_TAG,
                    interaction,
                    delta,
                    &theme,
                    &ta_style(),
                );
                let edit = use_text_edit_state(TA_TAG);
                if modifiers.shift_key() {
                    let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
                    edit.set_selection(anchor, new_byte);
                } else {
                    edit.set_caret(new_byte);
                }
                return true;
            }
            _ => {}
        }
        // Forward everything else to the External (modifier-aware Json
        // shape so native Shift+Arrow / Ctrl+A reach the substrate's
        // selection arms — R763).
        let Some(node) = scene.find_external_with_tag_mut(TA_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let args = if modifiers == pinion_core::Modifiers::empty() {
            IntrospectValue::Text(key.to_owned())
        } else {
            IntrospectValue::Json(serde_json::json!({
                "key": key,
                "shift": modifiers.shift_key(),
                "ctrl": modifiers.control_key(),
                "alt": modifiers.alt_key(),
                "meta": modifiers.meta_key(),
            }))
        };
        match intro.invoke("key", args) {
            Ok(IntrospectValue::Bool(handled)) => handled,
            _ => false,
        }
    }

    /// R56.2.a §5.13 — platform IME composition (mirror of
    /// hello-textfield; the textarea reuses the same substrate funnel).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(TA_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let args = match event {
            pinion_core::CompositionEvent::Start => {
                IntrospectValue::Json(serde_json::json!({ "action": "start" }))
            }
            pinion_core::CompositionEvent::Update(text) => {
                IntrospectValue::Json(serde_json::json!({ "action": "update", "data": text }))
            }
            pinion_core::CompositionEvent::Commit(text) => {
                IntrospectValue::Json(serde_json::json!({ "action": "end", "data": text }))
            }
            pinion_core::CompositionEvent::Cancel => {
                IntrospectValue::Json(serde_json::json!({ "action": "cancel" }))
            }
            _ => return false,
        };
        intro.invoke("composition", args).is_ok()
    }

    fn fmt_state_log(state: &(TextFieldState, u32)) -> String {
        format!("{} / caret={}", state.0.as_name(), state.1)
    }
}

impl WidgetA11y for TextAreaView {
    /// R764 §5.40 — ARIA `textbox` carrying the live multi-line text as
    /// [`AccessValue::Text`]. (The multi-line `aria-multiline` refinement
    /// is an additive a11y axis — deferred until a 2nd textarea consumer
    /// per [[abstraction-needs-second-consumer]].)
    fn access_node(state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret) = state;
        let text = use_text_edit_state(TA_TAG).text();
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, TextFieldState::Disabled),
            hovered: false,
            pressed: false,
            checked: None,
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::TextInput)
                .with_value(AccessValue::Text(text))
                .with_state(access_state),
        ]
    }
}

impl WidgetView for TextAreaView {
    type Renderer = HelloTextAreaRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }

    fn ime_caret_rect(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(TA_TAG) {
            return None;
        }
        let (interaction, caret_byte) = *state;
        let field_rect = pinion_shell::rect_for_tag(scene, TA_TAG)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        Some(tf_paint::ime_caret_rect_for(
            TA_TAG,
            interaction,
            caret_byte,
            field_rect,
            &theme,
            &ta_style(),
        ))
    }

    /// R762 / R763 reused verbatim against the multi-line layout: the
    /// hit-test (`byte_for_field_point`) picks the line by `y`, so a
    /// click / drag / Shift-click on any line resolves correctly.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        if focused != Some(TA_TAG) {
            return None;
        }
        let byte = hit_test_area_byte(*state, scene, x, y)?;
        let edit = use_text_edit_state(TA_TAG);
        if extend {
            let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
            edit.set_selection(anchor, byte);
            Some(anchor)
        } else {
            edit.set_caret(byte);
            Some(byte)
        }
    }

    fn select_drag_to_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        if focused != Some(TA_TAG) {
            return false;
        }
        let Some(byte) = hit_test_area_byte(*state, scene, x, y) else {
            return false;
        };
        let edit = use_text_edit_state(TA_TAG);
        let before = (edit.caret(), edit.selection_anchor());
        edit.set_selection(anchor, byte);
        before != (edit.caret(), edit.selection_anchor())
    }
}

/// R764 §5.36 §5.22 — shared pointer hit-test for the press + drag
/// hooks (mirror of hello-textfield's `hit_test_field_byte`): resolve a
/// window-local pixel point to a byte via the `byte_for_field_point`
/// SSOT against the *multi-line* `ta_style()` layout.
fn hit_test_area_byte(
    state: (TextFieldState, u32),
    scene: &Scene,
    x: f32,
    y: f32,
) -> Option<usize> {
    let (interaction, _caret_byte) = state;
    let field_rect = pinion_shell::rect_for_tag(scene, TA_TAG)?;
    let theme = use_theme(THEME_TAG).theme_animated();
    Some(tf_paint::byte_for_field_point(
        TA_TAG,
        interaction,
        x,
        y,
        field_rect,
        &theme,
        &ta_style(),
    ))
}

fn main() {
    pinion_shell::run::<TextAreaView>();
}

#[cfg(test)]
mod tests {
    use super::{view, TextAreaView, TA_TAG};
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene, WidgetCore};

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r764_view_carries_textarea_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag(TA_TAG), "paint scene carries the textarea tag");
        });
    }

    #[test]
    fn r764_enter_inserts_newline() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("ab".to_owned());
            edit.set_caret(1);
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            // create_external seeds when empty; we set non-empty above so
            // the buffer is "ab" and Enter splits it at the caret.
            assert_eq!(edit.text(), "a\nb", "Enter inserts a newline at the caret");
            assert_eq!(edit.caret(), 2, "caret advances past the inserted newline");
        });
    }

    #[test]
    fn r764_arrow_down_moves_to_next_line() {
        with_owner(|| {
            let edit = use_text_edit_state(TA_TAG);
            edit.set_text("abc\nxyz".to_owned());
            edit.set_caret(1); // after 'a' on line 0
            let mut scene =
                Scene::External(ExternalNode::new(TextAreaView::create_external()).with_tag(TA_TAG));
            // Paint once so the layout cache holds the shaped multi-line
            // Layout the vertical-move helper reads.
            let _ = view((TextFieldState::Focused, 1), &Frame::default());
            assert!(TextAreaView::apply_key(
                &mut scene,
                Some(TA_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            // Line 1 starts at byte 4 ("abc\n" = 4 bytes); column 1 → byte 5.
            assert_eq!(edit.caret(), 5, "ArrowDown lands on the next line at the same column");
        });
    }
}
