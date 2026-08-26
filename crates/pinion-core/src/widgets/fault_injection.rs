//! R1853 §5.51 — **the faults a target's own settings admit**, derived from the
//! declaration and confirmed by running it.
//!
//! # Why this is a derivation and not a list
//!
//! A fault-injection panel needs to offer the faults a target can actually have.
//! Written down, that list is a second account of the declaration beside it — and
//! this workspace has paid for that shape four times (R1738, R1784, R1795, R1798,
//! each a correct gate over a CONSTANT population). The declaration already
//! decides the answer: a [`FieldType`] says what a value may be, and
//! [`FieldType::encode`] is the one place a text becomes a defect. So the
//! injectable set is read off the shapes, and every entry is checked by asking
//! `encode` whether it really produces the arm it claims.
//!
//! ⇒ **an entry that cannot be confirmed is not in the list.** The derivation is
//! not a promise about the declaration; it is a measurement of it.
//!
//! # ★★★★★ What the shapes make impossible
//!
//! The vocabulary has three arms and no field admits all three, which is the
//! point rather than an accident:
//!
//! | shape | wrong type | out of range |
//! |---|---|---|
//! | [`FieldType::Text`] | — every text is acceptable | — nothing is bounded |
//! | [`FieldType::Formatted`] | a text of the wrong shape | — |
//! | [`FieldType::Integer`] | a text that is not a number | a number past a bound |
//! | [`FieldType::Boolean`] | anything but the two words | — |
//! | [`FieldType::Choice`] | — every text is a candidate | a word outside the set |
//! | [`FieldType::Flags`] | a repeated word | a word outside the set |
//! | [`FieldType::List`] | whatever the element shape admits | |
//!
//! A free-text field therefore contributes NOTHING here, and that absence is a
//! fact about the target rather than a hole in this module: nothing a person can
//! type into it will stop the node coming up. The same reasoning is why an
//! unbounded integer is not a shape this vocabulary has — see
//! [`FieldType::Integer`]'s own note.
//!
//! # ★★★★★ And a fault the settings have that a FORM cannot inject
//!
//! [`DefectKind::UnknownKey`] is a real fault of the configuration — a path the
//! declaration does not contain, which the target warns about and starts anyway.
//! It is **not** offered here, and that is R1853's own measurement rather than a
//! scope decided in advance: [`ConfigForm::adopt`] reports a leaf the form has no
//! row for as *unplaceable* and does not take it, so a form structurally cannot
//! hold that fault. Offering it would be offering an act the panel cannot
//! perform.
//!
//! # What is NOT here, named rather than absent
//!
//! [`Scope`] has three arms because there are three different reasons, and a
//! consumer that merged them could not tell a fault it might one day reach from
//! one no declaration will ever describe: [`Scope::Settings`] is derivable,
//! [`Scope::Document`] is the settings' fault a form cannot reach, and
//! [`Scope::World`] — a link that drops, a peer that never answers, a clock that
//! runs backwards — is not derivable from any declaration at all.

use crate::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType, Source,
};

/// Which arm of [`ConfigDefect`], without the payload.
///
/// A separate type because an *offer* names a kind and a *report* carries the
/// evidence: `ConfigDefect` arms hold the key, the wanted type and the seen
/// value, none of which exists before the fault is injected. Using the report
/// type for the offer would mean inventing those strings to describe a fault
/// nobody has caused yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefectKind {
    /// A key the target does not know. It warns and starts anyway.
    UnknownKey,
    /// A known key holding a value of the wrong type. The target refuses to
    /// start.
    WrongType,
    /// A known key holding a value of the right type and out of bounds. The
    /// target refuses to start.
    OutOfRange,
}

impl DefectKind {
    /// Every arm, so a consumer enumerates rather than spelling three out.
    pub const ALL: [Self; 3] = [Self::UnknownKey, Self::WrongType, Self::OutOfRange];

    /// The wire spelling, which is [`ConfigDefect::wire`]'s — one vocabulary for
    /// the offer and the report, because a client that read them as two would
    /// have to correlate them.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::UnknownKey => "unknown_key",
            Self::WrongType => "wrong_type",
            Self::OutOfRange => "out_of_range",
        }
    }

    /// Parse a wire spelling back — the inverse of [`wire`](Self::wire).
    #[must_use]
    pub fn from_wire(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.wire() == word)
    }

    /// Whether a fault of this kind stops a launch.
    ///
    /// Delegates to [`ConfigDefect::blocks`] rather than restating it, so the
    /// offer and the report cannot disagree about what an injected fault will
    /// do.
    #[must_use]
    pub fn blocks(self) -> bool {
        self.witness().blocks()
    }

    /// Which kind a reported defect is.
    #[must_use]
    pub const fn of(defect: &ConfigDefect) -> Self {
        match defect {
            ConfigDefect::UnknownKey { .. } => Self::UnknownKey,
            ConfigDefect::WrongType { .. } => Self::WrongType,
            ConfigDefect::OutOfRange { .. } => Self::OutOfRange,
        }
    }

    /// A payload-free representative, for asking `ConfigDefect` a question that
    /// depends only on the arm.
    fn witness(self) -> ConfigDefect {
        match self {
            Self::UnknownKey => ConfigDefect::UnknownKey { key: String::new() },
            Self::WrongType => ConfigDefect::WrongType {
                key: String::new(),
                want: String::new(),
                got: String::new(),
            },
            Self::OutOfRange => ConfigDefect::OutOfRange {
                key: String::new(),
                allowed: String::new(),
            },
        }
    }
}

/// One fault a declaration admits, and a value that causes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    /// The configuration path the fault is injected at. For
    /// [`DefectKind::UnknownKey`] this is a path the declaration does NOT have.
    pub key: String,
    /// Which fault it is.
    pub kind: DefectKind,
    /// A value that causes it — the thing a panel writes into the row.
    pub value: String,
    /// Whether the running node keeps its old value until restarted.
    ///
    /// The field's own [`Applies`], carried here so a panel can say what an
    /// injection will actually do without asking a second surface. `None` for
    /// the form-level unknown key, which belongs to no field.
    pub applies: Option<Applies>,
    /// Which part of the declaration admits this fault, in words.
    ///
    /// Not decoration: it is the answer to *why is this offered and that one
    /// not*, and a panel that could not say it would be offering a list a reader
    /// has to take on trust.
    pub admitted_by: String,
}

impl Injection {
    /// Whether injecting this stops a launch.
    #[must_use]
    pub fn blocks(&self) -> bool {
        self.kind.blocks()
    }
}

/// Where a fault lives, and therefore whether a FORM can inject it.
///
/// ★★★★★ Three arms, and the middle one is the round's own measurement. A panel
/// offering configuration faults next to a tool whose subject is a network will
/// be read as claiming those are the faults there are — so the boundary is data
/// a consumer can show. An absence nobody names is indistinguishable from an
/// oversight, which is this project's most repeated finding and the reason
/// `Unavailable`, `Silence` and `Standing` all exist.
///
/// ⚠ The three are not one boundary with a count. They are three DIFFERENT
/// reasons, and a consumer that merged them could not tell a fault it might one
/// day inject from one no declaration will ever describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// A fault of the configuration, at a row the declaration HAS. Derivable
    /// here, because the shape says what a value may be and therefore what it
    /// may not.
    Settings,
    /// A fault of the configuration at a path the declaration does NOT have.
    ///
    /// ★★★★★ Measured at R1853 and it is why this arm exists: a
    /// [`ConfigForm`] holds declared rows, and
    /// [`ConfigForm::adopt`] reports a leaf it has no row for as
    /// *unplaceable* rather than taking it. So
    /// [`DefectKind::UnknownKey`] is a real fault of the settings that a form
    /// **cannot reach** — producing it means editing the document behind the
    /// form. Offering it on a panel would be offering an act the panel cannot
    /// perform.
    Document,
    /// A fault of the world — a link that drops, a peer that never answers, a
    /// message that arrives twice. **Not derivable from any declaration**, and
    /// out of scope by that fact rather than by a decision.
    World,
}

impl Scope {
    /// Every arm, so a consumer enumerates the boundary rather than assuming it.
    pub const ALL: [Self; 3] = [Self::Settings, Self::Document, Self::World];

    /// The wire spelling.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Document => "document",
            Self::World => "world",
        }
    }

    /// Whether a form can inject faults of this scope.
    #[must_use]
    pub const fn injectable(self) -> bool {
        matches!(self, Self::Settings)
    }

    /// Why, in a sentence a surface can show.
    #[must_use]
    pub const fn because(self) -> &'static str {
        match self {
            Self::Settings => {
                "the declaration says what a value may be, so what it may not be follows"
            }
            Self::Document => {
                "a form holds the rows the declaration has, and adopting a leaf it has \
                 no row for reports it unplaceable rather than taking it — so this fault \
                 is reached by editing the document, not by using the form"
            }
            Self::World => {
                "no configuration declares whether a link drops, so nothing here can \
                 derive it"
            }
        }
    }

    /// Which scope a fault kind belongs to.
    ///
    /// The map that makes the boundary checkable rather than described: every
    /// arm of [`DefectKind`] has a scope, and only one scope is injectable.
    #[must_use]
    pub const fn of(kind: DefectKind) -> Self {
        match kind {
            DefectKind::UnknownKey => Self::Document,
            DefectKind::WrongType | DefectKind::OutOfRange => Self::Settings,
        }
    }
}

/// Whether a fault can be injected at this row **at all** — the half of the
/// confirmation that is about the row rather than about the value.
///
/// ★★★★★ Measured at R1853, and it is the whole difference between an offer and
/// a claim. [`ConfigField::set`] refuses a row with no written half, so a row
/// that is *worked out* from another cannot receive a value: a fault offered
/// there is an act the offering surface cannot perform.
///
/// A *shared* row IS reachable: it has a written half, and writing it is the
/// same act that takes it over from its derivation.
///
/// ⚠ **Being hidden is deliberately not a reason.** It is tempting — a row the
/// screen does not show is a row nothing can point at — but
/// [`ConfigForm::verdict`] checks a hidden row like any other and says why:
/// hiding is a screen decision, and a value that blocks a start-up with nothing
/// on screen is exactly the case that rule exists for. So a fault at a hidden
/// row is real and performable, and whether to *show* it is the surface's
/// question rather than this one's.
#[must_use]
pub fn reachable(field: &ConfigField) -> bool {
    !matches!(field.source(), Source::Derived(_))
}

/// Every fault one field's declaration admits, each confirmed **twice**: that
/// [`FieldType::encode`] really produces that arm for the value, and that the
/// row can receive a value at all ([`reachable`]).
///
/// ⇒ An entry that does not confirm is not returned. The list is a measurement
/// of the declaration and not a claim about it.
#[must_use]
pub fn injectable_at(field: &ConfigField) -> Vec<Injection> {
    if !reachable(field) {
        return Vec::new();
    }
    let key = field.key().to_string();
    let shape = field.shape();
    let applies = Some(field.applies());
    candidates(shape)
        .into_iter()
        .filter_map(|(kind, value, admitted_by)| {
            // ★★★★★ THE CONFIRMATION. `encode` is the one place a text becomes a
            // defect, so asking it is asking the only authority there is — and a
            // candidate the shape was expected to admit but does not is dropped
            // rather than offered.
            let produced = shape.encode(&key, &value).err()?;
            (DefectKind::of(&produced) == kind).then(|| Injection {
                key: key.clone(),
                kind,
                value: value.clone(),
                applies,
                admitted_by: admitted_by.to_string(),
            })
        })
        .collect()
}

/// Every fault the form admits: the fields' own, plus the one that belongs to
/// the form.
///
/// Sorted by key and then kind, so two calls over one declaration give one
/// order — a panel whose rows moved between frames would be a panel nobody can
/// press.
#[must_use]
pub fn injectable(form: &ConfigForm) -> Vec<Injection> {
    // ★★★★★ R1853 — `DefectKind::UnknownKey` is NOT here, and the reason is a
    // measurement rather than a scope decision made in advance. This function's
    // first draft offered it as *the form's own* fault, on the reading that a
    // path the declaration does not contain belongs to the form rather than to
    // any row. Then `ConfigForm::adopt` was read: a leaf the form has no row for
    // is reported UNPLACEABLE and not taken. So a form cannot hold that fault,
    // and a panel offering it would offer an act the panel cannot perform.
    //
    // The fault is real and it is the settings'. What it is not is INJECTABLE
    // from here, and [`Scope::Document`] is where that distinction lives so a
    // surface can show it instead of leaving the absence to look like an
    // oversight.
    let mut out: Vec<Injection> = form.fields().iter().flat_map(injectable_at).collect();
    out.sort_by(|a, b| a.key.cmp(&b.key).then(a.kind.cmp(&b.kind)));
    out
}

/// The candidate faults a shape is expected to admit, before confirmation.
///
/// Each is `(kind, a value that should cause it, why the declaration admits
/// it)`. Deliberately generous — a candidate that does not confirm is dropped by
/// [`injectable_at`], so this function may reason about the shape and does not
/// have to be right about `encode`'s every branch.
fn candidates(shape: &FieldType) -> Vec<(DefectKind, String, String)> {
    match shape {
        // Nothing. Every text is acceptable, so no value a person can type will
        // keep this target from coming up — a fact about the target, not a gap.
        FieldType::Text => Vec::new(),
        FieldType::Formatted { of } => vec![(
            DefectKind::WrongType,
            "!".to_string(),
            format!("the text has to be {}", of.wanted()),
        )],
        FieldType::Integer { min, max } => {
            let mut out = vec![(
                DefectKind::WrongType,
                "not a number".to_string(),
                "the value has to be a whole number".to_string(),
            )];
            // Past whichever bound is reachable. `i64::MAX` has no successor, so
            // a field bounded there admits no over-range value at all — the
            // saturating arithmetic makes that come out as an absent candidate
            // rather than as a wrong one.
            let over = max.checked_add(1);
            let under = min.checked_sub(1);
            if let Some(n) = over.or(under) {
                out.push((
                    DefectKind::OutOfRange,
                    n.to_string(),
                    format!("the value has to be {min}..={max}"),
                ));
            }
            out
        }
        FieldType::Boolean => vec![(
            DefectKind::WrongType,
            "maybe".to_string(),
            "the value has to be true or false".to_string(),
        )],
        FieldType::Choice { of } => vec![(
            DefectKind::OutOfRange,
            outsider(of),
            format!("the value has to be one of {}", joined(of)),
        )],
        FieldType::Flags { of } => {
            let mut out = vec![(
                DefectKind::OutOfRange,
                outsider(of),
                format!("every word has to be one of {}", joined(of)),
            )];
            if let Some(first) = of.first() {
                out.push((
                    DefectKind::WrongType,
                    format!("{first}{}{first}", FieldType::SEPARATOR),
                    "the value is a set, so a repeated word is not one".to_string(),
                ));
            }
            out
        }
        // Whatever the element admits, at the element's own value: `encode`
        // delegates to the element shape, so a single bad element is a bad list.
        FieldType::List { of } => candidates(of),
    }
}

/// A word that is not in `of`.
///
/// Built by extending the options rather than by picking a literal, so a set that
/// happens to contain the literal cannot make this silently acceptable.
fn outsider(of: &[std::borrow::Cow<'static, str>]) -> String {
    let mut word = of
        .first()
        .map_or_else(|| "x".to_string(), ToString::to_string);
    while of.iter().any(|one| one.as_ref() == word) {
        word.push('x');
    }
    word
}

fn joined(of: &[std::borrow::Cow<'static, str>]) -> String {
    of.iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use super::{DefectKind, Scope, injectable, injectable_at, reachable};
    use crate::widgets::config_form::{Applies, ConfigDefect, ConfigField, ConfigForm, FieldType};
    use crate::widgets::text_format::TextFormat;

    fn field(key: &'static str, shape: FieldType, value: &'static str) -> ConfigField {
        ConfigField::new(key, "t", Applies::Restart, value).with_shape(shape)
    }

    fn kinds(field: &ConfigField) -> Vec<DefectKind> {
        let mut out: Vec<DefectKind> = injectable_at(field).into_iter().map(|i| i.kind).collect();
        out.sort_unstable();
        out
    }

    /// ★★★★★ The table in the module header, asserted rather than described.
    #[test]
    fn a_shape_decides_which_faults_it_admits() {
        assert_eq!(
            kinds(&field("t", FieldType::Text, "anything")),
            Vec::new(),
            "★ free text admits NOTHING — no value a person types keeps the target down"
        );
        assert_eq!(
            kinds(&field("b", FieldType::Boolean, "true")),
            vec![DefectKind::WrongType],
            "a boolean can be the wrong type and cannot be out of range"
        );
        assert_eq!(
            kinds(&field("n", FieldType::Integer { min: 0, max: 10 }, "5")),
            vec![DefectKind::WrongType, DefectKind::OutOfRange],
            "a bounded integer admits both"
        );
        assert_eq!(
            kinds(&field(
                "c",
                FieldType::Choice {
                    of: vec!["a".into(), "b".into()]
                },
                "a"
            )),
            vec![DefectKind::OutOfRange],
            "★ a choice cannot be the wrong TYPE — every text is a candidate word"
        );
        assert_eq!(
            kinds(&field(
                "f",
                FieldType::Flags {
                    of: vec!["a".into(), "b".into()]
                },
                "a"
            )),
            vec![DefectKind::WrongType, DefectKind::OutOfRange],
            "a set admits an outsider and a repeat"
        );
        assert_eq!(
            kinds(&field(
                "fmt",
                FieldType::Formatted {
                    of: TextFormat::Number { min: 1, max: 9 }
                },
                "5"
            )),
            vec![DefectKind::WrongType],
            "a formatted text can only be the wrong shape — the shape's own \
             bounds are part of what `Formatted` calls a wrong type"
        );
        // A list delegates to its element, so it admits what the element does.
        assert_eq!(
            kinds(&field(
                "l",
                FieldType::List {
                    of: Box::new(FieldType::Integer { min: 0, max: 3 })
                },
                "1"
            )),
            vec![DefectKind::WrongType, DefectKind::OutOfRange],
        );
        assert_eq!(
            kinds(&field(
                "lt",
                FieldType::List {
                    of: Box::new(FieldType::Text)
                },
                "a"
            )),
            Vec::new(),
            "★ a list of free text admits nothing either — the absence composes"
        );
    }

    /// ★★★★★ THE CONFIRMATION, which is what makes this a measurement: every
    /// offered value really produces the arm it is offered as, asked of `encode`
    /// itself.
    #[test]
    fn every_offer_is_confirmed_by_the_encoder() {
        let fields = vec![
            field("n", FieldType::Integer { min: 2, max: 9 }, "5"),
            field("b", FieldType::Boolean, "true"),
            field(
                "c",
                FieldType::Choice {
                    of: vec!["one".into(), "two".into()],
                },
                "one",
            ),
            field("t", FieldType::Text, "free"),
        ];
        let form = ConfigForm::new(fields, Vec::new());
        let offers = injectable(&form);
        assert!(!offers.is_empty());
        assert!(
            offers.iter().all(|o| o.kind != DefectKind::UnknownKey),
            "★ a form cannot hold a key its declaration lacks, so it is never \
             offered — see `Scope::Document`"
        );
        for offer in &offers {
            let shape = form
                .field(&offer.key)
                .expect("an offer names a declared field")
                .shape();
            let produced = shape
                .encode(&offer.key, &offer.value)
                .expect_err("an offered value is a defect");
            assert_eq!(
                DefectKind::of(&produced),
                offer.kind,
                "★ {:?} at {} is offered as {:?} and encodes as {:?}",
                offer.value,
                offer.key,
                offer.kind,
                DefectKind::of(&produced),
            );
            assert!(
                !offer.admitted_by.is_empty(),
                "an offer says which part of the declaration admits it"
            );
        }
        // The free-text field contributes nothing, so no offer names it.
        assert!(
            !offers.iter().any(|o| o.key == "t"),
            "free text must not appear: {offers:?}"
        );
    }

    /// ★★★★★ THE COUNTERFACTUAL THIS MODULE EXISTS FOR: adding a field to the
    /// declaration adds its faults, with nothing else edited.
    #[test]
    fn a_field_added_to_the_declaration_appears_without_editing_anything() {
        let before = ConfigForm::new(vec![field("b", FieldType::Boolean, "true")], Vec::new());
        let mut fields = before.fields().to_vec();
        fields.push(field("n", FieldType::Integer { min: 0, max: 4 }, "2"));
        let after = ConfigForm::new(fields, Vec::new());

        let keys = |form: &ConfigForm| {
            let mut out: Vec<String> = injectable(form).into_iter().map(|i| i.key).collect();
            out.sort();
            out.dedup();
            out
        };
        let grew: Vec<String> = keys(&after)
            .into_iter()
            .filter(|key| !keys(&before).contains(key))
            .collect();
        assert_eq!(
            grew,
            vec!["n".to_string()],
            "★ the new field's faults are offered because the DECLARATION changed"
        );
        assert_eq!(
            injectable(&after).len(),
            injectable(&before).len() + 2,
            "and a bounded integer brings exactly its two"
        );
        // ⚠ THE COUNTERFACTUAL'S OTHER HALF: a hand-kept list would pass the
        // check above only if somebody had also edited it, so the test has to
        // show that NOTHING was edited. Nothing here touches this module or the
        // panel — the only change is a `ConfigField` in a local vector.
        assert_eq!(
            before.fields().len() + 1,
            after.fields().len(),
            "one field added, and that is the whole of the edit"
        );
    }

    /// ★★★★★ R1853's own measurement: a form cannot inject the settings' third
    /// fault, and that is a property of `ConfigForm` rather than of this module.
    #[test]
    fn a_form_cannot_inject_a_key_its_declaration_lacks() {
        let form = ConfigForm::new(vec![field("real", FieldType::Boolean, "true")], Vec::new());
        assert!(
            injectable(&form)
                .iter()
                .all(|o| o.kind != DefectKind::UnknownKey),
            "not offered, because it cannot be performed"
        );
        // The reason, checkable: `adopt` reports a leaf with no row as
        // unplaceable rather than taking it — so there is no form state in which
        // that key is held.
        let mut form = form;
        let document = serde_json::json!({ "real.unknown": 1 });
        let adopted = form.adopt(&document);
        assert!(
            adopted.unplaceable.iter().any(|key| key == "real.unknown"),
            "★ the form REFUSES the undeclared leaf: {adopted:?}"
        );
        assert!(form.field("real.unknown").is_none(), "and does not gain it");
        // And the boundary says so, in a sentence a surface can show.
        assert_eq!(Scope::of(DefectKind::UnknownKey), Scope::Document);
        assert!(!Scope::Document.injectable());
        assert!(
            Scope::Document.because().contains("unplaceable"),
            "the reason names the mechanism: {}",
            Scope::Document.because()
        );
        // ⚠ It is still a REAL fault, and a non-blocking one — the vocabulary
        // has not lost an arm, only this panel has lost a way to reach it.
        assert!(!DefectKind::UnknownKey.blocks());
    }

    /// The offer's verdict is the report's, delegated rather than restated.
    #[test]
    fn what_an_injection_does_comes_from_the_defect_vocabulary() {
        for kind in DefectKind::ALL {
            let witness = match kind {
                DefectKind::UnknownKey => ConfigDefect::UnknownKey { key: String::new() },
                DefectKind::WrongType => ConfigDefect::WrongType {
                    key: String::new(),
                    want: String::new(),
                    got: String::new(),
                },
                DefectKind::OutOfRange => ConfigDefect::OutOfRange {
                    key: String::new(),
                    allowed: String::new(),
                },
            };
            assert_eq!(kind.blocks(), witness.blocks());
            assert_eq!(kind.wire(), witness.wire(), "one vocabulary, not two");
            assert_eq!(DefectKind::from_wire(kind.wire()), Some(kind));
            assert_eq!(DefectKind::of(&witness), kind);
        }
        assert_eq!(DefectKind::from_wire("link_dropped"), None);
        assert_eq!(DefectKind::ALL.len(), 3);
    }

    /// ★★★★★ The boundary is DATA, so a surface can show it instead of leaving
    /// the absence to look like an oversight.
    #[test]
    fn the_faults_this_cannot_inject_are_named_rather_than_absent() {
        assert_eq!(Scope::ALL.len(), 3, "three reasons, not one boundary");
        assert!(Scope::Settings.injectable());
        assert!(!Scope::Document.injectable());
        assert!(!Scope::World.injectable());
        assert!(
            Scope::World.because().contains("link drops"),
            "the reason names the kind of fault it means: {}",
            Scope::World.because()
        );
        // ★ Every defect arm has a scope, so the boundary is a total map rather
        // than a sentence about the arms somebody remembered.
        for kind in DefectKind::ALL {
            let scope = Scope::of(kind);
            assert_eq!(
                scope.injectable(),
                kind != DefectKind::UnknownKey,
                "{kind:?} -> {scope:?}"
            );
        }
        let mut wires: Vec<&str> = Scope::ALL.iter().map(|s| s.wire()).collect();
        wires.sort_unstable();
        wires.dedup();
        assert_eq!(wires.len(), Scope::ALL.len(), "the spellings are distinct");
        for scope in Scope::ALL {
            assert!(!scope.because().is_empty(), "{scope:?} says why");
        }
    }

    /// An integer bounded at the end of its own type admits no over-range value,
    /// and the absence is arithmetic rather than a special case.
    #[test]
    fn a_bound_with_no_successor_admits_no_out_of_range() {
        let at_the_end = field(
            "n",
            FieldType::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            "0",
        );
        assert_eq!(
            kinds(&at_the_end),
            vec![DefectKind::WrongType],
            "★ nothing is outside i64, so only a parse failure remains"
        );
    }

    /// ★★★★★ R1853 — the second confirmation, and it is what separates an offer
    /// from a claim.
    ///
    /// A row worked out from another has no written half, so
    /// [`ConfigField::set`] refuses it: every fault its *shape* admits is an act
    /// nothing can perform. Measured rather than reasoned about — the same
    /// declaration offers faults before the derivation and none after it.
    #[test]
    fn a_row_that_cannot_receive_a_value_offers_no_fault() {
        let bounded = field("n", FieldType::Integer { min: 0, max: 10 }, "5");
        assert!(
            !injectable_at(&bounded).is_empty(),
            "the premise: this shape admits faults while the row is writable"
        );
        assert!(reachable(&bounded), "and the row is reachable");

        let worked_out = field("n", FieldType::Integer { min: 0, max: 10 }, "5")
            .derived_from("another row this one is computed from");
        assert!(
            !reachable(&worked_out),
            "★ a row with no written half cannot receive a value"
        );
        assert!(
            injectable_at(&worked_out).is_empty(),
            "★ so the same declaration must offer nothing there: {:?}",
            kinds(&worked_out),
        );
    }

    /// ★★★ And the case the boundary deliberately does NOT cover, asserted so
    /// nobody narrows it later by intuition.
    ///
    /// A hidden row still offers its faults, because `ConfigForm::verdict`
    /// checks a hidden row like any other — its own doc says hiding is a screen
    /// decision and a value blocking a start-up with nothing on screen is the
    /// case that rule exists for. So the fault is real and performable, and
    /// whether to show it belongs to the surface.
    #[test]
    fn a_row_the_screen_hides_still_offers_its_faults() {
        let hidden = field("n", FieldType::Integer { min: 0, max: 10 }, "5").with_hidden(true);
        assert!(
            reachable(&hidden),
            "★ hidden is a screen decision, not an unreachable row"
        );
        assert_eq!(
            kinds(&hidden),
            vec![DefectKind::WrongType, DefectKind::OutOfRange],
            "★ and the declaration admits what it always admitted"
        );
    }
}
