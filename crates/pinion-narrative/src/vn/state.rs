//! The reactive VN runtime: a deterministic play-head over a [`VnScript`].
//!
//! One `Rc<VnState>` is the single source of truth for "where in the
//! set-piece are we, how much of the line has the typewriter revealed, and
//! how long is left on the timer". The immutable script is held behind an
//! `Rc`; the mutable part is a single [`Signal`] over a `Copy`
//! [`VnRuntime`]. The view reads it, the
//! [`VnExternal`](crate::vn::VnExternal) drives it — the same one-Rc-SSOT
//! reactive-holder pattern [`NarrativeState`](crate::state::NarrativeState)
//! uses.
//!
//! **Determinism is the whole point.** Advancement is a pure function of
//! caller-supplied `dt` — [`tick`](VnState::tick) never reads a wall clock.
//! That is what lets the runner be driven one fixed step at a time over the
//! JSON-RPC wire with zero flakiness ([[zero-flake-policy]]), exactly as the
//! audio arc's `render` step-verb pumps the mixer (R1293). The real
//! game-loop `scene/tick {dt}` fixed-timestep is the north-star driver, but
//! its fan-out reaches only `Scene::ImmediateModeNode` paint drivers, and
//! this runner is a retained structured-scene `Scene::External` (§2 #1 — the
//! dialogue is queryable text, not opaque paint), so it is stepped by its
//! own deterministic `tick` verb — the same honestly-scoped choice
//! `hello-audio-rt` made for the RT audio surface.

use std::rc::Rc;

use pinion_core::reactive::{Owner, Signal};
use serde::{Deserialize, Serialize};

use crate::vn::model::{VnScript, VnStep};
use crate::vn::stage::{StageData, VnStage};

/// Typewriter reveal rate, characters per second. A line of `n` characters
/// is fully revealed after `n / CPS` seconds of accumulated `tick` time.
const CPS: u64 = 40;

/// What the runner is currently presenting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VnMode {
    /// A line is being revealed (or is fully revealed, awaiting `advance`).
    Line,
    /// A timed choice is live (awaiting `choose`, or a `tick` that expires
    /// the countdown).
    Choice,
    /// The play-head has run past the last step.
    End,
}

impl VnMode {
    /// The stable wire string for this mode (`scene/query mode`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Choice => "choice",
            Self::End => "end",
        }
    }
}

/// The resolution of a [`VnStep::TimedChoice`] — which option was taken and
/// whether the countdown chose it. `Copy` so it lives in [`VnRuntime`]; the
/// option's label / outcome are looked up from the immutable script, not
/// stored here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnResolution {
    /// The step index of the choice that was resolved.
    pub step: u16,
    /// The index (into that choice's `options`) that was taken.
    pub option: u16,
    /// `true` if the countdown auto-selected it, `false` if the player
    /// chose it in time.
    pub timed_out: bool,
}

/// The `Copy` mutable runtime of a VN play-through — the complete save
/// state. Every field is `#[serde(default)]` so a partial / older save
/// deserializes tolerantly (a missing field falls back to its default)
/// rather than failing the whole load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnRuntime {
    /// The current step index. Equal to `script.len()` at [`VnMode::End`].
    #[serde(default)]
    pub step: u16,
    /// Milliseconds accumulated on the current step (reset to 0 on every
    /// transition). Drives typewriter reveal on a line and the countdown on
    /// a choice.
    #[serde(default)]
    pub elapsed_ms: u32,
    /// `true` once the player has snapped the current line to full reveal
    /// (an `advance` on a partly-revealed line).
    #[serde(default)]
    pub snapped: bool,
    /// The most-recently resolved choice, if any (`None` before the first
    /// choice resolves).
    #[serde(default)]
    pub resolved: Option<VnResolution>,
}

/// A `Copy` snapshot of the visible play-head state — the
/// [`WidgetCore::State`](pinion_core::WidgetCore::State) a binding reads
/// back each frame so the shell repaints when the typewriter or the
/// countdown moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnCursor {
    /// The current step index.
    pub step: u16,
    /// Characters of the current line revealed so far.
    pub revealed_chars: u16,
    /// Milliseconds left on the current choice's countdown (0 off a choice).
    pub remaining_ms: u32,
}

/// The complete save state of a play-through: the dialogue play-head *and*
/// the visual stage, so a load restores exactly what was on screen. Every
/// field is `#[serde(default)]` so a partial / older save loads tolerantly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnSave {
    /// The dialogue play-head (step / reveal / countdown / resolution).
    #[serde(default)]
    pub runtime: VnRuntime,
    /// The visual stage (background + sprites).
    #[serde(default)]
    pub stage: StageData,
}

/// The reactive VN read/drive-model.
#[derive(Debug)]
pub struct VnState {
    script: Rc<VnScript>,
    runtime: Signal<VnRuntime>,
    stage: VnStage,
}

impl VnState {
    /// Build a fresh runner over `script`, positioned at the first step
    /// with nothing revealed, no timer elapsed, and an empty stage.
    #[must_use]
    pub fn new(script: VnScript) -> Self {
        Self {
            script: Rc::new(script),
            runtime: Signal::new(VnRuntime::default()),
            stage: VnStage::new(),
        }
    }

    /// The immutable script backing this runner.
    #[must_use]
    pub fn script(&self) -> &VnScript {
        &self.script
    }

    /// The visual stage director (background + sprites).
    #[must_use]
    pub fn stage(&self) -> &VnStage {
        &self.stage
    }

    /// The current runtime snapshot.
    #[must_use]
    pub fn runtime(&self) -> VnRuntime {
        self.runtime.get()
    }

    /// Total step count.
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.script.len()
    }

    /// The current step, if the play-head is in range (`None` at
    /// [`VnMode::End`]).
    #[must_use]
    pub fn current_step(&self) -> Option<&VnStep> {
        self.script.step(usize::from(self.runtime.get().step))
    }

    /// What the runner is presenting right now.
    #[must_use]
    pub fn mode(&self) -> VnMode {
        match self.current_step() {
            Some(VnStep::Line { .. }) => VnMode::Line,
            Some(VnStep::TimedChoice { .. }) => VnMode::Choice,
            None => VnMode::End,
        }
    }

    /// The full character count of the current line (0 off a line).
    #[must_use]
    pub fn line_len(&self) -> usize {
        match self.current_step() {
            Some(VnStep::Line { text, .. }) => text.chars().count(),
            _ => 0,
        }
    }

    /// How many characters of the current line the typewriter has revealed.
    /// Fully revealed once snapped or once enough `tick` time has elapsed.
    #[must_use]
    pub fn revealed_chars(&self) -> usize {
        let Some(VnStep::Line { text, .. }) = self.current_step() else {
            return 0;
        };
        let full = text.chars().count();
        let rt = self.runtime.get();
        if rt.snapped {
            return full;
        }
        let by_time = usize::try_from(u64::from(rt.elapsed_ms) * CPS / 1000).unwrap_or(full);
        by_time.min(full)
    }

    /// The revealed prefix of the current line (empty off a line).
    #[must_use]
    pub fn revealed_text(&self) -> String {
        match self.current_step() {
            Some(VnStep::Line { text, .. }) => text.chars().take(self.revealed_chars()).collect(),
            _ => String::new(),
        }
    }

    /// `true` when the current line is fully revealed (always `true` off a
    /// line, so an `advance` never stalls at [`VnMode::End`]).
    #[must_use]
    pub fn fully_revealed(&self) -> bool {
        match self.current_step() {
            Some(VnStep::Line { .. }) => self.revealed_chars() >= self.line_len(),
            _ => true,
        }
    }

    /// The current choice's timeout in milliseconds (0 off a choice, or for
    /// an untimed choice).
    #[must_use]
    pub fn timeout_ms(&self) -> u32 {
        match self.current_step() {
            Some(VnStep::TimedChoice { timeout_ms, .. }) => *timeout_ms,
            _ => 0,
        }
    }

    /// Milliseconds left on the current choice's countdown (0 off a choice
    /// or for an untimed choice).
    #[must_use]
    pub fn remaining_ms(&self) -> u32 {
        let timeout = self.timeout_ms();
        if timeout == 0 {
            return 0;
        }
        timeout.saturating_sub(self.runtime.get().elapsed_ms)
    }

    /// `true` when the current choice has a live timer that has run out.
    /// Always `false` off a choice and for an untimed choice (`timeout_ms
    /// == 0`), which never auto-resolves.
    #[must_use]
    pub fn expired(&self) -> bool {
        let timeout = self.timeout_ms();
        timeout > 0 && self.runtime.get().elapsed_ms >= timeout
    }

    /// Advance the play-head by `dt_ms` of logical time. Grows the
    /// typewriter reveal on a line; decrements the countdown on a choice and
    /// auto-resolves it (to the clamped `default_option`) if the timer runs
    /// out. Returns `true` iff a choice resolved via timeout during this
    /// tick.
    #[must_use]
    pub fn tick(&self, dt_ms: u32) -> bool {
        let mut rt = self.runtime.get();
        // At End there is nothing to advance.
        if usize::from(rt.step) >= self.script.len() {
            return false;
        }
        rt.elapsed_ms = rt.elapsed_ms.saturating_add(dt_ms);

        if let Some(VnStep::TimedChoice {
            options,
            timeout_ms,
            default_option,
            ..
        }) = self.script.step(usize::from(rt.step))
            && *timeout_ms > 0
            && rt.elapsed_ms >= *timeout_ms
        {
            let option = clamp_option(*default_option, options.len());
            let goto = options.get(usize::from(option)).and_then(|o| o.goto);
            let target = self.step_after(rt.step, goto);
            rt.resolved = Some(VnResolution {
                step: rt.step,
                option,
                timed_out: true,
            });
            rt.step = target;
            rt.elapsed_ms = 0;
            rt.snapped = false;
            self.runtime.set(rt);
            return true;
        }

        self.runtime.set(rt);
        false
    }

    /// The step the play-head moves to after leaving `current`: the explicit
    /// `goto` branch target if set, else the next step — both clamped into
    /// `[0, len]` (`len` = End).
    fn step_after(&self, current: u16, goto: Option<u16>) -> u16 {
        let max = u16::try_from(self.script.len()).unwrap_or(u16::MAX);
        match goto {
            Some(target) => target.min(max),
            None => current.saturating_add(1).min(max),
        }
    }

    /// Player action on a line: snap a partly-revealed line to full, or (if
    /// already full) step to the next step. A no-op on a choice (the player
    /// must [`choose`](Self::choose) or let it time out) and at
    /// [`VnMode::End`]. Returns whether anything changed.
    #[must_use]
    pub fn advance(&self) -> bool {
        let mut rt = self.runtime.get();
        match self.script.step(usize::from(rt.step)) {
            Some(VnStep::Line { goto, .. }) => {
                if self.fully_revealed() {
                    rt.step = self.step_after(rt.step, *goto);
                    rt.elapsed_ms = 0;
                    rt.snapped = false;
                    self.runtime.set(rt);
                    true
                } else {
                    rt.snapped = true;
                    self.runtime.set(rt);
                    true
                }
            }
            // A choice must be resolved by `choose` / timeout; at End there
            // is nowhere to advance.
            _ => false,
        }
    }

    /// Player action on a choice: take option `index`, recording it as the
    /// resolution and stepping past the choice. Errors if the current step
    /// is not a live choice or `index` is out of range.
    ///
    /// # Errors
    ///
    /// - [`ChooseError::NotAChoice`] — the play-head is on a line or at End.
    /// - [`ChooseError::OutOfRange`] — `index >= options.len()`.
    pub fn choose(&self, index: usize) -> Result<(), ChooseError> {
        let mut rt = self.runtime.get();
        match self.script.step(usize::from(rt.step)) {
            Some(VnStep::TimedChoice { options, .. }) => {
                if index >= options.len() {
                    return Err(ChooseError::OutOfRange);
                }
                let target = self.step_after(rt.step, options[index].goto);
                rt.resolved = Some(VnResolution {
                    step: rt.step,
                    option: u16::try_from(index).unwrap_or(u16::MAX),
                    timed_out: false,
                });
                rt.step = target;
                rt.elapsed_ms = 0;
                rt.snapped = false;
                self.runtime.set(rt);
                Ok(())
            }
            _ => Err(ChooseError::NotAChoice),
        }
    }

    /// Jump the play-head to `step`, clamped into `[0, len]` (`len` = End).
    /// Resets the current-step timer / reveal; keeps the resolution history.
    /// The seam a save/load or an AI scrub drives (mirrors
    /// [`NarrativeState::goto`](crate::state::NarrativeState::goto)).
    /// Returns whether the play-head moved.
    #[must_use]
    pub fn goto(&self, step: u16) -> bool {
        let max = u16::try_from(self.script.len()).unwrap_or(u16::MAX);
        let step = step.min(max);
        let mut rt = self.runtime.get();
        if rt.step == step {
            return false;
        }
        rt.step = step;
        rt.elapsed_ms = 0;
        rt.snapped = false;
        self.runtime.set(rt);
        true
    }

    /// The complete save state of this play-through — the dialogue play-head
    /// *and* the visual stage — to hand back to [`load`](Self::load) later and
    /// resume exactly here.
    #[must_use]
    pub fn save(&self) -> VnSave {
        VnSave {
            runtime: self.runtime.get(),
            stage: self.stage.data(),
        }
    }

    /// Restore the play-head and stage from a [`VnSave`] — the load half of
    /// save/load. The saved `step` is clamped into `[0, len]`, so a save
    /// written against a different / shorter script cannot land out of range;
    /// a dangling `resolved` pointer degrades to no readable outcome (see
    /// [`resolved_option`](Self::resolved_option)) rather than a panic.
    /// `elapsed_ms` / `snapped` are restored as-is — they re-derive the
    /// typewriter reveal and the countdown deterministically — and the whole
    /// stage is restored so a load brings back what was on screen.
    pub fn load(&self, save: VnSave) {
        let max = u16::try_from(self.script.len()).unwrap_or(u16::MAX);
        self.runtime.set(VnRuntime {
            step: save.runtime.step.min(max),
            ..save.runtime
        });
        self.stage.set_data(save.stage);
    }

    /// The most-recently resolved choice, if any.
    #[must_use]
    pub fn resolution(&self) -> Option<VnResolution> {
        self.runtime.get().resolved
    }

    /// The taken option of the most-recent resolution, looked up from the
    /// script (`None` if nothing resolved or the record dangles).
    #[must_use]
    pub fn resolved_option(&self) -> Option<&crate::vn::model::VnOption> {
        let r = self.runtime.get().resolved?;
        match self.script.step(usize::from(r.step)) {
            Some(VnStep::TimedChoice { options, .. }) => options.get(usize::from(r.option)),
            _ => None,
        }
    }
}

/// Clamp an author-declared default-option index into a non-empty option
/// list; an out-of-range value (or an empty list) degrades to 0.
fn clamp_option(index: usize, count: usize) -> u16 {
    if count == 0 {
        return 0;
    }
    u16::try_from(index.min(count - 1)).unwrap_or(0)
}

/// Why a [`VnState::choose`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooseError {
    /// The play-head is not on a choice (it is on a line or at End).
    NotAChoice,
    /// The option index is `>=` the choice's option count.
    OutOfRange,
}

/// Retrieve (building once) the `Rc<VnState>` for `key` from the current
/// [`Owner`] scope — the one-Rc SSOT the view and the External share.
/// `load` produces the script the first time only.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope.
#[must_use]
pub fn use_vn_state(key: &'static str, load: impl FnOnce() -> VnScript) -> Rc<VnState> {
    Owner::current()
        .expect("use_vn_state must be called within an active Owner scope")
        .cache(key, || VnState::new(load()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vn::model::{VnOption, VnStep};
    use pinion_core::reactive::Owner;

    fn script() -> VnScript {
        VnScript::new(vec![
            VnStep::line("무녀", "돌아오지 마라"), // 8 chars
            VnStep::timed_choice(
                "부름이 들린다",
                vec![
                    VnOption::new("돌아본다", "answer"),
                    VnOption::new("버틴다", "endure"),
                ],
                4000,
                1, // default = 버틴다 / endure
            ),
            VnStep::narration("물이 발목을 감는다"),
        ])
    }

    fn with_state<R>(body: impl FnOnce(&VnState) -> R) -> R {
        let owner = Owner::new();
        owner.run(|| body(&VnState::new(script())))
    }

    #[test]
    fn typewriter_reveals_over_time_and_snaps() {
        with_state(|s| {
            assert_eq!(s.mode(), VnMode::Line);
            assert_eq!(s.line_len(), 7); // "돌아오지 마라" = 7 chars (space counts)
            assert_eq!(s.revealed_chars(), 0, "nothing revealed at t=0");
            // 40 cps -> 100 ms reveals 4 chars.
            assert!(!s.tick(100));
            assert_eq!(s.revealed_chars(), 4);
            assert_eq!(s.revealed_text(), "돌아오지");
            assert!(!s.fully_revealed());
            // advance() on a partly-revealed line snaps it to full.
            assert!(s.advance());
            assert!(s.fully_revealed());
            assert_eq!(s.revealed_text(), "돌아오지 마라");
            // advance() on a full line steps to the next (the choice).
            assert!(s.advance());
            assert_eq!(s.mode(), VnMode::Choice);
        });
    }

    #[test]
    fn countdown_times_out_to_default_option() {
        with_state(|s| {
            assert!(s.advance()); // snap the line
            assert!(s.advance()); // step onto the choice
            assert_eq!(s.mode(), VnMode::Choice);
            assert_eq!(s.timeout_ms(), 4000);
            assert_eq!(s.remaining_ms(), 4000);
            assert!(!s.expired());
            // Drain 3.5 s — still live.
            assert!(!s.tick(3500));
            assert_eq!(s.remaining_ms(), 500);
            // The tick that crosses the timeout resolves to the default.
            assert!(s.tick(600), "the countdown expired this tick");
            assert_eq!(s.mode(), VnMode::Line, "stepped past the choice");
            let r = s.resolution().expect("resolved");
            assert_eq!(r.step, 1);
            assert_eq!(r.option, 1, "default_option was 1");
            assert!(r.timed_out);
            assert_eq!(
                s.resolved_option().map(|o| o.outcome.as_str()),
                Some("endure")
            );
        });
    }

    #[test]
    fn choose_before_timeout_records_the_player_option() {
        with_state(|s| {
            assert!(s.goto(1)); // jump straight onto the choice
            assert_eq!(s.mode(), VnMode::Choice);
            s.choose(0).expect("valid choice");
            let r = s.resolution().expect("resolved");
            assert_eq!(r.option, 0);
            assert!(!r.timed_out, "player chose in time");
            assert_eq!(
                s.resolved_option().map(|o| o.outcome.as_str()),
                Some("answer")
            );
            assert_eq!(s.mode(), VnMode::Line, "stepped past the choice");
        });
    }

    #[test]
    fn choose_errors_off_a_choice_and_out_of_range() {
        with_state(|s| {
            assert_eq!(s.choose(0), Err(ChooseError::NotAChoice), "on a line");
            assert!(s.goto(1));
            assert_eq!(s.choose(9), Err(ChooseError::OutOfRange));
        });
    }

    #[test]
    fn play_head_reaches_end_and_is_inert() {
        with_state(|s| {
            assert!(s.goto(3)); // len == 3 -> End
            assert_eq!(s.mode(), VnMode::End);
            assert!(!s.tick(1000), "End does not advance");
            assert!(!s.advance(), "End does not advance");
            assert!(s.fully_revealed(), "End reads as fully revealed");
        });
    }

    #[test]
    fn empty_script_is_immediately_end() {
        let owner = Owner::new();
        owner.run(|| {
            let s = VnState::new(VnScript::default());
            assert_eq!(s.mode(), VnMode::End);
            assert!(!s.tick(100));
            assert!(!s.advance());
        });
    }

    /// A diamond: choice at step 0 branches to step 1 (option A) or step 3
    /// (option B); the option-A line jumps to the End so it does not flow
    /// into the option-B branch.
    fn branching_script() -> VnScript {
        VnScript::new(vec![
            VnStep::timed_choice(
                "goes where?",
                vec![
                    VnOption::new("A", "a").goto(1),
                    VnOption::new("B", "b").goto(3),
                ],
                2000,
                0, // timeout -> option A (goto 1)
            ),
            VnStep::narration("branch A step 1").with_goto(4), // jump past B
            VnStep::narration("branch A step 2 (skipped by the jump)"),
            VnStep::narration("branch B step 3"),
            VnStep::narration("shared ending step 4"),
        ])
    }

    #[test]
    fn choose_option_branches_to_its_goto_target() {
        let owner = Owner::new();
        owner.run(|| {
            let s = VnState::new(branching_script());
            assert_eq!(s.mode(), VnMode::Choice);
            s.choose(1).expect("option B"); // goto 3
            assert_eq!(s.runtime().step, 3, "branched to B's target, not step 1");
            assert_eq!(s.resolved_option().map(|o| o.outcome.as_str()), Some("b"));
        });
    }

    #[test]
    fn timeout_branches_via_the_default_options_goto() {
        let owner = Owner::new();
        owner.run(|| {
            let s = VnState::new(branching_script());
            assert!(s.tick(2500), "timed out"); // default option 0 -> goto 1
            assert_eq!(s.runtime().step, 1, "timeout took option A's branch");
            assert!(s.resolution().is_some_and(|r| r.timed_out));
        });
    }

    #[test]
    fn line_goto_jumps_past_the_other_branch() {
        let owner = Owner::new();
        owner.run(|| {
            let s = VnState::new(branching_script());
            s.choose(0).expect("option A"); // goto 1
            assert_eq!(s.runtime().step, 1);
            // Advancing past step 1 jumps to step 4 (its with_goto), skipping
            // steps 2 and 3 (branch B), not flowing linearly into them.
            assert!(s.advance()); // snap
            assert!(s.advance()); // jump to 4
            assert_eq!(s.runtime().step, 4, "line goto skipped the B branch");
        });
    }

    #[test]
    fn goto_target_past_end_clamps_to_end() {
        let owner = Owner::new();
        owner.run(|| {
            // Empty text -> line_len 0 -> fully revealed at once, so one
            // advance takes the jump.
            let s = VnState::new(VnScript::new(vec![VnStep::narration("").with_goto(99)]));
            assert!(s.fully_revealed());
            assert!(s.advance()); // jump via goto(99)
            assert_eq!(s.mode(), VnMode::End, "an over-range goto clamps to End");
        });
    }

    #[test]
    fn save_and_load_round_trip_restores_play_head_and_stage() {
        use crate::vn::stage::{SpritePos, VnSprite};
        with_state(|s| {
            // Drive to a distinctive mid-line state AND stage a sprite.
            assert!(!s.tick(100)); // reveal 4 chars of the first line
            s.stage().set_background("tideflat");
            s.stage()
                .show(VnSprite::new("mudang", "calm", SpritePos::Center, 1));
            let saved = s.save();
            assert_eq!(saved.runtime.step, 0);
            assert_eq!(saved.stage.sprites.len(), 1);

            // Mutate both surfaces away.
            assert!(s.advance());
            assert!(s.advance());
            assert_eq!(s.mode(), VnMode::Choice);
            assert!(s.stage().hide("mudang"));
            assert!(s.stage().clear_background());
            assert_ne!(s.save(), saved, "state moved on");

            // Load restores exactly the saved play-head AND stage.
            s.load(saved.clone());
            assert_eq!(s.save(), saved);
            assert_eq!(s.mode(), VnMode::Line);
            assert_eq!(s.revealed_chars(), 4, "reveal re-derived from elapsed");
            assert_eq!(s.stage().background().as_deref(), Some("tideflat"));
            assert_eq!(s.stage().sprite_count(), 1, "the sprite came back");
        });
    }

    #[test]
    fn load_clamps_an_out_of_range_saved_step() {
        with_state(|s| {
            // A save written against a longer script (or a corrupt one) with a
            // step past the end must clamp to End, not panic or read garbage.
            s.load(VnSave {
                runtime: VnRuntime {
                    step: 999,
                    ..VnRuntime::default()
                },
                ..VnSave::default()
            });
            assert_eq!(s.mode(), VnMode::End);
            assert_eq!(
                s.save().runtime.step,
                u16::try_from(s.step_count()).unwrap()
            );
        });
    }

    #[test]
    fn save_survives_a_json_round_trip_and_is_tolerant() {
        with_state(|s| {
            assert!(s.goto(1));
            s.choose(0).expect("choose"); // a resolved state to serialize
            let saved = s.save();
            let json = serde_json::to_string(&saved).expect("serialize");
            let back: VnSave = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, saved, "the save blob round-trips through JSON");
            // A partial blob loads tolerantly (missing fields -> defaults).
            let partial: VnSave =
                serde_json::from_str(r#"{"runtime":{"step":2}}"#).expect("partial");
            assert_eq!(partial.runtime.step, 2);
            assert!(partial.stage.sprites.is_empty());
        });
    }
}
