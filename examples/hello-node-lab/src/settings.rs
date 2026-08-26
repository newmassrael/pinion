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
//! # ★★★★★ R1840 — and the third, which is about THIS FILE
//!
//! The two meters above divide by the table below, and the table below is
//! written **by hand, in the same crate as the screen it types, by whoever
//! last edited that screen**. R1690 built it that way and registered the
//! defect in the same round: a declaration that falls behind its target loses
//! leaves from the denominator, so the coverage figure RISES. Drift reads as
//! progress, and nothing here could see it, because nothing outside this file
//! had an opinion about what the target takes.
//!
//! [`sourced_paths`] is that opinion, and [`drift`] is the comparison. The
//! source is `docs/analyzer-config-surface.json` — outside this crate, on the
//! same footing as the specification pins the other screens are judged against
//! (`ls docs/analyzer-*-spec.json` says how many; this line first said THREE
//! and there were twelve, which is why it now names a command instead of a
//! number), and for the same stated reason: a specification written by the
//! same hand in the same edit as its subject means a check is asking the
//! subject for the answer.
//!
//! ⚠ **What the first run of that comparison measured, so a reader is not
//! surprised by it**: 9 of the 53 sourced paths are named here. Not because
//! forty-four options are absent — several are here under a *different
//! spelling* (`discovery.multicast` against the source's
//! `discovery.multicast.enabled`, one TLS certificate against the source's
//! separate listen and connect ones) — and that is the finding rather than an
//! excuse. A configuration document is exported with these keys verbatim
//! ([`crate::deploy`]), so a paraphrased path is one the target would not
//! take, and until this comparison existed nothing could tell a rename from an
//! absence.

use std::sync::OnceLock;

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

/// Any of these words.
fn flags(of: &[&'static str]) -> FieldType {
    FieldType::Flags {
        of: of.iter().map(|w| (*w).into()).collect(),
    }
}

/// **The option surface**, section by section.
///
/// Built once. The order is the order a section list would show, which is also
/// the order [`Reach::sentence`] names what is missing in.
pub fn schema() -> &'static ConfigSchema {
    static SCHEMA: OnceLock<ConfigSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        ConfigSchema::new(vec![
            // The node's own identity.
            //
            // ★★★★★ R1818 — `.unique()`, and the word IDENTITY above is why. A
            // value two nodes both answer to is not an identity, and until this
            // round nothing said so: R1690 declared the SHAPE and the form
            // enforced it at the document boundary, so an unparseable id was
            // refused by name while a person typing the same id into two cards
            // had both accepted in silence. Shape is a property of a value and
            // uniqueness is a property of a set; the form is one document and
            // cannot see its siblings, so the declaration belongs here and the
            // question is asked of every card at once in `LabState::defects`.
            SchemaLeaf::new("id", formatted(ident())).unique(),
            SchemaLeaf::new("label", FieldType::Text),
            SchemaLeaf::new("metadata.name", FieldType::Text),
            SchemaLeaf::new("metadata.note", FieldType::Text),
            // Where it listens and what it dials.
            SchemaLeaf::new("listen.endpoints", list_of(formatted(address()))),
            SchemaLeaf::new("connect.endpoints", list_of(formatted(address()))),
            SchemaLeaf::new(
                "connect.timeout_ms",
                FieldType::Integer {
                    min: 0,
                    max: 600_000,
                },
            ),
            SchemaLeaf::new(
                "open.retry_ms",
                FieldType::Integer {
                    min: 0,
                    max: 60_000,
                },
            ),
            // How it finds its neighbours without being told.
            SchemaLeaf::new("discovery.multicast", FieldType::Boolean),
            SchemaLeaf::new("discovery.address", formatted(endpoint())),
            SchemaLeaf::new("discovery.interface", FieldType::Text),
            // How traffic is carried across the graph.
            SchemaLeaf::new(
                "routing.mode",
                choice(&["peer_to_peer", "client", "router"]),
            ),
            SchemaLeaf::new("routing.hops", FieldType::Integer { min: 1, max: 64 }),
            SchemaLeaf::new("aggregation.prefixes", list_of(formatted(plain_path()))),
            SchemaLeaf::new("namespace", formatted(plain_path())),
            SchemaLeaf::new(
                "downsampling.rate",
                FieldType::Integer { min: 1, max: 1000 },
            ),
            // What is stamped on a message and what is squeezed.
            SchemaLeaf::new("timestamping.enabled", FieldType::Boolean),
            SchemaLeaf::new("timestamping.drop_future", FieldType::Boolean),
            SchemaLeaf::new("compression.enabled", FieldType::Boolean),
            SchemaLeaf::new("qos.priority", FieldType::Integer { min: 0, max: 7 }),
            SchemaLeaf::new("qos.congestion", choice(&["block", "drop"])),
            // The link layer.
            SchemaLeaf::new(
                "transport.link.tx.batch_size",
                FieldType::Integer { min: 0, max: 65535 },
            ),
            SchemaLeaf::new(
                "transport.link.tx.queue_depth",
                FieldType::Integer { min: 1, max: 1024 },
            ),
            SchemaLeaf::new(
                "transport.link.rx.buffer_size",
                FieldType::Integer { min: 0, max: 65535 },
            ),
            // Credentials and the files holding them: the free strings, and
            // the reason the free class is not empty. A path on somebody's
            // disk has no shape this tool can check.
            SchemaLeaf::new("transport.link.tls.certificate", FieldType::Text),
            SchemaLeaf::new("transport.link.tls.private_key", FieldType::Text),
            SchemaLeaf::new("transport.link.tls.root_authority", FieldType::Text),
            SchemaLeaf::new("transport.auth.user", FieldType::Text),
            SchemaLeaf::new("transport.auth.password", FieldType::Text),
            // Who may do what, and where that is asked.
            SchemaLeaf::new("control.permissions", flags(&["read", "write"])),
            SchemaLeaf::new("control.endpoint", formatted(endpoint())),
            SchemaLeaf::new("admin.enabled", FieldType::Boolean),
            SchemaLeaf::new("admin.permissions", flags(&["read", "write"])),
            SchemaLeaf::new(
                "queries.timeout_ms",
                FieldType::Integer {
                    min: 0,
                    max: 600_000,
                },
            ),
            // Extensions.
            SchemaLeaf::new("plugins.names", list_of(FieldType::Text)),
            SchemaLeaf::new("plugins.autoload", FieldType::Boolean),
        ])
        .expect("the option surface is a document")
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

/// ★★★★★ R1840 — **the paths the TARGET declares**, read from outside this
/// crate.
///
/// [`schema`] above is written by hand, in this file, by whoever last edited
/// the screen it types. R1690 built it that way and registered the defect in
/// the same round: every meter over it divides by *what we wrote down*, and a
/// declaration that falls behind its target loses leaves from the denominator,
/// so the coverage figure RISES. Drift reads as progress.
///
/// This is the other side of that comparison. Its provenance is in the file —
/// what it was extracted from, when, and the fact that the reference it came
/// from itself cites the target's own default configuration document by line
/// range, which is the derivation point R1690 said was missing.
///
/// # Panics
///
/// If the pin is not a surface — unreadable JSON, no `paths`, an entry with no
/// `path`, or a duplicate. All are defects in the pin rather than states the
/// running screen can reach, and all must stop the build rather than quietly
/// weaken the comparison.
pub fn sourced_paths() -> &'static [String] {
    static PATHS: OnceLock<Vec<String>> = OnceLock::new();
    PATHS.get_or_init(|| {
        let doc: serde_json::Value =
            serde_json::from_str(SURFACE_JSON).expect("docs/analyzer-config-surface.json parses");
        let rows = doc["paths"]
            .as_array()
            .expect("the sourced surface has a `paths` array");
        let mut out: Vec<String> = rows
            .iter()
            .map(|row| {
                row["path"]
                    .as_str()
                    .expect("every sourced entry names a path")
                    .to_owned()
            })
            .collect();
        let before = out.len();
        out.sort();
        out.dedup();
        assert_eq!(
            before,
            out.len(),
            "the sourced surface names each path once"
        );
        assert!(
            !out.is_empty(),
            "a sourced surface of nothing compares nothing"
        );
        out
    })
}

/// **What this screen's hand-written surface and the target's declaration
/// disagree about.**
///
/// The number that matters is `sourced_only`: paths the target takes which
/// this tool cannot say, each of which is missing from the denominator of
/// every meter above and therefore inflating all of them.
pub fn drift() -> SurfaceDrift<'static> {
    schema().against(sourced_paths())
}

#[cfg(test)]
mod tests {
    use super::{address, endpoint, ident, plain_path, schema, shape_of, strings};
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
        assert!(
            total > schema().leaves().len(),
            "\u{2605} the source is larger than the declaration \u{2014} which is \
             the whole finding: {total} sourced against {} declared",
            schema().leaves().len(),
        );

        // ★ The ratchets. Measured at R1840 on the first run of this
        // comparison; both may fall and neither may rise.
        assert!(
            drift.sourced_only.len() <= 44,
            "\u{2605} paths the target takes and this surface cannot name: {} \
             (ratchet 44) \u{2014} {:?}",
            drift.sourced_only.len(),
            drift.sourced_only,
        );
        assert!(
            drift.declared_only.len() <= 27,
            "\u{2605} paths this surface names and the source does not: {} \
             (ratchet 27) \u{2014} {:?}",
            drift.declared_only.len(),
            drift.declared_only,
        );
        assert!(
            hit > 0,
            "the two surfaces overlap at all, or the comparison is between \
             two vocabularies rather than two surfaces",
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
}
