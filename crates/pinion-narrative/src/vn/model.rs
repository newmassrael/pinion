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

/// One authored option in a [`VnStep::TimedChoice`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnOption {
    /// The label shown to the player (e.g. `"돌아본다"`).
    #[serde(default)]
    pub label: String,
    /// The outcome tag recorded when this option is taken — the seam a
    /// later round reads to branch the telling (map an outcome to a
    /// world-line). For the MVP it is an opaque, queryable string.
    #[serde(default)]
    pub outcome: String,
}

impl VnOption {
    /// Build an option from a player-facing `label` and its `outcome` tag.
    #[must_use]
    pub fn new(label: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            outcome: outcome.into(),
        }
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
    },
}

impl VnStep {
    /// Build a spoken [`Self::Line`].
    #[must_use]
    pub fn line(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Line {
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    /// Build a narrated [`Self::Line`] (no speaker).
    #[must_use]
    pub fn narration(text: impl Into<String>) -> Self {
        Self::Line {
            speaker: String::new(),
            text: text.into(),
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
/// Deliberately just a `Vec<VnStep>`: an authored script is linear, and
/// branching (an outcome jumping to a different step / world-line) is a
/// later round's addition that reads the [`VnOption::outcome`] tags, not a
/// shape this MVP bakes in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VnScript {
    /// The steps to play, in author order.
    #[serde(default)]
    pub steps: Vec<VnStep>,
}

impl VnScript {
    /// Build a script from its steps.
    #[must_use]
    pub fn new(steps: Vec<VnStep>) -> Self {
        Self { steps }
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
