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
//! * [`ConfigForm::pending_restart`] — which **edited** fields need one. A form
//!   that reported every restart-scoped field would tell a user to restart for
//!   values they never touched.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

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
        }
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

    /// Whether any defect in `defects` stops a launch.
    ///
    /// Free-standing over a defect list rather than stored, so a form cannot
    /// report itself launchable while holding an error.
    #[must_use]
    pub fn verdict(defects: &[ConfigDefect]) -> Verdict {
        let blocking = defects.iter().filter(|d| d.blocks()).count();
        let warning = defects.len() - blocking;
        Verdict { blocking, warning }
    }
}

/// What a launch gate concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    blocking: usize,
    warning: usize,
}

impl Verdict {
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
    use super::{Applies, ConfigDefect, ConfigField, ConfigForm, FormError};

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
        let clean = ConfigForm::verdict(&[]);
        assert!(clean.may_launch() && clean.sentence().contains("launch is open"));

        let warned = ConfigForm::verdict(&[ConfigDefect::UnknownKey { key: "who".into() }]);
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

        let blocked = ConfigForm::verdict(&[
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
