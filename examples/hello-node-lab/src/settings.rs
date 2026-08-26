//! R1690 — **the option surface this tool is an editor for**, and the shapes
//! its values have to have.
//!
//! The inspector knows which rows a node opens with. What it could not say
//! before this module existed is the question a person picking the tool asks
//! first — *can I configure the thing with it?* — because nothing anywhere
//! declared what "the thing" accepts. A palette of eleven keys over a surface
//! of eleven is finished; over a surface of forty it is a start, and the two
//! looked identical.
//!
//! So this is the surface, declared: every path the target takes and the shape
//! each one holds. Two things follow from having it, and neither is available
//! without it.
//!
//! # The palette takes its shapes from here
//!
//! [`shape_of`] is what the inspector types its rows with, so a row cannot be
//! offered at a shape the target does not accept. That is the repair for the
//! defect this module was written to expose: the node identifier is read by a
//! parser, and this screen offered it as **free text** — every value went in,
//! the form said nothing, and the node would not have come up.
//!
//! # The two meters
//!
//! [`ConfigSchema::reached_by_keys`] and [`ConfigSchema::strings`] are the
//! reference tool's remaining two self-censuses, which it publishes beside its
//! operation list and its save partition. Both are computed from the palette
//! against this table, so **neither number is written down anywhere** — drop a
//! field and the figure falls on its own, which is the only version of a
//! coverage meter worth painting.
//!
//! **Vocabulary is neutral by construction**, as everywhere else in this
//! example: the paths are the ones the tool class uses generally, and what is
//! being reproduced is that a surface of this size exists and that a palette
//! covers a knowable fraction of it.
//!
//! # ★★★★★ R1840 / R1842 — and the third, which is about THIS FILE
//!
//! The two meters above divide by a table of paths, and until R1842 that table
//! was written **by hand, in the same crate as the screen it types, by whoever
//! last edited that screen**. R1690 built it that way and registered the
//! defect in the same round: a declaration that falls behind its target loses
//! leaves from the denominator, so the coverage figure RISES. Drift reads as
//! progress, and nothing here could see it, because nothing outside this file
//! had an opinion about what the target takes.
//!
//! [`sourced_paths`] is that opinion. R1840 built it and the comparison
//! ([`drift`]) beside it, and measured the two surfaces disagreeing about
//! seventy-one paths. **R1842 is the move itself**: the population below is
//! now the sourced one, leaf for leaf, and what this file still decides is the
//! *shape* each path holds ([`refinements`]). The source is
//! `docs/analyzer-config-surface.json` — outside this crate, on the same
//! footing as the specification pins the other screens are judged against
//! (`ls docs/analyzer-*-spec.json` says how many; this line first said THREE
//! and there were twelve, which is why it now names a command instead of a
//! number), and for the same stated reason: a specification written by the
//! same hand in the same edit as its subject means a check is asking the
//! subject for the answer.
//!
//! ⚠ **What the comparison found before the move, so a reader knows what it
//! was worth**: 9 of the 53 paths the source then held were named here. Not
//! because forty-four options were absent — several were here under a
//! *different spelling* (`discovery.multicast` against the source's
//! `discovery.multicast.enabled`, one TLS certificate against the source's
//! separate listen and connect ones) — and that was the finding rather than an
//! excuse. A configuration document is exported with these keys verbatim
//! ([`crate::deploy`]), so a paraphrased path is one the target would not
//! take, and until that comparison existed nothing could tell a rename from an
//! absence.
//!
//! ⚠⚠ **And R1842 found the pin itself reading the wrong list.** R1840 took
//! the surface from the reference's field *catalogue* — the keys its inspector
//! offers to add — which the reference's own text calls the frequently-used
//! ones put in front, explicitly *not* the boundary of the configuration. That
//! is the reference's palette, so the meter was dividing a palette by a
//! palette. The reference keeps the boundary separately, read from the
//! target's own start-up dump and used as its only gate against a mistyped
//! key; it is larger by more than a factor of two, and it is what the pin
//! holds now.

use std::sync::OnceLock;

use crate::graph::Role;
use pinion_core::widgets::config_form::FieldType;
use pinion_core::widgets::config_schema::{
    ConfigSchema, Reach, SchemaLeaf, StringCensus, SurfaceDrift,
};
use pinion_core::widgets::text_format::{CharClass, CharSet, Span, TextFormat};

/// A host: a dotted quad, or a name.
fn host() -> TextFormat {
    TextFormat::Either {
        of: vec![
            TextFormat::split('.', TextFormat::number(0, 255), Span::exactly(4)),
            TextFormat::Chars {
                allow: CharSet::of(&[CharClass::Letter, CharClass::Digit]).and("-."),
                len: Span::between(1, 253),
            },
        ],
    }
}

/// `<host>:<port>` — where something listens, without saying how it is reached.
fn endpoint() -> TextFormat {
    TextFormat::then(host(), ':', TextFormat::number(0, 65535))
}

/// `<transport>/<host>:<port>` — the addresses the canvas draws links between.
///
/// The transports are the palette's own legend, which is what makes a mistyped
/// one refusable: a link is authored between pins that agree on the transport,
/// so a word outside that set is an address no pin on this screen has.
fn address() -> TextFormat {
    TextFormat::then(
        TextFormat::word(&["tcp", "tls", "quic", "udp", "ws"]),
        '/',
        endpoint(),
    )
}

/// A slash-separated path with no wildcard in it.
///
/// The third format, and the one whose absence is least visible: a wildcard
/// here is accepted by every text box and refused by the thing that resolves
/// it, so the value looks right until the node starts.
fn plain_path() -> TextFormat {
    TextFormat::split(
        '/',
        TextFormat::Chars {
            allow: CharSet::of(&[CharClass::Letter, CharClass::Digit]).and("-_"),
            len: Span::between(1, 64),
        },
        Span::between(1, 16),
    )
}

/// A lower-case hexadecimal identifier.
fn ident() -> TextFormat {
    TextFormat::Chars {
        allow: CharSet::of(&[CharClass::LowerHex]),
        len: Span::between(1, 32),
    }
}

/// A list of the shape.
fn list_of(of: FieldType) -> FieldType {
    FieldType::List { of: Box::new(of) }
}

/// A string of the shape.
fn formatted(of: TextFormat) -> FieldType {
    FieldType::Formatted { of }
}

/// One of these words.
fn choice(of: &[&'static str]) -> FieldType {
    FieldType::Choice {
        of: of.iter().map(|w| (*w).into()).collect(),
    }
}

/// A whole number of milliseconds, up to ten minutes.
fn millis() -> FieldType {
    FieldType::Integer {
        min: 0,
        max: 600_000,
    }
}

/// The paths **no two documents may share a value at**.
///
/// ★★★★★ R1818 — a value two nodes both answer to is not an identity, and
/// until that round nothing said so: R1690 declared the SHAPE and the form
/// enforced it at the document boundary, so an unparseable id was refused by
/// name while a person typing the same id into two cards had both accepted in
/// silence. Shape is a property of a value and uniqueness is a property of a
/// set; the form is one document and cannot see its siblings, so the
/// declaration belongs here and the question is asked of every card at once in
/// `LabState::defects`.
///
/// ★ R1842 — a **declaration about a sourced path**, not a path of its own:
/// [`schema`] refuses to build if one of these is not on the surface, because
/// a uniqueness rule attached to a path the source dropped would stop being
/// enforced with nothing saying so.
const UNIQUE_PATHS: &[&str] = &["id"];

/// **The shapes this screen knows better than the source does.**
///
/// ★★★★★ R1842 — the half of the surface that stays this crate's, and the
/// reason the move did not simply delete this file. The source types each leaf
/// with one coarse word — `array`, `bool`, `number`, `object`, `string`, and
/// `unknown` for the three it cannot type at all — which is enough to keep the
/// target from refusing to start on a wrong type, and not enough to refuse the
/// values a person actually mistypes. [`FieldType`] is finer: an address with
/// a transport word in front of it, an identifier a parser reads, an integer
/// with a range, a word from a fixed set.
///
/// So the POPULATION is the source's and the SHAPE is refined here, per path.
/// Every entry is a claim that this tree knows something the coarse word does
/// not carry; a path with nothing to add is absent from this list and takes
/// [`coarse`].
///
/// ⚠ **A refinement keyed at a path the source does not declare is refused by
/// [`schema`]** rather than ignored. That is the failure this whole round is
/// about, one level down: a refinement whose key stops matching would silently
/// widen the leaf back to free text, and a meter that got *weaker* is exactly
/// the kind of drift that reads as nothing happening.
fn refinements() -> Vec<(&'static str, FieldType)> {
    vec![
        // Read by a parser, and this screen offered it as free text for three
        // rounds before R1690 said so.
        ("id", formatted(ident())),
        // The addresses the canvas draws links between.
        ("listen.endpoints", list_of(formatted(address()))),
        ("connect.endpoints", list_of(formatted(address()))),
        // A discovery address names where to shout, with no transport word.
        ("discovery.multicast.address", formatted(endpoint())),
        // A key space this tool's rows hold with no wildcard in it.
        ("namespace", formatted(plain_path())),
        // ★ The role words come from `Role::MODES` rather than being spelled
        // again here: the mode a card implies and the mode the target takes
        // are one set, and two spellings of one set is the defect this file
        // exists to remove.
        ("mode", choice(&Role::MODES)),
        ("routing.peer.mode", choice(&["peer_to_peer", "linkstate"])),
        ("control.default_permission", choice(&["deny", "allow"])),
        // The transports the palette's own legend has — a word outside that
        // set is an address no pin on this screen has.
        (
            "transport.link.protocols",
            list_of(choice(&["tcp", "tls", "quic", "udp", "ws"])),
        ),
        // Ranges. The source says `number` and stops there, so an out-of-range
        // value is a defect only where a bound is declared below.
        (
            "transport.link.tx.batch_size",
            FieldType::Integer { min: 0, max: 65535 },
        ),
        (
            "transport.link.rx.buffer_size",
            FieldType::Integer { min: 0, max: 65535 },
        ),
        (
            "discovery.multicast.ttl",
            FieldType::Integer { min: 1, max: 255 },
        ),
        (
            "transport.unicast.max_links",
            FieldType::Integer { min: 1, max: 1024 },
        ),
        ("listen.timeout_ms", millis()),
        ("connect.timeout_ms", millis()),
        ("queries.timeout_ms", millis()),
        ("routing.interests.timeout", millis()),
        ("discovery.delay", millis()),
        ("discovery.timeout", millis()),
    ]
}

/// The shape a source-declared type word means, with nothing refined.
///
/// ★ `object` and `unknown` become free text rather than being dropped, and
/// the census below then reports them in the class whose name says what they
/// are: a string this tool cannot check. Dropping them would take them out of
/// the denominator, which is the direction this round exists to close.
fn coarse(shape: &str) -> FieldType {
    match shape {
        "bool" => FieldType::Boolean,
        // No bound, because the source declares none. A per-path range is a
        // refinement, and where there is none the only refusal this can make
        // is "that is not a whole number".
        "number" => FieldType::Integer {
            min: 0,
            max: i64::MAX,
        },
        "array" => list_of(FieldType::Text),
        "string" | "object" | "unknown" => FieldType::Text,
        other => panic!("the sourced surface types a leaf `{other}`, which has no shape here"),
    }
}

/// **The option surface**, section by section.
///
/// ★★★★★ R1842 — **the population is [`sourced_paths`]'s**, leaf for leaf.
/// What this crate decides is the shape ([`refinements`]), and what it may not
/// decide any more is which paths exist. Before this round the list was
/// written here by hand and every meter divided by it, so the tool's coverage
/// was measured against the tool's own memory of its target.
///
/// Built once. The order is the source's, which is also the order
/// [`Reach::sentence`] names what is missing in.
///
/// # Panics
///
/// If a [`refinements`] entry or a [`UNIQUE_PATHS`] entry names a path the
/// source does not declare, or if the sourced paths cannot all be one document
/// (see [`ConfigSchema::new`]). All three are defects in this file or the pin
/// rather than states the running screen can reach, so they stop the first
/// test rather than the first save.
pub fn schema() -> &'static ConfigSchema {
    static SCHEMA: OnceLock<ConfigSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let refined = refinements();
        let sourced = |path: &str| sourced_surface().iter().any(|leaf| leaf.path == path);
        for (path, _) in &refined {
            assert!(
                sourced(path),
                "a shape is refined at {path}, which the sourced surface does not declare",
            );
        }
        for path in UNIQUE_PATHS {
            assert!(
                sourced(path),
                "uniqueness is declared at {path}, which the sourced surface does not declare",
            );
        }
        let leaves = sourced_surface()
            .iter()
            .map(|leaf| {
                let ty = refined
                    .iter()
                    .find(|(path, _)| *path == leaf.path)
                    .map_or_else(|| coarse(&leaf.shape), |(_, ty)| ty.clone());
                let declared = SchemaLeaf::new(leaf.path.clone(), ty);
                if UNIQUE_PATHS.contains(&leaf.path.as_str()) {
                    declared.unique()
                } else {
                    declared
                }
            })
            .collect();
        ConfigSchema::new(leaves).expect("the sourced option surface is a document")
    })
}

/// **The shape the palette must offer `path` at.**
///
/// `None` for a path the surface does not declare, which is not an error: a
/// key typed in by hand can be newer than this table, and the form already
/// reports it as an unknown key without blocking. It is free text until
/// somebody declares otherwise, because that is the only honest thing to say
/// about a string nothing knows the shape of.
pub fn shape_of(path: &str) -> Option<FieldType> {
    schema().ty(path).cloned()
}

/// [`shape_of`], or free text — what a row is typed with.
pub fn shape_or_free(path: &str) -> FieldType {
    shape_of(path).unwrap_or(FieldType::Text)
}

/// **How much of the surface a catalogue reaches.**
///
/// The catalogue is the union over every role's opening form and its offered
/// chips, which is what makes this a measurement of the *tool* rather than of
/// whichever node happens to be selected.
pub fn reach(catalogue: &[(&str, &FieldType)]) -> Reach {
    schema().reached_by_keys(catalogue)
}

/// **How much of the string surface is pinned down.**
pub fn strings() -> StringCensus {
    schema().strings()
}

// ── R1840: the surface, SOURCED ─────────────────────────────────────────────

/// The sourced option surface, as text, compiled in.
///
/// `include_str!` rather than a read at run time, for the reason every other
/// pin in this tree is compiled in: a source that goes missing must break the
/// build, not silently stop being compared. A comparison that answers "nothing
/// is missing" because it found no file is the failure mode this whole round
/// is about.
const SURFACE_JSON: &str = include_str!("../../../docs/analyzer-config-surface.json");

/// One leaf of the sourced surface: the path, and the **coarse** word the
/// source types it with.
///
/// The two are kept together because they arrive together and mean nothing
/// apart: a path with no type cannot be built into a leaf, and a type with no
/// path cannot be refined.
#[derive(Debug, Clone)]
pub struct SourcedLeaf {
    /// The configuration path, in the neutral spelling the pin records.
    pub path: String,
    /// The source's own word for what it holds — see [`coarse`] for the ones
    /// this tree understands.
    pub shape: String,
}

/// ★★★★★ R1840 — **the surface the TARGET declares**, read from outside this
/// crate, and since R1842 the population [`schema`] is built from.
///
/// R1690 wrote that population by hand, in this file, and registered the defect
/// in the same round: every meter over it divides by *what we wrote down*, and
/// a declaration that falls behind its target loses leaves from the
/// denominator, so the coverage figure RISES. Drift reads as progress.
///
/// The provenance is in the file — what it was extracted from, when, and the
/// fact that the reference it came from itself cites the target's own default
/// configuration document by line range, which is the derivation point R1690
/// said was missing.
///
/// # Panics
///
/// If the pin is not a surface — unreadable JSON, no `paths`, an entry with no
/// `path` or no `shape`, or a duplicate. All are defects in the pin rather
/// than states the running screen can reach, and all must stop the build
/// rather than quietly weaken the surface.
pub fn sourced_surface() -> &'static [SourcedLeaf] {
    static SURFACE: OnceLock<Vec<SourcedLeaf>> = OnceLock::new();
    SURFACE.get_or_init(|| {
        let doc: serde_json::Value =
            serde_json::from_str(SURFACE_JSON).expect("docs/analyzer-config-surface.json parses");
        let rows = doc["paths"]
            .as_array()
            .expect("the sourced surface has a `paths` array");
        let out: Vec<SourcedLeaf> = rows
            .iter()
            .map(|row| SourcedLeaf {
                path: row["path"]
                    .as_str()
                    .expect("every sourced entry names a path")
                    .to_owned(),
                shape: row["shape"]
                    .as_str()
                    .expect("every sourced entry names the shape the source types it with")
                    .to_owned(),
            })
            .collect();
        let mut seen: Vec<&str> = out.iter().map(|leaf| leaf.path.as_str()).collect();
        seen.sort_unstable();
        let unique = seen.len();
        seen.dedup();
        assert_eq!(
            unique,
            seen.len(),
            "the sourced surface names each path once"
        );
        assert!(
            !out.is_empty(),
            "a sourced surface of nothing measures nothing"
        );
        out
    })
}

/// The sourced surface's paths alone, sorted — what [`drift`] compares against.
///
/// Derived from [`sourced_surface`] rather than parsed a second time: two
/// readings of one file are two lists that can disagree.
pub fn sourced_paths() -> &'static [String] {
    static PATHS: OnceLock<Vec<String>> = OnceLock::new();
    PATHS.get_or_init(|| {
        let mut out: Vec<String> = sourced_surface()
            .iter()
            .map(|leaf| leaf.path.clone())
            .collect();
        out.sort();
        out
    })
}

/// **What this screen's surface and the target's declaration disagree about.**
///
/// The number that matters is `sourced_only`: paths the target takes which
/// this tool cannot say, each of which is missing from the denominator of
/// every meter above and therefore inflating all of them.
///
/// ★★★★★ R1842 — **both sides are empty now, and that is a structural claim
/// rather than a lucky measurement**: [`schema`] takes its population from
/// [`sourced_paths`], so the only way either list can grow again is a leaf
/// declared somewhere other than the pin. Keeping the comparison after the
/// move is the point — it is what makes that impossible to do quietly, and it
/// is the same instrument that measured 44 and 27 before the move.
pub fn drift() -> SurfaceDrift<'static> {
    schema().against(sourced_paths())
}

#[cfg(test)]
mod tests {
    use super::{address, endpoint, ident, plain_path, refinements, schema, shape_of, strings};
    use pinion_core::widgets::config_form::FieldType;

    /// The surface is a document, and it is big enough to be one.
    ///
    /// The size assertion is the load-bearing half: a meter over a five-leaf
    /// surface reports a full palette and says nothing, which is what a table
    /// written to make the number look good would be.
    #[test]
    fn r1690_the_option_surface_is_a_document_of_a_real_size() {
        let schema = schema();
        assert!(
            schema.leaves().len() >= 30,
            "a surface a palette can trivially cover is not a measurement: {}",
            schema.leaves().len(),
        );
        assert!(schema.roots().len() >= 12, "{:?}", schema.roots());
        // Built once and shared: two schemas would be two surfaces.
        assert!(std::ptr::eq(schema, super::schema()));
    }

    /// ★★★ R1690 — the identifier is a formatted string, and this is the
    /// assertion that says the screen's rows cannot go back to free text.
    ///
    /// It is the defect the schema was written to expose: this screen typed
    /// `id` as free text for its whole life, so a node named `zz!` was accepted
    /// by the form and refused by the thing it configures.
    #[test]
    fn r1690_the_identifier_is_not_free_text() {
        assert!(
            matches!(shape_of("id"), Some(FieldType::Formatted { .. })),
            "{:?}",
            shape_of("id"),
        );
        let FieldType::Formatted { of } = shape_of("id").expect("declared") else {
            unreachable!("asserted above")
        };
        assert!(of.judge("a1").acceptable());
        assert!(of.judge("zz").refused(), "a value the parser would refuse");
        assert!(of.judge("").refused() || !of.judge("").acceptable());
    }

    /// The three formats accept what the screen actually holds and refuse the
    /// near misses.
    ///
    /// Driven through the formats rather than through the schema so a failure
    /// names which shape is wrong.
    #[test]
    fn r1690_the_formats_take_this_screens_values() {
        for good in ["tcp/0.0.0.0:7447", "tcp/10.0.0.21:7449", "ws/host-a:1"] {
            assert!(address().judge(good).acceptable(), "{good}");
        }
        for bad in [
            "tcp/0.0.0.0",        // no port
            "sctp/0.0.0.0:7447",  // not a transport this screen has
            "tcp/0.0.0.0:99999",  // port out of range
            "tcp/0.0.0.0:7447/x", // trailing rubbish
        ] {
            assert!(!address().judge(bad).acceptable(), "{bad}");
        }
        assert!(endpoint().judge("224.0.0.1:7446").acceptable());
        assert!(!endpoint().judge("224.0.0.1").acceptable());
        assert!(plain_path().judge("group/one").acceptable());
        assert!(
            !plain_path().judge("group/*").acceptable(),
            "a wildcard is the value that looks right and does not resolve",
        );
        assert!(ident().judge("beef").acceptable());
        assert!(!ident().judge("BEEF").acceptable(), "one spelling only");
    }

    /// ★★★ R1690 — every string leaf is in exactly one class, and all three
    /// classes are populated.
    ///
    /// The second half is what stops the census being a tautology: a surface
    /// where every string is free would satisfy "exactly one class" and measure
    /// nothing.
    #[test]
    fn r1690_the_string_surface_uses_all_three_classes() {
        let census = strings();
        assert!(!census.choices.is_empty(), "{census:?}");
        assert!(!census.formats.is_empty(), "{census:?}");
        assert!(
            !census.free.is_empty(),
            "credentials and file paths have no shape this tool can check: {census:?}",
        );
        assert_eq!(
            census.total(),
            census.choices.len() + census.formats.len() + census.free.len(),
        );
        // The addresses are lists of a formatted string, and the census sees
        // through the list — a meter that looked only at scalars would report
        // this screen's two most important strings as no string surface at all.
        assert!(
            census.formats.iter().any(|p| p == "listen.endpoints"),
            "{:?}",
            census.formats,
        );
    }

    /// ★★★★★ R1840 — **the surface is sourced from outside this crate, and
    /// the two ratchets are what the sourcing bought.**
    ///
    /// R1690 declared the surface here, by hand, and registered the defect in
    /// the same round: every meter divides by it, so a declaration that falls
    /// behind its target loses leaves from the denominator and the coverage
    /// figure RISES. Nothing could see that, because nothing outside this file
    /// had an opinion about what the target takes.
    ///
    /// [`sourced_paths`] is that opinion, extracted from the behaviour
    /// reference's own field table — where the reference marks which of its
    /// rows are configuration paths and which are not, so the split is read
    /// rather than judged — and carrying the derivation point in the pin.
    ///
    /// # What the two numbers mean, and why they are pinned separately
    ///
    /// They are opposite claims and a single figure would average them:
    ///
    /// * `sourced_only` — the target declares it and this surface does not
    ///   name it. **A ceiling that must fall.** Each one is missing from every
    ///   meter's denominator.
    /// * `declared_only` — this surface names it and the source does not.
    ///   **Also a ceiling that must fall**, and for a reason the first
    ///   measurement made plain: these are not inventions, they are
    ///   PARAPHRASES. `discovery.multicast` against the source's
    ///   `discovery.multicast.enabled`, `routing.mode` against
    ///   `routing.peer.mode`, one `transport.link.tls.certificate` against the
    ///   source's separate listen and connect certificates. The surface was
    ///   written from a memory of the target rather than from it.
    ///
    /// ⚠ And that is why the drift was undetectable before this file existed:
    /// nothing could tell *we renamed it* from *we do not have it*. It matters
    /// because a configuration document is exported with these keys VERBATIM
    /// (`crate::deploy`), so a paraphrased path is one the target would not
    /// take.
    ///
    /// ⚠⚠ Both sides of the comparison are neutralised by the same
    /// conventions, so a mismatch here is a STRUCTURAL difference and not an
    /// artefact of the substitution. A gate that compared a neutral path with
    /// a confidential one would report the substitution as a defect forever.
    #[test]
    fn r1840_the_option_surface_is_sourced_and_its_drift_is_ratcheted() {
        let drift = super::drift();
        let (hit, total) = drift.covered();

        assert_eq!(
            total,
            super::sourced_paths().len(),
            "the denominator is the SOURCED surface, not this crate's own",
        );
        // ★★★★★ R1842 — a FLOOR on the source, and it is the hole the move
        // would otherwise open. Once the population comes from the pin, the two
        // drift lists below cannot see the pin itself losing a row: both sides
        // shrink together and the comparison stays empty, while every meter's
        // denominator falls and every coverage figure RISES. That is exactly
        // the defect this whole round is about, moved one file outward. So the
        // size of the source is a ratchet: it may grow when the reference is
        // re-read and it may not shrink without somebody lowering this number
        // and saying why.
        assert!(
            total >= 111,
            "\u{2605} the sourced surface has shrunk to {total} \u{2014} a \
             smaller denominator makes every meter on this screen read higher",
        );
        assert_eq!(
            hit, total,
            "\u{2605}\u{2605} R1842 \u{2014} the population IS the source's, so \
             every sourced path is declared: {hit} of {total}",
        );

        // ★ The ratchets, at their floor. Measured at R1840 on the first run of
        // this comparison as 44 and 27, and moved by R1842 rather than
        // re-measured: the population is taken from the source, so both sides
        // are empty by construction and the only way to raise one is to declare
        // a leaf somewhere other than the pin. That is exactly what this
        // assertion now refuses.
        assert!(
            drift.sourced_only.is_empty(),
            "\u{2605} paths the target takes and this surface cannot name: {:?}",
            drift.sourced_only,
        );
        assert!(
            drift.declared_only.is_empty(),
            "\u{2605} paths this surface names and the source does not: {:?}",
            drift.declared_only,
        );

        // ★★ The provenance, which is the half R1690 named as missing. A
        // sourced surface with no recorded derivation point is a second
        // hand-written list.
        let doc: serde_json::Value =
            serde_json::from_str(super::SURFACE_JSON).expect("the pin parses");
        assert!(
            doc["$extracted"]["on"].is_string() && doc["$extracted"]["from"].is_string(),
            "the pin records WHEN it was extracted and FROM WHAT",
        );
        assert!(
            doc["$extracted"]["cites_target_declaration"]
                .as_u64()
                .is_some_and(|n| n > 0),
            "and that the reference it came from cites the target's own \
             declaration \u{2014} the derivation point, recorded rather than assumed",
        );
        assert!(
            doc["$substituted"]["fact"].as_bool() == Some(true),
            "and that the vocabulary is substituted, which is the fact a \
             reader needs to know a path here is not the target's spelling",
        );

        // ★★★ And the rows the reference declares are NOT configuration, kept
        // rather than dropped: a path the target has no key for is a different
        // fact from a path we are missing, and a census that could not tell
        // them apart would report the first as a gap forever.
        let not_config = doc["not_config"]["names"]
            .as_array()
            .expect("the pin keeps what is not configuration");
        assert!(!not_config.is_empty());

        // ⚠ The two lists are NOT disjoint, and the first draft of this
        // assertion said they were. The reference's split is per ROW: a row is
        // configuration or it is not, and one word can be both on two
        // different rows — the target's own identity path on an
        // infrastructure row, and the argument a traffic program takes. So
        // what is asserted is that every overlap is DECLARED, which keeps the
        // check while admitting the fact it found.
        let overlap_declared: Vec<&str> = doc["not_config"]["$also_a_path"]["names"]
            .as_array()
            .expect("the pin declares which names are both")
            .iter()
            .map(|n| n.as_str().expect("a name"))
            .collect();
        for name in not_config {
            let name = name.as_str().expect("a name");
            if super::sourced_paths().iter().any(|p| p == name) {
                assert!(
                    overlap_declared.contains(&name),
                    "{name} is both a path and declared not-configuration, and \
                     the pin does not say so",
                );
            }
        }
        for name in &overlap_declared {
            assert!(
                super::sourced_paths().iter().any(|p| p == name)
                    && not_config.iter().any(|n| n.as_str() == Some(*name)),
                "{name} is declared to be both and is not \u{2014} a declared \
                 overlap that stopped being one is how this admission would \
                 rot into a licence",
            );
        }
    }

    /// ★★★★★ R1842 — **every refinement REACHES a leaf, and the coarse word is
    /// what the rest hold.**
    ///
    /// The gate one level below the drift ratchet, and the one the move
    /// created: the population cannot fall behind the source any more, so the
    /// remaining way for this file to stop describing its target is for a
    /// refinement's KEY to stop matching. Nothing would break — the leaf would
    /// simply take [`coarse`](super::coarse) and widen back to free text, and
    /// every meter would keep reporting it as covered. A meter that got weaker
    /// is drift in the direction that reads as nothing happening.
    ///
    /// [`schema`] refuses a refinement whose path is not sourced, so this
    /// asserts the other half: that the refinement is the shape the leaf ends
    /// up with. A build that accepted the key and dropped the shape would pass
    /// the first check and fail this one.
    #[test]
    fn r1842_a_refinement_is_the_shape_its_leaf_holds() {
        let refined = refinements();
        assert!(
            !refined.is_empty(),
            "a surface refined nowhere is the source's coarse words alone, and \
             the identifier would be free text again",
        );
        let mut keys: Vec<&str> = refined.iter().map(|(path, _)| *path).collect();
        keys.sort_unstable();
        let all = keys.len();
        keys.dedup();
        assert_eq!(all, keys.len(), "a path is refined once: {keys:?}");

        for (path, ty) in &refined {
            assert_eq!(
                shape_of(path).as_ref(),
                Some(ty),
                "{path} is refined and the schema does not hold that shape",
            );
        }

        // ★ And the other side: a path with no refinement holds the source's
        // coarse word. Asserted on a leaf the source types `bool`, which is the
        // arm a reader is least likely to check by hand.
        assert_eq!(
            shape_of("transport.unicast.lowlatency"),
            Some(FieldType::Boolean),
            "an unrefined leaf takes the shape the source's own type word means",
        );
    }

    /// ★★★ R1842 — the source's coarse vocabulary is **closed**, and a word
    /// outside it stops the build.
    ///
    /// The pin is edited by hand when the reference is re-read, and a typed
    /// word nobody mapped would otherwise have to be given some default —
    /// which is how `object` would quietly become free text without anybody
    /// choosing that. Every word the pin uses is asserted to be one this file
    /// answers, so a new one is a compile-stopping question rather than a
    /// silent widening.
    #[test]
    fn r1842_every_sourced_type_word_has_a_shape() {
        let known = ["bool", "number", "array", "string", "object", "unknown"];
        for leaf in super::sourced_surface() {
            assert!(
                known.contains(&leaf.shape.as_str()),
                "the pin types {} as `{}`, which this file has no shape for",
                leaf.path,
                leaf.shape,
            );
        }
        // Not a tautology only because the pin really uses more than one of
        // them: a surface typed `string` throughout would satisfy the loop and
        // measure nothing.
        let words: std::collections::BTreeSet<&str> = super::sourced_surface()
            .iter()
            .map(|leaf| leaf.shape.as_str())
            .collect();
        assert!(words.len() >= 4, "{words:?}");
    }
}
