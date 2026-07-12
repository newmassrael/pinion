//! The AI-first drive surface: a [`VnExternal`] wrapping the VN runner.
//!
//! The §5.15 `External` escape hatch used the way §2 #2 / #7 intend — not to
//! embed an opaque subsystem, but to expose the VN runner over the RPC
//! introspection triad so an AI agent (or the `hello-vn-tide` python demo)
//! drives the whole set-piece as data:
//!
//! - **query** — `mode` / `step` / `step_count` / `speaker` / `line` /
//!   `revealed` / `revealed_chars` / `line_len` / `fully_revealed` /
//!   `prompt` / `options` / `option_count` / `timeout_ms` / `remaining_ms` /
//!   `expired` / `resolved` / `outcome` / `outcome_label` / `timed_out` /
//!   `script`.
//! - **intervene** — set `step` to scrub the play-head (clamped) — the
//!   save/load + AI-scrub seam.
//! - **invoke** — the deterministic verbs `tick {ms}` (advance logical
//!   time), `advance` (snap / step a line), `choose {index}` (take a choice
//!   option), and the save/load pair `save` (return the play-head as an
//!   opaque JSON blob) / `load {blob}` (restore it).
//!
//! Every state transition emits a §5.20 intent (`vn.advanced` /
//! `vn.timeout` / `vn.chosen`) so the shell repaints and an RPC observer can
//! watch the play-through.
//!
//! ## The `tick` step-verb — honestly scoped
//!
//! `tick {ms}` advances the runner by exactly `ms` of *logical* time, read
//! from the argument, never a wall clock — so every assertion in the wire
//! demo observes settled state with no background thread and no sleeps
//! ([[zero-flake-policy]]). This is the same choice `hello-audio-rt`'s
//! `render` step-verb made: the north-star driver is the fixed-timestep
//! game-loop `scene/tick {dt}`, but that fan-out reaches only
//! `Scene::ImmediateModeNode` paint drivers, and this runner is deliberately
//! a retained structured-scene `Scene::External` (§2 #1 — the dialogue is
//! queryable text, not opaque paint). Driving the runner from the real
//! game-loop tick (making it an immediate-mode node) is the acknowledged
//! Phase-C follow-up the presentation layer — sprites / transitions — will
//! force; this round proves the VN control + presentation surface over the
//! wire, which is the thing it never had.

use std::rc::Rc;

use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, int_of,
};
use pinion_core::intent::Intent;

use crate::vn::model::VnStep;
use crate::vn::state::{ChooseError, VnRuntime, VnState};

/// Emitted when [`advance`](VnExternal) moves the play-head.
pub const ADVANCED_INTENT: &str = "vn.advanced";
/// Emitted when a `tick` expires a choice's countdown.
pub const TIMEOUT_INTENT: &str = "vn.timeout";
/// Emitted when the player takes a choice option.
pub const CHOSEN_INTENT: &str = "vn.chosen";

/// Default logical step (milliseconds) for `tick` with no argument — one
/// ~60 fps frame, the natural "one frame" default a caller expects.
const DEFAULT_TICK_MS: u32 = 16;
/// Ceiling on a single `tick`'s `ms`, so a fat-fingered / hostile
/// `tick 1e11` cannot overflow the accumulator. Ten minutes of logical time
/// is far past any real single step.
const MAX_TICK_MS: i64 = 600_000;

/// `External` adapter over a shared [`VnState`].
#[derive(Debug)]
pub struct VnExternal {
    state: Rc<VnState>,
    pending_intents: Vec<Intent>,
}

impl VnExternal {
    /// Wrap a shared runner. The `Rc` is the same one the view reads, so a
    /// transition here repaints there.
    #[must_use]
    pub fn new(state: Rc<VnState>) -> Self {
        Self {
            state,
            pending_intents: Vec::new(),
        }
    }

    fn emit(&mut self, tag: &'static str, value: IntrospectValue) {
        self.pending_intents.push(Intent::new_static(tag, value));
    }

    /// A compact status object returned by every drive verb, so a client
    /// reads the immediate result of its own action without a follow-up
    /// round-trip.
    fn status(&self) -> IntrospectValue {
        IntrospectValue::json(&serde_json::json!({
            "mode": self.state.mode().as_str(),
            "step": self.state.runtime().step,
            "step_count": self.state.step_count(),
            "revealed_chars": self.state.revealed_chars(),
            "line_len": self.state.line_len(),
            "fully_revealed": self.state.fully_revealed(),
            "remaining_ms": self.state.remaining_ms(),
            "timeout_ms": self.state.timeout_ms(),
            "expired": self.state.expired(),
            "resolved": self.state.resolution().is_some(),
        }))
    }

    /// The current choice's options as a JSON array (empty off a choice).
    fn options_json(&self) -> IntrospectValue {
        match self.state.current_step() {
            Some(VnStep::TimedChoice { options, .. }) => IntrospectValue::json(options),
            _ => IntrospectValue::json(&Vec::<crate::vn::model::VnOption>::new()),
        }
    }

    fn choose(&mut self, index: usize) -> Result<IntrospectValue, InvokeError> {
        match self.state.choose(index) {
            Ok(()) => {
                if let Some(r) = self.state.resolution() {
                    self.emit(CHOSEN_INTENT, IntrospectValue::json(&r));
                }
                Ok(self.status())
            }
            // A precondition failure (off a choice, or index out of range) is
            // Rejected, not TypeMismatch — the args were well-typed, the
            // action refused. Retrying with a valid index may succeed.
            Err(ChooseError::NotAChoice | ChooseError::OutOfRange) => Err(InvokeError::Rejected),
        }
    }
}

// Paints nothing (the binding's `view` is the paint scene) and drains
// `self.pending_intents` — the RPC-only read-write introspection skeleton.
pinion_core::intent_query_external_impl!(VnExternal);

impl ExternalIntrospect for VnExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("mode", "text"),
            ("step", "int"),
            ("step_count", "int"),
            ("speaker", "text"),
            ("line", "text"),
            ("revealed", "text"),
            ("revealed_chars", "int"),
            ("line_len", "int"),
            ("fully_revealed", "bool"),
            ("prompt", "text"),
            ("options", "json"),
            ("option_count", "int"),
            ("timeout_ms", "int"),
            ("remaining_ms", "int"),
            ("expired", "bool"),
            ("resolved", "bool"),
            ("outcome", "text"),
            ("outcome_label", "text"),
            ("timed_out", "bool"),
            ("script", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "mode" => Some(IntrospectValue::Text(
                self.state.mode().as_str().to_string(),
            )),
            "step" => Some(IntrospectValue::Int(i64::from(self.state.runtime().step))),
            "step_count" => Some(IntrospectValue::Int(int_of(self.state.step_count()))),
            "speaker" => Some(IntrospectValue::Text(match self.state.current_step() {
                Some(VnStep::Line { speaker, .. }) => speaker.clone(),
                _ => String::new(),
            })),
            "line" => Some(IntrospectValue::Text(match self.state.current_step() {
                Some(VnStep::Line { text, .. }) => text.clone(),
                _ => String::new(),
            })),
            "revealed" => Some(IntrospectValue::Text(self.state.revealed_text())),
            "revealed_chars" => Some(IntrospectValue::Int(int_of(self.state.revealed_chars()))),
            "line_len" => Some(IntrospectValue::Int(int_of(self.state.line_len()))),
            "fully_revealed" => Some(IntrospectValue::Bool(self.state.fully_revealed())),
            "prompt" => Some(IntrospectValue::Text(match self.state.current_step() {
                Some(VnStep::TimedChoice { prompt, .. }) => prompt.clone(),
                _ => String::new(),
            })),
            "options" => Some(self.options_json()),
            "option_count" => Some(IntrospectValue::Int(int_of(
                match self.state.current_step() {
                    Some(VnStep::TimedChoice { options, .. }) => options.len(),
                    _ => 0,
                },
            ))),
            "timeout_ms" => Some(IntrospectValue::Int(i64::from(self.state.timeout_ms()))),
            "remaining_ms" => Some(IntrospectValue::Int(i64::from(self.state.remaining_ms()))),
            "expired" => Some(IntrospectValue::Bool(self.state.expired())),
            "resolved" => Some(IntrospectValue::Bool(self.state.resolution().is_some())),
            "outcome" => Some(IntrospectValue::Text(
                self.state
                    .resolved_option()
                    .map(|o| o.outcome.clone())
                    .unwrap_or_default(),
            )),
            "outcome_label" => Some(IntrospectValue::Text(
                self.state
                    .resolved_option()
                    .map(|o| o.label.clone())
                    .unwrap_or_default(),
            )),
            "timed_out" => Some(IntrospectValue::Bool(
                self.state.resolution().is_some_and(|r| r.timed_out),
            )),
            "script" => Some(IntrospectValue::json(self.state.script())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if path == "step" {
            return match value {
                IntrospectValue::Int(n) => {
                    let step = u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0);
                    if self.state.goto(step) {
                        self.emit(ADVANCED_INTENT, IntrospectValue::Int(i64::from(step)));
                    }
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        // Any other declared field is read-via-query, write-via-invoke.
        if self.query(path).is_some() {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "tick" => {
                let ms = parse_tick_ms(&args)?;
                if self.state.tick(ms) {
                    if let Some(r) = self.state.resolution() {
                        self.emit(TIMEOUT_INTENT, IntrospectValue::json(&r));
                    }
                }
                Ok(self.status())
            }
            "advance" => {
                if self.state.advance() {
                    self.emit(
                        ADVANCED_INTENT,
                        IntrospectValue::Int(i64::from(self.state.runtime().step)),
                    );
                }
                Ok(self.status())
            }
            "choose" => match args {
                IntrospectValue::Int(n) => self.choose(usize::try_from(n).unwrap_or(usize::MAX)),
                IntrospectValue::Json(v) => {
                    match v.get("index").and_then(serde_json::Value::as_u64) {
                        Some(i) => self.choose(usize::try_from(i).unwrap_or(usize::MAX)),
                        None => Err(InvokeError::TypeMismatch),
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Return the complete save state as an opaque JSON blob a client
            // persists and hands back to `load` to resume exactly here.
            "save" => Ok(IntrospectValue::json(&self.state.save())),
            // Restore from a save blob (tolerant deserialize; the runner
            // clamps the step into range). Bad JSON is a type error.
            "load" => match args {
                IntrospectValue::Json(v) => match serde_json::from_value::<VnRuntime>(v) {
                    Ok(runtime) => {
                        self.state.load(runtime);
                        Ok(self.status())
                    }
                    Err(_) => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Parse the `tick` verb's millisecond step: `Null` → one ~60 fps frame, an
/// `Int` → that many ms clamped to `[0, MAX_TICK_MS]` (a negative → 0, an
/// over-large → the ceiling), anything else → a type error.
fn parse_tick_ms(args: &IntrospectValue) -> Result<u32, InvokeError> {
    match args {
        IntrospectValue::Null => Ok(DEFAULT_TICK_MS),
        IntrospectValue::Int(n) => Ok(u32::try_from((*n).clamp(0, MAX_TICK_MS)).unwrap_or(0)),
        _ => Err(InvokeError::TypeMismatch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vn::model::{VnOption, VnScript, VnStep};
    use pinion_core::external::External;
    use pinion_core::reactive::Owner;

    fn script() -> VnScript {
        VnScript::new(vec![
            VnStep::line("무녀", "돌아오지 마라"),
            VnStep::timed_choice(
                "부름이 들린다",
                vec![
                    VnOption::new("돌아본다", "answer"),
                    VnOption::new("버틴다", "endure"),
                ],
                4000,
                1,
            ),
        ])
    }

    fn with_external<R>(body: impl FnOnce(&mut VnExternal) -> R) -> R {
        let owner = Owner::new();
        owner.run(|| {
            let state = Rc::new(VnState::new(script()));
            body(&mut VnExternal::new(state))
        })
    }

    fn text(v: Option<IntrospectValue>) -> String {
        match v {
            Some(IntrospectValue::Text(s)) => s,
            other => panic!("expected Text, got {other:?}"),
        }
    }

    fn int(v: Option<IntrospectValue>) -> i64 {
        match v {
            Some(IntrospectValue::Int(n)) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn query_projects_the_current_line() {
        with_external(|ext| {
            assert_eq!(text(ext.query("mode")), "line");
            assert_eq!(text(ext.query("speaker")), "무녀");
            assert_eq!(text(ext.query("line")), "돌아오지 마라");
            assert_eq!(int(ext.query("revealed_chars")), 0);
            assert_eq!(int(ext.query("line_len")), 7);
            assert_eq!(int(ext.query("step_count")), 2);
            assert!(ext.query("bogus").is_none());
        });
    }

    #[test]
    fn tick_reveals_and_emits_no_intent_on_a_line() {
        with_external(|ext| {
            ext.invoke("tick", IntrospectValue::Int(100)).expect("tick");
            assert_eq!(int(ext.query("revealed_chars")), 4);
            assert_eq!(text(ext.query("revealed")), "돌아오지");
            assert!(!ext.is_dirty(), "a mere reveal queues no intent");
        });
    }

    #[test]
    fn save_returns_a_blob_that_load_restores() {
        with_external(|ext| {
            ext.invoke("tick", IntrospectValue::Int(100)).unwrap(); // reveal 4
            let blob = match ext.invoke("save", IntrospectValue::Null) {
                Ok(IntrospectValue::Json(v)) => v,
                other => panic!("save returns a JSON blob, got {other:?}"),
            };
            // Move on, then load the blob back.
            ext.invoke("advance", IntrospectValue::Null).unwrap();
            ext.invoke("advance", IntrospectValue::Null).unwrap();
            assert_eq!(
                text(ext.query("mode")),
                "choice",
                "moved off the saved state"
            );
            ext.invoke("load", IntrospectValue::Json(blob))
                .expect("load");
            assert_eq!(
                text(ext.query("mode")),
                "line",
                "restored to the saved line"
            );
            assert_eq!(int(ext.query("revealed_chars")), 4, "reveal re-derived");
            // A non-JSON load arg is a type error, not a silent no-op.
            assert!(matches!(
                ext.invoke("load", IntrospectValue::Int(3)),
                Err(InvokeError::TypeMismatch)
            ));
        });
    }

    #[test]
    fn advance_snaps_then_steps_and_emits_intent() {
        with_external(|ext| {
            ext.invoke("advance", IntrospectValue::Null).expect("snap");
            assert!(ext.query("fully_revealed").is_some());
            assert_eq!(text(ext.query("revealed")), "돌아오지 마라");
            ext.invoke("advance", IntrospectValue::Null).expect("step");
            assert_eq!(text(ext.query("mode")), "choice");
            assert!(ext.is_dirty(), "advance queues vn.advanced");
            let mut tags = Vec::new();
            ext.drain_intents(&mut |i| tags.push(i.tag));
            assert!(tags.iter().any(|t| t == ADVANCED_INTENT));
        });
    }

    #[test]
    fn timeout_over_the_verb_resolves_default_and_emits_timeout() {
        with_external(|ext| {
            ext.invoke("advance", IntrospectValue::Null).unwrap(); // snap
            ext.invoke("advance", IntrospectValue::Null).unwrap(); // onto choice
            assert_eq!(text(ext.query("mode")), "choice");
            assert_eq!(int(ext.query("remaining_ms")), 4000);
            ext.invoke("tick", IntrospectValue::Int(5000)).unwrap();
            assert_eq!(text(ext.query("mode")), "end");
            assert!(ext.query("expired").is_some());
            assert_eq!(text(ext.query("outcome")), "endure", "default option");
            assert!(ext.query("timed_out").is_some());
            let mut tags = Vec::new();
            ext.drain_intents(&mut |i| tags.push(i.tag));
            assert!(tags.iter().any(|t| t == TIMEOUT_INTENT));
        });
    }

    #[test]
    fn choose_over_the_verb_records_and_emits_chosen() {
        with_external(|ext| {
            ext.intervene("step", IntrospectValue::Int(1))
                .expect("scrub onto choice");
            assert_eq!(text(ext.query("mode")), "choice");
            ext.invoke("choose", IntrospectValue::Int(0))
                .expect("choose");
            assert_eq!(text(ext.query("outcome")), "answer");
            assert_eq!(text(ext.query("outcome_label")), "돌아본다");
            let mut tags = Vec::new();
            ext.drain_intents(&mut |i| tags.push(i.tag));
            assert!(tags.iter().any(|t| t == CHOSEN_INTENT));
        });
    }

    #[test]
    fn failure_surfaces_are_loud() {
        with_external(|ext| {
            // choose off a choice -> Rejected.
            assert!(matches!(
                ext.invoke("choose", IntrospectValue::Int(0)),
                Err(InvokeError::Rejected)
            ));
            // wrong tick type -> TypeMismatch.
            assert!(matches!(
                ext.invoke("tick", IntrospectValue::Text("soon".into())),
                Err(InvokeError::TypeMismatch)
            ));
            // unknown verb -> UnknownPath.
            assert!(matches!(
                ext.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            ));
            // intervene of a read-only field -> ReadOnly; unknown -> UnknownPath.
            assert!(matches!(
                ext.intervene("mode", IntrospectValue::Text("x".into())),
                Err(InterveneError::ReadOnly)
            ));
            assert!(matches!(
                ext.intervene("nope", IntrospectValue::Int(0)),
                Err(InterveneError::UnknownPath)
            ));
            // out-of-range choose on a real choice -> Rejected.
            ext.intervene("step", IntrospectValue::Int(1)).unwrap();
            assert!(matches!(
                ext.invoke("choose", IntrospectValue::Int(9)),
                Err(InvokeError::Rejected)
            ));
        });
    }
}
