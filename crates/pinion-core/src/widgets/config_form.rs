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
            (Self::Text | Self::Choice { .. }, Value::String(s)) => Ok(s.clone()),
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
    value: String,
    /// What the field was when the form was opened, so an edit is knowable.
    original: String,
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
            original: value.clone(),
            value,
            shape: FieldType::Text,
            custom: false,
            hidden: false,
        }
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
        self.shape.encode(&self.key, &self.value)
    }

    /// Every way this row is wrong, in the order a reader meets them.
    ///
    /// A custom key is reported **as well as** any shape defect, not instead of
    /// it: a hand-typed path holding an unparseable value is two separate
    /// pieces of news, and one of them blocks.
    #[must_use]
    pub fn defects(&self) -> Vec<ConfigDefect> {
        let mut found = Vec::new();
        if self.custom {
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

    /// The current value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether the value differs from what the form opened with.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.value != self.original
    }

    /// Set the value.
    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    /// Accept the current value as the settled one — what a successful launch
    /// does, after which nothing is pending a restart.
    pub fn settle(&mut self) {
        self.original.clone_from(&self.value);
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
    addable: Vec<ConfigField>,
}

impl ConfigForm {
    /// A form holding `fields`, offering `addable` as the keys not yet set.
    ///
    /// A key present in both is **kept only in `fields`**: a form that offered
    /// to add something it already has would let a user create a duplicate row,
    /// and two rows for one path is a configuration with no single value.
    #[must_use]
    pub fn new(fields: Vec<ConfigField>, addable: Vec<ConfigField>) -> Self {
        let mut form = Self {
            fields: Vec::new(),
            addable: Vec::new(),
        };
        for field in fields {
            form.upsert(field);
        }
        for candidate in addable {
            if form.field(candidate.key()).is_none()
                && !form.addable.iter().any(|f| f.key() == candidate.key())
            {
                form.addable.push(candidate);
            }
        }
        form
    }

    /// Put a field in, replacing any row at the same key.
    fn upsert(&mut self, field: ConfigField) {
        if let Some(at) = self.fields.iter().position(|f| f.key() == field.key()) {
            self.fields[at] = field;
        } else {
            self.fields.push(field);
        }
    }

    /// The rows the form shows, in order.
    #[must_use]
    pub fn fields(&self) -> &[ConfigField] {
        &self.fields
    }

    /// The keys this node could still be given.
    #[must_use]
    pub fn addable(&self) -> &[ConfigField] {
        &self.addable
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
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> Result<(), FormError> {
        let field = self
            .fields
            .iter_mut()
            .find(|f| f.key() == key)
            .ok_or_else(|| FormError::NoSuchField(key.to_string()))?;
        field.set(value);
        Ok(())
    }

    /// Move an offered key into the form.
    ///
    /// # Errors
    ///
    /// [`FormError::NotAddable`].
    pub fn add(&mut self, key: &str) -> Result<(), FormError> {
        let at = self
            .addable
            .iter()
            .position(|f| f.key() == key)
            .ok_or_else(|| FormError::NotAddable(key.to_string()))?;
        let field = self.addable.remove(at);
        self.upsert(field);
        Ok(())
    }

    /// Take a key back out, returning it to the offered set.
    ///
    /// # Errors
    ///
    /// [`FormError::NoSuchField`].
    pub fn remove(&mut self, key: &str) -> Result<(), FormError> {
        let at = self
            .fields
            .iter()
            .position(|f| f.key() == key)
            .ok_or_else(|| FormError::NoSuchField(key.to_string()))?;
        let field = self.fields.remove(at);
        self.addable.push(field);
        self.addable.sort_by(|a, b| a.key().cmp(b.key()));
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

    /// Accept every current value — what a successful launch does.
    pub fn settle(&mut self) {
        for field in &mut self.fields {
            field.settle();
        }
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
        let blocking: Vec<ConfigDefect> = self
            .defects()
            .into_iter()
            .filter(ConfigDefect::blocks)
            .collect();
        if !blocking.is_empty() {
            return Err(DocumentError::Defective(blocking));
        }
        let mut root = Map::new();
        for field in &self.fields {
            let encoded = field
                .encoded()
                .map_err(|d| DocumentError::Defective(vec![d]))?;
            Self::place(&mut root, field.key(), encoded)?;
        }
        Ok(Value::Object(root))
    }

    /// Put `value` at the dotted `key` inside `root`, creating the objects on
    /// the way.
    fn place(root: &mut Map<String, Value>, key: &str, value: Value) -> Result<(), DocumentError> {
        let mut here = root;
        let mut walked: Vec<&str> = Vec::new();
        let mut segments = key.split('.').peekable();
        while let Some(segment) = segments.next() {
            walked.push(segment);
            if segments.peek().is_none() {
                if here.contains_key(segment) {
                    return Err(DocumentError::PathCollision {
                        key: key.to_string(),
                        at: walked.join("."),
                    });
                }
                here.insert(segment.to_string(), value);
                return Ok(());
            }
            let next = here
                .entry(segment.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            here = next
                .as_object_mut()
                .ok_or_else(|| DocumentError::PathCollision {
                    key: key.to_string(),
                    at: walked.join("."),
                })?;
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
                Ok(text) => {
                    field.set(text);
                    adopted.set.push(path);
                }
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
}

impl Adopted {
    /// Whether every leaf of the document reached a row.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.unplaceable.is_empty() && self.refused.is_empty()
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
}

impl std::fmt::Display for FormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchField(key) => write!(f, "this node has no field {key}"),
            Self::NotAddable(key) => write!(f, "{key} is not a key this node kind offers"),
        }
    }
}

impl std::error::Error for FormError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Applies, ConfigDefect, ConfigField, ConfigForm, DocumentError, FieldType, FormError,
        Verdict,
    };

    /// The five rows the reference tool's node inspector shows, with the shapes
    /// they are declared with. Every arm of [`FieldType`] appears — which is
    /// the point: the vocabulary was chosen from what one real inspector needs,
    /// not from what a widget catalogue offers.
    fn inspector() -> ConfigForm {
        ConfigForm::new(
            vec![
                ConfigField::new("id", "text", Applies::Restart, "a1"),
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
