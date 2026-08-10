//! R704 §5.38 §5.50 — `DatePicker` widget: a single-month calendar
//! grid with single-day selection + previous/next-month navigation.
//!
//! An inline date picker presents one month as a 7-column calendar grid
//! (the WAI-ARIA `grid` role with `gridcell` children). Exactly one day
//! may be selected at a time; activating a day replaces the previous
//! selection. The header's previous/next navigation rolls the displayed
//! month (with year rollover) without changing the selection — the
//! Material / `SwiftUI` / the toolkit calendar widget convention.
//!
//! `DatePicker` is a single coordinator (mirroring
//! [`RadioGroup`](crate::widgets::radio_group::RadioGroup) — a "select
//! 1 of N" model). It owns the displayed year/month, the selected
//! [`CivilDate`], and per-visible-day interaction state, and provides
//! indexed access by day number: [`state`](DatePicker::state) /
//! [`is_selected`](DatePicker::is_selected) /
//! [`send`](DatePicker::send) / [`step_month`](DatePicker::step_month).
//!
//! Visual scene placement is the application's responsibility (same
//! contract as [`RadioGroup`](crate::widgets::radio_group::RadioGroup)): the binding composes the header + 6×7
//! grid via `pinion_widget_paint::datepicker` and queries the
//! coordinator for per-day state.
//!
//! The [`DatePickerExternal`] adapter exposes the picker on the §5.12
//! RPC surface:
//!
//! * `query "year"` / `query "month"` / `query "days"` →
//!   [`IntrospectValue::Int`] — displayed year / month (1..=12) /
//!   day count
//! * `query "selected"` → [`IntrospectValue::Bool`] — any selection?
//! * `query "selected_year"` / `query "selected_month"` /
//!   `query "selected_day"` → [`IntrospectValue::Int`] (each `-1`
//!   when no selection), enough to reconstruct the selected date
//! * `query "state.<d>"` / `query "selected.<d>"` — per-day analogs
//!   keyed by day number (`1..=days`)
//! * `invoke "send" → "<day>:<EventName>"` — drive one day cell
//! * `invoke "send" → "PrevMonth"` / `"NextMonth"` — roll the month
//!
//! On selection-change transitions, the §5.20 channel emits a
//! `"selected"` intent carrying the new day-of-month as
//! [`IntrospectValue::Int`] (the day always belongs to the displayed
//! month at activation time, mirroring `RadioGroup`'s index payload).

use crate::WidgetStateName;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState};
use crate::widgets::selection;
use crate::widgets::{IntentEmitter, WidgetTransition};

/// A calendar date in the proleptic Gregorian calendar.
///
/// `month` is 1-based (`1..=12`); `day` is 1-based (`1..=31`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    /// Year (proleptic Gregorian; may be negative for years BCE).
    pub year: i32,
    /// Month, 1..=12.
    pub month: u8,
    /// Day of month, 1-based.
    pub day: u8,
}

/// Whether `year` is a leap year in the Gregorian calendar (divisible
/// by 4, except centuries not divisible by 400).
#[must_use]
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days in `month` of `year` (1..=12 → 28..=31).
///
/// Returns `0` for an out-of-range month (callers pass `1..=12`).
#[must_use]
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// The day of the week for `date` (`0` = Sunday .. `6` = Saturday).
///
/// Sakamoto's algorithm over the proleptic Gregorian calendar.
#[must_use]
pub fn day_of_week(date: CivilDate) -> u8 {
    /// Sakamoto's month offset table (index = month − 1).
    const MONTH_OFFSET: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = date.year;
    let month = usize::from(date.month);
    if month < 3 {
        y -= 1;
    }
    let offset = MONTH_OFFSET[month.saturating_sub(1).min(11)];
    let dow = y + y / 4 - y / 100 + y / 400 + offset + i32::from(date.day);
    // Rust `%` can be negative for negative years; normalise to 0..=6.
    u8::try_from(dow.rem_euclid(7)).unwrap_or(0)
}

/// The day of the week (`0` = Sunday .. `6` = Saturday) of the first
/// day of `month` in `year`.
#[must_use]
pub fn weekday_of_first(year: i32, month: u8) -> u8 {
    day_of_week(CivilDate {
        year,
        month,
        day: 1,
    })
}

/// Roll `(year, month)` by `delta` months, crossing the year boundary
/// at the Jan↔Dec edge.
///
/// `delta` of `-1` steps back one month, `+1` forward; larger
/// magnitudes apply step-wise. Returns the new `(year, month)` with
/// `month` in `1..=12`.
#[must_use]
pub fn add_months(year: i32, month: u8, delta: i32) -> (i32, u8) {
    // Convert (year, month) to a 0-based absolute month count, shift,
    // then split back. i64 widens to avoid overflow at the i32 edges.
    let total = i64::from(year) * 12 + i64::from(month) - 1 + i64::from(delta);
    let new_year = total.div_euclid(12);
    let new_month = total.rem_euclid(12) + 1;
    (
        i32::try_from(new_year).unwrap_or(year),
        u8::try_from(new_month).unwrap_or(month),
    )
}

/// Logical date picker with framework-owned single-day selection. See
/// module docs for the design rationale.
///
/// Reuses the [`Radio`] leaf statechart per day cell (the same
/// "select 1 of N" leaf [`RadioGroup`](crate::widgets::radio_group::RadioGroup) composes), so day cells inherit
/// the canonical `{Idle, Hover, Pressed, Disabled}` interaction model +
/// activate edge. The grid is rebuilt on each month change, so the
/// per-day `Radio` vector is re-sized to the displayed month's length.
pub struct DatePicker {
    displayed_year: i32,
    displayed_month: u8,
    /// One [`Radio`] leaf per visible day of the displayed month,
    /// indexed by `day − 1`.
    days: Vec<Radio>,
    /// The selected date, retained across month navigation even when
    /// not in the displayed month.
    selected: Option<CivilDate>,
    /// R704 §5.40 — the WAI-ARIA roving active descendant: the day cell
    /// the grid's keyboard cursor currently addresses (1-based), or
    /// `None` before any navigation. Independent of `selected` — arrow
    /// keys move it without committing a selection (the date-grid mirror
    /// of [`RadioGroup`](crate::widgets::radio_group::RadioGroup)'s
    /// `focused_index`). Activation (`send` `PointerUp` edge) syncs it to
    /// the activated day per the WAI-ARIA "activation moves focus" rule.
    focused_day: Option<u8>,
}

impl DatePicker {
    /// Construct a picker showing `(year, month)` with optional initial
    /// selection. All visible days start idle.
    #[must_use]
    pub fn new(year: i32, month: u8, selected: Option<CivilDate>) -> Self {
        let mut this = Self {
            displayed_year: year,
            displayed_month: month,
            days: Vec::new(),
            selected,
            focused_day: None,
        };
        this.rebuild_days();
        this
    }

    /// The displayed year.
    #[must_use]
    pub fn displayed_year(&self) -> i32 {
        self.displayed_year
    }

    /// The displayed month (1..=12).
    #[must_use]
    pub fn displayed_month(&self) -> u8 {
        self.displayed_month
    }

    /// The number of days in the displayed month.
    #[must_use]
    pub fn days_in_displayed_month(&self) -> u8 {
        days_in_month(self.displayed_year, self.displayed_month)
    }

    /// The currently selected date, or `None`.
    #[must_use]
    pub fn selected(&self) -> Option<CivilDate> {
        self.selected
    }

    /// The selected day number *if and only if* the selection lies in
    /// the displayed month; `None` otherwise (selection in another
    /// month, or no selection).
    #[must_use]
    pub fn selected_day_in_view(&self) -> Option<u8> {
        self.selected
            .filter(|d| d.year == self.displayed_year && d.month == self.displayed_month)
            .map(|d| d.day)
    }

    /// Reset the day-leaf vector to the displayed month's length, with
    /// the in-view selected day (if any) pre-selected.
    fn rebuild_days(&mut self) {
        let n = usize::from(self.days_in_displayed_month());
        let in_view = self.selected_day_in_view();
        self.days = (0..n)
            .map(|i| {
                let mut r = Radio::new();
                if in_view == Some(u8::try_from(i + 1).unwrap_or(0)) {
                    r.set_selected(true);
                }
                r
            })
            .collect();
    }

    /// Drive `event` to the day cell for day number `day` (1-based). If
    /// the event activates that cell (`false → true` selected), every
    /// other cell is deselected and `selected` snaps to that date.
    ///
    /// Out-of-range `day` is a silent no-op (the router rejects bad
    /// composite sub-indices upstream; this guards the model path).
    pub fn send(&mut self, day: u8, event: RadioEvent) {
        if day < 1 || day > self.days_in_displayed_month() {
            return;
        }
        let idx = usize::from(day - 1);
        let was_selected = self.days[idx].is_selected();
        self.days[idx].send(event);
        // R735.1 §5.38 — single-select sibling-deselect (shared 4-consumer
        // substrate). On a fresh activation it clears the other days and
        // reports the gain so the selected date + active descendant sync.
        if selection::select_exclusive(&mut self.days, idx, was_selected) {
            self.selected = Some(CivilDate {
                year: self.displayed_year,
                month: self.displayed_month,
                day,
            });
            // R704 §5.40 — WAI-ARIA "activation moves focus": the
            // activate edge syncs the active descendant to the chosen
            // day (mirror of `RadioGroup::send` R51.90).
            self.focused_day = Some(day);
        }
    }

    /// R704 §5.40 — the roving active-descendant day (1-based), or
    /// `None` before any navigation. See [`Self::focused_day`].
    #[must_use]
    pub fn focused_day(&self) -> Option<u8> {
        self.focused_day
    }

    /// R704 §5.40 — set the roving active descendant. `None` clears it;
    /// `Some(d)` is stored as-is (callers validate against
    /// [`Self::days_in_displayed_month`]). Independent of selection —
    /// this neither activates the day nor fires the `"selected"` intent
    /// (the date-grid mirror of `RadioGroup::set_focused_index`).
    pub fn set_focused_day(&mut self, day: Option<u8>) {
        self.focused_day = day;
    }

    /// Interaction state of the day cell for `day` (1-based), or
    /// [`RadioState::Idle`] for an out-of-range day.
    #[must_use]
    pub fn state(&self, day: u8) -> RadioState {
        if day < 1 || day > self.days_in_displayed_month() {
            return RadioState::Idle;
        }
        self.days[usize::from(day - 1)].state()
    }

    /// Whether the day cell for `day` (1-based) is selected in the
    /// displayed month. `false` for an out-of-range day.
    #[must_use]
    pub fn is_selected(&self, day: u8) -> bool {
        if day < 1 || day > self.days_in_displayed_month() {
            return false;
        }
        self.days[usize::from(day - 1)].is_selected()
    }

    /// Roll the displayed month by `delta` (e.g. `-1` previous, `+1`
    /// next) with year rollover. The selection is unchanged; the day
    /// grid is rebuilt for the new month (re-selecting the in-view day
    /// if the selection lands back in view).
    pub fn step_month(&mut self, delta: i32) {
        let (y, m) = add_months(self.displayed_year, self.displayed_month, delta);
        self.displayed_year = y;
        self.displayed_month = m;
        self.rebuild_days();
        // Clamp the active descendant into the new month so a roll from a
        // 31-day month onto a shorter one never strands the cursor on a
        // non-existent day.
        if let Some(d) = self.focused_day {
            self.focused_day = Some(d.min(self.days_in_displayed_month()).max(1));
        }
    }
}

impl Default for DatePicker {
    /// Default constructs a picker at year 1970, January, unselected.
    /// Applications call `DatePicker::new(year, month, selected)` with a
    /// concrete month; `Default` exists to satisfy
    /// [`IntentEmitter`]`<W: Default>` generic bounds.
    fn default() -> Self {
        Self::new(1970, 1, None)
    }
}

/// `DatePicker` transition contract (R51.12 substrate). The event pairs
/// the day number with the underlying [`RadioEvent`]; the snapshot is
/// the selected date option; detect emits `"selected"` (the new
/// day-of-month as [`IntrospectValue::Int`]) whenever the selection
/// moves to a new date.
///
/// Selection transitions that emit:
///
/// * `None → Some(d)` — first selection
/// * `Some(a) → Some(b)` where `a != b` — switch (including same day in
///   a different month after navigation)
///
/// Transitions that stay silent (idempotent):
///
/// * `Some(a) → Some(a)` — re-activate the same date
/// * `None → None` — non-activating event
impl WidgetTransition for DatePicker {
    type Event = (u8, RadioEvent);
    type Snapshot = Option<CivilDate>;

    fn snapshot(&self) -> Self::Snapshot {
        self.selected
    }

    fn drive(&mut self, event: Self::Event) {
        let (day, ev) = event;
        self.send(day, ev);
    }

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        if before != after {
            if let Some(date) = after {
                return vec![Intent::new_static(
                    "selected",
                    IntrospectValue::Int(i64::from(date.day)),
                )];
            }
        }
        Vec::new()
    }
}

/// `External` adapter wrapping a [`DatePicker`]. Surfaces picker state
/// to the §5.12 `scene/query` / `scene/rewind` / `scene/invoke` paths
/// and emits a `"selected"` intent (the new day as
/// [`IntrospectValue::Int`]) on selection-change transitions.
pub struct DatePickerExternal {
    em: IntentEmitter<DatePicker>,
}

impl DatePickerExternal {
    /// Construct a picker showing `(year, month)` with optional initial
    /// selection.
    #[must_use]
    pub fn new(year: i32, month: u8, selected: Option<CivilDate>) -> Self {
        Self {
            em: IntentEmitter::new(DatePicker::new(year, month, selected)),
        }
    }

    /// Drive `event` to the day cell for `day` (1-based). Queues a
    /// `"selected"` intent on selection-change transitions.
    pub fn send(&mut self, day: u8, event: RadioEvent) {
        self.em.dispatch((day, event));
    }

    /// The displayed year.
    #[must_use]
    pub fn displayed_year(&self) -> i32 {
        self.em.inner.displayed_year()
    }

    /// The displayed month (1..=12).
    #[must_use]
    pub fn displayed_month(&self) -> u8 {
        self.em.inner.displayed_month()
    }

    /// The number of days in the displayed month.
    #[must_use]
    pub fn days_in_displayed_month(&self) -> u8 {
        self.em.inner.days_in_displayed_month()
    }

    /// The currently selected date, or `None`.
    #[must_use]
    pub fn selected(&self) -> Option<CivilDate> {
        self.em.inner.selected()
    }

    /// Interaction state of the day cell for `day` (1-based).
    #[must_use]
    pub fn state(&self, day: u8) -> RadioState {
        self.em.inner.state(day)
    }

    /// Whether the day cell for `day` (1-based) is selected.
    #[must_use]
    pub fn is_selected(&self, day: u8) -> bool {
        self.em.inner.is_selected(day)
    }

    /// Roll the displayed month by `delta` (`-1` previous, `+1` next).
    /// Selection unchanged; does not fire the `"selected"` intent.
    pub fn step_month(&mut self, delta: i32) {
        self.em.inner.step_month(delta);
    }

    /// R704 §5.40 — the roving active-descendant day (1-based), or `None`.
    /// See [`DatePicker::focused_day`].
    #[must_use]
    pub fn focused_day(&self) -> Option<u8> {
        self.em.inner.focused_day()
    }

    /// R704 §5.40 — set the roving active descendant (AT navigation /
    /// arrow-key cursor). Does not activate the day or fire the
    /// `"selected"` intent. Out-of-range / zero days are rejected.
    fn resolve_day_intervene(&self, i: i64) -> Result<u8, InterveneError> {
        // R1565 — NOT `wire::resolve_index`: a day is ONE-based and inclusive
        // (`1..=days`), which `[0, count)` cannot state, so the range this
        // sentence names would be a lie if it borrowed that helper's.
        let days = self.days_in_displayed_month();
        let Ok(day) = u8::try_from(i) else {
            return Err(InterveneError::out_of_range(format!(
                "{i} is not a day number"
            )));
        };
        if day < 1 || day > days {
            return Err(InterveneError::out_of_range(format!(
                "day {day} is outside the displayed month (it has {days}, so 1..={days})"
            )));
        }
        Ok(day)
    }
}

impl Default for DatePickerExternal {
    /// Default constructs a picker at 1970-01, unselected.
    fn default() -> Self {
        Self::new(1970, 1, None)
    }
}

impl core::fmt::Debug for DatePickerExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DatePickerExternal")
            .field("displayed_year", &self.displayed_year())
            .field("displayed_month", &self.displayed_month())
            .field("selected", &self.selected())
            .finish()
    }
}

impl External for DatePickerExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for DatePickerExternal {
    fn schema(&self) -> IntrospectSchema {
        // The per-day paths advertise their `<day>` placeholder the same
        // way `send` documents its `"<day>:<EventName>"` wire format —
        // discovery metadata for AI clients (`scene/schema` RPC), not a
        // static enumeration of concrete paths.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("year", "int"),
                    SchemaField::new("month", "int"),
                    SchemaField::new("days", "int"),
                    SchemaField::new("selected", "bool"),
                    SchemaField::new("selected_year", "int"),
                    SchemaField::new("selected_month", "int"),
                    SchemaField::new("selected_day", "int"),
                    SchemaField::new("focused_day", "int"),
                    // (R1353.1) `day` is a CALENDAR day: `1..=days`, one-based and
                    // inclusive (see `query`'s `day < 1 || day > days_in_…`
                    // guard). `IndexOf("days")` means `0..days`, so declaring it
                    // here would be false at BOTH ends — it would promise day 0
                    // (which does not exist) and deny the last day of the month
                    // (which does). `days` is published and readable; what is
                    // missing is a way to SAY "one-based, inclusive", so this
                    // says nothing rather than something wrong. A second
                    // one-based family is what should force that variant, not
                    // this one alone.
                    SchemaField::parametric(
                        "state.<day>",
                        "string",
                        const { &[SchemaArg::open("day", "int")] },
                    ),
                    SchemaField::parametric(
                        "selected.<day>",
                        "bool",
                        const { &[SchemaArg::open("day", "int")] },
                    ),
                    SchemaField::send("string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "year" => Some(IntrospectValue::Int(i64::from(self.displayed_year()))),
            "month" => Some(IntrospectValue::Int(i64::from(self.displayed_month()))),
            "days" => Some(IntrospectValue::Int(i64::from(
                self.days_in_displayed_month(),
            ))),
            "selected" => Some(IntrospectValue::Bool(self.selected().is_some())),
            "selected_year" => Some(IntrospectValue::Int(
                self.selected().map_or(-1, |d| i64::from(d.year)),
            )),
            "selected_month" => Some(IntrospectValue::Int(
                self.selected().map_or(-1, |d| i64::from(d.month)),
            )),
            "selected_day" => Some(IntrospectValue::Int(
                self.selected().map_or(-1, |d| i64::from(d.day)),
            )),
            // R704 §5.40 — the roving active descendant. `-1` until an
            // arrow / Home / End / activation lands a value (mirror of
            // `RadioGroup`'s `focused_index`, which uses `Null`; the
            // picker uses `-1` for parity with its other `int` day slots).
            "focused_day" => Some(IntrospectValue::Int(
                self.focused_day().map_or(-1, i64::from),
            )),
            _ => {
                // Per-day query paths: `state.<d>` (interaction state
                // name, mirrors `Radio::query("state")`) and
                // `selected.<d>` (the in-view selected bit). Out-of-
                // range days and malformed suffixes return `None`.
                if let Some(day_str) = path.strip_prefix("state.") {
                    let day: u8 = day_str.parse().ok()?;
                    if day < 1 || day > self.days_in_displayed_month() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(self.state(day).as_name().to_string()));
                }
                if let Some(day_str) = path.strip_prefix("selected.") {
                    let day: u8 = day_str.parse().ok()?;
                    if day < 1 || day > self.days_in_displayed_month() {
                        return None;
                    }
                    return Some(IntrospectValue::Bool(self.is_selected(day)));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // R704 §5.40 — the roving active descendant is the one
            // writable slot: AT `Focus` actions + the binding's arrow-key
            // roving land here (mirror of `RadioGroup`'s `focused_index`
            // intervene). It moves the cursor only — no activation, no
            // `"selected"` intent. `Null` clears it; `Int(d)` validates
            // against the displayed month.
            "focused_day" => match value {
                IntrospectValue::Int(i) => {
                    let day = self.resolve_day_intervene(i)?;
                    self.em.inner.set_focused_day(Some(day));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_focused_day(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // The remaining slots are read-only: the displayed month is
            // driven through `invoke "send" → "PrevMonth"/"NextMonth"` and
            // the selection through the day-cell activate wire, never by
            // direct slot assignment. This mirrors the `RadioGroup`
            // convention where the commit-class paths fire the `"selected"`
            // intent.
            "year" | "month" | "days" | "selected" | "selected_year" | "selected_month"
            | "selected_day" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Wire format: "<day>:<EventName>" drives one day cell; the
            // sentinels "PrevMonth" / "NextMonth" roll the displayed
            // month. Returns the new selected day (or `Null`) for a day
            // send, and `Null` for a month roll.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    match s.as_str() {
                        "PrevMonth" => {
                            self.step_month(-1);
                            return Ok(IntrospectValue::Null);
                        }
                        "NextMonth" => {
                            self.step_month(1);
                            return Ok(IntrospectValue::Null);
                        }
                        _ => {}
                    }
                    // R880.1 — the `split_send_payload` `:` grammar SSOT
                    // strips a held-modifier third segment ("prev:PointerUp:c"
                    // would otherwise read "PointerUp:c" as the event name
                    // and the month-roll click was silently rejected).
                    let crate::composite_tag::SendPayload {
                        key,
                        event: event_name,
                        ..
                    } = crate::composite_tag::require_send_payload("datepicker.send", s)?;
                    // Composite nav sub-tags: a click on the paint
                    // `"<tag>#prev"` / `"<tag>#next"` button arrives here
                    // as `"prev:<EventName>"` / `"next:<EventName>"` (the
                    // R51.42 `'#'`-split funnel). Roll the month once, on
                    // the `PointerUp` edge — the other cycle events
                    // (Enter / Down / Leave) are accepted as no-ops so the
                    // full pointer cycle a click produces is not rejected.
                    match key {
                        "prev" => {
                            if event_name == PointerWireEvent::Up.as_wire_name() {
                                self.step_month(-1);
                            }
                            return Ok(IntrospectValue::Null);
                        }
                        "next" => {
                            if event_name == PointerWireEvent::Up.as_wire_name() {
                                self.step_month(1);
                            }
                            return Ok(IntrospectValue::Null);
                        }
                        _ => {}
                    }
                    let day: u8 = key.parse().map_err(|_| {
                        InvokeError::rejected(format!(
                            "datepicker.send: target {key:?} is neither a day number \
                             nor the \"prev\" / \"next\" month step"
                        ))
                    })?;
                    if day < 1 || day > self.days_in_displayed_month() {
                        return Err(InvokeError::rejected(format!(
                            "datepicker.send: day {day} is outside the displayed month \
                             (it has {} days)",
                            self.days_in_displayed_month()
                        )));
                    }
                    let ev =
                        crate::widget_core::require_event::<RadioEvent>("datepicker", event_name)?;
                    self.send(day, ev);
                    Ok(match self.selected() {
                        Some(d) => IntrospectValue::Int(i64::from(d.day)),
                        None => IntrospectValue::Null,
                    })
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

    /// Drive the full pointer click cycle on day `d` — the sequence the
    /// `InputRouter` produces for a click, activating the day cell.
    fn activate(p: &mut DatePicker, d: u8) {
        p.send(d, RadioEvent::PointerEnter);
        p.send(d, RadioEvent::PointerDown);
        p.send(d, RadioEvent::PointerUp);
        p.send(d, RadioEvent::PointerLeave);
    }

    fn activate_ext(p: &mut DatePickerExternal, d: u8) {
        p.send(d, RadioEvent::PointerEnter);
        p.send(d, RadioEvent::PointerDown);
        p.send(d, RadioEvent::PointerUp);
        p.send(d, RadioEvent::PointerLeave);
    }

    // ── pure date helpers ─────────────────────────────────────────

    #[test]
    fn leap_year_rules() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2026));
    }

    #[test]
    fn days_in_month_known() {
        assert_eq!(days_in_month(2026, 5), 31);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
    }

    #[test]
    fn day_of_week_known() {
        // 2026-05-01 is a Friday (5).
        assert_eq!(
            day_of_week(CivilDate {
                year: 2026,
                month: 5,
                day: 1
            }),
            5
        );
        // 2000-01-01 is a Saturday (6).
        assert_eq!(
            day_of_week(CivilDate {
                year: 2000,
                month: 1,
                day: 1
            }),
            6
        );
    }

    #[test]
    fn weekday_of_first_matches() {
        assert_eq!(weekday_of_first(2026, 5), 5);
    }

    #[test]
    fn add_months_forward_year_rollover() {
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
    }

    #[test]
    fn add_months_backward_year_rollover() {
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
    }

    #[test]
    fn add_months_within_year() {
        assert_eq!(add_months(2026, 5, 1), (2026, 6));
        assert_eq!(add_months(2026, 5, -1), (2026, 4));
    }

    // ── DatePicker model ──────────────────────────────────────────

    #[test]
    fn new_picker_has_no_selection() {
        let p = DatePicker::new(2026, 5, None);
        assert_eq!(p.selected(), None);
        assert_eq!(p.displayed_year(), 2026);
        assert_eq!(p.displayed_month(), 5);
        assert_eq!(p.days_in_displayed_month(), 31);
    }

    #[test]
    fn select_day_sets_selection() {
        let mut p = DatePicker::new(2026, 5, None);
        activate(&mut p, 15);
        assert_eq!(
            p.selected(),
            Some(CivilDate {
                year: 2026,
                month: 5,
                day: 15
            })
        );
        assert!(p.is_selected(15));
        assert!(!p.is_selected(14));
    }

    #[test]
    fn selecting_second_day_deselects_first() {
        let mut p = DatePicker::new(2026, 5, None);
        activate(&mut p, 3);
        activate(&mut p, 20);
        assert_eq!(
            p.selected(),
            Some(CivilDate {
                year: 2026,
                month: 5,
                day: 20
            })
        );
        assert!(!p.is_selected(3));
        assert!(p.is_selected(20));
    }

    #[test]
    fn next_month_advances_keeps_selection() {
        let mut p = DatePicker::new(2026, 5, None);
        activate(&mut p, 10);
        p.step_month(1);
        assert_eq!(p.displayed_month(), 6);
        assert_eq!(p.displayed_year(), 2026);
        // Selection retained, but day 10 of June is not selected.
        assert_eq!(
            p.selected(),
            Some(CivilDate {
                year: 2026,
                month: 5,
                day: 10
            })
        );
        assert!(!p.is_selected(10));
    }

    #[test]
    fn prev_month_year_rollover() {
        let mut p = DatePicker::new(2026, 1, None);
        p.step_month(-1);
        assert_eq!(p.displayed_month(), 12);
        assert_eq!(p.displayed_year(), 2025);
    }

    #[test]
    fn next_month_year_rollover() {
        let mut p = DatePicker::new(2026, 12, None);
        p.step_month(1);
        assert_eq!(p.displayed_month(), 1);
        assert_eq!(p.displayed_year(), 2027);
    }

    #[test]
    fn navigating_back_into_selection_reselects_in_view() {
        let mut p = DatePicker::new(2026, 5, None);
        activate(&mut p, 10);
        p.step_month(1); // June
        assert!(!p.is_selected(10));
        p.step_month(-1); // back to May
        assert!(
            p.is_selected(10),
            "in-view selected day re-selects on return"
        );
    }

    // ── External + intent emission ────────────────────────────────

    #[test]
    fn external_first_select_emits_selected_intent_with_day() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        activate_ext(&mut p, 15);
        assert!(p.is_dirty());
        let mut harvested = Vec::new();
        p.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(15));
    }

    #[test]
    fn external_invoke_send_day_activate_returns_day() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let out = p
                .invoke("send", IntrospectValue::Text(format!("5:{ev}")))
                .unwrap();
            if ev == "PointerUp" {
                assert_eq!(out, IntrospectValue::Int(5));
            }
        }
        assert_eq!(
            p.selected(),
            Some(CivilDate {
                year: 2026,
                month: 5,
                day: 5
            })
        );
    }

    #[test]
    fn external_invoke_month_nav_rolls_displayed() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        assert_eq!(
            p.invoke("send", IntrospectValue::Text("PrevMonth".to_string())),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(p.displayed_month(), 4);
        assert_eq!(p.days_in_displayed_month(), 30);
        assert_eq!(
            p.invoke("send", IntrospectValue::Text("NextMonth".to_string())),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(p.displayed_month(), 5);
    }

    #[test]
    fn external_invoke_composite_nav_click_rolls_month_on_pointer_up() {
        // A click on the paint `"<tag>#prev"` / `"<tag>#next"` button
        // funnels through the InputRouter as the full pointer cycle
        // `prev:PointerEnter` .. `prev:PointerUp` .. `prev:PointerLeave`.
        // The month must roll exactly once, on the PointerUp edge.
        let mut p = DatePickerExternal::new(2026, 5, None);
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            assert_eq!(
                p.invoke("send", IntrospectValue::Text(format!("prev:{ev}"))),
                Ok(IntrospectValue::Null),
                "prev:{ev} accepted as no-op or roll edge",
            );
        }
        assert_eq!(
            p.displayed_month(),
            4,
            "prev cycle rolls back one month once"
        );
        assert_eq!(p.displayed_year(), 2026);
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = p.invoke("send", IntrospectValue::Text(format!("next:{ev}")));
        }
        assert_eq!(
            p.displayed_month(),
            5,
            "next cycle rolls forward one month once"
        );
    }

    #[test]
    fn r880_1_nav_click_with_modifier_segment_still_rolls() {
        // "prev:PointerUp:c" (the R781 modifier segment) must still roll
        // the month — the pre-R880.1 hand-rolled split read "PointerUp:c"
        // as the event name and rejected the click.
        let mut p = DatePickerExternal::new(2026, 5, None);
        assert_eq!(
            p.invoke("send", IntrospectValue::Text("prev:PointerUp:c".into())),
            Ok(IntrospectValue::Null),
        );
        assert_eq!(p.displayed_month(), 4, "Ctrl+click still rolls the month");
    }

    #[test]
    fn focused_day_intervene_and_query_round_trip() {
        // R704 §5.40 — the roving active descendant is the one writable
        // slot. `-1` until set; `Int(d)` validates against the month;
        // `Null` clears; out-of-range / wrong-variant rejected.
        let mut p = DatePickerExternal::new(2026, 5, None);
        assert_eq!(p.query("focused_day"), Some(IntrospectValue::Int(-1)));
        p.intervene("focused_day", IntrospectValue::Int(15))
            .unwrap();
        assert_eq!(p.focused_day(), Some(15));
        assert_eq!(p.query("focused_day"), Some(IntrospectValue::Int(15)));
        // No `"selected"` intent — focused_day is navigation, not commit.
        assert!(!p.is_dirty());
        p.intervene("focused_day", IntrospectValue::Null).unwrap();
        assert_eq!(p.focused_day(), None);
        assert_out_of_range_saying(
            &p.intervene("focused_day", IntrospectValue::Int(99)),
            "day 99 is outside the displayed month",
        );
        // R1565 — and the range it names is ONE-based, which is why this
        // surface does not borrow `wire::resolve_index`'s `0..count`.
        assert_out_of_range_saying(
            &p.intervene("focused_day", IntrospectValue::Int(0)),
            "so 1..=31",
        );
        assert_eq!(
            p.intervene("focused_day", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn activation_syncs_focused_day_and_month_roll_clamps_it() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        activate_ext(&mut p, 31);
        // WAI-ARIA "activation moves focus": active descendant follows.
        assert_eq!(p.focused_day(), Some(31));
        // Roll May (31) → June (30): the active descendant clamps to 30.
        p.step_month(1);
        assert_eq!(p.focused_day(), Some(30));
    }

    #[test]
    fn external_invoke_out_of_range_day_rejected() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        assert_refused_saying(
            &p.invoke("send", IntrospectValue::Text("99:PointerUp".to_string())),
            "day 99 is outside the displayed month (it has 31 days)",
        );
        assert_refused_saying(
            &p.invoke("send", IntrospectValue::Text("PointerUp".to_string())),
            "malformed send payload \"PointerUp\"",
        );
    }

    #[test]
    fn external_introspect_round_trip() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        activate_ext(&mut p, 15);
        assert_eq!(p.query("year"), Some(IntrospectValue::Int(2026)));
        assert_eq!(p.query("month"), Some(IntrospectValue::Int(5)));
        assert_eq!(p.query("days"), Some(IntrospectValue::Int(31)));
        assert_eq!(p.query("selected"), Some(IntrospectValue::Bool(true)));
        assert_eq!(p.query("selected_year"), Some(IntrospectValue::Int(2026)));
        assert_eq!(p.query("selected_month"), Some(IntrospectValue::Int(5)));
        assert_eq!(p.query("selected_day"), Some(IntrospectValue::Int(15)));
        assert_eq!(p.query("selected.15"), Some(IntrospectValue::Bool(true)));
        assert_eq!(p.query("selected.14"), Some(IntrospectValue::Bool(false)));
        assert_eq!(p.query("selected.99"), None);
        assert_eq!(p.query("state.99"), None);
        assert_eq!(p.query("bogus"), None);
    }

    #[test]
    fn external_selection_absent_outside_displayed_month() {
        let mut p = DatePickerExternal::new(2026, 5, None);
        activate_ext(&mut p, 15);
        p.step_month(1); // June
        // No June day reports selected, but the date is preserved.
        assert_eq!(p.query("selected.15"), Some(IntrospectValue::Bool(false)));
        assert_eq!(p.query("selected_month"), Some(IntrospectValue::Int(5)));
    }
}
