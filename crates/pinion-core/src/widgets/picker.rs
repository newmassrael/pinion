//! ★★★★★ R1732 §5.38 §5.40 §2 #7 — **choosing one of a fixed set of words from
//! a control that is collapsed until you open it.**
//!
//! # What was missing, measured
//!
//! [`FieldType::Choice`](super::config_form::FieldType::Choice) is "exactly one
//! of these words", and every surface that drew one drew **all** of them, side
//! by side, always. Measured on the analysis tool's own inspector with
//! `FormStyle::default()`: a three-word roster spends 229 px of a 284 px pane
//! on a row whose answer is one word, a six-word roster overruns its control by
//! 50 px and a seven-word one by 113 px — the option rectangles are laid from
//! the control's left edge with no wrap, no clip and no scroll, so they are
//! simply painted outside it.
//!
//! The behaviour reference collapses the same field: its inspector draws a text
//! field, a switch, a two-word permission pair and — for an enumeration — a
//! **closed** control showing the current word, whose roster appears only when
//! it is opened. So the expanded row is not merely wide, it is not what is
//! being reproduced.
//!
//! # What this type is, and what it is not
//!
//! A `Picker` is the **transient state of choosing**: the roster, and where in
//! it the reader currently is. It is *not* the value — the value lives in the
//! document the form edits, and stays there untouched until a choice is
//! committed. A screen that has no `Picker` has no open picker; there is no
//! `open` flag, because "closed and highlighting the fourth option" is a state
//! that should not be spellable.
//!
//! # Against the reference toolkit at 6.11.1
//!
//! Measured by building a probe against its collapsed chooser and **running**
//! it, not by reading about it.
//!
//! | question | there | here |
//! |---|---|---|
//! | ask what committing right now would choose | **nothing answers.** Of 123 members, exactly two name the highlight and **both are signals** — an event you had to be listening for, never a value | [`Picker::highlighted`] |
//! | a check written the natural way, while the reader has moved the highlight | **passes.** Asserted the committed index while the open roster showed a different row, and it held | [`Picker::at`] is the same fact the paint reads |
//! | a roster with nothing in it | **accepted** — count 0, index −1, empty text, no complaint | [`PickerDefect::NoOptions`] |
//! | a word the document holds that the roster does not offer | **silently ignored**: the call returns `void`, emits no signal, and the control goes on showing another word | [`Picker::holding`] names it |
//! | an index past the end | **accepted**, and clears the control to nothing | [`Picker::point_at`] returns `false` and moves nothing |
//! | typing a letter, on the closed control | **commits.** The value moved from the first word to the fifth without the roster ever being shown | opening is what a letter does; see [`Picked`] |
//! | typing it again, over a roster with three words starting with it | **does not advance** — four presses all stayed on the second match | [`Picker::typed`] advances and wraps |
//! | arrows on the closed control | **commit, one document write per press**, and do not wrap at the end | arrows open, and movement is not a write |
//!
//! The second row is the one worth the module. A conformance check is only
//! worth writing if it fails when the product stops matching, and there the
//! most natural check cannot see the difference the reader is looking at.
//!
//! # Examples
//!
//! ```
//! use pinion_core::widgets::picker::{Picked, Picker};
//!
//! let mut picker = Picker::over(["block", "drop", "defer"], "drop")
//!     .expect("a roster with something in it");
//! assert_eq!(picker.highlighted(), "drop");
//!
//! // Moving is not writing: the document still holds `drop` until a choice.
//! assert_eq!(picker.key("ArrowDown"), Picked::Moved);
//! assert_eq!(picker.highlighted(), "defer");
//! assert_eq!(picker.key("ArrowDown"), Picked::Moved, "and it wraps");
//! assert_eq!(picker.highlighted(), "block");
//!
//! assert_eq!(picker.key("Enter"), Picked::Chose("block".to_owned()));
//! ```

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Why a roster could not be picked from.
///
/// One arm, and it is the one the floor accepts: a chooser over nothing is a
/// control that can only disappoint, and the place to refuse it is where it is
/// built rather than where somebody presses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerDefect {
    /// The roster was empty.
    NoOptions,
}

impl PickerDefect {
    /// What went wrong, for a reader.
    #[must_use]
    pub const fn sentence(&self) -> &'static str {
        match self {
            Self::NoOptions => "a picker needs at least one option to offer",
        }
    }
}

impl std::fmt::Display for PickerDefect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.sentence())
    }
}

impl std::error::Error for PickerDefect {}

/// What a key did to a picker.
///
/// A value rather than a pair of callbacks, because the two interesting answers
/// — *this is what committing chose* and *this was dismissed* — are the ones a
/// caller has to act on, and a caller that has to remember to subscribe is a
/// caller that will not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// The highlight moved. Nothing has been written.
    Moved,
    /// The reader chose this word. The caller writes it wherever the value
    /// lives, and closes the picker.
    Chose(String),
    /// The reader dismissed the picker without choosing. The caller closes it
    /// and leaves the value alone.
    Dismissed,
    /// The key means nothing here and belongs to whatever is behind the picker.
    Ignored,
}

/// Where the reader is in a roster of words, while a value is being chosen.
///
/// Held by whatever is showing the picker, for exactly as long as it is open.
///
/// ★★★★ The wire form goes back through [`Self::over`]'s rule rather than
/// straight into the fields. A snapshot is a door into this type as much as the
/// constructor is — §2 #3's `dry_run` restores signals from one — so a
/// deserialised empty roster would be an invalid state arriving by the one
/// route that skipped the check, and [`Self::is_empty`] would be a lie rather
/// than an invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PickerWire")]
pub struct Picker {
    options: Vec<Cow<'static, str>>,
    at: usize,
    holding: Option<String>,
}

/// The shape a [`Picker`] takes on the wire, before its rule is applied.
#[derive(Deserialize)]
struct PickerWire {
    options: Vec<Cow<'static, str>>,
    at: usize,
    holding: Option<String>,
}

impl TryFrom<PickerWire> for Picker {
    type Error = PickerDefect;

    fn try_from(wire: PickerWire) -> Result<Self, Self::Error> {
        if wire.options.is_empty() {
            return Err(PickerDefect::NoOptions);
        }
        Ok(Self {
            // Clamped rather than refused: a highlight past the end is a
            // snapshot taken over a roster that has since shortened, and the
            // last option is the truthful answer to "where was the reader".
            at: wire.at.min(wire.options.len() - 1),
            options: wire.options,
            holding: wire.holding,
        })
    }
}

impl Picker {
    /// Open over `options`, highlighting `chosen`.
    ///
    /// `chosen` is what the document holds. A word the roster does not offer is
    /// **kept** — the highlight starts at the first option and [`Self::holding`]
    /// names what is really in the document, because a picker that quietly
    /// showed the first word would be reporting a value nothing wrote.
    ///
    /// # Errors
    ///
    /// [`PickerDefect::NoOptions`] when the roster is empty.
    pub fn over<I, S>(options: I, chosen: &str) -> Result<Self, PickerDefect>
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'static, str>>,
    {
        let options: Vec<Cow<'static, str>> = options.into_iter().map(Into::into).collect();
        if options.is_empty() {
            return Err(PickerDefect::NoOptions);
        }
        let found = options.iter().position(|o| o == chosen);
        Ok(Self {
            at: found.unwrap_or(0),
            holding: match found {
                Some(_) => None,
                None => Some(chosen.to_owned()),
            },
            options,
        })
    }

    /// The words on offer, in the order they are shown.
    #[must_use]
    pub fn options(&self) -> &[Cow<'static, str>] {
        &self.options
    }

    /// How many words are on offer. Never zero.
    #[must_use]
    pub fn len(&self) -> usize {
        self.options.len()
    }

    /// Always `false` — kept so a reader of [`Self::len`] is not left to infer
    /// it, and asserted by a test so the invariant is checked rather than
    /// claimed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Which option the highlight is on.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// **The word committing right now would choose.**
    ///
    /// The member the floor has no equivalent of: there the same fact is only
    /// ever announced as it changes, so a test, an agent and a screen reader
    /// all have to have been listening.
    #[must_use]
    pub fn highlighted(&self) -> &str {
        &self.options[self.at]
    }

    /// The word the document holds when the roster does not offer it.
    ///
    /// `None` in the ordinary case. `Some(word)` is a **named difference**
    /// rather than a silent substitution: the value is still that word, and
    /// whatever draws the picker can say so.
    #[must_use]
    pub fn holding(&self) -> Option<&str> {
        self.holding.as_deref()
    }

    /// Put the highlight on `index`.
    ///
    /// Returns `false` and moves nothing when the index is past the end — an
    /// out-of-range index is a caller's mistake, and the floor's answer to it
    /// is to clear the control to no value at all.
    pub fn point_at(&mut self, index: usize) -> bool {
        if index >= self.options.len() {
            return false;
        }
        self.at = index;
        true
    }

    /// Move the highlight one step, wrapping at both ends.
    pub fn step(&mut self, forward: bool) {
        let n = self.options.len();
        self.at = if forward {
            (self.at + 1) % n
        } else {
            (self.at + n - 1) % n
        };
    }

    /// Move the highlight to the next option starting with `ch`, wrapping.
    ///
    /// Searching starts at the option **after** the highlight, so pressing the
    /// same letter walks every word that begins with it and comes back round.
    /// Returns `false` when no option starts with it, leaving the highlight
    /// where it was.
    ///
    /// Case-insensitive, on the reader's side of the question: a roster is
    /// written in whatever case the configuration uses and a keyboard is not.
    pub fn typed(&mut self, ch: char) -> bool {
        let n = self.options.len();
        let wanted = ch.to_lowercase().next().unwrap_or(ch);
        for offset in 1..=n {
            let index = (self.at + offset) % n;
            let first = self.options[index]
                .chars()
                .next()
                .and_then(|c| c.to_lowercase().next());
            if first == Some(wanted) {
                self.at = index;
                return true;
            }
        }
        false
    }

    /// What a key does to an open picker, under the W3C single-select listbox
    /// model: the arrows move, `Home` / `End` jump, `Enter` and `Space` choose,
    /// `Escape` dismisses, and a printable character searches.
    ///
    /// Movement is deliberately **not** a write. The floor's collapsed control
    /// commits on every arrow press, so a keyboard reader walking a roster of
    /// six leaves six values in the document and, on a field whose scope is a
    /// restart, six restarts owed.
    pub fn key(&mut self, key: &str) -> Picked {
        match key {
            "ArrowDown" | "ArrowRight" => {
                self.step(true);
                Picked::Moved
            }
            "ArrowUp" | "ArrowLeft" => {
                self.step(false);
                Picked::Moved
            }
            "Home" => {
                self.at = 0;
                Picked::Moved
            }
            "End" => {
                self.at = self.options.len() - 1;
                Picked::Moved
            }
            "Enter" | " " | "Space" => Picked::Chose(self.highlighted().to_owned()),
            "Escape" => Picked::Dismissed,
            other => {
                let mut chars = other.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) if self.typed(ch) => Picked::Moved,
                    _ => Picked::Ignored,
                }
            }
        }
    }

    /// What a key does to the **closed** control that would open this picker.
    ///
    /// Free of any picker, because it is the question asked before one exists:
    /// a screen holding no picker has to know which keys make it hold one. The
    /// keys are the ones the open picker answers, so a reader never has to
    /// learn that the roster must be opened first.
    #[must_use]
    pub fn opens(key: &str) -> bool {
        matches!(
            key,
            "ArrowDown" | "ArrowUp" | "Enter" | " " | "Space" | "F4"
        ) || key.chars().count() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::{Picked, Picker, PickerDefect};

    fn severities() -> Picker {
        Picker::over(["trace", "debug", "info", "warn", "error", "fatal"], "info")
            .expect("a roster with something in it")
    }

    /// ★★★ The floor accepts a chooser over nothing; this refuses one, at the
    /// place it is built.
    #[test]
    fn r1732_a_roster_with_nothing_in_it_is_refused() {
        let empty: [&'static str; 0] = [];
        assert_eq!(
            Picker::over(empty, "anything"),
            Err(PickerDefect::NoOptions)
        );
        assert_eq!(
            PickerDefect::NoOptions.sentence(),
            "a picker needs at least one option to offer",
        );
        // And the refusal is why `is_empty` can be a constant.
        assert!(!severities().is_empty());
        assert_eq!(severities().len(), 6);
    }

    /// ★★★★★ The member the floor has only as a signal: what committing right
    /// now would choose, asked as a question.
    #[test]
    fn r1732_the_highlight_is_a_value_and_moving_it_writes_nothing() {
        let mut picker = severities();
        assert_eq!(picker.highlighted(), "info");
        assert_eq!(picker.at(), 2);
        assert_eq!(picker.key("ArrowDown"), Picked::Moved);
        assert_eq!(picker.key("ArrowDown"), Picked::Moved);
        assert_eq!(
            picker.highlighted(),
            "error",
            "★ the reader has moved two rows, and this says so"
        );
        assert_eq!(
            picker.key("Enter"),
            Picked::Chose("error".to_owned()),
            "★★ and only now is there a word to write",
        );
    }

    /// Both ends wrap, in both directions, and the jumps land on the ends.
    #[test]
    fn r1732_the_roster_wraps_at_both_ends() {
        let mut picker = severities();
        picker.point_at(0);
        assert_eq!(picker.key("ArrowUp"), Picked::Moved);
        assert_eq!(picker.highlighted(), "fatal", "★ up from the first wraps");
        assert_eq!(picker.key("ArrowDown"), Picked::Moved);
        assert_eq!(picker.highlighted(), "trace", "★ and down from the last");
        assert_eq!(picker.key("End"), Picked::Moved);
        assert_eq!(picker.highlighted(), "fatal");
        assert_eq!(picker.key("Home"), Picked::Moved);
        assert_eq!(picker.highlighted(), "trace");
    }

    /// ★★★★ The floor's typeahead stops on the first match it finds and stays
    /// there however often the letter is pressed. This walks them all.
    #[test]
    fn r1732_typing_a_letter_walks_every_word_that_starts_with_it() {
        let mut picker =
            Picker::over(["drop", "block", "defer", "discard"], "block").expect("a roster");
        assert_eq!(picker.key("d"), Picked::Moved);
        assert_eq!(picker.highlighted(), "defer");
        assert_eq!(picker.key("d"), Picked::Moved);
        assert_eq!(picker.highlighted(), "discard");
        assert_eq!(picker.key("d"), Picked::Moved);
        assert_eq!(picker.highlighted(), "drop", "★ and it wraps round");
        assert_eq!(picker.key("D"), Picked::Moved, "★ case is the roster's");
        assert_eq!(picker.highlighted(), "defer");
        assert_eq!(
            picker.key("z"),
            Picked::Ignored,
            "★ a letter nothing starts with belongs to whatever is behind",
        );
        assert_eq!(picker.highlighted(), "defer", "and it moved nothing");
    }

    /// ★★★★★ A word the roster does not offer is kept and named, where the
    /// floor ignores the call and goes on showing something else.
    #[test]
    fn r1732_a_word_the_roster_does_not_offer_is_named_rather_than_replaced() {
        let picker = Picker::over(["block", "drop"], "retire").expect("a roster");
        assert_eq!(picker.holding(), Some("retire"));
        assert_eq!(
            picker.highlighted(),
            "block",
            "★ the highlight has to start somewhere, and it is not a claim about the value",
        );
        assert_eq!(
            severities().holding(),
            None,
            "the ordinary case says nothing"
        );
    }

    /// An index past the end moves nothing, where the floor accepts it and
    /// clears the control.
    #[test]
    fn r1732_an_index_past_the_end_is_refused() {
        let mut picker = Picker::over(["block", "drop"], "block").expect("a roster");
        assert!(!picker.point_at(7));
        assert_eq!(picker.at(), 0);
        assert_eq!(picker.highlighted(), "block");
        assert!(picker.point_at(1));
        assert_eq!(picker.highlighted(), "drop");
    }

    /// ★★★★ A snapshot goes back through the same rule the constructor does —
    /// the door §2 #3's `dry_run` opens, and the one an invalid state would
    /// otherwise arrive by.
    #[test]
    fn r1732_a_restored_picker_is_judged_like_a_built_one() {
        let mut picker = severities();
        picker.key("End");
        let wire = serde_json::to_string(&picker).expect("a picker is a value");
        let back: Picker = serde_json::from_str(&wire).expect("and it comes back");
        assert_eq!(back, picker);
        assert_eq!(back.highlighted(), "fatal");

        let empty: Result<Picker, _> =
            serde_json::from_str(r#"{"options":[],"at":0,"holding":null}"#);
        assert!(
            empty.is_err(),
            "★ a roster with nothing in it is refused at every door, not only the constructor",
        );
        let past_the_end: Picker =
            serde_json::from_str(r#"{"options":["a","b"],"at":9,"holding":null}"#)
                .expect("a shortened roster still restores");
        assert_eq!(
            past_the_end.highlighted(),
            "b",
            "★ and the highlight lands on the last option rather than off the end",
        );
    }

    /// Dismissing is its own answer, distinct from choosing what was under the
    /// highlight.
    #[test]
    fn r1732_dismissing_is_not_choosing() {
        let mut picker = severities();
        picker.key("ArrowDown");
        assert_eq!(picker.key("Escape"), Picked::Dismissed);
    }

    /// The keys that open a closed control are the keys the open one answers,
    /// so nothing has to be learned twice.
    #[test]
    fn r1732_the_keys_that_open_are_the_keys_that_drive() {
        for key in ["ArrowDown", "ArrowUp", "Enter", " ", "d"] {
            assert!(Picker::opens(key), "{key} opens the roster");
            assert_ne!(
                severities().key(key),
                Picked::Ignored,
                "{key} does something once it is open",
            );
        }
        for key in ["Tab", "PageDown", "Backspace"] {
            assert!(!Picker::opens(key), "{key} is not this control's");
        }
    }
}
