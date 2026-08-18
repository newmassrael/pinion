//! R1650 §5.21 — a **node inspector that is the settings editor**: typed
//! fields addressed by their configuration path, each saying whether changing
//! it takes effect now or needs a restart, and a launch gate that separates the
//! three ways a configuration is wrong.
//!
//! # Why this is a framework type and not an application's form
//!
//! The capability list this axis is judged by puts a node inspector in its
//! must-have tier and states the requirement in one line: **every field carries
//! an applies-live versus needs-restart badge, and without it "I changed it and
//! nothing happened" reads as a tool bug.** Measured at R1646: no property grid
//! exists in any crate here — it lives in two examples — so the badge had
//! nowhere to be. That is the gap this closes.
//!
//! The badge is not decoration. A configuration of this kind has **very few**
//! live-editable keys; everything else changes a value in memory that the
//! running node will never read. A form that does not say which is which
//! converts a correct tool into an apparently broken one, and it converts a
//! correct *user* into one who restarts everything out of superstition.
//!
//! # Three defects, not one, because the failures are not alike
//!
//! Field experience with a lab built this way records that a wrong
//! configuration fails in **three different ways**, and that a gate collapsing
//! them into "invalid" is the wrong gate:
//!
//! | defect | what the target does | so the gate |
//! |---|---|---|
//! | [`UnknownKey`](ConfigDefect::UnknownKey) | warns, **starts anyway**, and silently ignores the key | warns — and must not block |
//! | [`WrongType`](ConfigDefect::WrongType) | panics during start-up | blocks |
//! | [`OutOfRange`](ConfigDefect::OutOfRange) | panics during start-up | blocks |
//!
//! The first row is the one that makes this worth a type. A key the target does
//! not know is **not** a reason to refuse to launch — refusing would make the
//! tool stricter than the thing it configures, and a user with a newer target
//! than the tool could not start anything. But it is also not nothing: the node
//! that comes up is *not the one that was drawn*, and only the tool is in a
//! position to say so. So it is a warning that is **reported and counted**
//! rather than either fatal or silent.
//!
//! # What is derived rather than stored
//!
//! * [`ConfigForm::verdict`] — a launch is blocked exactly when some defect
//!   blocks. Not a flag anybody sets, so a form cannot be "valid" and hold an
//!   error at the same time.
//! * [`ConfigForm::defects`] — R1651. The defect list is **derived from the
//!   fields**, not handed in: a field's [`FieldType`] says what its text has to
//!   mean, so text that does not parse *is* a [`ConfigDefect::WrongType`] and
//!   text outside the declared bounds *is* a [`ConfigDefect::OutOfRange`].
//!   Before R1651 a caller passed the list, which let a screen show a clean
//!   gate over a form holding an unparseable value.
//! * [`ConfigForm::pending_restart`] — which **edited** fields need one. A form
//!   that reported every restart-scoped field would tell a user to restart for
//!   values they never touched.
//! * [`ConfigForm::document`] — R1651. The **deployable configuration**, nested
//!   from the dotted paths the rows are addressed by. The form is the editor and
//!   the document is its output, so there is no second mapping to keep in step;
//!   [`ConfigForm::adopt`] reads one back and *names* every path it could not
//!   place rather than dropping it.
//!
//! # The floor this is judged against, measured rather than read
//!
//! The interface this axis is judged by is the mature toolkit's form layout at
//! 6.11 together with its settings store. R1651 built that toolkit from source
//! and **ran** a form of three rows through it, because R1557 recorded three
//! doc statements out of three being false; every number below is what the
//! laid-out geometry reported, not what a manual says.
//!
//! What it has, and this must therefore have:
//!
//! * **A row-wrap policy with three settings** — label beside the control,
//!   label above it, or beside-unless-it-does-not-fit. The third is derived per
//!   row: in a 320 px box all three rows stayed beside their controls, and in a
//!   140 px box only the row whose label measured 153 px moved to its own line.
//! * **A field-growth policy with three settings** — measured beside the label,
//!   a control was 108 px at its size hint and 161 px when allowed to grow; when
//!   wrapped, 108 px against 320 px.
//! * **Hiding a row without removing it** — hiding the middle of three rows took
//!   the form from 159 px to 104 px while the row count stayed 3.
//!
//! And what it does not have, which is why this is a type and not a layout:
//!
//! * **No applies-scope.** Across the five classes a settings form is built
//!   from, 105 distinct declared properties, and not one is about whether an
//!   edit reaches the running program. A control does carry an edited flag, so
//!   the missing half is not dirtiness — it is [`Applies`], and without it
//!   nothing can roll the two up into "you must restart for *these*".
//! * **No defect that warns without blocking.** Its per-field verdict has three
//!   values, and the middle one means *keep typing*: measured, an integer
//!   control bounded 0..=65535 answers that same middle value for the empty
//!   string and for `70000`. "Not finished" and "out of range" are one answer,
//!   and neither is "wrong, and the program will start anyway" — which is the
//!   arm [`ConfigDefect::UnknownKey`] exists for.
//! * **No form-to-document derivation.** The mapping between a form and the
//!   settings it edits is written by hand, twice — once to load and once to
//!   save — with nothing checking the two agree. [`ConfigForm::document`] and
//!   [`ConfigForm::adopt`] are one mapping with a round-trip law over it.
//!
//! One measured finding is worth recording because it cuts the other way: the
//! label→control accessible relation there is **not** automatic. Passing a
//! label the application owns leaves the relation unset, and only the overload
//! where the layout builds the label itself wires it up — so a form assembled
//! the way an application actually assembles one has controls with no
//! accessible name. Here the row's access node carries the key, always.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::text_format::TextFormat;

/// Whether changing a field reaches a running node.
///
/// A property of the **key**, not of an instance: two forms over the same
/// configuration must not disagree about whether a path is live-editable, so
/// this travels with the field definition and there is no setter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    pinion_derive::VariantCensus,
)]
#[serde(rename_all = "snake_case")]
#[variant_census(all)]
pub enum Applies {
    /// The running node reads the new value. The rare case, and the reason the
    /// other one has to be visible.
    Hot,
    /// The value changes in memory and the running node keeps the old one until
    /// it is restarted.
    Restart,
}

impl Applies {
    /// Both, so a consumer enumerates rather than spelling two out.
    pub const ALL: [Self; 2] = [Self::Hot, Self::Restart];

    /// The badge this shows, and its wire spelling — one word, because the
    /// badge sits in a form row and a sentence would not fit.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Hot => "hot",
            Self::Restart => "restart",
        }
    }

    /// The applies-scope that word names.
    #[must_use]
    pub fn from_wire(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.wire() == word)
    }

    /// Whether editing this field leaves the running node out of date.
    #[must_use]
    pub const fn needs_restart(self) -> bool {
        matches!(self, Self::Restart)
    }
}

/// Where a row's value came from.
///
/// ★★★★★ R1716 — **a row that nobody typed is not the same row with a flag
/// on it.** A settings form of this kind shows values it worked out for
/// itself — the mode a node's role implies, the addresses the drawn graph
/// dials — beside the values a person wrote, and a reader who cannot tell them
/// apart is left with two wrong beliefs at once: that the tool is waiting for
/// them to fill something in, and that what they see is what they said.
///
/// So the fact is carried as **where it came from**, not as a boolean. The
/// name goes on the badge verbatim, because "this is derived" answers nothing a
/// person can act on and "this comes from the role" tells them where to go and
/// change it.
///
/// # What the floor does here, measured rather than read
///
/// The mature toolkit at 6.11 was built and **run** for this round. It can
/// derive a value — a bound property recomputed 20 → 42 when its source moved
/// 10 → 21 — so the capability is parity and not a gap. Four things it does
/// not do, and each is why an arm of this type carries what it carries:
///
/// * **The answer is a bool.** Asking whether a value is derived returns
///   yes-or-no; nothing names the source, and the binding object itself only
///   says whether it is null and what type it holds.
/// * **Authoring over a derived value drops the derivation silently** —
///   measured: one ordinary value-changed notification, no separate news, and
///   afterwards the dropped derivation is unreachable. [`Takeover`] is that
///   act with an announcement attached.
/// * **A read-only cell is a view's convention, not the value's guarantee** —
///   measured: writing to a cell with editing cleared *succeeded* and changed
///   it. Here the refusal is the value's own, so a screen cannot forget it.
/// * **Nothing can say why.** A locked cell answers 3 of 256 standard roles and
///   none of them is a reason; driving its editor logs a line and returns void.
/// # ★★★★★ R1717 — one key can have TWO contributors
///
/// [`Self::Shared`] is the arm the behaviour canon has and R1716 did not: a
/// node may be told to dial an address this canvas does not draw *and* the
/// addresses it does draw, and those are not competing answers — they are two
/// contributions to one list. R1716 let the written half win the whole row,
/// which made the picture and the configuration disagree with only a gate
/// warning to say so.
///
/// # What the floor does here, measured rather than read
///
/// The mature toolkit at 6.11.1 was built and **run** for R1717, and it does
/// compose two contributors — a settings store falls back to a second store, so
/// a key only the second one holds still answers, and taking the written half
/// away brings the other back. So the *capability* is parity. Four things it
/// does not do:
///
/// * **The composition is whole-key, never element-wise.** Measured: a written
///   one-element list beside a worked-out three-element list answers **one**
///   element. One store wins the key entire; there is no union.
/// * **No reader can ask which store answered.** The file name is the written
///   store's whatever answered, and the scope is the asking object's. Telling
///   them apart needs a *second* object with fallbacks switched off, and it
///   answers about the **key**, not about the value.
/// * **The binding half ends on a contribution.** Writing into a derived value
///   clears its derivation, and after the source moves the value does not
///   follow it any more.
/// * **A cell holding a composed value answers 2 of 256 standard roles**, and
///   none of them is how much of it is not the reader's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Somebody wrote all of it.
    Authored,
    /// Nobody wrote any of it; this screen worked it out, from the thing this
    /// names.
    ///
    /// The word is shown to a reader — `role`, `wire`, `kind default` — so it
    /// is written for them and not for a log.
    Derived(Cow<'static, str>),
    /// R1717 — **both.** Somebody wrote part of it and the rest was worked out
    /// from the thing this names, and the row shows their composition.
    ///
    /// Only a shape that [`FieldType::merges`] can be in this state: two
    /// contributions to a single value are not a composition, they are a
    /// contradiction, and [`ConfigField::with_derived`] refuses one.
    Shared(Cow<'static, str>),
}

impl Default for Source {
    fn default() -> Self {
        Self::Authored
    }
}

impl Source {
    /// Every answer, so a consumer enumerates rather than spelling them out.
    ///
    /// The names alone — a census over the vocabulary needs the words and a
    /// value of each arm would need a source name that means nothing.
    pub const WORDS: [&'static str; 3] = ["authored", "derived", "shared"];

    /// Whether a person may write this row.
    ///
    /// ★★★ R1717 — the question every consumer was really asking when it
    /// asked whether the value was authored, and the two stopped being the same
    /// question the moment a row could be **partly** authored. Spelled as its
    /// own predicate so that a screen painting a control, a form refusing a
    /// write and an accessibility tree marking a node read-only all read one
    /// fact.
    #[must_use]
    pub const fn writable(&self) -> bool {
        !matches!(self, Self::Derived(_))
    }

    /// What part of the value was worked out, and from what — `None` when
    /// nothing was.
    #[must_use]
    pub fn derived_from(&self) -> Option<&str> {
        match self {
            Self::Authored => None,
            Self::Derived(from) | Self::Shared(from) => Some(from),
        }
    }

    /// The word this answer is spelled with on the wire and in a census.
    #[must_use]
    pub const fn wire(&self) -> &'static str {
        match self {
            Self::Authored => Self::WORDS[0],
            Self::Derived(_) => Self::WORDS[1],
            Self::Shared(_) => Self::WORDS[2],
        }
    }
}

/// What a screen worked out for one row, held **beside** what somebody wrote
/// rather than instead of it.
///
/// ★★★ R1717 — carrying the value and not only the source name is what makes
/// the composition re-runnable: a row whose written half changes has to be able
/// to say what it shows *now*, and a type that had thrown the worked-out half
/// away could only do that by asking the screen again at every keystroke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derivation {
    /// What the value was worked out from — the word a reader sees.
    from: Cow<'static, str>,
    /// What it was worked out to be.
    value: String,
}

impl Derivation {
    /// What this was worked out from.
    #[must_use]
    pub fn from(&self) -> &str {
        &self.from
    }

    /// What it was worked out to be, before any written half joins it.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Where a row's value goes when the form ships a document.
///
/// ★★★ R1716 — the second half of the same question, and a separate axis
/// because the two answers cross: a row can be derived and still belong in the
/// document (the mode a role implies is configuration), and a row can be
/// authored and belong nowhere near it (which machine to start this on is not a
/// setting of the thing being started).
///
/// Without this the only honest thing a form could do with such a row is
/// refuse to hold it, and then the screen keeps a second list beside the form —
/// which is the drift this widget exists to end. [`Composed::aside`] is the
/// rollup, so "did not fit" and "does not belong" stay different pieces of news.
///
/// The floor has the *bit* — a property can be marked not-worth-storing, and 6
/// of one control's 80 properties are — and it has no word for what such a row
/// is **instead**, which is the half a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Goes {
    /// Into the document, at this row's path.
    Document,
    /// Not into the document. The word says what the row is about instead —
    /// `placement`, `run argument` — because "not in the file" is not something
    /// a person can act on and "this is where it runs" is.
    Aside(Cow<'static, str>),
}

impl Default for Goes {
    fn default() -> Self {
        Self::Document
    }
}

impl Goes {
    /// Whether this row's value belongs in the document.
    #[must_use]
    pub const fn into_document(&self) -> bool {
        matches!(self, Self::Document)
    }

    /// What the row is about instead, when it is not configuration.
    #[must_use]
    pub fn instead(&self) -> Option<&str> {
        match self {
            Self::Document => None,
            Self::Aside(word) => Some(word),
        }
    }
}

/// What a field's text is supposed to mean.
///
/// R1651. Separate from [`ConfigField::ty`], which is the **word the
/// configuration calls this kind of value** and is what the badge shows —
/// `locator[]`, `perm`, `int`. This is the *structure*, and it is what makes
/// the defect list derivable: a field declared [`Self::Integer`] whose text does
/// not parse is a [`ConfigDefect::WrongType`] with nobody having to notice, and
/// one that parses outside its bounds is a [`ConfigDefect::OutOfRange`].
///
/// The arms are the document's type set, not a widget catalogue: a form is an
/// editor for a configuration document, and a shape the document cannot hold is
/// a shape this must not offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, pinion_derive::VariantCensus)]
#[serde(rename_all = "snake_case", tag = "shape")]
pub enum FieldType {
    /// Any text. The document holds a string.
    Text,
    /// **A string that has to parse.** The document holds a string, and a
    /// string of the wrong shape is a defect at this boundary rather than a
    /// start-up failure downstream.
    ///
    /// ★★★ R1690 — distinct from [`Self::Text`] because the difference is what
    /// happens next: free text is accepted by whatever reads it, and this is
    /// not. Every field of this kind was typed as free text before the arm
    /// existed, which made the tool quietly more permissive than the thing it
    /// configures — the value goes in, the form says nothing, and the target
    /// refuses to start.
    ///
    /// The shape is declared as data so it can be judged before the value is
    /// finished, said in a sentence, and read by an agent that wants to know
    /// what a field will take. See [`TextFormat`].
    Formatted {
        /// The shape the text has to have.
        of: TextFormat,
    },
    /// A whole number the document holds as a number, bounded inclusively.
    ///
    /// Bounded rather than free because an unbounded integer field cannot
    /// produce an [`ConfigDefect::OutOfRange`], and a form that can only ever
    /// report a parse failure is back to one defect kind.
    Integer {
        /// Smallest accepted value.
        min: i64,
        /// Largest accepted value.
        max: i64,
    },
    /// `true` or `false`, held as a document boolean.
    Boolean,
    /// Exactly one of a fixed set of words.
    Choice {
        /// The accepted words, in the order a picker offers them.
        of: Vec<Cow<'static, str>>,
    },
    /// Any subset of a fixed set of words, held as a document array.
    ///
    /// Distinct from a [`Self::List`] of [`Self::Choice`] because the elements
    /// are a *set*: order does not matter, a repeat is a defect, and the screen
    /// shows every option with the chosen ones marked rather than a text box.
    Flags {
        /// The options, in the order they are shown.
        of: Vec<Cow<'static, str>>,
    },
    /// Zero or more of the inner shape, held as a document array.
    List {
        /// What each element is.
        of: Box<FieldType>,
    },
}

impl FieldType {
    /// The separator a [`Self::List`] or [`Self::Flags`] field's text uses, and
    /// the spelling [`ConfigForm::adopt`] writes back.
    ///
    /// One constant rather than a parameter: two forms over one configuration
    /// disagreeing about how a list is spelled would make the same document
    /// read back differently depending on which screen opened it.
    pub const SEPARATOR: &'static str = ", ";

    /// Encode `text` as this shape.
    ///
    /// # Errors
    ///
    /// The defect the text has, named at `key`.
    pub fn encode(&self, key: &str, text: &str) -> Result<Value, ConfigDefect> {
        let wrong = |want: &str| ConfigDefect::WrongType {
            key: key.to_string(),
            want: want.to_string(),
            got: format!("{text:?}"),
        };
        match self {
            Self::Text => Ok(Value::String(text.to_string())),
            Self::Formatted { of } => {
                // Both non-acceptable judgements are one defect here. The
                // difference between them is about a caret — whether a value
                // may stand while it is being typed — and this boundary is
                // asked about a value that is being committed.
                let judged = of.judge(text);
                if judged.acceptable() {
                    Ok(Value::String(text.to_string()))
                } else {
                    Err(wrong(judged.wanted()))
                }
            }
            Self::Integer { min, max } => {
                let n: i64 = text.trim().parse().map_err(|_| wrong("a whole number"))?;
                if n < *min || n > *max {
                    return Err(ConfigDefect::OutOfRange {
                        key: key.to_string(),
                        allowed: format!("{min}..={max}"),
                    });
                }
                Ok(Value::Number(n.into()))
            }
            Self::Boolean => match text.trim() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(wrong("true or false")),
            },
            Self::Choice { of } => {
                let word = text.trim();
                if of.iter().any(|o| o == word) {
                    Ok(Value::String(word.to_string()))
                } else {
                    Err(ConfigDefect::OutOfRange {
                        key: key.to_string(),
                        allowed: Self::joined(of),
                    })
                }
            }
            Self::Flags { of } => {
                let mut chosen: Vec<Value> = Vec::new();
                let mut seen: Vec<&str> = Vec::new();
                for word in Self::split(text) {
                    if !of.iter().any(|o| o == word) {
                        return Err(ConfigDefect::OutOfRange {
                            key: key.to_string(),
                            allowed: Self::joined(of),
                        });
                    }
                    if seen.contains(&word) {
                        return Err(wrong("a set, with no repeats"));
                    }
                    seen.push(word);
                    chosen.push(Value::String(word.to_string()));
                }
                Ok(Value::Array(chosen))
            }
            Self::List { of } => Self::split(text)
                .map(|element| of.encode(key, element))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
        }
    }

    /// Read a document value back as this shape's text, in the canonical
    /// spelling — the inverse of [`Self::encode`] up to that spelling.
    ///
    /// # Errors
    ///
    /// A value the shape cannot hold, named at `key`.
    pub fn decode(&self, key: &str, value: &Value) -> Result<String, ConfigDefect> {
        let wrong = |want: &str| ConfigDefect::WrongType {
            key: key.to_string(),
            want: want.to_string(),
            got: value.to_string(),
        };
        match (self, value) {
            (Self::Text | Self::Formatted { .. } | Self::Choice { .. }, Value::String(s)) => {
                Ok(s.clone())
            }
            (Self::Integer { .. }, Value::Number(n)) => Ok(n.to_string()),
            (Self::Boolean, Value::Bool(b)) => Ok(b.to_string()),
            (Self::Flags { .. } | Self::List { .. }, Value::Array(items)) => {
                let inner = match self {
                    Self::List { of } => of.as_ref().clone(),
                    _ => Self::Text,
                };
                let parts = items
                    .iter()
                    .map(|item| inner.decode(key, item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(parts.join(Self::SEPARATOR))
            }
            (Self::Text, _) => Err(wrong("text")),
            (Self::Formatted { of }, _) => Err(wrong(&of.wanted())),
            (Self::Integer { .. }, _) => Err(wrong("a whole number")),
            (Self::Boolean, _) => Err(wrong("true or false")),
            (Self::Choice { .. }, _) => Err(wrong("one word")),
            (Self::Flags { .. } | Self::List { .. }, _) => Err(wrong("a list")),
        }
    }

    /// The words a [`Self::Choice`] or [`Self::Flags`] field offers, empty for
    /// the shapes that offer none.
    ///
    /// One accessor rather than two so a screen that paints options does not
    /// have to know which of the two arms it is looking at — what differs
    /// between them is how many may be chosen, which is [`Self::one_only`].
    #[must_use]
    pub fn options(&self) -> &[Cow<'static, str>] {
        match self {
            Self::Choice { of } | Self::Flags { of } => of,
            _ => &[],
        }
    }

    /// Whether choosing one option unchooses the others.
    #[must_use]
    pub const fn one_only(&self) -> bool {
        matches!(self, Self::Choice { .. })
    }

    /// ★★★★★ R1717 — whether a value of this shape can hold **two
    /// contributions at once and SHOW which is which**.
    ///
    /// A list can: the addresses somebody wrote and the addresses a canvas
    /// draws are both true of one node, a list that showed only one of them
    /// would ship a configuration that contradicts the picture, and a list is
    /// drawn one element per line — so [`ConfigField::element_source`] has a
    /// line to answer for. A single value cannot hold two at all: a mode worked
    /// out from a role and a mode somebody typed are competing answers, and a
    /// form that "composed" them would invent a third.
    ///
    /// ★★ [`Self::Flags`] is a set, so it could hold two — and it is **not**
    /// here, which was the closing audit of the round that wrote this line. Its
    /// members are drawn as chips with no per-member place to say "this one is
    /// the screen's", so a shared one would paint a chosen chip a person could
    /// press and silently adopt: the exact freeze this axis exists to end, one
    /// shape over. The rule this answers is therefore about what can be SHOWN,
    /// not only about what a value can hold — and the day a chip can carry its
    /// own provenance this becomes `List | Flags` and the painter follows.
    ///
    /// So the rule is the **shape's**, not a flag a screen sets per row — which
    /// is what makes [`ConfigField::with_derived`] able to refuse rather than
    /// silently pick a winner.
    #[must_use]
    pub const fn merges(&self) -> bool {
        matches!(self, Self::List { .. })
    }

    /// A written half and a worked-out one composed, in the order a reader
    /// meets them: what somebody wrote first, then what the screen worked out
    /// and they had not already said.
    ///
    /// Written-first because the written half is the one a person is looking
    /// for, and de-duplicated because one address said twice is one address —
    /// the behaviour canon composes in exactly this order for exactly this
    /// reason.
    ///
    /// A shape that does not [`merges`](Self::merges) answers the written half
    /// alone; nothing can reach that, because a row of such a shape is never
    /// allowed to hold both.
    #[must_use]
    pub fn compose(&self, written: &str, worked_out: &str) -> String {
        if !self.merges() {
            return written.to_string();
        }
        let mut out: Vec<&str> = Self::split(written).collect();
        for element in Self::split(worked_out) {
            if !out.contains(&element) {
                out.push(element);
            }
        }
        out.join(Self::SEPARATOR)
    }

    /// The elements a [`Self::List`] or [`Self::Flags`] text holds, in order.
    ///
    /// Public because a painter drawing a row per element must split it the
    /// same way the encoder does — two spellings of "what the commas mean" is
    /// how a screen shows four rows for a value the document reads as three.
    pub fn elements(text: &str) -> impl Iterator<Item = &str> {
        Self::split(text)
    }

    fn split(text: &str) -> impl Iterator<Item = &str> {
        text.split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
    }

    fn joined(of: &[Cow<'static, str>]) -> String {
        of.iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(Self::SEPARATOR)
    }
}

/// One way a configuration is wrong.
///
/// Three arms because the target fails three different ways — see the module
/// docs. Each carries the field it is about, so a defect can be shown *on the
/// row* rather than only in a list at the bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, pinion_derive::VariantCensus)]
#[serde(rename_all = "snake_case", tag = "defect")]
pub enum ConfigDefect {
    /// A key the target does not know. It warns and starts, ignoring the key —
    /// so the node that comes up is not the one that was drawn.
    UnknownKey {
        /// The path that will be ignored.
        key: String,
    },
    /// A known key holding a value of the wrong type. The target panics during
    /// start-up.
    WrongType {
        /// The path.
        key: String,
        /// The type the key is declared with.
        want: String,
        /// What the value looks like instead.
        got: String,
    },
    /// A known key holding a value of the right type and outside what the
    /// target accepts. It panics during start-up.
    OutOfRange {
        /// The path.
        key: String,
        /// What is acceptable, in the words the field declares it with.
        allowed: String,
    },
}

impl ConfigDefect {
    /// One representative of every arm, for a consumer covering the vocabulary.
    ///
    /// A function rather than a `const`: every arm carries owned strings, and a
    /// `const` array of them cannot be dropped at compile time. Its length is
    /// checked against [`Self::ARMS`] in this module's tests instead of by the
    /// build, which is the weaker of the two and is said here rather than left
    /// looking like the stronger one.
    #[must_use]
    pub fn all() -> [Self; 3] {
        [
            Self::UnknownKey { key: String::new() },
            Self::WrongType {
                key: String::new(),
                want: String::new(),
                got: String::new(),
            },
            Self::OutOfRange {
                key: String::new(),
                allowed: String::new(),
            },
        ]
    }

    /// The field this defect is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::UnknownKey { key }
            | Self::WrongType { key, .. }
            | Self::OutOfRange { key, .. } => key,
        }
    }

    /// This defect's wire spelling.
    #[must_use]
    pub const fn wire(&self) -> &'static str {
        match self {
            Self::UnknownKey { .. } => "unknown_key",
            Self::WrongType { .. } => "wrong_type",
            Self::OutOfRange { .. } => "out_of_range",
        }
    }

    /// **Whether this defect stops a launch.**
    ///
    /// Derived from which arm it is, and the whole reason the vocabulary has
    /// three: an unknown key must NOT block, because refusing would make the
    /// tool stricter than the thing it configures and a user whose target is
    /// newer than the tool could not start anything.
    #[must_use]
    pub const fn blocks(&self) -> bool {
        match self {
            Self::UnknownKey { .. } => false,
            Self::WrongType { .. } | Self::OutOfRange { .. } => true,
        }
    }

    /// What the person reads on the row.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::UnknownKey { key } => {
                format!("{key} is not a key the target knows; it starts and ignores it")
            }
            Self::WrongType { key, want, got } => {
                format!("{key} is declared {want} and holds {got}")
            }
            Self::OutOfRange { key, allowed } => format!("{key} is outside {allowed}"),
        }
    }
}

/// One field of a node's configuration.
///
/// The **key is the configuration path**, verbatim — not a label with a lookup
/// table beside it. That is the property the capability list asks for by name,
/// and it is what lets a form row, an exported configuration file and a defect
/// report all be about the same thing without a translation nobody maintains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigField {
    key: Cow<'static, str>,
    ty: Cow<'static, str>,
    applies: Applies,
    /// ★★★★★ R1717 — **the half somebody wrote**, and `None` when nobody
    /// has. Not "the value": the value is what the row SHOWS, which is this
    /// composed with [`Self::worked_out`] and is therefore derived.
    ///
    /// The split is what makes `edited` answerable. R1716 held one string, so a
    /// row whose shown value included a contribution the reader never made
    /// would have had two answers to "did somebody change this" — and that is
    /// the reason R1716 chose not to compose at all.
    written: Option<String>,
    /// The written half when the form was opened, so an edit is knowable.
    original: Option<String>,
    /// R1717 — what this screen worked out for this key. See [`Derivation`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worked_out: Option<Derivation>,
    /// R1651 — the shape the text has to have. Defaults to [`FieldType::Text`],
    /// which is what a field with no declared structure already was.
    #[serde(default)]
    shape: FieldType,
    /// R1651 — a path the target does not declare, typed in by hand.
    ///
    /// The tool cannot know every key of a target newer than itself, so a
    /// custom path is not an error; it is the one thing that *derives* a
    /// [`ConfigDefect::UnknownKey`], and that defect warns without blocking.
    #[serde(default)]
    custom: bool,
    /// R1651 — a row that is not shown and has not been removed.
    ///
    /// The distinction the mature toolkit's form layout draws with its
    /// row-visibility control: the row keeps its place and its value, and takes
    /// no space. A form that had to remove a row to hide it would lose where it
    /// goes back.
    #[serde(default)]
    hidden: bool,
    /// R1716 — where this value goes. See [`Goes`].
    ///
    /// Skipped when it is the ordinary answer, so a stored form written before
    /// this existed reads back unchanged and one written now is byte-identical
    /// to what that reader expects.
    #[serde(default, skip_serializing_if = "Goes::into_document")]
    goes: Goes,
}

impl Default for FieldType {
    fn default() -> Self {
        Self::Text
    }
}

impl ConfigField {
    /// A field at `key`, declared `ty`, with that applies-scope and value.
    #[must_use]
    pub fn new(
        key: impl Into<Cow<'static, str>>,
        ty: impl Into<Cow<'static, str>>,
        applies: Applies,
        value: impl Into<String>,
    ) -> Self {
        let value = value.into();
        Self {
            key: key.into(),
            ty: ty.into(),
            applies,
            original: Some(value.clone()),
            written: Some(value),
            worked_out: None,
            shape: FieldType::Text,
            custom: false,
            hidden: false,
            goes: Goes::Document,
        }
    }

    /// This row's value is not somebody's writing — it is worked out from the
    /// thing `from` names, and that word is what the badge shows.
    ///
    /// ★★ R1716 — the consequences are the type's, not a screen's: the row
    /// refuses [`set`](Self::set), the form refuses to
    /// [`remove`](ConfigForm::remove) it, and the painter draws no control for
    /// it. A screen that had to remember all three would eventually paint a
    /// text box over a value nobody can write.
    #[must_use]
    pub fn derived_from(mut self, from: impl Into<Cow<'static, str>>) -> Self {
        let value = self.written.take().unwrap_or_default();
        self.original = None;
        self.worked_out = Some(Derivation {
            from: from.into(),
            value,
        });
        self
    }

    /// ★★★★★ R1717 — this screen ALSO worked something out for this key,
    /// **beside** what somebody wrote.
    ///
    /// The row then shows the two composed ([`FieldType::compose`]), keeps
    /// being writable, and writes only the half that is somebody's — so a wire
    /// the canvas draws reaches the configuration without taking a hand-written
    /// address away, which is the behaviour canon's rule and the one R1716
    /// could not express.
    ///
    /// A written half that contributes **nothing** — an empty list — leaves the
    /// row wholly [`Source::Derived`]. That is not a special case bolted on: a
    /// person who empties their half has given the row back, and a row that
    /// claimed to be partly theirs while holding none of it would offer a
    /// take-over of something they already own.
    ///
    /// # Errors
    ///
    /// [`FormError::Unmergeable`] — this row's shape holds a single value, so a
    /// written value and a worked-out one cannot both stand. The caller has to
    /// decide which, and the two ways to say so are this builder's absence and
    /// [`Self::derived_from`].
    pub fn with_derived(
        mut self,
        from: impl Into<Cow<'static, str>>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        if self.written.is_some() && !self.shape.merges() {
            return Err(FormError::Unmergeable {
                key: self.key.to_string(),
                ty: self.ty.to_string(),
            });
        }
        self.worked_out = Some(Derivation {
            from: from.into(),
            value: value.into(),
        });
        self.settle_ownership();
        Ok(self)
    }

    /// Give the row back to its derivation when the written half contributes
    /// nothing to it.
    ///
    /// One place, called by every act that can empty a written half, so
    /// "written but contributing nothing" is a state this type cannot be in and
    /// no consumer has to test for.
    fn settle_ownership(&mut self) {
        if self.worked_out.is_none() {
            return;
        }
        let contributes = match &self.written {
            Some(written) => !self.shape.compose(written, "").is_empty(),
            None => false,
        };
        if !contributes {
            self.written = None;
            self.original = None;
        }
    }

    /// This row does not go into the document; it is about `instead` — see
    /// [`Goes`].
    #[must_use]
    pub fn goes_aside(mut self, instead: impl Into<Cow<'static, str>>) -> Self {
        self.goes = Goes::Aside(instead.into());
        self
    }

    /// Where this value came from.
    ///
    /// ★★★ R1717 — **derived from the two halves**, not stored beside them. A
    /// row that held its own answer to this could be told it was authored while
    /// holding a derivation, and every consumer of that answer would be wrong
    /// at once.
    #[must_use]
    pub fn source(&self) -> Source {
        match (&self.worked_out, &self.written) {
            (None, _) => Source::Authored,
            (Some(worked_out), None) => Source::Derived(worked_out.from.clone()),
            (Some(worked_out), Some(_)) => Source::Shared(worked_out.from.clone()),
        }
    }

    /// The half somebody wrote — `None` when nobody has.
    ///
    /// ★★ R1717 — this is what a screen **stores**. The value below is what it
    /// shows, and storing that instead is how a wire's contribution freezes
    /// into somebody's configuration without them saying so.
    #[must_use]
    pub fn written(&self) -> Option<&str> {
        self.written.as_deref()
    }

    /// What this screen worked out for this row — `None` when nothing did.
    #[must_use]
    pub const fn worked_out(&self) -> Option<&Derivation> {
        self.worked_out.as_ref()
    }

    /// **The row somebody wrote**, with the derivation dropped — `None` when
    /// nobody wrote any of it.
    ///
    /// ★★★ R1717 — what a caller **stores**. A shared row put away whole
    /// carries its derivation with it, and the next read composes the same
    /// worked-out value onto a written half that already holds it; two reads
    /// later the two are indistinguishable and the person owns addresses they
    /// never typed. Returning `None` rather than an emptied row is what lets
    /// the caller skip a wholly derived row without testing for it twice.
    #[must_use]
    pub fn written_row(&self) -> Option<Self> {
        self.written.as_ref()?;
        let mut mine = self.clone();
        mine.worked_out = None;
        Some(mine)
    }

    /// Where this value goes.
    #[must_use]
    pub const fn goes(&self) -> &Goes {
        &self.goes
    }

    /// Declare what the text has to mean, so this row's defects are derivable.
    #[must_use]
    pub fn with_shape(mut self, shape: FieldType) -> Self {
        self.shape = shape;
        self
    }

    /// Mark this as a path the target does not declare — see [`Self::custom`].
    #[must_use]
    pub const fn as_custom(mut self) -> Self {
        self.custom = true;
        self
    }

    /// Keep the row and its value out of the screen — see [`Self::hidden`].
    #[must_use]
    pub const fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// The shape this row's text has to have.
    #[must_use]
    pub const fn shape(&self) -> &FieldType {
        &self.shape
    }

    /// Whether this is a path the target does not declare.
    #[must_use]
    pub const fn custom(&self) -> bool {
        self.custom
    }

    /// Whether this row is held but not shown.
    #[must_use]
    pub const fn hidden(&self) -> bool {
        self.hidden
    }

    /// This row's value as the document holds it.
    ///
    /// # Errors
    ///
    /// The defect the text has.
    pub fn encoded(&self) -> Result<Value, ConfigDefect> {
        self.shape.encode(&self.key, &self.value())
    }

    /// Every way this row is wrong, in the order a reader meets them.
    ///
    /// A custom key is reported **as well as** any shape defect, not instead of
    /// it: a hand-typed path holding an unparseable value is two separate
    /// pieces of news, and one of them blocks.
    #[must_use]
    pub fn defects(&self) -> Vec<ConfigDefect> {
        let mut found = Vec::new();
        // ★ R1716 — a row that goes aside is not a configuration path, so "the
        // target does not know this key" is not news about it; it is a false
        // warning the person cannot act on, about a key the target was never
        // going to be shown. The shape defect below still stands: a placement
        // that does not parse is as wrong as a setting that does not.
        if self.custom && self.goes.into_document() {
            found.push(ConfigDefect::UnknownKey {
                key: self.key.to_string(),
            });
        }
        if let Err(defect) = self.encoded() {
            found.push(defect);
        }
        found
    }

    /// The configuration path this row addresses.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The declared type, in the words the configuration uses for it.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.ty
    }

    /// Whether changing this reaches a running node.
    #[must_use]
    pub const fn applies(&self) -> Applies {
        self.applies
    }

    /// **What the row shows** — the two halves composed.
    ///
    /// ★★★★★ R1717 — computed rather than held. The shown value is a
    /// function of what somebody wrote and what the screen worked out, and a
    /// copy of it kept beside them would be a third place the same fact lives:
    /// every write would have to remember to refresh it, and the one that
    /// forgot would show a value the document does not contain.
    ///
    /// It borrows in both the ordinary cases and only allocates for a row that
    /// genuinely has two contributors, which is the rare one.
    #[must_use]
    pub fn value(&self) -> Cow<'_, str> {
        match (&self.written, &self.worked_out) {
            (Some(written), None) => Cow::Borrowed(written.as_str()),
            (None, Some(worked_out)) => Cow::Borrowed(worked_out.value.as_str()),
            (Some(written), Some(worked_out)) => {
                Cow::Owned(self.shape.compose(written, &worked_out.value))
            }
            // Not reachable through this type's constructors: `new` writes a
            // half and `derived_from` works one out. A form read back from a
            // document that names neither shows nothing, which is the only
            // honest thing left to show.
            (None, None) => Cow::Borrowed(""),
        }
    }

    /// Where the element at `at` of the shown value came from.
    ///
    /// ★★★★★ R1717 — **provenance reaches the element, because editing
    /// does.** A list of addresses is shown one per line and a person edits one
    /// line at a time, so a row that could only answer for the whole value
    /// would leave every line looking equally theirs. It is not: an address the
    /// canvas drew cannot be typed away — the link is still drawn — and a box
    /// that accepted the edit would write the canvas's whole contribution into
    /// their half and reorder its neighbours in the same act. Both were
    /// measured on this screen the first time it was driven.
    ///
    /// Derivable rather than stored, and derivable only because
    /// [`FieldType::compose`] puts the written half FIRST: the elements before
    /// the written half's count are somebody's and the rest are the
    /// derivation's. That is the second thing the composition order buys.
    ///
    /// A row with one contributor answers the same thing for every element as
    /// it does for itself, and an index past the end answers for the row —
    /// there is nothing there to have come from anywhere.
    #[must_use]
    pub fn element_source(&self, at: usize) -> Source {
        let Some(worked_out) = &self.worked_out else {
            return Source::Authored;
        };
        let Some(written) = &self.written else {
            return Source::Derived(worked_out.from.clone());
        };
        if at < FieldType::elements(written).count() {
            Source::Authored
        } else {
            Source::Derived(worked_out.from.clone())
        }
    }

    /// How many of the shown value's elements came from the derivation rather
    /// than from somebody — `0` on a row with one contributor.
    ///
    /// ★★ R1717 — the number the floor cannot produce: a cell holding a
    /// composed value answers 2 of 256 standard roles there and none of them is
    /// this. A reader looking at four addresses needs to know that three of
    /// them will change when the drawing does.
    #[must_use]
    pub fn derived_elements(&self) -> usize {
        let (Some(written), Some(worked_out)) = (&self.written, &self.worked_out) else {
            return 0;
        };
        let mine: Vec<&str> = FieldType::elements(written).collect();
        FieldType::elements(&worked_out.value)
            .filter(|element| !mine.contains(element))
            .count()
    }

    /// Whether the **written half** differs from what the form opened with.
    ///
    /// ★★★★★ R1717 — a question about writing, so it is asked of the half
    /// somebody wrote. A row whose shown value moved because the canvas moved
    /// was not edited by anybody, and a form that said it was would offer a
    /// "put it back" that puts nothing back.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.written != self.original
    }

    /// Set the value.
    ///
    /// ★★★★★ R1716 — **the refusal is the value's own.** The floor's
    /// read-only is a view's manners: measured at 6.11, writing into a cell
    /// whose editing had been cleared returned success and changed the value,
    /// so the guarantee holds exactly as long as every writer remembers. Here
    /// the row that knows where its value came from is the row that answers,
    /// and a screen cannot forget on its behalf.
    ///
    /// # Errors
    ///
    /// [`FormError::Derived`] — the value is worked out rather than written,
    /// and the error names what from. [`ConfigForm::author`] is the way to take
    /// it over.
    pub fn set(&mut self, value: impl Into<String>) -> Result<(), FormError> {
        if self.written.is_none() {
            let from = self.worked_out.as_ref().map_or("", |w| w.from.as_ref());
            return Err(FormError::Derived {
                key: self.key.to_string(),
                from: from.to_owned(),
            });
        }
        self.written = Some(value.into());
        // ★★ R1717 — writing an empty half onto a shared row gives it back to
        // the derivation, which is the same act `ConfigForm::remove` performs
        // and has to mean the same thing however a person reaches it.
        self.settle_ownership();
        Ok(())
    }

    /// Give this row back to the thing it is worked out from: the written half
    /// goes and the derivation stands alone.
    ///
    /// ★★★ R1717 — the mirror of [`Self::author`], and what
    /// [`ConfigForm::remove`] does to a shared row instead of taking it off the
    /// screen. Taking the row away would be wrong twice: the derivation is
    /// still true, so the row is back one render later, and the reader would
    /// have watched a row they did not delete disappear and return.
    ///
    /// # Errors
    ///
    /// [`FormError::NotDerived`] — nothing is deriving this row, so there is
    /// nobody to give it back to.
    pub fn disown(&mut self) -> Result<(), FormError> {
        if self.worked_out.is_none() {
            return Err(FormError::NotDerived(self.key.to_string()));
        }
        self.written = None;
        self.original = None;
        Ok(())
    }

    /// Take this row over: it becomes somebody's to write, holding the value it
    /// was last worked out to be.
    ///
    /// ★★★★★ R1716 — **the act the floor performs silently.** Measured at
    /// 6.11: writing into a derived value drops its derivation with one
    /// ordinary value-changed notification and no way afterwards to learn what
    /// was dropped. A person who does that to a mode they meant to read has no
    /// signal at all. So the act is named, it answers what it displaced, and
    /// the value it leaves behind is the derived one — taking a value over
    /// starts from what it *was*, never from empty.
    ///
    /// # Errors
    ///
    /// [`FormError::NotDerived`] — the row is already somebody's writing, so
    /// there is nothing to take over.
    pub fn author(&mut self) -> Result<Takeover, FormError> {
        let seeded = self.value().into_owned();
        let Some(worked_out) = self.worked_out.take() else {
            return Err(FormError::NotDerived(self.key.to_string()));
        };
        self.written = Some(seeded.clone());
        self.original = Some(seeded.clone());
        Ok(Takeover {
            key: self.key.to_string(),
            was: worked_out.from.into_owned(),
            seeded,
        })
    }

    /// Accept the current value as the settled one — what a successful launch
    /// does, after which nothing is pending a restart.
    pub fn settle(&mut self) {
        self.original.clone_from(&self.written);
    }

    /// Put the value back to what the form opened with.
    ///
    /// ★ R1678 — the mirror of [`settle`](Self::settle), and the asymmetry it
    /// closes is the whole reason it exists: a form could move its baseline
    /// FORWARD and had no way back to it. `edited()` asks the question, this
    /// answers it, and after this call `edited()` is false by construction —
    /// which is what makes "is there anything to put back" and "put it back"
    /// one fact instead of two.
    pub fn revert(&mut self) {
        self.written.clone_from(&self.original);
    }
}

/// A node's whole configuration form: the fields it holds, and the keys it
/// could still be given.
///
/// The unset keys are part of the value rather than a list the screen keeps,
/// because "which fields can be added" is a fact about the node's kind and a
/// second copy would drift from the first the moment a kind gains a key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ConfigForm {
    fields: Vec<ConfigField>,
    /// ★★ R1686 — **the kind's catalogue**: every key this form is willing to
    /// offer, held or not, invariant for the form's life.
    ///
    /// R1650 modelled the offered set as a mutable bucket that
    /// [`remove`](Self::remove) pushed into, and that made the list a record of
    /// this one node's history rather than a fact about its kind: a path typed
    /// by hand through [`add_typed`](Self::add_typed) and then taken out came
    /// back as a chip, so the form began offering, to every later reader, a key
    /// somebody once mistyped. Measured on the reference tool this widget's
    /// shape comes from: its offered list is derived, `catalogue(role)` minus
    /// what is held, and a hand-typed key removed simply goes.
    ///
    /// The opening `fields` are in here too, because a row the form opened
    /// holding is by definition a key this kind can have — that is what makes
    /// taking one out reversible with the same gesture that adds one.
    catalogue: Vec<ConfigField>,
    /// R1686 — rows taken out, each already put back to its opening value.
    ///
    /// Not "the removed rows the screen might want back": the rows
    /// [`revert`](Self::revert) has to be able to restore. A row is only ever
    /// moved from `fields` to here, so `fields ∪ parked` covers every key in
    /// `opened_with` and the revert cannot fail to find one.
    ///
    /// It is *not* what [`addable`](Self::addable) reads — a parked row whose
    /// key the catalogue does not name is restorable and not offerable, which
    /// is exactly the hand-typed path's case.
    parked: Vec<ConfigField>,
    /// R1678 — the keys this form opened with, in the order it showed them.
    ///
    /// The membership half of "has this been changed", which
    /// [`ConfigField::edited`] cannot carry: a row that was ADDED after the
    /// form opened is not edited (its value is its original), and a row that
    /// was REMOVED is not in `fields` to be asked. Both are changes, and
    /// neither was expressible until this list existed.
    ///
    /// A list of keys rather than a copy of the rows, because
    /// [`add`](Self::add) and [`remove`](Self::remove) only ever MOVE a field
    /// between `fields` and `parked` — the union of the two covers it, so the
    /// keys are enough to put the membership back and no row is ever lost.
    opened_with: Vec<String>,
}

impl ConfigForm {
    /// A form holding `fields`, offering `addable` as the keys not yet set.
    ///
    /// A key present in both is **kept only in `fields`**: a form that offered
    /// to add something it already has would let a user create a duplicate row,
    /// and two rows for one path is a configuration with no single value.
    ///
    /// R1686 — `addable` is the **curated** part of the catalogue and keeps its
    /// order, which is the order the chips are laid out in; the opening rows
    /// are appended after it, so a key only takes a place in that row once it
    /// has been removed and the curated order is never disturbed by history.
    #[must_use]
    pub fn new(fields: Vec<ConfigField>, addable: Vec<ConfigField>) -> Self {
        let mut form = Self::default();
        for field in fields {
            form.upsert(field);
        }
        for candidate in addable {
            if !form.catalogue.iter().any(|f| f.key() == candidate.key()) {
                form.catalogue.push(candidate);
            }
        }
        for held in &form.fields {
            if !form.catalogue.iter().any(|f| f.key() == held.key()) {
                form.catalogue.push(held.clone());
            }
        }
        form.opened_with = form.fields.iter().map(|f| f.key().to_owned()).collect();
        form
    }

    /// Put a field in, replacing any row at the same key.
    ///
    /// ★★ R1686 — **a row the form opened with goes back where it opened.** A
    /// plain push made "take it out and put it back" move the row to the end,
    /// which is not the identity it looks like: the order is what a reader
    /// navigates a form by, and [`edited`](Self::edited) reads the key order as
    /// one of the two ways a form can differ from how it opened — so a row that
    /// came back in the wrong place left the form permanently reporting a
    /// change nobody had made. The reference tool cannot have this bug because
    /// it hides a row with an overlay and never touches the definition's order;
    /// this type mutates in place, so the order has to be **derived** from the
    /// opening key list instead.
    ///
    /// Rows added after the form opened are not in that list and follow, in the
    /// order they were added.
    fn upsert(&mut self, field: ConfigField) {
        if let Some(at) = self.fields.iter().position(|f| f.key() == field.key()) {
            self.fields[at] = field;
            return;
        }
        let Some(rank) = self.opened_with.iter().position(|k| k == field.key()) else {
            self.fields.push(field);
            return;
        };
        let mut at = self.fields.len();
        for (index, held) in self.fields.iter().enumerate() {
            let held_rank = self.opened_with.iter().position(|k| k == held.key());
            if held_rank.is_none_or(|held_rank| held_rank > rank) {
                at = index;
                break;
            }
        }
        self.fields.insert(at, field);
    }

    /// The rows the form shows, in order.
    #[must_use]
    pub fn fields(&self) -> &[ConfigField] {
        &self.fields
    }

    /// The keys this node could still be given.
    ///
    /// ★★ R1686 — **derived** from the catalogue rather than stored, so it is a
    /// fact about the kind and not a record of what this node has been through.
    /// [`add`](Self::add) accepts exactly what this offers, which is the rule
    /// that keeps a declaration a precondition of dispatch.
    #[must_use]
    pub fn addable(&self) -> Vec<&ConfigField> {
        self.catalogue
            .iter()
            .filter(|c| !self.fields.iter().any(|f| f.key() == c.key()))
            .collect()
    }

    /// The row at that path.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&ConfigField> {
        self.fields.iter().find(|f| f.key() == key)
    }

    /// Set the value at that path.
    ///
    /// # Errors
    ///
    /// [`FormError::NoSuchField`] — a path this form does not hold. Named
    /// rather than inserted, because a typo silently becoming a new key is how
    /// an unknown-key warning gets created by the tool itself.
    ///
    /// [`FormError::Derived`] — R1716, a row whose value is worked out rather
    /// than written. The refusal comes from the row, not from a check here, so
    /// every path into a value passes it.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> Result<(), FormError> {
        let field = self
            .fields
            .iter_mut()
            .find(|f| f.key() == key)
            .ok_or_else(|| FormError::NoSuchField(key.to_string()))?;
        field.set(value)
    }

    /// Take the row at that path over — see [`ConfigField::author`].
    ///
    /// # Errors
    ///
    /// [`FormError::NoSuchField`] or [`FormError::NotDerived`].
    pub fn author(&mut self, key: &str) -> Result<Takeover, FormError> {
        self.fields
            .iter_mut()
            .find(|f| f.key() == key)
            .ok_or_else(|| FormError::NoSuchField(key.to_string()))?
            .author()
    }

    /// The rows this form worked out for itself, in the order it shows them.
    ///
    /// R1716 — the rollup the floor has no shape for: it can be asked, one
    /// value at a time, whether that value is derived, and there is no list.
    #[must_use]
    pub fn derived(&self) -> Vec<&ConfigField> {
        self.fields
            .iter()
            .filter(|f| f.worked_out().is_some())
            .collect()
    }

    /// R1717 — the rows a person and this screen **both** have something to say
    /// about, in the order it shows them.
    ///
    /// A separate rollup from [`Self::derived`] rather than a filter over it,
    /// because the two answer different questions and a reader of one is not
    /// asking the other: `derived` is "which of these does the screen keep up
    /// to date", and this is "which of these will change under somebody who
    /// thinks they own it".
    #[must_use]
    pub fn shared(&self) -> Vec<&ConfigField> {
        self.fields
            .iter()
            .filter(|f| matches!(f.source(), Source::Shared(_)))
            .collect()
    }

    /// Move an offered key into the form.
    ///
    /// R1686 — the row that comes back is the one that was taken out, if this
    /// form took one out, and otherwise a fresh copy of the catalogue's. Both
    /// hold their opening value, because [`remove`](Self::remove) puts a row
    /// back to it on the way out: adding a key is adding a *field*, never
    /// resurrecting a value nobody can see they still have.
    ///
    /// # Errors
    ///
    /// [`FormError::NotAddable`] — a key [`addable`](Self::addable) does not
    /// offer, which includes a hand-typed path that has been removed.
    pub fn add(&mut self, key: &str) -> Result<(), FormError> {
        let offered = self.catalogue.iter().find(|f| f.key() == key).cloned();
        let (Some(offered), None) = (offered, self.field(key)) else {
            return Err(FormError::NotAddable(key.to_string()));
        };
        let field = match self.parked.iter().position(|f| f.key() == key) {
            Some(at) => self.parked.remove(at),
            None => offered,
        };
        self.upsert(field);
        Ok(())
    }

    /// Put a row the form was never offering into it (R1683).
    ///
    /// ★★ **The catalogue is a list of the keys worth reaching for, not the
    /// boundary of what a configuration has.** [`Self::add`] moves a key out of
    /// the offered set, which is exactly right for a chip a person pressed and
    /// exactly wrong for a path they TYPED — a settings form that can only hold
    /// what somebody thought to offer cannot claim to edit a configuration. The
    /// reference tool this shape comes from says so beside its own key box and
    /// derives a descriptor for any path its schema knows.
    ///
    /// The caller supplies the whole [`ConfigField`], because what a typed path
    /// is — its type, its shape, whether it applies hot — is knowledge this
    /// widget does not have and the application does.
    ///
    /// # Errors
    ///
    /// [`FormError::AlreadyHeld`] when a row of that key is already in the
    /// form. Refused rather than upserted: "add" and "replace what is there"
    /// are different requests, and a person typing a key they already have has
    /// made a mistake worth being told about.
    pub fn add_typed(&mut self, field: ConfigField) -> Result<(), FormError> {
        let key = field.key().to_owned();
        if self.fields.iter().any(|f| f.key() == key) {
            return Err(FormError::AlreadyHeld(key));
        }
        // A key the catalogue WAS offering stops being offered because the form
        // now holds it — `addable` derives that, so there is nothing to do
        // here. R1683 had to retain it out of a bucket by hand.
        self.parked.retain(|f| f.key() != key);
        self.upsert(field);
        Ok(())
    }

    /// Take a key back out.
    ///
    /// ★★ R1686 — the row leaves **as it opened**, not as it was last typed
    /// into. A removed row holding an edit is a ghost: it is off the screen,
    /// nothing shows the value, and [`add`](Self::add) would resurrect a number
    /// nobody could see they still had. The reference tool drops the edit in
    /// the same act that takes the row away, and rebuilds it from its
    /// definition when it comes back.
    ///
    /// Whether the key is **offered again** afterwards is not decided here: it
    /// is decided by whether the catalogue names it, which
    /// [`addable`](Self::addable) derives. A key the form opened holding, or one
    /// it was offered, comes back as a chip; a path typed in by hand does not.
    ///
    /// ★★ R1716 — **a row nobody wrote is not a row anybody may take away.**
    /// Removing it would put the form back in front of the same value one
    /// render later, because the derivation is still true; the act a person
    /// actually wants there is [`author`](Self::author), and the seat the
    /// painter draws on such a row offers exactly that.
    ///
    /// # Errors
    ///
    /// [`FormError::NoSuchField`], or [`FormError::Derived`] naming what the
    /// value comes from.
    pub fn remove(&mut self, key: &str) -> Result<(), FormError> {
        let at = self
            .fields
            .iter()
            .position(|f| f.key() == key)
            .ok_or_else(|| FormError::NoSuchField(key.to_string()))?;
        match self.fields[at].source() {
            // Nobody wrote it, so there is nothing here to take away.
            Source::Derived(from) => {
                return Err(FormError::Derived {
                    key: key.to_string(),
                    from: from.into_owned(),
                });
            }
            // ★★★★★ R1717 — **the written half goes and the row stays.** The
            // derivation is still true, so a row taken off the screen is back
            // one render later; what a person asked for is to stop owning it,
            // and that is what they get. The seat the painter draws on such a
            // row says so in its own word.
            Source::Shared(_) => {
                self.fields[at].disown()?;
                return Ok(());
            }
            Source::Authored => {}
        }
        let mut field = self.fields.remove(at);
        field.revert();
        self.parked.retain(|f| f.key() != key);
        self.parked.push(field);
        Ok(())
    }

    /// The **edited** fields whose change will not reach a running node.
    ///
    /// Edited, not merely restart-scoped: a form that listed every
    /// restart-scoped field would tell a person to restart for values they
    /// never touched, which is the superstition the badge exists to end.
    #[must_use]
    pub fn pending_restart(&self) -> Vec<&ConfigField> {
        self.fields
            .iter()
            .filter(|f| f.edited() && f.applies().needs_restart())
            .collect()
    }

    /// Accept every current value **and the current shape** — what a successful
    /// launch does.
    ///
    /// R1678 added the second half. A launch's meaning is "what is running now
    /// IS what the screen shows", and a form that settled its values while
    /// still reporting its added rows as changes would contradict that in the
    /// one place a person looks to check it.
    ///
    /// R1686 added the third half, and it is a consequence of the first two: a
    /// settled form's opening state is what it now shows, so a row that is not
    /// shown is not part of it and there is nothing for `parked` to keep. A
    /// later [`add`](Self::add) of an offered key is then what it says — a
    /// fresh field from the catalogue — rather than a row from before a launch
    /// that no longer describes anything running.
    pub fn settle(&mut self) {
        for field in &mut self.fields {
            field.settle();
        }
        self.parked.clear();
        self.opened_with = self.fields.iter().map(|f| f.key().to_owned()).collect();
    }

    /// ★★ R1678 — whether this form differs from the state it opened in, in
    /// **either** way it can: a value that was changed, or a row that was
    /// added or taken away.
    ///
    /// Derived, and that is the point. The reference tool keeps every edit as
    /// an overlay on the opening state, so it answers this with one `is_empty`
    /// and reverts with one `clear`; this type mutates in place, so the same
    /// fact has to come from a comparison — and it has to come from ONE
    /// comparison, or a screen showing a "put it back" affordance would be
    /// deciding for itself when there is anything to put back.
    ///
    /// The membership half is not decoration: measured on the reference, three
    /// of its four gated reset affordances are gated on a predicate that counts
    /// added and hidden rows as well as edited values.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.fields.iter().any(ConfigField::edited)
            || self.fields.len() != self.opened_with.len()
            || self
                .fields
                .iter()
                .zip(&self.opened_with)
                .any(|(field, key)| field.key() != key)
    }

    /// ★★ R1678 — put the whole form back to the state it opened in: every
    /// value, and every row that was added or taken away.
    ///
    /// The rows themselves are never rebuilt, only moved back: [`add`](Self::add)
    /// and [`remove`](Self::remove) shuttle a field between `fields` and
    /// `parked`, so this partitions that union by the opening key list and
    /// restores the opening ORDER — which a set-based repair would lose, and
    /// the order is what a reader navigates the form by.
    ///
    /// ★ R1686 — which is why `parked` keeps a hand-typed row it will never
    /// offer again. Restorable and offerable are different questions, and a
    /// revert that answered the second one would drop a path the form opened
    /// holding just because nothing was willing to suggest it.
    ///
    /// [`edited`](Self::edited) is false afterwards, always. That is asserted
    /// rather than assumed, because the two are one fact and a revert that left
    /// the form still reporting a change would make the affordance that ran it
    /// reappear.
    pub fn revert(&mut self) {
        let mut held: Vec<ConfigField> = std::mem::take(&mut self.fields);
        held.append(&mut self.parked);
        for field in &mut held {
            field.revert();
        }
        for key in &self.opened_with {
            if let Some(at) = held.iter().position(|f| f.key() == key) {
                let field = held.remove(at);
                self.fields.push(field);
            }
        }
        // Whatever the opening list did not name was added after the form
        // opened, so it is not part of the state being restored and nothing is
        // waiting for it. A catalogue key among them is offered again by
        // derivation; a hand-typed one is gone, which is what "put it back to
        // how it opened" means for a row that was not there when it opened.
        drop(held);
        self.parked.clear();
    }

    /// Every way this form is wrong, row by row, in row order.
    ///
    /// R1651 — **derived**, where R1650 took the list as an argument. A caller
    /// that assembled its own list could show a clean gate over a form holding
    /// an unparseable value; there is now one source for both.
    ///
    /// A [`ConfigField::hidden`] row is still checked. Hiding a row is a screen
    /// decision and a launch does not care what was on screen — a gate that
    /// skipped hidden rows would let a value block a start-up with nothing on
    /// the screen saying which.
    #[must_use]
    pub fn defects(&self) -> Vec<ConfigDefect> {
        self.fields.iter().flat_map(ConfigField::defects).collect()
    }

    /// Whether this form's own defects stop a launch.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        Verdict::over(&self.defects())
    }

    /// The **deployable configuration** these rows describe: one document,
    /// nested from the dotted paths the rows are addressed by.
    ///
    /// This is the derivation that makes the key being the configuration path
    /// worth insisting on. A hidden row still contributes — see
    /// [`Self::defects`] for why the screen does not get a vote here either.
    ///
    /// # Errors
    ///
    /// [`DocumentError::Defective`] carrying every defect, because a document
    /// emitted from a form with a wrong-typed value would be a file that fails
    /// at start-up with the tool having said it was fine. A warning does not
    /// stop it: an unknown key is written out, since the target is the thing
    /// that decides whether it knows a key.
    ///
    /// [`DocumentError::PathCollision`] when one key is a prefix of another —
    /// `a` and `a.b` cannot both be values in one document, and silently
    /// dropping either is how a configuration loses a setting nobody deleted.
    pub fn document(&self) -> Result<Value, DocumentError> {
        // ★ Derived from `compose`, which is the total form of the same walk.
        // Two functions that both nested the rows would be two answers to one
        // question, and the pair a caller most needs to trust — "is this
        // shippable" and "what did not fit" — is exactly the pair that must not
        // be able to disagree. The narrowing happens here and nowhere else.
        let composed = self.compose();
        let defects: Vec<ConfigDefect> = composed
            .unexpressed
            .iter()
            .filter_map(|row| match &row.why {
                Unexpressible::Defective(defect) => Some(defect.clone()),
                Unexpressible::Collides { .. } => None,
            })
            .collect();
        if !defects.is_empty() {
            return Err(DocumentError::Defective(defects));
        }
        // A value defect is reported ahead of a collision, which is the order
        // this returned them in before it was derived: a wrong value is about
        // one row and a collision is about two, so the reader wants the smaller
        // news first.
        if let Some((key, at)) = composed.unexpressed.iter().find_map(|row| match &row.why {
            Unexpressible::Collides { at } => Some((row.key.clone(), at.clone())),
            Unexpressible::Defective(_) => None,
        }) {
            return Err(DocumentError::PathCollision { key, at });
        }
        Ok(composed.document)
    }

    /// The document these rows describe **together with the rows that could not
    /// go into it** — the total form of [`Self::document`].
    ///
    /// # ★★★ R1687 — the read half was already total and this half was not
    ///
    /// [`Self::adopt`] answers a document with [`Adopted`], whose own
    /// documentation says it out loud: *every leaf is either placed on a row or
    /// **named** in [`Adopted::unplaceable`]; nothing is dropped quietly.* The
    /// reason given there is that a configuration written by a newer target is
    /// the normal case.
    ///
    /// The way back had no such form. [`Self::document`] answers a whole
    /// document or none at all, so a caller whose job is to **ship** the
    /// configuration and **report** what did not fit had nothing to call: one
    /// unparseable row and it is handed an error instead of the other forty
    /// rows, which are fine and are what somebody asked for. The asymmetry was
    /// invisible while the only caller was a launch gate, because a gate only
    /// ever wanted the yes-or-no.
    ///
    /// So the two directions now say the same thing in the same shape, and
    /// [`Self::document`] is the narrowing of this rather than a second walk.
    ///
    /// **It never fails.** A row that cannot be carried is news about that row,
    /// not about the document — which is the whole point, and is why the return
    /// type has no `Result` to unwrap past.
    #[must_use]
    pub fn compose(&self) -> Composed {
        let mut root = Map::new();
        let mut unexpressed: Vec<Unexpressed> = Vec::new();
        let mut aside: Vec<Aside> = Vec::new();
        for field in &self.fields {
            // ★★ R1716 — a row that goes aside is answered BEFORE it is
            // encoded, because "this is not configuration" is not a failure to
            // express it: putting it through the same walk would either invent
            // a path the target does not have or report a defect about a row
            // that was never headed for the document.
            if let Some(instead) = field.goes().instead() {
                aside.push(Aside {
                    key: field.key().to_string(),
                    shown: field.value().to_string(),
                    instead: instead.to_owned(),
                });
                continue;
            }
            let mut refuse = |why| {
                unexpressed.push(Unexpressed {
                    key: field.key().to_string(),
                    shown: field.value().to_string(),
                    why,
                });
            };
            let encoded = match field.encoded() {
                Ok(value) => value,
                Err(defect) => {
                    refuse(Unexpressible::Defective(defect));
                    continue;
                }
            };
            if let Err(at) = Self::place(&mut root, field.key(), encoded) {
                refuse(Unexpressible::Collides { at });
            }
        }
        Composed {
            document: Value::Object(root),
            unexpressed,
            aside,
        }
    }

    /// Put `value` at the dotted `key` inside `root`, creating the objects on
    /// the way.
    ///
    /// # Errors
    ///
    /// The prefix already holding a value, which is the only way this can fail.
    /// It answers the prefix rather than a [`DocumentError`] so that the two
    /// callers can each say it in their own words — and so that neither has an
    /// arm for a variant this cannot produce.
    fn place(root: &mut Map<String, Value>, key: &str, value: Value) -> Result<(), String> {
        let mut here = root;
        let mut walked: Vec<&str> = Vec::new();
        let mut segments = key.split('.').peekable();
        while let Some(segment) = segments.next() {
            walked.push(segment);
            if segments.peek().is_none() {
                if here.contains_key(segment) {
                    return Err(walked.join("."));
                }
                here.insert(segment.to_string(), value);
                return Ok(());
            }
            let next = here
                .entry(segment.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            here = next.as_object_mut().ok_or_else(|| walked.join("."))?;
        }
        Ok(())
    }

    /// Read a configuration document back into these rows.
    ///
    /// The inverse of [`Self::document`], and the half that makes the pair
    /// worth having: a form and a settings file that are mapped by hand are
    /// mapped **twice**, and nothing checks the two agree.
    ///
    /// Every leaf the document holds is either placed on the row at that path
    /// or **named** in [`Adopted::unplaceable`]. Nothing is dropped quietly: a
    /// configuration written by a newer target is the normal case, and a form
    /// that swallowed the keys it did not recognise would let a person save the
    /// file back with those settings gone.
    ///
    /// Values are written in the canonical spelling, so a list typed
    /// `read,write` reads back `read, write`. That normalisation is the point
    /// rather than an accident — it is what makes the document the single
    /// spelling of the value.
    pub fn adopt(&mut self, document: &Value) -> Adopted {
        let mut leaves: Vec<(String, &Value)> = Vec::new();
        Self::leaves(String::new(), document, &mut leaves);
        let mut adopted = Adopted::default();
        for (path, value) in leaves {
            let Some(field) = self.fields.iter_mut().find(|f| f.key() == path) else {
                adopted.unplaceable.push(path);
                continue;
            };
            match field.shape.decode(&path, value) {
                Ok(text) => match field.set(text) {
                    Ok(()) => adopted.set.push(path),
                    // ★★ R1716 — a file does not get to seize a row from the
                    // thing it is worked out from. The leaf is reported instead
                    // of written, which is the same promise this method has
                    // always made about the keys it cannot place: a document
                    // whose `mode` differs from what the role implies is news,
                    // and adopting it silently would make the screen show a
                    // value its own derivation contradicts.
                    Err(_) => adopted.derived.push(path),
                },
                Err(defect) => adopted.refused.push(defect),
            }
        }
        adopted
    }

    /// Every scalar and array in `value`, addressed by its dotted path.
    fn leaves<'a>(prefix: String, value: &'a Value, out: &mut Vec<(String, &'a Value)>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    Self::leaves(path, child, out);
                }
            }
            other => out.push((prefix, other)),
        }
    }
}

/// What [`ConfigForm::adopt`] did with a document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Adopted {
    /// The paths that landed on a row.
    pub set: Vec<String>,
    /// The paths this form has no row for — reported, never dropped.
    pub unplaceable: Vec<String>,
    /// The values a row exists for and cannot hold.
    pub refused: Vec<ConfigDefect>,
    /// ★★ R1716 — the paths whose row works its value out for itself.
    ///
    /// Not written and not dropped. A screen that wants the file's value there
    /// takes the row over first ([`ConfigForm::author`]) — which is a decision
    /// somebody makes, not one a load performs on their behalf.
    #[serde(default)]
    pub derived: Vec<String>,
}

impl Adopted {
    /// Whether every leaf of the document reached a row.
    ///
    /// R1716 — a derived path counts against it, because the document's value
    /// for that path is not what the form now shows, and a reader who was told
    /// "complete" would believe it was.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.unplaceable.is_empty() && self.refused.is_empty() && self.derived.is_empty()
    }
}

/// A document and everything the form holds that could not go into it.
///
/// The mirror image of [`Adopted`], and deliberately the same shape: reading a
/// document names what it could not place, so writing one names what it could
/// not carry. See [`ConfigForm::compose`] for why the pair had to be made
/// symmetric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// Every row that could be expressed, nested from its dotted path.
    pub document: Value,
    /// Every row that could not, in the order the form holds them.
    pub unexpressed: Vec<Unexpressed>,
    /// ★★ R1716 — every row that was never headed there, and what it is
    /// about instead.
    ///
    /// A third bucket rather than a second meaning for the second one: "did not
    /// fit" is news a person has to act on and "does not belong" is the form
    /// working correctly, and a reader who cannot tell them apart reads a
    /// healthy form as a broken one. It is also the list the floor cannot
    /// produce — it can mark a property not-worth-storing and has no way to
    /// gather the marked ones, let alone say what they are instead.
    pub aside: Vec<Aside>,
}

impl Composed {
    /// Whether every row this form holds reached the document.
    ///
    /// Named to match [`Adopted::complete`] because it is the same question
    /// asked in the other direction.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.unexpressed.is_empty()
    }
}

/// One row a document cannot carry, and why.
///
/// It carries the value **as the row shows it** rather than a parsed one —
/// there is no parsed one, which is generally the reason it is here. A reader
/// of a report has to be able to find the row on the screen, and what is on the
/// screen is this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unexpressed {
    /// The configuration path the row is addressed by.
    pub key: String,
    /// The value verbatim, as the row shows it.
    pub shown: String,
    /// Why it could not be carried.
    pub why: Unexpressible,
}

/// One row that is deliberately not in the document, and what it is instead.
///
/// ★★ R1716 — it carries the same two fields a row that *failed* carries, so
/// a screen listing both reads one shape twice; what differs is the third,
/// which is a **word for what this row is about** rather than a reason it went
/// wrong. Nothing here is a defect and nothing here blocks a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aside {
    /// The path the row is addressed by.
    pub key: String,
    /// The value verbatim, as the row shows it.
    pub shown: String,
    /// What the row is about instead of configuration — `placement`,
    /// `run argument`.
    pub instead: String,
}

/// What a row's value was taken over from — see [`ConfigField::author`].
///
/// ★★★★★ R1716 — **the news the floor does not send.** Measured at 6.11,
/// authoring over a derived value produces one ordinary value-changed
/// notification and leaves the displaced derivation unreachable, so a person
/// who did it by accident cannot find out what they lost. Every field here is
/// one of the three things they would need: which row, what it came from, and
/// what it was holding at the moment they took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Takeover {
    /// The row that is now somebody's to write.
    pub key: String,
    /// What its value used to be worked out from.
    pub was: String,
    /// The value it kept — the derived one, so authoring starts from what the
    /// screen was already showing rather than from nothing.
    pub seeded: String,
}

impl Takeover {
    /// The one line a person reads when it happens.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!("{} is yours now; it came from {}", self.key, self.was)
    }
}

/// Why a row could not go into a document.
///
/// Two arms and not one, because they are about different numbers of rows: a
/// defect is a fact about this row alone, and a collision is a fact about this
/// row **and another one** — so a report that flattened them would lose the
/// only part of a collision a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unexpressible {
    /// The value cannot be turned into one a document can hold.
    Defective(ConfigDefect),
    /// A prefix of this row's path already holds a value, so the path would
    /// have to be a value and an object at once.
    Collides {
        /// The prefix already taken.
        at: String,
    },
}

impl Unexpressible {
    /// The one line a person reads beside the row.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Defective(defect) => defect.sentence(),
            Self::Collides { at } => format!("{at} already holds a value"),
        }
    }
}

/// Why a form could not produce a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// Values that would make the target fail at start-up.
    Defective(Vec<ConfigDefect>),
    /// One key is a prefix of another, so the document would need a path to be
    /// a value and an object at once.
    PathCollision {
        /// The key that could not be placed.
        key: String,
        /// The prefix already taken.
        at: String,
    },
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Defective(defects) => {
                write!(f, "{} value(s) would fail at start-up: ", defects.len())?;
                let sentences: Vec<String> = defects.iter().map(ConfigDefect::sentence).collect();
                f.write_str(&sentences.join("; "))
            }
            Self::PathCollision { key, at } => {
                write!(f, "{key} cannot be placed: {at} already holds a value")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

/// What a launch gate concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    blocking: usize,
    warning: usize,
}

impl Verdict {
    /// The verdict over a defect list somebody else assembled — the target's
    /// own report, rather than the form's derivation.
    ///
    /// [`ConfigForm::verdict`] is the one to reach for over a form's own rows;
    /// this stays public because the gate on screen shows both, and a target
    /// that refuses a value the form thought fine is news the screen has to be
    /// able to state in the same words.
    #[must_use]
    pub fn over(defects: &[ConfigDefect]) -> Self {
        let blocking = defects.iter().filter(|d| d.blocks()).count();
        let warning = defects.len() - blocking;
        Self { blocking, warning }
    }

    /// How many defects stop the launch.
    #[must_use]
    pub const fn blocking(self) -> usize {
        self.blocking
    }

    /// How many are reported and do not stop it.
    #[must_use]
    pub const fn warning(self) -> usize {
        self.warning
    }

    /// Whether a launch may proceed.
    #[must_use]
    pub const fn may_launch(self) -> bool {
        self.blocking == 0
    }

    /// The sentence the gate shows.
    ///
    /// It names the warnings even when it opens, because "nothing is stopping
    /// you" and "nothing is wrong" are different statements and a gate that
    /// only ever said the first is how a partly-specified graph gets launched
    /// without anybody noticing.
    #[must_use]
    pub fn sentence(&self) -> String {
        match (self.blocking, self.warning) {
            (0, 0) => "nothing to fix — launch is open".to_string(),
            (0, w) => format!("nothing is blocking launch; {w} warning(s) stand"),
            (b, 0) => format!("{b} error(s) block launch"),
            (b, w) => format!("{b} error(s) block launch; {w} warning(s) stand"),
        }
    }
}

/// What a form refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    /// A path the form does not hold.
    NoSuchField(String),
    /// A key the node's kind does not offer.
    NotAddable(String),
    /// A key the form already holds a row for (R1683).
    AlreadyHeld(String),
    /// R1716 — a row whose value is worked out rather than written, and what it
    /// is worked out from.
    ///
    /// The source travels with the refusal because a refusal a person cannot
    /// act on is barely better than a silent one: `mode is worked out from the
    /// role` tells them where to go.
    Derived {
        /// The row that refused.
        key: String,
        /// What its value comes from.
        from: String,
    },
    /// R1716 — a row asked to be taken over that nobody was deriving.
    NotDerived(String),
    /// R1717 — a row asked to hold a written half and a worked-out one at once
    /// when its shape holds a **single** value.
    ///
    /// Two contributions to one address list compose; two contributions to one
    /// mode contradict. The refusal names the declared type, because that is
    /// the word on the row's badge and so the word the caller can look for.
    Unmergeable {
        /// The row that refused.
        key: String,
        /// The type the configuration calls it, verbatim from the row.
        ty: String,
    },
}

impl std::fmt::Display for FormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchField(key) => write!(f, "this node has no field {key}"),
            Self::NotAddable(key) => write!(f, "{key} is not a key this node kind offers"),
            Self::AlreadyHeld(key) => write!(f, "this node already holds {key}"),
            Self::Derived { key, from } => {
                write!(f, "{key} is worked out from the {from}, not written here")
            }
            Self::NotDerived(key) => write!(f, "{key} is already yours to write"),
            Self::Unmergeable { key, ty } => write!(
                f,
                "{key} holds one {ty}, so a written value and a worked-out one cannot both stand"
            ),
        }
    }
}

impl std::error::Error for FormError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::text_format::{CharClass, CharSet, Span, TextFormat};
    use super::{
        Applies, Aside, ConfigDefect, ConfigField, ConfigForm, Derivation, DocumentError,
        FieldType, FormError, Source, Takeover, Unexpressible, Value, Verdict,
    };

    /// The five rows the reference tool's node inspector shows, with the shapes
    /// they are declared with. Every arm of [`FieldType`] appears — which is
    /// the point: the vocabulary was chosen from what one real inspector needs,
    /// not from what a widget catalogue offers.
    fn inspector() -> ConfigForm {
        ConfigForm::new(
            vec![
                // ★ R1690 — the identifier is this inspector's formatted
                // string, and was free text until the arm for it existed. It
                // is the field the arm was added FOR: the target reads it with
                // a parser, so a value this form accepts and that parser does
                // not is a node that will not come up.
                ConfigField::new("id", "id", Applies::Restart, "a1").with_shape(
                    FieldType::Formatted {
                        of: TextFormat::Chars {
                            allow: CharSet::of(&[CharClass::LowerHex]),
                            len: Span::between(1, 32),
                        },
                    },
                ),
                ConfigField::new("listen.endpoints", "locator[]", Applies::Restart, "t/0.0:1")
                    .with_shape(FieldType::List {
                        of: Box::new(FieldType::Text),
                    }),
                ConfigField::new("connect.endpoints", "locator[]", Applies::Hot, "t/2.1:3")
                    .with_shape(FieldType::List {
                        of: Box::new(FieldType::Text),
                    }),
                ConfigField::new(
                    "control.permissions",
                    "perm",
                    Applies::Restart,
                    "read, write",
                )
                .with_shape(FieldType::Flags {
                    of: vec!["read".into(), "write".into()],
                }),
                ConfigField::new(
                    "transport.link.tx.batch_size",
                    "int",
                    Applies::Restart,
                    "65535",
                )
                .with_shape(FieldType::Integer { min: 0, max: 65535 }),
            ],
            vec![
                ConfigField::new("discovery.multicast", "bool", Applies::Restart, "false")
                    .with_shape(FieldType::Boolean),
                ConfigField::new("routing.mode", "mode", Applies::Restart, "peer_to_peer")
                    .with_shape(FieldType::Choice {
                        of: vec!["peer_to_peer".into(), "client".into()],
                    }),
                // ★★★ R1690 — a genuinely free string, and it took the
                // formatted arm to expose that there had not been one. Every
                // row above holds a value something downstream parses; this
                // holds a note a person writes for another person, which is
                // what free text is FOR. Before the arm existed the identifier
                // sat in this class by default and hid the fact that the class
                // was otherwise empty.
                ConfigField::new("metadata.note", "text", Applies::Hot, ""),
            ],
        )
    }

    fn form() -> ConfigForm {
        ConfigForm::new(
            vec![
                ConfigField::new("id", "text", Applies::Restart, "a1"),
                ConfigField::new("listen.endpoints", "locator[]", Applies::Restart, "t/0:1"),
                ConfigField::new("connect.endpoints", "locator[]", Applies::Hot, "t/2:3"),
            ],
            vec![
                ConfigField::new("scouting.multicast", "bool", Applies::Restart, "false"),
                ConfigField::new("timestamping", "bool", Applies::Restart, "false"),
            ],
        )
    }

    #[test]
    fn r1650_the_applies_scope_is_a_two_word_closed_vocabulary() {
        assert_eq!(Applies::ALL.len(), Applies::ARMS);
        let mut seen = std::collections::BTreeSet::new();
        for applies in Applies::ALL {
            assert!(seen.insert(applies.wire()), "{} spells two", applies.wire());
            assert_eq!(Applies::from_wire(applies.wire()), Some(applies));
        }
        assert_eq!(Applies::from_wire("later"), None, "closed set");
        assert!(!Applies::Hot.needs_restart());
        assert!(Applies::Restart.needs_restart());
    }

    #[test]
    fn r1650_an_unknown_key_warns_and_the_other_two_block() {
        // ★ The whole reason the vocabulary has three arms. A gate that
        // refused to launch on an unknown key would be stricter than the thing
        // it configures, and a user whose target is newer than the tool could
        // not start anything.
        assert!(!ConfigDefect::UnknownKey { key: "x".into() }.blocks());
        assert!(
            ConfigDefect::WrongType {
                key: "x".into(),
                want: "int".into(),
                got: "text".into()
            }
            .blocks()
        );
        assert!(
            ConfigDefect::OutOfRange {
                key: "x".into(),
                allowed: "1..=9".into()
            }
            .blocks()
        );
        assert_eq!(
            ConfigDefect::all().len(),
            ConfigDefect::ARMS,
            "all() covers it"
        );
    }

    #[test]
    fn r1650_every_defect_names_its_field_and_reads_as_a_sentence() {
        let mut seen = std::collections::BTreeSet::new();
        for defect in ConfigDefect::all() {
            assert!(seen.insert(defect.wire()), "{} spells two", defect.wire());
            assert!(!defect.sentence().is_empty());
        }
        let one = ConfigDefect::WrongType {
            key: "transport.link.tx.batch_size".into(),
            want: "int".into(),
            got: "\"big\"".into(),
        };
        assert_eq!(one.key(), "transport.link.tx.batch_size");
        assert!(one.sentence().contains("batch_size") && one.sentence().contains("int"));
    }

    #[test]
    fn r1650_a_verdict_is_derived_from_the_defects_and_says_what_stands() {
        let clean = Verdict::over(&[]);
        assert!(clean.may_launch() && clean.sentence().contains("launch is open"));

        let warned = Verdict::over(&[ConfigDefect::UnknownKey { key: "who".into() }]);
        assert!(
            warned.may_launch(),
            "a warning must not block — that is the point of three arms"
        );
        assert_eq!((warned.blocking(), warned.warning()), (0, 1));
        assert!(
            warned.sentence().contains("warning"),
            "★ and it SAYS so: 'nothing is blocking you' and 'nothing is wrong' \
             are different statements: {}",
            warned.sentence()
        );

        let blocked = Verdict::over(&[
            ConfigDefect::UnknownKey { key: "who".into() },
            ConfigDefect::OutOfRange {
                key: "rate".into(),
                allowed: "1..=1000".into(),
            },
        ]);
        assert!(!blocked.may_launch());
        assert_eq!((blocked.blocking(), blocked.warning()), (1, 1));
    }

    #[test]
    fn r1650_only_an_edited_restart_field_is_pending() {
        // ★ Edited, not merely restart-scoped. A form listing every
        // restart-scoped field tells a person to restart for values they never
        // touched, which is the superstition the badge exists to end.
        let mut form = form();
        assert!(form.pending_restart().is_empty(), "nothing touched yet");

        form.set("connect.endpoints", "t/9:9").expect("a hot field");
        assert!(
            form.pending_restart().is_empty(),
            "a HOT field reaches the running node, so nothing is pending"
        );

        form.set("listen.endpoints", "t/0:2")
            .expect("a restart field");
        let pending: Vec<&str> = form.pending_restart().iter().map(|f| f.key()).collect();
        assert_eq!(
            pending,
            vec!["listen.endpoints"],
            "only the one that was edited"
        );

        form.settle();
        assert!(form.pending_restart().is_empty(), "a launch settles them");
    }

    #[test]
    fn r1650_a_path_the_form_does_not_hold_is_refused_by_name() {
        // A typo silently becoming a new key is how the tool itself creates the
        // unknown-key warning it exists to report.
        let mut form = form();
        assert_eq!(
            form.set("listen.endpoint", "x"),
            Err(FormError::NoSuchField("listen.endpoint".to_string())),
            "one character wrong is a refusal, not a new field"
        );
        assert_eq!(form.fields().len(), 3, "and the form is unchanged");
    }

    #[test]
    fn r1650_adding_a_key_moves_it_out_of_the_offered_set() {
        let mut form = form();
        assert_eq!(form.addable().len(), 2);
        form.add("timestamping").expect("offered");
        assert_eq!(form.fields().len(), 4);
        assert_eq!(form.addable().len(), 1, "and it is no longer offered");
        assert_eq!(
            form.add("timestamping"),
            Err(FormError::NotAddable("timestamping".to_string())),
            "so it cannot be added twice"
        );
        form.remove("timestamping").expect("held");
        assert_eq!(form.addable().len(), 2, "removing offers it again");
    }

    #[test]
    fn r1686_a_hand_typed_path_taken_out_leaves_no_chip_behind() {
        // ★★ The offered set is a fact about the node's KIND — the keys worth
        // reaching for. A path somebody typed is a fact about this one node,
        // and a form that started offering it because it once held it would be
        // publishing one node's history as the kind's catalogue. Measured on
        // the reference tool: its offered list is derived as
        // `catalogue(role) - held`, so a hand-typed key removed simply goes.
        let mut form = form();
        form.add_typed(
            ConfigField::new(
                "transport.unicast.lowlatency",
                "bool",
                Applies::Restart,
                "false",
            )
            .as_custom(),
        )
        .expect("not held");
        form.remove("transport.unicast.lowlatency").expect("held");
        assert!(
            form.addable()
                .iter()
                .all(|f| f.key() != "transport.unicast.lowlatency"),
            "a typed path is not a catalogue entry: offered {:?}",
            form.addable()
                .into_iter()
                .map(ConfigField::key)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn r1686_a_row_taken_out_and_put_back_comes_back_as_it_opened() {
        // ★★ Taking a row out drops what was typed into it. Otherwise the value
        // is a GHOST: the row is off the screen, nothing shows the edit, and
        // putting the key back resurrects a number nobody can see they still
        // have. The reference drops the edit overlay in the same act that hides
        // the row, and re-adding rebuilds the row from the definition.
        let mut form = form();
        form.set("id", "edited").expect("held");
        assert!(form.edited());
        form.remove("id").expect("held");
        form.add("id").expect("offered again");
        assert_eq!(
            form.field("id").expect("back").value(),
            "a1",
            "the row came back holding what it opened with"
        );
        assert!(
            !form.edited(),
            "and a form at its opening rows and values reports no change"
        );
    }

    #[test]
    fn r1686_a_row_put_back_goes_where_it_opened_rather_than_last() {
        // ★★ Found by the test above rather than reasoned: the value came back
        // and the form still reported a change, because a plain push put the
        // row at the END. The order is what a reader navigates a form by, and
        // it is half of what `edited` compares — so a row that came back in the
        // wrong place left the form permanently dirty with nothing on screen
        // saying why.
        let mut form = form();
        let opening: Vec<String> = form.fields().iter().map(|f| f.key().to_owned()).collect();
        form.remove("listen.endpoints").expect("the middle row");
        form.add("listen.endpoints").expect("offered again");
        assert_eq!(
            form.fields()
                .iter()
                .map(ConfigField::key)
                .collect::<Vec<_>>(),
            opening.iter().map(String::as_str).collect::<Vec<_>>(),
            "the middle row came back in the middle"
        );

        // A row added after the form opened has no opening place, so it goes
        // last — and taking it out and putting it back keeps it there rather
        // than promoting it past rows that were there first.
        form.add("timestamping").expect("offered");
        form.remove("timestamping").expect("held");
        form.add("timestamping").expect("offered again");
        assert_eq!(
            form.fields().last().map(ConfigField::key),
            Some("timestamping")
        );
    }

    #[test]
    fn r1650_a_key_offered_and_held_is_held_once() {
        // Two rows for one path is a configuration with no single value.
        let form = ConfigForm::new(
            vec![ConfigField::new("id", "text", Applies::Restart, "a1")],
            vec![
                ConfigField::new("id", "text", Applies::Restart, "other"),
                ConfigField::new("id", "text", Applies::Restart, "again"),
            ],
        );
        assert_eq!(form.fields().len(), 1);
        assert!(
            form.addable().is_empty(),
            "a form must not offer to add what it already holds"
        );
        assert_eq!(
            form.field("id").expect("held").value(),
            "a1",
            "the held one wins"
        );
    }

    #[test]
    fn r1651_every_shape_in_the_vocabulary_is_exercised_by_one_real_inspector() {
        // The census direction that matters: not "the enum has six arms" but
        // "the screen this was designed for reaches all six". An arm no real
        // form needs is a widget catalogue entry, and this module is not one.
        let form = inspector();
        let mut reached: Vec<&str> = Vec::new();
        for field in form.fields().iter().chain(form.addable()) {
            reached.push(match field.shape() {
                FieldType::Text => "text",
                FieldType::Integer { .. } => "integer",
                FieldType::Boolean => "boolean",
                FieldType::Choice { .. } => "choice",
                FieldType::Flags { .. } => "flags",
                FieldType::List { .. } => "list",
                FieldType::Formatted { .. } => "formatted",
            });
        }
        reached.sort_unstable();
        reached.dedup();
        assert_eq!(
            reached.len(),
            FieldType::ARMS,
            "the inspector reaches {reached:?} of {} shapes",
            FieldType::ARMS
        );
    }

    #[test]
    fn r1651_a_defect_is_derived_from_the_text_rather_than_handed_in() {
        // ★ R1650 took the defect list as an argument, which let a screen show
        // a clean gate over a form holding an unparseable value. Nothing here
        // tells the form it is wrong.
        let mut form = inspector();
        assert!(
            form.defects().is_empty(),
            "the opening values are all sound"
        );
        assert!(form.verdict().may_launch());

        form.set("transport.link.tx.batch_size", "big")
            .expect("held");
        let defects = form.defects();
        assert_eq!(defects.len(), 1, "{defects:?}");
        assert_eq!(defects[0].wire(), "wrong_type");
        assert_eq!(defects[0].key(), "transport.link.tx.batch_size");
        assert!(!form.verdict().may_launch(), "and it blocks the launch");

        form.set("transport.link.tx.batch_size", "70000")
            .expect("held");
        let defects = form.defects();
        assert_eq!(
            defects[0].wire(),
            "out_of_range",
            "★ parseable and outside the bounds is a DIFFERENT answer from \
             unparseable — the reference toolkit's per-field verdict gives the \
             same middle value for this and for an empty box"
        );
        assert!(
            defects[0].sentence().contains("0..=65535"),
            "and it says what would be accepted: {}",
            defects[0].sentence()
        );
    }

    #[test]
    fn r1651_a_hand_typed_path_warns_and_the_launch_still_opens() {
        // The tool cannot know the keys of a target newer than itself. A custom
        // path is therefore the one thing that DERIVES an unknown-key warning,
        // and that warning must not block.
        let mut form = inspector();
        form.add("routing.mode").expect("offered");
        assert!(form.defects().is_empty(), "an offered key is a known key");

        let mut hand = ConfigForm::new(
            vec![
                ConfigField::new("plugins.stats.port", "int", Applies::Restart, "8000")
                    .with_shape(FieldType::Integer { min: 1, max: 65535 })
                    .as_custom(),
            ],
            vec![],
        );
        let defects = hand.defects();
        assert_eq!(defects.len(), 1);
        assert_eq!(defects[0].wire(), "unknown_key");
        assert!(hand.verdict().may_launch(), "a warning does not block");
        assert_eq!(hand.verdict().warning(), 1);

        // And a hand-typed path holding a bad value is TWO pieces of news, one
        // of which blocks. A form reporting only the first would let the second
        // reach start-up.
        hand.set("plugins.stats.port", "0").expect("held");
        let both = hand.defects();
        assert_eq!(both.len(), 2, "{both:?}");
        assert_eq!(both[0].wire(), "unknown_key");
        assert_eq!(both[1].wire(), "out_of_range");
        assert!(!hand.verdict().may_launch());
    }

    #[test]
    fn r1651_the_form_is_the_deployable_document_nested_by_its_own_paths() {
        let document = inspector()
            .document()
            .expect("the opening values are sound");
        assert_eq!(
            document,
            json!({
                "id": "a1",
                "listen": { "endpoints": ["t/0.0:1"] },
                "connect": { "endpoints": ["t/2.1:3"] },
                "control": { "permissions": ["read", "write"] },
                "transport": { "link": { "tx": { "batch_size": 65535 } } },
            }),
            "the dotted key IS the path, and the declared shape IS the type"
        );
    }

    #[test]
    fn r1651_a_document_round_trips_through_the_form_it_came_from() {
        // ★ The law that makes one mapping worth having where the toolkit has
        // two: a form and a settings file mapped by hand are mapped twice, and
        // nothing checks the two agree.
        let mut form = inspector();
        let out = form.document().expect("sound");
        let adopted = form.adopt(&out);
        assert!(adopted.complete(), "{adopted:?}");
        assert_eq!(adopted.set.len(), 5, "every leaf landed on its row");
        assert_eq!(
            form.document().expect("still sound"),
            out,
            "document -> form -> document is identity"
        );
    }

    #[test]
    fn r1651_a_key_the_form_has_no_row_for_is_named_rather_than_dropped() {
        // A configuration written by a newer target is the normal case. A form
        // that swallowed the keys it did not know would let a person save the
        // file back with those settings gone.
        let mut form = inspector();
        let adopted = form.adopt(&json!({
            "id": "a2",
            "transport": { "link": { "tx": { "batch_size": 4096 } } },
            "scheduling": { "queue_depth": 8 },
        }));
        assert_eq!(adopted.set, vec!["id", "transport.link.tx.batch_size"]);
        assert_eq!(
            adopted.unplaceable,
            vec!["scheduling.queue_depth"],
            "named, and the caller decides what to do about it"
        );
        assert!(!adopted.complete());
        assert_eq!(form.field("id").expect("held").value(), "a2");
        assert_eq!(
            form.field("transport.link.tx.batch_size")
                .expect("held")
                .value(),
            "4096",
            "and a number came back as the text the row edits"
        );
    }

    #[test]
    fn r1651_a_value_a_row_cannot_hold_is_refused_by_name_not_written() {
        let mut form = inspector();
        let adopted =
            form.adopt(&json!({ "transport": { "link": { "tx": { "batch_size": "big" }}}}));
        assert!(adopted.set.is_empty());
        assert_eq!(adopted.refused.len(), 1);
        assert_eq!(adopted.refused[0].wire(), "wrong_type");
        assert_eq!(
            form.field("transport.link.tx.batch_size")
                .expect("held")
                .value(),
            "65535",
            "★ and the row still holds what it held — a refused document must \
             not half-apply"
        );
    }

    #[test]
    fn r1651_adopting_rewrites_a_list_in_the_canonical_spelling() {
        // Stated rather than hidden: the normalisation IS the point. The
        // document is the single spelling of the value, so a round trip is
        // also a formatter.
        let mut form = inspector();
        form.set("control.permissions", "write,read").expect("held");
        assert_eq!(
            form.document().expect("sound")["control"]["permissions"],
            json!(["write", "read"]),
            "order is the author's, and a set is written as it was typed"
        );
        let out = form.document().expect("sound");
        form.adopt(&out);
        assert_eq!(
            form.field("control.permissions").expect("held").value(),
            "write, read",
            "the separator is canonical after a round trip; the order is not \
             touched"
        );
    }

    #[test]
    fn r1651_a_repeat_in_a_set_is_a_defect_and_an_unoffered_word_is_out_of_range() {
        let mut form = inspector();
        form.set("control.permissions", "read, read").expect("held");
        assert_eq!(
            form.defects()[0].wire(),
            "wrong_type",
            "a set has no repeats"
        );

        form.set("control.permissions", "read, execute")
            .expect("held");
        let out_of_range = &form.defects()[0];
        assert_eq!(out_of_range.wire(), "out_of_range");
        assert!(
            out_of_range.sentence().contains("read, write"),
            "and it lists what is on offer: {}",
            out_of_range.sentence()
        );
    }

    #[test]
    fn r1651_a_document_is_refused_when_a_value_would_fail_at_start_up() {
        let mut form = inspector();
        form.set("transport.link.tx.batch_size", "70000")
            .expect("held");
        let refused = form.document().expect_err("a blocking value stops it");
        let DocumentError::Defective(defects) = &refused else {
            panic!("{refused:?}");
        };
        assert_eq!(defects.len(), 1);
        assert!(refused.to_string().contains("start-up"), "{refused}",);

        // A WARNING does not stop it: the target is the thing that decides
        // whether it knows a key, so an unknown key is written out.
        let custom = ConfigForm::new(
            vec![ConfigField::new("plugins.name", "text", Applies::Restart, "stats").as_custom()],
            vec![],
        );
        assert_eq!(
            custom.document().expect("a warning is not a refusal"),
            json!({ "plugins": { "name": "stats" } })
        );
    }

    #[test]
    fn r1651_a_key_that_is_a_prefix_of_another_cannot_be_placed() {
        // `a` and `a.b` cannot both be values in one document. Dropping either
        // is how a configuration loses a setting nobody deleted.
        let form = ConfigForm::new(
            vec![
                ConfigField::new("transport", "text", Applies::Restart, "auto"),
                ConfigField::new("transport.mtu", "int", Applies::Restart, "1500")
                    .with_shape(FieldType::Integer { min: 1, max: 9000 }),
            ],
            vec![],
        );
        let refused = form.document().expect_err("one path, two meanings");
        assert_eq!(
            refused,
            DocumentError::PathCollision {
                key: "transport.mtu".to_string(),
                at: "transport".to_string(),
            }
        );
    }

    /// ★★★ R1687 — **the write half is total now, like the read half always
    /// was.**
    ///
    /// A form holding one unparseable row still describes forty good ones, and
    /// before this the only way to ask for them was `document`, which answers
    /// nothing at all. This is the shape [`Adopted`] has had all along in the
    /// other direction.
    #[test]
    fn r1687_a_form_composes_what_it_can_and_names_what_it_cannot() {
        let mut form = inspector();
        form.set("transport.link.tx.batch_size", "70000")
            .expect("held");

        let composed = form.compose();
        assert!(!composed.complete(), "one row cannot be carried");
        assert_eq!(composed.unexpressed.len(), 1);
        let row = &composed.unexpressed[0];
        assert_eq!(row.key, "transport.link.tx.batch_size");
        assert_eq!(row.shown, "70000", "the value AS THE ROW SHOWS IT");
        assert_eq!(
            row.why,
            Unexpressible::Defective(ConfigDefect::OutOfRange {
                key: "transport.link.tx.batch_size".to_string(),
                allowed: "0..=65535".to_string(),
            })
        );
        assert!(
            row.why.sentence().contains("outside"),
            "{}",
            row.why.sentence()
        );

        // ★ The other six rows are still there — which is the whole reason this
        // exists. `document` would have handed back an error and nothing else.
        let carried = composed.document.as_object().expect("an object");
        assert!(carried.contains_key("id"), "{carried:?}");
        assert!(carried.contains_key("listen"), "{carried:?}");
        assert!(
            composed.document.pointer("/transport/link/tx").is_none(),
            "and the refused row left NOTHING behind: {carried:?}"
        );
    }

    /// ★★★★★ R1687 — **the two halves cannot disagree**, because one is the
    /// other narrowed.
    ///
    /// This is the assertion that makes deriving `document` from `compose`
    /// worth anything: over a spread of forms — clean, warned, wrong-typed,
    /// out-of-range, colliding — the answer to "is this shippable" is exactly
    /// "did every row reach the document". A `compose` that quietly swallowed a
    /// row would pass its own test above and fail here.
    ///
    /// ★ The warned form is the interesting one: an unknown key is a defect and
    /// is still **carried**, so a `compose` that treated every defect as
    /// unexpressed would make `document` start refusing a form it has always
    /// accepted.
    ///
    /// ★★★★★ **The biconditional alone is not enough, and a counterfactual is
    /// what said so.** `document` is DERIVED from `compose`, so a `compose`
    /// that silently swallowed a defective row moves both sides together: the
    /// row vanishes, `unexpressed` is empty, `complete()` is true, `document`
    /// answers `Ok`, and the two agree — on the wrong thing. The check read
    /// like coverage and was measuring a function against itself, which is the
    /// class R1681.1, R1682 and R1684.1 each met in a different place.
    ///
    /// So the third assertion below is the load-bearing one: the unexpressed
    /// rows are compared against [`ConfigForm::defects`], which reaches the
    /// fields by its own path and knows nothing about the walk. A row that
    /// falls out of the document without being named now has somewhere to fail.
    #[test]
    fn r1687_a_form_ships_exactly_when_every_row_reached_the_document() {
        let wrong = {
            let mut form = inspector();
            // A repeat in a SET — `Flags` refuses it as a wrong type rather
            // than as a range, so the two blocking arms are both covered here.
            form.set("control.permissions", "read, read").expect("held");
            form
        };
        let out_of_range = {
            let mut form = inspector();
            form.set("transport.link.tx.batch_size", "70000")
                .expect("held");
            form
        };
        let warned = ConfigForm::new(
            vec![ConfigField::new("plugins.name", "text", Applies::Restart, "stats").as_custom()],
            vec![],
        );
        let colliding = ConfigForm::new(
            vec![
                ConfigField::new("transport", "text", Applies::Restart, "auto"),
                ConfigField::new("transport.mtu", "int", Applies::Restart, "1500")
                    .with_shape(FieldType::Integer { min: 1, max: 9000 }),
            ],
            vec![],
        );

        for (name, form) in [
            ("clean", inspector()),
            ("warned", warned),
            ("wrong-typed", wrong),
            ("out-of-range", out_of_range),
            ("colliding", colliding),
        ] {
            let composed = form.compose();
            let shipped = form.document();
            assert_eq!(
                shipped.is_ok(),
                composed.complete(),
                "{name}: document() and compose() disagree — {shipped:?} against {:?}",
                composed.unexpressed
            );
            if let Ok(document) = shipped {
                assert_eq!(
                    document, composed.document,
                    "{name}: and when it ships, it ships the SAME document"
                );
            }

            // ★★★★★ The independent path. `defects()` walks the fields itself
            // and has nothing to do with the composing walk, so a `compose`
            // that drops a row without naming it fails HERE even though both
            // sides of the biconditional above moved together.
            let mut blocking: Vec<String> = form
                .defects()
                .iter()
                .filter(|defect| defect.blocks())
                .map(|defect| defect.key().to_string())
                .collect();
            let mut named: Vec<String> = composed
                .unexpressed
                .iter()
                .filter(|row| matches!(row.why, Unexpressible::Defective(_)))
                .map(|row| row.key.clone())
                .collect();
            blocking.sort_unstable();
            named.sort_unstable();
            assert_eq!(
                named, blocking,
                "{name}: every row that blocks a launch is a row the document \
                 could not carry, and every row the document could not carry \
                 for a VALUE reason blocks — these are two derivations of one \
                 fact and nothing may fall between them"
            );
        }
    }

    /// ★★ R1687 — a collision names the row that could not be placed **and the
    /// prefix that took its path**, which is the half a person can act on.
    ///
    /// Two rows are involved and only one of them is at fault-by-position, so a
    /// report naming just the key would leave a reader hunting for the other.
    #[test]
    fn r1687_a_collision_names_the_prefix_that_took_the_path() {
        let form = ConfigForm::new(
            vec![
                ConfigField::new("transport", "text", Applies::Restart, "auto"),
                ConfigField::new("transport.mtu", "int", Applies::Restart, "1500")
                    .with_shape(FieldType::Integer { min: 1, max: 9000 }),
            ],
            vec![],
        );
        let composed = form.compose();
        assert_eq!(composed.unexpressed.len(), 1);
        assert_eq!(composed.unexpressed[0].key, "transport.mtu");
        assert_eq!(
            composed.unexpressed[0].why,
            Unexpressible::Collides {
                at: "transport".to_string()
            }
        );
        // The row that got there first is carried, so the document is not empty
        // — a caller shipping this gets the setting that could be expressed.
        assert_eq!(composed.document, json!({ "transport": "auto" }));
    }

    #[test]
    fn r1651_a_hidden_row_keeps_its_place_its_value_and_its_defect() {
        // The distinction the mature toolkit's form layout draws with its
        // row-visibility control — measured there: hiding the middle of three
        // rows took the form from 159 px to 104 px and left the row count at 3.
        // A gate that skipped hidden rows would let a value block a start-up
        // with nothing on screen saying which.
        let form = ConfigForm::new(
            vec![
                ConfigField::new("id", "text", Applies::Restart, "a1"),
                ConfigField::new(
                    "transport.link.tx.batch_size",
                    "int",
                    Applies::Restart,
                    "70000",
                )
                .with_shape(FieldType::Integer { min: 0, max: 65535 })
                .with_hidden(true),
            ],
            vec![],
        );
        assert_eq!(form.fields().len(), 2, "hidden is not removed");
        assert!(form.fields()[1].hidden());
        assert_eq!(
            form.defects().len(),
            1,
            "★ and the launch gate still sees it"
        );
        assert!(!form.verdict().may_launch());
    }

    /// ★★ R1678 — a form says whether it differs from what it opened as, and
    /// puts itself back.
    ///
    /// The three ways it can differ are asserted SEPARATELY, because a
    /// value-only baseline answers the first and silently misses the other two
    /// — and that is exactly the state this type was in: `ConfigField::edited`
    /// has existed since R1651 and a row ADDED after the form opened is not
    /// edited, while a row REMOVED is not there to be asked.
    #[test]
    fn r1678_a_form_says_whether_it_differs_from_what_it_opened_as() {
        let opened = form();
        assert!(
            !opened.edited(),
            "a form as it opens has nothing to put back"
        );

        // (1) a value.
        let mut changed = form();
        changed.set("connect.endpoints", "t/9:9").expect("hot row");
        assert!(changed.edited(), "an edited value is a difference");
        changed.revert();
        assert_eq!(changed, opened, "and reverting restores it exactly");
        assert!(!changed.edited());

        // (2) a row that was added — NOT edited, and still a difference.
        let mut grown = form();
        grown.add("timestamping").expect("offered");
        assert!(
            !grown.fields().iter().any(ConfigField::edited),
            "★ the added row's value IS its original, so no field is edited — \
             this is the case a value-only baseline cannot see"
        );
        assert!(grown.edited(), "★ and it is a difference all the same");
        grown.revert();
        assert_eq!(grown, opened, "the row goes back to being offered");

        // (3) a row that was taken away.
        let mut shrunk = form();
        shrunk.remove("connect.endpoints").expect("held");
        assert!(shrunk.edited(), "a removed row is a difference");
        shrunk.revert();
        assert_eq!(
            shrunk, opened,
            "★ including its ORDER — a set-based repair would put it back last"
        );

        // Composed, because a session is not one edit: every kind at once.
        let mut all = form();
        all.set("id", "a9").expect("held");
        all.add("timestamping").expect("offered");
        all.remove("listen.endpoints").expect("held");
        assert!(all.edited());
        all.revert();
        assert_eq!(all, opened, "one revert undoes all three kinds together");

        // A launch accepts the shape as well as the values, so a form settled
        // after growing a row reports nothing to put back — and reverting then
        // keeps the new row rather than dropping it.
        let mut settled = form();
        settled.add("timestamping").expect("offered");
        settled.settle();
        assert!(!settled.edited(), "★ a launch accepts the shape too");
        let after = settled.clone();
        settled.revert();
        assert_eq!(settled, after, "and the settled shape is what revert keeps");
    }

    /// A form shaped like the one the behaviour canon shows: rows somebody
    /// wrote, a row the screen works out from the node's role, and a row that
    /// is about where the node runs rather than about its configuration.
    fn with_derived() -> ConfigForm {
        let mut form = form();
        form.upsert(
            ConfigField::new("mode", "mode", Applies::Restart, "peer")
                .with_shape(FieldType::Choice {
                    of: vec!["peer".into(), "client".into(), "router".into()],
                })
                .derived_from("role"),
        );
        form.upsert(
            ConfigField::new("host", "text", Applies::Restart, "127.0.0.1")
                .derived_from("kind default")
                .goes_aside("placement"),
        );
        form
    }

    /// ★★★★★ R1716 — the fact is WHERE IT CAME FROM, not a flag.
    #[test]
    fn r1716_a_row_says_what_worked_its_value_out() {
        let form = with_derived();
        let mode = form.field("mode").expect("held");
        assert_eq!(mode.source().derived_from(), Some("role"));
        assert!(!mode.source().writable());
        assert_eq!(
            form.field("id").expect("held").source().derived_from(),
            None,
            "and a row somebody wrote names nothing, because nobody worked it out"
        );
        let worked_out: Vec<&str> = form.derived().iter().map(|f| f.key()).collect();
        assert_eq!(
            worked_out,
            ["mode", "host"],
            "★ the rollup the floor has no shape for: it answers one value at a \
             time and cannot gather them"
        );
    }

    /// ★★★★★ R1716 — the refusal is the VALUE'S, which is the half the floor
    /// leaves to a view: measured at 6.11, writing into a cell whose editing
    /// had been cleared returned success and changed the value.
    #[test]
    fn r1716_a_derived_row_refuses_the_write_the_floor_would_have_taken() {
        let mut form = with_derived();
        assert_eq!(
            form.set("mode", "router"),
            Err(FormError::Derived {
                key: "mode".to_owned(),
                from: "role".to_owned(),
            }),
            "★ and the refusal NAMES the source, so a person knows where to go"
        );
        assert_eq!(
            form.field("mode").expect("held").value(),
            "peer",
            "★ the value did not move — the floor's did"
        );
        assert!(
            FormError::Derived {
                key: "mode".to_owned(),
                from: "role".to_owned(),
            }
            .to_string()
            .contains("role"),
            "and the sentence a person reads carries it too"
        );
    }

    /// ★★ R1716 — nor may it be taken away: the derivation is still true, so
    /// the row would be back one render later.
    #[test]
    fn r1716_a_derived_row_cannot_be_taken_away() {
        let mut form = with_derived();
        assert_eq!(
            form.remove("mode"),
            Err(FormError::Derived {
                key: "mode".to_owned(),
                from: "role".to_owned(),
            })
        );
        assert!(form.field("mode").is_some(), "the row is still there");
        assert_eq!(form.remove("id"), Ok(()), "an authored row still goes");
    }

    /// ★★★★★ R1716 — the act the floor performs silently, announced.
    #[test]
    fn r1716_taking_a_row_over_says_what_it_displaced() {
        let mut form = with_derived();
        let took = form.author("mode").expect("derived");
        assert_eq!(
            took,
            Takeover {
                key: "mode".to_owned(),
                was: "role".to_owned(),
                seeded: "peer".to_owned(),
            },
            "★ which row, what it came from, and what it was holding — the \
             three things the floor drops"
        );
        assert!(
            took.sentence().contains("role"),
            "and the one line a person reads names the source"
        );
        assert_eq!(
            form.field("mode").expect("held").value(),
            "peer",
            "★ taking a value over starts from what it WAS, never from empty"
        );
        assert_eq!(form.set("mode", "router"), Ok(()), "and now it is theirs");
        assert_eq!(
            form.author("mode"),
            Err(FormError::NotDerived("mode".to_owned())),
            "★ a second take-over has nothing to take"
        );
        assert_eq!(form.remove("mode"), Ok(()), "and it can be taken away now");
    }

    /// ★★★ R1716 — derived and aside are different axes, and the document is
    /// where they cross.
    #[test]
    fn r1716_a_derived_row_is_configuration_and_an_aside_row_is_not() {
        let composed = with_derived().compose();
        let document = composed.document.as_object().expect("object");
        assert_eq!(
            document.get("mode").and_then(Value::as_str),
            Some("peer"),
            "★ a value the screen worked out is still configuration"
        );
        assert!(
            !document.contains_key("host"),
            "★ and a row about where the node runs is not, so it is not in the file"
        );
        assert_eq!(
            composed.aside,
            vec![Aside {
                key: "host".to_owned(),
                shown: "127.0.0.1".to_owned(),
                instead: "placement".to_owned(),
            }],
            "★ named rather than dropped, and with a word for what it IS"
        );
        assert!(
            composed.unexpressed.is_empty() && composed.complete(),
            "★★ 'does not belong' is not 'did not fit' — a healthy form must \
             not read as a broken one"
        );
    }

    /// ★★ R1716 — an unknown-key warning is about a configuration path, and a
    /// row that goes aside is not one.
    #[test]
    fn r1716_an_aside_row_is_not_warned_about_as_a_key_the_target_lacks() {
        let mut form = form();
        form.upsert(
            ConfigField::new("host", "text", Applies::Restart, "10.0.0.2")
                .as_custom()
                .goes_aside("placement"),
        );
        assert_eq!(
            form.defects(),
            vec![],
            "★ the target was never going to be shown this key"
        );
        let mut warned = form.clone();
        warned.upsert(ConfigField::new("host", "text", Applies::Restart, "10.0.0.2").as_custom());
        assert_eq!(
            warned.defects(),
            vec![ConfigDefect::UnknownKey {
                key: "host".to_owned()
            }],
            "★ the same row headed for the document IS warned about — one \
             counterfactual apart"
        );
    }

    /// ★★ R1716 — a file does not get to seize a row from its source.
    #[test]
    fn r1716_adopting_a_document_does_not_seize_a_derived_row() {
        let mut form = with_derived();
        let adopted = form.adopt(&json!({ "mode": "router", "id": "b2" }));
        assert_eq!(adopted.derived, ["mode"], "★ reported, never written");
        assert_eq!(adopted.set, ["id"], "and the authored row did take it");
        assert_eq!(
            form.field("mode").expect("held").value(),
            "peer",
            "★ the screen still shows what the role implies"
        );
        assert!(
            !adopted.complete(),
            "★★ and the load is NOT complete — a reader told otherwise would \
             believe the file's mode was in front of them"
        );
    }

    /// ★★★ R1717 — the stored form carries the two HALVES and never the
    /// answer they add up to.
    ///
    /// R1716 stored `source` beside the value, which meant a form could be read
    /// back saying it was authored while holding a derivation. Here `source` is
    /// computed, so it is not in the stored form at all and cannot disagree
    /// with what is: a row nobody derived writes no derivation, and one that
    /// was derived writes what it was worked out from and to.
    #[test]
    fn r1717_a_stored_form_carries_the_halves_and_not_the_answer() {
        let stored = serde_json::to_string(&form()).expect("serialize");
        assert!(
            !stored.contains("source") && !stored.contains("goes"),
            "★ the answer is not written down at all: {stored}"
        );
        assert!(
            !stored.contains("worked_out"),
            "★ nor is a derivation nobody has: {stored}"
        );
        let mut derived = form();
        derived.upsert(
            ConfigField::new("mode", "mode", Applies::Restart, "peer").derived_from("role"),
        );
        let text = serde_json::to_string(&derived).expect("serialize");
        assert!(
            text.contains(r#""worked_out":{"from":"role","value":"peer"}"#),
            "and a row that WAS worked out carries both halves of that: {text}"
        );
        let mut back: ConfigForm = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, derived, "★ and it survives the round trip");
        assert_eq!(
            back.set("mode", "router"),
            Err(FormError::Derived {
                key: "mode".to_owned(),
                from: "role".to_owned(),
            }),
            "★★ including the refusal — a form read back off a wire is not a \
             form that forgot who owns its values"
        );
    }

    /// A row a person and the screen both contribute to: two addresses written,
    /// three worked out from the wires, one of them already named.
    fn shared() -> ConfigField {
        ConfigField::new(
            "connect.endpoints",
            "address[]",
            Applies::Hot,
            "t/mine:1, t/9:9",
        )
        .with_shape(FieldType::List {
            of: Box::new(FieldType::Text),
        })
        .with_derived("wire", "t/a:1, t/9:9, t/b:2")
        .expect("a list holds two contributions")
    }

    /// ★★★★★ R1717 — one key, two contributors, and the row shows their
    /// composition: what somebody wrote first, then what the canvas worked out
    /// and they had not already said.
    #[test]
    fn r1717_a_row_composes_what_was_written_with_what_was_worked_out() {
        let row = shared();
        assert_eq!(
            row.value(),
            "t/mine:1, t/9:9, t/a:1, t/b:2",
            "★ written first, worked-out after, and the address they both name once"
        );
        assert_eq!(row.written(), Some("t/mine:1, t/9:9"));
        assert_eq!(row.worked_out().map(Derivation::from), Some("wire"));
        assert_eq!(row.source(), Source::Shared("wire".into()));
        assert_eq!(row.source().wire(), "shared");
        assert!(
            row.source().writable(),
            "★★ and it is still theirs to type in — the half that is not \
             theirs does not take the row away from them"
        );
        assert_eq!(
            row.derived_elements(),
            2,
            "★★ two of the four came from the canvas; the third worked-out \
             address was already written down and is not counted twice"
        );
    }

    /// ★★★★★ R1717 — a shape that holds ONE value refuses to hold two
    /// contributions, and the refusal is the type's rather than a screen's.
    #[test]
    fn r1717_a_single_valued_shape_refuses_two_contributions() {
        let scalar = ConfigField::new("mode", "mode", Applies::Restart, "peer").with_shape(
            FieldType::Choice {
                of: vec!["peer".into(), "router".into()],
            },
        );
        assert!(!FieldType::Boolean.merges());
        assert!(
            !FieldType::Choice {
                of: vec!["peer".into()]
            }
            .merges()
        );
        assert!(
            FieldType::List {
                of: Box::new(FieldType::Text)
            }
            .merges()
        );
        // ★★ A set COULD hold two contributions, and does not, because its
        // members are drawn as chips with nowhere to say which chip is the
        // screen's — a shared one would paint a chosen chip a person could
        // press and silently adopt. The rule is about what can be shown.
        assert!(
            !FieldType::Flags {
                of: vec!["read".into()]
            }
            .merges()
        );
        assert_eq!(
            ConfigField::new("control.permissions", "perm", Applies::Restart, "read")
                .with_shape(FieldType::Flags {
                    of: vec!["read".into(), "write".into()],
                })
                .with_derived("role", "write"),
            Err(FormError::Unmergeable {
                key: "control.permissions".to_owned(),
                ty: "perm".to_owned(),
            }),
            "★★★ and the refusal is the type's, so no screen can reach that state"
        );
        assert_eq!(
            scalar.with_derived("role", "router"),
            Err(FormError::Unmergeable {
                key: "mode".to_owned(),
                ty: "mode".to_owned(),
            }),
            "★ two answers to one mode contradict; they do not compose"
        );
    }

    /// ★★★★★ R1717 — `edited` is a question about the half somebody WROTE.
    ///
    /// The failure it prevents: a row whose shown value moved because the
    /// canvas moved was not edited by anybody, and a form that said it was
    /// would offer a "put it back" that puts nothing back.
    #[test]
    fn r1717_a_moving_derivation_is_not_somebody_editing() {
        let row = shared();
        assert!(!row.edited(), "★ nobody has typed anything yet");
        let moved = ConfigField::new(
            "connect.endpoints",
            "address[]",
            Applies::Hot,
            "t/mine:1, t/9:9",
        )
        .with_shape(FieldType::List {
            of: Box::new(FieldType::Text),
        })
        .with_derived("wire", "t/a:1, t/9:9, t/b:2, t/c:3")
        .expect("a list holds two contributions");
        assert_ne!(moved.value(), row.value(), "★ the canvas drew another link");
        assert!(
            !moved.edited(),
            "★★★ and that is still not an edit — the written half did not move"
        );
        let mut typed = shared();
        typed
            .set("t/mine:1, t/9:9, t/mine:2")
            .expect("theirs to write");
        assert!(typed.edited(), "★ this is");
        assert_eq!(
            typed.value(),
            "t/mine:1, t/9:9, t/mine:2, t/a:1, t/b:2",
            "★★ and the canvas still reaches the row afterwards — a keystroke \
             does not freeze the drawing into somebody's configuration"
        );
    }

    /// ★★★★★ R1717 — what a caller STORES is the written half.
    #[test]
    fn r1717_the_row_a_caller_stores_is_the_written_half() {
        let mine = shared().written_row().expect("somebody wrote some of it");
        assert_eq!(mine.value(), "t/mine:1, t/9:9");
        assert_eq!(mine.source(), Source::Authored);
        assert_eq!(
            mine.derived_elements(),
            0,
            "★ a stored row has nothing worked out in it"
        );
        let worked_out =
            ConfigField::new("mode", "mode", Applies::Restart, "peer").derived_from("role");
        assert_eq!(
            worked_out.written_row(),
            None,
            "★★ and a row nobody wrote stores NOTHING — an emptied row would \
             put a key in the file that somebody would then own"
        );
    }

    /// ★★★★★ R1717 — emptying the written half gives the row back, however a
    /// person reaches it.
    #[test]
    fn r1717_an_emptied_written_half_gives_the_row_back() {
        let mut typed = shared();
        typed.set("  ,  ").expect("theirs to write");
        assert_eq!(
            typed.source(),
            Source::Derived("wire".into()),
            "★ they emptied their half, so the row is the canvas's again"
        );
        assert_eq!(typed.value(), "t/a:1, t/9:9, t/b:2");
        assert_eq!(
            typed.set("anything"),
            Err(FormError::Derived {
                key: "connect.endpoints".to_owned(),
                from: "wire".to_owned(),
            }),
            "★★ and the row refuses them the way any worked-out row does"
        );
        let mut seat = shared();
        seat.disown().expect("something is deriving it");
        assert_eq!(seat.source(), Source::Derived("wire".into()));
        assert_eq!(
            seat.value(),
            "t/a:1, t/9:9, t/b:2",
            "★★★ the same state by the other door — one act, one rule"
        );
        let mut nothing_derives_it = ConfigField::new("id", "text", Applies::Restart, "a1");
        assert_eq!(
            nothing_derives_it.disown(),
            Err(FormError::NotDerived("id".to_owned())),
            "★ and a row nobody derives has nobody to give back to"
        );
    }

    /// ★★★★★ R1717 — `remove` on a shared row takes the written half out and
    /// LEAVES the row, because the derivation is still true.
    #[test]
    fn r1717_removing_a_shared_row_gives_it_back_rather_than_taking_it_away() {
        let mut form = ConfigForm::new(vec![shared()], Vec::new());
        form.remove("connect.endpoints").expect("their half goes");
        let row = form
            .field("connect.endpoints")
            .expect("★ the row is still on the screen — the canvas still draws it");
        assert_eq!(row.source(), Source::Derived("wire".into()));
        assert_eq!(row.value(), "t/a:1, t/9:9, t/b:2");
        assert_eq!(
            form.remove("connect.endpoints"),
            Err(FormError::Derived {
                key: "connect.endpoints".to_owned(),
                from: "wire".to_owned(),
            }),
            "★★ and a second press has nothing left to take"
        );
    }

    /// ★★★★ R1717 — taking a shared row over adopts what the canvas said,
    /// and says so.
    #[test]
    fn r1717_taking_a_shared_row_over_adopts_the_whole_composition() {
        let mut form = ConfigForm::new(vec![shared()], Vec::new());
        let took = form
            .author("connect.endpoints")
            .expect("something derives it");
        assert_eq!(
            took,
            Takeover {
                key: "connect.endpoints".to_owned(),
                was: "wire".to_owned(),
                seeded: "t/mine:1, t/9:9, t/a:1, t/b:2".to_owned(),
            },
            "★ it starts from what was on the screen, never from their half alone"
        );
        let row = form.field("connect.endpoints").expect("held");
        assert_eq!(row.source(), Source::Authored);
        assert_eq!(row.written(), Some("t/mine:1, t/9:9, t/a:1, t/b:2"));
        assert!(
            !row.edited(),
            "★★ and taking a row over is not typing in it — nothing is pending \
             a restart because of it"
        );
        assert_eq!(
            form.shared().len(),
            0,
            "★ the rollup follows: nothing is shared any more"
        );
    }

    /// ★★★ R1717 — the two rollups answer different questions, so a row can
    /// be in both.
    #[test]
    fn r1717_shared_rows_are_derived_rows_and_are_their_own_rollup() {
        let form = ConfigForm::new(
            vec![
                ConfigField::new("id", "text", Applies::Restart, "a1"),
                ConfigField::new("mode", "mode", Applies::Restart, "peer").derived_from("role"),
                shared(),
            ],
            Vec::new(),
        );
        let derived: Vec<&str> = form.derived().iter().map(|f| f.key()).collect();
        assert_eq!(
            derived,
            ["mode", "connect.endpoints"],
            "★ 'which of these does the screen keep up to date' — both"
        );
        let shared_rows: Vec<&str> = form.shared().iter().map(|f| f.key()).collect();
        assert_eq!(
            shared_rows,
            ["connect.endpoints"],
            "★★ 'which of these will change under somebody who thinks they own \
             it' — only the one they can type in"
        );
        assert_eq!(Source::WORDS.len(), 3);
        let mut seen = std::collections::BTreeSet::new();
        for source in [
            Source::Authored,
            Source::Derived("role".into()),
            Source::Shared("wire".into()),
        ] {
            assert!(seen.insert(source.wire()), "{} spells two", source.wire());
        }
    }

    /// ★★★★★ R1717 — provenance reaches the ELEMENT, because editing does.
    ///
    /// The failure it prevents was measured the first time a screen was driven
    /// into this state: an edit over a line the canvas contributed wrote the
    /// canvas's whole contribution into somebody's half and reordered its
    /// neighbours in the same act.
    #[test]
    fn r1717_an_element_says_which_half_it_came_from() {
        let row = shared();
        assert_eq!(
            row.value(),
            "t/mine:1, t/9:9, t/a:1, t/b:2",
            "two written, then the two worked out that were not already said"
        );
        assert_eq!(row.element_source(0), Source::Authored);
        assert_eq!(
            row.element_source(1),
            Source::Authored,
            "★ an address the canvas ALSO names is still theirs — they wrote it, \
             and its place in the row is the place their half put it"
        );
        assert_eq!(row.element_source(2), Source::Derived("wire".into()));
        assert_eq!(row.element_source(3), Source::Derived("wire".into()));
        assert_eq!(
            row.element_source(99),
            Source::Derived("wire".into()),
            "★ past the end there is nothing to have come from anywhere, and \
             answering for the row is the honest fallback"
        );
        let mine = ConfigField::new("id", "text", Applies::Restart, "a1");
        assert_eq!(
            mine.element_source(0),
            Source::Authored,
            "★★ a row with ONE contributor answers the same for every element \
             as it does for itself"
        );
        let theirs =
            ConfigField::new("mode", "mode", Applies::Restart, "peer").derived_from("role");
        assert_eq!(theirs.element_source(0), Source::Derived("role".into()));
    }

    /// ★★★★ R1717 — the document ships the composition, and a row that goes
    /// aside still does not.
    #[test]
    fn r1717_a_document_ships_both_contributions() {
        let form = ConfigForm::new(vec![shared()], Vec::new());
        assert_eq!(
            form.document().expect("shippable"),
            json!({
                "connect": {
                    "endpoints": ["t/mine:1", "t/9:9", "t/a:1", "t/b:2"]
                }
            }),
            "★ the picture and the configuration say the same thing, which is \
             the whole reason the composition exists"
        );
    }

    #[test]
    fn r1650_a_form_round_trips_through_its_wire_form() {
        let mut form = form();
        form.set("connect.endpoints", "t/7:7").expect("hot");
        let json = serde_json::to_string(&form).expect("serialize");
        let back: ConfigForm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, form);
        assert_eq!(
            back.field("connect.endpoints").expect("held").applies(),
            Applies::Hot,
            "including which fields reach a running node"
        );
    }
}
