//! `todomvc` — R655 §5.16 first composed multi-widget application
//! verifying pinion's AI-native composition primitives end-to-end.
//!
//! ## Phase-2 application-tier entry
//!
//! Every prior `examples/hello-*` binding showcases **one** widget;
//! this binding is the first that composes **two** in a single
//! `WidgetView`: a [`TextFieldExternal`] input row at top and a
//! dynamic `Vec<String>` todo list rendered as a vertical column of
//! [`Scene::Text`] children below it. The `TasteJS` `TodoMVC` spec
//! is the canonical multi-widget benchmark (CRUD + filter +
//! persistence) — R655 lands only the **scaffolding** layer (input
//! + Enter-to-submit + static list rendering); R656 adds per-item
//!   toggle / edit / delete, R657 filtering, R658 persistence
//!   ([[r652-substrate-roi-matrix]] Phase-2 cascade plan).
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state +
//!   caret byte offset, inherited verbatim from hello-textfield.
//!   The textfield reactive text content lives on the
//!   [`TextEditState`] reached via [`use_text_edit_state`]`(TF_TAG)`,
//!   and the **todo list** lives on a separate
//!   `Signal<Vec<String>>` reached via [`use_todos`]`(TODOS_KEY)` —
//!   both reactive primitives are out-of-band from `Self::State`
//!   (which must be `Copy` per R51.173).
//! - Composition: the view fn returns a vertical
//!   [`Scene::Container`] holding `[title, field, status, list]`.
//!   The list child is itself a `Scene::Container` (tagged
//!   [`LIST_TAG`]) carrying one `Scene::Text` per todo entry, built
//!   by iterating `todos.get()` — the [[r655-todomvc-scaffolding]]
//!   pattern: dynamic `Vec<T> -> Vec<Scene>` directly in the view
//!   fn, no list-renderer substrate yet (deferred until 2nd consumer
//!   per [[abstraction-needs-second-consumer]]).
//! - Submit wire: [`apply_key`](WidgetCore::apply_key) intercepts
//!   `"Enter"` BEFORE delegating to
//!   [`TextFieldExternal::invoke`]`("key", Text)`. On Enter, the
//!   binding reads `text_state.text()`, trims, and (when non-empty)
//!   appends to the `Signal<Vec<String>>` via `set_with` + clears
//!   the textfield via `text_state.set_text(String::new())`. Other
//!   keys fall through to the textfield's standard W3C key wire.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p todomvc
//! ```
//!
//! Tab into the input → caret appears + blinks. Type "milk" →
//! `Enter` → "milk" appears in the list, field clears. Type
//! "eggs" → `Enter` → list has 2 entries. Press `d` to disable the
//! field, `e` to re-enable.

use std::cell::RefCell;
use std::rc::Rc;

use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::{caret_rect_for_byte_offset, CaretRect, LayoutCache};

// pinion-forge codegen output. Defines `pub struct TodoMvcRenderer`
// + async `new<W: Into<wgpu::SurfaceTarget<'static>>>` + sync
// `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>` can
// construct + render + resize it.
vello_renderer_impl!(TodoMvcRenderer, TodoMvcRendererError);

/// Tag for the textfield widget. Matches the `WidgetCore::tag`
/// (paint-root + input-router hit-test target) and the
/// [`use_text_edit_state`] / [`use_caret_blink`] cache keys, so the
/// `create_external` factory and the view fn both resolve to the same
/// reactive `Rc<TextEditState>` + `Rc<CaretBlink>` instances.
const TF_TAG: &str = "main_textfield";

/// (R655 §5.16) Tag for the todo list `Scene::Container` so RPC
/// `scene/query` can introspect the list shape independently of the
/// textfield via the existing `Scene::find_*` walkers.
const LIST_TAG: &str = "todo_list";

/// (R655 §5.16) [`Owner::cache`] key for the reactive
/// `Signal<Vec<String>>` carrying the todo entries. Symmetric with
/// the [`TF_TAG`] / [`use_text_edit_state`] convention — the
/// [`apply_key`] handler and the view fn both resolve through this
/// key, so the same `Rc<Signal<Vec<String>>>` instance is shared and
/// reactive subscriptions land in the same store. The Phase-2
/// cascade (R656 toggle/delete) will swap `Vec<String>` for
/// `Vec<TodoItem>` while keeping this hook as the substrate seam.
const TODOS_KEY: &str = "todomvc.todos";

const WIN_W: u32 = 480;
// R655 §5.16 — tall enough to host the textfield section (title +
// field + status ≈ 120 px) plus a ~280 px todo list region, mirroring
// the macOS / iOS Reminders-app vertical rhythm. The list grows
// downward; clipping into a scroll container is deferred to a later
// round when scroll-on-overflow surfaces (per
// [[abstraction-needs-second-consumer]]).
const WIN_H: u32 = 480;

/// (R57.X.textfield §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` so the example gallery shares one provider when a
/// host binds them together.
const THEME_TAG: &str = "app";

/// (R655 §5.16) `Owner::cache`-keyed hook returning the shared
/// `Rc<Signal<Vec<String>>>` of submitted todo entries. Symmetric
/// with [`use_text_edit_state`] / [`use_caret_blink`] hook shape —
/// the [`Owner::cache`] dedup guarantees one `Rc` across the view fn
/// (subscribes via `.get()` to re-run paint on submit) and
/// [`apply_key`] (mutates via `.set_with(|v| v.push(...))` on
/// Enter). Single-source-of-truth for the todo list state; the
/// signal's equality-skip suppresses the re-run when an empty push
/// would no-op (the Enter handler guards on `!text.trim().is_empty()`
/// so this rarely triggers in practice).
fn use_todos() -> Rc<Signal<Vec<String>>> {
    Owner::current()
        .expect("use_todos requires an active Owner scope")
        .cache(TODOS_KEY, || Signal::new(Vec::<String>::new()))
}

/// (R655 §5.16) Title color for the todo list section header — same
/// `OnSurface` role the textfield title above resolves through, so
/// the two labels read with identical weight against the surface
/// fill. Lifted out of the view fn body so the symbol shows up in
/// tests + grep audits as a single source.
const LIST_TITLE_FONT_SIZE_PX: u32 = 14;

/// (R655 §5.16) Per-item font size for the rendered todo text —
/// matches the textfield status line (12 px) so the visual rhythm
/// reads as "compact secondary content under the active input".
const LIST_ITEM_FONT_SIZE_PX: u32 = 14;

/// (R655 §5.16) Vertical gap between list items, smaller than the
/// section-level [`ROW_GAP`] so items pack tighter than the
/// title/field/status rows. 6 px ≈ 0.5× the row gap.
const LIST_ITEM_GAP: u32 = 6;

// Field surface — 360×40 with 8 px padding, 4 px corner radius. Fill
// shifts on focus to give the user a clear "the input is live" cue
// without an explicit border-colour change (CSS `:focus-visible`
// convention scaled down to a flat-fill palette).
//
// (R57.X.textfield §5.50) Fill / text / caret / selection / preedit
// colors are now theme-resolved per the M3 TextField role mapping
// captured in [`build_text_style`] + the view body. Pre-cleanup these
// lived as `Color::rgb(...)` consts; the migration replaces every
// site with `theme.resolve(role)` so the same view-fn renders
// correctly under both light and dark palettes.
const FIELD_W: u32 = 360;
const FIELD_H: u32 = 40;
const FIELD_PAD: u32 = 8;
const FIELD_CORNER: u32 = 4;
/// (R57.X.textfield §5.50) Selection tint alpha — the rect paints at
/// this opacity over the [`ColorRole::Accent`] color so the underlying
/// glyphs stay readable while the band marks the selection range.
/// 0xA0 matches the macOS / Chrome "system selection" overlay weight.
const SELECTION_ALPHA: u8 = 0xa0;
/// (R57.X.textfield §5.50) Preedit background tint alpha — fainter
/// than selection so the IME composition segment reads as
/// "provisional, not yet committed". 0x40 ≈ 25 % opacity.
const PREEDIT_BG_ALPHA: u8 = 0x40;
const PREEDIT_UNDERLINE_THICKNESS: u32 = 1;
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

/// (R57.X.textfield §5.50) Material 3 `TextField` text foreground —
/// `ColorRole::OnSurface` when enabled, `ColorRole::OnSurfaceMuted`
/// when the field is in the disabled posture. Lifted out of the
/// view-fn + `ime_caret_rect` so both paths produce the same
/// `TextStyle.fg` and the shared [`LayoutCache`] key matches across
/// the two calls — the pre-cleanup `(text, style, max_width)` tuple
/// hit relied on a `TEXT_COLOR` literal, the role-resolved variant
/// keeps that hit while picking up theme swaps.
fn text_fg_for(theme: &Theme, interaction: TextFieldState) -> Color {
    if matches!(interaction, TextFieldState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// (R57.X.textfield §5.50) Material 3 `TextField` filled-variant
/// container fill — `ColorRole::SurfaceContainerHighest` is the
/// canonical M3 `TextField` "filled" surface. Focused state lifts
/// one tier (R51 mirror) to `SurfaceContainerHigh` so the active
/// field reads as elevated without a heavy border ring. Disabled
/// fades toward `Surface` per the M3 38 % disabled overlay
/// convention.
fn field_fill_for(theme: &Theme, interaction: TextFieldState) -> Color {
    match interaction {
        TextFieldState::Idle => theme.resolve(ColorRole::SurfaceContainerHighest),
        TextFieldState::Focused | TextFieldState::Editing => {
            theme.resolve(ColorRole::SurfaceContainerHigh)
        }
        TextFieldState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

/// (R57.X.textfield §5.50) Selection rect tint — semi-transparent
/// `ColorRole::Accent` overlay. Building the color manually preserves
/// the M3 caret-color = Accent identity (the selection inherits the
/// active-control hue) while honouring [`SELECTION_ALPHA`] so the
/// glyphs under the band stay readable.
fn selection_fill(theme: &Theme) -> Color {
    let a = theme.resolve(ColorRole::Accent);
    Color::rgba(a.r, a.g, a.b, SELECTION_ALPHA)
}

/// (R57.X.textfield §5.50) Preedit background tint — fainter Accent
/// overlay than [`selection_fill`] so the IME composition segment
/// reads as provisional. Companion role for [`preedit_underline`].
fn preedit_bg_fill(theme: &Theme) -> Color {
    let a = theme.resolve(ColorRole::Accent);
    Color::rgba(a.r, a.g, a.b, PREEDIT_BG_ALPHA)
}

/// (R57.X.textfield §5.50) Preedit underline color — opaque Accent.
/// Mirrors the M3 / canonical IME convention where the underline
/// matches the active control hue (caret + underline + selection all
/// resolve through `ColorRole::Accent` so a palette swap re-stains
/// the field's interactive affordances coherently).
fn preedit_underline(theme: &Theme) -> Color {
    theme.resolve(ColorRole::Accent)
}

/// R56.1.e §5.22 / R56.2.b §5.22 — `Owner::cache`-keyed clipboard
/// hook. Mirrors the [`use_text_edit_state`] / [`use_caret_blink`]
/// hooks; the cache key dedups so the External's `attach_clipboard`
/// and any later (carry) view-fn read resolve to the same
/// `Rc<dyn Clipboard>` instance.
///
/// R56.2.b §5.22 — prefers the platform-backed
/// [`ArboardClipboard`] (Wayland `wl_data_device` + X11 CLIPBOARD +
/// macOS `NSPasteboard` + Windows `OpenClipboard` via the canonical
/// Rust ecosystem `arboard` crate) and falls back to the in-memory
/// impl on init failure (headless CI, sandboxed display-less
/// container, broken Wayland socket). The fallback keeps the
/// keyboard-shortcut UX functional (Ctrl/Cmd+C → Ctrl/Cmd+V
/// round-trip within the running hello-textfield process) at the
/// cost of cross-process clipboard sharing.
///
/// The dispatch is wrapped in [`AppClipboard`] so the
/// `Owner::cache<V>` slot stores a single `Sized` type regardless of
/// which inner impl wins; downstream consumers receive the
/// `Rc<dyn Clipboard>` trait-object shape through the
/// [`AppClipboard`] `Clipboard` impl's forwarding pair.
fn use_clipboard(key: &'static str) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("use_clipboard requires an active Owner scope")
        .cache(key, || {
            AppClipboard(match ArboardClipboard::try_new() {
                Ok(arboard) => Box::new(arboard) as Box<dyn Clipboard>,
                Err(e) => {
                    eprintln!(
                        "hello-textfield: ArboardClipboard init failed \
                         ({e}); falling back to InMemoryClipboard \
                         (cross-process clipboard disabled)",
                    );
                    Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>
                }
            })
        });
    cb
}

/// R56.2.b §5.22 — `Sized` wrapper around `Box<dyn Clipboard>` so
/// the [`use_clipboard`] hook can park either an
/// [`ArboardClipboard`] (platform-backed, the common case) or an
/// [`InMemoryClipboard`] (fallback when the platform clipboard
/// daemon is unreachable) inside the same `Owner::cache<V>` slot.
/// The framework `Owner::cache<V>` API requires `V: 'static` and
/// chooses a concrete `V` per slot; the typed-erased
/// `Box<dyn Clipboard>` interior here is the single concrete `V`
/// the hello-textfield binding stores while the dispatch chooses
/// at runtime which impl backs it.
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
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

    // R56.2.f §5.38 §5.22 — preedit splice via the substrate helper.
    // Returns (effective_text, visual_caret_byte, preedit_byte_range):
    // the composed view of "committed buffer + spliced preedit" the
    // user sees during IME composition. When no composition is active
    // (or the preedit string is empty), effective_text == committed
    // text and the range is None. Mirrors W3C compositionupdate
    // canonical caret-at-preedit-end semantics. The TUI binding
    // (hello-textfield-tui) and the `ime_caret_rect` impl below use
    // the same helper so all three paths share one splice — the
    // LayoutCache key (effective_text + style) hits across the paths
    // and no per-path duplication can drift.
    let (effective_text, visual_caret_byte, preedit_byte_range) =
        text_state.splice_preedit(caret_byte as usize);
    let preedit = text_state.preedit();

    // (R57.X.textfield §5.50) Active palette — `use_theme` auto-
    // subscribes this view-fn so a `ThemeProvider::set_theme` from
    // anywhere in the application re-runs the view + repaints the
    // field + caret + selection band with the new tones.
    // (R586 §5.50) `theme_animated` opts in to the R57.X.theme-fade
    // cross-fade; the at-rest snap path keeps the instant contract
    // identical to `theme()` once the spring has settled.
    let theme = use_theme(THEME_TAG).theme_animated();
    let text_style = TextStyle::new()
        .with_size_px(FONT_SIZE_PX)
        .with_fg(text_fg_for(&theme, interaction));

    // Caret geometry — shape the current effective_text once via the
    // shared `LayoutCache`, then look up the cursor rect at the
    // visual caret offset. The `LayoutCache::layout` LRU returns the
    // same `Layout` reference for the same `(text, style, max_width)`
    // tuple, so re-runs of the view fn inside the same paint cycle
    // reuse the shaped run instead of re-shaping per call.
    //
    // R56.1.f.3 §5.22 — selection range pixel geometry. When a
    // selection is active, two extra `caret_rect_for_byte_offset`
    // lookups derive the start + end x offsets so the selection box
    // can paint behind the text. The same shared `LayoutCache`
    // reuses the shaped run across all three queries — no repeated
    // shaping work.
    //
    // R56.1.g.3 §5.22 — preedit pixel range derives from two more
    // `caret_rect_for_byte_offset` calls against the same shaped
    // effective_text run; the underline + tinted background paint
    // behind the glyphs in the preedit byte range.
    let layout_cache = use_layout_cache("hello_textfield.layout_cache");
    let selection_range = text_state.selection_range();
    let (caret_pixel_rect, selection_pixel, preedit_pixel) = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(effective_text.as_str(), &text_style, None);
        #[allow(
            clippy::cast_precision_loss,
            reason = "CARET_WIDTH fits f32 losslessly (2 << 23 ceiling)"
        )]
        let rect = caret_rect_for_byte_offset(
            layout,
            visual_caret_byte,
            CARET_WIDTH as f32,
        );
        let height_floor = saturating_f32_to_u32(rect.height).max(FONT_SIZE_PX);
        let caret = (
            saturating_f32_to_u32(rect.x),
            saturating_f32_to_u32(rect.y),
            height_floor,
        );
        let selection = selection_range.map(|(start, end)| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "CARET_WIDTH fits f32 losslessly"
            )]
            let start_rect =
                caret_rect_for_byte_offset(layout, start, CARET_WIDTH as f32);
            #[allow(
                clippy::cast_precision_loss,
                reason = "CARET_WIDTH fits f32 losslessly"
            )]
            let end_rect = caret_rect_for_byte_offset(layout, end, CARET_WIDTH as f32);
            let start_x = saturating_f32_to_u32(start_rect.x);
            let end_x = saturating_f32_to_u32(end_rect.x);
            let sel_y = saturating_f32_to_u32(start_rect.y);
            let sel_h = saturating_f32_to_u32(start_rect.height).max(FONT_SIZE_PX);
            (start_x, sel_y, end_x.saturating_sub(start_x), sel_h)
        });
        let preedit_p = preedit_byte_range.map(|(start, end)| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "CARET_WIDTH fits f32 losslessly"
            )]
            let start_rect =
                caret_rect_for_byte_offset(layout, start, CARET_WIDTH as f32);
            #[allow(
                clippy::cast_precision_loss,
                reason = "CARET_WIDTH fits f32 losslessly"
            )]
            let end_rect = caret_rect_for_byte_offset(layout, end, CARET_WIDTH as f32);
            let start_x = saturating_f32_to_u32(start_rect.x);
            let end_x = saturating_f32_to_u32(end_rect.x);
            let pre_y = saturating_f32_to_u32(start_rect.y);
            let pre_h = saturating_f32_to_u32(start_rect.height).max(FONT_SIZE_PX);
            (start_x, pre_y, end_x.saturating_sub(start_x), pre_h)
        });
        (caret, selection, preedit_p)
    };
    let (caret_layout_x, caret_layout_y, caret_box_height) = caret_pixel_rect;

    let field_fill = field_fill_for(&theme, interaction);

    // Text node — natural-flow child of the field container. Empty
    // text is rendered as a zero-width run so the caret still appears
    // at x=0 inside the padded field.
    //
    // R56.1.g.3 §5.22 — during composition, the rendered text is the
    // composed `effective_text` (committed buffer + spliced preedit
    // at the caret position), not the raw `text_state.text()` buffer.
    let text_node = Scene::Text(TextNode::styled(
        effective_text.clone(),
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

    let mut field_children: Vec<Scene> = Vec::with_capacity(4);
    // R56.1.f.3 §5.22 — selection rect paints BEFORE text_node so
    // the glyphs render on top of the tinted band. Vello composites
    // children in vector order (later children paint atop earlier).
    if let Some((sel_x, sel_y, sel_w, sel_h)) = selection_pixel {
        if sel_w > 0 {
            let sel_left = FIELD_PAD.saturating_add(sel_x);
            let sel_top = FIELD_PAD.saturating_add(sel_y);
            let selection_box = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(selection_fill(&theme)))
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(sel_w, sel_h))
                            .with_absolute_position(sel_left, sel_top),
                    ),
            );
            field_children.push(selection_box);
        }
    }
    // R56.1.g.3 §5.22 — preedit background tint paints BEFORE the
    // text node (same layering rule as the selection band) so glyphs
    // composite on top of the tint. The IME affordance reads as
    // "this run is provisional".
    if let Some((pre_x, pre_y, pre_w, pre_h)) = preedit_pixel {
        if pre_w > 0 {
            let pre_left = FIELD_PAD.saturating_add(pre_x);
            let pre_top = FIELD_PAD.saturating_add(pre_y);
            let preedit_bg = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(preedit_bg_fill(&theme)))
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(pre_w, pre_h))
                            .with_absolute_position(pre_left, pre_top),
                    ),
            );
            field_children.push(preedit_bg);
        }
    }
    field_children.push(text_node);
    // R56.1.g.3 §5.22 — preedit underline paints AFTER the text node
    // so the line sits over the descender region (visual "this is a
    // preedit run" affordance). 1 px line below the text baseline +
    // a sliver of descender room — the canonical IME underline shape.
    if let Some((pre_x, pre_y, pre_w, pre_h)) = preedit_pixel {
        if pre_w > 0 {
            let pre_left = FIELD_PAD.saturating_add(pre_x);
            let underline_top = FIELD_PAD
                .saturating_add(pre_y)
                .saturating_add(pre_h)
                .saturating_sub(PREEDIT_UNDERLINE_THICKNESS);
            let underline = Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(preedit_underline(&theme)),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(pre_w, PREEDIT_UNDERLINE_THICKNESS))
                        .with_absolute_position(pre_left, underline_top),
                ),
            );
            field_children.push(underline);
        }
    }
    if caret_painted {
        let caret_left = FIELD_PAD.saturating_add(caret_layout_x);
        let caret_top = FIELD_PAD.saturating_add(caret_layout_y);
        let caret_box = Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(theme.resolve(ColorRole::Accent))).with_layout(
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
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // R56.1.g.3 §5.22 — status line carries the preedit state so the
    // AI side can verify composition lifecycle through the visible
    // status row (mirror of the `scene/query` `preedit` slot —
    // observable both visually and over RPC).
    let preedit_status = match preedit.as_ref() {
        Some(p) => format!(" | preedit=\"{p}\""),
        None => String::new(),
    };
    let status_str = format!(
        "{} | caret={} | text=\"{}\"{}",
        text_field_state_name(interaction),
        caret_byte,
        text,
        preedit_status,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    // R655 §5.16 — todo list section: a header label + one
    // `Scene::Text` per submitted entry, packed in a tagged
    // `Scene::Container` so RPC `scene/query` can address the list
    // independently from the textfield. Reading `todos.get()`
    // subscribes the view fn to the `Signal<Vec<String>>`, so a
    // `set_with` from the Enter handler re-runs paint with the new
    // entries on the next frame.
    let todos = use_todos();
    let entries: Vec<String> = todos.get();
    let todos_list = build_todos_list(&theme, &entries);

    Scene::Container(
        ContainerNode::new(vec![title, field, status, todos_list])
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

/// (R655 §5.16) Build the todo list section `Scene::Container`:
///
/// - When `entries` is empty, returns an empty container (still
///   tagged [`LIST_TAG`] so the RPC introspection path can confirm
///   the list region exists; otherwise hit-test routing for a
///   future delete button would lose its anchor).
/// - When `entries` is non-empty, returns a header
///   `"Todos (<N>)"` label followed by one `Scene::Text` per entry.
///
/// Helper lifted out of the view-fn body so the symbol shows up in
/// the test surface — the [[r655-todomvc-scaffolding]] regression
/// battery walks this output to pin item count + ordering. Dynamic
/// `Vec<T> -> Vec<Scene>` rendering is per-binding right now; if a
/// 2nd composed application reuses the shape, a `list_view` helper
/// lifts to substrate per [[abstraction-needs-second-consumer]].
fn build_todos_list(theme: &Theme, entries: &[String]) -> Scene {
    let header_style = TextStyle::new()
        .with_size_px(LIST_TITLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));
    let item_style = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));

    let header_text = if entries.is_empty() {
        String::from("No todos yet — type and press Enter")
    } else {
        format!("Todos ({})", entries.len())
    };
    let header = Scene::Text(TextNode::styled(
        header_text,
        Rect::default(),
        header_style,
    ));

    // Each row tagged `todo_item#<i>` so a future delete / toggle
    // round (R656) can route hit-tests to the per-item callback.
    // For R655 scaffolding the items are purely visual — no
    // interaction yet, no `External` per item — so the substrate
    // cost stays at zero. The tag-with-index pattern mirrors
    // `hello-listbox`'s `listbox_row(i, ...)` (R55.G.20 convention).
    let mut children: Vec<Scene> = Vec::with_capacity(entries.len() + 1);
    children.push(header);
    for (idx, entry) in entries.iter().enumerate() {
        let row = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                entry.clone(),
                Rect::default(),
                item_style.clone(),
            ))])
            .with_tag(format!("todo_item#{idx}"))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
        );
        children.push(row);
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(LIST_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(LIST_ITEM_GAP),
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
struct TodoMvcView;

impl WidgetCore for TodoMvcView {
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
        // R56.1.e §5.22 — in-memory clipboard backing the demo's
        // Ctrl+C / Ctrl+X / Ctrl+V keystrokes. Shared via
        // `Owner::cache` so the dispatch path always resolves to
        // the same `Rc<dyn Clipboard>` across paint cycles
        // (mirror of the `use_caret_blink` hook shape; the
        // application surface gets a tag-keyed singleton without
        // touching `thread_local!`).
        let clipboard = use_clipboard(TF_TAG);
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
        "pinion todomvc (R655 §5.16) — first composed app"
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
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TF_TAG) {
            return false;
        }
        // R655 §5.16 — Enter submits the current textfield content as
        // a new todo entry, then clears the field. The submit path
        // runs BEFORE delegating to `TextFieldExternal.invoke("key",
        // ...)` so the textfield's substrate never sees Enter (which
        // it would otherwise drop on the floor for a single-line
        // input). Trim guard mirrors the TasteJS TodoMVC spec: blank
        // entries are never added. Modifiers are ignored — plain
        // Enter and Shift+Enter both submit, matching the canonical
        // single-line search-bar / chat-input UX.
        if key == "Enter" {
            let text_state = use_text_edit_state(TF_TAG);
            let raw = text_state.text();
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let entry = trimmed.to_owned();
                let todos = use_todos();
                todos.set_with(|prev| {
                    let mut next = prev.clone();
                    next.push(entry.clone());
                    next
                });
                // `set_text(String::new())` atomically clears
                // text + caret + selection + preedit via the
                // R56.1.f `batch` (see `text_edit.rs:360-369`), so
                // a single call is the textbook reset.
                text_state.set_text(String::new());
            }
            return true;
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

    /// R56.2.a §5.13 §5.38 — delegate platform IME composition events
    /// to [`TextFieldExternal::invoke`]`("composition", Json{action,
    /// data?})`. The pinion-shell `WindowEvent::Ime` arm converts
    /// winit's cross-platform [`Ime`](winit::event::Ime) enum into
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
        let Some(node) = scene.find_external_with_tag_mut(TF_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let args = match event {
            pinion_core::CompositionEvent::Start => {
                IntrospectValue::Json(serde_json::json!({ "action": "start" }))
            }
            pinion_core::CompositionEvent::Update(text) => IntrospectValue::Json(
                serde_json::json!({ "action": "update", "data": text }),
            ),
            pinion_core::CompositionEvent::Commit(text) => IntrospectValue::Json(
                serde_json::json!({ "action": "end", "data": text }),
            ),
            pinion_core::CompositionEvent::Cancel => {
                IntrospectValue::Json(serde_json::json!({ "action": "cancel" }))
            }
            // `CompositionEvent` is `#[non_exhaustive]`; defer
            // any future variant (delete_surrounding etc.) to the
            // shell's fallback by reporting unhandled here.
            _ => return false,
        };
        intro.invoke("composition", args).is_ok()
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
        format!(
            "{} / caret={}",
            text_field_state_name(state.0),
            state.1,
        )
    }
}

impl WidgetA11y for TodoMvcView {
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

impl WidgetView for TodoMvcView {
    type Renderer = TodoMvcRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
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
    /// hands it to [`Window::set_ime_cursor_area`].
    ///
    /// Width is the caret pixel width (`CARET_WIDTH = 2px`); some
    /// IMEs use this as the popup anchor width. Height carries the
    /// `FONT_SIZE_PX` floor so the candidate popup never collapses
    /// to a sliver when the layout's reported `height` is short
    /// (the same floor the view fn applies for the visible caret
    /// box).
    #[allow(
        clippy::similar_names,
        reason = "field_origin_x_f / field_origin_y_f mirror the field_rect.x / field_rect.y source; renaming further would obscure the source-mapping symmetry the caret arithmetic depends on"
    )]
    fn ime_caret_rect(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(TF_TAG) {
            return None;
        }
        let (interaction, caret_byte) = *state;
        let text_state = use_text_edit_state(TF_TAG);
        // R56.2.f §5.38 §5.22 — splice via the substrate helper so the
        // effective_text + visual_caret_byte match the view fn
        // exactly. The LayoutCache key (effective_text + style)
        // hits the same cached layout the view fn produced this
        // frame — the candidate-popup geometry tracks the rendered
        // cursor with zero extra shaping work.
        let (effective_text, visual_caret_byte, _preedit_byte_range) =
            text_state.splice_preedit(caret_byte as usize);
        // Mirror the view fn's text style (incl. interaction-dependent
        // fg) so the `(text, style, max_width)` cache key matches and
        // the `LayoutCache::layout` lookup is a hit. (R57.X.textfield
        // §5.50) Both sites resolve through [`text_fg_for`] so a
        // palette swap re-keys both LayoutCache reads in lock-step
        // without drifting the cache identity. (R586 §5.50) Mirror
        // the view-fn migration — both sites read the same animated
        // palette so the cache identity stays in lock-step during the
        // R57.X.theme-fade cross-fade and snaps together at rest.
        //
        // (R587 §5.36) Why lock-step beats rolling this site back to
        // `theme()` during the fade: `LayoutKey` includes `fg_color`,
        // so each frame the view fn emits a fresh entry under the
        // in-flight lerp color. With lock-step both sites query the
        // *same* fresh entry (same-frame cache hit, zero extra shape
        // pass). Reading `theme()` here instead would key a separate
        // target-color entry per frame, doubling the cache footprint
        // and adding one shape per first-frame-after-flip — strictly
        // worse than the lock-step path. The over-specified key (paint
        // metadata in a shape-only cache) is a latent perf hazard for
        // long / multi-line consumers; carried as a Rule of Three split
        // on `pinion_text::cache::LayoutKey`.
        let theme = use_theme(THEME_TAG).theme_animated();
        let text_style = TextStyle::new()
            .with_size_px(FONT_SIZE_PX)
            .with_fg(text_fg_for(&theme, interaction));
        let field_rect = pinion_shell::rect_for_tag(scene, TF_TAG)?;
        let layout_cache = use_layout_cache("hello_textfield.layout_cache");
        let caret_local = {
            let mut cache = layout_cache.borrow_mut();
            let layout = cache.layout(effective_text.as_str(), &text_style, None);
            #[allow(
                clippy::cast_precision_loss,
                reason = "CARET_WIDTH fits f32 losslessly (2 << 23 ceiling)"
            )]
            let cw = CARET_WIDTH as f32;
            caret_rect_for_byte_offset(layout, visual_caret_byte, cw)
        };
        #[allow(
            clippy::cast_precision_loss,
            reason = "field_rect.{x,y} are u32 viewport coords; window sizes never approach 2^24 logical px"
        )]
        let field_origin_x_f = field_rect.x as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "field_rect.{x,y} are u32 viewport coords; window sizes never approach 2^24 logical px"
        )]
        let field_origin_y_f = field_rect.y as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "FIELD_PAD + FONT_SIZE_PX are small u32 constants"
        )]
        let pad_f = FIELD_PAD as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "FONT_SIZE_PX small u32 constant"
        )]
        let font_size_f = FONT_SIZE_PX as f32;
        Some(CaretRect::new(
            field_origin_x_f + pad_f + caret_local.x,
            field_origin_y_f + pad_f + caret_local.y,
            caret_local.width.max(1.0),
            caret_local.height.max(font_size_f),
        ))
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
    pinion_shell::run::<TodoMvcView>();
}

#[cfg(test)]
mod tests {
    //! R655 §5.16 — todomvc-specific regression battery. Substrate
    //! correctness for the embedded `TextField` widget is covered by
    //! `hello-textfield`'s own tests (R56.1.b.1) — this module pins
    //! only the **composition** layer this binding introduces: the
    //! todo list section presence, item-count → child-count
    //! invariant, and the [`use_todos`] reactive hook dedup contract
    //! the Enter handler relies on. The R55.G.22 paint-root tag
    //! convention is pinned via the framework fixture call.
    //!
    //! Note: the Enter handler itself runs against a `&mut Scene`
    //! that the runtime owns; testing it requires the shell's input
    //! loop. The handler's signal mutation is exercised indirectly
    //! here by manipulating the `Signal<Vec<String>>` directly under
    //! a private `Owner` and asserting the view-fn renders the new
    //! entries — the same observable surface the visible app
    //! depends on.
    use super::{
        build_todos_list, use_todos, view, TodoMvcView, LIST_TAG, TF_TAG,
    };
    use pinion_core::reactive::Owner;
    use pinion_core::theme::Theme;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene};

    /// Run `f` inside a fresh `Owner` scope so reactive hooks
    /// resolve. Mirrors the framework's
    /// `root_owner.run(|| V::view(...))` wrap; tests use a private
    /// scope so each test starts with empty Owner cache state.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    /// Walk the scene tree and return the children of the
    /// `Scene::Container` whose tag matches `tag`. Returns an empty
    /// vec when the tag is not found. Used to confirm the todo list
    /// section's child count + per-item tag layout.
    fn find_children_for_tag<'a>(scene: &'a Scene, tag: &str) -> Vec<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return c.children.iter().collect();
                }
                for child in &c.children {
                    let n = find_children_for_tag(child, tag);
                    if !n.is_empty() {
                        return n;
                    }
                }
                Vec::new()
            }
            Scene::Scroll(s) => find_children_for_tag(&s.content, tag),
            _ => Vec::new(),
        }
    }

    /// Extract the text content of every `Scene::Text` node reachable
    /// under `scene`, depth-first in declaration order. Used to
    /// confirm submitted entries appear as visible text in the same
    /// order the user typed them.
    fn walk_text(s: &Scene, out: &mut Vec<String>) {
        match s {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    walk_text(child, out);
                }
            }
            Scene::Scroll(s) => walk_text(&s.content, out),
            _ => {}
        }
    }
    fn collect_text_nodes(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        walk_text(scene, &mut out);
        out
    }

    // ─────────────────────────────────────────────────────────────
    // Composition — tag presence + list region existence
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_view_carries_tf_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(TF_TAG),
                "paint scene must carry the TextField widget tag",
            );
        });
    }

    #[test]
    fn r655_view_carries_list_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(LIST_TAG),
                "paint scene must carry the todo-list tag even when empty",
            );
        });
    }

    #[test]
    fn r655_g20_view_contains_composite_paint_root_tag() {
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TodoMvcView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }

    // ─────────────────────────────────────────────────────────────
    // List rendering — entry count → child count invariant
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_empty_list_renders_placeholder_header_only() {
        with_owner(|| {
            // `use_todos()` returns an empty Vec by default
            // (Signal::new(Vec::new()) seed via Owner::cache).
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let children = find_children_for_tag(&scene, LIST_TAG);
            assert_eq!(
                children.len(),
                1,
                "empty list has just the placeholder header (no item rows)",
            );
        });
    }

    #[test]
    fn r655_list_grows_with_signal_pushes() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push("milk".to_owned());
                next.push("eggs".to_owned());
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let children = find_children_for_tag(&scene, LIST_TAG);
            // header + 2 item rows
            assert_eq!(
                children.len(),
                3,
                "list children = header + N items (N=2)",
            );
        });
    }

    #[test]
    fn r655_list_items_carry_indexed_tags() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push("alpha".to_owned());
                next.push("beta".to_owned());
                next.push("gamma".to_owned());
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            // Per-item tag pattern `todo_item#<i>` mirrors
            // hello-listbox's `main_list#<i>` (R55.G.20 convention).
            for i in 0..3 {
                let needle = format!("todo_item#{i}");
                assert!(
                    scene.contains_tag(needle.as_str()),
                    "scene must carry {needle} for item {i}",
                );
            }
        });
    }

    fn find_tagged_container<'a>(
        s: &'a Scene,
        tag: &str,
        acc: &mut Option<&'a Scene>,
    ) {
        if acc.is_some() {
            return;
        }
        if let Scene::Container(c) = s
            && c.tag.as_deref() == Some(tag)
        {
            *acc = Some(s);
            return;
        }
        if let Scene::Container(c) = s {
            for child in &c.children {
                find_tagged_container(child, tag, acc);
            }
        }
    }

    #[test]
    fn r655_list_items_render_entry_text_in_order() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push("first".to_owned());
                next.push("second".to_owned());
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            // The list container holds the header + per-item rows;
            // walk the list region only (not the textfield section)
            // so the textfield's status line text doesn't leak in.
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present after push");
            let texts = collect_text_nodes(list);
            // Header text first, then entries in submission order.
            // Header now reads `Todos (2)`; entries follow.
            assert_eq!(texts.first().map(String::as_str), Some("Todos (2)"));
            assert!(
                texts.iter().any(|t| t == "first"),
                "list must render 'first' entry",
            );
            assert!(
                texts.iter().any(|t| t == "second"),
                "list must render 'second' entry",
            );
            // Order: 'first' must appear before 'second' in the
            // text-node walk (declaration order = paint order).
            let i_first = texts.iter().position(|t| t == "first").expect("first");
            let i_second = texts.iter().position(|t| t == "second").expect("second");
            assert!(
                i_first < i_second,
                "submission order preserved (first before second)",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // use_todos hook — Owner::cache dedup + Signal contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_use_todos_dedups_across_calls() {
        with_owner(|| {
            let a = use_todos();
            let b = use_todos();
            // Two calls in the same Owner scope must resolve to the
            // identical `Rc<Signal<Vec<String>>>` (Owner::cache key
            // dedup), so a mutation on one is visible to the other.
            assert!(
                std::rc::Rc::ptr_eq(&a, &b),
                "use_todos() must return the same Rc within an Owner scope",
            );
        });
    }

    #[test]
    fn r655_use_todos_set_with_appends_in_order() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push("a".to_owned());
                next
            });
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push("b".to_owned());
                next
            });
            let snapshot = todos.get();
            assert_eq!(
                snapshot,
                vec!["a".to_owned(), "b".to_owned()],
                "set_with closure semantics — sequential appends preserve order",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // build_todos_list — pure helper for the list region. Tests
    // exercise the helper directly with theme + entries so the
    // shape contract is pinned without going through the reactive
    // Owner cache.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_build_todos_list_empty_returns_placeholder_header() {
        let theme = Theme::light();
        let scene = build_todos_list(&theme, &[]);
        let texts = collect_text_nodes(&scene);
        assert_eq!(
            texts,
            vec!["No todos yet — type and press Enter".to_owned()],
            "empty list shows the placeholder hint",
        );
    }

    #[test]
    fn r655_build_todos_list_header_reflects_entry_count() {
        let theme = Theme::light();
        let scene = build_todos_list(
            &theme,
            &["x".to_owned(), "y".to_owned(), "z".to_owned()],
        );
        let texts = collect_text_nodes(&scene);
        assert_eq!(texts.first().map(String::as_str), Some("Todos (3)"));
        assert_eq!(texts.len(), 4, "header + 3 items");
    }
}
