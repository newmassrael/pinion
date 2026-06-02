//! `hello-datepicker` — R704 §5.38 inline **date picker**: the GUI
//! consumer of the R704
//! [`DatePicker`](pinion_core::widgets::datepicker::DatePicker)
//! single-day-selection coordinator + the §5.50
//! [`pinion_widget_paint::datepicker`] month-grid paint.
//!
//! A date picker presents one month as a 7-column calendar grid (the
//! WAI-ARIA `grid` role with `gridcell` children) plus a header carrying
//! previous / next-month navigation. Exactly one day may be selected;
//! activating a day replaces the previous selection (single-day
//! exclusion — framework-owned in the coordinator, like a `RadioGroup`'s
//! sibling-deselect). Navigation rolls the displayed month with year
//! rollover and leaves the selection untouched.
//!
//! ## Composition — one composite External
//!
//! The state scene holds **one** [`DatePickerExternal`] at the
//! [`PRIMARY_TAG`] composite paint root (mirroring `hello-radio-group`
//! over [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)).
//! Each day cell is tagged `"datepicker#<day>"` (the R51.41 composite
//! paint convention), so the
//! [`InputRouter`](pinion_runtime::input::InputRouter) R51.42 `'#'`-split
//! routes a click on day `d` to
//! `invoke("send", Text("<d>:<EventName>"))` against the single
//! coordinator, which enforces single-day exclusion on the activate edge
//! and emits the §5.20 `"selected"` intent (the new day-of-month).
//!
//! ## Keyboard model (WAI-ARIA APG date grid)
//!
//! The grid is a **single Tab stop** ([`focusable_tags`] returns the grid
//! root + the two nav buttons), and the focused day is an internal roving
//! *active descendant* — the coordinator's `focused_day` slot — not a
//! shell-focus tag. This is the WAI-ARIA grid pattern (single tab stop +
//! `aria-activedescendant`), the same model `hello-radio-group` uses, and
//! it keeps the boot-seeded `focusable_tags` enumeration stable across
//! month changes (no month-varying per-day Tab stops). While the grid root
//! owns shell focus:
//! - `ArrowLeft` / `ArrowRight` move the active descendant `∓1` / `±1`,
//!   **clamped within the displayed month** (no month-crossing on arrow
//!   in this first slice — see the deferral note below); from no active
//!   descendant, the first arrow lands on day 1;
//! - `ArrowUp` / `ArrowDown` move `∓7` / `±7`, clamped;
//! - `Home` / `End` jump to day 1 / the last day of the month;
//! - `PageUp` / `PageDown` roll to the previous / next month (through the
//!   coordinator's `"PrevMonth"` / `"NextMonth"` wire); the coordinator
//!   clamps the active descendant into the new (possibly shorter) month;
//! - `Enter` / `Space` activate (select) the active-descendant day.
//!
//! The active descendant moves through the coordinator's `focused_day`
//! intervene slot (the date-grid mirror of `RadioGroup`'s `focused_index`)
//! so the visible focus ring, the AT `aria-activedescendant`, and the RPC
//! `focused_day` query stay in lockstep. A mouse click on a day cell
//! routes through the input router's `'#'`-split to the coordinator's
//! `"<d>:<EventName>"` send, whose activate edge syncs `focused_day` to
//! the clicked day.
//!
//! **Deferred axis**: month-crossing arrow navigation (`ArrowRight` on the
//! last day rolling to the next month's day 1, etc.) is a standard APG
//! refinement but adds month-roll-on-arrow coupling; this first slice
//! clamps arrows within the displayed month and offers PageUp/PageDown
//! for explicit month navigation. The coordinator already supports the
//! roll, so the axis is purely a binding-side keyboard extension.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::datepicker::{
    days_in_month, CivilDate, DatePickerExternal,
};
use pinion_core::widgets::radio::RadioState;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::datepicker::{
    view_datepicker, DatePickerStyle, DisplayedMonth, MONTH_NAMES, WEEKDAY_NAMES,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDatePickerRenderer, HelloDatePickerRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 420;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Composite paint root tag carrying the single [`DatePickerExternal`].
/// Day cells are tagged `"datepicker#<day>"`; the nav buttons are
/// `"datepicker#prev"` / `"datepicker#next"` (composite `'#'` sub-tags so
/// a click routes to the coordinator's `"prev:"` / `"next:"` send).
const PRIMARY_TAG: &str = "datepicker";
const PREV_TAG: &str = "datepicker#prev";
const NEXT_TAG: &str = "datepicker#next";

/// Fixed deterministic initial month: May 2026, no initial selection.
const INITIAL_YEAR: i32 = 2026;
const INITIAL_MONTH: u8 = 5;

/// Maximum days in any Gregorian month — the fixed width of the per-day
/// interaction array, so [`PickerState`] stays `Copy` (the
/// [`pinion_core::WidgetCore::State`] bound) instead of carrying a heap
/// `Vec`. Mirrors `hello-radio-group`'s `[(RadioState, bool); N]`.
const MAX_DAYS: usize = 31;

/// Cached projection of the picker: the displayed month coordinates, the
/// selected date, and one [`RadioState`] per day of the displayed month
/// (indexed by `day − 1`; slots beyond the month's day count stay
/// [`RadioState::Idle`]). Read from the single [`DatePickerExternal`]'s
/// introspect slots. `Copy` (fixed-size array) so the shell hands the
/// snapshot into the `paint_producer` closure without lifetime gymnastics
/// — the `hello-radio-group` `GroupState` convention.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PickerState {
    year: i32,
    month: u8,
    selected: Option<CivilDate>,
    /// R704 §5.40 — the roving active descendant (1-based day), or `None`.
    /// The grid is a single Tab stop; this is the day the keyboard cursor
    /// addresses *inside* the grid (the date-grid mirror of
    /// `hello-radio-group`'s `GroupState::focused`). Read from the
    /// coordinator's `focused_day` introspect slot.
    focused_day: Option<u8>,
    day_states: [RadioState; MAX_DAYS],
}

impl PickerState {
    fn idle(year: i32, month: u8) -> Self {
        Self {
            year,
            month,
            selected: None,
            focused_day: None,
            day_states: [RadioState::Idle; MAX_DAYS],
        }
    }

    fn day_state(&self, day: u8) -> RadioState {
        if day < 1 {
            return RadioState::Idle;
        }
        self.day_states
            .get(usize::from(day - 1))
            .copied()
            .unwrap_or(RadioState::Idle)
    }

    /// The interaction-state slice for the displayed month's days,
    /// `[..days_in_month]`, indexed by `day − 1` — the shape
    /// [`view_datepicker`] expects.
    fn day_state_slice(&self) -> &[RadioState] {
        let days = usize::from(days_in_month(self.year, self.month));
        &self.day_states[..days.min(MAX_DAYS)]
    }
}

/// Read the live `focused_day` active descendant from the picker's
/// introspect surface (`-1` / absent → `None`).
fn read_focused_day(intro: &dyn pinion_core::external::ExternalIntrospect) -> Option<u8> {
    match intro.query("focused_day") {
        Some(IntrospectValue::Int(d)) if d >= 1 => u8::try_from(d).ok(),
        _ => None,
    }
}

/// Read the displayed month's day count from the introspect surface
/// (defaults to 28 if unavailable — the safe lower bound).
fn read_days(intro: &dyn pinion_core::external::ExternalIntrospect) -> u8 {
    match intro.query("days") {
        Some(IntrospectValue::Int(d)) => u8::try_from(d).unwrap_or(28),
        _ => 28,
    }
}

/// Move the roving active descendant by `delta` days, clamped within the
/// displayed month. From `None`, an arrow lands on day 1. Writes the new
/// day back through the coordinator's `focused_day` intervene slot.
fn move_focused_day(intro: &mut dyn pinion_core::external::ExternalIntrospect, delta: i32) {
    let days = i32::from(read_days(intro));
    let next = match read_focused_day(intro) {
        // First arrow into the grid lands on day 1 (the WAI-ARIA "focus
        // enters the grid" entry), regardless of direction.
        None => 1,
        Some(current) => (i32::from(current) + delta).clamp(1, days),
    };
    let _ = intro.intervene("focused_day", IntrospectValue::Int(i64::from(next)));
}

/// Set the roving active descendant to a specific day (Home / End),
/// clamped within the displayed month.
fn set_focused_day(intro: &mut dyn pinion_core::external::ExternalIntrospect, day: u8) {
    let days = read_days(intro);
    let clamped = day.clamp(1, days);
    let _ = intro.intervene("focused_day", IntrospectValue::Int(i64::from(clamped)));
}

/// The day reported as the grid's `aria-activedescendant` when the grid
/// owns focus: the roving `focused_day` if set, else the in-view selected
/// day, else day 1 — mirrors `hello-radio-group`'s `active_radio_index`.
/// Clamped into the displayed month so a stale value never points past
/// the month's last day.
fn active_day(state: &PickerState) -> u8 {
    let in_view_selected = state
        .selected
        .filter(|d| d.year == state.year && d.month == state.month)
        .map(|d| d.day);
    let raw = state.focused_day.or(in_view_selected).unwrap_or(1);
    raw.clamp(1, days_in_month(state.year, state.month))
}

/// view-fn (§6.3): pure sync mapping [`PickerState`] -> [`Scene`].
/// Wraps [`view_datepicker`] in a centred surface container, mirroring
/// `hello-accordion-single`'s outer container + theme.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &PickerState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = DatePickerStyle::m3();
    // The keyboard-focus ring is the shell's job: R694 `paint_focus_ring`
    // strokes it around the tag this binding reports focused — the roving
    // active-descendant day cell, resolved through `access_focus_target`
    // (`aria-activedescendant`). That is the same single focus-ring path
    // every other widget uses (`RadioGroup` draws no ring of its own), so
    // the view fn paints no focus state itself — doing so would double the
    // ring against the shell's stroke.
    let picker = view_datepicker(
        PRIMARY_TAG,
        DisplayedMonth { year: state.year, month: state.month },
        state.selected,
        state.day_state_slice(),
        &theme,
        &style,
    );
    Scene::Container(
        ContainerNode::new(vec![picker])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct DatePickerView;

impl WidgetCore for DatePickerView {
    type State = PickerState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's composite `"<d>:<EventName>"` dispatch (pointer),
    // never the shell's enum-typed `keybinding` channel — so `()`
    // satisfies the trait's `Copy` bound without an unused event variant
    // (mirror of `hello-radio-group` / `hello-accordion-single`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(DatePickerExternal::new(INITIAL_YEAR, INITIAL_MONTH, None))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> PickerState {
        let mut out = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // The introspect channel is the single source of truth: an AI
        // client running `scene/query /datepicker/external/year` sees
        // exactly the value the view fn renders.
        out.year = match intro.query("year") {
            Some(IntrospectValue::Int(y)) => i32::try_from(y).unwrap_or(INITIAL_YEAR),
            _ => INITIAL_YEAR,
        };
        out.month = match intro.query("month") {
            Some(IntrospectValue::Int(m)) => u8::try_from(m).unwrap_or(INITIAL_MONTH),
            _ => INITIAL_MONTH,
        };
        out.selected = if matches!(intro.query("selected"), Some(IntrospectValue::Bool(true))) {
            let year = intro.query("selected_year").and_then(|v| match v {
                IntrospectValue::Int(y) => i32::try_from(y).ok(),
                _ => None,
            });
            let month = intro.query("selected_month").and_then(|v| match v {
                IntrospectValue::Int(m) => u8::try_from(m).ok(),
                _ => None,
            });
            let day = intro.query("selected_day").and_then(|v| match v {
                IntrospectValue::Int(d) => u8::try_from(d).ok(),
                _ => None,
            });
            match (year, month, day) {
                (Some(year), Some(month), Some(day)) => Some(CivilDate { year, month, day }),
                _ => None,
            }
        } else {
            None
        };
        out.focused_day = read_focused_day(intro);
        let days = days_in_month(out.year, out.month);
        for d in 1..=days {
            let st = match intro.query(&format!("state.{d}")) {
                Some(IntrospectValue::Text(name)) => RadioState::from_name_or_default(&name),
                _ => RadioState::Idle,
            };
            if let Some(slot) = out.day_states.get_mut(usize::from(d - 1)) {
                *slot = st;
            }
        }
        out
    }

    fn view(state: PickerState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // No typed events reach the keybinding channel; the single source
        // of truth for the keymap lives in `apply_key`.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-datepicker (R704 §5.38 inline month calendar)"
    }

    /// WAI-ARIA APG date grid focus model: the grid is a **single Tab
    /// stop** (`PRIMARY_TAG`), with the focused day tracked as an internal
    /// roving *active descendant* (the coordinator's `focused_day` slot) —
    /// the same single-tab-stop + active-descendant model
    /// `hello-radio-group` uses, and the correct WAI-ARIA grid pattern.
    /// The two month-nav buttons are their own Tab stops. All three tags
    /// are static, so the boot-seeded `focusable_tags` enumeration is
    /// stable across month changes (no phantom per-day Tab stops).
    fn focusable_tags() -> Vec<&'static str> {
        vec![PRIMARY_TAG, PREV_TAG, NEXT_TAG]
    }

    /// WAI-ARIA APG date-grid keyboard model. Routing depends on which Tab
    /// stop owns shell focus:
    /// - `PRIMARY_TAG` (the grid): arrows / Home / End move the roving
    ///   active descendant (`focused_day`) within the displayed month
    ///   (clamped — month-crossing arrows are a deferred axis); `PageUp` /
    ///   `PageDown` roll the month; Enter / Space activate the active
    ///   descendant day.
    /// - `PREV_TAG` / `NEXT_TAG` (the nav buttons): Enter / Space roll the
    ///   month (the keyboard mirror of clicking the composite `#prev` /
    ///   `#next` cell).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // Nav-button activation: Enter / Space on the focused prev / next
        // button rolls the month. (A click reaches the same coordinator
        // via the InputRouter `'#'`-split → `prev:` / `next:` send.)
        if focused == Some(PREV_TAG) || focused == Some(NEXT_TAG) {
            if !matches!(key, "Enter" | "Space") {
                return false;
            }
            let wire = if focused == Some(PREV_TAG) { "PrevMonth" } else { "NextMonth" };
            let Scene::External(node) = scene else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            return intro
                .invoke("send", IntrospectValue::Text(wire.to_string()))
                .is_ok();
        }
        // All other keys route only when the grid itself owns focus.
        if focused != Some(PRIMARY_TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match key {
            // Arrow roving moves the active descendant (clamped within the
            // month). From no active descendant, the first arrow lands on
            // day 1 (the `move_focused_day` / `set_focused_day` default).
            "ArrowLeft" => {
                move_focused_day(intro, -1);
                true
            }
            "ArrowRight" => {
                move_focused_day(intro, 1);
                true
            }
            "ArrowUp" => {
                move_focused_day(intro, -7);
                true
            }
            "ArrowDown" => {
                move_focused_day(intro, 7);
                true
            }
            "Home" => {
                set_focused_day(intro, 1);
                true
            }
            "End" => {
                let last = read_days(intro);
                set_focused_day(intro, last);
                true
            }
            "PageUp" | "PageDown" => {
                let wire = if key == "PageUp" { "PrevMonth" } else { "NextMonth" };
                intro
                    .invoke("send", IntrospectValue::Text(wire.to_string()))
                    .is_ok()
            }
            "Enter" | "Space" => {
                // Activate the active-descendant day. Run the full pointer
                // cycle through the coordinator's wire format so single-day
                // exclusion + the §5.20 intent fire exactly as a click
                // would (the Radio leaf activates on the PointerUp-out-of-
                // Pressed edge, so a lone PointerUp from Idle is a no-op).
                // With no active descendant yet, Enter selects day 1 — the
                // WAI-ARIA "activate the focused cell" default.
                let day = read_focused_day(intro).unwrap_or(1);
                let mut handled = false;
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    handled |= intro
                        .invoke("send", IntrospectValue::Text(format!("{day}:{ev}")))
                        .is_ok();
                }
                handled
            }
            _ => false,
        }
    }

    fn fmt_state_log(state: &PickerState) -> String {
        let sel = state
            .selected
            .map_or_else(|| "none".to_string(), |d| format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));
        format!("{:04}-{:02} sel={sel}", state.year, state.month)
    }
}

impl WidgetA11y for DatePickerView {
    /// R704 §5.40 — composite AccessKit tree contribution. Emits the
    /// `grid` root, 7 `columnheader` weekday nodes, one `gridcell` per
    /// visible day (carrying `aria-selected` + position/size-in-set), and
    /// the two nav buttons. The single-day-selection invariant lives in
    /// the coordinator; at most one gridcell reports
    /// `aria-selected="true"`.
    fn access_node(state: &PickerState, focused: Option<&str>) -> Vec<AccessNode> {
        let days = days_in_month(state.year, state.month);
        let month_name = MONTH_NAMES[usize::from(state.month.clamp(1, 12)) - 1];
        let selected_day = state
            .selected
            .filter(|d| d.year == state.year && d.month == state.month)
            .map(|d| d.day);
        // When the grid owns shell focus, exactly one day cell is the
        // active descendant (the `aria-activedescendant` target). The
        // resolution mirrors `hello-radio-group`: the roving `focused_day`
        // if set, else the in-view selected day, else day 1.
        let grid_focused = focused == Some(PRIMARY_TAG);
        let active_day = active_day(state);

        let mut nodes: Vec<AccessNode> =
            Vec::with_capacity(usize::from(days) + 10);

        // Grid root + weekday column headers as children.
        let mut grid = AccessNode::new(PRIMARY_TAG, AriaRole::Grid)
            .with_name(format!("{month_name} {}", state.year));
        for col in 0..7 {
            grid = grid.with_child(format!("{PRIMARY_TAG}_wh{col}"));
        }
        for day in 1..=days {
            grid = grid.with_child(format!("{PRIMARY_TAG}#{day}"));
        }
        nodes.push(grid);

        for (col, weekday) in WEEKDAY_NAMES.iter().enumerate() {
            nodes.push(
                AccessNode::new(format!("{PRIMARY_TAG}_wh{col}"), AriaRole::ColumnHeader)
                    .with_name(*weekday),
            );
        }

        for day in 1..=days {
            let cell_tag = format!("{PRIMARY_TAG}#{day}");
            let is_selected = selected_day == Some(day);
            let interaction = state.day_state(day);
            let cell_focused = grid_focused && active_day == day;
            nodes.push(
                AccessNode::new(&cell_tag, AriaRole::GridCell)
                    .with_name(format!("{month_name} {day}, {}", state.year))
                    .with_selected(is_selected)
                    .with_position_in_set(u32::from(day))
                    .with_size_of_set(u32::from(days))
                    .with_state(AccessState {
                        focused: cell_focused,
                        ..AccessState::from_interaction(interaction, None)
                    }),
            );
        }

        nodes.push(
            AccessNode::new(PREV_TAG, AriaRole::Button)
                .with_name("Previous month")
                .with_state(AccessState {
                    focused: focused == Some(PREV_TAG),
                    ..AccessState::default()
                }),
        );
        nodes.push(
            AccessNode::new(NEXT_TAG, AriaRole::Button)
                .with_name("Next month")
                .with_state(AccessState {
                    focused: focused == Some(NEXT_TAG),
                    ..AccessState::default()
                }),
        );
        nodes
    }

    /// R704 §5.40 — composite focus model (mirror of `hello-radio-group`).
    /// When the grid owns shell focus, focus stays on the grid root
    /// (`PRIMARY_TAG`) and the active-descendant day cell is reported as
    /// the `aria-activedescendant`. Any other focused tag (a nav button)
    /// passes through atomically.
    fn access_focus_target(state: &PickerState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(PRIMARY_TAG) {
            Some(AccessFocus::composite(
                PRIMARY_TAG,
                format!("{PRIMARY_TAG}#{}", active_day(state)),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R704 §5.40 — composite child action dispatch. A day cell carries a
    /// composite tag (`"datepicker#<n>"`), so an AT `Click` / `Default`
    /// on a day splits at `'#'` and arrives here with the day number.
    /// Activation runs the pointer cycle against the coordinator's wire
    /// format (the same path a real click takes, so single-day exclusion
    /// holds). `Focus` is accepted without mutation; other actions
    /// decline so the shell keeps its fallback chain.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        // Nav buttons carry the composite sub-tags "prev" / "next"; a
        // Click rolls the month through the same coordinator wire the
        // keyboard PageUp/PageDown path uses.
        if matches!(sub_tag, "prev" | "next") {
            return match action {
                AccessAction::Click | AccessAction::Default => {
                    let wire = if sub_tag == "prev" { "PrevMonth" } else { "NextMonth" };
                    intro
                        .invoke("send", IntrospectValue::Text(wire.to_string()))
                        .is_ok()
                }
                AccessAction::Focus => true,
                AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => {
                    false
                }
            };
        }
        let Ok(day) = sub_tag.parse::<u8>() else {
            return false;
        };
        // Bounds-check against the displayed month.
        let days = match intro.query("days") {
            Some(IntrospectValue::Int(d)) => u8::try_from(d).unwrap_or(0),
            _ => 0,
        };
        if day < 1 || day > days {
            return false;
        }
        match action {
            AccessAction::Click | AccessAction::Default => {
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    let _ = intro.invoke("send", IntrospectValue::Text(format!("{day}:{ev}")));
                }
                true
            }
            AccessAction::Focus => true,
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for DatePickerView {
    type Renderer = HelloDatePickerRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<DatePickerView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh picker scene mirroring `create_external` so
    /// `apply_key` / `access_child_invoke` walk the live shell's exact
    /// topology (one composite `Scene::External` at `PRIMARY_TAG`).
    fn scene_fixture() -> Scene {
        Scene::External(
            ExternalNode::new(DatePickerView::create_external()).with_tag(PRIMARY_TAG),
        )
    }

    fn displayed_month(scene: &Scene) -> (i32, u8) {
        let Scene::External(node) = scene else {
            return (0, 0);
        };
        let intro = node.handle.introspect().expect("introspect");
        let year = match intro.query("year") {
            Some(IntrospectValue::Int(y)) => i32::try_from(y).unwrap_or(0),
            _ => 0,
        };
        let month = match intro.query("month") {
            Some(IntrospectValue::Int(m)) => u8::try_from(m).unwrap_or(0),
            _ => 0,
        };
        (year, month)
    }

    fn selected_day(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("selected_day") {
            Some(IntrospectValue::Int(d)) if d >= 1 => Some(d),
            _ => None,
        }
    }

    // ── view / composition ────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DatePickerView>(
            PickerState::idle(INITIAL_YEAR, INITIAL_MONTH),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_day_cell_tag_for_may_2026() {
        let state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        let scene = pinion_core::Owner::new().run(|| view(&state, &Frame::new()));
        for day in 1..=31u8 {
            assert!(scene.contains_tag(&format!("datepicker#{day}")), "day {day} present");
        }
    }

    /// Read the roving active-descendant day from the live scene.
    fn focused_day_of(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("focused_day") {
            Some(IntrospectValue::Int(d)) if d >= 1 => Some(d),
            _ => None,
        }
    }

    /// Drive a key against the grid root (the single Tab stop) on `scene`.
    fn grid_key(scene: &mut Scene, key: &str) -> bool {
        DatePickerView::apply_key(
            scene,
            Some(PRIMARY_TAG),
            key,
            pinion_core::Modifiers::empty(),
        )
    }

    #[test]
    fn focusable_tags_lists_grid_and_nav() {
        assert_eq!(
            DatePickerView::focusable_tags(),
            vec![PRIMARY_TAG, PREV_TAG, NEXT_TAG],
        );
    }

    // ── apply_key roving (active descendant) ──────────────────────

    #[test]
    fn first_arrow_enters_grid_at_day_one() {
        let mut scene = scene_fixture();
        // No active descendant yet -> the first arrow lands on day 1.
        assert!(grid_key(&mut scene, "ArrowRight"));
        assert_eq!(focused_day_of(&scene), Some(1));
    }

    #[test]
    fn arrow_right_then_advances_active_descendant() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "End"); // -> 31
        assert_eq!(focused_day_of(&scene), Some(31));
        grid_key(&mut scene, "Home"); // -> 1
        assert_eq!(focused_day_of(&scene), Some(1));
        grid_key(&mut scene, "ArrowRight"); // -> 2
        assert_eq!(focused_day_of(&scene), Some(2));
    }

    #[test]
    fn arrow_down_adds_seven_clamped() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "End"); // 31 (May)
        // 31 + 7 = 38 -> clamp to 31.
        grid_key(&mut scene, "ArrowDown");
        assert_eq!(focused_day_of(&scene), Some(31));
    }

    #[test]
    fn arrow_left_clamps_at_day_one() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "Home"); // 1
        grid_key(&mut scene, "ArrowLeft"); // clamp at 1
        assert_eq!(focused_day_of(&scene), Some(1));
    }

    #[test]
    fn keys_ignored_when_grid_not_focused() {
        let mut scene = scene_fixture();
        // Background focus / a day-cell tag is not the grid root -> declined.
        assert!(!DatePickerView::apply_key(
            &mut scene,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert!(!DatePickerView::apply_key(
            &mut scene,
            Some("datepicker#5"),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(focused_day_of(&scene), None);
    }

    // ── apply_key activation + month nav ──────────────────────────

    #[test]
    fn enter_activates_active_descendant_day() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "Home"); // -> 1
        grid_key(&mut scene, "ArrowRight"); // -> 2
        grid_key(&mut scene, "ArrowRight"); // -> 3
        assert!(grid_key(&mut scene, "Enter"));
        assert_eq!(selected_day(&scene), Some(3));
    }

    #[test]
    fn space_activates_active_descendant_day() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "End"); // -> 31
        assert!(grid_key(&mut scene, "Space"));
        assert_eq!(selected_day(&scene), Some(31));
    }

    #[test]
    fn enter_with_no_active_descendant_selects_day_one() {
        let mut scene = scene_fixture();
        assert!(grid_key(&mut scene, "Enter"));
        assert_eq!(selected_day(&scene), Some(1));
    }

    #[test]
    fn page_down_then_page_up_round_trips_month() {
        let mut scene = scene_fixture();
        assert!(grid_key(&mut scene, "PageDown"));
        assert_eq!(displayed_month(&scene), (2026, 6));
        assert!(grid_key(&mut scene, "PageUp"));
        assert_eq!(displayed_month(&scene), (2026, 5));
    }

    #[test]
    fn enter_on_next_button_advances_month() {
        let mut scene = scene_fixture();
        assert!(DatePickerView::apply_key(
            &mut scene,
            Some(NEXT_TAG),
            "Enter",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(displayed_month(&scene), (2026, 6));
    }

    #[test]
    fn space_on_prev_button_steps_back_month() {
        let mut scene = scene_fixture();
        assert!(DatePickerView::apply_key(
            &mut scene,
            Some(PREV_TAG),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(displayed_month(&scene), (2026, 4));
    }

    #[test]
    fn active_descendant_clamps_when_month_shortens() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "End"); // May day 31
        grid_key(&mut scene, "PageDown"); // -> June (30 days)
        // The active descendant clamps from 31 to 30.
        assert_eq!(focused_day_of(&scene), Some(30));
    }

    // ── a11y ──────────────────────────────────────────────────────

    #[test]
    fn emits_grid_root_with_weekday_and_day_children() {
        let nodes =
            DatePickerView::access_node(&PickerState::idle(INITIAL_YEAR, INITIAL_MONTH), None);
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].name.as_deref(), Some("May 2026"));
        for col in 0..7 {
            assert_eq!(nodes[1 + col].role, AriaRole::ColumnHeader);
        }
        assert_eq!(nodes[1].name.as_deref(), Some("Sunday"));
    }

    #[test]
    fn day_cells_carry_gridcell_role_and_set_position() {
        let nodes =
            DatePickerView::access_node(&PickerState::idle(INITIAL_YEAR, INITIAL_MONTH), None);
        // 1 grid + 7 columnheaders -> first day cell at index 8.
        let first_cell = &nodes[8];
        assert_eq!(first_cell.role, AriaRole::GridCell);
        assert_eq!(first_cell.name.as_deref(), Some("May 1, 2026"));
        assert_eq!(first_cell.position_in_set, Some(1));
        assert_eq!(first_cell.size_of_set, Some(31));
    }

    #[test]
    fn selected_day_cell_carries_aria_selected() {
        let mut state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        state.selected = Some(CivilDate { year: 2026, month: 5, day: 15 });
        let nodes = DatePickerView::access_node(&state, None);
        let cell = nodes.iter().find(|n| n.tag == "datepicker#15").expect("day 15");
        assert_eq!(cell.selected, Some(true));
        let other = nodes.iter().find(|n| n.tag == "datepicker#14").expect("day 14");
        assert_eq!(other.selected, Some(false));
    }

    #[test]
    fn nav_buttons_are_buttons_with_names() {
        let nodes =
            DatePickerView::access_node(&PickerState::idle(INITIAL_YEAR, INITIAL_MONTH), None);
        let prev = nodes.iter().find(|n| n.tag == PREV_TAG).expect("prev");
        let next = nodes.iter().find(|n| n.tag == NEXT_TAG).expect("next");
        assert_eq!(prev.role, AriaRole::Button);
        assert_eq!(prev.name.as_deref(), Some("Previous month"));
        assert_eq!(next.name.as_deref(), Some("Next month"));
    }

    #[test]
    fn grid_focus_marks_active_descendant_cell_focused() {
        let mut state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        state.focused_day = Some(12);
        let nodes = DatePickerView::access_node(&state, Some(PRIMARY_TAG));
        let cell = nodes.iter().find(|n| n.tag == "datepicker#12").expect("day 12");
        assert!(cell.state.focused, "active descendant cell is focused");
        let other = nodes.iter().find(|n| n.tag == "datepicker#11").expect("day 11");
        assert!(!other.state.focused);
    }

    #[test]
    fn unfocused_grid_marks_no_cell_focused() {
        let mut state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        state.focused_day = Some(12);
        let nodes = DatePickerView::access_node(&state, None);
        assert!(nodes.iter().all(|n| !n.state.focused));
    }

    #[test]
    fn access_focus_target_reports_active_descendant() {
        let mut state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        state.focused_day = Some(20);
        let target = DatePickerView::access_focus_target(&state, Some(PRIMARY_TAG))
            .expect("grid focused returns Some");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("datepicker#20"));
    }

    #[test]
    fn access_focus_target_falls_back_to_selected_then_day_one() {
        let mut state = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        state.selected = Some(CivilDate { year: 2026, month: 5, day: 9 });
        let target = DatePickerView::access_focus_target(&state, Some(PRIMARY_TAG))
            .expect("grid focused returns Some");
        assert_eq!(target.active_descendant.as_deref(), Some("datepicker#9"));
        let bare = PickerState::idle(INITIAL_YEAR, INITIAL_MONTH);
        let t2 = DatePickerView::access_focus_target(&bare, Some(PRIMARY_TAG))
            .expect("grid focused returns Some");
        assert_eq!(t2.active_descendant.as_deref(), Some("datepicker#1"));
    }

    // ── access_child_invoke ───────────────────────────────────────

    #[test]
    fn access_child_invoke_click_selects_day() {
        let mut scene = scene_fixture();
        assert!(DatePickerView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "20",
            AccessAction::Click,
        ));
        assert_eq!(selected_day(&scene), Some(20));
    }

    #[test]
    fn access_child_invoke_nav_button_rolls_month() {
        let mut scene = scene_fixture();
        assert!(DatePickerView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "next",
            AccessAction::Click,
        ));
        assert_eq!(displayed_month(&scene), (2026, 6));
    }

    #[test]
    fn access_child_invoke_out_of_range_declines() {
        let mut scene = scene_fixture();
        assert!(!DatePickerView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "99",
            AccessAction::Click,
        ));
        assert!(!DatePickerView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "foo",
            AccessAction::Click,
        ));
        assert_eq!(selected_day(&scene), None);
    }

    // ── read_state round trip ─────────────────────────────────────

    #[test]
    fn read_state_reflects_selection_and_month() {
        let mut scene = scene_fixture();
        grid_key(&mut scene, "End"); // active descendant -> 31
        assert!(grid_key(&mut scene, "Enter"));
        let state = DatePickerView::read_state(&scene);
        assert_eq!(state.year, 2026);
        assert_eq!(state.month, 5);
        assert_eq!(state.selected, Some(CivilDate { year: 2026, month: 5, day: 31 }));
        // The activate edge syncs the active descendant to the chosen day.
        assert_eq!(state.focused_day, Some(31));
    }
}
