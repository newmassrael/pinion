//! **The option surface a form is an editor for**, and how much of it the form
//! can actually reach.
//!
//! A [`ConfigForm`] holds the rows a node has and the keys its palette offers
//! to add. What it cannot say is the question a person picking the tool asks
//! first: *can I configure the thing with it?* A form knows its own rows. It
//! does not know what it is missing, and a catalogue that has drifted a release
//! behind the thing it configures looks exactly like a complete one.
//!
//! [`ConfigSchema`] is that missing half — the declared set of paths the target
//! accepts, with the shape each one holds — and the two meters over it are the
//! measurement:
//!
//! * [`ConfigSchema::reached_by`] — **how much of the surface the palette can
//!   author**, as two numbers, because one hides the other: a tool can touch
//!   every top-level section and still reach a small fraction of the leaves.
//! * [`ConfigSchema::strings`] — **how much of the string surface is pinned
//!   down**, partitioned into the three kinds a string can be.
//!
//! # The numbers are derived, never written down
//!
//! Both are computed from the form's own catalogue against the schema. Nothing
//! anywhere states a score, so forgetting a field makes the number fall by
//! itself — which is the only version of this meter worth having. A recorded
//! completion is a claim about the day it was recorded.
//!
//! # What this checks that a coverage count does not
//!
//! A count answers "is the key reachable" and stops. [`Reach`] also carries
//! [`mistyped`](Reach::mistyped): a key the palette offers at a **different
//! shape** than the schema declares. That is the failure a count is blind to
//! and the more common one — the key is there, the row is there, the value the
//! person types is accepted, and the target refuses to start. A palette that
//! offers a formatted string as free text reaches the key and cannot author it.

use std::borrow::Cow;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::config_form::{ConfigForm, FieldType};

/// One leaf of the configuration, and the shape it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaLeaf {
    /// The configuration path, verbatim — the same spelling
    /// [`ConfigField::key`](super::config_form::ConfigField::key) uses, because
    /// the two are compared and a translation table between them is a third
    /// thing to keep right.
    pub path: Cow<'static, str>,
    /// What the leaf holds.
    pub ty: FieldType,
}

impl SchemaLeaf {
    /// A leaf at `path` holding `ty`.
    #[must_use]
    pub fn new(path: impl Into<Cow<'static, str>>, ty: FieldType) -> Self {
        Self {
            path: path.into(),
            ty,
        }
    }

    /// The first segment of the path — the section this leaf belongs to.
    #[must_use]
    pub fn root(&self) -> &str {
        self.path.split('.').next().unwrap_or(&self.path)
    }
}

/// Why a schema could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// Two leaves declare the same path.
    Duplicate {
        /// The path declared twice.
        path: String,
    },
    /// One leaf's path is a prefix of another's, so the same path is both a
    /// value and a section.
    ///
    /// Rejected rather than resolved because there is no answer: a document
    /// cannot hold a string at `a.b` and an object at `a.b` at once, and a
    /// schema that admitted both would make [`ConfigForm::document`] produce
    /// one of them silently.
    ///
    /// [`ConfigForm::document`]: super::config_form::ConfigForm::document
    Nested {
        /// The shorter path.
        outer: String,
        /// The path it is a prefix of.
        inner: String,
    },
    /// A path with an empty segment, which no document can address.
    Empty {
        /// The offending path.
        path: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate { path } => write!(f, "{path} is declared twice"),
            Self::Nested { outer, inner } => {
                write!(f, "{outer} is a value and also the start of {inner}")
            }
            Self::Empty { path } => write!(f, "{path} has an empty segment"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// The declared option surface.
///
/// See the [module documentation](self) for what it is for and why the meters
/// over it are derived rather than recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSchema {
    leaves: Vec<SchemaLeaf>,
}

impl ConfigSchema {
    /// Build a schema from its leaves.
    ///
    /// # Errors
    ///
    /// [`SchemaError`], when the paths cannot all be a document at once. Fail
    /// fast, at the declaration: a schema is a constant, so a broken one is a
    /// mistake to find at the first test rather than the first save.
    pub fn new(leaves: Vec<SchemaLeaf>) -> Result<Self, SchemaError> {
        for leaf in &leaves {
            if leaf.path.is_empty() || leaf.path.split('.').any(str::is_empty) {
                return Err(SchemaError::Empty {
                    path: leaf.path.to_string(),
                });
            }
        }
        for (i, leaf) in leaves.iter().enumerate() {
            for other in &leaves[i + 1..] {
                if leaf.path == other.path {
                    return Err(SchemaError::Duplicate {
                        path: leaf.path.to_string(),
                    });
                }
                if let Some((outer, inner)) = strict_prefix(&leaf.path, &other.path) {
                    return Err(SchemaError::Nested {
                        outer: outer.to_string(),
                        inner: inner.to_string(),
                    });
                }
            }
        }
        Ok(Self { leaves })
    }

    /// Every leaf, in declaration order.
    #[must_use]
    pub fn leaves(&self) -> &[SchemaLeaf] {
        &self.leaves
    }

    /// The sections, in declaration order, each once.
    #[must_use]
    pub fn roots(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for leaf in &self.leaves {
            let root = leaf.root();
            if !seen.contains(&root) {
                seen.push(root);
            }
        }
        seen
    }

    /// What the schema says `path` holds.
    #[must_use]
    pub fn ty(&self, path: &str) -> Option<&FieldType> {
        self.leaves
            .iter()
            .find(|leaf| leaf.path == path)
            .map(|leaf| &leaf.ty)
    }

    /// **How much of the surface `form`'s catalogue can author.**
    ///
    /// The catalogue is the form's rows *and* the keys it offers to add: both
    /// are ways to reach a key, and counting only one would report a palette as
    /// narrow because its keys happen to open already present.
    ///
    /// A leaf counts as reached when a catalogue key **equals** it, and nothing
    /// weaker. ★★★★★ R1690 — this was first written to count a key that is
    /// merely on the same root-to-leaf line, which is what a coverage figure
    /// over two path lists usually means and is wrong for *this* type: a
    /// [`ConfigField`] holds a scalar, [`FieldType`] has no arm for an object,
    /// so a row keyed at a **section** produces a document with a string where
    /// the target wants a subtree. Such a key reaches nothing, and counting it
    /// as reaching everything beneath it inflates the figure in the direction
    /// nobody checks. It is reported instead — see [`Reach::unauthorable`].
    ///
    /// [`ConfigField`]: super::config_form::ConfigField
    #[must_use]
    pub fn reached_by(&self, form: &ConfigForm) -> Reach {
        let catalogue: Vec<(&str, &FieldType)> = form
            .fields()
            .iter()
            .chain(form.addable())
            .map(|f| (f.key(), f.shape()))
            .collect();
        self.reached_by_keys(&catalogue)
    }

    /// [`Self::reached_by`] against a catalogue given directly, for a caller
    /// whose palette is not one [`ConfigForm`] — a screen with a form per node
    /// reaches the surface with all of them together, and measuring one at a
    /// time would report the union as the smallest part.
    #[must_use]
    pub fn reached_by_keys(&self, catalogue: &[(&str, &FieldType)]) -> Reach {
        let mut leaves_missing: Vec<String> = Vec::new();
        let mut reached_roots: BTreeSet<&str> = BTreeSet::new();
        for leaf in &self.leaves {
            if catalogue.iter().any(|(key, _)| *key == leaf.path) {
                reached_roots.insert(leaf.root());
            } else {
                leaves_missing.push(leaf.path.to_string());
            }
        }
        // ★★ The sections figure is DERIVED from the leaves one rather than
        // counted beside it. Two independent counts over one palette can tell
        // different stories — "every section reached, almost no leaves" is a
        // sentence a reader would have to reconcile — and here the first is a
        // fold of the second by construction.
        let mut roots_missing: Vec<String> = Vec::new();
        for root in self.roots() {
            if !reached_roots.contains(root) {
                roots_missing.push(root.to_string());
            }
        }
        let mut mistyped: Vec<Mistyped> = Vec::new();
        let mut unknown: BTreeSet<String> = BTreeSet::new();
        let mut unauthorable: BTreeSet<String> = BTreeSet::new();
        for (key, offered) in catalogue {
            let on_a_line = self.leaves.iter().any(|leaf| on_one_line(key, &leaf.path));
            match self.ty(key) {
                Some(declared) if declared == *offered => {}
                Some(declared) => mistyped.push(Mistyped {
                    path: (*key).to_string(),
                    declared: declared.clone(),
                    offered: (*offered).clone(),
                }),
                // Not a leaf, and the two ways of not being one take different
                // repairs: a key inside the surface names a section and needs
                // to name the leaf it meant, a key outside it is a path this
                // schema has never heard of.
                None if on_a_line => {
                    unauthorable.insert((*key).to_string());
                }
                None => {
                    unknown.insert((*key).to_string());
                }
            }
        }
        Reach {
            root_total: self.roots().len(),
            roots_missing,
            leaf_total: self.leaves.len(),
            leaves_missing,
            mistyped,
            unknown: unknown.into_iter().collect(),
            unauthorable: unauthorable.into_iter().collect(),
        }
    }

    /// **How much of the string surface is pinned down.**
    ///
    /// Every leaf that holds a string somewhere is in exactly one of the three
    /// classes, decided by the shape it is declared with rather than by a table
    /// beside it. Exclusive and total by construction: a leaf cannot be in two
    /// classes and cannot be in none, which is the improvement over a
    /// classification kept in lists — those can disagree and can omit, and the
    /// meter over them exists to catch that.
    ///
    /// What stays measurable is [`free`](StringCensus::free): a string nobody
    /// has given a shape. That is the same gap the list version reports as
    /// "unclassified", said in a way that cannot be wrong.
    #[must_use]
    pub fn strings(&self) -> StringCensus {
        let mut census = StringCensus::default();
        for leaf in &self.leaves {
            match string_class(&leaf.ty) {
                Some(StringClass::Choice) => census.choices.push(leaf.path.to_string()),
                Some(StringClass::Format) => census.formats.push(leaf.path.to_string()),
                Some(StringClass::Free) => census.free.push(leaf.path.to_string()),
                None => {}
            }
        }
        census
    }
}

/// Which of the three kinds a string leaf is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringClass {
    /// One of a fixed set of words.
    Choice,
    /// A string that has to parse.
    Format,
    /// Anything.
    Free,
}

/// The class of the string a shape holds, or `None` when it holds no string.
///
/// Recurses through the container shapes, because a list of formatted strings
/// is a formatted string surface — a meter that looked only at scalar leaves
/// would report a screen whose addresses are all lists as having no string
/// surface at all.
fn string_class(ty: &FieldType) -> Option<StringClass> {
    match ty {
        FieldType::Text => Some(StringClass::Free),
        FieldType::Formatted { .. } => Some(StringClass::Format),
        FieldType::Choice { .. } | FieldType::Flags { .. } => Some(StringClass::Choice),
        FieldType::List { of } => string_class(of),
        FieldType::Integer { .. } | FieldType::Boolean => None,
    }
}

/// A catalogue key offered at a shape the schema does not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mistyped {
    /// The path.
    pub path: String,
    /// What the schema says it holds.
    pub declared: FieldType,
    /// What the palette offers it as.
    pub offered: FieldType,
}

impl Mistyped {
    /// What a person reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} is declared {} and the palette offers it as {}",
            self.path,
            shape_word(&self.declared),
            shape_word(&self.offered)
        )
    }
}

/// A shape, in one or two words, for a sentence.
fn shape_word(ty: &FieldType) -> String {
    match ty {
        FieldType::Text => "free text".to_string(),
        FieldType::Formatted { of } => of.wanted(),
        FieldType::Integer { min, max } => format!("a whole number {min} to {max}"),
        FieldType::Boolean => "true or false".to_string(),
        // R1787 — the shared `one of a, b, c` rendering, lifted at its third
        // consumer beside the declaration it is the person-facing half of.
        FieldType::Choice { of } => crate::external::one_of_phrase(of.iter().map(Cow::as_ref)),
        FieldType::Flags { of } => {
            let words: Vec<&str> = of.iter().map(Cow::as_ref).collect();
            format!("any of {}", words.join(", "))
        }
        FieldType::List { of } => format!("a list of ({})", shape_word(of)),
    }
}

/// How much of a schema a palette reaches.
///
/// Two numbers, and the pair is the point: the sections are the coarse view a
/// reader forms an impression from, and the leaves are the distance actually
/// left. Reporting the first alone reads as done at the moment it is least
/// true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    /// How many sections the schema has.
    pub root_total: usize,
    /// The sections nothing in the catalogue touches.
    pub roots_missing: Vec<String>,
    /// How many leaves the schema has.
    pub leaf_total: usize,
    /// The leaves nothing in the catalogue touches.
    ///
    /// Not an indictment on its own: a form with a way to type a path by hand
    /// can still author these. It is the list of what that way is *for*.
    pub leaves_missing: Vec<String>,
    /// Keys the palette offers at the wrong shape.
    ///
    /// The failure a reach count cannot see — see the [module
    /// documentation](self).
    pub mistyped: Vec<Mistyped>,
    /// Catalogue keys on no line of the schema at all, sorted.
    ///
    /// Drift in the direction a count reads as *progress*: a mistyped path
    /// raises no number and answers nothing.
    pub unknown: Vec<String>,
    /// Catalogue keys that name a **section** of the schema rather than a leaf,
    /// sorted.
    ///
    /// ★★★ R1690 — separate from [`Self::unknown`] because the two take
    /// different repairs, and separate from the reached set because a row
    /// cannot hold a section at all: a form field is a scalar, so such a key
    /// composes a string where the target wants a subtree. The palette author
    /// meant one of the leaves under it and has to say which.
    pub unauthorable: Vec<String>,
}

impl Reach {
    /// Sections reached.
    #[must_use]
    pub const fn root_hit(&self) -> usize {
        self.root_total - self.roots_missing.len()
    }

    /// Leaves reached.
    #[must_use]
    pub const fn leaf_hit(&self) -> usize {
        self.leaf_total - self.leaves_missing.len()
    }

    /// Whether the palette reaches all of it, at the declared shapes, with
    /// nothing offered that the schema does not know.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.roots_missing.is_empty() && self.leaves_missing.is_empty() && self.sound()
    }

    /// Whether anything is **wrong**, as opposed to merely absent.
    ///
    /// Separate from [`Self::complete`] because the two deserve different
    /// answers on screen: a palette that does not offer every leaf is a
    /// deliberate, ordinary state, and one that offers a key at the wrong
    /// shape is a defect regardless of how much it covers.
    #[must_use]
    pub const fn sound(&self) -> bool {
        self.mistyped.is_empty() && self.unknown.is_empty() && self.unauthorable.is_empty()
    }

    /// The short label a meter shows: the two numbers.
    ///
    /// ★ R1690 — **sections**, not "fields", and the word was chosen by looking
    /// at the screen. The reference calls its top-level keys fields, and on a
    /// panel that sits directly above a form made of *field rows* that word
    /// reads as a count of those rows — which on the first screen this was
    /// painted on happened to be eleven, the very number beside it. A label
    /// whose two plausible readings agree by coincidence is the worst kind.
    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "sections {}/{} · leaves {}/{}",
            self.root_hit(),
            self.root_total,
            self.leaf_hit(),
            self.leaf_total
        )
    }

    /// The long form, naming what is missing and what is wrong.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if !self.roots_missing.is_empty() {
            lines.push(format!(
                "nothing in the palette reaches {}",
                self.roots_missing.join(", ")
            ));
        }
        if !self.leaves_missing.is_empty() {
            lines.push(format!(
                "{} leaves are not in the catalogue and are typed in by hand",
                self.leaves_missing.len()
            ));
        }
        for wrong in &self.mistyped {
            lines.push(wrong.sentence());
        }
        for key in &self.unknown {
            lines.push(format!("{key} is not a key the schema declares"));
        }
        for key in &self.unauthorable {
            lines.push(format!(
                "{key} names a section, and a row holds one value — say which leaf"
            ));
        }
        if lines.is_empty() {
            lines.push("the palette reaches all of it".to_string());
        }
        lines.join("\n")
    }
}

/// The string surface, partitioned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StringCensus {
    /// Leaves whose string is one of a fixed set of words.
    pub choices: Vec<String>,
    /// Leaves whose string has to parse.
    pub formats: Vec<String>,
    /// Leaves whose string is anything — **the gap**.
    pub free: Vec<String>,
}

impl StringCensus {
    /// How many leaves hold a string.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.choices.len() + self.formats.len() + self.free.len()
    }

    /// How many hold a string that is pinned down one way or the other.
    #[must_use]
    pub const fn pinned(&self) -> usize {
        self.choices.len() + self.formats.len()
    }

    /// The short label a meter shows.
    #[must_use]
    pub fn label(&self) -> String {
        format!("strings {}/{}", self.pinned(), self.total())
    }
}

/// Whether `a` and `b` are on one root-to-leaf line: equal, or one an ancestor
/// of the other.
fn on_one_line(a: &str, b: &str) -> bool {
    a == b || is_ancestor(a, b) || is_ancestor(b, a)
}

/// Whether `outer` names a section that `inner` is inside.
fn is_ancestor(outer: &str, inner: &str) -> bool {
    inner.len() > outer.len()
        && inner.starts_with(outer)
        && inner.as_bytes().get(outer.len()) == Some(&b'.')
}

/// The two paths, ordered outer-first, when one is strictly inside the other.
fn strict_prefix<'a>(a: &'a str, b: &'a str) -> Option<(&'a str, &'a str)> {
    if is_ancestor(a, b) {
        Some((a, b))
    } else if is_ancestor(b, a) {
        Some((b, a))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::config_form::{Applies, ConfigField, ConfigForm, FieldType};
    use super::super::text_format::{CharClass, CharSet, Span, TextFormat};
    use super::{ConfigSchema, Mistyped, SchemaError, SchemaLeaf};

    fn ident() -> FieldType {
        FieldType::Formatted {
            of: TextFormat::Chars {
                allow: CharSet::of(&[CharClass::LowerHex]),
                len: Span::between(1, 32),
            },
        }
    }

    /// A small surface with the shape a real one has: several sections, some
    /// with more than one leaf, and all three kinds of string.
    fn schema() -> ConfigSchema {
        ConfigSchema::new(vec![
            SchemaLeaf::new("id", ident()),
            SchemaLeaf::new("label", FieldType::Text),
            SchemaLeaf::new(
                "routing.mode",
                FieldType::Choice {
                    of: vec!["client".into(), "router".into()],
                },
            ),
            SchemaLeaf::new("routing.hops", FieldType::Integer { min: 1, max: 8 }),
            SchemaLeaf::new("discovery.multicast", FieldType::Boolean),
            SchemaLeaf::new(
                "listen.endpoints",
                FieldType::List {
                    of: Box::new(FieldType::Text),
                },
            ),
        ])
        .expect("the fixture is a document")
    }

    /// A palette reaching `id` and everything under `routing`.
    fn palette() -> ConfigForm {
        ConfigForm::new(
            vec![ConfigField::new("id", "id", Applies::Restart, "a1").with_shape(ident())],
            vec![
                ConfigField::new("routing.mode", "mode", Applies::Restart, "client").with_shape(
                    FieldType::Choice {
                        of: vec!["client".into(), "router".into()],
                    },
                ),
                ConfigField::new("routing.hops", "int", Applies::Restart, "3")
                    .with_shape(FieldType::Integer { min: 1, max: 8 }),
            ],
        )
    }

    /// ★★★ R1690 — the two numbers, and the reason there are two.
    ///
    /// The sections say 2 of 5 and the leaves say 3 of 6, and the gap between
    /// those fractions is the whole argument for reporting both: a palette that
    /// covered one leaf of a ten-leaf section would show as reaching that
    /// section, and a reader would take "sections 5/5" for done.
    #[test]
    fn r1690_reach_reports_sections_and_leaves_separately() {
        let reach = schema().reached_by(&palette());
        assert_eq!(reach.root_total, 5, "{:?}", schema().roots());
        assert_eq!(
            reach.root_hit(),
            2,
            "id and routing: {:?}",
            reach.roots_missing
        );
        assert_eq!(reach.leaf_total, 6);
        assert_eq!(
            reach.leaf_hit(),
            3,
            "id, routing.mode, routing.hops: {:?}",
            reach.leaves_missing
        );
        assert!(!reach.complete());
        assert!(reach.sound(), "nothing is offered at the wrong shape");
    }

    /// ★★★★★ R1690 — **the number falls by itself when the palette narrows.**
    ///
    /// The property the meter exists for and the one it is easiest to build
    /// without: a coverage figure written down anywhere stays where it was
    /// written. Asserted by building the same palette without one field and
    /// reading the number, so a re-implementation that cached or recorded it
    /// fails here.
    #[test]
    fn r1690_a_narrower_palette_lowers_the_number_with_nobody_editing_it() {
        let schema = schema();
        let before = schema.reached_by(&palette());
        let narrowed = ConfigForm::new(
            Vec::new(),
            palette()
                .addable()
                .into_iter()
                .cloned()
                .collect::<Vec<ConfigField>>(),
        );
        let after = schema.reached_by(&narrowed);
        assert_eq!(after.leaf_hit(), before.leaf_hit() - 1);
        assert_eq!(after.root_hit(), before.root_hit() - 1);
        assert!(
            after.leaves_missing.iter().any(|p| p == "id"),
            "and it says which: {:?}",
            after.leaves_missing
        );
    }

    /// ★★★ R1690 — reach is a property of the **palette**, not of what is on
    /// screen at this instant.
    ///
    /// A row a person took out is still authorable, because the catalogue
    /// offers it back as a chip. A meter that fell when somebody hid a row
    /// would be measuring the session rather than the tool, and would drop
    /// every time a user tidied their inspector.
    ///
    /// Found by a first draft of the test above that removed the row and
    /// expected the number to move; it did not, and the model was right.
    #[test]
    fn r1690_taking_a_row_out_does_not_narrow_what_the_tool_can_author() {
        let schema = schema();
        let before = schema.reached_by(&palette());
        let mut without = palette();
        without.remove("id").expect("the palette holds it");
        assert!(
            without.fields().iter().all(|f| f.key() != "id"),
            "the row really is off the screen",
        );
        let after = schema.reached_by(&without);
        assert_eq!(after.leaf_hit(), before.leaf_hit());
        assert!(
            without.addable().iter().any(|f| f.key() == "id"),
            "and the reason is that it is offered back",
        );
    }

    /// ★★★★★ R1690 — a key at the wrong shape is REACHED and still cannot be
    /// authored.
    ///
    /// The failure a coverage count is blind to, and the one this round found
    /// on a real screen: the palette offers the identifier as free text, the
    /// row appears, a person types anything into it, and the target refuses to
    /// start. Every count says the key is covered.
    #[test]
    fn r1690_a_key_offered_at_the_wrong_shape_is_reached_and_unsound() {
        let schema = schema();
        let weak = ConfigForm::new(
            vec![ConfigField::new("id", "text", Applies::Restart, "a1")],
            Vec::new(),
        );
        let reach = schema.reached_by(&weak);
        assert!(
            reach.leaves_missing.iter().all(|p| p != "id"),
            "a count says it is covered",
        );
        assert!(
            !reach.sound(),
            "and the shape check says it is not authorable"
        );
        assert_eq!(reach.mistyped.len(), 1);
        let sentence = reach.mistyped[0].sentence();
        assert!(
            sentence.contains("id") && sentence.contains("free text"),
            "the report names the key and what it was offered as: {sentence}",
        );
        // The strong direction: offering it at the DECLARED shape is sound, so
        // this is a statement about the shape and not about the key.
        assert!(schema.reached_by(&palette()).sound());
    }

    /// A catalogue key on no line of the schema is drift the count reads as
    /// progress, so it is reported by name.
    #[test]
    fn r1690_a_key_the_schema_does_not_know_is_named() {
        let schema = schema();
        let stray = ConfigForm::new(
            vec![ConfigField::new(
                "routng.mode",
                "mode",
                Applies::Hot,
                "client",
            )],
            Vec::new(),
        );
        let reach = schema.reached_by(&stray);
        assert_eq!(reach.unknown, vec!["routng.mode".to_string()]);
        assert!(!reach.sound());
        assert_eq!(reach.leaf_hit(), 0, "and it reached nothing");
    }

    /// ★★★★★ R1690 — **a catalogue key that names a section can author
    /// nothing**, and the first draft of this meter counted it as reaching
    /// every leaf under it.
    ///
    /// Proven rather than argued, through the form's own composition: a row is
    /// a scalar, so a key at a section produces a document with a **string**
    /// where the target wants a subtree — and the schema refuses to declare the
    /// two together, which is the same fact from the other side. So a key like
    /// this is not a reach; it is a palette author who meant one of the leaves
    /// under it and has not said which.
    #[test]
    fn r1690_a_catalogue_key_that_names_a_section_can_author_nothing() {
        let schema = schema();
        let section = ConfigForm::new(
            vec![ConfigField::new("routing", "map", Applies::Hot, "")],
            Vec::new(),
        );
        // What such a row actually composes to.
        let document = section.document().expect("the row encodes as a string");
        assert_eq!(
            document.get("routing").and_then(|v| v.as_str()),
            Some(""),
            "a scalar where the schema says a section begins: {document}",
        );
        // ...and no schema can hold both, which is why it is not authorable.
        assert!(matches!(
            ConfigSchema::new(vec![
                SchemaLeaf::new("routing", FieldType::Text),
                SchemaLeaf::new("routing.mode", FieldType::Text),
            ]),
            Err(SchemaError::Nested { .. })
        ));

        let reach = schema.reached_by(&section);
        assert_eq!(reach.leaf_hit(), 0, "so it reaches nothing: {reach:?}");
        // ★★★★★ And the SECTION figure is zero too, which is the assertion a
        // counterfactual demanded: this is the only palette on which the fold
        // and an independent count disagree — a section-naming key is on the
        // section's line while reaching no leaf in it — so without this line
        // "the sections figure is derived from the leaves" could be broken with
        // every test still green.
        assert_eq!(
            reach.root_hit(),
            0,
            "a key that authors no leaf of a section has not reached the section",
        );
        assert_eq!(reach.unauthorable, vec!["routing".to_string()]);
        assert!(reach.unknown.is_empty(), "it IS on the surface, though");
        assert!(!reach.sound());
        assert!(
            reach.sentence().contains("names a section"),
            "and the report says what to do about it: {}",
            reach.sentence(),
        );
        // A key DEEPER than a declared leaf is the same kind of not-a-leaf, and
        // is reported the same way rather than counted.
        let deeper = ConfigForm::new(
            vec![ConfigField::new(
                "listen.endpoints.0",
                "address",
                Applies::Hot,
                "",
            )],
            Vec::new(),
        );
        let reach = schema.reached_by(&deeper);
        assert_eq!(reach.leaf_hit(), 0);
        assert_eq!(reach.unauthorable, vec!["listen.endpoints.0".to_string()]);
    }

    /// The sections figure is a fold of the leaves figure, not a second count.
    ///
    /// Without this, a palette naming one leaf of every section would report
    /// every section reached and almost no leaves — two numbers telling
    /// different stories about the same catalogue.
    #[test]
    fn r1690_the_sections_figure_is_derived_from_the_leaves() {
        let schema = schema();
        let empty = ConfigForm::new(Vec::new(), Vec::new());
        let reach = schema.reached_by(&empty);
        assert_eq!(reach.leaf_hit(), 0);
        assert_eq!(reach.root_hit(), 0, "no leaf reached is no section reached");
        let one = ConfigForm::new(
            vec![
                ConfigField::new("routing.mode", "mode", Applies::Hot, "client").with_shape(
                    FieldType::Choice {
                        of: vec!["client".into(), "router".into()],
                    },
                ),
            ],
            Vec::new(),
        );
        let reach = schema.reached_by(&one);
        assert_eq!(reach.leaf_hit(), 1);
        assert_eq!(
            reach.root_hit(),
            1,
            "one leaf of a two-leaf section reaches the section, and only it",
        );
        assert!(reach.roots_missing.iter().any(|r| r == "discovery"));
    }

    /// ★★★ R1690 — the string surface is partitioned, and the free ones are
    /// the gap.
    ///
    /// Exclusive and total by construction rather than by a check, because the
    /// class is read off the declared shape. What stays measurable — and what
    /// this asserts moves — is how many strings nobody has pinned down.
    #[test]
    fn r1690_every_string_leaf_is_in_exactly_one_class() {
        let schema = schema();
        let census = schema.strings();
        assert_eq!(
            census.total(),
            4,
            "id, label, routing.mode and the list of endpoints hold strings; \
             the integer and the boolean do not: {census:?}",
        );
        assert_eq!(census.formats, vec!["id".to_string()]);
        assert_eq!(census.choices, vec!["routing.mode".to_string()]);
        assert_eq!(
            census.free,
            vec!["label".to_string(), "listen.endpoints".to_string()],
            "a list of free text is a free string surface",
        );
        assert_eq!(census.pinned(), 2);
        assert_eq!(
            census.total(),
            census.choices.len() + census.formats.len() + census.free.len(),
            "the three are the whole of it",
        );
    }

    /// Pinning a string down moves it out of the gap and into a class.
    ///
    /// The counter-assertion for the census: without it, a partition that put
    /// everything in one class would pass the totality check above.
    #[test]
    fn r1690_pinning_a_string_moves_it_out_of_the_gap() {
        let loose = ConfigSchema::new(vec![SchemaLeaf::new("id", FieldType::Text)]).expect("ok");
        assert_eq!(loose.strings().free.len(), 1);
        assert_eq!(loose.strings().pinned(), 0);
        let pinned = ConfigSchema::new(vec![SchemaLeaf::new("id", ident())]).expect("ok");
        assert!(pinned.strings().free.is_empty());
        assert_eq!(pinned.strings().pinned(), 1);
        assert_eq!(pinned.strings().label(), "strings 1/1");
    }

    /// A schema whose paths cannot all be one document is refused where it is
    /// declared.
    #[test]
    fn r1690_a_schema_that_is_not_a_document_is_refused() {
        let duplicate = ConfigSchema::new(vec![
            SchemaLeaf::new("a.b", FieldType::Text),
            SchemaLeaf::new("a.b", FieldType::Boolean),
        ]);
        assert_eq!(
            duplicate,
            Err(SchemaError::Duplicate {
                path: "a.b".to_string()
            })
        );
        let nested = ConfigSchema::new(vec![
            SchemaLeaf::new("a.b", FieldType::Text),
            SchemaLeaf::new("a.b.c", FieldType::Text),
        ]);
        assert_eq!(
            nested,
            Err(SchemaError::Nested {
                outer: "a.b".to_string(),
                inner: "a.b.c".to_string(),
            }),
            "a path cannot be a value and a section at once",
        );
        assert!(matches!(
            ConfigSchema::new(vec![SchemaLeaf::new("a..b", FieldType::Text)]),
            Err(SchemaError::Empty { .. })
        ));
        // And the near miss is accepted: a shared prefix that is not a segment
        // boundary is a different section.
        assert!(
            ConfigSchema::new(vec![
                SchemaLeaf::new("a.b", FieldType::Text),
                SchemaLeaf::new("a.bc", FieldType::Text),
            ])
            .is_ok(),
            "`a.bc` is not inside `a.b`",
        );
    }

    /// The label is the two fractions, in the order a reader takes them.
    #[test]
    fn r1690_the_label_carries_both_numbers() {
        let reach = schema().reached_by(&palette());
        assert_eq!(reach.label(), "sections 2/5 · leaves 3/6");
        let sentence = reach.sentence();
        assert!(
            sentence.contains("discovery") && sentence.contains("label"),
            "the long form names the sections nothing reaches: {sentence}",
        );
    }

    /// ★★★★ R1718 — every way a palette can fall short is said, and no two of
    /// them read alike.
    ///
    /// This producer is not an enum: it is a report that appends a line per
    /// kind of shortfall, so the arms are the five `if`/`for` blocks plus the
    /// clean case. Driving them one at a time is what makes the count mean
    /// something — a report driven only with everything wrong at once would
    /// pass while two of its lines read identically.
    #[test]
    fn r1718_every_way_a_palette_falls_short_is_said_and_distinct() {
        use crate::test_fixtures::speech::assert_speaks;

        let schema = schema();
        let row = |key: &'static str, ty: &'static str| {
            ConfigForm::new(
                vec![ConfigField::new(key, ty, Applies::Restart, "a1")],
                Vec::new(),
            )
        };
        // Every leaf the schema declares, so nothing is missing and nothing is
        // wrong: the clean line, which a report of shortfalls still has to say.
        let whole = ConfigForm::new(
            schema
                .leaves()
                .iter()
                .map(|leaf| {
                    ConfigField::new(leaf.path.clone(), "x", Applies::Restart, "")
                        .with_shape(leaf.ty.clone())
                })
                .collect(),
            Vec::new(),
        );
        let said = [
            ("clean", schema.reached_by(&whole).sentence()),
            ("roots missing", schema.reached_by(&palette()).sentence()),
            ("mistyped", schema.reached_by(&row("id", "text")).sentence()),
            (
                "unknown",
                schema.reached_by(&row("routng.mode", "mode")).sentence(),
            ),
            (
                "unauthorable",
                schema.reached_by(&row("routing", "map")).sentence(),
            ),
        ];
        assert_speaks("Reach", 5, &said, &[]);

        let one = Mistyped {
            path: "routing.hops".to_owned(),
            declared: FieldType::Integer { min: 1, max: 8 },
            offered: FieldType::Text,
        };
        assert_speaks("Mistyped", 1, &[("Mistyped", one.sentence())], &[]);
    }
}
