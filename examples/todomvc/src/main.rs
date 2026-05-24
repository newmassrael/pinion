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

use std::cell::Cell;
use std::rc::Rc;

use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, IntrospectSchema,
    IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
// R657 §5.16 §5.38 — lifted TextField paint substrate shared with
// hello-textfield (2nd consumer reached per
// [[abstraction-needs-second-consumer]]).
use pinion_widget_paint::text_field as tf_paint;

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

/// (R658 §5.16) Singleton paint + state tag for the per-item toggle
/// (completed flag) handler. Mirrors [`DELETE_TAG`] — registered via
/// [`WidgetCore::create_extra_externals`] as the 2nd sibling
/// [`ExtraExternal`] under primary tag `"todo_toggle"`. The
/// per-row paint tag is `"todo_toggle#<id>"`; the R51.42 §5.35
/// composite-tag wire splits on `#` so the [`InputRouter`] resolves
/// the primary against [`TodoToggleExternal`] and forwards the
/// sub-index `<id>` as part of `invoke("send", "{id}:{Event}")`.
/// 2nd consumer of the multi-External `create_extra_externals`
/// substrate ([[multi-external-substrate-extra-externals-pattern]],
/// R55.D.5 listbox + scrollbar was 1st, R658 todomvc toggle + delete
/// is 2nd).
const TOGGLE_TAG: &str = "todo_toggle";

/// (R658 §5.16) Per-item toggle-button paint tag prefix. Full
/// per-item tag is `"todo_toggle#<id>"`. Mirrors [`DELETE_TAG_PREFIX`].
const TOGGLE_TAG_PREFIX: &str = "todo_toggle";

/// (R658 §5.16) Width of the per-item toggle button. Matches the
/// 24×24 WCAG 2.5.5 AAA target-size recommendation [`DELETE_BUTTON_W`]
/// uses for the destructive affordance, so the two side buttons sit
/// symmetrical at the row edges.
const TOGGLE_BUTTON_W: u32 = 24;
/// (R658 §5.16) Height of the per-item toggle button. Square hit
/// target, symmetric with [`DELETE_BUTTON_H`].
const TOGGLE_BUTTON_H: u32 = 24;
/// (R658 §5.16) Font size for the `☐` / `☑` toggle glyphs. 18 px to
/// match the destructive [`DELETE_FONT_SIZE_PX`] glyph weight so the
/// two row affordances read with equal visual prominence.
const TOGGLE_FONT_SIZE_PX: u32 = 18;

/// (R658 §5.16) Unchecked toggle glyph (Unicode `BALLOT BOX`,
/// U+2610). Pulled into a named const + `\u{...}` escape per
/// `[[non-ascii-literal-named-const-escape]]` so the source file
/// stays ASCII-only.
const TOGGLE_GLYPH_UNCHECKED: &str = "\u{2610}";

/// (R658 §5.16) Checked toggle glyph (Unicode
/// `BALLOT BOX WITH CHECK`, U+2611). Single-codepoint canonical for
/// "completed todo" used by the `TasteJS` `TodoMVC` HTML reference,
/// by `macOS` Reminders, and by GitHub Markdown task lists — Unicode
/// parley/swash shaping draws this without needing a custom SVG
/// asset.
const TOGGLE_GLYPH_CHECKED: &str = "\u{2611}";

/// (R658 §5.16) [`Owner::cache`] key for the reactive [`ScrollState`]
/// owning the todo list's vertical scroll offset + max-y bound.
/// Mirror of `hello-listbox`'s `SCROLL_KEY = "main_list_scroll"`
/// shape — the [`use_scroll_state`] hook resolves through this key
/// inside the view fn, and the runtime's [`compute_layout`] pass
/// writes the laid-out max-y back through the same `Rc<ScrollState>`
/// on the next frame.
const LIST_SCROLL_KEY: &str = "todomvc.list_scroll";

/// (R658 §5.16) Vertical viewport height for the todo list scroll
/// container. Sized so 5 fully-visible rows fit (each ~30 px tall at
/// 14 px font + 6 px gap → 36 px row total × 5 = 180 px) plus the
/// "Todos (N)" header (~24 px) plus a few pixels of breathing
/// room — the 6th-onward row scrolls beneath the visible window edge
/// via wheel / Arrow keys / future scrollbar drag (R55.D.4 substrate
/// inherited via [`use_scroll_state`], reused per
/// [[abstraction-needs-second-consumer]]).
const LIST_VIEWPORT_H: u32 = 220;
/// (R658 §5.16) Horizontal viewport width — same width as the text
/// input field above so the list and the field visually align (the
/// `tf_paint::view_field` substrate paints a 360-px-wide field, so
/// 360 px gives a flush column).
const LIST_VIEWPORT_W: u32 = 360;

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
// field + status ≈ 120 px) plus the [`LIST_VIEWPORT_H`] todo list
// region, mirroring the macOS / iOS Reminders-app vertical rhythm.
// R658 §5.16 — the list region is now a [`ScrollNode`] of fixed
// viewport height [`LIST_VIEWPORT_H`]; rows past the visible window
// scroll via wheel / Arrow / future scrollbar drag rather than
// causing the outer window flex to overflow. The WIN_H constant
// (R655 carry magic) is therefore now `title_section + gap +
// LIST_VIEWPORT_H + padding` rather than an unbounded growth budget.
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
    /// (R658 §5.16) Completion flag. Defaults to `false` on submit;
    /// flipped by [`TodoToggleExternal`] in response to a click on
    /// the per-row `todo_toggle#<id>` button (paint-side composite-
    /// tag wire) OR the direct `invoke("toggle", Int(id))` RPC
    /// route. When `true`, the per-row paint substitutes the
    /// `☐` (U+2610) glyph with `☑` (U+2611) and the entry text
    /// renders through [`ColorRole::OnSurfaceMuted`] to signal
    /// "done" without obstructing readability (no strikethrough
    /// today — text-decoration is a R663+ framework primitive
    /// candidate per [[abstraction-needs-second-consumer]]).
    pub completed: bool,
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

// R657 §5.16 §5.38 — TextField paint sizing + alpha tuning live in
// `tf_paint::TextFieldStyle::m3_filled` in pinion-widget-paint.
// The binding uses the M3 default for the input field.

/// Title label font size (kept binding-local — it's part of the
/// surrounding chrome composition, not the field substrate).
const TITLE_FONT_SIZE_PX: u32 = 18;
const STATUS_FONT_SIZE_PX: u32 = 12;

// Gap between title / field / status line in the root column flex —
// matches the macOS / iOS settings-pane vertical rhythm (~16 px
// between related controls).
const ROW_GAP: u32 = 16;

// R657 §5.16 §5.38 — use_layout_cache lifted to
// `pinion_widget_paint::text_field::use_text_field_layout_cache`
// (private impl detail of `view_field` + `ime_caret_rect_for`).

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

// R657 §5.16 §5.38 — saturating_f32_to_u32 lifted to
// `pinion_widget_paint::text_field` (private helper used by
// `view_field` + `ime_caret_rect_for`).

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

    // R657 §5.16 §5.38 — TextField paint composition lifted to
    // `tf_paint::view_field`. The binding's view fn composes only
    // its surrounding chrome (title + status + todo list +
    // delete-X) around the lifted field substrate.
    //
    // (R57.X.textfield §5.50) Active palette — `use_theme` auto-
    // subscribes this view-fn so a `ThemeProvider::set_theme` from
    // anywhere in the application re-runs the view + repaints the
    // field + caret + selection band + delete glyphs. R586 §5.50
    // `theme_animated` opts in to the R57.X.theme-fade cross-fade.
    let theme = use_theme(THEME_TAG).theme_animated();

    let field = tf_paint::view_field(
        TF_TAG,
        interaction,
        caret_byte,
        &theme,
        &tf_paint::TextFieldStyle::m3_filled(),
        "Text input",
    );

    // R657 — status line still reads the reactive TextEditState
    // directly so the AI side can verify composition lifecycle
    // through the visible status row. The field paint already
    // walked it through `tf_paint::view_field`; both subscriptions
    // land on the same `Rc<TextEditState>` per Owner::cache dedup.
    let text_state = use_text_edit_state(TF_TAG);
    let text = text_state.text();
    let preedit = text_state.preedit();

    let title = Scene::Text(TextNode::styled(
        "TextField",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
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
        tf_paint::text_field_state_name(interaction),
        caret_byte,
        text,
        preedit_status,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    // R655/R656 §5.16 — todo list section: a header label + one
    // row per submitted entry, packed in a tagged
    // `Scene::Container` so RPC `scene/query` can address the list
    // independently from the textfield. Reading `todos.get()`
    // subscribes the view fn to the `Signal<Vec<TodoItem>>`, so a
    // `set_with` from the Enter handler OR from
    // [`TodoDeleteExternal::delete_by_id`] (R656) OR from
    // [`TodoToggleExternal::toggle_by_id`] (R658) re-runs paint with
    // the new entries on the next frame. R656 swapped the inner
    // element type from `String` to [`TodoItem`] so each row carries
    // a stable u64 id under deletes; R658 added `completed: bool`.
    //
    // R658 §5.16 — the list region is wrapped in a [`ScrollNode`]
    // anchored on the shared `Rc<ScrollState>` (key
    // [`LIST_SCROLL_KEY`]) so 6+ entries scroll smoothly within the
    // fixed [`LIST_VIEWPORT_H`] window instead of pushing the outer
    // window flex past [`WIN_H`]. The substrate (R55.A / R55.B /
    // R55.G.5 layout pass) writes the `max_y` bound automatically
    // from the laid-out content — no manual `set_max` plumbing in
    // the binding. 2nd consumer of the [`use_scroll_state`] hook
    // beyond `hello-listbox` (R51.190 / R55.G), so per
    // [[abstraction-needs-second-consumer]] the substrate stays as
    // is; if a 3rd consumer surfaces a common "list-with-scroll"
    // shape, the wrapper lifts to a `view_scroll_list` helper.
    let todos = use_todos();
    let entries: Vec<TodoItem> = todos.get();
    let todos_list_content = build_todos_list(&theme, &entries);

    let scroll_state = use_scroll_state(LIST_SCROLL_KEY);
    let todos_scroll = Scene::Scroll(ScrollNode::from_state(
        scroll_state,
        Rect::new(0, 0, LIST_VIEWPORT_W, LIST_VIEWPORT_H),
        todos_list_content,
    ));

    Scene::Container(
        ContainerNode::new(vec![title, field, status, todos_scroll])
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

/// (R658 §5.16) Singleton per-item toggle (completed-flag) handler.
/// Registered via [`WidgetCore::create_extra_externals`] under
/// primary tag [`TOGGLE_TAG`] (`"todo_toggle"`) as the **2nd**
/// sibling [`ExtraExternal`] alongside [`TodoDeleteExternal`]. The
/// state-scene root composes as
/// `Scene::Container([primary_textfield, todo_delete, todo_toggle])`
/// per R55.D.5 §5.45 — multi-External lookup walks
/// [`Scene::find_external_with_tag`] so primary / extras read-side
/// stays shape-agnostic.
///
/// 2nd consumer of `create_extra_externals` at the framework level
/// — `hello-listbox` (R55.D.5) registered the listbox + scrollbar
/// pair as the 1st consumer; the R658 todomvc binding is the 2nd
/// (`todo_delete` + `todo_toggle` pair). The
/// [[multi-external-substrate-extra-externals-pattern]] memory
/// stays valid without any substrate change because the R55.D.5
/// substrate already supports an arbitrary `Vec<ExtraExternal>`.
///
/// Wire form — identical to [`TodoDeleteExternal`]:
/// - Paint scene emits `Scene::Container { tag: "todo_toggle#<id>" }`
///   per row (paint-side composite tag).
/// - [`InputRouter`]'s `dispatch_send` splits the tag on `#` and
///   forwards the sub-index `<id>` as part of `invoke("send",
///   "<id>:PointerDown")`.
/// - The direct RPC route `scene/invoke {path: "/external/todo_toggle",
///   method: "toggle", args: <id>}` reaches `invoke("toggle",
///   Int(<id>))` and produces the same `Signal::set_with` mutation.
///
/// Mutation contract: `PointerDown` / direct `toggle` invocation
/// **flips** `completed` (not "set to true") so a 2nd click on a
/// completed row returns it to active. `PointerUp` / `PointerEnter`
/// / `PointerLeave` / `PointerCancel` are accepted-as-no-op
/// (Bool(false)) so the `InputRouter`'s standard dispatch cycle
/// does not see `Rejected` for routine paint events — same
/// convention as the destructive [`TodoDeleteExternal::invoke`]
/// path.
#[derive(Debug)]
pub struct TodoToggleExternal {
    /// Shared `Rc<Signal<Vec<TodoItem>>>` — the same instance
    /// [`TodoDeleteExternal::todos`] holds. Both Externals register
    /// inside the framework's `root_owner.run(...)` wrap from
    /// [`create_extra_externals`] so [`use_todos`] dedups against
    /// the same `Owner::cache` slot.
    todos: Rc<Signal<Vec<TodoItem>>>,
}

impl TodoToggleExternal {
    /// Construct a fresh handler bound to the supplied todo list
    /// signal. Symmetric with [`TodoDeleteExternal::new`].
    #[must_use]
    pub fn new(todos: Rc<Signal<Vec<TodoItem>>>) -> Self {
        Self { todos }
    }

    /// (R658 §5.16) Flip the `completed` flag of the entry whose
    /// `id` matches `target_id`. No-op when no such entry exists
    /// (idempotent: a double-click that triggers two `PointerDown`s
    /// before the next paint cycle redraws the row reverts to the
    /// pre-click state, exactly the W3C "click toggles" UX
    /// convention — for a R658 demo this is the textbook minimum;
    /// if a future round wires up "debounce within frame" the call
    /// site stays unchanged). Uses [`Signal::set_with`] so the
    /// reactive equality-skip suppresses the view-fn re-run when
    /// no entry matched (rare edge case where AI client sends a
    /// stale id — silent absorbtion is the safe default).
    fn toggle_by_id(&self, target_id: u64) {
        self.todos.set_with(|prev| {
            prev.iter()
                .map(|item| {
                    if item.id == target_id {
                        TodoItem {
                            completed: !item.completed,
                            ..item.clone()
                        }
                    } else {
                        item.clone()
                    }
                })
                .collect()
        });
    }

    /// Mirror of [`TodoDeleteExternal::parse_send_payload`] — kept as
    /// a private helper rather than a shared free fn because R658 is
    /// the **2nd** consumer of the composite-tag parse idiom and
    /// `[[abstraction-needs-second-consumer]]` reads the substantive
    /// 5-LOC body as **not yet** a substrate lift candidate (a 3rd
    /// consumer — R660+ edit / a 4th composed app — would trigger
    /// the lift into `pinion_core::composite_tag::parse_send_payload`).
    fn parse_send_payload(payload: &str) -> Option<(u64, &str)> {
        let (id_str, event_name) = payload.split_once(':')?;
        if event_name.is_empty() {
            return None;
        }
        let id: u64 = id_str.parse().ok()?;
        Some((id, event_name))
    }
}

impl External for TodoToggleExternal {
    /// All three backends supported — paint is owned by the view fn
    /// (the per-row checkbox glyph), so backend declaration is
    /// purely about dispatch surface. Mirror of
    /// [`TodoDeleteExternal::backends`].
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    /// Framework drives repaint — toggling `completed` mutates the
    /// shared `Rc<Signal<Vec<TodoItem>>>`, which auto-subscribes the
    /// view-fn for the next paint cycle.
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    /// UI-thread sync — mutations land on the same thread as the
    /// view-fn / IME caret / ARIA enrich subscribers.
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

impl ExternalIntrospect for TodoToggleExternal {
    /// Read-only counters + the toggle action surface:
    /// - `count`: total entry count (mirror of
    ///   [`TodoDeleteExternal`]'s `count`).
    /// - `completed_count`: number of entries whose `completed` flag
    ///   is `true` — convenience for AI clients verifying derived
    ///   state without walking the JSON `ids` array.
    /// - `ids_completed`: JSON array of ids whose `completed` is
    ///   true.
    /// - `send`: R51.42 wire form `"<id>:<EventName>"`.
    /// - `toggle`: direct typed `Int(id)` → `Bool(new_completed)`
    ///   route, parallel to `delete`'s direct `Int(id)` shape.
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("completed_count", "int"),
            ("ids_completed", "json"),
            ("send", "string"),
            ("toggle", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let snapshot = self.todos.get();
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(snapshot.len()).expect("todo count must fit in i64"),
            )),
            "completed_count" => {
                let n = snapshot.iter().filter(|i| i.completed).count();
                Some(IntrospectValue::Int(
                    i64::try_from(n).expect("completed count must fit in i64"),
                ))
            }
            "ids_completed" => {
                let arr: Vec<serde_json::Value> = snapshot
                    .iter()
                    .filter(|i| i.completed)
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
            "count" | "completed_count" | "ids_completed" => {
                Err(InterveneError::ReadOnly)
            }
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
            // with payload `"<id>:PointerDown"`. PointerDown commits
            // the toggle (single-shot flip); PointerUp / Enter /
            // Leave / Cancel return `Bool(false)` so the dispatch
            // loop sees "handled, no state change" and never
            // `Rejected`s a routine paint event. Same convention as
            // [`TodoDeleteExternal::invoke`].
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
                        self.toggle_by_id(id);
                        // R658 — Bool(was_present) reports whether
                        // the toggle observed an existing id (so the
                        // AI client can distinguish "I flipped an
                        // existing item" from "no-op against an
                        // unknown id" without an extra `query`
                        // round-trip).
                        Ok(IntrospectValue::Bool(was_present))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R658 §5.16 — direct toggle by id. Returns the
            // **post-toggle** `completed` value as `Bool(...)` so the
            // AI client can confirm the new state in one
            // round-trip (rather than re-querying `ids_completed`).
            // Unknown ids return `Bool(false)` — semantically
            // "no flip happened" without distinguishing "unknown id"
            // vs "id existed and is now false". For the rare case
            // the caller needs the distinction, `query("count")` /
            // `query("ids_completed")` already expose enough state.
            "toggle" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    self.toggle_by_id(id);
                    let post_completed = self
                        .todos
                        .get()
                        .iter()
                        .any(|item| item.id == id && item.completed);
                    Ok(IntrospectValue::Bool(post_completed))
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
#[allow(
    clippy::too_many_lines,
    reason = "single-purpose list builder — splitting per-row construction into a helper hurts locality without reducing complexity (R658 §5.16 toggle + entry + delete trio composes one row)"
)]
fn build_todos_list(theme: &Theme, entries: &[TodoItem]) -> Scene {
    let header_style = TextStyle::new()
        .with_size_px(LIST_TITLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));
    // (R658 §5.16) Active vs completed text colour split. The active
    // ramp lifts the text to `OnSurface` (full contrast — "to do");
    // the completed ramp drops to `OnSurfaceMuted` so finished
    // entries visually recede without a strikethrough decoration
    // (text-decoration substrate is a R663+ candidate per
    // [[abstraction-needs-second-consumer]] — no 2nd consumer wants
    // strikethrough yet, so a muted-fade affordance carries the
    // "done" semantic alone). Both styles are pre-built outside the
    // loop so per-row construction stays a single `.clone()` call.
    let item_style_active = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let item_style_completed = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));
    let delete_style = TextStyle::new()
        .with_size_px(DELETE_FONT_SIZE_PX)
        .with_fg(delete_glyph_color(theme));
    // (R658 §5.16) Toggle glyph colour ramp — matches the entry
    // text on the same row so the row reads as a single visual
    // unit. The W3C ARIA "checkbox" widget convention leaves colour
    // modulation to CSS; pinion's design-token equivalent is the
    // `ColorRole` palette, so `OnSurface` vs `OnSurfaceMuted`
    // parallels the M3 checked vs unchecked tint without inventing
    // a new role just for this binding.
    let toggle_style_active = TextStyle::new()
        .with_size_px(TOGGLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let toggle_style_completed = TextStyle::new()
        .with_size_px(TOGGLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));

    // (R658 §5.16) Header text reflects the completed count when at
    // least one entry is completed (mirrors the TasteJS TodoMVC
    // "<N> items left" footer; we condense to a single line). The
    // R655/R656 empty-list placeholder + plain-count header
    // (`Todos (N)`) stay unchanged when nothing is completed.
    let header_text = if entries.is_empty() {
        String::from("No todos yet — type and press Enter")
    } else {
        let completed = entries.iter().filter(|i| i.completed).count();
        if completed == 0 {
            format!("Todos ({})", entries.len())
        } else {
            format!(
                "Todos ({} of {} completed)",
                completed,
                entries.len(),
            )
        }
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
        let toggle_tag = format!("{TOGGLE_TAG_PREFIX}#{}", item.id);

        // (R658 §5.16) Toggle button — left-side `☐`/`☑` glyph
        // mirroring the right-side `×` delete button's hit-target +
        // composite-tag wire shape. The chosen glyph reflects the
        // current `completed` state: U+2610 (BALLOT BOX) for active,
        // U+2611 (BALLOT BOX WITH CHECK) for completed.
        let (toggle_glyph_str, toggle_style_for_row) = if item.completed {
            (TOGGLE_GLYPH_CHECKED, toggle_style_completed.clone())
        } else {
            (TOGGLE_GLYPH_UNCHECKED, toggle_style_active.clone())
        };
        let toggle_glyph = Scene::Text(TextNode::styled(
            toggle_glyph_str,
            Rect::default(),
            toggle_style_for_row,
        ));
        let toggle_button = Scene::Container(
            ContainerNode::new(vec![toggle_glyph])
                .with_tag(toggle_tag)
                // (R658 §5.16) Static "Toggle complete" aria-label
                // so screen readers announce a stable affordance
                // name. The dynamic checked / unchecked state lives
                // on the per-item AriaRole::CheckBox AccessNode
                // emitted by [`access_node`] (R658 §5.40), which is
                // the W3C-canonical place for state in `aria-checked`.
                .with_aria_label("Toggle complete")
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(TOGGLE_BUTTON_W, TOGGLE_BUTTON_H)),
                ),
        );

        let item_style_for_row = if item.completed {
            item_style_completed.clone()
        } else {
            item_style_active.clone()
        };
        let entry_text = Scene::Text(TextNode::styled(
            item.text.clone(),
            Rect::default(),
            item_style_for_row,
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

        // R658 §5.16 — row children become
        // `[toggle_button, entry_text, delete_button]` with
        // `JustifyContent::SpaceBetween`: toggle pins to the left
        // edge, delete pins to the right, the entry text floats
        // between them. The row's `todo_item#<id>` outer tag stays
        // the catch-all for clicks that land on the text (which
        // produce no External dispatch today — R660+ edit candidate
        // per [[abstraction-needs-second-consumer]]).
        let row = Scene::Container(
            ContainerNode::new(vec![toggle_button, entry_text, delete_button])
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
        // R658 §5.16 — two sibling Externals share the same
        // `Rc<Signal<Vec<TodoItem>>>` (delete + toggle). The
        // R55.D.5 substrate composes the state-scene root as
        // `Scene::Container([primary_textfield, todo_delete,
        // todo_toggle])`; multi-External lookup walks
        // `Scene::find_external_with_tag` so the existing read /
        // dispatch sites stay shape-agnostic. 2nd consumer of
        // multi-External `create_extra_externals` at the framework
        // level (`hello-listbox` listbox + scrollbar = 1st), per
        // [[multi-external-substrate-extra-externals-pattern]].
        vec![
            ExtraExternal::new(
                DELETE_TAG,
                Box::new(TodoDeleteExternal::new(todos.clone())),
            ),
            ExtraExternal::new(
                TOGGLE_TAG,
                Box::new(TodoToggleExternal::new(todos)),
            ),
        ]
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
        // R657 §5.16 §5.38 — delegate to the lifted helper so
        // hello-textfield + todomvc share one read-state seam. The
        // R55.D.5 single-vs-multi External shape is handled
        // transparently by `find_external_with_tag`.
        tf_paint::read_text_field_state(scene, TF_TAG)
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
                    // R658 §5.16 — every freshly submitted entry
                    // starts active (`completed: false`). The user
                    // toggles to completed via the per-row `☐`/`☑`
                    // button OR an AI client via the direct
                    // `invoke("toggle", Int(id))` RPC path.
                    next.push(TodoItem {
                        id,
                        text: entry_text.clone(),
                        completed: false,
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
            tf_paint::text_field_state_name(state.0),
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
        // R658 §5.40 — per-row also emits an AriaRole::CheckBox
        // node for the toggle button with `aria-checked` reflecting
        // the entry's `completed` field. The W3C WAI-ARIA 1.2
        // canonical mapping for a "checkbox" widget puts the
        // checked/unchecked state on `aria-checked`, not on any
        // visible glyph — screen readers read this announcement
        // independent of the row's text.
        let entries = use_todos().get();
        let mut list_node = AccessNode::new(LIST_TAG, AriaRole::List);
        for item in &entries {
            let row_tag = format!("{ITEM_TAG_PREFIX}#{}", item.id);
            let delete_tag = format!("{DELETE_TAG_PREFIX}#{}", item.id);
            let toggle_tag = format!("{TOGGLE_TAG_PREFIX}#{}", item.id);
            list_node = list_node.with_child(row_tag.clone());
            nodes.push(
                AccessNode::new(row_tag, AriaRole::ListItem)
                    .with_value(AccessValue::Text(item.text.clone())),
            );
            // R658 §5.40 — checkbox node carries `aria-checked` via
            // `AccessState::checked = Some(<bool>)`. The
            // `enrich_names_from_scene` pass picks up the static
            // "Toggle complete" aria-label from the paint scene; the
            // dynamic state lives here (W3C-canonical split).
            nodes.push(
                AccessNode::new(toggle_tag, AriaRole::CheckBox).with_state(AccessState {
                    focused: false,
                    disabled: false,
                    hovered: false,
                    pressed: false,
                    checked: Some(item.completed),
                }),
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
    fn ime_caret_rect(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(TF_TAG) {
            return None;
        }
        let (interaction, caret_byte) = *state;
        // R657 §5.16 §5.38 — field rect walk stays binding-side; the
        // caret composition (splice + LayoutCache lookup + window-
        // coord sum) is the lifted helper.
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
}

// R657 §5.16 §5.38 — parse_text_field_state / text_field_state_name
// lifted to `pinion_widget_paint::text_field` and reached via
// `tf_paint::parse_text_field_state` / `tf_paint::text_field_state_name`.

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
        TodoDeleteExternal, TodoItem, TodoMvcView, TodoToggleExternal, DELETE_GLYPH,
        DELETE_TAG, DELETE_TAG_PREFIX, ITEM_TAG_PREFIX, LIST_SCROLL_KEY, LIST_TAG,
        TF_TAG, TOGGLE_GLYPH_CHECKED, TOGGLE_GLYPH_UNCHECKED, TOGGLE_TAG,
        TOGGLE_TAG_PREFIX,
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                    completed: false,
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
                        completed: false,
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
        // R658 §5.16 — recurse through `Scene::Scroll` content (the
        // todo list is now wrapped in a ScrollNode so the tagged
        // container lives inside `scroll.content`, not at the
        // top-level Container layer).
        if let Scene::Scroll(sc) = s {
            find_tagged_container(&sc.content, tag, acc);
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "second".to_owned(),
                    completed: false,
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
                    completed: false,
                });
                next
            });
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next
            });
            let snapshot = todos.get();
            assert_eq!(
                snapshot,
                vec![
                    TodoItem {
                        id: 1,
                        text: "a".to_owned(),
                        completed: false,
                    },
                    TodoItem {
                        id: 2,
                        text: "b".to_owned(),
                        completed: false,
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
                    completed: false,
                },
                TodoItem {
                    id: 20,
                    text: "y".to_owned(),
                    completed: false,
                },
                TodoItem {
                    id: 30,
                    text: "z".to_owned(),
                    completed: false,
                },
            ],
        );
        let texts = collect_text_nodes(&scene);
        assert_eq!(texts.first().map(String::as_str), Some("Todos (3)"));
        // R658 — text walk = header + per-row(toggle_glyph +
        // entry_text + delete_glyph) = 1 + 3 * 3 = 10. R656 was
        // header + per-row(entry + delete) = 1 + 3*2 = 7; the R658
        // toggle glyph adds one more text node per row.
        assert_eq!(
            texts.len(),
            10,
            "R658: header + 3 * (toggle + entry + delete glyph) = 10 text nodes",
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "gamma".to_owned(),
                    completed: false,
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
                        completed: false,
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "beta".to_owned(),
                    completed: false,
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
                    completed: false,
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 200,
                    text: "second".to_owned(),
                    completed: false,
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
                    completed: false,
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: false,
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 42,
                    text: "answer".to_owned(),
                    completed: false,
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
        // R658 §5.16 — extras = [TodoDeleteExternal, TodoToggleExternal]
        // (2 entries). DELETE_TAG still registered, TOGGLE_TAG added
        // alongside per the R55.D.5 multi-External composition. The
        // R656 invariant (DELETE_TAG present) survives — the test
        // now asserts on the tag membership, not on a hard length of
        // one.
        with_owner(|| {
            let extras: Vec<ExtraExternal> =
                <TodoMvcView as WidgetCore>::create_extra_externals();
            assert_eq!(
                extras.len(),
                2,
                "R658: exactly two extras (delete + toggle)",
            );
            let tags: Vec<&str> = extras.iter().map(|e| e.tag).collect();
            assert!(tags.contains(&DELETE_TAG), "DELETE_TAG still registered");
            assert!(tags.contains(&TOGGLE_TAG), "R658 TOGGLE_TAG registered");
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
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                    completed: false,
                });
                next
            });
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // R658 §5.40 — expected order:
            // [textbox,
            //  listitem#1, checkbox(toggle#1), button(delete#1),
            //  listitem#2, checkbox(toggle#2), button(delete#2),
            //  list_root]
            // = 1 + 2 * 3 + 1 = 8. R656 was 1 + 2*2 + 1 = 6;
            // R658 inserts one CheckBox node per row between the
            // ListItem and its Button (toggle is left of the entry
            // text in the visible paint order; AT cursor traversal
            // mirrors visible order).
            assert_eq!(nodes.len(), 8, "R658: 1 + 2 * 3 + 1 = 8 nodes");
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[1].role, AriaRole::ListItem);
            assert_eq!(nodes[1].tag, "todo_item#1");
            assert_eq!(nodes[2].role, AriaRole::CheckBox);
            assert_eq!(nodes[2].tag, "todo_toggle#1");
            assert_eq!(
                nodes[2].state.checked,
                Some(false),
                "R658: aria-checked starts false for fresh entries",
            );
            assert_eq!(nodes[3].role, AriaRole::Button);
            assert_eq!(nodes[3].tag, "todo_delete#1");
            assert_eq!(nodes[4].role, AriaRole::ListItem);
            assert_eq!(nodes[4].tag, "todo_item#2");
            assert_eq!(nodes[5].role, AriaRole::CheckBox);
            assert_eq!(nodes[5].tag, "todo_toggle#2");
            assert_eq!(nodes[5].state.checked, Some(false));
            assert_eq!(nodes[6].role, AriaRole::Button);
            assert_eq!(nodes[6].tag, "todo_delete#2");
            // list root references both items by tag.
            assert_eq!(nodes[7].role, AriaRole::List);
            assert_eq!(
                nodes[7].children,
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
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            // header + toggle glyph + entry + × glyph
            assert!(
                texts.iter().any(|t| t == DELETE_GLYPH),
                "per-row × glyph present",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — TodoItem.completed migration + toggle glyph
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_fresh_todo_item_starts_active() {
        // The Enter handler stamps `completed: false` on freshly
        // pushed entries (mirrored in `r656_allocate_does_not_collide`
        // setup). Confirm directly via a `TodoItem` ctor: the field
        // is a plain `bool` with no derived default, so a Clone/Debug
        // round-trip preserves the explicit value.
        let item = TodoItem {
            id: 1,
            text: "fresh".to_owned(),
            completed: false,
        };
        assert!(!item.completed, "R658: fresh todo defaults to active");
        let toggled = TodoItem {
            completed: !item.completed,
            ..item.clone()
        };
        assert!(toggled.completed, "R658: flip flips the bool");
        assert_eq!(toggled.id, item.id, "id stays stable under toggle");
        assert_eq!(toggled.text, item.text, "text stays stable under toggle");
    }

    #[test]
    fn r658_unchecked_toggle_glyph_appears_per_active_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present (inside Scroll)");
            let texts = collect_text_nodes(list);
            assert!(
                texts.iter().any(|t| t == TOGGLE_GLYPH_UNCHECKED),
                "R658: active row paints unchecked U+2610 glyph",
            );
            assert!(
                !texts.iter().any(|t| t == TOGGLE_GLYPH_CHECKED),
                "R658: active row must NOT paint checked U+2611",
            );
        });
    }

    #[test]
    fn r658_checked_toggle_glyph_appears_per_completed_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: true,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present (inside Scroll)");
            let texts = collect_text_nodes(list);
            assert!(
                texts.iter().any(|t| t == TOGGLE_GLYPH_CHECKED),
                "R658: completed row paints checked U+2611 glyph",
            );
            assert!(
                !texts.iter().any(|t| t == TOGGLE_GLYPH_UNCHECKED),
                "R658: completed row must NOT paint unchecked U+2610",
            );
        });
    }

    #[test]
    fn r658_view_carries_toggle_tag_per_item() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "x".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "y".to_owned(),
                    completed: true,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            for id in [11_u64, 22] {
                let needle = format!("{TOGGLE_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(needle.as_str()),
                    "R658: scene must carry {needle} per-item toggle tag",
                );
            }
        });
    }

    #[test]
    fn r658_completed_header_reflects_progress() {
        // Header text is `"Todos (N)"` when no entry is completed
        // and `"Todos (X of N completed)"` when at least one is.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: true,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            assert_eq!(
                texts.first().map(String::as_str),
                Some("Todos (1 of 3 completed)"),
                "R658: progress header reflects completed count",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — TodoToggleExternal: composite-tag + direct invoke
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_toggle_external_send_pointerdown_flips_completed() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "alpha".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke(
                    "send",
                    IntrospectValue::Text("11:PointerDown".to_owned()),
                )
                .expect("PointerDown for id=11 must succeed");
            assert_eq!(
                result,
                IntrospectValue::Bool(true),
                "send/PointerDown returns Bool(was_present)",
            );
            let snapshot = use_todos().get();
            // id=11 flipped, id=22 untouched. Stable count.
            assert_eq!(snapshot.len(), 2);
            assert!(
                snapshot.iter().find(|i| i.id == 11).unwrap().completed,
                "id=11 toggled to completed=true",
            );
            assert!(
                !snapshot.iter().find(|i| i.id == 22).unwrap().completed,
                "id=22 untouched (sibling completed flag preserved)",
            );
        });
    }

    #[test]
    fn r658_toggle_external_send_double_flips_back() {
        // Two consecutive PointerDowns flip the completed flag on +
        // off — the toggle is monoidal, NOT idempotent (a 2nd click
        // un-completes the entry).
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "flip".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerDown".to_owned()),
                )
                .unwrap();
            assert!(use_todos().get()[0].completed, "1st flip → true");
            handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerDown".to_owned()),
                )
                .unwrap();
            assert!(
                !use_todos().get()[0].completed,
                "2nd flip → false (monoidal toggle, NOT idempotent)",
            );
        });
    }

    #[test]
    fn r658_toggle_external_send_pointerup_is_no_op() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "stays".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerUp".to_owned()),
                )
                .unwrap();
            assert_eq!(
                result,
                IntrospectValue::Bool(false),
                "PointerUp accepted as no-op (no Rejected)",
            );
            assert!(
                !use_todos().get()[0].completed,
                "PointerUp does NOT flip completed",
            );
        });
    }

    #[test]
    fn r658_toggle_external_direct_invoke_returns_post_state() {
        // Direct `invoke("toggle", Int(id))` returns the **post-flip**
        // completed bool (parallel to delete's `was_present`, but for
        // toggle the post-state is the AI-useful value).
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 100,
                    text: "x".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(100))
                .unwrap();
            assert_eq!(
                result,
                IntrospectValue::Bool(true),
                "1st toggle of id=100 returns Bool(true) post-flip",
            );
            let result2 = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(100))
                .unwrap();
            assert_eq!(
                result2,
                IntrospectValue::Bool(false),
                "2nd toggle of id=100 returns Bool(false) post-flip",
            );
        });
    }

    #[test]
    fn r658_toggle_external_unknown_id_is_silent() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "alone".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(999))
                .unwrap();
            // Unknown id → Bool(false) (no flip happened, item not
            // found). The existing entry is unaffected.
            assert_eq!(result, IntrospectValue::Bool(false));
            assert!(!use_todos().get()[0].completed);
        });
    }

    #[test]
    fn r658_toggle_external_query_completed_count() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: true,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: true,
                });
                next
            });
            let handler = TodoToggleExternal::new(use_todos());
            let intro = handler.introspect().unwrap();
            assert_eq!(
                intro.query("count").unwrap(),
                IntrospectValue::Int(3),
                "count slot mirrors total entries",
            );
            assert_eq!(
                intro.query("completed_count").unwrap(),
                IntrospectValue::Int(2),
                "completed_count slot mirrors completed entries",
            );
            // ids_completed lists only completed ids in declaration order.
            assert_eq!(
                intro.query("ids_completed").unwrap(),
                IntrospectValue::Json(serde_json::json!([1, 3])),
                "ids_completed JSON array preserves insertion order",
            );
        });
    }

    #[test]
    fn r658_toggle_external_malformed_send_rejected() {
        with_owner(|| {
            let mut handler = TodoToggleExternal::new(use_todos());
            // Missing colon → Rejected.
            let no_colon = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("noseparator".to_owned()));
            assert!(no_colon.is_err());
            // Non-integer sub-index → Rejected.
            let bad_id = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("xx:PointerDown".to_owned()));
            assert!(bad_id.is_err());
            // Empty event name → Rejected.
            let no_event = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("1:".to_owned()));
            assert!(no_event.is_err());
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — ScrollNode wrap (WIN_H magic cleanup)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_view_wraps_list_in_scroll_node() {
        // R658 — the LIST_TAG container must live inside a
        // Scene::Scroll wrapper (WIN_H magic cleanup). Walk the
        // top-level outer Container's children and confirm at least
        // one Scene::Scroll wraps a container carrying the
        // LIST_SCROLL_KEY-derived tag.
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0), &Frame::default());
            let mut found_scroll_with_list = false;
            if let Scene::Container(outer) = &scene {
                for child in &outer.children {
                    if let Scene::Scroll(sn) = child {
                        if sn.content.contains_tag(LIST_TAG) {
                            found_scroll_with_list = true;
                            // The Scroll node carries the
                            // LIST_SCROLL_KEY as its derived tag (via
                            // ScrollNode::from_state ← ScrollState::tag).
                            assert_eq!(
                                sn.tag.as_deref(),
                                Some(LIST_SCROLL_KEY),
                                "R658: ScrollNode tag derived from LIST_SCROLL_KEY",
                            );
                        }
                    }
                }
            }
            assert!(
                found_scroll_with_list,
                "R658: list region wrapped in Scene::Scroll(...) carrying LIST_TAG inside",
            );
        });
    }

    #[test]
    fn r658_scroll_state_persists_across_view_calls() {
        with_owner(|| {
            // First view call instantiates ScrollState in the Owner
            // cache.
            let _ = view((TextFieldState::Idle, 0), &Frame::default());
            let scroll_state_a =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            let scroll_state_b =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            assert!(
                std::rc::Rc::ptr_eq(&scroll_state_a, &scroll_state_b),
                "R658: use_scroll_state(LIST_SCROLL_KEY) dedups across calls",
            );
        });
    }

    #[test]
    fn r658_scroll_state_offset_round_trips() {
        with_owner(|| {
            let _ = view((TextFieldState::Idle, 0), &Frame::default());
            let scroll_state =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            // Declare a max bound + scroll. The view fn's
            // ScrollNode::from_state reads offset() on the next paint
            // so the value flows out to the rendered geometry.
            scroll_state.set_max(0, 200);
            scroll_state.scroll_to(0, 80);
            assert_eq!(
                scroll_state.offset(),
                (0, 80),
                "R658: scroll_to round-trips into offset()",
            );
        });
    }
}
