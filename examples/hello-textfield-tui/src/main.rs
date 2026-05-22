//! `hello-textfield-tui` — R56.1.b.1.tui §5.41 §5.38 TUI sibling of
//! the Vello `hello-textfield` (R56.1.b.1).
//!
//! ## What this binding proves
//!
//! [`§2 invariant #6`](https://pinion.dev/spec#invariants) — GUI / TUI
//! dual rendering from one canonical scene structure. The §5.38
//! `TextField` widget composes identically across both backends:
//! the SCXML statechart (R56.1.a) + reactive [`TextEditState`]
//! (R56.1.b) + W3C keystroke surface (R56.1.d) live in `pinion-core`,
//! shared verbatim between the Vello `hello-textfield` and this
//! `hello-textfield-tui`. Only the rendering shell + the visible
//! frame coordinates differ.
//!
//! ## What's different from the Vello sibling
//!
//! - **Caret blink**: the terminal native cursor blinks by itself
//!   (xterm `vt220` mode etc.), and the R56.1.c [`CaretBlink`] /
//!   R56.1.h `attach_blink` substrate adds animation infrastructure
//!   that the TUI shell does not yet consume. This binding omits the
//!   blink attach and paints a solid block-cursor at the byte offset
//!   instead. The text content + caret byte composition still flows
//!   through [`use_text_edit_state`] verbatim.
//! - **Caret geometry**: the cell-based paint walker maps pixel
//!   coords through `PIXEL_PER_CELL_X` (8 px) / `PIXEL_PER_CELL_Y`
//!   (16 px), so the caret column is `byte_offset` cells (1 ASCII byte
//!   per cell — matches every realistic TUI input). The parley
//!   [`caret_rect_for_byte_offset`] (§5.36) helper is not used here
//!   because TUI runs without a font shaping layer.
//! - **Focus**: the TUI shell passes `Some(V::tag())` unconditionally
//!   (single-focusable-binding model — [[substrate-incompleteness-
//!   signal]] defers the TUI [`FocusManager`] axis until a second
//!   focusable TUI binding lands). So the field acts as if it is
//!   permanently focused; `apply_key` still routes through
//!   [`TextFieldExternal::invoke`]`("key", ...)` for the canonical
//!   keystroke surface.
//!
//! ## Try it
//!
//! ```bash
//! cargo run -p hello-textfield-tui
//! ```
//!
//! Type ASCII characters → text appears, cursor advances.
//! `Backspace` / `Delete` / `ArrowLeft` / `ArrowRight` / `Home` /
//! `End` navigate. `d` disables, `e` re-enables. `Esc` quits the
//! alternate-screen session and restores the terminal.

use std::borrow::Cow;
use std::io::Stdout;

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{style, Color, Frame, WidgetCore};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// Widget tag — matches both [`WidgetCore::tag`] and the
/// [`use_text_edit_state`] cache key, so the External's
/// `attach_state` and the view fn's text read resolve to the same
/// reactive `Rc<TextEditState>` (typed `Owner::cache` lookup per
/// [[owner-cache-typed-key]]).
const TF_TAG: &str = "hello_textfield_tui";

// Pixel-space coordinates the paint walker maps to cells via the
// pinion-tui `PIXEL_PER_CELL_*` (8 / 16) constants. Field is 30
// cells wide × 1 row tall starting at column 2 / row 3 of the
// terminal — mirrors the macOS Settings input rhythm scaled to a
// terminal aesthetic.
const FIELD_LEFT_PX: u32 = 16; // col 2
const FIELD_TOP_PX: u32 = 48; // row 3
const FIELD_WIDTH_PX: u32 = 240; // 30 cells
const FIELD_HEIGHT_PX: u32 = 16; // 1 row
const CELL_PX_X: u32 = 8;
const CELL_PX_Y: u32 = 16;
const TEXT_INNER_PAD_PX: u32 = 8; // 1-cell left padding inside the field

// Status + hint lines below the field.
const STATUS_TOP_PX: u32 = 96; // row 6
const HINT_TOP_PX: u32 = 128; // row 8
const TEXT_ROW_HEIGHT_PX: u32 = 16;
const TEXT_BLOCK_WIDTH_PX: u32 = 640;

const ROOT_WIDTH_PX: u32 = 640;
const ROOT_HEIGHT_PX: u32 = 240;

const TITLE_TOP_PX: u32 = 16; // row 1

/// Block-cursor character — solid full-cell block. Pinned as a
/// `&'static str` const literal per [[non-ascii-literal-named-const-
/// escape]] (raw glyph in a doc comment is fine; literal forms in
/// source go through `\u{XXXX}` escape).
const CURSOR_GLYPH: &str = "\u{2588}";

struct HelloTextFieldTui;

impl WidgetCore for HelloTextFieldTui {
    /// State shape: `(TextFieldState, u32)` — identical to the
    /// Vello sibling. The substrate's `read_state` lifts both
    /// fields through the §5.15 introspect channel each frame; the
    /// text content itself lives on the reactive [`TextEditState`]
    /// the view fn reaches via [`use_text_edit_state`].
    type State = (TextFieldState, u32);

    type Event = TextFieldEvent;

    /// (R56.1.b.1 substrate) `create_external` runs inside
    /// `root_owner.run` so the [`use_text_edit_state`] hook resolves
    /// against the binding's root reactive scope. The External and
    /// the view fn share the same `Rc<TextEditState>` via the
    /// (`TypeId`, `TF_TAG`) typed cache slot.
    ///
    /// No `attach_blink` — TUI defers caret animation to the
    /// terminal's native cursor; see the module-level docs.
    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(TF_TAG);
        Box::new(TextFieldExternal::new().attach_state(text_state))
    }

    fn tag() -> &'static str {
        TF_TAG
    }

    /// Mirror of the Vello sibling's `read_state`. The TUI substrate
    /// composes its state-scene root identically (R55.D.5 single-
    /// External shape — no `create_extra_externals` override).
    fn read_state(scene: &Scene) -> Self::State {
        let Some(node) = scene.find_external_with_tag(TF_TAG) else {
            return (TextFieldState::Idle, 0);
        };
        let Some(intro) = node.handle.introspect() else {
            return (TextFieldState::Idle, 0);
        };
        let interaction = match intro.query("state") {
            Some(IntrospectValue::Text(name)) => parse_text_field_state(&name),
            _ => TextFieldState::Idle,
        };
        let caret = match intro.query("caret") {
            Some(IntrospectValue::Int(n)) => u32::try_from(n.max(0)).unwrap_or(u32::MAX),
            _ => 0,
        };
        (interaction, caret)
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        let (interaction, caret_byte) = state;
        let text_state = use_text_edit_state(TF_TAG);
        let text = text_state.text();

        // Field background — colour cue for state. Idle stays a dark
        // grey; Focused / Editing lifts to a slightly deeper navy;
        // Disabled is a muted brown-grey (same chromatic muting
        // convention the Vello sibling uses).
        let field_fill: Color = match interaction {
            TextFieldState::Idle => Color::rgb(0x20, 0x28, 0x30),
            TextFieldState::Focused | TextFieldState::Editing => {
                Color::rgb(0x14, 0x1c, 0x28)
            }
            TextFieldState::Disabled => Color::rgb(0x4a, 0x42, 0x38),
        };
        let border_color: Color = match interaction {
            TextFieldState::Disabled => Color::rgb(0x70, 0x70, 0x70),
            _ => Color::rgb(0xe0, 0xe0, 0xe0),
        };

        // Text node — natural-flow inside the field, padded one cell
        // from the left edge so the border does not clip the glyphs.
        let mut text_node = TextNode::default();
        text.clone_into(&mut text_node.content);
        text_node.rect = Rect::new(
            FIELD_LEFT_PX + TEXT_INNER_PAD_PX,
            FIELD_TOP_PX,
            FIELD_WIDTH_PX - TEXT_INNER_PAD_PX,
            CELL_PX_Y,
        );
        text_node.style = style::TextStyle::default();

        // Field container — bordered cell box that the paint walker
        // maps to a single-row box with `+`/`-`/`|` ASCII border per
        // its conventions.
        let mut field_container = ContainerNode::default();
        field_container.rect = Rect::new(
            FIELD_LEFT_PX,
            FIELD_TOP_PX,
            FIELD_WIDTH_PX,
            FIELD_HEIGHT_PX,
        );
        field_container.tag = Some(Cow::Borrowed(TF_TAG));
        field_container.style =
            BoxStyle::filled(field_fill).with_border(Border::new(border_color, 1));
        field_container.children.push(Scene::Text(text_node));

        // Caret — paint a solid block-cursor glyph at the byte
        // offset's cell column. Skipped while Disabled (no edit
        // affordance for a disabled field). 1 ASCII byte = 1 cell in
        // the TUI demo's input space; multi-byte UTF-8 would skew the
        // column by the cluster-count gap (carry to a future TUI
        // grapheme-cluster axis).
        if !matches!(interaction, TextFieldState::Disabled) {
            let mut cursor_node = TextNode::default();
            CURSOR_GLYPH.clone_into(&mut cursor_node.content);
            let cursor_left_px = FIELD_LEFT_PX
                .saturating_add(TEXT_INNER_PAD_PX)
                .saturating_add(caret_byte.saturating_mul(CELL_PX_X));
            cursor_node.rect = Rect::new(cursor_left_px, FIELD_TOP_PX, CELL_PX_X, CELL_PX_Y);
            cursor_node.style = style::TextStyle::default();
            field_container.children.push(Scene::Text(cursor_node));
        }

        // Title above the field — single-line caption mirroring the
        // Vello sibling's "TextField" header.
        let mut title = TextNode::default();
        "TextField (TUI)".clone_into(&mut title.content);
        title.rect = Rect::new(FIELD_LEFT_PX, TITLE_TOP_PX, TEXT_BLOCK_WIDTH_PX, TEXT_ROW_HEIGHT_PX);
        title.style = style::TextStyle::default();

        // Status line — text-only mirror so the AI side can verify
        // by reading the scene tree even when the TTY snapshot is
        // unavailable.
        let mut status = TextNode::default();
        let status_str = format!(
            "state: {} | caret: {} | text: \"{}\"",
            text_field_state_name(interaction),
            caret_byte,
            text,
        );
        status_str.clone_into(&mut status.content);
        status.rect = Rect::new(FIELD_LEFT_PX, STATUS_TOP_PX, TEXT_BLOCK_WIDTH_PX, TEXT_ROW_HEIGHT_PX);
        status.style = style::TextStyle::default();

        // Hint line — keyboard cheatsheet, single line so the
        // paint walker's row floor does not split it.
        let mut hint = TextNode::default();
        "Type/Backspace/Arrow/Home/End/Delete, d/e disable, Esc quit"
            .clone_into(&mut hint.content);
        hint.rect = Rect::new(FIELD_LEFT_PX, HINT_TOP_PX, TEXT_BLOCK_WIDTH_PX, TEXT_ROW_HEIGHT_PX);
        hint.style = style::TextStyle::default();

        let mut root = ContainerNode::default();
        root.rect = Rect::new(0, 0, ROOT_WIDTH_PX, ROOT_HEIGHT_PX);
        root.children.push(Scene::Text(title));
        root.children.push(Scene::Container(field_container));
        root.children.push(Scene::Text(status));
        root.children.push(Scene::Text(hint));

        Scene::Container(root)
    }

    fn event_name(event: Self::Event) -> &'static str {
        match event {
            TextFieldEvent::Focus => "Focus",
            TextFieldEvent::Blur => "Blur",
            TextFieldEvent::BeginEdit => "BeginEdit",
            TextFieldEvent::CommitEdit => "CommitEdit",
            TextFieldEvent::CancelEdit => "CancelEdit",
            TextFieldEvent::Disable => "Disable",
            TextFieldEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-textfield-tui (R56.1.b.1.tui §5.41 §5.38)"
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(TextFieldEvent::Disable),
            "e" => Some(TextFieldEvent::Enable),
            _ => None,
        }
    }

    /// R56.1.d §5.38 §5.22 — delegate W3C UI Events keystroke to
    /// [`TextFieldExternal::invoke`]`("key", Text(key))`. Identical
    /// to the Vello sibling, including the focus-tag short-circuit
    /// (the TUI shell passes `Some(V::tag())` unconditionally, so
    /// the gate is currently a no-op but stays in place for the
    /// future TUI [`FocusManager`] axis).
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(TF_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match intro.invoke("key", IntrospectValue::Text(key.to_owned())) {
            Ok(IntrospectValue::Bool(handled)) => handled,
            _ => false,
        }
    }

    fn fmt_state_log(state: &Self::State) -> String {
        format!(
            "{} / caret={}",
            text_field_state_name(state.0),
            state.1,
        )
    }
}

impl WidgetA11y for HelloTextFieldTui {
    /// R56.1.b.1 §5.40 — `AriaRole::TextInput` (lowers to
    /// `accesskit::Role::TextInput`) carrying the live text content
    /// as `AccessValue::Text`. Identical contract to the Vello
    /// sibling; the (R56.1.b.1) `root_owner.run` wrap around
    /// `V::access_node` in `collect_access_emit_inputs` lets this
    /// hook reach the same `Rc<TextEditState>` the view fn resolves.
    fn access_node(state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret) = state;
        let text = use_text_edit_state(TF_TAG).text();
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

impl WidgetViewTui for HelloTextFieldTui {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn parse_text_field_state(name: &str) -> TextFieldState {
    match name {
        "Focused" => TextFieldState::Focused,
        "Editing" => TextFieldState::Editing,
        "Disabled" => TextFieldState::Disabled,
        _ => TextFieldState::Idle,
    }
}

fn text_field_state_name(state: TextFieldState) -> &'static str {
    match state {
        TextFieldState::Idle => "Idle",
        TextFieldState::Focused => "Focused",
        TextFieldState::Editing => "Editing",
        TextFieldState::Disabled => "Disabled",
    }
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloTextFieldTui>() {
        eprintln!("hello-textfield-tui: shell error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.b.1.tui §5.41 §5.38 — TTY-free snapshot regression
    //! battery. Uses [`pinion_tui::render_one_frame`] to materialise
    //! the view fn's paint into a ratatui [`Buffer`] without a real
    //! terminal, then asserts on per-cell glyphs.

    use super::{
        parse_text_field_state, text_field_state_name, HelloTextFieldTui, CURSOR_GLYPH,
        TF_TAG,
    };
    use pinion_a11y::{AccessValue, AriaRole, WidgetA11y};
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::WidgetCore;

    /// Reactive scope wrapper — `render_one_frame` does not wrap
    /// `V::view` in `root_owner.run` (the runtime shell does), so
    /// tests provide their own scope so the `use_text_edit_state`
    /// hook resolves.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    // ─────────────────────────────────────────────────────────────
    // Name <-> state round-trip
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_1_tui_state_name_round_trips() {
        for s in [
            TextFieldState::Idle,
            TextFieldState::Focused,
            TextFieldState::Editing,
            TextFieldState::Disabled,
        ] {
            assert_eq!(parse_text_field_state(text_field_state_name(s)), s);
        }
    }

    // ─────────────────────────────────────────────────────────────
    // ARIA — role + value + state
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_1_tui_access_node_role_is_text_input() {
        with_owner(|| {
            let nodes =
                HelloTextFieldTui::access_node(&(TextFieldState::Idle, 0), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[0].tag, TF_TAG);
        });
    }

    #[test]
    fn r56_1_b_1_tui_access_node_value_carries_live_text() {
        with_owner(|| {
            let state = use_text_edit_state(TF_TAG);
            state.set_text("TUI typed".to_owned());
            let nodes = HelloTextFieldTui::access_node(
                &(TextFieldState::Focused, 0),
                None,
            );
            assert_eq!(
                nodes[0].value,
                Some(AccessValue::Text("TUI typed".to_owned())),
            );
        });
    }

    #[test]
    fn r56_1_b_1_tui_access_node_disabled_flag_set_when_disabled() {
        with_owner(|| {
            let nodes = HelloTextFieldTui::access_node(
                &(TextFieldState::Disabled, 0),
                None,
            );
            assert!(nodes[0].state.disabled);
        });
    }

    // ─────────────────────────────────────────────────────────────
    // View — paint scene shape via render_one_frame snapshot
    // ─────────────────────────────────────────────────────────────

    /// Helper — convert the rendered buffer's row N into a String for
    /// easier assertion. Trims trailing whitespace so absolute width
    /// differences don't break the contains-substring checks.
    fn row_text(buf: &pinion_tui::ratatui::buffer::Buffer, row: u16) -> String {
        let area = buf.area;
        let mut s = String::with_capacity(area.width as usize);
        for col in 0..area.width {
            s.push_str(buf[(col, row)].symbol());
        }
        s.trim_end().to_owned()
    }

    #[test]
    fn r56_1_b_1_tui_view_carries_field_tag() {
        with_owner(|| {
            use pinion_core::Frame;
            let scene = HelloTextFieldTui::view(
                (TextFieldState::Focused, 0),
                &Frame::default(),
            );
            assert!(scene.contains_tag(TF_TAG));
        });
    }

    #[test]
    fn r56_1_b_1_tui_render_snapshot_includes_title_and_status() {
        with_owner(|| {
            let state = use_text_edit_state(TF_TAG);
            state.set_text("abc".to_owned());
            let buf = pinion_tui::render_one_frame::<HelloTextFieldTui>(
                (TextFieldState::Focused, 3),
                80,
                12,
            );
            // Title row 1 — "TextField (TUI)" at col 2 onwards.
            let title_row = row_text(&buf, 1);
            assert!(
                title_row.contains("TextField"),
                "row 1 must contain 'TextField' title (got {title_row:?})",
            );
            // Status row 6 — "state: Focused | caret: 3 | text: \"abc\"".
            let status_row = row_text(&buf, 6);
            assert!(
                status_row.contains("Focused"),
                "status row must contain Focused (got {status_row:?})",
            );
            assert!(
                status_row.contains("caret: 3"),
                "status row must contain caret position (got {status_row:?})",
            );
            assert!(
                status_row.contains("abc"),
                "status row must contain the live text (got {status_row:?})",
            );
        });
    }

    #[test]
    fn r56_1_b_1_tui_render_snapshot_paints_cursor_glyph_when_focused() {
        with_owner(|| {
            let state = use_text_edit_state(TF_TAG);
            state.set_text("hi".to_owned());
            let buf = pinion_tui::render_one_frame::<HelloTextFieldTui>(
                (TextFieldState::Focused, 2),
                80,
                12,
            );
            // Field row 3 — text starts at col 3 (left pad of 1 cell
            // inside col 2 border). Glyphs: col 3 = 'h', col 4 = 'i',
            // col 5 = cursor glyph.
            let field_row = row_text(&buf, 3);
            assert!(
                field_row.contains(CURSOR_GLYPH),
                "field row must paint the cursor glyph when focused (got {field_row:?})",
            );
            assert!(
                field_row.contains("hi"),
                "field row must paint the text content (got {field_row:?})",
            );
        });
    }

    #[test]
    fn r56_1_b_1_tui_render_snapshot_omits_cursor_when_disabled() {
        with_owner(|| {
            let state = use_text_edit_state(TF_TAG);
            state.set_text("locked".to_owned());
            let buf = pinion_tui::render_one_frame::<HelloTextFieldTui>(
                (TextFieldState::Disabled, 6),
                80,
                12,
            );
            let field_row = row_text(&buf, 3);
            assert!(
                !field_row.contains(CURSOR_GLYPH),
                "disabled field must NOT paint a cursor (got {field_row:?})",
            );
            assert!(
                field_row.contains("locked"),
                "disabled field still paints the text (got {field_row:?})",
            );
        });
    }
}
