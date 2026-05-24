//! `todomvc` — R655/R656 §5.16 first composed multi-widget application
//! verifying pinion's AI-native composition primitives end-to-end.
//!
//! ## Phase-2 application-tier entry
//!
//! Every prior `examples/hello-*` binding showcases **one** widget;
//! this binding is the first that composes **multiple** in a single
//! `WidgetView`: a [`TextFieldExternal`] input row at top, a
//! [`TodoDeleteExternal`] singleton handler (R656) registered via
//! [`WidgetCore::create_extra_externals`], and a dynamic
//! `Vec<TodoItem>` todo list rendered as a vertical column of
//! per-item rows (text label + delete X button) below the input.
//! The `TasteJS` `TodoMVC` spec is the canonical multi-widget
//! benchmark (CRUD + filter + persistence) — R655 landed the
//! scaffolding (input + Enter-to-submit + static list); R656 lands
//! stable per-item IDs + delete + per-item ARIA list/listitem
//! semantics; R657 hoists the textfield substrate into a renderer
//! helper; R658 toggle; R659 filter; R660 edit; R661 persistence
//! ([[r652-substrate-roi-matrix]] Phase-2 cascade plan).
//!
//! ## Architecture
//!
//! - State shape: `(TextFieldState, u32)` — interaction state +
//!   caret byte offset, inherited verbatim from hello-textfield.
//!   The textfield reactive text content lives on the
//!   [`TextEditState`] reached via [`use_text_edit_state`]`(TF_TAG)`,
//!   and the **todo list** lives on a separate
//!   `Signal<Vec<TodoItem>>` reached via [`use_todos`]`(TODOS_KEY)` —
//!   both reactive primitives are out-of-band from `Self::State`
//!   (which must be `Copy` per R51.173).
//! - Composition: the view fn returns a vertical
//!   [`Scene::Container`] holding `[title, field, status, list]`.
//!   The list child is itself a `Scene::Container` (tagged
//!   [`LIST_TAG`]) carrying one row per todo entry, where each row
//!   is a `Scene::Container` (tagged `todo_item#{id}` — stable u64
//!   id, NOT array index, per R656) holding `[text_label,
//!   delete_button]`. The delete button is a sub-`Scene::Container`
//!   tagged `todo_delete#{id}` whose paint-side hit-test routes
//!   through the existing R51.42 composite-tag wire (split into
//!   primary `todo_delete` + sub-index `<id>`) into
//!   [`TodoDeleteExternal::invoke`]`("send", Text("<id>:PointerDown"))`,
//!   which retains the [`Signal<Vec<TodoItem>>`] minus the matched
//!   id. No new framework substrate — R656 reuses the per-radio
//!   composite-tag pattern [`RadioGroupExternal`] already exercises
//!   per [[abstraction-needs-second-consumer]].
//! - Stable identity: each [`TodoItem`] carries a monotonic
//!   `u64` `id` allocated by the [`use_next_todo_id`] hook
//!   (`Owner::cache`-keyed `Cell<u64>`). The id survives sibling
//!   deletes — the surviving items keep their original tags
//!   `todo_item#7`, `todo_item#42`, `todo_item#88` rather than
//!   resequencing under a fresh `0`-based array index (which would
//!   make any in-flight RPC `scene/click {path:"todo_item#1"}`
//!   target a different logical row after a sibling delete). The
//!   `R656` AI-side verification script
//!   `tools/demos/todomvc_r656.py` pins this contract end-to-end.
//! - Submit wire: [`apply_key`](WidgetCore::apply_key) intercepts
//!   `"Enter"` BEFORE delegating to
//!   [`TextFieldExternal::invoke`]`("key", Text)`. On Enter, the
//!   binding reads `text_state.text()`, trims, allocates a fresh
//!   id via [`use_next_todo_id`], and (when non-empty) appends the
//!   resulting [`TodoItem`] to the `Signal<Vec<TodoItem>>` via
//!   `set_with` + clears the textfield via
//!   `text_state.set_text(String::new())`. Other keys fall through
//!   to the textfield's standard W3C key wire.
//! - Delete wire (mouse + RPC): the per-item delete button is a
//!   tagged `Scene::Container` (no [`External`]) — clicks land on
//!   the [`InputRouter`]'s tag hover-target, then `dispatch_send`
//!   forwards `PointerDown` to the singleton [`TodoDeleteExternal`]
//!   via the R51.42 composite-tag split. The RPC verification path
//!   uses `scene/click {path: "todo_delete#<id>"}` (same wire) or
//!   `scene/invoke {path: "/external/todo_delete", method: "delete",
//!   args: <id>}` (direct), and both routes converge on the same
//!   `set_with` mutation. ARIA: list root carries
//!   [`AriaRole::List`] (R656 §5.40); each item carries
//!   [`AriaRole::ListItem`] with the entry text as `AccessValue::Text`.
//!
//! ## Try it
//!
//! ```text
//! cargo run --release -p todomvc
//! ```
//!
//! Tab into the input → caret appears + blinks. Type "milk" →
//! `Enter` → "milk" appears in the list (with a red × delete
//! button on the right), field clears. Type "eggs" → `Enter` →
//! list has 2 entries. Click the × next to "milk" → only "eggs"
//! remains, and (per R656 stable-id contract) "eggs" still
//! carries its original `todo_item#{id}` tag — no resequencing.
//! Press `d` to disable the field, `e` to re-enable.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, IntrospectSchema,
    IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widget_core::ExtraExternal;
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
/// `Signal<Vec<TodoItem>>` carrying the todo entries. Symmetric with
/// the [`TF_TAG`] / [`use_text_edit_state`] convention — the
/// [`apply_key`] handler, the view fn, and [`TodoDeleteExternal`]
/// (R656) all resolve through this key, so the same
/// `Rc<Signal<Vec<TodoItem>>>` instance is shared and reactive
/// subscriptions land in the same store. R656 swapped the inner
/// element type from `String` to [`TodoItem`] (id + text) for stable
/// per-item identity under deletes.
const TODOS_KEY: &str = "todomvc.todos";

/// (R656 §5.16) [`Owner::cache`] key for the monotonic
/// `Rc<Cell<u64>>` counter that [`use_next_todo_id`] increments to
/// allocate the `id` field of each fresh [`TodoItem`]. Separate from
/// [`TODOS_KEY`] so the counter does not subscribe view-fn renders
/// to its mutations (a `Cell<u64>` is not reactive — only the
/// `Signal<Vec<TodoItem>>` it feeds is).
const NEXT_ID_KEY: &str = "todomvc.next_id";

/// (R656 §5.16) Singleton paint + state tag for the per-item delete
/// button handler. The paint scene emits one
/// `Scene::Container { tag: "todo_delete#<id>" }` per todo entry,
/// and the state scene holds exactly one [`TodoDeleteExternal`]
/// registered via [`create_extra_externals`] under the primary tag
/// `"todo_delete"`. The R51.42 §5.35 composite-tag wire splits the
/// paint tag on `#` so the [`InputRouter`] resolves the primary
/// against the state scene and forwards the sub-index `<id>` to
/// the External through the `invoke("send", "{id}:{Event}")`
/// channel — the canonical pinion-native pattern
/// [`RadioGroupExternal`] established at R51.43 and reused here at
/// the application tier without any new framework substrate.
const DELETE_TAG: &str = "todo_delete";

/// (R656 §5.16) Per-item paint tag prefix for the row container.
/// Full row tag is `"todo_item#<id>"` (stable u64 id, NOT array
/// index — R656 corrects the R655 index-based tagging that would
/// alias `todo_item#1` to a different logical row after a sibling
/// delete). Listed as a constant so RPC verify demos + tests
/// reference one source of truth.
const ITEM_TAG_PREFIX: &str = "todo_item";

/// (R656 §5.16) Per-item delete-button paint tag prefix. Full
/// per-item tag is `"todo_delete#<id>"`; the `#` separator triggers
/// the R51.42 composite-tag split so the [`InputRouter`] routes the
/// click into [`TodoDeleteExternal`] (registered under primary tag
/// [`DELETE_TAG`]).
const DELETE_TAG_PREFIX: &str = "todo_delete";

/// (R656 §5.16) Width of the per-item delete button in logical
/// pixels. Sized to comfortably host the `MULTIPLICATION_SIGN` (×,
/// U+00D7) glyph at [`DELETE_FONT_SIZE_PX`] with a few pixels of
/// padding on each side; large enough to satisfy the WCAG 2.5.5
/// "target size (minimum)" 24×24 CSS px recommendation for AAA.
const DELETE_BUTTON_W: u32 = 24;
/// (R656 §5.16) Height of the per-item delete button. Matches
/// [`DELETE_BUTTON_W`] for a square hit target — the most
/// forgiving shape for touch and mouse alike.
const DELETE_BUTTON_H: u32 = 24;
/// (R656 §5.16) Font size for the `×` delete glyph. 18 px gives the
/// glyph the same visual weight as the entry text below it without
/// dominating the row.
const DELETE_FONT_SIZE_PX: u32 = 18;

/// (R656 §5.16) WAI-AA WCAG 1.4.6 contrast-compliant tinting of
/// the `×` delete glyph. Resolves through [`ColorRole::Error`]
/// (R590 §5.50) so the destructive affordance reads with the same
/// hue the rest of the M3 palette reserves for error state — the
/// glyph is visually identified as "danger / destructive" without
/// the row needing a separate "are you sure?" confirmation step
/// (the per-item delete is a single-click destructive action by
/// `TasteJS` `TodoMVC` convention).
fn delete_glyph_color(theme: &Theme) -> Color {
    theme.resolve(ColorRole::Error)
}

/// (R656 §5.16) The visible `×` glyph itself (Unicode
/// `MULTIPLICATION_SIGN`, U+00D7). Pulled into a named constant
/// + `\u{...}` escape per `[[non-ascii-literal-named-const-escape]]`
///   so the source file stays ASCII-only.
const DELETE_GLYPH: &str = "\u{00D7}";

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

/// (R656 §5.16) Single todo entry — stable monotonic `id` allocated
/// by [`use_next_todo_id`] plus the user-typed `text` from the
/// input field at submit time. `Clone` so [`Signal::set_with`]
/// closures can construct the next `Vec<TodoItem>` snapshot without
/// touching the original (the framework's reactive equality-skip
/// path needs the previous snapshot intact for the cheap-`Eq`
/// compare). `PartialEq` so the `set_with` equality check ever
/// fires; `Debug` for the `Vec<TodoItem>` `Debug` chain the
/// `fmt_state_log` path walks for stderr.
///
/// The `id` field is `u64` rather than `usize` so the wire-form
/// `"todo_item#<id>"` paint tag and the
/// `invoke("send", "{id}:PointerDown")` payload stay
/// architecture-independent — every desktop/wasm/embedded backend
/// agrees on the integer representation. `u64::MAX` is a
/// practically unbounded id space (an app that adds one todo per
/// nanosecond for ~580 years would still not exhaust it), so the
/// counter never needs reuse — every deleted item's id stays
/// retired, and the AI client / RPC verify scripts can safely
/// remember per-id state across the lifetime of the process.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    /// Stable monotonic identifier — allocated once at submit time
    /// by [`use_next_todo_id`] and never reused. Survives sibling
    /// deletes (per R656 stable-id contract): if items
    /// `[#1, #2, #3]` exist and `#2` is deleted, the surviving
    /// items keep tags `todo_item#1` + `todo_item#3` (NOT
    /// resequenced to `#0` + `#1`).
    pub id: u64,
    /// User-typed entry text. Trimmed at submit time so
    /// leading/trailing whitespace doesn't leak into the rendered
    /// list (the trim guard in `apply_key` also drops blank-only
    /// submissions per the `TasteJS` `TodoMVC` spec).
    pub text: String,
}

/// (R655/R656 §5.16) `Owner::cache`-keyed hook returning the shared
/// `Rc<Signal<Vec<TodoItem>>>` of submitted todo entries. Symmetric
/// with [`use_text_edit_state`] / [`use_caret_blink`] hook shape —
/// the [`Owner::cache`] dedup guarantees one `Rc` across the view fn
/// (subscribes via `.get()` to re-run paint on submit/delete),
/// [`apply_key`] (mutates via `.set_with(|v| v.push(...))` on
/// Enter), [`create_extra_externals`] (constructs the singleton
/// [`TodoDeleteExternal`] with a clone of this `Rc`), and the
/// `access_node` hook (walks the current snapshot to emit one
/// `AccessNode` per item under the list root). Single-source-of-
/// truth for the todo list state; the signal's equality-skip
/// suppresses the re-run when a no-op `set_with` would not change
/// the snapshot.
///
/// R656 §5.16 — element type swapped from `String` to [`TodoItem`]
/// (id + text) for stable per-item identity under deletes.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope. Every
/// runtime invocation site (view fn, `apply_key`,
/// `create_extra_externals`, `access_node`) is wrapped by the
/// substrate's `root_owner.run`, so this only fires under unit
/// tests that forget the `with_owner` helper.
#[must_use]
pub fn use_todos() -> Rc<Signal<Vec<TodoItem>>> {
    Owner::current()
        .expect("use_todos requires an active Owner scope")
        .cache(TODOS_KEY, || Signal::new(Vec::<TodoItem>::new()))
}

/// (R656 §5.16) `Owner::cache`-keyed hook returning a monotonic
/// `Rc<Cell<u64>>` counter. The Enter-key submit handler invokes
/// [`Cell::get`] + [`Cell::set`] to allocate a fresh id for each
/// new [`TodoItem`]; the counter starts at `1` so deleted-then-
/// reused-tag-name confusion is impossible (id `0` is reserved as
/// a sentinel for "no item" by convention, though no current code
/// path relies on it — defensive carry).
///
/// Returns an `Rc<Cell<u64>>` (not just `u64`) so the call site
/// can both *read* the current counter (for tests, mostly) and
/// *advance* it through one shared instance. The hook is not
/// reactive — `Cell<u64>` does not implement the [`Signal`]
/// substrate, by design: id allocation is a one-way side effect
/// of the Enter handler, and view fns must not subscribe to it
/// (they re-render on `Signal<Vec<TodoItem>>` updates instead,
/// which already carries the freshly-allocated id inside the new
/// element).
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same
/// shape as [`use_todos`] — only test paths can trigger).
#[must_use]
pub fn use_next_todo_id() -> Rc<Cell<u64>> {
    Owner::current()
        .expect("use_next_todo_id requires an active Owner scope")
        .cache(NEXT_ID_KEY, || Cell::new(1_u64))
}

/// (R656 §5.16) Allocate and return the next fresh `id`. Convenience
/// wrapper over [`use_next_todo_id`] — fetches the counter, reads
/// the current value, advances by 1, and returns the pre-advance
/// snapshot so the caller can stamp it onto a fresh [`TodoItem`].
/// Saturating-add at `u64::MAX` is technically a defensive guard
/// (a single process would have to allocate ~580 years of
/// nanosecond-spaced ids to hit it) but explicit for textbook
/// integer-overflow hygiene.
#[must_use]
pub fn allocate_todo_id() -> u64 {
    let counter = use_next_todo_id();
    let current = counter.get();
    counter.set(current.saturating_add(1));
    current
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

    // R655/R656 §5.16 — todo list section: a header label + one
    // row per submitted entry, packed in a tagged
    // `Scene::Container` so RPC `scene/query` can address the list
    // independently from the textfield. Reading `todos.get()`
    // subscribes the view fn to the `Signal<Vec<TodoItem>>`, so a
    // `set_with` from the Enter handler OR from
    // [`TodoDeleteExternal::delete_by_id`] re-runs paint with the
    // new entries on the next frame. R656 swapped the inner element
    // type from `String` to [`TodoItem`] so each row carries a
    // stable u64 id under deletes.
    let todos = use_todos();
    let entries: Vec<TodoItem> = todos.get();
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

/// (R656 §5.16) Singleton per-item delete handler. Registered via
/// [`WidgetCore::create_extra_externals`] under primary tag
/// [`DELETE_TAG`] (`"todo_delete"`); the substrate composes the
/// state-scene root as
/// `Scene::Container([primary_textfield, this_handler])` per R55.D.5
/// §5.45. Paint-side per-item delete buttons tagged
/// `"todo_delete#<id>"` rely on the R51.42 §5.35 composite-tag wire:
/// the [`InputRouter`]'s `dispatch_send` splits the paint tag on
/// `#`, walks the state scene for an [`External`] whose primary tag
/// matches `"todo_delete"`, and forwards the sub-index `<id>` as
/// part of the wire-form `invoke("send", "{id}:{Event}")`. This
/// External parses the wire, narrows the sub-index back to `u64`,
/// and calls [`Signal::set_with`] to retain the surviving items.
///
/// One instance per binding (no per-item allocation) means
/// `create_extra_externals` runs exactly once at shell boot, which
/// is the only lifecycle anchor the R55.D.5 substrate exposes — the
/// id space is dynamic, but the handler that resolves ids to delete
/// actions is static, mirroring the
/// [`RadioGroupExternal`] / [`ListBoxExternal`] pattern where one
/// External owns N indexed children.
///
/// ## Why a separate External, not a [`WidgetCore::update`] reducer?
///
/// The R51.166 §5.23 R27 `update` reducer reads the cached
/// `Self::State` snapshot and returns `Vec<Command>` for async/IO
/// dispatch; it does NOT have direct access to the application's
/// `Signal<Vec<TodoItem>>` outside of opaque [`Command`] dispatch
/// to a registered [`Handler`]. The R656 delete contract is
/// synchronous-pure: a click on `todo_delete#<id>` must immediately
/// produce a `Vec<TodoItem>` mutation on the next paint cycle, no
/// async hop. The composite-tag → External → `set_with` path is the
/// pinion-canonical synchronous mutation route for paint-side hit
/// events, exactly the pattern hello-listbox uses for per-row
/// selection.
///
/// Future axes (R658 toggle, R660 edit) extend this same External
/// with additional `invoke` paths (`"toggle"`, `"edit"`, ...) so the
/// state scene still has exactly one extra External — keeping the
/// boot-time allocation footprint flat regardless of how many CRUD
/// affordances land.
#[derive(Debug)]
pub struct TodoDeleteExternal {
    /// Shared reference to the same `Rc<Signal<Vec<TodoItem>>>`
    /// the view fn / Enter handler / `access_node` hook resolve via
    /// [`use_todos`]. Mutations go through [`Signal::set_with`] so
    /// the framework's reactive equality-skip + view-fn re-run
    /// cascade fires automatically — no manual repaint plumbing.
    todos: Rc<Signal<Vec<TodoItem>>>,
}

impl TodoDeleteExternal {
    /// Construct a fresh handler bound to the supplied todo list
    /// signal. The caller (typically [`create_extra_externals`])
    /// resolves the signal via [`use_todos`] inside the framework's
    /// `root_owner.run` wrap so this `Rc` and the view fn's `Rc`
    /// are the same instance.
    #[must_use]
    pub fn new(todos: Rc<Signal<Vec<TodoItem>>>) -> Self {
        Self { todos }
    }

    /// Remove the entry whose `id` matches `target_id`. No-op when
    /// no such entry exists (idempotent: a double-click that
    /// triggers `PointerDown` then a stale RPC retry both converge on
    /// the same observed end-state). Uses
    /// [`Signal::set_with`] so the reactive equality-skip suppresses
    /// the view-fn re-run when the target id was already absent.
    fn delete_by_id(&self, target_id: u64) {
        self.todos.set_with(|prev| {
            let mut next = prev.clone();
            next.retain(|item| item.id != target_id);
            next
        });
    }

    /// Parse the R51.42 §5.35 composite-tag send payload
    /// `"<id>:<EventName>"` into `(id, event_name)`. Returns `None`
    /// when the wire-form is malformed (missing `:`, non-integer
    /// sub-index, or empty event name) — the dispatcher then yields
    /// [`InvokeError::Rejected`] to the caller. Matches the
    /// [`RadioGroupExternal`] R51.43 wire-format convention exactly
    /// so AI-side scripts that already know the composite-tag idiom
    /// (e.g. `tools/demos/hello_listbox_row_click.py`) don't need
    /// to learn a new shape for the R656 delete path.
    fn parse_send_payload(payload: &str) -> Option<(u64, &str)> {
        let (id_str, event_name) = payload.split_once(':')?;
        if event_name.is_empty() {
            return None;
        }
        let id: u64 = id_str.parse().ok()?;
        Some((id, event_name))
    }
}

impl External for TodoDeleteExternal {
    /// All three backends (GUI / TUI / RPC) supported — the External
    /// itself paints nothing (the view fn owns the delete-button
    /// glyph rendering), so the backend declaration is purely about
    /// which dispatch paths can deliver a delete event. The RPC
    /// path is the primary AI-driving surface; GUI is the mouse-
    /// click path; TUI is a placeholder for the R680+ TUI carry
    /// (no `examples/todomvc-tui` binding ships in R656). On
    /// unsupported backends the substrate skips this External
    /// rather than rejecting — a delete-less TUI variant is a
    /// degraded but functioning render, not a fatal scene-rejection
    /// case.
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    /// Framework drives repaint — this External produces no paint
    /// surface of its own; mutations propagate through the
    /// [`Signal<Vec<TodoItem>>`] subscription, which triggers the
    /// view fn re-run via the substrate's reactive paint loop.
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    /// UI-thread synchronous — the delete mutation lands on the same
    /// thread the [`Signal::set_with`] subscribers (view fn, IME
    /// caret rect, ARIA enrich) run on, so no cross-thread
    /// synchronisation is needed.
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt in to symbolic introspection so the RPC verify path can
    /// reach `query("count")` / `query("ids")` / `invoke("delete",
    /// Int(id))` for direct delete (parallel to the indirect
    /// `scene/click {path: "todo_delete#<id>"}` route).
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for TodoDeleteExternal {
    /// Read-only counters + the action surface. `count` and `ids`
    /// expose the same shape the view fn paints so AI scripts can
    /// cross-check the list state without walking the paint scene.
    /// The `send` slot documents the R51.42 wire form
    /// (`<id>:<EventName>`); the `delete` slot is the direct-form
    /// shortcut that takes a typed `Int(id)` and produces a typed
    /// `Bool(was_present)` return so the RPC verify scripts can
    /// distinguish "I deleted an existing item" from "no-op
    /// against an already-deleted id".
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("ids", "json"),
            ("send", "string"),
            ("delete", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => {
                let n = self.todos.get().len();
                Some(IntrospectValue::Int(
                    i64::try_from(n).expect("todo count must fit in i64"),
                ))
            }
            "ids" => {
                let snapshot = self.todos.get();
                let arr: Vec<serde_json::Value> = snapshot
                    .iter()
                    .map(|item| serde_json::Value::from(item.id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "count" | "ids" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R51.42 §5.35 — InputRouter's `dispatch_send` lands here
            // with payload `"<id>:PointerDown"` (or `PointerUp` /
            // `PointerEnter` / `PointerLeave` / `PointerCancel`).
            // R656 acts on `PointerDown` only — the X target is small
            // (24×24 px), drag-off cancel semantics are not needed,
            // and treating PointerDown as the single-shot delete
            // edge matches the canonical "destructive icon: press
            // commits" UX. PointerUp / Leave / Enter / Cancel are
            // accepted-and-ignored (return `Bool(false)`) so the
            // framework's substrate dispatch never sees a `Rejected`
            // for a routine paint cycle.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name) = Self::parse_send_payload(payload)
                        .ok_or(InvokeError::Rejected)?;
                    if event_name == "PointerDown" {
                        let was_present = self
                            .todos
                            .get()
                            .iter()
                            .any(|item| item.id == id);
                        self.delete_by_id(id);
                        Ok(IntrospectValue::Bool(was_present))
                    } else {
                        // R51.32 §5.15 — accepted but no-op for the
                        // non-PointerDown phases. `Bool(false)`
                        // signals "handled, no state change", which
                        // the InputRouter's dispatch loop reads as
                        // "do not re-route to a sibling".
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R656 §5.16 — direct delete by id. Skips the R51.42
            // composite-tag wire so AI scripts can call
            // `scene/invoke {path: "/external/todo_delete", method:
            // "delete", args: <id>}` without first having to
            // construct the `<id>:PointerDown` text payload. Returns
            // `Bool(was_present)` so the caller can distinguish
            // "deleted an existing entry" from "no-op against an
            // already-deleted id".
            "delete" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    let was_present = self
                        .todos
                        .get()
                        .iter()
                        .any(|item| item.id == id);
                    self.delete_by_id(id);
                    Ok(IntrospectValue::Bool(was_present))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// (R655/R656 §5.16) Build the todo list section `Scene::Container`:
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
fn build_todos_list(theme: &Theme, entries: &[TodoItem]) -> Scene {
    let header_style = TextStyle::new()
        .with_size_px(LIST_TITLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));
    let item_style = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let delete_style = TextStyle::new()
        .with_size_px(DELETE_FONT_SIZE_PX)
        .with_fg(delete_glyph_color(theme));

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

    // R656 §5.16 — each row tagged `todo_item#<id>` (stable u64 id,
    // NOT array index — the R655 index-based tagging would alias
    // `todo_item#1` to a different logical row after a sibling
    // delete, breaking any in-flight AI-side reference). The
    // per-row delete affordance is a sub-`Scene::Container` tagged
    // `todo_delete#<id>` that hosts the visible `×` glyph; the
    // R51.42 §5.35 composite-tag wire splits this paint tag at the
    // `#` so the [`InputRouter`] routes the click into the singleton
    // [`TodoDeleteExternal`] registered under primary tag
    // [`DELETE_TAG`] (`"todo_delete"`). Mirrors `hello-listbox`'s
    // `listbox_row(i, ...)` (R55.G.20) and `RadioGroupExternal`'s
    // composite-tag pattern (R51.43) — no new framework substrate
    // needed.
    let mut children: Vec<Scene> = Vec::with_capacity(entries.len() + 1);
    children.push(header);
    for item in entries {
        let row_tag = format!("{ITEM_TAG_PREFIX}#{}", item.id);
        let delete_tag = format!("{DELETE_TAG_PREFIX}#{}", item.id);

        let entry_text = Scene::Text(TextNode::styled(
            item.text.clone(),
            Rect::default(),
            item_style.clone(),
        ));

        // R656 §5.16 — the delete button is a tagged Container
        // hosting the `×` glyph centred inside the 24×24 hit-target.
        // No [`External`] per-item — the singleton
        // [`TodoDeleteExternal`] resolves the sub-index off the
        // composite tag at dispatch time. The button is NOT listed
        // in [`focusable_tags`] so a mouse click does not steal
        // focus from the text field (the user can press Enter
        // again immediately to add the next todo without a manual
        // refocus hop).
        let delete_glyph = Scene::Text(TextNode::styled(
            DELETE_GLYPH,
            Rect::default(),
            delete_style.clone(),
        ));
        let delete_button = Scene::Container(
            ContainerNode::new(vec![delete_glyph])
                .with_tag(delete_tag)
                // R51.69 §5.40 — accessible name carried as
                // `aria-label` so screen readers announce
                // "Delete <item text>" instead of just the glyph.
                // The static "Delete" label is intentional — the
                // per-item descriptive name lives on the parent
                // ListItem AccessNode via `enrich_names_from_scene`.
                .with_aria_label("Delete")
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(DELETE_BUTTON_W, DELETE_BUTTON_H)),
                ),
        );

        let row = Scene::Container(
            ContainerNode::new(vec![entry_text, delete_button])
                .with_tag(row_tag)
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::SpaceBetween)
                        .with_align_items(AlignItems::Center)
                        .with_gap(LIST_ITEM_GAP),
                ),
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

    /// (R656 §5.16 R55.D.5 §5.45) — register the singleton
    /// [`TodoDeleteExternal`] handler tagged [`DELETE_TAG`] alongside
    /// the primary textfield. The substrate wraps the call in
    /// `root_owner.run(...)` (per [[multi-external-substrate-extra-externals-pattern]])
    /// so [`use_todos`] resolves to the same
    /// `Rc<Signal<Vec<TodoItem>>>` the view fn and Enter handler
    /// later resolve through the same `Owner::cache` key. The
    /// state-scene root then becomes
    /// `Scene::Container([External(text_field), External(todo_delete)])`,
    /// and the existing `find_external_with_tag(TF_TAG)` read site
    /// stays shape-agnostic (R55.D.5 cascade lesson) — no change to
    /// [`Self::read_state`] needed.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let todos = use_todos();
        vec![ExtraExternal::new(
            DELETE_TAG,
            Box::new(TodoDeleteExternal::new(todos)),
        )]
    }

    /// (R55.D.5 §5.45) Multi-External binding (R656 adds the
    /// [`TodoDeleteExternal`] singleton alongside the primary
    /// textfield via [`Self::create_extra_externals`]).
    /// `find_external_with_tag` handles both the single-External
    /// and the multi-External shapes (R55.D.5 cascade lesson), so
    /// the read site stays shape-agnostic — the textfield's
    /// cached projection still resolves through `TF_TAG`, and the
    /// todo-delete handler's state lives entirely behind its own
    /// `Rc<Signal<Vec<TodoItem>>>` (not part of the cached
    /// `Self::State` snapshot, by design — the list is reactive
    /// and re-derives the rendered shape every paint).
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
                let entry_text = trimmed.to_owned();
                // R656 §5.16 — allocate a fresh stable u64 id from
                // the monotonic counter BEFORE pushing, so the
                // resulting `TodoItem` carries an id that no future
                // delete + re-add cycle can re-use. The id stays
                // bound to this entry for its entire lifetime in
                // the list, even if siblings come and go.
                let id = allocate_todo_id();
                let todos = use_todos();
                todos.set_with(|prev| {
                    let mut next = prev.clone();
                    next.push(TodoItem {
                        id,
                        text: entry_text.clone(),
                    });
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
    /// R56.1.b.1 / R656 §5.40 — multi-node a11y tree:
    ///
    /// 1. **Text input** (`TF_TAG`, [`AriaRole::TextInput`]): ARIA
    ///    `textbox` carrying the live text content as
    ///    [`AccessValue::Text`]. The [`AccessState::focused`] bit
    ///    tracks the actual focus owner so AT cursor follows the
    ///    field across Tab and `click_to_focus`.
    /// 2. **List root** ([`LIST_TAG`], [`AriaRole::List`]): WAI-ARIA
    ///    1.2 §5.3.5 `list` container. Owns the per-item children
    ///    via [`AccessNode::with_child`] in declaration order so AT
    ///    tools announce "item N of M" for the user's screen
    ///    reader. R656 §5.40 first consumer of the new
    ///    [`AriaRole::List`] variant.
    /// 3. **Per-item entry** (`todo_item#<id>`,
    ///    [`AriaRole::ListItem`]): one node per [`TodoItem`],
    ///    carrying the entry text as [`AccessValue::Text`] and the
    ///    stable u64 id as part of the tag. R656 §5.40 first
    ///    consumer of the new [`AriaRole::ListItem`] variant.
    /// 4. **Per-item delete button** (`todo_delete#<id>`,
    ///    [`AriaRole::Button`]): the destructive affordance, named
    ///    "Delete" via the paint-side `with_aria_label` so screen
    ///    readers announce "Delete button" + the parent item's
    ///    text. The button is NOT focusable through
    ///    [`Self::focusable_tags`] (the focus stays on the
    ///    textfield so the user can submit more entries) but AT can
    ///    still address it through the click action — keyboard-
    ///    only delete + Tab-walk-into-list is a R660+ carry per
    ///    [[abstraction-needs-second-consumer]].
    ///
    /// The (R56.1.b.1 substrate) `root_owner.run` wrap around
    /// `V::access_node` in `collect_access_emit_inputs` lets this
    /// hook reach the same `Rc<TextEditState>` and
    /// `Rc<Signal<Vec<TodoItem>>>` the view fn resolves through
    /// [`use_text_edit_state`] / [`use_todos`].
    ///
    /// The `name` field is populated by
    /// [`enrich_names_from_scene`](pinion_a11y::enrich_names_from_scene)
    /// against each container's `aria_label` override (set in
    /// `view` / `build_todos_list`) — the literal labels live in
    /// exactly one place.
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

        let mut nodes = vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::TextInput)
                .with_value(AccessValue::Text(text))
                .with_state(access_state),
        ];

        // R656 §5.40 — list root + items + delete buttons.
        let entries = use_todos().get();
        let mut list_node = AccessNode::new(LIST_TAG, AriaRole::List);
        for item in &entries {
            let row_tag = format!("{ITEM_TAG_PREFIX}#{}", item.id);
            let delete_tag = format!("{DELETE_TAG_PREFIX}#{}", item.id);
            list_node = list_node.with_child(row_tag.clone());
            nodes.push(
                AccessNode::new(row_tag, AriaRole::ListItem)
                    .with_value(AccessValue::Text(item.text.clone())),
            );
            // R656 §5.40 — delete button is a Button child of the
            // ListItem (semantically) but pinion's flat AccessNode
            // surface keeps siblings at the same level; the AT
            // tree builder ([`pinion_a11y::AccessTreeBuilder`])
            // resolves children via `with_child` references —
            // future work on parent/child wiring for nested-button
            // semantics is the R660+ a11y carry (no current AT
            // consumer requires it — screen readers walk Children
            // pointers anyway).
            nodes.push(AccessNode::new(delete_tag, AriaRole::Button));
        }
        // The list root must register every item as a child so the
        // AT cursor traversal order matches the visible paint
        // order — `with_child` records reference tags that the
        // tree builder resolves to NodeIds.
        nodes.push(list_node);

        nodes
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
    //! R655 / R656 §5.16 — todomvc-specific regression battery.
    //! Substrate correctness for the embedded `TextField` widget is
    //! covered by `hello-textfield`'s own tests (R56.1.b.1) — this
    //! module pins only the **composition** + **stable-id** layers
    //! this binding introduces: the todo list section presence,
    //! item-count → child-count invariant, per-item tag = stable id
    //! (R656), [`use_todos`] reactive hook dedup contract, the
    //! [`use_next_todo_id`] monotonic counter contract, and the
    //! [`TodoDeleteExternal`] composite-tag delete wire (parsed
    //! through both `invoke("send", "<id>:PointerDown")` and the
    //! direct `invoke("delete", Int(<id>))` paths). The R55.G.22
    //! paint-root tag convention is pinned via the framework fixture
    //! call.
    //!
    //! Note: the Enter handler itself runs against a `&mut Scene`
    //! that the runtime owns; testing it requires the shell's input
    //! loop. The handler's signal mutation is exercised indirectly
    //! here by manipulating the `Signal<Vec<TodoItem>>` directly
    //! under a private `Owner` and asserting the view-fn renders
    //! the new entries — the same observable surface the visible
    //! app depends on.
    use super::{
        allocate_todo_id, build_todos_list, use_next_todo_id, use_todos, view,
        TodoDeleteExternal, TodoItem, TodoMvcView, DELETE_GLYPH, DELETE_TAG,
        DELETE_TAG_PREFIX, ITEM_TAG_PREFIX, LIST_TAG, TF_TAG,
    };
    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::external::{External, IntrospectValue};
    use pinion_core::reactive::Owner;
    use pinion_core::theme::Theme;
    use pinion_core::widget_core::ExtraExternal;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene, WidgetCore};

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
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                });
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
    fn r656_list_items_carry_stable_id_tags() {
        with_owner(|| {
            // R656 — per-item paint tag is `todo_item#<id>` (stable
            // u64), NOT `todo_item#<array_index>`. Use non-sequential
            // ids to lock in the "id, not index" contract — `7 / 42
            // / 99` are visibly distinct from `0 / 1 / 2`.
            let todos = use_todos();
            let ids = [7_u64, 42, 99];
            let names = ["alpha", "beta", "gamma"];
            todos.set_with(|prev| {
                let mut next = prev.clone();
                for (id, name) in ids.iter().zip(names.iter()) {
                    next.push(TodoItem {
                        id: *id,
                        text: (*name).to_owned(),
                    });
                }
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            for id in &ids {
                let needle = format!("{ITEM_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(needle.as_str()),
                    "scene must carry {needle} for stable id {id}",
                );
                let delete_needle = format!("{DELETE_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(delete_needle.as_str()),
                    "scene must carry {delete_needle} delete-button tag for stable id {id}",
                );
            }
            // R656 — confirm the OLD R655 index-based tags are NOT
            // present (the contract migration is complete, no dual-
            // namespacing of `todo_item#0` + `todo_item#7` for the
            // same logical item).
            for idx in 0..3 {
                let stale = format!("{ITEM_TAG_PREFIX}#{idx}");
                assert!(
                    !scene.contains_tag(stale.as_str()),
                    "R655 index-based tag {stale} must NOT survive the R656 migration",
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
                next.push(TodoItem {
                    id: 1,
                    text: "first".to_owned(),
                });
                next.push(TodoItem {
                    id: 2,
                    text: "second".to_owned(),
                });
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
            // Header now reads `Todos (2)`; entries follow. R656 —
            // each row also paints a `DELETE_GLYPH` `\u{00D7}` (×)
            // text node, so the per-row text walk yields
            // `[entry_text, "×"]` in declaration order.
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
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                });
                next
            });
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                });
                next
            });
            let snapshot = todos.get();
            assert_eq!(
                snapshot,
                vec![
                    TodoItem {
                        id: 1,
                        text: "a".to_owned()
                    },
                    TodoItem {
                        id: 2,
                        text: "b".to_owned()
                    },
                ],
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
    fn r656_build_todos_list_header_reflects_entry_count() {
        let theme = Theme::light();
        let scene = build_todos_list(
            &theme,
            &[
                TodoItem {
                    id: 10,
                    text: "x".to_owned(),
                },
                TodoItem {
                    id: 20,
                    text: "y".to_owned(),
                },
                TodoItem {
                    id: 30,
                    text: "z".to_owned(),
                },
            ],
        );
        let texts = collect_text_nodes(&scene);
        assert_eq!(texts.first().map(String::as_str), Some("Todos (3)"));
        // R656 — text walk = header + per-row(entry_text + DELETE_GLYPH)
        // = 1 + 3 * 2 = 7. The R655 contract (1 + 3 = 4) was strictly
        // text-node count; R656's delete glyph adds the second text per
        // row.
        assert_eq!(
            texts.len(),
            7,
            "R656: header + 3 * (entry + delete glyph) = 7 text nodes",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — stable id contract: per-item tag survives sibling delete
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_stable_id_survives_sibling_delete() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "alpha".to_owned(),
                });
                next.push(TodoItem {
                    id: 2,
                    text: "beta".to_owned(),
                });
                next.push(TodoItem {
                    id: 3,
                    text: "gamma".to_owned(),
                });
                next
            });
            // Delete the middle item (id=2).
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.retain(|item| item.id != 2);
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            // Surviving items keep their ORIGINAL stable ids — no
            // resequencing to {0,1}. This is the R656 invariant.
            assert!(
                scene.contains_tag("todo_item#1"),
                "alpha (id=1) survives with its original tag",
            );
            assert!(
                !scene.contains_tag("todo_item#2"),
                "beta (id=2) is removed (tag gone)",
            );
            assert!(
                scene.contains_tag("todo_item#3"),
                "gamma (id=3) survives with its original tag",
            );
            // The list count drops from 3 to 2; the header reflects.
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            assert_eq!(
                texts.first().map(String::as_str),
                Some("Todos (2)"),
                "header reflects post-delete count",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — use_next_todo_id / allocate_todo_id monotonic contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_allocate_todo_id_is_monotonic() {
        with_owner(|| {
            let a = allocate_todo_id();
            let b = allocate_todo_id();
            let c = allocate_todo_id();
            assert!(a < b && b < c, "ids must be strictly increasing");
        });
    }

    #[test]
    fn r656_use_next_todo_id_dedups_across_calls() {
        with_owner(|| {
            let a = use_next_todo_id();
            let b = use_next_todo_id();
            assert!(
                std::rc::Rc::ptr_eq(&a, &b),
                "use_next_todo_id must return the same Rc within an Owner scope",
            );
        });
    }

    #[test]
    fn r656_allocate_does_not_collide_with_existing_ids() {
        // Worked scenario from `apply_key`'s Enter handler: allocate
        // ids monotonically through `allocate_todo_id`, push into
        // `use_todos`, and assert every per-item id is unique.
        with_owner(|| {
            let todos = use_todos();
            for text in ["one", "two", "three", "four"] {
                let id = allocate_todo_id();
                todos.set_with(|prev| {
                    let mut next = prev.clone();
                    next.push(TodoItem {
                        id,
                        text: text.to_owned(),
                    });
                    next
                });
            }
            let snapshot = todos.get();
            let mut ids: Vec<u64> = snapshot.iter().map(|i| i.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(
                ids.len(),
                snapshot.len(),
                "every allocated id must be unique",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — TodoDeleteExternal: composite-tag wire + direct invoke
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_delete_external_send_pointerdown_removes_item() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "alpha".to_owned(),
                });
                next.push(TodoItem {
                    id: 22,
                    text: "beta".to_owned(),
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("11:PointerDown".to_owned()))
                .expect("PointerDown for id=11 must succeed");
            // R51.42 wire — Bool(was_present) reports whether the
            // delete observed an existing id.
            assert_eq!(result, IntrospectValue::Bool(true));
            let snapshot = use_todos().get();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, 22);
            assert_eq!(snapshot[0].text, "beta");
        });
    }

    #[test]
    fn r656_delete_external_send_pointerup_is_no_op() {
        // PointerUp / Enter / Leave / Cancel are accepted-but-ignored
        // (Bool(false)) so the InputRouter's normal dispatch cycle
        // does not see a Rejected for routine paint events.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "stays".to_owned(),
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("5:PointerUp".to_owned()))
                .expect("PointerUp accepted as no-op");
            assert_eq!(result, IntrospectValue::Bool(false));
            // Item remains — PointerUp is ignored.
            assert_eq!(use_todos().get().len(), 1);
        });
    }

    #[test]
    fn r656_delete_external_direct_invoke_path() {
        // Direct RPC route: `scene/invoke {path: "/external/todo_delete",
        // method: "delete", args: <id>}` reaches `invoke("delete",
        // Int(id))` without going through the composite-tag wire.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 100,
                    text: "first".to_owned(),
                });
                next.push(TodoItem {
                    id: 200,
                    text: "second".to_owned(),
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("delete", IntrospectValue::Int(100))
                .expect("direct delete must succeed");
            assert_eq!(result, IntrospectValue::Bool(true));
            let snapshot = use_todos().get();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, 200);
        });
    }

    #[test]
    fn r656_delete_external_unknown_id_is_idempotent() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "alone".to_owned(),
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("delete", IntrospectValue::Int(999))
                .expect("delete of unknown id returns Ok(Bool(false))");
            // R656 — was_present = false because id=999 never existed.
            assert_eq!(result, IntrospectValue::Bool(false));
            // The surviving item is unaffected.
            assert_eq!(use_todos().get().len(), 1);
        });
    }

    #[test]
    fn r656_delete_external_query_count_matches_signal() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                });
                next
            });
            let handler = TodoDeleteExternal::new(use_todos());
            let count = handler
                .introspect()
                .expect("introspect wired")
                .query("count")
                .expect("count slot exposed");
            assert_eq!(count, IntrospectValue::Int(3));
        });
    }

    #[test]
    fn r656_delete_external_query_ids_returns_json_array() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "seven".to_owned(),
                });
                next.push(TodoItem {
                    id: 42,
                    text: "answer".to_owned(),
                });
                next
            });
            let handler = TodoDeleteExternal::new(use_todos());
            let ids = handler
                .introspect()
                .expect("introspect wired")
                .query("ids")
                .expect("ids slot exposed");
            // Snapshot ordering matches insertion order.
            assert_eq!(
                ids,
                IntrospectValue::Json(serde_json::json!([7, 42])),
            );
        });
    }

    #[test]
    fn r656_delete_external_malformed_send_payload_rejected() {
        with_owner(|| {
            let mut handler = TodoDeleteExternal::new(use_todos());
            // Missing colon → Rejected.
            let no_colon = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("noseparator".to_owned()));
            assert!(no_colon.is_err());
            // Non-integer sub-index → Rejected.
            let bad_id = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("xx:PointerDown".to_owned()));
            assert!(bad_id.is_err());
            // Empty event name → Rejected.
            let no_event = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("1:".to_owned()));
            assert!(no_event.is_err());
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — WidgetCore::create_extra_externals registers the singleton
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_create_extra_externals_registers_todo_delete() {
        with_owner(|| {
            let extras: Vec<ExtraExternal> =
                <TodoMvcView as WidgetCore>::create_extra_externals();
            assert_eq!(extras.len(), 1, "exactly one extra External");
            assert_eq!(extras[0].tag, DELETE_TAG);
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 §5.40 — WidgetA11y::access_node emits List + ListItem + Button
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_access_node_emits_list_root_with_no_items() {
        with_owner(|| {
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // [textbox, list]
            assert_eq!(nodes.len(), 2);
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[1].role, AriaRole::List);
            assert_eq!(nodes[1].tag, LIST_TAG);
            assert!(nodes[1].children.is_empty(), "empty list, no children");
        });
    }

    #[test]
    fn r656_access_node_emits_listitem_and_button_per_entry() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                });
                next
            });
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // Expected order: [textbox, listitem#1, button(delete#1),
            // listitem#2, button(delete#2), list_root]
            assert_eq!(nodes.len(), 6);
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[1].role, AriaRole::ListItem);
            assert_eq!(nodes[1].tag, "todo_item#1");
            assert_eq!(nodes[2].role, AriaRole::Button);
            assert_eq!(nodes[2].tag, "todo_delete#1");
            assert_eq!(nodes[3].role, AriaRole::ListItem);
            assert_eq!(nodes[3].tag, "todo_item#2");
            assert_eq!(nodes[4].role, AriaRole::Button);
            assert_eq!(nodes[4].tag, "todo_delete#2");
            // list root references both items by tag.
            assert_eq!(nodes[5].role, AriaRole::List);
            assert_eq!(
                nodes[5].children,
                vec!["todo_item#1".to_owned(), "todo_item#2".to_owned()],
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — DELETE_GLYPH appears in the per-row text walk
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_delete_glyph_appears_per_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "row".to_owned(),
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            // header + entry + × glyph
            assert!(
                texts.iter().any(|t| t == DELETE_GLYPH),
                "per-row × glyph present",
            );
        });
    }
}
