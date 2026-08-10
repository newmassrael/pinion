//! The AI-first drive surface: a [`VnExternal`] wrapping the VN runner.
//!
//! The §5.15 `External` escape hatch used the way §2 #2 / #7 intend — not to
//! embed an opaque subsystem, but to expose the VN runner over the RPC
//! introspection triad so an AI agent (or the `hello-vn-tide` python demo)
//! drives the whole set-piece as data:
//!
//! - **query** — the dialogue: `mode` / `step` / `step_count` / `speaker` /
//!   `line` / `revealed` / `revealed_chars` / `line_len` / `fully_revealed` /
//!   `prompt` / `options` / `option_count` / `timeout_ms` / `remaining_ms` /
//!   `expired` / `resolved` / `outcome` / `outcome_label` / `timed_out` /
//!   `script`; and the stage: `background` / `sprites` / `sprite_count`.
//! - **intervene** — set `step` to scrub the play-head (clamped) — the
//!   save/load + AI-scrub seam.
//! - **invoke** — the dialogue verbs `tick {ms}` / `advance` /
//!   `choose {index}`; the save/load pair `save` (return the whole save state
//!   as an opaque JSON blob) / `load {blob}`; and the stage director verbs
//!   `set_background {source}` / `clear_background` / `show {id,source,at?,layer?}`
//!   / `hide {id}` / `move {id,at}` / `set_sprite_source {id,source}`.
//!
//! Every state transition emits a §5.20 intent (`vn.advanced` /
//! `vn.timeout` / `vn.chosen`). These are the retained-reducer signal: the
//! shell's dispatch tail drains them into `V::update` on the same call that
//! emits them, so they are **not** independently observable via a later
//! `scene/intents` poll (that is a framework-wide §5.20 v1 limitation, not a
//! VN one). An RPC client watches the play-through by polling the queryable
//! state (`mode` / `step` / `outcome` / …), which is the §2 #7 surface; the
//! intents' emission is covered by the crate unit tests. (Repaint itself is
//! driven by the shell's state-change detection, not by these intents — a
//! `tick` reveal repaints while emitting no intent.)
//!
//! ## The `tick` step-verb vs. the real-time clock — honestly scoped
//!
//! `tick {ms}` advances the runner by exactly `ms` of *logical* time, read
//! from the argument, never a wall clock — so every assertion in the headless
//! wire demo observes settled state with no background thread and no sleeps
//! ([[zero-flake-policy]]).
//!
//! **Real-time play is landed, not deferred.** [`crate::vn::clock::VnClock`] is
//! a retained [`Tickable`](pinion_core::animation::Tickable) (the `CaretBlink`
//! pattern) that an interactive window's paint cycle advances on the wall clock
//! — the-tide's felt urgency, and *not* Phase-C.
//!
//! The bespoke `tick` verb is genuinely forced for the deterministic harness —
//! but for the honest reason, not the earlier false "`scene/tick` can't reach a
//! retained `Scene::External`" claim. `scene/tick {dt}` *does* reach this
//! runner's clock (it enqueues a `DeferredInput::Tick` that ticks the same
//! `tick_animations` path). It simply cannot drive it *deterministically* over
//! the wire: the offscreen `PINION_HIDDEN_WINDOW` window still repaints on the
//! wall clock — even under `scene/set_fps 0` an RPC-induced repaint ticks the
//! animation clock by wall-clock — so a registered clock drifts the play-head
//! run-to-run (verified). The harness therefore registers no clock and drives
//! time by this verb; the interactive window gets real time from the clock.

use std::rc::Rc;

use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    SchemaField, int_of,
};
use pinion_core::intent::Intent;

use crate::vn::model::VnStep;
use crate::vn::stage::{SpritePos, VnSprite};
use crate::vn::state::{ChooseError, VnSave, VnState};

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
            // R1564 — the comment above already named the two, and they left
            // by the same door. They are different operator problems: one waits
            // for the script to reach a choice, the other picks a valid index.
            Err(ChooseError::NotAChoice) => Err(InvokeError::rejected(
                "choose: the script is not at a choice right now",
            )),
            Err(ChooseError::OutOfRange) => Err(InvokeError::rejected(format!(
                "choose: no option {index} at this choice"
            ))),
        }
    }

    /// The current stage (background + sprites) as JSON — returned by every
    /// director verb so a client sees the immediate result.
    fn stage_json(&self) -> IntrospectValue {
        IntrospectValue::json(&self.state.stage().data())
    }

    /// `show {id, source, at?, layer?}` — place / update a sprite on stage.
    fn show(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Json(v) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = v
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or(InvokeError::TypeMismatch)?;
        let source = v
            .get("source")
            .and_then(serde_json::Value::as_str)
            .ok_or(InvokeError::TypeMismatch)?;
        // `at` / `layer` are optional (default centre / layer 0); a present but
        // unrecognised `at` string is a hard type error, not a silent default.
        let at = match v.get("at").and_then(serde_json::Value::as_str) {
            Some(s) => SpritePos::parse(s).ok_or(InvokeError::TypeMismatch)?,
            None => SpritePos::default(),
        };
        let layer = u8::try_from(
            v.get("layer")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        )
        .unwrap_or(u8::MAX);
        self.state
            .stage()
            .show(VnSprite::new(id, source, at, layer));
        Ok(self.stage_json())
    }

    /// `move {id, at}` — reposition a sprite. A missing sprite is Rejected.
    fn move_sprite(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Json(v) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = v
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or(InvokeError::TypeMismatch)?;
        let at = v
            .get("at")
            .and_then(serde_json::Value::as_str)
            .and_then(SpritePos::parse)
            .ok_or(InvokeError::TypeMismatch)?;
        if self.state.stage().move_to(id, at) {
            Ok(self.stage_json())
        } else {
            Err(InvokeError::rejected(format!(
                "move: no sprite {id:?} on this stage"
            )))
        }
    }

    /// `set_sprite_source {id, source}` — swap a sprite's source (an
    /// expression change). A missing sprite is Rejected.
    fn set_sprite_source(
        &mut self,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Json(v) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = v
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or(InvokeError::TypeMismatch)?;
        let source = v
            .get("source")
            .and_then(serde_json::Value::as_str)
            .ok_or(InvokeError::TypeMismatch)?;
        if self.state.stage().set_source(id, source) {
            Ok(self.stage_json())
        } else {
            Err(InvokeError::rejected(format!(
                "set_sprite_source: no sprite {id:?} on this stage"
            )))
        }
    }
}

// Paints nothing (the binding's `view` is the paint scene) and drains
// `self.pending_intents` — the RPC-only read-write introspection skeleton.
pinion_core::intent_query_external_impl!(VnExternal);

/// R1637 §5.15 — every path this surface answers, as a `const` a WRAPPER can
/// compose with [`SchemaField::concat`] instead of restating.
///
/// It lived as an inline `const` block inside `schema()`, so a consumer
/// layering its own verbs over this one (`hello-vn-tide` adds `save_slot` /
/// `load_slot`) could only forward the declaration UNCHANGED — publishing a
/// contract that omitted its own two verbs while the wire happily ran them.
/// R1501 made composition the answer to that shape; this is what makes
/// composition reachable here.
pub const SCHEMA_FIELDS: &[SchemaField] = &[
    SchemaField::new("mode", "text"),
    SchemaField::new("step", "int"),
    SchemaField::new("step_count", "int"),
    SchemaField::new("speaker", "text"),
    SchemaField::new("line", "text"),
    SchemaField::new("revealed", "text"),
    SchemaField::new("revealed_chars", "int"),
    SchemaField::new("line_len", "int"),
    SchemaField::new("fully_revealed", "bool"),
    SchemaField::new("prompt", "text"),
    SchemaField::new("options", "json"),
    SchemaField::new("option_count", "int"),
    SchemaField::new("timeout_ms", "int"),
    SchemaField::new("remaining_ms", "int"),
    SchemaField::new("expired", "bool"),
    SchemaField::new("resolved", "bool"),
    SchemaField::new("outcome", "text"),
    SchemaField::new("outcome_label", "text"),
    SchemaField::new("timed_out", "bool"),
    SchemaField::new("script", "json"),
    SchemaField::new("background", "text"),
    SchemaField::new("sprites", "json"),
    SchemaField::new("sprite_count", "int"),
    // R1637 — the runner's and the stage director's verbs,
    // declared. The module doc above lists all eleven in prose
    // and the declaration listed none, so the surface built
    // "so an AI agent drives the whole set-piece as data"
    // published only the half an agent can read.
    SchemaField::action("tick", "json"),
    SchemaField::action("advance", "json"),
    SchemaField::action("choose", "json"),
    SchemaField::action("save", "json"),
    SchemaField::action("load", "json"),
    SchemaField::action("set_background", "json"),
    SchemaField::action("clear_background", "json"),
    SchemaField::action("show", "json"),
    SchemaField::action("hide", "json"),
    SchemaField::action("move", "json"),
    SchemaField::action("set_sprite_source", "json"),
];

impl ExternalIntrospect for VnExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(SCHEMA_FIELDS)
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
            "background" => Some(match self.state.stage().background() {
                Some(source) => IntrospectValue::Text(source),
                None => IntrospectValue::Null,
            }),
            "sprites" => Some(IntrospectValue::json(&self.state.stage().sprites())),
            "sprite_count" => Some(IntrospectValue::Int(int_of(
                self.state.stage().sprite_count(),
            ))),
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
                IntrospectValue::Json(v) => match serde_json::from_value::<VnSave>(v) {
                    Ok(save) => {
                        self.state.load(save);
                        Ok(self.status())
                    }
                    Err(_) => Err(InvokeError::TypeMismatch),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // ── stage director verbs (background + sprites) ──
            "set_background" => match args {
                IntrospectValue::Text(source) => {
                    self.state.stage().set_background(source);
                    Ok(self.stage_json())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "clear_background" => {
                let _ = self.state.stage().clear_background();
                Ok(self.stage_json())
            }
            "show" => self.show(&args),
            "hide" => match args {
                IntrospectValue::Text(id) => {
                    let _ = self.state.stage().hide(&id);
                    Ok(self.stage_json())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "move" => self.move_sprite(&args),
            "set_sprite_source" => self.set_sprite_source(&args),
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
    use pinion_core::test_fixtures::assert_refused_saying;

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
    fn stage_director_verbs_place_and_query_sprites() {
        with_external(|ext| {
            // Empty stage at boot.
            assert!(matches!(
                ext.query("background"),
                Some(IntrospectValue::Null)
            ));
            assert_eq!(int(ext.query("sprite_count")), 0);

            ext.invoke("set_background", IntrospectValue::Text("tideflat".into()))
                .expect("set_background");
            assert_eq!(text(ext.query("background")), "tideflat");

            // show {id, source, at, layer}
            let show = IntrospectValue::Json(serde_json::json!({
                "id": "mudang", "source": "mudang_calm", "at": "left", "layer": 1
            }));
            ext.invoke("show", show).expect("show");
            assert_eq!(int(ext.query("sprite_count")), 1);
            let sprites = match ext.query("sprites") {
                Some(IntrospectValue::Json(serde_json::Value::Array(a))) => a,
                other => panic!("sprites is a JSON array, got {other:?}"),
            };
            assert_eq!(sprites[0]["id"], "mudang");
            assert_eq!(sprites[0]["at"], "left");

            // move + re-express by id.
            ext.invoke(
                "move",
                IntrospectValue::Json(serde_json::json!({"id": "mudang", "at": "center"})),
            )
            .expect("move");
            ext.invoke(
                "set_sprite_source",
                IntrospectValue::Json(
                    serde_json::json!({"id": "mudang", "source": "mudang_grave"}),
                ),
            )
            .expect("re-express");
            let sprites = match ext.query("sprites") {
                Some(IntrospectValue::Json(serde_json::Value::Array(a))) => a,
                other => panic!("got {other:?}"),
            };
            assert_eq!(sprites[0]["at"], "center");
            assert_eq!(sprites[0]["source"], "mudang_grave");

            // hide, then clear background.
            ext.invoke("hide", IntrospectValue::Text("mudang".into()))
                .expect("hide");
            assert_eq!(int(ext.query("sprite_count")), 0);
            ext.invoke("clear_background", IntrospectValue::Null)
                .expect("clear");
            assert!(matches!(
                ext.query("background"),
                Some(IntrospectValue::Null)
            ));

            // Failure surfaces: missing id, bad position, move of an absent sprite.
            assert!(matches!(
                ext.invoke(
                    "show",
                    IntrospectValue::Json(serde_json::json!({"source": "x"}))
                ),
                Err(InvokeError::TypeMismatch)
            ));
            assert!(matches!(
                ext.invoke(
                    "show",
                    IntrospectValue::Json(
                        serde_json::json!({"id": "a", "source": "x", "at": "nope"})
                    )
                ),
                Err(InvokeError::TypeMismatch)
            ));
            assert_refused_saying(
                &ext.invoke(
                    "move",
                    IntrospectValue::Json(serde_json::json!({"id": "ghost", "at": "left"})),
                ),
                "no sprite \"ghost\" on this stage",
            );
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
            // choose off a choice -> Rejected. R1564 — and it says WHICH of
            // the two refusals it is; the out-of-range one below used to be
            // the same value.
            assert_refused_saying(
                &ext.invoke("choose", IntrospectValue::Int(0)),
                "the script is not at a choice right now",
            );
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
            assert_refused_saying(
                &ext.invoke("choose", IntrospectValue::Int(9)),
                "no option 9 at this choice",
            );
        });
    }
}
