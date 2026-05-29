//! `settings-panel` — R667 §5.16 Phase A finalisation: the 2nd
//! composed multi-widget application after `examples/todomvc`.
//!
//! Material 3 Settings pattern with a left vertical
//! [`RadioGroupExternal`] nav rail (5 sections — theme / appearance
//! / profile / notifications / actions) and a right detail pane
//! that swaps body content based on the active nav index.
//!
//! Section breakdown:
//! Theme — [`ToggleExternal`] dark/light flip; the `update` reducer
//! calls `ThemeProvider::set_mode` on the canonical `app` provider
//! so the whole window re-themes from one signal write (mirror of
//! `examples/hello-theme`). Appearance — [`SliderExternal`]
//! horizontal font-scale slider; value persists and a preview
//! `Text` mirrors it so AI introspection can read via `scene/query`.
//! Profile — [`TextFieldExternal`] for display name (4th consumer
//! of `pinion_widget_paint::text_field::view_field`). Notifications —
//! static labels listing channels (interactive toggles carry to
//! R668). Actions — static "auto-save" status text + reset hint
//! (interactive reset button carries to R668 with `ButtonExternal`).
//!
//! Persistence (R665 §3 §5.15 substrate, 2nd application consumer):
//! `SettingsPersistedState` carries nav index + dark mode + font
//! scale + display name; `use_settings_persistence` mirrors
//! todomvc's `use_persistence_boot` (`Effect`-retention pattern,
//! batched hydration). Every change writes through immediately —
//! `Apply` / `Cancel` buttons are explicitly UX-only and never
//! required to commit.
//!
//! AI introspection (§2 #2): every section exposes its state
//! through `scene/query` against the per-widget `external/<slot>`
//! paths. The R667 demo (`tools/demos/settings_panel_r667.py`)
//! drives every section via `scene/invoke v1` paths (R666 §5.34
//! multi-External addressing) + `scene/key character` arc (R666
//! §5.37) and verifies the launch-kill-relaunch persistence cycle.

use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::WidgetA11y;
use pinion_core::clipboard::Clipboard;
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::intent::Intent;
use pinion_core::reactive::{batch, Effect, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::storage::{InMemoryStorage, Storage};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme, ThemeMode};
use pinion_core::widgets::caret_blink::CaretBlink;
use pinion_core::widgets::checkbox::{CheckboxExternal, CheckboxState};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::scrollbar::{use_scrollbar_interaction, ScrollBarExternal};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::slider::{SliderExternal, SliderState};
use pinion_core::widgets::text_edit::TextEditState;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::widgets::toggle::{ToggleExternal, ToggleState};
use pinion_core::{
    intent_tag, scale_normalized_to_px, Color, Command, Frame, Scene, WidgetCore,
    WidgetStateName,
};
use pinion_core::InMemoryClipboard;
use pinion_platform_storage::open_app_storage;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::checkbox::{view_checkbox, CheckboxStyle};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::text_field as tf_paint;
use pinion_widget_paint::text_field::TextFieldStyle;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(SettingsPanelRenderer, SettingsPanelRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 720;
const WIN_H: u32 = 480;

/// Left nav rail width — M3 `NavigationRail` default is 80 px;
/// bumped to 200 to host the section labels alongside the indicator
/// pill.
const NAV_W: u32 = 200;
/// Per-nav-row size. Height matches M3 `NavigationRailItem` (~64 px);
/// width = `NAV_W` minus `2 × NAV_GAP` so the indicator pill stays
/// inside the rail's outline.
const NAV_ROW_H: u32 = 48;
const NAV_ROW_PAD: u32 = 12;
const NAV_GAP: u32 = 4;

const SECTION_PAD: u32 = 32;
const ROW_GAP: u32 = 16;
const TITLE_FONT_PX: u32 = 24;
const LABEL_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;

/// Theme toggle widget visual constants (mirror of `hello-theme`'s
/// track/knob dimensions so the M3 look reads identical to the
/// reference binding).
const TOGGLE_TRACK_W: u32 = 52;
const TOGGLE_TRACK_H: u32 = 32;
const TOGGLE_TRACK_RADIUS: u32 = 16;
const TOGGLE_TRACK_PAD: u32 = 4;
const TOGGLE_KNOB_SIZE: u32 = 24;
const TOGGLE_KNOB_RADIUS: u32 = 12;

/// Font scale slider visual constants (mirror of `hello-slider`).
const SLIDER_TRACK_W: u32 = 280;
const SLIDER_TRACK_H: u32 = 8;
const SLIDER_TRACK_RADIUS: u32 = 4;
const SLIDER_THUMB_SIZE: u32 = 16;
const SLIDER_THUMB_RADIUS: u32 = 8;
const SLIDER_RANGE: u32 = SLIDER_TRACK_W - SLIDER_THUMB_SIZE;

/// `TextField` input row size — wide enough for typical display
/// names without horizontal scroll.
const PROFILE_FIELD_W: u32 = 360;
const PROFILE_FIELD_H: u32 = 48;

/// Number of nav-rail sections. Used by `RadioGroupExternal::new`
/// and the `read_nav_radio_states` walk; to add a 6th section, bump
/// this constant and extend the per-section view dispatcher in
/// `view_detail_pane`.
const NAV_COUNT: usize = 5;

// ─── tags (paint-side + Owner::cache key namespace) ───────────────

/// Primary External — the Profile section's display-name
/// `TextField`. Matches the `WidgetCore::tag()` convention todomvc /
/// hello-* use.
const PROFILE_TF_TAG: &str = "profile_display_name";
/// Nav rail `RadioGroupExternal` tag. Composite per-radio tags
/// follow the `nav_rail#0` … `nav_rail#4` shape that
/// [`RadioGroupExternal`] emits through `pinion_widget_paint`'s
/// composite-tag substrate.
const NAV_TAG: &str = "nav_rail";
/// Theme dark-mode toggle `ExtraExternal` tag.
const THEME_TOGGLE_TAG: &str = "theme_toggle";
/// Font scale slider `ExtraExternal` tag.
const FONT_SLIDER_TAG: &str = "font_slider";
/// (R669) Notifications-section `CheckboxExternal` base tag — each
/// of the [`NOTIFICATION_COUNT`] channels uses one of the
/// [`NOTIF_INSTANCE_TAGS`] entries (composite-tag substrate per
/// R55.D.5) for its instance. The base tag itself is reserved for
/// the section's root container so `Scene::contains_tag` /
/// `rect_for_tag` queries find the cluster as a whole.
const NOTIF_TAG_BASE: &str = "notifications";

/// (R669) Per-channel composite-tag instances. Static `&'static str`
/// array so [`ExtraExternal::new`] can carry the slot without
/// runtime allocation — `format!("{NOTIF_TAG_BASE}#{i}")` would
/// produce a `String` whose lifetime cannot satisfy the trait's
/// `&'static str` argument without a `Box::leak`. Tying the tag
/// inventory to a const array also pins it in one place for the
/// composite-tag substrate's [[composite-tag-parse_send_payload]]
/// consumer to keep the index ordering aligned with
/// [`NOTIFICATION_LABELS`].
const NOTIF_INSTANCE_TAGS: [&str; NOTIFICATION_COUNT] = [
    "notifications#0",
    "notifications#1",
    "notifications#2",
    "notifications#3",
    "notifications#4",
    "notifications#5",
];
/// (R669) Notifications-section `ScrollBarExternal` tag — the 4th
/// `pinion-widget-paint::scrollbar` consumer (hello-listbox = 1st,
/// todomvc = 2nd, settings-panel detail pane is potential 3rd,
/// notifications cluster is the realised 4th).
const NOTIF_SCROLLBAR_TAG: &str = "notifications_scrollbar";

/// Root owner cache key for the [`ThemeProvider`]. `"app"` matches
/// the convention shared with `hello-toggle` / `hello-theme` /
/// `todomvc` so a future host binding can share one provider across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Persistence schema bump (R665 mirror). R669 raised v1 → v2 to
/// add the `notifications: [bool; 6]` field for the 6-channel
/// `CheckboxExternal` cluster ([`view_notifications_section`]). The
/// hydrate path's [`migrate_v1_to_v2`] back-fills the missing
/// `notifications` field on v1 saves so existing users do not lose
/// their nav-index / dark-mode / font-scale / display-name on
/// upgrade — the migrator is the R665 schema-migrator carry's first
/// implementation, the textbook canonical pattern for every future
/// breaking-change axis.
const PERSISTED_SCHEMA_VERSION: u32 = 2;
const STORAGE_APP_NAME: &str = "pinion-settings-panel";
const STORAGE_STATE_KEY: &str = "state.json";

/// Default font scale — `0.5` sits in the middle of the slider so
/// the first paint reads visually centred.
const DEFAULT_FONT_SCALE: f32 = 0.5;

/// (R669 §5.15) — number of `CheckboxExternal` slots in the
/// Notifications section. Named so the `NotificationChannel`
/// labels, the `composite-tag` indices (`notif#0`..`notif#5`), and
/// the `notifications: [bool; NOTIFICATION_COUNT]` persistence slot
/// all derive from one source.
const NOTIFICATION_COUNT: usize = 6;

/// (R669) — Notifications channel labels. Order matches the persisted
/// `notifications: [bool; NOTIFICATION_COUNT]` array index; reordering
/// or renaming is a breaking schema change (would need v2 → v3 bump
/// + a migrator that walks the previous-order array).
const NOTIFICATION_LABELS: [&str; NOTIFICATION_COUNT] = [
    "Email — transactional",
    "Push — mobile alerts",
    "Marketing — newsletter",
    "Digest — weekly roll-up",
    "Security — sign-in alerts",
    "Feedback — user research invites",
];

/// (R669) — first-install / v1-upgrade defaults for the 6 channels.
/// Transactional + security default on (canonical platform defaults);
/// marketing + digest default off. Mirrors Apple Mail / Gmail default
/// per Material 3 a11y "least surprise on first install".
const NOTIFICATION_DEFAULTS: [bool; NOTIFICATION_COUNT] = [
    true,  // Email — transactional
    true,  // Push — mobile alerts
    false, // Marketing — newsletter
    false, // Digest — weekly roll-up
    true,  // Security — sign-in alerts
    false, // Feedback — user research invites
];

// ─── persistence state ────────────────────────────────────────────

/// Serializable snapshot of every persisted setting. Single-blob
/// schema (R665 [[r665-storage-substrate]] convention) — atomic
/// load + save through one `Storage::save` call keeps the on-disk
/// state consistent across crashes.
///
/// R669 §5.15 — `notifications` field added at schema v2. Existing
/// v1 saves on disk skip the field (serde rejects strict
/// missing-field decode); the hydrate path explicitly tries the v2
/// shape first, then falls back to [`SettingsPersistedStateV1`] +
/// [`migrate_v1_to_v2`] when v1 is on disk. The migrator pattern is
/// the R665 schema-migrator carry's first implementation; every
/// future schema bump follows the same `try-current → fallback-prev
/// → migrate` shape.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SettingsPersistedState {
    schema_version: u32,
    nav_index: u32,
    dark_mode: bool,
    font_scale: f32,
    display_name: String,
    /// (R669) per-channel on/off flags. Array index follows
    /// [`NOTIFICATION_LABELS`] ordering — reordering or renaming
    /// labels is a breaking change that bumps to v3 + ships a
    /// shape-preserving v2 → v3 migrator.
    notifications: [bool; NOTIFICATION_COUNT],
}

/// R669 §5.15 — explicit shape of v1-on-disk records (`notifications`
/// field missing). Only used inside the hydrate fallback path; never
/// serialised again (the next save writes the v2 shape).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct SettingsPersistedStateV1 {
    schema_version: u32,
    nav_index: u32,
    dark_mode: bool,
    font_scale: f32,
    display_name: String,
}

/// R669 §5.15 — back-fill the v1-shape record with the
/// [`NOTIFICATION_DEFAULTS`] so the v2 codepath does not need
/// branched-shape handling beyond this single boundary. The
/// returned [`SettingsPersistedState`] carries the bumped
/// `schema_version` so the next `save` round-trips through the v2
/// serialiser cleanly (no double-bump risk on subsequent reads).
fn migrate_v1_to_v2(v1: SettingsPersistedStateV1) -> SettingsPersistedState {
    SettingsPersistedState {
        schema_version: PERSISTED_SCHEMA_VERSION,
        nav_index: v1.nav_index,
        dark_mode: v1.dark_mode,
        font_scale: v1.font_scale,
        display_name: v1.display_name,
        notifications: NOTIFICATION_DEFAULTS,
    }
}

impl SettingsPersistedState {
    fn snapshot(
        nav: &Cell<u32>,
        dark: &Signal<bool>,
        scale: &Cell<f32>,
        name: &Signal<String>,
        notifications: &[Rc<Signal<bool>>; NOTIFICATION_COUNT],
    ) -> Self {
        // R669 — materialise the [bool; N] array from the 6 signal
        // handles. `Signal::get` subscribes the calling Effect, so
        // any channel flip triggers the save Effect to re-fire and
        // persist the new array.
        let mut bits = [false; NOTIFICATION_COUNT];
        for (i, sig) in notifications.iter().enumerate() {
            bits[i] = sig.get();
        }
        Self {
            schema_version: PERSISTED_SCHEMA_VERSION,
            nav_index: nav.get(),
            dark_mode: dark.get(),
            font_scale: scale.get(),
            display_name: name.get(),
            notifications: bits,
        }
    }
}

/// Owns the persistence save [`Effect`] for the app's lifetime.
/// Returned through [`Owner::cache`] so the Effect handle is
/// retained inside the cached slot (the `Owner::cleanup` queue holds
/// only `Weak` — dropping the handle would silently GC the Effect,
/// per [[effect-driven-driver-monotonic-counter]] / [[r665-storage-substrate]]).
struct PersistenceBootMarker {
    _save_effect: Effect,
}

/// Sized newtype around `Box<dyn Storage>` so `Owner::cache<V>` can
/// store a single concrete `V`. Mirrors todomvc's `AppStorage` /
/// `AppClipboard` shape per [[owner-cache-typed-key]].
struct AppStorage(Box<dyn Storage>);

impl Storage for AppStorage {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.0.load(key)
    }
    fn save(&self, key: &str, bytes: &[u8]) {
        self.0.save(key, bytes);
    }
    fn remove(&self, key: &str) {
        self.0.remove(key);
    }
}

fn build_storage() -> Box<dyn Storage> {
    if let Ok(custom_dir) = std::env::var("PINION_STORAGE_DIR") {
        match pinion_platform_storage::FileStorage::try_new(std::path::PathBuf::from(&custom_dir)) {
            Ok(file) => return Box::new(file) as Box<dyn Storage>,
            Err(e) => eprintln!(
                "settings-panel: FileStorage init at {custom_dir:?} via PINION_STORAGE_DIR \
                 failed ({e}); falling back to default app data dir",
            ),
        }
    }
    match open_app_storage(STORAGE_APP_NAME) {
        Ok(file) => Box::new(file) as Box<dyn Storage>,
        Err(e) => {
            eprintln!(
                "settings-panel: open_app_storage({STORAGE_APP_NAME:?}) failed ({e}); \
                 persistence will be in-memory only (state lost on exit)",
            );
            Box::new(InMemoryStorage::new()) as Box<dyn Storage>
        }
    }
}

#[must_use]
fn use_storage() -> Rc<AppStorage> {
    let owner = Owner::current().expect("use_storage requires an active Owner scope");
    owner.cache("settings_panel.storage", || AppStorage(build_storage()))
}

/// Sized newtype around `Box<dyn Clipboard>` for the same reason.
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
}

/// Reactive store for `nav_index` (current detail section). `Cell`
/// not `Signal` because the view reads through `read_state` walking
/// the nav `RadioGroup`'s `selected_index` slot.
#[must_use]
fn use_nav_index() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_nav_index requires an active Owner scope");
    owner.cache("settings_panel.nav_index", || Cell::new(0))
}

#[must_use]
fn use_dark_mode() -> Rc<Signal<bool>> {
    let owner = Owner::current().expect("use_dark_mode requires an active Owner scope");
    owner.cache("settings_panel.dark_mode", || Signal::new(false))
}

#[must_use]
fn use_font_scale() -> Rc<Cell<f32>> {
    let owner = Owner::current().expect("use_font_scale requires an active Owner scope");
    owner.cache("settings_panel.font_scale", || Cell::new(DEFAULT_FONT_SCALE))
}

#[must_use]
fn use_display_name() -> Rc<Signal<String>> {
    let owner = Owner::current().expect("use_display_name requires an active Owner scope");
    owner.cache("settings_panel.display_name", || Signal::new(String::new()))
}

/// (R669 §5.15) — per-channel `Signal<bool>` slot for the
/// Notifications cluster. Wrapped in a Rust newtype so
/// [`Owner::cache`] gets a single concrete `V` (per
/// [[owner-cache-typed-key]]) instead of an array of `Rc`s that the
/// cache slot would type-erase.
struct NotificationChannels {
    signals: [Rc<Signal<bool>>; NOTIFICATION_COUNT],
}

impl NotificationChannels {
    fn new() -> Self {
        // R669 — initial values are [`NOTIFICATION_DEFAULTS`]; the
        // persistence hydrate path overrides via `set` once the v2
        // record reads back (or via the v1→v2 migrator's bit-array
        // fallback for first-install / upgrade boots).
        Self {
            signals: std::array::from_fn(|i| {
                Rc::new(Signal::new(NOTIFICATION_DEFAULTS[i]))
            }),
        }
    }
}

#[must_use]
fn use_notification_channels() -> Rc<NotificationChannels> {
    let owner = Owner::current().expect(
        "use_notification_channels requires an active Owner scope",
    );
    owner.cache("settings_panel.notifications", NotificationChannels::new)
}

/// (R669 §5.45) — per-notifications-section `ScrollState` slot. 4th
/// `ScrollBarExternal` consumer; hosts the
/// [[scrollbar-interaction-signal-substrate]] tracking the
/// scrollbar interaction state (idle / hover / dragging) the M3
/// state-layer ramp reads. Delegates to the canonical framework
/// hook so cache-key collisions across consumers are impossible
/// (the framework hook prefixes its slot with the per-tag
/// namespace).
#[must_use]
fn use_notif_scrollbar() -> Rc<ScrollState> {
    pinion_core::widgets::scroll::use_scroll_state(NOTIF_SCROLLBAR_TAG)
}

#[must_use]
fn use_text_edit_state(tag: &'static str) -> Rc<TextEditState> {
    let owner = Owner::current().expect("use_text_edit_state requires an active Owner scope");
    owner.cache(tag, TextEditState::new)
}

#[must_use]
fn use_caret_blink(tag: &'static str) -> Rc<CaretBlink> {
    let owner = Owner::current().expect("use_caret_blink requires an active Owner scope");
    owner.cache(tag, CaretBlink::new)
}

#[must_use]
fn use_clipboard(tag: &'static str) -> Rc<AppClipboard> {
    let owner = Owner::current().expect("use_clipboard requires an active Owner scope");
    owner.cache(tag, || AppClipboard(Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>))
}

/// Run the persistence boot pass exactly once per `Owner`. Mirrors
/// `todomvc::use_persistence_boot` — pre-resolves every dependent
/// cache slot to dodge nested `Owner::cache` `BorrowMut`s (per the
/// R666 framework guard / [[owner-cache-no-nested-factory]]).
fn use_settings_persistence() -> Rc<PersistenceBootMarker> {
    let storage = use_storage();
    let nav = use_nav_index();
    let dark = use_dark_mode();
    let scale = use_font_scale();
    let name = use_display_name();
    let notif = use_notification_channels();
    let display_name_text_state = use_text_edit_state(PROFILE_TF_TAG);
    let owner = Owner::current().expect("use_settings_persistence requires an active Owner scope");
    let owner_for_effect = owner.clone();
    owner.cache("settings_panel.persistence.boot", move || {
            // (1) Hydrate. R669 §5.15 — try the current v2 shape
            // first; on `serde` reject (missing `notifications`
            // field) fall back to the explicit v1 shape and run
            // [`migrate_v1_to_v2`]. Either path lands a fully-
            // populated [`SettingsPersistedState`]. Schema-version
            // mismatch beyond the v1→v2 chain (future v3+) takes
            // the "start fresh" arm so the user does not get
            // partial-state corruption.
            if let Some(bytes) = storage.load(STORAGE_STATE_KEY) {
                let state_opt: Option<SettingsPersistedState> =
                    match serde_json::from_slice::<SettingsPersistedState>(&bytes) {
                        Ok(state)
                            if state.schema_version == PERSISTED_SCHEMA_VERSION =>
                        {
                            Some(state)
                        }
                        Ok(state) => {
                            eprintln!(
                                "settings-panel: persisted schema {} \u{2260} supported {} \
                                 \u{2014} starting fresh (saved state will be overwritten)",
                                state.schema_version, PERSISTED_SCHEMA_VERSION,
                            );
                            None
                        }
                        Err(_) => {
                            // R669 — v2 decode failed; try v1 +
                            // migrate before declaring corruption.
                            match serde_json::from_slice::<SettingsPersistedStateV1>(&bytes) {
                                Ok(v1) if v1.schema_version == 1 => {
                                    Some(migrate_v1_to_v2(v1))
                                }
                                Ok(v1) => {
                                    eprintln!(
                                        "settings-panel: v1-shape record carries unexpected \
                                         schema_version {}; starting fresh",
                                        v1.schema_version,
                                    );
                                    None
                                }
                                Err(e) => {
                                    eprintln!(
                                        "settings-panel: persisted state at \
                                         {STORAGE_STATE_KEY:?} failed to deserialize as \
                                         v2 or v1 ({e}) \u{2014} starting fresh",
                                    );
                                    None
                                }
                            }
                        }
                    };
                if let Some(state) = state_opt {
                    let display_text = state.display_name.clone();
                    let notif_bits = state.notifications;
                    batch(|| {
                        nav.set(state.nav_index);
                        dark.set(state.dark_mode);
                        scale.set(state.font_scale);
                        name.set(state.display_name);
                        for (i, sig) in notif.signals.iter().enumerate() {
                            sig.set(notif_bits[i]);
                        }
                    });
                    // R668 §5.38 — push persisted font scale into
                    // the text_scale substrate.
                    let target_scale =
                        0.5_f32 + state.font_scale.clamp(0.0, 1.0) * 1.5_f32;
                    pinion_core::text_scale::set_text_scale(target_scale);
                    display_name_text_state.set_text(display_text);
                }
            }
            // (2) Install save Effect. Subscribes to dark + name +
            // each notification Signal so any flip triggers a re-
            // serialise. font_scale Cell + nav_index Cell stay
            // outside the reactive graph; the dark / name re-fire
            // tricks in `update` flush them through this Effect.
            let storage_e = storage.clone();
            let nav_e = nav.clone();
            let dark_e = dark.clone();
            let scale_e = scale.clone();
            let name_e = name.clone();
            let notif_e = notif.clone();
            let save_effect = Effect::new(&owner_for_effect, move || {
                let snap = SettingsPersistedState::snapshot(
                    &nav_e, &dark_e, &scale_e, &name_e, &notif_e.signals,
                );
                match serde_json::to_vec(&snap) {
                    Ok(bytes) => storage_e.save(STORAGE_STATE_KEY, &bytes),
                    Err(e) => eprintln!(
                        "settings-panel: persistence serialize failed ({e}); skipped",
                    ),
                }
            });
            PersistenceBootMarker { _save_effect: save_effect }
        })
}

// ─── nav rail (RadioGroup walker) ─────────────────────────────────

/// Per-radio interaction state for the nav rail. Mirrors todomvc's
/// `FilterRadioStates` shape.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NavRadioStates {
    pub states: [RadioState; NAV_COUNT],
    pub selected: usize,
}

impl Default for NavRadioStates {
    fn default() -> Self {
        Self {
            states: [RadioState::Idle; NAV_COUNT],
            selected: 0,
        }
    }
}

fn read_nav_radio_states(scene: &Scene) -> NavRadioStates {
    let mut out = NavRadioStates::default();
    let Some(node) = scene.find_external_with_tag(NAV_TAG) else {
        return out;
    };
    let Some(intro) = node.handle.introspect() else {
        return out;
    };
    if let Some(IntrospectValue::Int(sel)) = intro.query("selected_index") {
        if let Ok(s) = usize::try_from(sel) {
            out.selected = s.min(NAV_COUNT - 1);
        }
    }
    for (i, slot) in out.states.iter_mut().enumerate() {
        let key = format!("state.{i}");
        if let Some(IntrospectValue::Text(s)) = intro.query(&key) {
            *slot = RadioState::from_name_or_default(&s);
        }
    }
    out
}

// ─── theme toggle walker ──────────────────────────────────────────

fn read_theme_toggle(scene: &Scene) -> (ToggleState, bool) {
    let Some(node) = scene.find_external_with_tag(THEME_TOGGLE_TAG) else {
        return (ToggleState::Idle, false);
    };
    let Some(intro) = node.handle.introspect() else {
        return (ToggleState::Idle, false);
    };
    let state = intro
        .query("state")
        .and_then(|v| match v {
            IntrospectValue::Text(s) => Some(ToggleState::from_name_or_default(&s)),
            _ => None,
        })
        .unwrap_or(ToggleState::Idle);
    let on = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
    (state, on)
}

// ─── slider walker ────────────────────────────────────────────────

#[allow(
    clippy::cast_possible_truncation,
    reason = "f64 → f32 narrowing is intentional; slider value is in [0.0, 1.0]"
)]
fn read_font_slider(scene: &Scene) -> (SliderState, f32) {
    let Some(node) = scene.find_external_with_tag(FONT_SLIDER_TAG) else {
        return (SliderState::Idle, DEFAULT_FONT_SCALE);
    };
    let Some(intro) = node.handle.introspect() else {
        return (SliderState::Idle, DEFAULT_FONT_SCALE);
    };
    let state = intro
        .query("state")
        .and_then(|v| match v {
            IntrospectValue::Text(s) => Some(SliderState::from_name_or_default(&s)),
            _ => None,
        })
        .unwrap_or(SliderState::Idle);
    let value = intro
        .query("value")
        .and_then(|v| match v {
            IntrospectValue::Float(f) => Some(f as f32),
            _ => None,
        })
        .unwrap_or(DEFAULT_FONT_SCALE);
    (state, value.clamp(0.0, 1.0))
}

// ─── M3 state-layer overlays (mirror hello-*) ─────────────────────

fn nav_row_fill(theme: &Theme, state: RadioState, is_selected: bool) -> Color {
    if is_selected {
        // M3 NavigationRailItem active indicator = secondaryContainer
        // (closest mapped role is SurfaceContainerHighest).
        let base = theme.resolve(ColorRole::SurfaceContainerHighest);
        match state {
            RadioState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
            RadioState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
            _ => base,
        }
    } else {
        match state {
            RadioState::Hover => theme
                .resolve(ColorRole::Surface)
                .lerp(theme.resolve(ColorRole::OnSurface), 0.08),
            RadioState::Pressed => theme
                .resolve(ColorRole::Surface)
                .lerp(theme.resolve(ColorRole::OnSurface), 0.12),
            _ => Color::TRANSPARENT,
        }
    }
}

fn toggle_track_fill(theme: &Theme, state: ToggleState, on: bool) -> Color {
    if on {
        match state {
            ToggleState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
            _ => theme.resolve(ColorRole::Accent),
        }
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    }
}

fn slider_accent(theme: &Theme, state: SliderState) -> Color {
    let base = theme.resolve(ColorRole::Accent);
    match state {
        SliderState::Idle => base,
        SliderState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        SliderState::Dragging => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        SliderState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

// ─── section labels ───────────────────────────────────────────────

const SECTION_LABELS: [&str; NAV_COUNT] = [
    "Theme",
    "Appearance",
    "Profile",
    "Notifications",
    "Actions",
];

// ─── nav rail view ────────────────────────────────────────────────

fn view_nav_rail(theme: &Theme, nav: &NavRadioStates) -> Scene {
    let mut rows: Vec<Scene> = Vec::with_capacity(NAV_COUNT);
    for (i, label) in SECTION_LABELS.iter().enumerate() {
        let st = nav.states[i];
        let selected = nav.selected == i;
        let fill = nav_row_fill(theme, st, selected);
        let fg = if selected {
            theme.resolve(ColorRole::OnSurface)
        } else {
            theme.resolve(ColorRole::OnSurfaceMuted)
        };
        let label_text = Scene::Text(TextNode::styled(
            *label,
            Rect::default(),
            TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(fg),
        ));
        let row = Scene::Container(
            ContainerNode::new(vec![label_text])
                // RadioGroupExternal addresses children by composite
                // tag (`<group>#<index>`), so the per-row hit-test
                // tag must follow that shape.
                .with_tag(format!("{NAV_TAG}#{i}"))
                .with_style(BoxStyle::filled(fill).with_corner_radius(NAV_ROW_H / 2))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Start)
                        .with_align_items(AlignItems::Center)
                        .with_padding(Rect::new(
                            NAV_ROW_PAD,
                            0,
                            NAV_ROW_PAD,
                            0,
                        ))
                        .with_size(Size::px(NAV_W - 2 * NAV_GAP, NAV_ROW_H)),
                ),
        );
        rows.push(row);
    }
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(NAV_TAG)
            .with_aria_label("Settings sections")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(
                Border::new(theme.resolve(ColorRole::Outline), 1),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(NAV_GAP, NAV_GAP, NAV_GAP, NAV_GAP))
                    .with_gap(NAV_GAP)
                    .with_size(Size::px(NAV_W, WIN_H)),
            ),
    )
}

// ─── per-section detail views ─────────────────────────────────────

fn view_theme_section(theme: &Theme, toggle_state: ToggleState, on: bool) -> Scene {
    let title = Scene::Text(TextNode::styled(
        "Appearance — Dark mode",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let track_fill = toggle_track_fill(theme, toggle_state, on);
    let knob_fill = match toggle_state {
        ToggleState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
        _ if on => theme.resolve(ColorRole::OnAccent),
        _ => theme.resolve(ColorRole::Outline),
    };
    let knob = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(knob_fill).with_corner_radius(TOGGLE_KNOB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(TOGGLE_KNOB_SIZE, TOGGLE_KNOB_SIZE))),
    );
    let track = Scene::Container(
        ContainerNode::new(vec![knob])
            .with_tag(THEME_TOGGLE_TAG)
            .with_aria_label("Dark mode")
            .with_style(BoxStyle::filled(track_fill).with_corner_radius(TOGGLE_TRACK_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(if on { JustifyContent::End } else { JustifyContent::Start })
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(TOGGLE_TRACK_W, TOGGLE_TRACK_H))
                    .with_padding(Rect::new(
                        TOGGLE_TRACK_PAD,
                        TOGGLE_TRACK_PAD,
                        TOGGLE_TRACK_PAD,
                        TOGGLE_TRACK_PAD,
                    )),
            ),
    );
    let status = Scene::Text(TextNode::styled(
        if on { "Dark mode is on" } else { "Light mode is on" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, track, status])
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    )
}

fn view_appearance_section(theme: &Theme, state: SliderState, value: f32) -> Scene {
    let title = Scene::Text(TextNode::styled(
        "Appearance — Font scale",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let filled_color = slider_accent(theme, state);
    let unfilled_color = match state {
        SliderState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(theme.resolve(ColorRole::Surface), 0.38),
        _ => theme.resolve(ColorRole::SurfaceContainerHighest),
    };
    let on_accent = theme.resolve(ColorRole::OnAccent);
    let thumb_fill = match state {
        SliderState::Idle | SliderState::Hover => on_accent,
        SliderState::Dragging => on_accent.lerp(theme.resolve(ColorRole::Accent), 0.2),
        SliderState::Disabled => on_accent.lerp(theme.resolve(ColorRole::Surface), 0.38),
    };
    let filled_w = scale_normalized_to_px(value, SLIDER_RANGE);
    let unfilled_w = SLIDER_RANGE.saturating_sub(filled_w);
    let filled = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(filled_color).with_corner_radius(SLIDER_TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(filled_w, SLIDER_TRACK_H))),
    );
    let unfilled = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(unfilled_color).with_corner_radius(SLIDER_TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(unfilled_w, SLIDER_TRACK_H))),
    );
    let thumb = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(thumb_fill).with_corner_radius(SLIDER_THUMB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(SLIDER_THUMB_SIZE, SLIDER_THUMB_SIZE))),
    );
    let track = Scene::Container(
        ContainerNode::new(vec![filled, thumb, unfilled])
            .with_tag(FONT_SLIDER_TAG)
            .with_aria_label("Font scale")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(SLIDER_TRACK_W, SLIDER_THUMB_SIZE)),
            ),
    );
    let status = Scene::Text(TextNode::styled(
        format!("Font scale = {:.2}", value.clamp(0.0, 1.0)),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, track, status])
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    )
}

fn view_profile_section(
    theme: &Theme,
    field_state: TextFieldState,
    caret_byte: u32,
) -> Scene {
    let title = Scene::Text(TextNode::styled(
        "Profile — Display name",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let style = TextFieldStyle {
        field_w: PROFILE_FIELD_W,
        field_h: PROFILE_FIELD_H,
        ..TextFieldStyle::m3_filled()
    };
    let field = tf_paint::view_field(
        PROFILE_TF_TAG,
        field_state,
        caret_byte,
        theme,
        &style,
        "Your display name",
    );
    let hint = Scene::Text(TextNode::styled(
        "Saved automatically as you type",
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, field, hint])
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// (R669 §5.15 §5.45 §5.50) — 6-channel Notifications section. Pins
/// the M3 a11y "Notifications" pattern (Apple HIG / Material 3 list-
/// of-switches), composing one [`view_checkbox`] row per channel
/// inside a [`Scene::Scroll`] viewport so an arbitrary channel
/// count is supported without breaking the detail-pane layout. The
/// 4th [`ScrollBarExternal`] consumer rides this section's viewport.
///
/// `interactions` carries each channel's SCXML statechart projection
/// (Idle / Hover / Pressed / Disabled) freshly walked from the
/// state scene by [`read_notification_states`]; `checked_bits`
/// carries the per-channel `bool` from the channel's
/// `use_notification_channels().signals[i]` (the reactive ground
/// truth — the External's `value` slot mirrors it via the seed in
/// [`SettingsPanelView::create_extra_externals`] + the `value_changing`
/// reducer in [`SettingsPanelView::update`]).
fn view_notifications_section(
    theme: &Theme,
    interactions: [CheckboxState; NOTIFICATION_COUNT],
    checked_bits: [bool; NOTIFICATION_COUNT],
) -> Scene {
    let title = Scene::Text(TextNode::styled(
        "Notifications",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let checkbox_style = CheckboxStyle::m3_filled();
    let rows: Vec<Scene> = NOTIF_INSTANCE_TAGS
        .iter()
        .enumerate()
        .map(|(i, tag)| {
            view_checkbox(
                tag,
                interactions[i],
                checked_bits[i],
                theme,
                &checkbox_style,
                NOTIFICATION_LABELS[i],
            )
        })
        .collect();
    let list = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(NOTIF_ROW_GAP),
        ),
    );
    let scroll_state = use_notif_scrollbar();
    let viewport = Scene::Scroll(ScrollNode::from_state(
        scroll_state.clone(),
        Rect::new(0, 0, NOTIF_VIEWPORT_W, NOTIF_VIEWPORT_H),
        list,
    ));
    let scrollbar_interaction = use_scrollbar_interaction(NOTIF_SCROLLBAR_TAG);
    let scrollbar_style =
        VerticalScrollbarStyle::material(NOTIF_VIEWPORT_H, NOTIF_SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );
    let scroll_region = Scene::Container(
        ContainerNode::new(vec![viewport, scrollbar_visual])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );
    Scene::Container(
        ContainerNode::new(vec![title, scroll_region])
            .with_tag(NOTIF_TAG_BASE)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// (R669) M3-defined viewport dimensions for the notifications
/// scrollable list. 6 channels × (~48 px row + 8 px gap) ≈ 336 px;
/// viewport at 280 px guarantees overflow on every Phase A render
/// so the 4th [`ScrollBarExternal`] consumer is exercised.
const NOTIF_VIEWPORT_W: u32 = 480;
const NOTIF_VIEWPORT_H: u32 = 280;
/// Gap between notifications rows in logical pixels. Tighter than
/// the section-level [`ROW_GAP`] because the rows themselves carry
/// the M3 checkbox row's internal padding.
const NOTIF_ROW_GAP: u32 = 8;

/// (R669 §5.15) — read the per-channel SCXML state for the 6
/// `CheckboxExternal`s from the live state scene. The composite-tag
/// substrate stores each channel under its composite path
/// ([`NOTIF_INSTANCE_TAGS`]); [`Scene::find_external_with_tag`]
/// resolves the per-channel handle and the standard introspect
/// "state" slot carries the SCXML projection.
///
/// Returns `Idle` for channels whose External is not yet wired
/// (during the create-then-paint window) or whose introspect channel
/// is opted out. The bool sidecar lives in the
/// [`use_notification_channels`] Signal handles — view fn reads it
/// from there, not from the external — so this walker only carries
/// the interaction-state half.
fn read_notification_states(
    scene: &Scene,
) -> [CheckboxState; NOTIFICATION_COUNT] {
    let mut out = [CheckboxState::Idle; NOTIFICATION_COUNT];
    for (i, tag) in NOTIF_INSTANCE_TAGS.iter().enumerate() {
        if let Some(node) = scene.find_external_with_tag(tag) {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Text(s)) = intro.query("state") {
                    out[i] = CheckboxState::from_name_or_default(&s);
                }
            }
        }
    }
    out
}

/// (R669 §5.15) — read the per-channel `checked` value sidecar
/// directly from each [`CheckboxExternal`]'s `value` slot. The
/// SCXML statechart is the post-click source of truth (the
/// statechart's `Click` arc flips the slot internally); the
/// reactive [`use_notification_channels`] Signal handles mirror it
/// via [`SettingsPanelView::update`] only after the intent fires.
/// Reading from the External in [`SettingsPanelView::read_state`]
/// keeps the path Owner-scope-free (mirrors
/// [`read_notification_states`]) since the framework does not wrap
/// `read_state` in `root_owner.run` per the canonical
/// non-reactive-snapshot contract.
fn read_notification_checked(
    scene: &Scene,
) -> [bool; NOTIFICATION_COUNT] {
    let mut out = [false; NOTIFICATION_COUNT];
    for (i, tag) in NOTIF_INSTANCE_TAGS.iter().enumerate() {
        if let Some(node) = scene.find_external_with_tag(tag) {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Bool(v)) = intro.query("value") {
                    out[i] = v;
                }
            }
        }
    }
    out
}

fn view_actions_section(theme: &Theme) -> Scene {
    let title = Scene::Text(TextNode::styled(
        "Actions",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let status = Scene::Text(TextNode::styled(
        "Settings are saved immediately. No Apply or Cancel needed.",
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let reset_hint = Scene::Text(TextNode::styled(
        "To reset, delete the app's data directory (see PINION_STORAGE_DIR env).",
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, status, reset_hint]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(ROW_GAP),
        ),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "10 state slots match the root tuple — see RootState"
)]
fn view_detail_pane(
    theme: &Theme,
    nav_index: usize,
    toggle_state: ToggleState,
    on: bool,
    slider_state: SliderState,
    slider_value: f32,
    field_state: TextFieldState,
    caret_byte: u32,
    notif_states: [CheckboxState; NOTIFICATION_COUNT],
    notif_checked: [bool; NOTIFICATION_COUNT],
) -> Scene {
    let body = match nav_index {
        0 => view_theme_section(theme, toggle_state, on),
        1 => view_appearance_section(theme, slider_state, slider_value),
        2 => view_profile_section(theme, field_state, caret_byte),
        3 => view_notifications_section(theme, notif_states, notif_checked),
        // The nav `RadioGroup` snaps to a valid index; the `_` arm
        // covers section index 4 (Actions) plus `u32::MAX` overflow.
        _ => view_actions_section(theme),
    };
    Scene::Container(
        ContainerNode::new(vec![body])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_padding(Rect::new(SECTION_PAD, SECTION_PAD, SECTION_PAD, SECTION_PAD))
                    // (R667 §5.21) flex_grow=1.0 stretches the detail
                    // pane to fill the remaining main-axis width
                    // (window width minus NAV_W). The outer Container
                    // (root view fn) anchors WIN_H × WIN_W explicitly
                    // so the vertical axis is bounded.
                    .with_flex_grow(1.0)
                    .with_size(Size::px(WIN_W - NAV_W, WIN_H)),
            ),
    )
}

// ─── root view fn ─────────────────────────────────────────────────

type RootState = (
    TextFieldState,
    u32,
    NavRadioStates,
    ToggleState,
    bool,
    SliderState,
    f32,
    // R669 §5.15 — per-channel notifications projection. SCXML state
    // array walked from the live state scene; bool array snapshot
    // from the [`use_notification_channels`] Signal handles.
    [CheckboxState; NOTIFICATION_COUNT],
    [bool; NOTIFICATION_COUNT],
);

#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_arguments)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (
        field_state,
        caret_byte,
        nav,
        toggle_state,
        on,
        slider_state,
        slider_value,
        notif_states,
        notif_checked,
    ) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    // R668 §5.38 — subscribe the view fn to the a11y text-scale
    // signal so dragging the slider live-previews every
    // `TextStyle::with_size_px` call in the subsequent paint walk.
    // The `.get()` read enters the reactive scope; the subsequent
    // `with_size_px` calls (inside both this view fn and the lifted
    // widget-paint substrate modules) re-read the thread-local
    // mirror per [[r668-text-scale-multiplier]]. The bound value
    // itself is intentionally unused — only the subscribe side-
    // effect matters.
    let _scale_subscription = pinion_core::text_scale::use_text_scale().get();
    let nav_pane = view_nav_rail(&theme, &nav);
    let detail = view_detail_pane(
        &theme,
        nav.selected,
        toggle_state,
        on,
        slider_state,
        slider_value,
        field_state,
        caret_byte,
        notif_states,
        notif_checked,
    );
    Scene::Container(
        ContainerNode::new(vec![nav_pane, detail])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── intent tags (R57.X.intent_tag!) ──────────────────────────────

const NAV_SELECTED_INTENT_TAG: &str = intent_tag!("nav_rail", "selected");
const THEME_TOGGLE_INTENT_TAG: &str = intent_tag!("theme_toggle", "toggle");

// ─── apply_key arms ───────────────────────────────────────────────

fn apply_key_profile(scene: &mut Scene, key: &str) -> bool {
    if key == "Enter" {
        let text_state = use_text_edit_state(PROFILE_TF_TAG);
        let name = use_display_name();
        name.set(text_state.text());
        return true;
    }
    let Some(node) = scene.find_external_with_tag_mut(PROFILE_TF_TAG) else {
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

fn apply_key_nav(scene: &mut Scene, key: &str) -> bool {
    let nav_state = read_nav_radio_states(scene);
    let current = nav_state.selected;
    let target = match key {
        "ArrowUp" => current.checked_sub(1).unwrap_or(NAV_COUNT - 1),
        "ArrowDown" => {
            if current + 1 >= NAV_COUNT {
                0
            } else {
                current + 1
            }
        }
        "Home" => 0,
        "End" => NAV_COUNT - 1,
        "Space" | "Enter" => current,
        _ => return false,
    };
    let Some(node) = scene.find_external_with_tag_mut(NAV_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        let _ = intro.invoke("send", IntrospectValue::Text(format!("{target}:{ev}")));
    }
    true
}

// ─── WidgetCore impl ──────────────────────────────────────────────

pub struct SettingsPanelView;

impl WidgetCore for SettingsPanelView {
    type State = RootState;
    type Event = TextFieldEvent;

    fn create_external() -> Box<dyn External> {
        let text_state = use_text_edit_state(PROFILE_TF_TAG);
        let blink = use_caret_blink(PROFILE_TF_TAG);
        let clipboard = use_clipboard(PROFILE_TF_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
        )
    }

    fn tag() -> &'static str {
        PROFILE_TF_TAG
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        let _persistence = use_settings_persistence();
        let nav_idx = use_nav_index();
        let dark = use_dark_mode();
        let scale = use_font_scale();
        let notif = use_notification_channels();

        // Seed the nav RadioGroup with the persisted index so the
        // first paint highlights the right section.
        let mut nav_group = RadioGroupExternal::new(NAV_COUNT);
        let _ = nav_group.intervene(
            "selected_index",
            IntrospectValue::Int(i64::from(nav_idx.get())),
        );

        // Seed the theme toggle with the persisted dark-mode flag.
        let mut theme_toggle = ToggleExternal::new();
        if dark.get() {
            let _ = theme_toggle.intervene("value", IntrospectValue::Bool(true));
            // Also push the persisted mode into the active provider
            // so the first paint reads the right palette without
            // waiting for the user to flip the toggle.
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
        }

        // Seed the slider with the persisted font scale.
        let mut font_slider = SliderExternal::new();
        let _ = font_slider.intervene(
            "value",
            IntrospectValue::Float(f64::from(scale.get().clamp(0.0, 1.0))),
        );

        // R669 §5.15 — 6 CheckboxExternal instances, one per
        // notifications channel. composite-tag `notifications#0`..
        // `notifications#5` so the input router dispatches per
        // channel independently while the section's root container
        // still carries the cluster's collective `notifications`
        // tag for scrollbar viewport hit-tests.
        //
        // Each external seeded with the hydrated Signal value so the
        // first paint paints the right state (checked vs unchecked)
        // even before any user interaction. CheckboxExternal
        // canonical `value` slot mirrors the bool sidecar; the
        // R654 [[r653-state-flags-bool-field]] retrofit makes this
        // the single-source-of-truth path.
        let mut notif_externals: Vec<ExtraExternal> =
            Vec::with_capacity(NOTIFICATION_COUNT);
        for (i, tag) in NOTIF_INSTANCE_TAGS.iter().enumerate() {
            let mut ext = CheckboxExternal::new();
            let _ = ext.intervene(
                "value",
                IntrospectValue::Bool(notif.signals[i].get()),
            );
            // R55.D.5 composite-tag: shell paint router walks the
            // base tag + parses the suffix per-row. (R688) The const
            // `&'static str` becomes a `Cow::Borrowed` in
            // `ExtraExternal::new` — still no runtime allocation.
            notif_externals.push(ExtraExternal::new(*tag, Box::new(ext)));
        }

        // R669 §5.45 — 4th `ScrollBarExternal` consumer (notifications
        // viewport overflow). attach_state binds the same
        // `ScrollState` the section's `view_vertical_scrollbar` paint
        // walker reads, so drag interactions on the visible thumb
        // mutate the shared `ScrollState::offset` slot and the next
        // paint walks the new offset. attach_interaction wires the
        // M3 thumb state-layer ramp (idle → hover → dragging).
        let notif_scroll_state = use_notif_scrollbar();
        let notif_scrollbar = ScrollBarExternal::new()
            .attach_state(notif_scroll_state)
            .attach_interaction(use_scrollbar_interaction(NOTIF_SCROLLBAR_TAG));

        let mut all = vec![
            ExtraExternal::new(NAV_TAG, Box::new(nav_group)),
            ExtraExternal::new(THEME_TOGGLE_TAG, Box::new(theme_toggle)),
            ExtraExternal::new(FONT_SLIDER_TAG, Box::new(font_slider)),
            ExtraExternal::new(NOTIF_SCROLLBAR_TAG, Box::new(notif_scrollbar)),
        ];
        all.append(&mut notif_externals);
        all
    }

    fn read_state(scene: &Scene) -> RootState {
        let (field_state, caret) = tf_paint::read_text_field_state(scene, PROFILE_TF_TAG);
        let nav = read_nav_radio_states(scene);
        let (toggle_state, on) = read_theme_toggle(scene);
        let (slider_state, slider_value) = read_font_slider(scene);
        // R669 — both halves walked from the live state scene:
        // SCXML interaction state via `read_notification_states`,
        // checked sidecar via `read_notification_checked`. The
        // reactive Signal handles are mirrors that V::update keeps
        // in sync with the External (canonical post-click flip
        // authority); reading from the External here keeps
        // read_state Owner-scope-free per the framework contract.
        let notif_states = read_notification_states(scene);
        let notif_checked = read_notification_checked(scene);
        (
            field_state,
            caret,
            nav,
            toggle_state,
            on,
            slider_state,
            slider_value,
            notif_states,
            notif_checked,
        )
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
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
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion settings-panel (R667 §5.16) — 2nd composed app / Phase A close"
    }

    fn focusable_tags() -> Vec<&'static str> {
        vec![PROFILE_TF_TAG, NAV_TAG]
    }

    fn update(_state: RootState, intent: &Intent) -> Vec<Command> {
        // Nav selection: persist + sync local nav_index Cell so the
        // save Effect picks up the new index (Cell isn't reactive on
        // its own; the snapshot reads it during the next Signal
        // fire — display name / dark mode covers this in v1).
        if intent.tag.as_ref() == NAV_SELECTED_INTENT_TAG {
            if let IntrospectValue::Int(i) = intent.payload {
                if let Ok(idx) = u32::try_from(i) {
                    use_nav_index().set(idx);
                    // Re-fire the dark mode Signal to flush the new
                    // nav_index through the save Effect (Cell isn't
                    // reactive; piggybacking on dark mode's Signal
                    // write is the simplest re-trigger until a 2nd
                    // non-reactive store needs a dedicated signal —
                    // YAGNI per [[abstraction-needs-second-consumer]]).
                    let dark = use_dark_mode();
                    dark.set(dark.get());
                }
            }
        }
        // Theme toggle: swap the provider's mode so the whole window
        // re-themes. Authority = intent.payload per
        // [[intent-payload-post-flip-authority]].
        if intent.tag.as_ref() == THEME_TOGGLE_INTENT_TAG {
            if let IntrospectValue::Bool(on) = intent.payload {
                use_theme(THEME_TAG).set_mode(if on { ThemeMode::Dark } else { ThemeMode::Light });
                use_dark_mode().set(on);
            }
        }
        // Slider value: persist on every value_changing fire. Cell
        // write doesn't subscribe view; the slider's own paint
        // re-renders through its `value` slot read.
        if intent.tag.as_ref() == intent_tag!("font_slider", "value_changing")
            || intent.tag.as_ref() == intent_tag!("font_slider", "value_committed")
        {
            if let IntrospectValue::Float(v) = intent.payload {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "slider value normalised to [0.0, 1.0]"
                )]
                let v32 = v as f32;
                use_font_scale().set(v32);
                // R668 §5.38 — wire the slider into the new
                // [`pinion_core::text_scale::use_text_scale`] substrate
                // so dragging the slider live-previews the a11y
                // text-scale multiplier across every paint site (every
                // TextStyle::with_size_px call now multiplies by the
                // thread-local mirror, and view fns subscribed to
                // use_text_scale().get() re-run on the signal fire).
                // Mapping: slider in [0.0, 1.0] → text_scale in
                // [0.5, 2.0] (linear), the M3 a11y range covering
                // 50 %–200 % zoom — large enough that the live
                // preview is visually obvious yet still inside the
                // 0.1..5.0 safety clamp.
                let target_scale = 0.5_f32 + v32.clamp(0.0, 1.0) * 1.5_f32;
                pinion_core::text_scale::use_text_scale().set(target_scale);
                // Re-fire dark mode Signal to flush snapshot (same
                // trick as nav_index above).
                let dark = use_dark_mode();
                dark.set(dark.get());
            }
        }
        // R669 §5.15 — Notifications channel toggle. CheckboxExternal
        // canonical `checked` intent fires on PointerUp /
        // KeyboardActivate inside the SCXML statechart; the payload
        // carries the post-flip bool ([[intent-payload-post-flip-authority]]).
        // The intent tag has the composite-tag shape
        // `notifications#{i}.checked` per
        // [[composite-tag-parse_send_payload]].
        for (i, tag) in NOTIF_INSTANCE_TAGS.iter().enumerate() {
            // The intent_tag! macro requires `&'static str` literals
            // so we build the composite tag at runtime + compare.
            // O(NOTIFICATION_COUNT) per intent dispatch — N=6, no
            // measurable overhead. `(*tag)` deref + format! holds the
            // composite shape `notifications#{i}.checked` matching
            // [[composite-tag-parse_send_payload]].
            let expected = format!("{tag}.checked");
            if intent.tag.as_ref() == expected {
                if let IntrospectValue::Bool(checked) = intent.payload {
                    let notif = use_notification_channels();
                    notif.signals[i].set(checked);
                }
                break;
            }
        }
        Vec::new()
    }

    fn keybinding(_key: &str) -> Option<TextFieldEvent> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        match focused {
            Some(PROFILE_TF_TAG) => apply_key_profile(scene, key),
            Some(NAV_TAG) => apply_key_nav(scene, key),
            _ => false,
        }
    }

    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(PROFILE_TF_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(PROFILE_TF_TAG) else {
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
            // `CompositionEvent` is non_exhaustive — collapse unknown
            // future variants into the same `cancel` semantics so an
            // IME spec extension never leaves the field mid-edit.
            _ => IntrospectValue::Json(serde_json::json!({ "action": "cancel" })),
        };
        intro.invoke("composition", args).is_ok()
    }
}

impl WidgetA11y for SettingsPanelView {
    // Default `access_node` returns Vec::new() — AT-invisible for v1.
    // Real ARIA wiring carries to R668 alongside the interactive
    // notifications/actions widgets.
}

impl WidgetView for SettingsPanelView {
    type Renderer = SettingsPanelRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<SettingsPanelView>();
}
