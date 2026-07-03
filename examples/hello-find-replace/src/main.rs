//! `hello-find-replace` — R903 §5.22 §5.52 find &amp; replace consumer for the
//! `TextField` widget.
//!
//! ## What it demonstrates
//!
//! Find &amp; replace is the depth slice that turns a text field into a code /
//! script editor. It rides entirely on the existing R56 text substrate
//! (`TextEditState` buffer + W3C selection + granular undo) plus the R903
//! additions:
//!
//! - **Find session on the buffer** — the needle + case / whole-word flags
//!   live on `TextEditState` itself, so the editor is *self-describing* to AI
//!   introspection: `scene/<tag>/external/find_matches` reports the editor's
//!   own count / ranges / current index without an agent reconstructing them
//!   from a find bar. The "current match" is implicit in the selection (the
//!   browser / VS Code model) — `find-next` searches forward from the
//!   selection and lands the selection on the hit, so repeated calls walk every
//!   match and wrap.
//! - **All-match highlight bands** — [`tf_paint::view_field`] paints a faint
//!   band behind every match (the same `find_matches` the status reads), with
//!   the current match's stronger selection band layered on top.
//! - **Replace All as one undo step** — the first consumer of the undo
//!   substrate's reserved macro-transaction axis
//!   ([`UndoStack::begin_macro`](pinion_core::undo::UndoStack::begin_macro) /
//!   [`end_macro`](pinion_core::undo::UndoStack::end_macro)): N splices fold
//!   into one Ctrl+Z.
//!
//! The find surface is driven AI-first over the [`TextFieldExternal`] RPC
//! channel (`find_query` / `find_case_sensitive` / `find_whole_word` intervene;
//! `find-next` / `find-prev` / `replace` / `replace-all` invoke) — §2 #2's
//! "RPC headless is the primary path" made concrete. The window shows the
//! editor, the live highlight bands, and a "{n} of {N}" status that all update
//! as the find state changes.
//!
//! The seeded document `"Red bored credit red end"` is chosen so the single
//! needle `"red"` exercises every toggle: substring → 4 matches, case-sensitive
//! → 3 (drops `Red`), whole-word → 2 (drops the `red` inside `bored` /
//! `credit`), case-sensitive AND whole-word → 1.
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state + caret byte
//!   offset. Text content + the find session live on the reactive
//!   `TextEditState` reached via `use_text_edit_state(TF_TAG)`.
//! - Plain text editing (type, Backspace, Arrow, Ctrl+Z) reuses the R56
//!   substrate unchanged; only the find surface is new.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p hello-find-replace
//! python3 tools/demos/r903_find_replace.py
//! ```

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::undo::use_undo_stack;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_text::CaretRect;
// R657 §5.16 §5.38 — lifted TextField paint substrate (view body +
// IME caret rect + state name lookup + SCXML read helper).
use pinion_widget_paint::text_field as tf_paint;

// pinion-forge codegen output. Defines `pub struct HelloFindReplaceRenderer`
// + async `new<W: Into<wgpu::SurfaceTarget<'static>>>` + sync
// `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>` can
// construct + render + resize it.
vello_renderer_impl!(HelloFindReplaceRenderer, HelloFindReplaceRendererError);

/// Tag for the textfield widget. Matches the `WidgetCore::tag`
/// (paint-root + input-router hit-test target) and the
/// [`use_text_edit_state`] / [`use_caret_blink`] cache keys, so the
/// `create_external` factory and the view fn both resolve to the same
/// reactive `Rc<TextEditState>` + `Rc<CaretBlink>` instances.
const TF_TAG: &str = "find_editor";

/// R903 §5.22 — the seeded document. Chosen so one needle ("red") exercises
/// every find toggle: substring matches 4 ("Red", the "red" inside "bo**red**"
/// and "c**red**it", and the standalone "red"); case-sensitive drops "Red" → 3;
/// whole-word keeps only the two standalone words → 2; case-sensitive AND
/// whole-word → 1 (just the lowercase standalone "red").
const SEED_TEXT: &str = "Red bored credit red end";

/// R903 §5.22 — tag for the find-status container so the demo can read the
/// "{n} of {N}" status as scene-as-data (`find_by_tag`).
const FIND_STATUS_TAG: &str = "find_status";

const WIN_W: u32 = 480;
const WIN_H: u32 = 200;

/// (R57.X.textfield §5.50) [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` so the example gallery shares one provider when a
/// host binds them together.
const THEME_TAG: &str = "app";

// R657 §5.16 §5.38 — Field surface sizing (360×40 with 8 px padding,
// 4 px corner radius), font + caret width, selection / preedit alpha
// tuning all live in [`tf_paint::TextFieldStyle::m3_filled`] in the
// pinion-widget-paint crate. The binding uses the M3 default; a
// custom-styled variant would pass a different `TextFieldStyle` to
// [`tf_paint::view_field`].

// Title label font size — kept binding-local because it's part of
// the surrounding chrome composition (not the field substrate).
const TITLE_FONT_SIZE_PX: u32 = 18;
const STATUS_FONT_SIZE_PX: u32 = 12;

// Gap between title / field / status line in the root column flex —
// matches the macOS / iOS settings-pane vertical rhythm (~16 px
// between related controls).
const ROW_GAP: u32 = 16;

// R657 §5.16 §5.38 — use_layout_cache / text_fg_for / field_fill_for
// / selection_fill / preedit_bg_fill / preedit_underline lifted to
// `pinion_widget_paint::text_field` (private impl details of
// [`tf_paint::view_field`] + [`tf_paint::ime_caret_rect_for`]).
//
// R790 §5.22 — the `Owner::cache`-keyed clipboard hook (`AppClipboard`
// wrapper + Arboard-with-InMemory-fallback) lifted to
// `pinion_platform_clipboard::use_app_clipboard`, shared by every
// text-field binding. The lifted wrapper forwards the selection-aware
// `copy_to` / `paste_from` to the inner impl (the pre-R790 per-binding
// copies dropped PRIMARY to the trait no-op default).

// R657 §5.16 §5.38 / R958.1 — `saturating_f32_to_u32` moved to
// `pinion_widget_paint::coord` (pub; the shared f32 -> u32 paint-coord
// seam: view_field / ime_caret_rect_for / line-gutter / menu anchor).

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
/// 1. `"Find & Replace"` title label (18 px white).
/// 2. The searchable field: 360×40, `tag = "find_editor"` for the input
///    router. The seeded text flows naturally; the find-match highlight bands
///    (R903) + the caret / selection overlays paint via
///    [`tf_paint::view_field`].
/// 3. Find-status bar (`tag = "find_status"`) — the "{n} of {N}" the editor
///    derives from its own find state, the visible mirror of the
///    `scene/<tag>/external/find_matches` RPC read.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors hello-toggle / hello-listbox"
)]
fn view(state: (TextFieldState, u32), _frame: &Frame) -> Scene {
    let (interaction, caret_byte) = state;

    // R657 §5.16 §5.38 — TextField paint composition (caret /
    // selection / preedit overlays) lifted to
    // [`tf_paint::view_field`] in the pinion-widget-paint crate.
    // The binding only composes its surrounding chrome (title +
    // status line) around the lifted field substrate.
    //
    // (R57.X.textfield §5.50) Active palette — `use_theme` auto-
    // subscribes this view-fn so a `ThemeProvider::set_theme` from
    // anywhere in the application re-runs the view + repaints the
    // field + caret + selection band with the new tones. R586 §5.50
    // `theme_animated` opts in to the R57.X.theme-fade cross-fade;
    // the at-rest snap path keeps the instant contract identical to
    // `theme()` once the spring has settled.
    let theme = use_theme(THEME_TAG).theme_animated();

    let field = tf_paint::view_field(
        TF_TAG,
        interaction,
        caret_byte,
        &theme,
        &tf_paint::TextFieldStyle::m3_filled(),
        "Searchable text",
    );

    let title = Scene::Text(TextNode::styled(
        "Find & Replace",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // R903 §5.22 — the find-status bar: the visible "{n} of {N}" the editor
    // derives from its own find state. Both the status and the field's
    // all-match highlight bands read the same `find_matches`, so what the user
    // sees and what `scene/<tag>/external/find_matches` reports never diverge.
    // The status text reads the reactive TextEditState directly; the field
    // paint already walked it through the lifted helper, both subscriptions
    // landing on the same `Rc<TextEditState>` per Owner::cache dedup.
    let text_state = use_text_edit_state(TF_TAG);
    let find_status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            find_status_text(&text_state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_SIZE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(FIND_STATUS_TAG.to_owned()),
    );

    Scene::Container(
        ContainerNode::new(vec![title, field, find_status])
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

/// R903 §5.22 — the human-readable find status, derived purely from the
/// editor's find state (needle + match list + current index). Empty needle →
/// a hint; an active needle → "{n} of {N}" when the selection is on a match,
/// else the bare match count. The same `find_matches` the highlight bands and
/// the `find_matches` RPC read, so the three never disagree.
fn find_status_text(text_state: &pinion_core::widgets::text_edit::TextEditState) -> String {
    let query = text_state.find_query();
    if query.is_empty() {
        return "Find: (set a needle via the find surface)".to_string();
    }
    let count = text_state.find_match_count();
    match text_state.find_current_index() {
        Some(idx) => format!("Find \"{query}\": {} of {count}", idx + 1),
        None if count == 1 => format!("Find \"{query}\": 1 match"),
        None => format!("Find \"{query}\": {count} matches"),
    }
}

/// `WidgetView` binding for the `TextField` widget.
///
/// State shape: `(TextFieldState, u32)` — the SCXML interaction state
/// plus the caret byte offset. The text content itself is reactive
/// (`Rc<TextEditState>` via `use_text_edit_state`), so it does not
/// (and cannot — `String` is not `Copy`) live in `Self::State`. The
/// view fn reads text via the same Owner-cache hook the External's
/// `attach_state` resolves through, so both sides see the same store.
struct FindReplaceView;

impl WidgetCore for FindReplaceView {
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
        // R903 §5.22 — seed the document so find / replace has matches to walk.
        // Seeded *before* the undo stack attaches, so the seed is the undo
        // floor (a Replace All can be undone back to the seed but not past it).
        if text_state.text().is_empty() {
            text_state.set_text(SEED_TEXT.to_string());
            // Seed an active needle too, so the example self-demonstrates: open
            // it and the four "red" matches are already highlighted with the
            // "{n} of {N}" status live. The find session is not journalled
            // (it is not buffer content), so this does not disturb the undo
            // floor.
            text_state.set_find_query("red");
        }
        // R796 §5.52 — journal edits onto a shared undo stack so Ctrl+Z /
        // Ctrl+Shift+Z (Ctrl+Y) undo / redo with word-level coalescing, and so
        // R903 Replace All folds its N splices into one undo step.
        text_state.attach_undo(use_undo_stack(TF_TAG));
        let blink = use_caret_blink(TF_TAG);
        // R56.1.e §5.22 — in-memory clipboard backing the demo's
        // Ctrl+C / Ctrl+X / Ctrl+V keystrokes. Shared via
        // `Owner::cache` so the dispatch path always resolves to
        // the same `Rc<dyn Clipboard>` across paint cycles
        // (mirror of the `use_caret_blink` hook shape; the
        // application surface gets a tag-keyed singleton without
        // touching `thread_local!`).
        let clipboard = use_app_clipboard(TF_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
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
        // R657 §5.16 §5.38 — delegate to the lifted helper that both
        // bindings (hello-textfield + todomvc) share. The same scene
        // walk + introspect read happens in one substrate place,
        // backward-compatible with the R55.D.5 single-vs-multi
        // External shape (find_external_with_tag handles both).
        tf_paint::read_text_field_state(scene, TF_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion hello-find-replace (R903 §5.22 §5.52)"
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
    /// `TextFieldExternal::invoke``("key", Text(key))`. Returns
    /// `true` when the External reports the key as recognized
    /// (matches the W3C `defaultPrevented` semantic — the framework
    /// then swallows the key from the focus / shortcut chain).
    ///
    /// The `focused != Some(TF_TAG)` short-circuit mirrors the
    /// roving-tabindex pattern from `hello-radio-group` /
    /// `hello-listbox`: keys only flow when this widget owns focus,
    /// avoiding the broadcast-to-every-widget aliasing that
    /// pre-R51.x `apply_key` suffered.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        // R764.1 §5.38 §5.22 — forward through the lifted SSOT
        // (`pinion_core::forward_key_to_field`); `modifiers` carries the
        // four W3C bits so native Shift+Arrow / Ctrl+A reach the
        // substrate's selection arms (R763). The roving-tabindex guard
        // above keeps the forward scoped to this widget's focus.
        pinion_core::forward_key_to_field(scene, TF_TAG, key, modifiers)
    }

    /// R56.2.a §5.13 §5.38 — delegate platform IME composition events
    /// to `TextFieldExternal::invoke``("composition", Json{action,
    /// data?})`. The pinion-shell `WindowEvent::Ime` arm converts
    /// winit's cross-platform `Ime` enum into
    /// pinion-native [`pinion_core::CompositionEvent`] (R56.2.a
    /// substrate) and routes here through `ShellCore::apply_composition`
    /// → `CoreShell::apply_composition` → `V::apply_composition`.
    /// This binding's impl reformats the typed enum back to the
    /// R56.1.g.2 wire-form so the substrate funnel (AI client RPC
    /// path + platform IME path) lands on the same code.
    ///
    /// Mapping table:
    ///
    /// | `CompositionEvent` variant | invoke args                                |
    /// |-----------------------------|--------------------------------------------|
    /// | `Start`                     | `{"action": "start"}`                      |
    /// | `Update(text)`              | `{"action": "update", "data": "<text>"}`   |
    /// | `Commit(text)`              | `{"action": "end",    "data": "<text>"}`   |
    /// | `Cancel`                    | `{"action": "cancel"}`                     |
    ///
    /// The `focused != Some(TF_TAG)` short-circuit mirrors
    /// [`Self::apply_key`]'s roving-tabindex pattern: composition
    /// events only flow when this widget owns focus, so an IME event
    /// arriving while focus rests on a non-text widget is dropped
    /// without disturbing the `TextField`'s substrate (defensive against
    /// a future per-focus `set_ime_allowed` regression that briefly
    /// leaks IME events past the focus boundary).
    ///
    /// Returns `true` whenever the invoke channel reports success
    /// (the R56.1.g.2 path always returns `Ok(Text(<state name>))`
    /// on a valid Json arg, so any `Ok` is treated as handled).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        // R764.1 §5.38 §5.13 — reformat + forward through the lifted SSOT.
        tf_paint::forward_composition_to_field(scene, TF_TAG, event)
    }

    /// R56.2.e §5.13 §5.22 — middle-mouse-button paste from PRIMARY.
    /// The pinion-shell `WindowEvent::MouseInput { Middle, Pressed }`
    /// arm routes through `ShellCore::middle_click` →
    /// `CoreShell::apply_middle_click` → here. This binding's impl
    /// reformats the trait call into the `paste-primary` invoke slot
    /// the R56.2.e.2 widget exposes — the substrate funnel keeps
    /// the AI-client RPC path and the platform middle-click path on
    /// the same code (mirror of `apply_composition` →
    /// `composition` invoke).
    ///
    /// The `focused != Some(TF_TAG)` short-circuit follows
    /// [`Self::apply_key`]'s roving-tabindex pattern so middle-click
    /// only pastes into the focused text field. The `_modifiers`
    /// arg is ignored — plain middle-click is the canonical X11 /
    /// Wayland PRIMARY paste, and Ctrl / Shift / Alt / Meta +
    /// middle-click is unspecified across desktops; this binding
    /// stays on the conservative path.
    ///
    /// Returns `true` whenever the invoke channel reports a
    /// non-empty PRIMARY payload was inserted (the R56.2.e.2 widget
    /// guards `paste-primary` against missing state / missing
    /// clipboard / empty PRIMARY internally, so any `Ok(Bool(true))`
    /// means the paste landed in the reactive text store).
    fn apply_middle_click(
        scene: &mut Scene,
        focused: Option<&str>,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(TF_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match intro.invoke("paste-primary", IntrospectValue::Null) {
            Ok(IntrospectValue::Bool(handled)) => handled,
            _ => false,
        }
    }

    fn fmt_state_log(state: &(TextFieldState, u32)) -> String {
        format!("{} / caret={}", state.0.as_name(), state.1,)
    }
}

impl WidgetA11y for FindReplaceView {
    /// R56.1.b.1 §5.40 — ARIA `textbox` role node carrying the live
    /// text content as `AccessValue::Text`. The
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
        let tag = <Self as WidgetCore>::tag();
        // R790 §5.40 — the textbox node mapping is the lifted SSOT shared
        // by every text-field binding (`tf_paint::text_field_a11y_node`).
        vec![tf_paint::text_field_a11y_node(
            tag,
            text,
            *interaction,
            focused == Some(tag),
        )]
    }
}

impl WidgetView for FindReplaceView {
    type Renderer = HelloFindReplaceRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    /// R56.2.c §5.13 §5.38 — publish the caret rect to the platform
    /// IME so the candidate window (ibus-hangul, fcitx5-hangul,
    /// macOS Hangul, Microsoft IME) positions next to the caret
    /// rather than at the default screen corner.
    ///
    /// Coordinate composition:
    ///
    /// 1. **Field rect in window coords** — walked from `scene` via
    ///    [`pinion_shell::rect_for_tag`]; the post-layout box of the
    ///    `TF_TAG` container carries the field's window-coord origin
    ///    (changes when the user resizes or the title text height
    ///    grows). This avoids hard-coding the field position in a
    ///    constant that would lie the first time the layout shifts.
    /// 2. **Text origin within the field** — `FIELD_PAD` on both axes
    ///    (matches the `with_padding(Rect::new(FIELD_PAD, …))` in
    ///    [`Self::view`]).
    /// 3. **Caret rect within the text layout** — same
    ///    `caret_rect_for_byte_offset` call the view fn runs (cache
    ///    hit on the `LayoutCache`, no re-shape) using the *visual*
    ///    caret byte (preedit-end during composition, substrate
    ///    caret otherwise — same splice the view fn produces so the
    ///    IME popup tracks the rendered cursor, not the latent
    ///    substrate cursor).
    ///
    /// Sum (1) + (2) + (3) → window-coord caret rect; the shell
    /// hands it to `Window::set_ime_cursor_area`.
    ///
    /// Width is the caret pixel width (`CARET_WIDTH = 2px`); some
    /// IMEs use this as the popup anchor width. Height carries the
    /// `FONT_SIZE_PX` floor so the candidate popup never collapses
    /// to a sliver when the layout's reported `height` is short
    /// (the same floor the view fn applies for the visible caret
    /// box).
    fn ime_caret_rect(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(TF_TAG) {
            return None;
        }
        let (interaction, caret_byte) = *state;
        // R657 §5.16 §5.38 — field rect walk stays binding-side (the
        // pinion-runtime helper is not pulled into pinion-widget-paint
        // — see crate docs for the dep-graph rationale). The caret
        // composition (splice + LayoutCache lookup + window-coord
        // sum) is the lifted [`tf_paint::ime_caret_rect_for`] helper.
        let field_rect = pinion_shell::rect_for_tag(scene, TF_TAG)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        Some(tf_paint::ime_caret_rect_for(
            TF_TAG,
            interaction,
            caret_byte,
            field_rect,
            &theme,
            &tf_paint::TextFieldStyle::m3_filled(),
        ))
    }

    /// R762 §5.36 §5.38 / R763 §5.22 — reverse of
    /// [`Self::ime_caret_rect`]: on a press inside the focused field,
    /// hit-test the cursor to a byte offset and either move the caret
    /// there (plain press → click-to-position, collapsing any selection)
    /// or — when Shift is held (`extend`) — extend the selection from
    /// the existing anchor to the hit byte (Shift-click). Returns the
    /// pinned selection anchor the shell stores to drive a subsequent
    /// drag. Both branches resolve the byte through the shared
    /// [`hit_test_field_byte`] helper (the `tf_paint::byte_for_field_point`
    /// SSOT — same `splice_preedit` + style + `LayoutCache` the visible
    /// caret is built from), so the glyph a click lands on and the byte
    /// it resolves to stay one SSOT.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        if focused != Some(TF_TAG) {
            return None;
        }
        // R801 §5.36 §5.35 — the shell hands the binding the router's
        // resolved hit-target. Move the caret only for a press the router
        // routed to this field; a press routed to a sibling widget while
        // the field keeps focus is not a caret move (the shell already
        // hit-tested it — no per-binding rect re-scan).
        if hit_tag != Some(TF_TAG) {
            return None;
        }
        let byte = hit_test_field_byte(*state, scene, x, y)?;
        let edit = use_text_edit_state(TF_TAG);
        if extend {
            // Shift-click: keep the existing anchor (latch the current
            // caret as the anchor when none) and move the focus end to
            // the hit byte. The retained anchor is returned so a drag
            // started by the shift-press continues from it.
            let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
            edit.set_selection(anchor, byte);
            Some(anchor)
        } else {
            // Plain press: collapse to a caret at the hit byte; that
            // byte becomes the drag anchor.
            edit.set_caret(byte);
            Some(byte)
        }
    }

    /// R763 §5.36 §5.22 — drag selection: while the button stays held
    /// after [`Self::position_caret_for_point`], the shell replays this
    /// for every cursor move. Hit-test the cursor and extend the
    /// selection from the pinned `anchor` (returned by the press) to the
    /// hit byte through the shared [`hit_test_field_byte`] SSOT, so a
    /// drag sweeps a live selection band. Returns `true` when the
    /// selection changed so the shell repaints only on real movement.
    fn select_drag_to_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        let Some(byte) = hit_test_field_byte(*state, scene, x, y) else {
            return false;
        };
        let edit = use_text_edit_state(TF_TAG);
        let before = (edit.caret(), edit.selection_anchor());
        edit.set_selection(anchor, byte);
        before != (edit.caret(), edit.selection_anchor())
    }
}

/// R762 §5.36 §5.38 / R763 §5.22 — shared pointer hit-test: resolve a
/// window-local pixel point to a byte offset in the field's text via
/// the [`tf_paint::byte_for_field_point`] SSOT (the same
/// `splice_preedit` + style + `LayoutCache` the visible caret is built
/// from). Returns `None` until the field rect is in the paint scene.
/// The single hit-test funnel shared by the press
/// ([`FindReplaceView::position_caret_for_point`]) and drag
/// ([`FindReplaceView::select_drag_to_point`]) hooks — divergence between
/// the two would let a drag select to a different byte than the press
/// caret landed on. Runs inside the shell root-owner scope (the
/// `use_theme` hook resolves there).
fn hit_test_field_byte(
    state: (TextFieldState, u32),
    scene: &Scene,
    x: f32,
    y: f32,
) -> Option<usize> {
    let (interaction, _caret_byte) = state;
    let field_rect = pinion_shell::rect_for_tag(scene, TF_TAG)?;
    let theme = use_theme(THEME_TAG).theme_animated();
    Some(tf_paint::byte_for_field_point(
        TF_TAG,
        interaction,
        x,
        y,
        field_rect,
        &theme,
        &tf_paint::TextFieldStyle::m3_filled(),
    ))
}

// R698 §5.16 §5.38 — TextField state <-> SCXML id mapping now routes
// through the `pinion_core::WidgetStateName` trait
// (`TextFieldState::as_name` / `TextFieldState::from_name_or_default`),
// retiring the local + paint-crate helpers.

fn main() {
    pinion_shell::run::<FindReplaceView>();
}

#[cfg(test)]
mod tests {
    //! R56.1.b.1 §5.38 — substrate-composition regression battery.
    //! Pinned at the binding level so a substrate rename / contract
    //! drift surfaces here before reaching the visible demo path.

    // R657 §5.16 §5.38 — substrate-side tests (state name lookup,
    // M3 color role mapping, view_field scene shape) moved into the
    // pinion-widget-paint crate where their helpers now live. Binding-
    // local tests below cover the BINDING contract: view-fn tag
    // presence, access_node shape, caret position smoke, palette-swap
    // reactive wiring through the binding's view fn.
    use super::{
        FIND_STATUS_TAG, FindReplaceView, SEED_TEXT, TF_TAG, THEME_TAG, find_status_text, view,
    };
    use pinion_a11y::{AccessValue, AriaRole, WidgetA11y};
    use pinion_core::reactive::Owner;
    use pinion_core::theme::{Theme, ThemeMode, use_theme};
    use pinion_core::widgets::caret_blink::use_caret_blink;
    use pinion_core::widgets::text_edit::use_text_edit_state;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Color, Frame, Scene};

    /// Run `f` inside a fresh `Owner` scope so reactive hooks
    /// resolve. Mirrors the framework's
    /// `root_owner.run(|| V::view(...))` wrap; tests use a private
    /// scope so each test starts with empty Owner cache state.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    // R657 §5.16 §5.38 — state-name round-trip + unknown-token
    // default coverage moved to `pinion-widget-paint::text_field`
    // tests where the helpers now live.

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

    // ─────────────────────────────────────────────────────────────
    // R903 §5.22 — find & replace binding surface
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r903_find_status_reports_match_count() {
        use pinion_core::widgets::text_edit::use_text_edit_state;
        with_owner(|| {
            let edit = use_text_edit_state(TF_TAG);
            edit.set_text(SEED_TEXT.to_owned());
            edit.set_find_query("red");
            // Four substring matches; no selection on a match → bare count.
            assert_eq!(find_status_text(&edit), "Find \"red\": 4 matches");
            // Whole-word drops the two inside `bored` / `credit` → 2 matches.
            edit.set_find_whole_word(true);
            assert_eq!(find_status_text(&edit), "Find \"red\": 2 matches");
            // Empty needle → the hint.
            edit.set_find_query("");
            assert!(find_status_text(&edit).starts_with("Find: (set a needle"));
        });
    }

    #[test]
    fn r903_find_status_reports_current_position() {
        use pinion_core::widgets::text_edit::use_text_edit_state;
        with_owner(|| {
            let edit = use_text_edit_state(TF_TAG);
            edit.set_text(SEED_TEXT.to_owned());
            edit.set_caret(0);
            edit.set_find_query("red");
            edit.find_next(); // selects the first match (the "Red" at 0)
            assert_eq!(find_status_text(&edit), "Find \"red\": 1 of 4");
        });
    }

    #[test]
    fn r903_active_find_adds_highlight_bands() {
        use pinion_core::widgets::text_edit::use_text_edit_state;
        with_owner(|| {
            let edit = use_text_edit_state(TF_TAG);
            edit.set_text(SEED_TEXT.to_owned());
            let plain = view((TextFieldState::Idle, 0), &Frame::default());
            let before = count_field_children(&plain, TF_TAG);
            edit.set_find_query("red");
            let lit = view((TextFieldState::Idle, 0), &Frame::default());
            let after = count_field_children(&lit, TF_TAG);
            assert!(
                after > before,
                "active find paints highlight bands behind the text ({before} -> {after})",
            );
        });
    }

    #[test]
    fn r903_view_carries_find_status_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(FIND_STATUS_TAG),
                "the find-status bar is present as scene-as-data",
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
    // R763 §5.22 — native keyboard modifier forwarding
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r763_native_shift_arrow_extends_selection() {
        use pinion_core::WidgetCore;
        use pinion_core::scene::ExternalNode;
        with_owner(|| {
            // The External attaches the same `use_text_edit_state(TF_TAG)`
            // Rc this test reads (one owner-cached holder per tag), so
            // mutations through `apply_key` surface on `edit`.
            let edit = use_text_edit_state(TF_TAG);
            edit.set_text("hello".to_owned());
            edit.set_caret(5);
            let mut scene = Scene::External(
                ExternalNode::new(FindReplaceView::create_external()).with_tag(TF_TAG),
            );
            let shift = pinion_core::Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
                meta: false,
            };
            // Pre-R763 the binding dropped `_modifiers` and always sent
            // the bare Text invoke shape, so native Shift+ArrowLeft
            // collapsed instead of extending. R763 forwards the Json
            // shape carrying the shift bit.
            assert!(FindReplaceView::apply_key(
                &mut scene,
                Some(TF_TAG),
                "ArrowLeft",
                shift
            ));
            assert_eq!(
                edit.selection_range(),
                Some((4, 5)),
                "native Shift+ArrowLeft extends one char left",
            );
            // Plain ArrowLeft (no modifiers) keeps the bare Text shape
            // and collapses the selection to its start.
            assert!(FindReplaceView::apply_key(
                &mut scene,
                Some(TF_TAG),
                "ArrowLeft",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(
                edit.selection_range(),
                None,
                "plain ArrowLeft collapses the selection",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // ARIA — role + value + state
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_b_1_access_node_role_is_text_input() {
        with_owner(|| {
            let nodes = FindReplaceView::access_node(&(TextFieldState::Idle, 0), None);
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

            let nodes = FindReplaceView::access_node(&(TextFieldState::Focused, 0), None);
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
            let unfocused = FindReplaceView::access_node(&(TextFieldState::Idle, 0), None);
            assert!(!unfocused[0].state.focused);

            let focused = FindReplaceView::access_node(&(TextFieldState::Focused, 0), Some(TF_TAG));
            assert!(focused[0].state.focused);
        });
    }

    #[test]
    fn r56_1_b_1_access_node_disabled_flag_set_when_disabled() {
        with_owner(|| {
            let nodes = FindReplaceView::access_node(&(TextFieldState::Disabled, 0), None);
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
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FindReplaceView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }

    // R657 §5.16 §5.38 — M3 fill role + selection-accent contract
    // covered by `pinion-widget-paint::text_field` tests; private
    // helpers field_fill_for / selection_fill / preedit_underline
    // are no longer reachable from this crate. The binding's
    // theme reactive wiring is still verified end-to-end by the
    // palette-swap test below (which observes the resulting paint
    // scene Container fill, not the private helper output).

    #[test]
    fn r57_x_textfield_palette_swap_propagates_through_view() {
        // End-to-end: changing the active theme between two
        // `view` invocations must surface a different field fill
        // somewhere in the rendered scene (the role-driven
        // resolution kicks in via the auto-subscribed
        // `use_theme().theme_animated()` read at the top of `view`).
        // R586 §5.50: the view-fn opts into R57.X.theme-fade, so the
        // dark assertion needs the spring to settle past the M3
        // short4 window — `settle_owner_animations` runs outside
        // `owner.run` because `tick_animations` advances the paint
        // clock independent of the active reactive scope.
        let owner = Owner::new();
        owner.run(|| {
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let light_scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene_contains_fill(
                &light_scene,
                Theme::light().surface_container_highest,
            ));
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            // Trigger the spring re-target; in-flight value discarded.
            let _ = view((TextFieldState::Idle, 0), &Frame::default());
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        owner.run(|| {
            let dark_scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(scene_contains_fill(
                &dark_scene,
                Theme::dark().surface_container_highest,
            ));
        });
    }

    fn scene_contains_fill(scene: &Scene, target: Color) -> bool {
        match scene {
            Scene::Container(n) => {
                n.style.fill == target || n.children.iter().any(|c| scene_contains_fill(c, target))
            }
            Scene::Box(n) => n.style.fill == target,
            _ => false,
        }
    }
}
