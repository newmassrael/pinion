//! The authored VN-script contract — the presentation-layer data a runner
//! plays.
//!
//! A visual-novel script is an ordered list of [`VnStep`]s: narrated /
//! spoken [`VnStep::Line`]s revealed by a typewriter, and
//! [`VnStep::TimedChoice`]s the player must answer before a countdown
//! expires. This is the **VN render axis** the-tide sits on
//! (`claudedocs/the-tide-vn-renpy-parity-requirements.md` §7): additive to
//! the [`crate::model`] scene-walk, not a replacement — where the walk is
//! the story's discrete skeleton (title / intent / disclosure), a VN script
//! is the *presentation* of one set-piece (who speaks, what is said, what
//! the player may do, and how long they have).
//!
//! The type is `serde`-derived so a script can be authored in Rust (as
//! `hello-vn-tide` does) today and loaded from a Mnemosyne projection later
//! — the same forward/backward-compatible posture [`crate::model`] takes:
//! every field is `#[serde(default)]` and the enum is `kind`-tagged, so an
//! older runner tolerates a newer script.

use serde::{Deserialize, Serialize};

use crate::vn::stage::StageOp;

/// One authored option in a [`VnStep::TimedChoice`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnOption {
    /// The label shown to the player (e.g. `"돌아본다"`).
    #[serde(default)]
    pub label: String,
    /// The outcome tag recorded when this option is taken — the seam a
    /// later round reads to map an outcome to a Mnemosyne world-line. An
    /// opaque, queryable string.
    #[serde(default)]
    pub outcome: String,
    /// Where the play-head goes when this option is taken: `Some(step)`
    /// branches to that step (clamped into range), `None` falls through to
    /// the next step. This is what makes a choice *matter* — different
    /// options lead to different steps. The pinion-side precursor of the
    /// [`ForkTree`](crate::model::ForkTree) branch topology the narrative
    /// scene-walk already models.
    #[serde(default)]
    pub goto: Option<u16>,
}

impl VnOption {
    /// Build an option from a player-facing `label` and its `outcome` tag,
    /// falling through to the next step when taken (no branch).
    #[must_use]
    pub fn new(label: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            outcome: outcome.into(),
            goto: None,
        }
    }

    /// Branch to step `to` when this option is taken (builder form).
    #[must_use]
    pub const fn goto(mut self, to: u16) -> Self {
        self.goto = Some(to);
        self
    }
}

/// One step in a [`VnScript`].
///
/// `kind`-tagged for a stable JSON wire form (`{"kind":"line",…}` /
/// `{"kind":"timed_choice",…}`) so the whole script is queryable as data
/// over `scene/query` (§2 #7) and loadable from an external projection.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VnStep {
    /// A narrated / spoken line, revealed character-by-character by the
    /// runner's typewriter.
    Line {
        /// Who speaks (empty for narration).
        #[serde(default)]
        speaker: String,
        /// The full line text; the typewriter reveals a growing prefix of
        /// it.
        #[serde(default)]
        text: String,
        /// Where the play-head goes when this line is advanced past:
        /// `Some(step)` jumps there (clamped), `None` falls through to the
        /// next step. Lets a branch's terminal line jump to the End (or a
        /// shared convergence step) instead of flowing into the next
        /// branch's steps — the peer of [`VnOption::goto`].
        #[serde(default)]
        goto: Option<u16>,
        /// Stage directives applied when the play-head enters this step, so
        /// the script drives its own staging (Ren'Py's interleaved
        /// `scene` / `show`). Empty = leave the persistent stage unchanged.
        #[serde(default)]
        stage: Vec<StageOp>,
    },
    /// A choice the player must resolve before the countdown expires —
    /// the-tide's "급박함" (urgency) core.
    TimedChoice {
        /// The question posed above the options.
        #[serde(default)]
        prompt: String,
        /// The options, in display order.
        #[serde(default)]
        options: Vec<VnOption>,
        /// How long (milliseconds) the player has before the choice
        /// auto-resolves to [`Self::TimedChoice::default_option`].
        #[serde(default)]
        timeout_ms: u32,
        /// Index into `options` selected automatically on timeout (clamped
        /// into range by the runner, so an out-of-range author value
        /// degrades to the first option rather than panicking).
        #[serde(default)]
        default_option: usize,
        /// Stage directives applied on entry (see [`Self::Line::stage`]).
        #[serde(default)]
        stage: Vec<StageOp>,
    },
}

impl VnStep {
    /// Build a spoken [`Self::Line`] that falls through to the next step.
    #[must_use]
    pub fn line(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Line {
            speaker: speaker.into(),
            text: text.into(),
            goto: None,
            stage: Vec::new(),
        }
    }

    /// Build a narrated [`Self::Line`] (no speaker).
    #[must_use]
    pub fn narration(text: impl Into<String>) -> Self {
        Self::Line {
            speaker: String::new(),
            text: text.into(),
            goto: None,
            stage: Vec::new(),
        }
    }

    /// Set a jump target on a [`Self::Line`] — advancing past it goes to
    /// `to` (clamped) instead of the next step. A no-op on a
    /// [`Self::TimedChoice`] (choice branching lives on each
    /// [`VnOption::goto`]).
    #[must_use]
    pub fn with_goto(mut self, to: u16) -> Self {
        if let Self::Line { goto, .. } = &mut self {
            *goto = Some(to);
        }
        self
    }

    /// Attach stage directives applied when the play-head enters this step
    /// (builder form) — the script driving its own staging.
    #[must_use]
    pub fn with_stage(mut self, ops: Vec<StageOp>) -> Self {
        match &mut self {
            Self::Line { stage, .. } | Self::TimedChoice { stage, .. } => *stage = ops,
        }
        self
    }

    /// The stage directives to apply when entering this step (empty if none).
    #[must_use]
    pub fn stage_ops(&self) -> &[StageOp] {
        match self {
            Self::Line { stage, .. } | Self::TimedChoice { stage, .. } => stage,
        }
    }

    /// Build a [`Self::TimedChoice`].
    #[must_use]
    pub fn timed_choice(
        prompt: impl Into<String>,
        options: Vec<VnOption>,
        timeout_ms: u32,
        default_option: usize,
    ) -> Self {
        Self::TimedChoice {
            prompt: prompt.into(),
            options,
            timeout_ms,
            default_option,
            stage: Vec::new(),
        }
    }

    /// `true` when this step is a [`Self::Line`].
    #[must_use]
    pub const fn is_line(&self) -> bool {
        matches!(self, Self::Line { .. })
    }

    /// `true` when this step is a [`Self::TimedChoice`].
    #[must_use]
    pub const fn is_choice(&self) -> bool {
        matches!(self, Self::TimedChoice { .. })
    }
}

/// An ordered VN script — the presentation-layer data a runner plays.
///
/// A `Vec<VnStep>` addressed by index: steps play in order by default, and
/// branching is expressed by per-step jump targets ([`VnOption::goto`] on a
/// choice, [`VnStep::with_goto`] on a line) rather than a nested tree — the
/// flat, index-addressed shape the wire form and an external projection both
/// consume most easily.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnScript {
    /// The steps to play, in author order.
    #[serde(default)]
    pub steps: Vec<VnStep>,
    /// Typewriter reveal rate in characters per second — the authored text
    /// speed (Ren'Py's `config.text_cps`). Defaults to
    /// [`DEFAULT_TEXT_SPEED_CPS`]; a value of 0 is treated as 1 by the runner
    /// so a line always reveals.
    #[serde(default = "default_text_speed")]
    pub text_speed_cps: u32,
}

impl Default for VnScript {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            text_speed_cps: DEFAULT_TEXT_SPEED_CPS,
        }
    }
}

/// The default typewriter speed, characters per second.
pub const DEFAULT_TEXT_SPEED_CPS: u32 = 40;

fn default_text_speed() -> u32 {
    DEFAULT_TEXT_SPEED_CPS
}

impl VnScript {
    /// Build a script from its steps at the default text speed.
    #[must_use]
    pub fn new(steps: Vec<VnStep>) -> Self {
        Self {
            steps,
            text_speed_cps: DEFAULT_TEXT_SPEED_CPS,
        }
    }

    /// Set the typewriter reveal rate (characters per second) — builder form.
    #[must_use]
    pub fn with_text_speed(mut self, cps: u32) -> Self {
        self.text_speed_cps = cps;
        self
    }

    /// Number of steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// `true` when the script has no steps — the runner degrades to an
    /// immediate `End` rather than panicking.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The step at `index`, if in range.
    #[must_use]
    pub fn step(&self, index: usize) -> Option<&VnStep> {
        self.steps.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_script_is_inert() {
        let script = VnScript::default();
        assert!(script.is_empty());
        assert_eq!(script.len(), 0);
        assert!(script.step(0).is_none());
    }

    #[test]
    fn step_constructors_classify() {
        assert!(VnStep::line("무녀", "돌아오지 마라").is_line());
        assert!(VnStep::narration("물이 찬다").is_line());
        assert!(
            VnStep::timed_choice("어쩔 텐가", vec![VnOption::new("버틴다", "hold")], 3000, 0)
                .is_choice()
        );
    }

    #[test]
    fn script_round_trips_through_json() {
        let script = VnScript::new(vec![
            VnStep::narration("갯벌에 물이 들어온다"),
            VnStep::timed_choice(
                "부름이 들린다",
                vec![
                    VnOption::new("돌아본다", "answer"),
                    VnOption::new("버틴다", "endure"),
                ],
                4000,
                1,
            ),
        ]);
        let json = serde_json::to_string(&script).expect("serialize");
        let back: VnScript = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(script, back);
        // The `kind` tag is the stable wire discriminator.
        assert!(json.contains("\"kind\":\"line\""));
        assert!(json.contains("\"kind\":\"timed_choice\""));
    }

    #[test]
    fn goto_builders_set_branch_targets_and_round_trip() {
        let opt = VnOption::new("돌아본다", "turn").goto(3);
        assert_eq!(opt.goto, Some(3));
        let line = VnStep::line("무녀", "끝").with_goto(9);
        assert!(matches!(line, VnStep::Line { goto: Some(9), .. }));
        // with_goto is a no-op on a choice (branching lives on the options).
        let choice = VnStep::timed_choice("?", vec![opt.clone()], 1000, 0).with_goto(5);
        assert!(choice.is_choice());

        let script = VnScript::new(vec![choice, line]);
        let back: VnScript =
            serde_json::from_str(&serde_json::to_string(&script).unwrap()).unwrap();
        assert_eq!(script, back, "goto targets survive the wire round-trip");
    }

    #[test]
    fn options_without_goto_default_to_fall_through() {
        let opt = VnOption::new("버틴다", "endure");
        assert_eq!(opt.goto, None, "no branch by default");
        let line = VnStep::narration("계속");
        assert!(matches!(line, VnStep::Line { goto: None, .. }));
    }

    #[test]
    fn tolerant_deserialize_fills_defaults() {
        // A line with only text; a choice with only a prompt.
        let json = r#"{"steps":[
            {"kind":"line","text":"..."},
            {"kind":"timed_choice","prompt":"?"}
        ]}"#;
        let script: VnScript = serde_json::from_str(json).expect("tolerant");
        assert_eq!(script.len(), 2);
        match script.step(1) {
            Some(VnStep::TimedChoice {
                options,
                timeout_ms,
                default_option,
                ..
            }) => {
                assert!(options.is_empty());
                assert_eq!(*timeout_ms, 0);
                assert_eq!(*default_option, 0);
            }
            other => panic!("expected a choice, got {other:?}"),
        }
    }
}
