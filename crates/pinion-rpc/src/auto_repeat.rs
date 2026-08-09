//! `scene/auto_repeat` RPC method dispatch — R1549 §5.35 §5.38 + §5.7 + §5.12.
//!
//! The READ face of press-and-hold auto-repeat: every pointer press
//! currently in flight, whether the widget under it is repeating, at what
//! cadence, how many repeats it has already fired, and when the next one
//! lands.
//!
//! ## Why this method exists at all
//!
//! The toolkit has no peer, and the gap is structural rather than an omission.
//! `autoRepeat()` answers a **static property of one
//! widget you already hold a pointer to**; the *run* — is this press
//! repeating right now, how many times has it fired, when is the next one
//! — lives in a private basic timer inside abstract button private and
//! is observable only through its side effects. There is no
//! application-wide enumeration of holds, and no way to ask a live the toolkit
//! application "is a button repeating?".
//!
//! For pinion that is not a nicety: §2 #2 makes the RPC plane the AI
//! agent's primary path and §2 #7 says the scene is queryable as data. An
//! agent that presses a spin arrow and then wants to know whether it is
//! stepping — or, on releasing, how many steps it caused — has exactly one
//! honest place to look, and it is this method.
//!
//! ## Shape
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/auto_repeat", "params": {}, "id": 1 }
//! ```
//!
//! ```json
//! {
//!   "holds": [
//!     {
//!       "pointer": 0,
//!       "target": "spin#inc",
//!       "repeating": true,
//!       "held_secs": 0.85,
//!       "fires": 6,
//!       "delay_secs": 0.3,
//!       "interval_secs": 0.1,
//!       "accel": 1.0,
//!       "min_interval_secs": 0.1,
//!       "next_fire_in_secs": 0.05
//!     }
//!   ]
//! }
//! ```
//!
//! An **empty** `holds` list is the answer for "no pointer is pressed" —
//! a real state, not an absence, so this method has no `*Unavailable`
//! token the way [`scene/cache_stats`](mod@crate::cache_stats) does. A
//! window that has never seen input has no presses, which is the same
//! answer, honestly reached.
//!
//! A press on a widget that does **not** repeat is still listed, with
//! `repeating: false` and the four cadence fields omitted. Dropping it
//! would make "held, and nothing will come of it" indistinguishable from
//! "not held", and those are the two states a stuck-button investigation
//! has to tell apart.
//!
//! ## Side-effect contract
//!
//! Read-only. The embedder pre-resolves the list from the window's
//! router; consulting it advances no clock, fires no repeat and schedules
//! no repaint. Ticking the hold is `scene/tick`'s job — the same one
//! entry the live paint clock uses.

use pinion_runtime::AutoRepeatHold;
use serde::Serialize;

/// One in-flight press as it appears on the wire.
///
/// Field-for-field a [`AutoRepeatHold`] projection, with the cadence
/// flattened out of the [`AutoRepeat`](pinion_core::AutoRepeat) it came
/// from: a JSON reader wants `interval_secs`, not `policy.interval_secs`,
/// and the four values are meaningless individually anyway (they are one
/// cadence).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoRepeatHoldOutcome {
    /// The pointer holding this press. `0` is the mouse; touches are
    /// numbered from `1` (see
    /// [`PointerId::touch`](pinion_runtime::PointerId::touch)).
    pub pointer: u64,
    /// The (possibly composite) paint tag under the press — the same
    /// vocabulary `scene/click` and `scene/invoke` address.
    pub target: String,
    /// Whether the target declares a repeat cadence **right now**.
    ///
    /// `false` is a real answer with three distinct causes, all of which
    /// an agent can then confirm from the widget's own introspection: the
    /// widget never repeats (a plain push button peer), it repeats but
    /// has run out of range (a spin arrow at its bound), or it disabled
    /// itself mid-hold.
    pub repeating: bool,
    /// Seconds this press has been held while armed. `0.0` while not
    /// repeating — including immediately after a press strays off its
    /// target and back, because a re-entry restarts the delay.
    pub held_secs: f32,
    /// Repeats fired so far during this press. `0` for a press that has
    /// not yet outlived its delay, and for one that never will.
    pub fires: u32,
    /// Declared hold before the first repeat, in seconds (the toolkit
    /// `autoRepeatDelay`). Omitted while `repeating` is `false` — there
    /// is no cadence to report, and a `0` would read as "fires
    /// instantly".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<f32>,
    /// Declared first repeat interval, in seconds (the toolkit
    /// `autoRepeatInterval`). Omitted while not repeating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<f32>,
    /// Per-fire interval multiplier; `1.0` = the toolkit's fixed cadence, below
    /// `1.0` = the peer of `setAccelerated(true)`. Omitted while not repeating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accel: Option<f32>,
    /// Floor an accelerating cadence bottoms out at, in seconds. Equal to
    /// `interval_secs` when acceleration is off. Omitted while not
    /// repeating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interval_secs: Option<f32>,
    /// Seconds until the next repeat fires, given the seconds already
    /// accrued. Omitted while not repeating.
    ///
    /// This is the field that makes the method *predictive* rather than
    /// merely descriptive: an agent reads it, ticks exactly that far, and
    /// knows a repeat landed — which is how a hold becomes reproducible
    /// without a wall clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_fire_in_secs: Option<f32>,
}

/// The `scene/auto_repeat` response body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoRepeatOutcome {
    /// Every in-flight press in the dispatch-scoped window, ordered by
    /// pointer so two simultaneous touch-holds read back deterministically.
    pub holds: Vec<AutoRepeatHoldOutcome>,
}

/// R1549 §5.35 §5.12 — project the embedder's pre-resolved hold census
/// onto the wire.
///
/// Total: there is no error arm. Every failure this method could have —
/// unknown window, window that never painted, backend with no pointer at
/// all — is already "no press is in flight", which is a truthful `holds:
/// []` rather than an `Unavailable` token. (The unknown-window id is
/// rejected ahead of method routing by the shared
/// [`unknown_window_verdict`](crate::unknown_window_verdict) gate, so
/// this arm never has to invent an answer for one.)
#[must_use]
pub fn auto_repeat(holds: &[AutoRepeatHold]) -> AutoRepeatOutcome {
    AutoRepeatOutcome {
        holds: holds
            .iter()
            .map(|hold| AutoRepeatHoldOutcome {
                pointer: hold.pointer.raw(),
                target: hold.target.clone(),
                repeating: hold.repeating,
                held_secs: hold.held_secs,
                fires: hold.fires,
                delay_secs: hold.policy.map(|p| p.delay_secs()),
                interval_secs: hold.policy.map(|p| p.interval_secs()),
                accel: hold.policy.map(|p| p.accel()),
                min_interval_secs: hold.policy.map(|p| p.min_interval_secs()),
                next_fire_in_secs: hold.next_fire_in_secs,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::AutoRepeat;
    use pinion_runtime::PointerId;

    fn hold(repeating: bool) -> AutoRepeatHold {
        AutoRepeatHold {
            pointer: PointerId::MOUSE,
            target: "spin#inc".to_owned(),
            repeating,
            held_secs: if repeating { 0.85 } else { 0.0 },
            fires: if repeating { 6 } else { 0 },
            policy: repeating.then(AutoRepeat::desktop),
            next_fire_in_secs: repeating.then_some(0.05),
        }
    }

    #[test]
    fn no_press_is_an_empty_list_not_an_error() {
        // The absence of an `Unavailable` token is the contract: "nothing
        // is held" is a fact, and a client polling during a gesture must
        // not have to distinguish it from a broken backend.
        assert!(auto_repeat(&[]).holds.is_empty());
    }

    #[test]
    fn repeating_hold_publishes_its_whole_cadence() {
        let out = auto_repeat(&[hold(true)]);
        let h = &out.holds[0];
        assert!(h.repeating);
        assert_eq!(h.fires, 6);
        assert_eq!(h.target, "spin#inc");
        assert_eq!(h.pointer, 0, "the mouse is pointer 0");
        assert_eq!(h.delay_secs, Some(AutoRepeat::DEFAULT_DELAY_SECS));
        assert_eq!(h.interval_secs, Some(AutoRepeat::DEFAULT_INTERVAL_SECS));
        assert_eq!(h.accel, Some(1.0), "the toolkit's fixed cadence");
        assert_eq!(h.next_fire_in_secs, Some(0.05));
    }

    #[test]
    fn non_repeating_hold_is_listed_with_no_cadence() {
        // Listed — a held press that will never fire is exactly what a
        // stuck-button investigation is looking for — but with the four
        // cadence keys ABSENT rather than zeroed, so `interval_secs: 0`
        // can never be read as "repeats infinitely fast".
        let out = auto_repeat(&[hold(false)]);
        let h = &out.holds[0];
        assert!(!h.repeating);
        assert_eq!(h.delay_secs, None);
        assert_eq!(h.interval_secs, None);
        assert_eq!(h.accel, None);
        assert_eq!(h.min_interval_secs, None);
        assert_eq!(h.next_fire_in_secs, None);
        let json = serde_json::to_value(&out).expect("serializes");
        let obj = json["holds"][0].as_object().expect("hold is an object");
        assert!(
            !obj.contains_key("interval_secs"),
            "an absent cadence is absent on the wire, not null-or-zero",
        );
        assert!(
            obj.contains_key("repeating"),
            "but the hold itself is reported",
        );
    }
}
