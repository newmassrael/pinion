//! `hello-async-data` — R923 §5.22 §5.23 §2: the first framework consumer
//! that renders a **view over an out-of-memory, page-fetched data source**.
//!
//! ## Why this exists (the genuine Model/View-at-scale frontier)
//!
//! Every prior data widget (`hello-virtual-list`, the data-grids, the
//! tree-grid outliner) renders rows that are **already resident in memory** —
//! the framework had never once rendered a view whose row data arrives
//! *asynchronously*, a page at a time, with a `Loading` placeholder in
//! between. That is the backbone the self-hosted editor's asset browser /
//! scene outliner needs (10k+ assets streamed from disk / a database /
//! the network, never all resident). R923 builds the first consumer of that
//! capability and proves it is AI-introspectable end to end.
//!
//! ## What this demonstrates
//!
//! A paged **asset browser**: `TOTAL_ASSETS` assets across `TOTAL_PAGES`
//! pages of `PAGE_SIZE` rows. Only the *current* page is ever materialised
//! (the scripted source synthesises a page's rows on demand — pages 2..N are
//! not resident until fetched). **Prev / Next / Reload** drive navigation;
//! each page transition fetches the new page asynchronously, surfacing the
//! three [`ResourceState`] arms as scene-as-data (§2 #7):
//!
//! - **Loading** — "Loading page N of M…", a skeleton placeholder in the list.
//! - **Ready** — the page's rows + "Page N of M — K assets".
//! - **Error** — page `ERROR_PAGE` is scripted-unavailable (a real source has
//!   unreadable pages), surfacing "Page N of M — error: …".
//!
//! ## Architecture (unidirectional, async-via-Resource, Effect-driven refetch)
//!
//! The data layer is declared **reactively**: a `page` [`Signal`] + a
//! `reload_nonce` [`Signal`] are the query parameters; an [`Effect`]
//! subscribed to both calls [`Resource::fetch_with`] whenever either changes,
//! feeding a deterministic deferred future into the owner-scoped
//! [`LocalTaskPump`](pinion_core::LocalTaskPump) the shell polls each frame
//! (R761.1). This is exactly the "auto-refetch on dep change, wired by
//! `Effect`" evolution the §5.22 `Resource` module doc anticipated.
//!
//! Navigation is unidirectional: three [`ButtonExternal`]s emit `"<tag>.click"`
//! intents; [`AsyncDataView::update`] only mutates the `page` / `reload_nonce`
//! Signals — it never fetches directly. The View merely *reads*
//! `resource.state()` (pure, §6.3). Boot fetches page 0 through the Effect's
//! eager initial run (installed once in `create_extra_externals`, preserving
//! view-fn purity).
//!
//! ## Latency model (ZERO-FLAKE)
//!
//! Fetch latency is a deterministic [`DeferredReady`] future — `Pending`
//! `FETCH_LATENCY_POLLS` times, then `Ready` — never a wall-clock `sleep`.
//! The `Loading` arm therefore stays observable for a fixed, generous poll
//! window, so `wait_query`/`wait_until` reliably catches `Loading` before the
//! terminal `Ready`/`Error` (the same deterministic deferred-future SSOT the
//! scripted file dialog uses; same discipline as the R761.1 deferred dialog).
//!
//! ## Verification
//!
//! `tools/demos/r923_async_data.py`: boot → page 1 rows; Next → Loading →
//! page 2; Next → `ERROR_PAGE` → Loading → Error; Next → page 4; Next → page 5
//! (Next disabled at the end); Reload → Loading → same page (nonce refetch);
//! Prev → page 4; a11y list / listitem / status / button roles throughout.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{DeferredReady, Effect, Owner, Resource, ResourceState, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore, use_local_task_pump};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, read_button_state,
};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloAsyncDataRenderer, HelloAsyncDataRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 380;
const THEME_TAG: &str = "app";

/// Rows per page.
const PAGE_SIZE: usize = 6;
/// Total pages in the (scripted) dataset.
const TOTAL_PAGES: usize = 5;
/// Total assets — only the current page is ever resident.
const TOTAL_ASSETS: usize = PAGE_SIZE * TOTAL_PAGES;
/// 0-based index of the page the scripted source cannot serve, so the
/// `Error` arm is exercised in the natural navigation flow ("Page 3 of 5").
const ERROR_PAGE: usize = 2;
/// `Pending` polls before a fetch resolves — a deterministic stand-in for
/// real source latency (disk / DB / network) that keeps `Loading` observable
/// across frames without a wall-clock sleep (ZERO-FLAKE). Each
/// `scene/snapshot from=paint` advances the shell-polled `LocalTaskPump` one
/// step (the poll lives in `compute_paint_scene_internal`), so the demo's own
/// snapshot polling drives `Loading → Ready` deterministically — never a
/// wall-clock race. Matches the R761.1 deferred-dialog poll count.
const FETCH_LATENCY_POLLS: u32 = 30;

const PREV_TAG: &str = "async_prev";
const NEXT_TAG: &str = "async_next";
const RELOAD_TAG: &str = "async_reload";
const STATUS_TAG: &str = "pager_status";
const LIST_TAG: &str = "asset_list";
/// Tag for the in-list placeholder/error line (stable across states).
const LIST_NOTE_TAG: &str = "asset_list_note";

const PREV_HOVER_KEY: &str = "prev_hover";
const NEXT_HOVER_KEY: &str = "next_hover";
const RELOAD_HOVER_KEY: &str = "reload_hover";

/// `Owner::cache` key for the current `page` index `Signal`.
const PAGE_KEY: &str = "async_data.page";
/// `Owner::cache` key for the reload-nonce `Signal` (a changing dep that
/// re-fires the refetch Effect for the *same* page).
const NONCE_KEY: &str = "async_data.reload_nonce";
/// `Owner::cache` key for the async page `Resource`.
const RESULT_KEY: &str = "async_data.result";
/// `Owner::cache` key for the lifetime-held auto-refetch `Effect` marker.
const REFETCH_KEY: &str = "async_data.refetch";

/// Asset kinds + file extensions, rotated per row so each page looks like a
/// real heterogeneous asset directory.
const KINDS: [(&str, &str); 6] = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
];

/// One row of the (out-of-memory) dataset. Derives serde because a
/// [`Resource`] payload must be (de)serializable for the §5.22
/// snapshot/restore contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AssetRow {
    name: String,
    kind: String,
    size_kb: u32,
}

/// The human-readable descriptor for a row — the single SSOT shared by the
/// painted [`TextNode`] and the `listitem` accessible name (so screen + AT
/// can never drift, per R886.1 / R922.1).
fn row_label(row: &AssetRow) -> String {
    format!("{} ({}, {} KB)", row.name, row.kind, row.size_kb)
}

/// Stable per-row tag (`asset_row_<global_index>`) shared by the painted row
/// container and its `listitem` `AccessNode`, so AT bounds attach to the paint.
fn row_tag(global_index: usize) -> String {
    format!("asset_row_{global_index}")
}

// ─── scripted out-of-memory data source ───────────────────────────────────

/// Synthesise page `page`'s rows behind a deterministic fetch latency. Only
/// the requested page is ever materialised — pages 2..N are not resident
/// until fetched (the out-of-memory contract). `ERROR_PAGE` is
/// scripted-unavailable. The latency carrier is the shared
/// [`DeferredReady`] SSOT (the same deterministic
/// `Pending`-countdown future the scripted file dialog uses, R923 lift), so
/// the source has no hand-rolled equivalent of its own.
fn fetch_page(page: usize) -> DeferredReady<Result<Vec<AssetRow>, String>> {
    let outcome = if page == ERROR_PAGE {
        Err(format!("source unavailable for page {}", page + 1))
    } else {
        Ok(page_rows(page))
    };
    DeferredReady::new(FETCH_LATENCY_POLLS, outcome)
}

/// The rows that *would* live on page `page` of the backing store.
fn page_rows(page: usize) -> Vec<AssetRow> {
    let base = page * PAGE_SIZE;
    (0..PAGE_SIZE)
        .map(|r| {
            let i = base + r;
            let (kind, ext) = KINDS[i % KINDS.len()];
            let size = (i * 37 + 11) % 900 + 8;
            AssetRow {
                name: format!("asset_{i:03}.{ext}"),
                kind: kind.to_owned(),
                size_kb: u32::try_from(size).unwrap_or(0),
            }
        })
        .collect()
}

// ─── reactive data layer (page Signal + nonce + Resource + refetch Effect) ─

/// The current page index `Signal` (0-based). Navigation mutates it; the
/// refetch [`Effect`] subscribes to it.
fn page_signal() -> Rc<Signal<usize>> {
    Owner::current()
        .expect("page_signal() requires an active Owner scope")
        .cache(PAGE_KEY, || Signal::new(0_usize))
}

/// The reload-nonce `Signal`. Bumping it re-fires the refetch Effect for the
/// *same* page (a changing dep — `Signal::set` of an equal value is a no-op,
/// so re-fetching needs a value that always changes).
fn reload_nonce() -> Rc<Signal<u64>> {
    Owner::current()
        .expect("reload_nonce() requires an active Owner scope")
        .cache(NONCE_KEY, || Signal::new(0_u64))
}

/// The async page carrier. Starts `Loading`; the refetch Effect's eager run
/// resolves page 0 at boot.
fn result() -> Rc<Resource<Vec<AssetRow>, String>> {
    Owner::current()
        .expect("result() requires an active Owner scope")
        .cache(RESULT_KEY, Resource::loading)
}

/// Lifetime marker holding the auto-refetch [`Effect`] (R665 effect
/// retention — the owner's cleanup queue holds the authoritative `Rc`, the
/// cache slot keeps the handle alive).
struct RefetchMarker {
    _effect: Effect,
}

/// Install the auto-refetch [`Effect`] exactly once. The Effect subscribes to
/// the `page` + `reload_nonce` Signals and, whenever either changes (including
/// its eager initial run at boot → page 0), drives a fresh [`fetch_page`]
/// future into the [`Resource`] through the shell-polled
/// [`LocalTaskPump`](pinion_core::LocalTaskPump).
///
/// Pre-resolves every dependent cache slot *before* the cache factory to dodge
/// a nested `Owner::cache` `BorrowMut` (the R666 guard /
/// [`owner-cache-no-nested-factory`]).
fn install_refetch() -> Rc<RefetchMarker> {
    let owner = Owner::current().expect("install_refetch() requires an active Owner scope");
    let page = page_signal();
    let nonce = reload_nonce();
    let res = result();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(REFETCH_KEY, move || {
        let page_e = page.clone();
        let nonce_e = nonce.clone();
        let res_e = res.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            // Subscribe to BOTH query params; either change triggers a fetch
            // of the current page (the nonce re-fetches the same page).
            let p = page_e.get();
            let _ = nonce_e.get();
            res_e.fetch_with(&*pump_e, fetch_page(p));
        });
        RefetchMarker { _effect: effect }
    })
}

// ─── status / list / button rendering ─────────────────────────────────────

/// The pager status line — the single SSOT for the visible text *and* the
/// `role=status` live-region accessible name.
fn pager_line(page: usize, state: &ResourceState<Vec<AssetRow>, String>) -> String {
    let human = page + 1;
    match state {
        ResourceState::Loading => format!("Loading page {human} of {TOTAL_PAGES}\u{2026}"),
        ResourceState::Ready(rows) => {
            format!(
                "Page {human} of {TOTAL_PAGES} \u{2014} {} assets",
                rows.len()
            )
        }
        ResourceState::Error(err) => {
            format!("Page {human} of {TOTAL_PAGES} \u{2014} error: {err}")
        }
    }
}

/// The in-list note shown for the non-`Ready` arms (placeholder / error), or
/// `None` when rows are present.
fn list_note(state: &ResourceState<Vec<AssetRow>, String>) -> Option<String> {
    match state {
        ResourceState::Loading => Some("Loading assets\u{2026}".to_owned()),
        ResourceState::Error(err) => Some(format!("Could not load this page: {err}")),
        ResourceState::Ready(_) => None,
    }
}

/// One asset row — a tagged container holding the single SSOT [`row_label`]
/// line (paint == `listitem` accessible name).
fn asset_row_scene(global_index: usize, row: &AssetRow, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            row_label(row),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(row_tag(global_index))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(WIN_W - 80, 28)),
        ),
    )
}

/// The asset list panel — the current page's rows, or a single note line for
/// the `Loading` / `Error` arms. `LIST_TAG` stays present across all states.
fn asset_list_scene(
    page: usize,
    state: &ResourceState<Vec<AssetRow>, String>,
    theme: &Theme,
) -> Scene {
    let children: Vec<Scene> = if let Some(note) = list_note(state) {
        let ink = if matches!(state, ResourceState::Error(_)) {
            ColorRole::Error
        } else {
            ColorRole::OnSurfaceMuted
        };
        vec![Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                note,
                Rect::default(),
                TextStyle::new()
                    .with_size_px(14)
                    .with_fg(theme.resolve(ink))
                    .with_align(TextAlign::Center),
            ))])
            .with_tag(LIST_NOTE_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(WIN_W - 80, 28)),
            ),
        )]
    } else if let ResourceState::Ready(rows) = state {
        rows.iter()
            .enumerate()
            .map(|(r, row)| asset_row_scene(page * PAGE_SIZE + r, row, theme))
            .collect()
    } else {
        Vec::new()
    };

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(LIST_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(8),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_gap(2)
                    .with_padding(Rect::new(12, 12, 12, 12))
                    .with_size(Size::px(WIN_W - 56, 220)),
            ),
    )
}

/// Render one nav button through the [`pinion_widget_paint::button`] SSOT.
fn nav_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    hover_key: &'static str,
    theme: &Theme,
) -> Scene {
    pinion_widget_paint::button::button_scene(
        label,
        state,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(120, 40))
            .with_label_font_size_px(15),
    )
}

/// `Disabled` at the clamped ends (page 0 → Prev; last page → Next), else the
/// live interaction posture read from the button's External.
fn clamped(posture: ButtonState, disabled: bool) -> ButtonState {
    if disabled {
        ButtonState::Disabled
    } else {
        posture
    }
}

/// Cached posture of the three nav buttons + their focus flags
/// (`[prev, next, reload]`). The page index + async result live in the
/// reactive layer the owner-scoped view + `access_node` read directly.
type AsyncDataViewState = (ButtonState, ButtonState, ButtonState);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading the
/// reactive `page` Signal + `Resource` for the status line / rows. No
/// side-effects (the refetch Effect is installed in `create_extra_externals`).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: AsyncDataViewState, _frame: &Frame) -> Scene {
    let (prev_posture, next_posture, reload_posture) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let page = page_signal().get();
    let res = result().state();

    let title = Scene::Text(TextNode::styled(
        "Async asset browser (paged, lazy-loaded)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            pager_line(page, &res),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    let list = asset_list_scene(page, &res, &theme);

    let prev_state = clamped(prev_posture, page == 0);
    let next_state = clamped(next_posture, page + 1 >= TOTAL_PAGES);
    let buttons = Scene::Container(
        ContainerNode::new(vec![
            nav_button(PREV_TAG, "Prev", prev_state, PREV_HOVER_KEY, &theme),
            nav_button(NEXT_TAG, "Next", next_state, NEXT_HOVER_KEY, &theme),
            nav_button(
                RELOAD_TAG,
                "Reload",
                reload_posture,
                RELOAD_HOVER_KEY,
                &theme,
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_gap(12),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, list, buttons])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(16),
            ),
    )
}

struct AsyncDataView;

impl WidgetCore for AsyncDataView {
    type State = AsyncDataViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Install the auto-refetch Effect FIRST (boot eager run → fetch
        // page 0) so the data layer is live before the first paint — and so
        // the side-effecting fetch never runs inside the pure view-fn.
        let _refetch = install_refetch();
        vec![
            ExtraExternal::new(NEXT_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(RELOAD_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        PREV_TAG
    }

    fn read_state(scene: &Scene) -> AsyncDataViewState {
        (
            read_button_state(scene, PREV_TAG),
            read_button_state(scene, NEXT_TAG),
            read_button_state(scene, RELOAD_TAG),
        )
    }

    fn view(state: AsyncDataViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-async-data (R923 §5.22 §5.23)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        [PREV_TAG, NEXT_TAG, RELOAD_TAG]
            .into_iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// R923 reducer: navigation mutates *only* the query Signals — the
    /// refetch Effect reacts and performs the fetch. Prev/Next clamp to the
    /// page bounds (so a click on a `Disabled` end button is a no-op); Reload
    /// bumps the nonce to re-fetch the current page.
    fn update(
        _state: AsyncDataViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "async_prev.click" => {
                let page = page_signal();
                page.set(page.get().saturating_sub(1));
            }
            "async_next.click" => {
                let page = page_signal();
                page.set((page.get() + 1).min(TOTAL_PAGES - 1));
            }
            "async_reload.click" => {
                let nonce = reload_nonce();
                nonce.set(nonce.get().wrapping_add(1));
            }
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for AsyncDataView {
    /// A WAI-ARIA `list` over the current page (one `listitem` per resident
    /// row, carrying its absolute 1-based `aria-posinset` against the full
    /// `aria-setsize = TOTAL_ASSETS`), a `role=status` live region for the
    /// pager line, and the three nav `button`s. The `listitem` names come
    /// from the same [`row_label`] SSOT the paint uses (R922.1: a headline's
    /// content must be created *and named* in the a11y tree, not left to
    /// enrich).
    fn access_node(state: &AsyncDataViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let (prev_posture, next_posture, reload_posture) = *state;
        let page = page_signal().get();
        let res = result().state();

        let mut list = AccessNode::new(LIST_TAG, AriaRole::List)
            .with_name("Assets")
            .with_size_of_set(u32::try_from(TOTAL_ASSETS).unwrap_or(u32::MAX));
        let mut items: Vec<AccessNode> = Vec::new();
        if let ResourceState::Ready(rows) = &res {
            for (r, row) in rows.iter().enumerate() {
                let global = page * PAGE_SIZE + r;
                let tag = row_tag(global);
                list = list.with_child(tag.clone());
                items.push(
                    AccessNode::new(tag, AriaRole::ListItem)
                        .with_name(row_label(row))
                        .with_position_in_set(u32::try_from(global + 1).unwrap_or(u32::MAX))
                        .with_size_of_set(u32::try_from(TOTAL_ASSETS).unwrap_or(u32::MAX)),
                );
            }
        }

        let prev_state = clamped(prev_posture, page == 0);
        let next_state = clamped(next_posture, page + 1 >= TOTAL_PAGES);

        let mut nodes = Vec::with_capacity(items.len() + 5);
        nodes.push(list);
        nodes.extend(items);
        nodes.push(AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(pager_line(page, &res)));
        nodes.push(
            AccessNode::new(PREV_TAG, AriaRole::Button)
                .with_state(button_a11y_state(prev_state, focused == Some(PREV_TAG))),
        );
        nodes.push(
            AccessNode::new(NEXT_TAG, AriaRole::Button)
                .with_state(button_a11y_state(next_state, focused == Some(NEXT_TAG))),
        );
        nodes.push(
            AccessNode::new(RELOAD_TAG, AriaRole::Button).with_state(button_a11y_state(
                reload_posture,
                focused == Some(RELOAD_TAG),
            )),
        );
        nodes
    }
}

impl WidgetView for AsyncDataView {
    type Renderer = HelloAsyncDataRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AsyncDataView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> AsyncDataViewState {
        (ButtonState::Idle, ButtonState::Idle, ButtonState::Idle)
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    fn text_under(scene: &Scene, tag: &str) -> Option<String> {
        let container = find_container(scene, tag)?;
        container.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            Scene::Container(_) => text_under(ch, tag),
            _ => None,
        })
    }

    fn status_text(scene: &Scene) -> Option<String> {
        text_under(scene, STATUS_TAG)
    }

    /// Drive the owner-scoped `LocalTaskPump` to completion — what the shell
    /// does once per frame (R761.1). A fetch is deferred `FETCH_LATENCY_POLLS`
    /// polls, so loop generously past that.
    fn drain_pump() {
        for _ in 0..(FETCH_LATENCY_POLLS + 8) {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    /// Install the refetch Effect (mirrors `create_extra_externals`).
    fn boot() {
        let _ = install_refetch();
    }

    #[test]
    fn r923_boot_loads_page_one_through_pump() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            // Before the pump runs, page 0's fetch is still in flight.
            assert_eq!(result().state(), ResourceState::Loading);
            drain_pump();
            view(idle(), &Frame::new())
        });
        assert_eq!(
            status_text(&scene).as_deref(),
            Some("Page 1 of 5 \u{2014} 6 assets"),
        );
        // First page's six rows are resident and tagged.
        for i in 0..PAGE_SIZE {
            assert!(
                find_container(&scene, &row_tag(i)).is_some(),
                "row {i} present on page 1",
            );
        }
    }

    #[test]
    fn r923_next_advances_page_and_refetches() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            drain_pump();
            // Navigate Next: reducer sets page=1 → Effect refetches.
            dispatch("async_next.click");
            // Mid-flight: Loading is observable before the pump drains.
            assert_eq!(result().state(), ResourceState::Loading);
            drain_pump();
            view(idle(), &Frame::new())
        });
        assert_eq!(
            status_text(&scene).as_deref(),
            Some("Page 2 of 5 \u{2014} 6 assets"),
        );
        // Page-2 rows present, page-1 rows gone.
        assert!(
            find_container(&scene, &row_tag(PAGE_SIZE)).is_some(),
            "row 6 present"
        );
        assert!(find_container(&scene, &row_tag(0)).is_none(), "row 0 gone");
    }

    #[test]
    fn r923_error_page_surfaces_error_arm() {
        let owner = Owner::new();
        let (state, note) = owner.run(|| {
            boot();
            drain_pump();
            // Step to ERROR_PAGE.
            for _ in 0..ERROR_PAGE {
                dispatch("async_next.click");
                drain_pump();
            }
            let scene = view(idle(), &Frame::new());
            (result().state(), text_under(&scene, LIST_NOTE_TAG))
        });
        assert!(
            matches!(state, ResourceState::Error(ref e) if e.contains("page 3")),
            "ERROR_PAGE resolves to Error, got {state:?}",
        );
        assert!(
            note.as_deref()
                .is_some_and(|n| n.contains("Could not load")),
            "list shows the error note, got {note:?}",
        );
    }

    #[test]
    fn r923_reload_nonce_refetches_same_page() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            let gen_before = result().generation();
            dispatch("async_reload.click");
            // Reload re-fired the Effect for the SAME page (generation bumped).
            assert_eq!(result().state(), ResourceState::Loading);
            assert_eq!(result().generation(), gen_before + 1);
            drain_pump();
            assert!(matches!(result().state(), ResourceState::Ready(_)));
            // Page index is unchanged.
            assert_eq!(page_signal().get(), 0);
        });
    }

    #[test]
    fn r923_next_clamps_at_last_page() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            for _ in 0..(TOTAL_PAGES + 3) {
                dispatch("async_next.click");
                drain_pump();
            }
            assert_eq!(page_signal().get(), TOTAL_PAGES - 1, "clamped at last page");
        });
    }

    #[test]
    fn r923_prev_clamps_at_first_page() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            dispatch("async_prev.click");
            assert_eq!(page_signal().get(), 0, "clamped at first page");
        });
    }

    #[test]
    fn r923_rapid_navigation_keeps_only_latest_page() {
        // Two Next clicks before the pump drains: the first fetch's token is
        // stale (generation advanced), so its completion no-ops — only the
        // latest page lands. The Resource cancellation contract (R26).
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            dispatch("async_next.click"); // page 1
            dispatch("async_next.click"); // page 2
            drain_pump();
            assert_eq!(page_signal().get(), 2);
            match result().state() {
                ResourceState::Ready(rows) => {
                    // Page 3 is ERROR_PAGE → this asserts page index 2 errored,
                    // proving navigation reached the latest page, not page 1.
                    panic!("expected ERROR_PAGE Error, got Ready({} rows)", rows.len());
                }
                ResourceState::Error(e) => assert!(e.contains("page 3")),
                ResourceState::Loading => panic!("still loading after drain"),
            }
        });
    }

    #[test]
    fn r923_disabled_buttons_clamp_at_bounds() {
        // Prev is Disabled at page 0; Next is Disabled at the last page.
        assert_eq!(clamped(ButtonState::Idle, true), ButtonState::Disabled);
        assert_eq!(clamped(ButtonState::Hover, false), ButtonState::Hover);
        let owner = Owner::new();
        let nodes = owner.run(|| {
            boot();
            drain_pump();
            <AsyncDataView as WidgetA11y>::access_node(&idle(), None)
        });
        let prev = nodes.iter().find(|n| n.tag == PREV_TAG).expect("prev node");
        assert!(prev.state.disabled, "Prev disabled on page 1");
        let next = nodes.iter().find(|n| n.tag == NEXT_TAG).expect("next node");
        assert!(!next.state.disabled, "Next enabled on page 1");
    }

    #[test]
    fn r923_a11y_list_with_six_listitems_and_status() {
        let owner = Owner::new();
        let nodes = owner.run(|| {
            boot();
            drain_pump();
            <AsyncDataView as WidgetA11y>::access_node(&idle(), None)
        });
        let list = nodes.iter().find(|n| n.tag == LIST_TAG).expect("list node");
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(list.children.len(), PAGE_SIZE, "six child rows claimed");
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ListItem)
                .count(),
            PAGE_SIZE,
        );
        // First listitem name == its painted SSOT row label.
        let first = nodes
            .iter()
            .find(|n| n.tag == row_tag(0))
            .expect("first row node");
        assert_eq!(first.position_in_set, Some(1));
        assert_eq!(
            first.size_of_set,
            Some(u32::try_from(TOTAL_ASSETS).unwrap())
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status)
        );
        assert_eq!(
            nodes.iter().filter(|n| n.role == AriaRole::Button).count(),
            3,
        );
    }

    #[test]
    fn r923_three_nav_buttons_are_focusable() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![
                PREV_TAG.to_owned(),
                NEXT_TAG.to_owned(),
                RELOAD_TAG.to_owned()
            ],
        );
    }

    #[test]
    fn r923_row_label_is_single_ssot() {
        let row = AssetRow {
            name: "asset_000.png".to_owned(),
            kind: "Texture".to_owned(),
            size_kb: 11,
        };
        assert_eq!(row_label(&row), "asset_000.png (Texture, 11 KB)");
    }

    /// Build a click intent for the given dotted tag.
    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_owned(), pinion_core::external::IntrospectValue::Null)
    }

    /// Dispatch a click intent for its side-effect (the empty command list is
    /// intentionally dropped).
    fn dispatch(tag: &str) {
        let _ = AsyncDataView::update(idle(), &intent(tag));
    }
}
