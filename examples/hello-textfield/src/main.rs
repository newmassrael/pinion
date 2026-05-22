//! `hello-textfield` — R56.1.b.1 §5.38 first visible consumer for the
//! `TextField` widget catalogue entry.
//!
//! ## Substrate verification
//!
//! [[substrate-incompleteness-signal]]. The R56 axis substrate (6
//! primitives across R56.1.a/b/b.2/c/d/h) closed before this binding
//! landed; `hello-textfield` is the first application that composes
//! them end-to-end. Boilerplate audit (textbook target: ≤ 5 LOC for
//! widget composition):
//!
//! 1. `TextFieldExternal::new()` (R56.1.a)
//! 2. `.attach_state(use_text_edit_state(TF_TAG))` (R56.1.b)
//! 3. `.attach_blink(use_caret_blink(TF_TAG))` (R56.1.h)
//!
//! Three builder calls. The view fn's caret rect derivation (one
//! `LayoutCache::layout` call + one [`caret_rect_for_byte_offset`]
//! call) is paint code, not composition — the substrate carries the
//! whole interaction-state machine through the typed `TextFieldEvent`
//! surface and the W3C `KeyboardEvent.key` mapping inside
//! [`TextFieldExternal::invoke`]`("key", Text)`. The two R56.1.b.1
//! substrate fixes that landed alongside this binding
//! (`root_owner.run` wraps around `V::create_external` /
//! `V::access_node` in `core_shell.rs` / `substrate.rs`) close the
//! [[callback-root-owner-wrap]] family for the two lifecycle hooks
//! that had been missed pre-R56.1.b.1.
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state +
//!   caret byte offset. Text content lives on the reactive
//!   [`TextEditState`] reached via `use_text_edit_state(TF_TAG)`
//!   ([`Owner::cache`]-keyed hook, shared between
//!   [`TextFieldExternal::attach_state`] in `create_external` and
//!   the view fn's text read — same cache key → same `Rc`).
//! - Visible value: 360×40 input box, white text on dark grey
//!   background, blinking caret on focus, ARIA `textbox` role.
//! - Input wire: [`apply_key`](WidgetCore::apply_key) delegates to
//!   [`TextFieldExternal::invoke`]`("key", Text)` (R56.1.d). The
//!   shell's `notify_focus_change` (R56.1.h) drives `Focus` / `Blur`
//!   events to the External, which then gates the blink animation
//!   via [`TextField::sync_blink`].
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p hello-textfield
//! ```
//!
//! Tab into the input → caret appears + blinks. Type characters →
//! text appears at caret; caret advances. `Backspace` / `Delete` /
//! `ArrowLeft` / `ArrowRight` / `Home` / `End` navigate. `Shift+Tab`
//! to blur → caret disappears, the SCXML transitions
//! `Focused → Idle`. Press `d` to disable, `e` to re-enable.

use std::cell::RefCell;
use std::rc::Rc;

use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::{caret_rect_for_byte_offset, LayoutCache};

// pinion-forge codegen output. Defines `pub struct HelloTextFieldRenderer`
// + async `new<W: Into<wgpu::SurfaceTarget<'static>>>` + sync
// `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>` can
// construct + render + resize it.
vello_renderer_impl!(HelloTextFieldRenderer, HelloTextFieldRendererError);

/// Tag for the textfield widget. Matches the `WidgetCore::tag`
/// (paint-root + input-router hit-test target) and the
/// [`use_text_edit_state`] / [`use_caret_blink`] cache keys, so the
/// `create_external` factory and the view fn both resolve to the same
/// reactive `Rc<TextEditState>` + `Rc<CaretBlink>` instances.
const TF_TAG: &str = "main_textfield";

const WIN_W: u32 = 480;
const WIN_H: u32 = 200;

// Window background — same dark navy hello-toggle uses, for visual
// consistency across the example gallery.
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);

// Field surface — 360×40 with 8 px padding, 4 px corner radius. Fill
// shifts on focus to give the user a clear "the input is live" cue
// without an explicit border-colour change (CSS `:focus-visible`
// convention scaled down to a flat-fill palette).
const FIELD_W: u32 = 360;
const FIELD_H: u32 = 40;
const FIELD_PAD: u32 = 8;
const FIELD_CORNER: u32 = 4;
const FIELD_FILL_IDLE: Color = Color::rgb(0x18, 0x20, 0x28);
const FIELD_FILL_FOCUSED: Color = Color::rgb(0x14, 0x1c, 0x24);
const FIELD_FILL_DISABLED: Color = Color::rgb(0x30, 0x30, 0x30);

const TEXT_COLOR: Color = Color::rgb(0xff, 0xff, 0xff);
const TEXT_COLOR_DISABLED: Color = Color::rgb(0x70, 0x70, 0x70);

const CARET_COLOR: Color = Color::rgb(0xff, 0xff, 0xff);
// 2 px caret reads cleanly on the integer-scaled 1.0× displays the
// hello-* gallery is sized for; Hi-DPI displays where AA softens
// single-pixel lines could drop to 1 px (the substrate
// `caret_rect_for_byte_offset` accepts the width as f32, the binding
// can pick per-DPI).
const CARET_WIDTH: u32 = 2;

const FONT_SIZE_PX: u32 = 18;

// Gap between title / field / status line in the root column flex —
// matches the macOS / iOS settings-pane vertical rhythm (~16 px
// between related controls).
const ROW_GAP: u32 = 16;

/// `Owner::cache`-keyed parley [`LayoutCache`] hook. Mirrors the
/// [`use_text_edit_state`] / [`use_caret_blink`] convention — the
/// view fn calls this each paint, the cache returns the same
/// `Rc<RefCell<LayoutCache>>` every time (Owner cache key dedup), and
/// the `RefCell` admits the `&mut self` parley `Layout` build /
/// lookup that `LayoutCache::layout` requires.
///
/// The cache key (`"hello_textfield.layout_cache"`) is binding-private
/// — no other view fn shares this `LayoutCache` instance, so a future
/// hello-textarea binding gets its own cache by passing a different
/// key. Per-binding caches are the canonical scope on this slice; a
/// framework-wide shared layout cache substrate is a separate axis
/// (the [[substrate-incompleteness-signal]] for a multi-textfield
/// binding hasn't fired yet).
fn use_layout_cache(key: &'static str) -> Rc<RefCell<LayoutCache>> {
    Owner::current()
        .expect("use_layout_cache requires an active Owner scope")
        .cache(key, || RefCell::new(LayoutCache::new()))
}

/// Saturating cast from layout-space f32 to paint-space u32. Negative
/// values clamp to 0; out-of-range positives clamp to `u32::MAX`.
/// `NaN` / `Infinity` clamp to 0 (defensive — parley's
/// [`caret_rect_for_byte_offset`] is `finite`-guaranteed by the
/// R56.1.b.2 test battery, but the saturating-cast convention stays
/// the textbook narrowing seam per [[r56-1-b-2-parley-f32-narrowing]]).
fn saturating_f32_to_u32(v: f32) -> u32 {
    // `u32::MAX as f32` rounds up to 4.294967296e9 (next representable
    // f32) — values >= that round-trip out of range, so the comparison
    // is well-defined as the saturating ceiling check despite the
    // f32 precision loss on the upper-bound constant itself.
    #[allow(
        clippy::cast_precision_loss,
        reason = "u32::MAX -> f32 rounds to a single saturating ceiling"
    )]
    let ceiling = u32::MAX as f32;
    if !v.is_finite() || v < 0.0 {
        0
    } else if v >= ceiling {
        u32::MAX
    } else {
        // `as` cast is bounded by the two guards above — any in-range
        // finite positive f32 truncates losslessly to u32 for the
        // paint-space dimensions this binding operates in (<= window
        // size in logical pixels).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded by is_finite / >=0 / < ceiling above"
        )]
        let out = v as u32;
        out
    }
}

/// view-fn (§6.3): pure-ish sync mapping `(state, frame) -> Scene`.
/// "Pure-ish" because the reactive [`Signal`](pinion_core::reactive::Signal)
/// reads inside [`use_text_edit_state`] / [`use_caret_blink`] subscribe
/// to the corresponding stores — the same `(state, frame)` always
/// yields the same `Scene` *for the same reactive store state*, which
/// is the canonical view-fn purity contract the rest of the example
/// gallery (`hello-listbox` `use_scroll_state`, `hello-radio-group`,
/// etc.) uses too.
///
/// Layout (top-to-bottom, centered):
/// 1. `"TextField"` title label (18 px white).
/// 2. The input field: 360×40, `tag = "main_textfield"` for the input
///    router. Text content flows naturally; a 2 px caret overlay paints
///    at the cursor byte position via [`LayoutStyle::with_absolute_position`]
///    (R55.D.6 substrate) when the field is `Focused` / `Editing` AND
///    the [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink)
///    phase is visible.
/// 3. Status line (`"<State> | caret=<n> | text=\"...\""`, 12 px
///    grey) — text-only state mirror so the AI side can verify the
///    same data the visible field renders via `scene/query`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::too_many_lines,
    reason = "view-fn shape mirrors hello-toggle / hello-listbox — one paint cycle, sequential composition"
)]
fn view(state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let (interaction, caret_byte) = state;

    let text_state = use_text_edit_state(TF_TAG);
    let blink = use_caret_blink(TF_TAG);
    let text = text_state.text();

    let text_style = TextStyle::new()
        .with_size_px(FONT_SIZE_PX)
        .with_fg(if matches!(interaction, TextFieldState::Disabled) {
            TEXT_COLOR_DISABLED
        } else {
            TEXT_COLOR
        });

    // Caret geometry — shape the current text once via the shared
    // `LayoutCache`, then look up the cursor rect at `caret_byte`.
    // The `LayoutCache::layout` LRU returns the same `Layout`
    // reference for the same `(text, style, max_width)` tuple, so
    // re-runs of the view fn inside the same paint cycle reuse the
    // shaped run instead of re-shaping per call.
    let layout_cache = use_layout_cache("hello_textfield.layout_cache");
    let caret_pixel_rect = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(text.as_str(), &text_style, None);
        #[allow(
            clippy::cast_precision_loss,
            reason = "CARET_WIDTH fits f32 losslessly (2 << 23 ceiling)"
        )]
        let rect = caret_rect_for_byte_offset(
            layout,
            caret_byte as usize,
            CARET_WIDTH as f32,
        );
        // Floor the caret height at the font size so an empty layout
        // (parley reports the font-derived line box, which tracks the
        // font size) never paints a 0-height caret.
        let height_floor = saturating_f32_to_u32(rect.height).max(FONT_SIZE_PX);
        (
            saturating_f32_to_u32(rect.x),
            saturating_f32_to_u32(rect.y),
            height_floor,
        )
    };
    let (caret_layout_x, caret_layout_y, caret_box_height) = caret_pixel_rect;

    let field_fill = match interaction {
        TextFieldState::Idle => FIELD_FILL_IDLE,
        TextFieldState::Focused | TextFieldState::Editing => FIELD_FILL_FOCUSED,
        TextFieldState::Disabled => FIELD_FILL_DISABLED,
    };

    // Text node — natural-flow child of the field container. Empty
    // text is rendered as a zero-width run so the caret still appears
    // at x=0 inside the padded field.
    let text_node = Scene::Text(TextNode::styled(
        text.clone(),
        Rect::default(),
        text_style,
    ));

    // Caret — only painted when the widget is focused (Focused or
    // Editing) AND the blink phase is currently visible. R56.1.h sync
    // ties the blink's enabled gate to the SCXML state, so the blink
    // is always paused (and `visible()` returns `false`) outside the
    // focused/editing posture. Reading `blink.visible()` subscribes
    // to the underlying Signal — the next phase flip auto-triggers a
    // view re-run via the substrate's reactive paint loop.
    let caret_painted = matches!(
        interaction,
        TextFieldState::Focused | TextFieldState::Editing,
    ) && blink.visible();

    let mut field_children: Vec<Scene> = Vec::with_capacity(2);
    field_children.push(text_node);
    if caret_painted {
        let caret_left = FIELD_PAD.saturating_add(caret_layout_x);
        let caret_top = FIELD_PAD.saturating_add(caret_layout_y);
        let caret_box = Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(CARET_COLOR)).with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(CARET_WIDTH, caret_box_height))
                    .with_absolute_position(caret_left, caret_top),
            ),
        );
        field_children.push(caret_box);
    }

    let field = Scene::Container(
        ContainerNode::new(field_children)
            .with_tag(TF_TAG)
            // R51.69 §5.40 — explicit accessible-name (WAI-ARIA
            // `aria-label`). Pinned at the field container so the
            // scene-walk name derivation in
            // [`enrich_names_from_scene`] populates the AccessNode's
            // `name` without a duplicate literal in `access_node`.
            .with_aria_label("Text input")
            .with_style(BoxStyle::filled(field_fill).with_corner_radius(FIELD_CORNER))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(FIELD_W, FIELD_H))
                    .with_padding(Rect::new(FIELD_PAD, FIELD_PAD, FIELD_PAD, FIELD_PAD)),
            ),
    );

    let title = Scene::Text(TextNode::styled(
        "TextField",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(Color::rgb(0xe0, 0xe0, 0xe0)),
    ));

    let status_str = format!(
        "{} | caret={} | text=\"{}\"",
        text_field_state_name(interaction),
        caret_byte,
        text,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(Color::rgb(0x90, 0x90, 0x90)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, field, status])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding for the [`TextField`] widget.
///
/// State shape: `(TextFieldState, u32)` — the SCXML interaction state
/// plus the caret byte offset. The text content itself is reactive
/// (`Rc<TextEditState>` via `use_text_edit_state`), so it does not
/// (and cannot — `String` is not `Copy`) live in `Self::State`. The
/// view fn reads text via the same Owner-cache hook the External's
/// `attach_state` resolves through, so both sides see the same store.
struct TextFieldView;

impl WidgetCore for TextFieldView {
    type State = (TextFieldState, u32);
    type Event = TextFieldEvent;

    /// (R56.1.b.1 substrate) `create_external` now runs inside
    /// `root_owner.run(...)`, so the `use_text_edit_state` /
    /// `use_caret_blink` hooks resolve against the same Owner the
    /// view fn will reach later — the External's attached `Rc` and
    /// the view fn's `Rc` are identical instances. Three builder
    /// calls is the substrate-incompleteness-signal boilerplate
    /// budget; staying under the budget signals the substrate
    /// composes cleanly without per-binding scaffolding.
    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(TF_TAG);
        let blink = use_caret_blink(TF_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink),
        )
    }

    fn tag() -> &'static str {
        TF_TAG
    }

    /// (R55.D.5 §5.45) Single-External binding — the state scene root
    /// stays `Scene::External(primary)`. `find_external_with_tag`
    /// handles both the single-External and the multi-External shapes
    /// (R55.D.5 cascade lesson), so the read site is shape-agnostic
    /// even though this binding doesn't use `create_extra_externals`.
    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
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
            // i64 → u32 — caret is bounded by text length, which is
            // u32-bounded by every realistic UI text input. Negative
            // values are unreachable (TextEditState clamps at the
            // intervene seam); `try_from` defends without a panic.
            Some(IntrospectValue::Int(n)) => u32::try_from(n.max(0)).unwrap_or(u32::MAX),
            _ => 0,
        };
        (interaction, caret)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        match event {
            TextFieldEvent::Focus => "Focus",
            TextFieldEvent::Blur => "Blur",
            TextFieldEvent::BeginEdit => "BeginEdit",
            TextFieldEvent::CommitEdit => "CommitEdit",
            TextFieldEvent::CancelEdit => "CancelEdit",
            TextFieldEvent::Disable => "Disable",
            TextFieldEvent::Enable => "Enable",
            // SCXML-internal variants (parley-emitted state ping
            // events that the public surface never accepts) — route
            // through a sentinel the parser rejects.
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-textfield (R56.1.b.1 §5.38)"
    }

    /// Two debugging shortcuts at the binary level: `d` disables the
    /// field, `e` re-enables it. The text-content keys (single
    /// printable chars + named edit keys) flow through `apply_key`
    /// because the framework reserves the `keybinding` channel for
    /// strongly-typed enum events.
    fn keybinding(key: &str) -> Option<TextFieldEvent> {
        match key {
            "d" => Some(TextFieldEvent::Disable),
            "e" => Some(TextFieldEvent::Enable),
            _ => None,
        }
    }

    /// R56.1.d §5.38 §5.22 — delegate W3C UI Events keystroke to
    /// [`TextFieldExternal::invoke`]`("key", Text(key))`. Returns
    /// `true` when the External reports the key as recognized
    /// (matches the W3C `defaultPrevented` semantic — the framework
    /// then swallows the key from the focus / shortcut chain).
    ///
    /// The `focused != Some(TF_TAG)` short-circuit mirrors the
    /// roving-tabindex pattern from `hello-radio-group` /
    /// `hello-listbox`: keys only flow when this widget owns focus,
    /// avoiding the broadcast-to-every-widget aliasing that
    /// pre-R51.x `apply_key` suffered.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
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
            // `Bool(false)` for unrecognized keys lands here; any
            // other shape (TypeMismatch / UnknownPath) is a substrate
            // bug — return false to defer to the shell's fallback
            // chain so a misconfiguration does not silently consume
            // the key.
            _ => false,
        }
    }

    fn fmt_state_log(state: &(TextFieldState, u32)) -> String {
        format!(
            "{} / caret={}",
            text_field_state_name(state.0),
            state.1,
        )
    }
}

impl WidgetA11y for TextFieldView {
    /// R56.1.b.1 §5.40 — ARIA `textbox` role node carrying the live
    /// text content as [`AccessValue::Text`]. The
    /// (R56.1.b.1 substrate) `root_owner.run` wrap around
    /// `V::access_node` in `collect_access_emit_inputs` lets this hook
    /// reach the same `Rc<TextEditState>` the view fn resolves through
    /// [`use_text_edit_state`].
    ///
    /// The `name` field is populated by
    /// [`enrich_names_from_scene`](pinion_a11y::enrich_names_from_scene)
    /// against the field container's `aria_label` override (set in
    /// `view`) — the literal `"Text input"` lives in exactly one place.
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

impl WidgetView for TextFieldView {
    type Renderer = HelloTextFieldRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

/// Inverse of the SCXML-emitted state name surface
/// (`text_field_state_name`). Defensive default (`Idle`) on any
/// unexpected token guards against a future SCXML rename leaking a
/// silent crash.
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
    pinion_shell::run::<TextFieldView>();
}

#[cfg(test)]
mod tests {
    //! R56.1.b.1 §5.38 — substrate-composition regression battery.
    //! Pinned at the binding level so a substrate rename / contract
    //! drift surfaces here before reaching the visible demo path.

    use super::{
        parse_text_field_state, text_field_state_name, view, TextFieldView, TF_TAG,
    };
    use pinion_a11y::{AccessValue, AriaRole, WidgetA11y};
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::caret_blink::use_caret_blink;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene};

    /// Run `f` inside a fresh `Owner` scope so reactive hooks
    /// resolve. Mirrors the framework's
    /// `root_owner.run(|| V::view(...))` wrap; tests use a private
    /// scope so each test starts with empty Owner cache state.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    // ─────────────────────────────────────────────────────────────
    // Name <-> state round-trip
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_1_state_name_round_trips() {
        for s in [
            TextFieldState::Idle,
            TextFieldState::Focused,
            TextFieldState::Editing,
            TextFieldState::Disabled,
        ] {
            assert_eq!(
                parse_text_field_state(text_field_state_name(s)),
                s,
                "round-trip must preserve {s:?}",
            );
        }
    }

    #[test]
    fn r56_1_b_1_unknown_state_name_defaults_to_idle() {
        assert_eq!(parse_text_field_state("wat"), TextFieldState::Idle);
        assert_eq!(parse_text_field_state(""), TextFieldState::Idle);
    }

    // ─────────────────────────────────────────────────────────────
    // View — caret rendering gate
    // ─────────────────────────────────────────────────────────────

    /// Walk the scene tree and count the immediate children of the
    /// container whose tag matches `tag`. Used to confirm the caret
    /// [`Scene::Box`] overlay is (or isn't) present.
    fn count_field_children(scene: &Scene, tag: &str) -> usize {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return c.children.len();
                }
                for child in &c.children {
                    let n = count_field_children(child, tag);
                    if n > 0 {
                        return n;
                    }
                }
                0
            }
            Scene::Scroll(s) => count_field_children(&s.content, tag),
            _ => 0,
        }
    }

    #[test]
    fn r56_1_b_1_view_carries_field_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(TF_TAG),
                "paint scene must carry the WidgetCore::tag (R55.G.17)",
            );
        });
    }

    #[test]
    fn r56_1_b_1_idle_state_has_no_caret_box() {
        with_owner(|| {
            // Idle => blink.visible() is false anyway, but the
            // caret_painted gate also short-circuits on the state
            // match. Either gate alone suffices; we test the
            // observable outcome.
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            // Field has exactly the text child (the caret overlay
            // would be a second child if painted).
            assert_eq!(
                count_field_children(&scene, TF_TAG),
                1,
                "Idle field paints text only — no caret overlay",
            );
        });
    }

    #[test]
    fn r56_1_b_1_focused_with_visible_blink_paints_caret() {
        with_owner(|| {
            // Focus the blink (so `visible()` returns true on the
            // post-enable initial phase per `set_enabled` contract).
            let blink = use_caret_blink(TF_TAG);
            blink.set_enabled(true);
            assert!(blink.visible(), "set_enabled(true) reveals the caret");

            let scene = view((TextFieldState::Focused, 0), &Frame::default());
            assert_eq!(
                count_field_children(&scene, TF_TAG),
                2,
                "Focused + blink visible paints text + caret overlay",
            );
        });
    }

    #[test]
    fn r56_1_b_1_focused_with_hidden_blink_omits_caret() {
        with_owner(|| {
            // Blink enabled but driven to the hidden phase — caret
            // skipped. (The blink starts hidden when fresh and stays
            // hidden until set_enabled fires.)
            let _blink = use_caret_blink(TF_TAG);
            // Note: do NOT call set_enabled — keep `visible == false`.
            let scene = view((TextFieldState::Focused, 0), &Frame::default());
            assert_eq!(
                count_field_children(&scene, TF_TAG),
                1,
                "Focused but blink hidden — no caret overlay",
            );
        });
    }

    #[test]
    fn r56_1_b_1_caret_position_tracks_text_state_caret_byte() {
        with_owner(|| {
            let text_state = use_text_edit_state(TF_TAG);
            text_state.set_text("hello".to_owned());

            let blink = use_caret_blink(TF_TAG);
            blink.set_enabled(true);

            // Caret at byte 0 vs byte 5 (end). The two paint scenes
            // should differ — the absolute_position of the caret box
            // shifts. We compare via debug rendering rather than
            // walking the scene by hand (the structural shape is the
            // same; the position is the diff).
            let scene_start = view((TextFieldState::Focused, 0), &Frame::default());
            let scene_end = view((TextFieldState::Focused, 5), &Frame::default());
            assert_ne!(
                format!("{scene_start:?}"),
                format!("{scene_end:?}"),
                "caret rect must differ at byte 0 vs byte 5",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // ARIA — role + value + state
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_1_access_node_role_is_text_input() {
        with_owner(|| {
            let nodes = TextFieldView::access_node(&(TextFieldState::Idle, 0), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[0].tag, TF_TAG);
        });
    }

    #[test]
    fn r56_1_b_1_access_node_value_carries_live_text() {
        with_owner(|| {
            let state = use_text_edit_state(TF_TAG);
            state.set_text("typed text".to_owned());

            let nodes = TextFieldView::access_node(&(TextFieldState::Focused, 0), None);
            assert_eq!(
                nodes[0].value,
                Some(AccessValue::Text("typed text".to_owned())),
                "AT-side value mirrors live TextEditState content",
            );
        });
    }

    #[test]
    fn r56_1_b_1_access_node_focused_flag_mirrors_focus() {
        with_owner(|| {
            let unfocused = TextFieldView::access_node(
                &(TextFieldState::Idle, 0),
                None,
            );
            assert!(!unfocused[0].state.focused);

            let focused = TextFieldView::access_node(
                &(TextFieldState::Focused, 0),
                Some(TF_TAG),
            );
            assert!(focused[0].state.focused);
        });
    }

    #[test]
    fn r56_1_b_1_access_node_disabled_flag_set_when_disabled() {
        with_owner(|| {
            let nodes = TextFieldView::access_node(
                &(TextFieldState::Disabled, 0),
                None,
            );
            assert!(nodes[0].state.disabled);
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R55.G.20 — composite paint root tag convention
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TextFieldView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }
}
