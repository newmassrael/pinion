//! `rpc/schema` — the SHAPE of every value this wire carries, declared on the
//! wire and gated against the types (R1539 §5.7 §5.12 §2 #2 §2 #7).
//!
//! # The gap this closes
//!
//! [`crate::methods`] discovers method NAMES. Its own module doc called the rest "the
//! natural next slice, added when a consumer needs it" — a defer on consumer
//! grounds, which [[the toolkit-parity-over-yagni]] does not admit, and which
//! R1538 then supplied a consumer for the hard way: it added `nodes_total` to [`crate::frame_timings::FrameTimingsMirror`] and
//! [`crate::frame_timings::FrameTimingsProduce`], and `r1465_mirror_work.py` — which asserts the EXACT key set those groups answer with —
//! went red in CI. Nothing between the edit and the push could see it: the
//! round's local gate runs the crates it changed and the demos it touched, and
//! that demo was neither.
//!
//! The defect is not the demo. It is that **a published response shape was not
//! written down anywhere a machine could check**, so growing one looked like
//! an ordinary struct edit. [`WIRE_TYPES`] writes it down, and
//! `census_matches_the_types` proves it true against the source — so the same
//! edit now fails in `pinion-rpc`'s own unit tests, in the round that makes it,
//! and the failure names the demos that assert on those very fields.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit's floor is meta-method: `parameterNames()`, `parameterTypes()` and `returnMetaType()` make a signature
//! discoverable at runtime. pinion offered method names and an OCC class and
//! nothing else, so it sat BELOW that floor.
//!
//! Two things here are past it:
//!
//! - **Structure, not just a type name.** `returnMetaType()` on a
//!   method answering with a variant map yields variant map — the keys are
//!   opaque, and a toolkit introspection client falls back to out-of-band
//!   documentation for them. A [`WireType`] states the key set, each key's JSON
//!   type, whether the key may be ABSENT, whether it may be `null`, and —
//!   through [`WireField::of`] — the named type nested at it, recursively
//!   (`$ref` into a definitions map, the shape JSON Schema uses, which is what
//!   lets `LayoutNode.children` name `LayoutNode` without the const cycle an
//!   inline census would need).
//! - **It is checked against the code.** `moc` generates the toolkit's meta-object from
//!   the declaration, and nothing anywhere asserts that a method returning
//!   variant map puts the documented keys in it — the toolkit's description cannot be
//!   wrong about the signature and cannot be right about the contents. Here
//!   `census_matches_the_types` parses the crate's own source and fails on any
//!   divergence, and `r1539_wire_states_its_shape.py` re-checks it a second,
//!   independent way: it CALLS methods and asserts the live response's key set
//!   equals the published census. An agent can verify the protocol description
//!   with the protocol.
//!
//! # What this census states, and what it does not
//!
//! It states, for every value this crate serializes: the **key set**, each
//! key's JSON type, whether the key may be absent, whether it may be `null`,
//! and **what type is nested at it**.
//!
//! It does NOT yet state **which method answers with which type**. That is the
//! one place the toolkit is still ahead — `returnMetaType()` binds a return type to a method
//! — and it is not published here because it cannot yet be published HONESTLY.
//! The census holds 28 `*Outcome` types against 91 routed methods, so most of the
//! surface builds its response with an ad-hoc `json!` / `Value` and has no type to
//! name. A `response` column would therefore be `null` for most methods, and an agent
//! reads a null return type as "answers with nothing", not as "not described
//! yet" — a description that is wrong is worse than one that is absent.
//! Binding them means giving those handlers response types first: a campaign,
//! and the named next slice of this axis, rather than a gap this table papers
//! over.
//!
//! It also does not state: the element type of an array of scalars
//! ([`WireTy::Array`] with no [`WireField::of`]); value ranges; or units.
//! [`WireTy::Any`] is an honest declaration of a genuinely dynamic value, not
//! a gap — six fields carry it, two of them naming [`WireField::of`]
//! `RequestId` (so they ARE described, by a [`WireShape::Scalar`] union), and
//! the other four are `serde_json::Value` payloads whose shape belongs to a
//! consumer rather than to this protocol.

use serde::{Deserialize, Serialize};

/// The JSON type a field's value takes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTy {
    /// A JSON number with no fractional part (Rust `u*` / `i*` / `usize`).
    Integer,
    /// A JSON number that may have one (Rust `f32` / `f64`).
    Number,
    /// A JSON string.
    String,
    /// A JSON boolean.
    Boolean,
    /// A JSON array. [`WireField::of`] names the element type when the
    /// elements are themselves censused; an array of scalars carries none.
    Array,
    /// A JSON object. [`WireField::of`] names its [`WireType`].
    Object,
    /// JSON `null`, as a member of a [`WireShape::Scalar`] union.
    Null,
    /// Deliberately unconstrained — a `serde_json::Value` whose shape belongs
    /// to a consumer rather than to this protocol.
    Any,
}

/// One key a [`WireShape::Object`] puts on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WireField {
    /// The serialized key, after any `#[serde(rename)]`.
    pub name: &'static str,
    /// `true` when the key may be ABSENT from a response — the field is
    /// `#[serde(skip_serializing_if = …)]`, so a missing key is a legal
    /// answer and not a protocol violation. A client must not require it.
    pub optional: bool,
    /// `true` when the key is always PRESENT but its value may be `null` — a
    /// bare `Option<T>` with no `skip_serializing_if`.
    ///
    /// Distinct from [`Self::optional`], and the distinction is the contract:
    /// an absent key and a `null` one are different answers, and a client that
    /// treats them alike breaks on exactly one of them. serde makes them look
    /// alike in Rust — both are `Option<T>` — which is why this census was
    /// initially wrong about twelve fields until the gate compared it with the
    /// source. The two are mutually exclusive UNLESS [`Self::patch`] says
    /// otherwise, and that exception is the reason the flag exists.
    pub nullable: bool,
    /// `true` when the key carries a THREE-state patch: absent leaves the axis
    /// alone, `null` clears it, and a value sets it.
    ///
    /// R1612.2 — the one shape that is legitimately both
    /// [`Self::optional`] and [`Self::nullable`], and it has to say so on the
    /// wire. A client reading both flags on a field cannot otherwise tell a
    /// deliberate three-state patch from someone having declared the same key
    /// two contradictory ways, and the invariant a reader checks becomes
    /// "either flag, never both" — which is exactly the check that went red
    /// when the first patch was published. Declaring it turns the rule from a
    /// prohibition into a BICONDITIONAL: both flags iff a patch, which catches
    /// the mistake in both directions instead of one.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub patch: bool,
    /// The JSON type of the value at this key. For a [`Self::nullable`] key,
    /// the type its value takes when it is not `null`.
    pub ty: WireTy,
    /// The censused type at this key — for [`WireTy::Object`] the object's own
    /// type, for [`WireTy::Array`] its ELEMENT type. Resolve it with
    /// [`wire_type`]. `None` means the value is a scalar or is
    /// [`WireTy::Any`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<&'static str>,
    /// R1616 — the CLOSED SET of strings this key accepts, when it takes one.
    ///
    /// A string-typed key whose value vocabulary is closed is not the same
    /// contract as one that takes any string, and a client cannot tell them
    /// apart from `ty` alone. Without this, the only way to learn a spelling
    /// is to guess one, be refused, and guess again — which is the shape
    /// R1566 already fixed for error payloads and R1610 immediately
    /// reintroduced one field over: an `UnknownLevel` refusal told a caller
    /// that its spelling was wrong and never what a right one looks like.
    ///
    /// **Not the same slot as [`Self::of`].** `of` names a censused TYPE and
    /// only a typed Rust field can carry one, so a key deliberately typed
    /// `String` — because its own parse must run before anything else in the
    /// message is applied, rather than aborting the whole frame at
    /// deserialization — has no way to reference an enum. This is the value
    /// half of the same question.
    ///
    /// The list must be **derived** from whatever owns the vocabulary, never
    /// retyped here: a second copy of a closed set is a second copy that goes
    /// stale silently. See `window_declare::LEVEL_WIRE_NAMES`, which is a
    /// `const` computed from the domain enum's own census.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<&'static [&'static str]>,
}

impl WireField {
    /// Declare a required key.
    #[must_use]
    pub const fn new(name: &'static str, ty: WireTy, of: Option<&'static str>) -> Self {
        Self {
            name,
            optional: false,
            nullable: false,
            patch: false,
            ty,
            of,
            values: None,
        }
    }

    /// R1616 — declare the closed set of strings this key accepts. See
    /// [`Self::values`]; pass a `const` derived from whatever owns the
    /// vocabulary rather than a list retyped here.
    #[must_use]
    pub const fn accepting(self, values: &'static [&'static str]) -> Self {
        Self {
            values: Some(values),
            ..self
        }
    }

    /// Mark the key as one a response may omit entirely.
    #[must_use]
    pub const fn optional(self) -> Self {
        Self {
            optional: true,
            ..self
        }
    }

    /// Mark the key as always present but permitted to carry `null`.
    #[must_use]
    pub const fn nullable(self) -> Self {
        Self {
            nullable: true,
            ..self
        }
    }

    /// Mark the key as a three-state patch: absent, `null`, or a value.
    ///
    /// Sets all three flags, because a patch IS both of the other two and
    /// leaving a caller to remember that is how the two readers of this census
    /// came to disagree.
    #[must_use]
    pub const fn patch(self) -> Self {
        Self {
            optional: true,
            nullable: true,
            patch: true,
            ..self
        }
    }
}

/// One arm of a [`WireShape::Union`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WireVariant {
    /// The value the union's discriminator key carries for this arm.
    pub name: &'static str,
    /// The keys this arm adds beside the discriminator.
    pub fields: &'static [WireField],
}

/// What a named wire type's JSON looks like.
///
/// Every arm is a STRUCT variant so the enum stays internally taggable —
/// serde cannot internally-tag a newtype variant holding a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireShape {
    /// A JSON object with exactly these keys.
    Object {
        /// The keys, in declaration order.
        fields: &'static [WireField],
    },
    /// A JSON string, always one of these values.
    Enum {
        /// The permitted strings.
        values: &'static [&'static str],
    },
    /// A JSON object carrying a discriminator key plus that arm's own keys —
    /// serde's internally-tagged representation.
    Union {
        /// The discriminator key.
        tag: &'static str,
        /// The arms, keyed by the discriminator's value.
        variants: &'static [WireVariant],
    },
    /// A bare JSON scalar that may take any of these types — serde's untagged
    /// representation over non-object variants.
    Scalar {
        /// The types the value may take.
        types: &'static [WireTy],
    },
}

/// A named type on the `pinion-rpc` wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WireType {
    /// The type's name — what a [`WireField::of`] reference resolves against.
    pub name: &'static str,
    /// Its JSON shape.
    pub shape: WireShape,
}

/// Every type this crate serializes onto the wire, sorted by name.
///
/// The SSOT for the published response contract, kept true to the Rust types
/// by `census_matches_the_types`, which parses this crate's own source and
/// asserts BOTH set-equality of the type names AND per-type equality of the
/// field lists. Adding, removing or renaming a serialized field without
/// editing this table fails that test.
pub const WIRE_TYPES: &[WireType] = &[
    WireType {
        name: "AcceleratorEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("accel", WireTy::String, None),
                WireField::new("layer", WireTy::String, None),
                WireField::new("target", WireTy::String, None),
                WireField::new("label", WireTy::String, None),
                WireField::new("shadowed", WireTy::Boolean, None),
                // Present and null when nothing claims the chord: "no widget"
                // is an answer, and omitting the key would read as "the
                // question was not asked".
                WireField::new("shadowed_by", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "AcceleratorsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("accelerators", WireTy::Array, Some("AcceleratorEntry")),
                WireField::new("focused", WireTy::String, None).nullable(),
                WireField::new("shadowing", WireTy::String, None).nullable(),
                WireField::new("probed", WireTy::String, None),
                // ABSENT rather than null when the request named no chord —
                // "not asked" and "asked, and it does nothing" are different
                // facts, and only the absence can say the first.
                WireField::new("chord", WireTy::Object, Some("ChordVerdict")).optional(),
            ],
        },
    },
    WireType {
        name: "AnchoredOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("kind", WireTy::String, None),
                WireField::new("declared", WireTy::String, None),
                WireField::new("display", WireTy::String, None).nullable(),
                WireField::new("at", WireTy::Array, None).nullable(),
            ],
        },
    },
    WireType {
        name: "AnimateControlOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("visited", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "AnimationStateOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("active", WireTy::Boolean, None),
                WireField::new("epsilon", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "AutoRepeatHoldOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("pointer", WireTy::Integer, None),
                WireField::new("target", WireTy::String, None),
                WireField::new("repeating", WireTy::Boolean, None),
                WireField::new("held_secs", WireTy::Number, None),
                WireField::new("fires", WireTy::Integer, None),
                // The five cadence keys are absent — not null — on a hold
                // that is not repeating: there is no cadence to state, and
                // a `0` would read as "fires instantly".
                WireField::new("delay_secs", WireTy::Number, None).optional(),
                WireField::new("interval_secs", WireTy::Number, None).optional(),
                WireField::new("accel", WireTy::Number, None).optional(),
                WireField::new("min_interval_secs", WireTy::Number, None).optional(),
                WireField::new("next_fire_in_secs", WireTy::Number, None).optional(),
            ],
        },
    },
    WireType {
        name: "AutoRepeatOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "holds",
                WireTy::Array,
                Some("AutoRepeatHoldOutcome"),
            )],
        },
    },
    WireType {
        name: "BlockFormatWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("left_indent_px", WireTy::Integer, None),
                WireField::new("right_indent_px", WireTy::Integer, None),
                WireField::new("space_above_px", WireTy::Integer, None),
                WireField::new("space_below_px", WireTy::Integer, None),
                WireField::new("heading_level", WireTy::Integer, None),
                // Absent as a level when the block is not a heading, which is a
                // different fact from level 0 — see `BlockFormat::aria_level`.
                WireField::new("aria_level", WireTy::Integer, None).nullable(),
            ],
        },
    },
    WireType {
        name: "CacheStatsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("hits", WireTy::Integer, None),
                WireField::new("misses", WireTy::Integer, None),
                WireField::new("paint_count", WireTy::Integer, None),
                WireField::new("entries", WireTy::Integer, None),
                WireField::new("hit_rate", WireTy::Number, None),
                WireField::new("last_damage_region", WireTy::Object, Some("CacheStatsRect"))
                    .optional(),
            ],
        },
    },
    WireType {
        name: "CacheStatsRect",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("w", WireTy::Integer, None),
                WireField::new("h", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "CaretStateOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("visible", WireTy::Boolean, None),
                WireField::new("enabled", WireTy::Boolean, None),
                WireField::new("period_secs", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "CellEditorEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("kind", WireTy::String, None),
                WireField::new("form", WireTy::String, None),
                WireField::new("buffer_is_text", WireTy::Boolean, None),
                WireField::new("inline_text", WireTy::Boolean, None),
                WireField::new("accepts_keystrokes", WireTy::Boolean, None),
                WireField::new("role", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "CellEditorsOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "editors",
                WireTy::Array,
                Some("CellEditorEntry"),
            )],
        },
    },
    WireType {
        name: "ChordVerdict",
        shape: WireShape::Object {
            fields: &[
                WireField::new("accel", WireTy::String, None),
                WireField::new("claimed_by", WireTy::String, None).nullable(),
                WireField::new("shadowed", WireTy::Boolean, None),
                WireField::new("shadowed_by", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "CmapSubtableInfo",
        shape: WireShape::Object {
            fields: &[
                WireField::new("platform_id", WireTy::Integer, None),
                WireField::new("encoding_id", WireTy::Integer, None),
                WireField::new("format", WireTy::Integer, None),
                WireField::new("supported", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "CmapSubtablesOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("version", WireTy::Integer, None),
                WireField::new("subtables", WireTy::Array, Some("CmapSubtableInfo")),
            ],
        },
    },
    WireType {
        name: "CmapSubtablesParams",
        shape: WireShape::Object {
            fields: &[WireField::new("font_id", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "ColorWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("r", WireTy::Integer, None),
                WireField::new("g", WireTy::Integer, None),
                WireField::new("b", WireTy::Integer, None),
                WireField::new("a", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "ComponentArgsInfo",
        shape: WireShape::Union {
            tag: "tag",
            variants: &[
                WireVariant {
                    name: "Offset",
                    fields: &[
                        WireField::new("x", WireTy::Integer, None),
                        WireField::new("y", WireTy::Integer, None),
                    ],
                },
                WireVariant {
                    name: "PointMatch",
                    fields: &[
                        WireField::new("parent", WireTy::Integer, None),
                        WireField::new("child", WireTy::Integer, None),
                    ],
                },
            ],
        },
    },
    WireType {
        name: "ComponentInfo",
        shape: WireShape::Object {
            fields: &[
                WireField::new("flags", WireTy::Integer, None),
                WireField::new("glyph_index", WireTy::Integer, None),
                WireField::new("args", WireTy::Object, Some("ComponentArgsInfo")),
                WireField::new("transform", WireTy::Object, Some("ComponentTransformInfo")),
            ],
        },
    },
    WireType {
        name: "ComponentTransformInfo",
        shape: WireShape::Union {
            tag: "tag",
            variants: &[
                WireVariant {
                    name: "Identity",
                    fields: &[],
                },
                WireVariant {
                    name: "Scale",
                    fields: &[WireField::new("scale", WireTy::Integer, None)],
                },
                WireVariant {
                    name: "XYScale",
                    fields: &[
                        WireField::new("x", WireTy::Integer, None),
                        WireField::new("y", WireTy::Integer, None),
                    ],
                },
                WireVariant {
                    name: "Matrix",
                    fields: &[
                        WireField::new("xx", WireTy::Integer, None),
                        WireField::new("xy", WireTy::Integer, None),
                        WireField::new("yx", WireTy::Integer, None),
                        WireField::new("yy", WireTy::Integer, None),
                    ],
                },
            ],
        },
    },
    WireType {
        name: "CoverageOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("id", WireTy::String, None),
                WireField::new("px", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "CrossWindowDropOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("drop", WireTy::Object, Some("ResolvedCrossWindowDrop")).nullable(),
            ],
        },
    },
    WireType {
        name: "CrossWindowDropParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Number, None),
                WireField::new("y", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "DeclaredWindow",
        shape: WireShape::Object {
            fields: &[
                WireField::new("id", WireTy::String, None),
                WireField::new("title", WireTy::String, None),
                WireField::new("position", WireTy::Array, None).nullable(),
                WireField::new("declared_size", WireTy::Array, None).nullable(),
                WireField::new("decorations", WireTy::Boolean, None),
                WireField::new("display", WireTy::String, None).nullable(),
                WireField::new("anchored", WireTy::Object, Some("AnchoredOutcome")).nullable(),
                WireField::new("level", WireTy::String, None)
                    .accepting(&crate::window_declare::LEVEL_WIRE_NAMES),
                WireField::new("level_outcome", WireTy::Object, Some("LevelOutcomeWire"))
                    .nullable(),
                // R1617 — which display the window is ACTUALLY on, per both
                // answerers. `anchored` above is the declaration's fate; this
                // is the window as it sits.
                WireField::new("display_home", WireTy::Object, Some("DisplayHomeWire")).nullable(),
            ],
        },
    },
    // R1629 — `scene/derivations`. The four kinds ride as an `accepting` set
    // derived from `DerivationKind::ALL`, so the census cannot promise a
    // vocabulary the answers do not carry.
    WireType {
        name: "DerivationSpanWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("start", WireTy::Integer, None),
                WireField::new("end", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "DerivationWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("kind", WireTy::String, None)
                    .accepting(&crate::derivations::KIND_WIRE_NAMES),
                WireField::new("source", WireTy::String, None)
                    .accepting(&crate::derivations::SOURCE_WIRE_NAMES),
                WireField::new("picture_has_more", WireTy::Boolean, None),
                WireField::new("name", WireTy::String, None),
                WireField::new("subject", WireTy::String, None).optional(),
                WireField::new("evidence", WireTy::Object, Some("EvidenceWire")),
                WireField::new("unit", WireTy::String, None).optional(),
                WireField::new("span", WireTy::Object, Some("DerivationSpanWire")).optional(),
            ],
        },
    },
    WireType {
        name: "DerivationsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("kind", WireTy::String, None),
                WireField::new("channel", WireTy::String, None)
                    .accepting(&crate::derivations::CHANNEL_WIRE_NAMES),
                WireField::new("published", WireTy::Boolean, None),
                WireField::new("domain", WireTy::String, None).optional(),
                WireField::new("derivations", WireTy::Array, Some("DerivationWire")).optional(),
                WireField::new("filter", WireTy::String, None)
                    .accepting(&crate::derivations::KIND_WIRE_NAMES)
                    .optional(),
            ],
        },
    },
    WireType {
        name: "DisabledEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("self_declared", WireTy::Boolean, None),
                WireField::new("declared_by", WireTy::String, None).nullable(),
                WireField::new("ink", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "DisabledOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "disabled",
                WireTy::Array,
                Some("DisabledEntry"),
            )],
        },
    },
    WireType {
        name: "DisplayAtOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("display", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "DisplayHomeWire",
        shape: WireShape::Object {
            fields: &[
                // R1617 — the vocabulary comes from the domain type that
                // PRODUCES it, and that type's own test proves the list is
                // exactly what its single constructor can emit. A hand list
                // here would be a second copy of a closed set (R1616).
                WireField::new("kind", WireTy::String, None)
                    .accepting(&pinion_core::display::DisplayHome::KINDS),
                WireField::new("derived", WireTy::String, None).nullable(),
                WireField::new("platform", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "DisplayOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("id", WireTy::String, None),
                WireField::new("label", WireTy::String, None),
                WireField::new("bounds", WireTy::Object, Some("DisplayRectOutcome")),
                WireField::new("scale", WireTy::Number, None),
                WireField::new("logical_size", WireTy::Object, Some("LogicalSizeOutcome")),
                WireField::new("refresh_mhz", WireTy::Integer, None).nullable(),
                WireField::new("primary", WireTy::Boolean, None),
                // R1621 — the usable region and its provenance travel together,
                // which is the type's whole point.
                WireField::new("usable", WireTy::Object, Some("UsableRegionWire")),
            ],
        },
    },
    WireType {
        name: "DisplayRectOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("w", WireTy::Integer, None),
                WireField::new("h", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "DisplaysOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("displays", WireTy::Array, Some("DisplayOutcome")),
                WireField::new("primary", WireTy::String, None).nullable(),
                WireField::new("fallback", WireTy::String, None).nullable(),
                WireField::new("bounding_box", WireTy::Object, Some("DisplayRectOutcome"))
                    .nullable(),
                WireField::new("covered_px", WireTy::Integer, None),
                WireField::new("gap_free", WireTy::Boolean, None),
                WireField::new("at", WireTy::Object, Some("DisplayAtOutcome")).optional(),
                WireField::new("placement", WireTy::Object, Some("PlacementOutcome")).optional(),
                WireField::new("anchored", WireTy::Object, Some("AnchoredOutcome")).optional(),
            ],
        },
    },
    WireType {
        name: "DisposeOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("existed", WireTy::Boolean, None)],
        },
    },
    WireType {
        name: "DisposeParams",
        shape: WireShape::Object {
            fields: &[WireField::new("font_id", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "DrawProfileOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("root", WireTy::Object, Some("DrawProfileRow")).nullable(),
                WireField::new("path", WireTy::String, None).nullable(),
                WireField::new("nodes", WireTy::Integer, None),
                WireField::new("nodes_total", WireTy::Integer, None),
                WireField::new("depth", WireTy::Integer, None).nullable(),
                WireField::new("heaviest_by", WireTy::String, None).nullable(),
                WireField::new("heaviest", WireTy::Array, Some("DrawProfileRank")),
            ],
        },
    },
    WireType {
        name: "DrawProfileRank",
        shape: WireShape::Object {
            fields: &[
                WireField::new("path", WireTy::String, None),
                WireField::new("kind", WireTy::String, None),
                WireField::new("tag", WireTy::String, None).nullable(),
                WireField::new("own", WireTy::Object, Some("DrawProfileWork")),
            ],
        },
    },
    WireType {
        name: "DrawProfileRow",
        shape: WireShape::Object {
            fields: &[
                WireField::new("path", WireTy::String, None),
                WireField::new("segment", WireTy::String, None).nullable(),
                WireField::new("kind", WireTy::String, None),
                WireField::new("tag", WireTy::String, None).nullable(),
                WireField::new("total", WireTy::Object, Some("DrawProfileWork")),
                WireField::new("own", WireTy::Object, Some("DrawProfileWork")),
                WireField::new("children", WireTy::Array, Some("DrawProfileRow")),
                WireField::new("children_omitted", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "DrawProfileWork",
        shape: WireShape::Object {
            fields: &[
                WireField::new("draws", WireTy::Integer, None),
                WireField::new("paths", WireTy::Integer, None),
                WireField::new("path_segments", WireTy::Integer, None),
                WireField::new("layers", WireTy::Integer, None),
                WireField::new("glyph_runs", WireTy::Integer, None),
                WireField::new("glyphs", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "ErrorEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("code", WireTy::Integer, None),
                WireField::new("message", WireTy::String, None),
                WireField::new("meaning", WireTy::String, None),
                WireField::new("data_is_prose", WireTy::Boolean, None),
                WireField::new("standard", WireTy::Boolean, None),
                WireField::new("data_vocabulary", WireTy::Array, None),
            ],
        },
    },
    WireType {
        name: "EvidenceWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("type", WireTy::String, None)
                    .accepting(&crate::derivations::EVIDENCE_WIRE_NAMES),
                WireField::new("name", WireTy::String, None).optional(),
                WireField::new("value", WireTy::Number, None).optional(),
                WireField::new("count", WireTy::Integer, None).optional(),
                WireField::new("flag", WireTy::Boolean, None).optional(),
            ],
        },
    },
    WireType {
        name: "ExportPdfOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("page_count", WireTy::Integer, None),
                WireField::new("page_width_pt", WireTy::Integer, None),
                WireField::new("page_height_pt", WireTy::Integer, None),
                WireField::new("object_count", WireTy::Integer, None),
                WireField::new("byte_len", WireTy::Integer, None),
                WireField::new("document", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "ExportPdfParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("page", WireTy::String, None).optional(),
                WireField::new("orientation", WireTy::String, None).optional(),
            ],
        },
    },
    WireType {
        name: "FamilyNameOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("name", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "FamilyNameParams",
        shape: WireShape::Object {
            fields: &[WireField::new("font_id", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "FocusSetParams",
        shape: WireShape::Object {
            fields: &[WireField::new("tag", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "FocusState",
        shape: WireShape::Object {
            fields: &[
                WireField::new("focused", WireTy::String, None).nullable(),
                WireField::new("tab_order", WireTy::Array, None).optional(),
            ],
        },
    },
    WireType {
        name: "FrameTimingsDraw",
        shape: WireShape::Object {
            fields: &[
                WireField::new("draws", WireTy::Integer, None),
                WireField::new("paths", WireTy::Integer, None),
                WireField::new("path_segments", WireTy::Integer, None),
                WireField::new("layers", WireTy::Integer, None),
                WireField::new("glyph_runs", WireTy::Integer, None),
                WireField::new("glyphs", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "FrameTimingsFocus",
        shape: WireShape::Object {
            fields: &[
                WireField::new("derivations_total", WireTy::Integer, None),
                WireField::new("retries_total", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "FrameTimingsLast",
        shape: WireShape::Object {
            fields: &[
                WireField::new("build_us", WireTy::Integer, None),
                WireField::new("encode_us", WireTy::Integer, None),
                WireField::new("acquire_us", WireTy::Integer, None),
                WireField::new("render_us", WireTy::Integer, None),
                WireField::new("gpu_us", WireTy::Integer, None).optional(),
                WireField::new("total_us", WireTy::Integer, None),
                WireField::new("other_us", WireTy::Integer, None),
                WireField::new("work_us", WireTy::Integer, None),
                WireField::new("settle_passes", WireTy::Integer, None),
                WireField::new("settled", WireTy::Boolean, None),
                WireField::new("shape_misses", WireTy::Integer, None),
                WireField::new("scene_nodes", WireTy::Integer, None),
                WireField::new("layout_nodes", WireTy::Integer, None),
                WireField::new("encode_nodes", WireTy::Integer, None),
                WireField::new("access_nodes", WireTy::Integer, None),
                WireField::new("draw", WireTy::Object, Some("FrameTimingsDraw")),
            ],
        },
    },
    WireType {
        name: "FrameTimingsMirror",
        shape: WireShape::Object {
            fields: &[
                WireField::new("scenes_total", WireTy::Integer, None),
                WireField::new("passes_total", WireTy::Integer, None),
                WireField::new("shape_misses_total", WireTy::Integer, None),
                WireField::new("unsettled_total", WireTy::Integer, None),
                WireField::new("nodes_total", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "FrameTimingsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("frame_count", WireTy::Integer, None),
                WireField::new("window_len", WireTy::Integer, None),
                WireField::new("last", WireTy::Object, Some("FrameTimingsLast")),
                WireField::new("window", WireTy::Object, Some("FrameTimingsWindow")),
                WireField::new("mean_fps", WireTy::Number, None),
                WireField::new("budget_us", WireTy::Integer, None).optional(),
                WireField::new("over_budget_frames", WireTy::Integer, None),
                WireField::new("worst_overrun_us", WireTy::Integer, None),
                WireField::new("jank_ratio", WireTy::Number, None),
                WireField::new("produce", WireTy::Object, Some("FrameTimingsProduce")),
                WireField::new("focus", WireTy::Object, Some("FrameTimingsFocus")),
                WireField::new("mirror", WireTy::Object, Some("FrameTimingsMirror")),
            ],
        },
    },
    WireType {
        name: "FrameTimingsProduce",
        shape: WireShape::Object {
            fields: &[
                WireField::new("passes_total", WireTy::Integer, None),
                WireField::new("shape_misses_total", WireTy::Integer, None),
                WireField::new("nodes_total", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "FrameTimingsWindow",
        shape: WireShape::Object {
            fields: &[
                WireField::new("min_total_us", WireTy::Integer, None),
                WireField::new("mean_total_us", WireTy::Integer, None),
                WireField::new("max_total_us", WireTy::Integer, None),
                WireField::new("mean_build_us", WireTy::Integer, None),
                WireField::new("mean_encode_us", WireTy::Integer, None),
                WireField::new("mean_acquire_us", WireTy::Integer, None),
                WireField::new("mean_render_us", WireTy::Integer, None),
                WireField::new("mean_gpu_us", WireTy::Integer, None).optional(),
                WireField::new("max_gpu_us", WireTy::Integer, None).optional(),
                WireField::new("gpu_sample_count", WireTy::Integer, None),
                WireField::new("gpu_timing_supported", WireTy::Boolean, None),
                WireField::new("gpu_dropped_total", WireTy::Integer, None),
                WireField::new("max_scene_nodes", WireTy::Integer, None),
                WireField::new("max_layout_nodes", WireTy::Integer, None),
                WireField::new("max_encode_nodes", WireTy::Integer, None),
                WireField::new("max_access_nodes", WireTy::Integer, None),
                WireField::new("max_draw", WireTy::Object, Some("FrameTimingsDraw")),
            ],
        },
    },
    WireType {
        name: "FullNameOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("name", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "GlyphHeaderInfo",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x_min", WireTy::Integer, None),
                WireField::new("y_min", WireTy::Integer, None),
                WireField::new("x_max", WireTy::Integer, None),
                WireField::new("y_max", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "GlyphIdForOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("glyph_id", WireTy::Integer, None).nullable()],
        },
    },
    WireType {
        name: "GlyphIdForParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("font_id", WireTy::Integer, None),
                WireField::new("codepoint", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "GlyphOutlineOutcome",
        shape: WireShape::Union {
            tag: "kind",
            variants: &[
                WireVariant {
                    name: "Empty",
                    fields: &[],
                },
                WireVariant {
                    name: "Simple",
                    fields: &[
                        WireField::new("header", WireTy::Object, Some("GlyphHeaderInfo")),
                        WireField::new("end_pts_of_contours", WireTy::Array, None),
                        WireField::new("instructions", WireTy::Array, None),
                        WireField::new("points", WireTy::Array, Some("GlyphPointInfo")),
                    ],
                },
                WireVariant {
                    name: "Composite",
                    fields: &[
                        WireField::new("header", WireTy::Object, Some("GlyphHeaderInfo")),
                        WireField::new("components", WireTy::Array, Some("ComponentInfo")),
                        WireField::new("instructions", WireTy::Array, None),
                    ],
                },
            ],
        },
    },
    WireType {
        name: "GlyphOutlineParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("font_id", WireTy::Integer, None),
                WireField::new("glyph_id", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "GlyphPointInfo",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("on_curve", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "GridDivergence",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("displayed_used_rows", WireTy::Integer, None),
                WireField::new("state_used_rows", WireTy::Integer, None),
                WireField::new("displayed_content_hash", WireTy::String, None),
                WireField::new("state_content_hash", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "GridEditorCell",
        shape: WireShape::Object {
            fields: &[
                WireField::new("row", WireTy::Integer, None),
                WireField::new("col", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "GridEditorEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("row", WireTy::Integer, None),
                WireField::new("col", WireTy::Integer, None),
                WireField::new("persistence", WireTy::String, None),
                WireField::new("kind", WireTy::String, None),
                WireField::new("form", WireTy::String, None),
                WireField::new("focused", WireTy::Boolean, None),
                WireField::new("dirty", WireTy::Boolean, None),
                WireField::new("malformed", WireTy::Boolean, None),
                // Null exactly when `malformed` — one `EditState` read decides
                // both, so the pair cannot disagree.
                WireField::new("value", WireTy::String, None).nullable(),
                WireField::new("seed", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "GridEditorsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("field_tag", WireTy::String, None),
                WireField::new("count", WireTy::Integer, None),
                WireField::new("focused", WireTy::Object, Some("GridEditorCell")).nullable(),
                WireField::new("editors", WireTy::Array, Some("GridEditorEntry")),
            ],
        },
    },
    WireType {
        name: "GridFidelityView",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("rect_w", WireTy::Integer, None),
                WireField::new("rect_h", WireTy::Integer, None),
                WireField::new("cols", WireTy::Integer, None),
                WireField::new("rows", WireTy::Integer, None),
                WireField::new("used_rows", WireTy::Integer, None),
                WireField::new("content_hash", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "GridSlotWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("row", WireTy::Integer, None),
                WireField::new("column", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "LayoutKind",
        shape: WireShape::Enum {
            values: &[
                "container",
                "box",
                "text",
                "path",
                "image",
                "external",
                "effect",
                "unknown",
            ],
        },
    },
    WireType {
        name: "LayoutNode",
        shape: WireShape::Object {
            fields: &[
                WireField::new("path", WireTy::String, None),
                WireField::new("kind", WireTy::String, Some("LayoutKind")),
                WireField::new("rect", WireTy::Object, Some("LayoutRect")),
                WireField::new("tag", WireTy::String, None).optional(),
                WireField::new("content", WireTy::String, None).optional(),
                WireField::new("line_count", WireTy::Integer, None),
                WireField::new("advance_px", WireTy::Integer, None),
                WireField::new("children", WireTy::Array, Some("LayoutNode")),
            ],
        },
    },
    WireType {
        name: "LayoutQueryParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("viewport", WireTy::Object, Some("ViewportSize")).optional(),
                WireField::new("path", WireTy::String, None).optional(),
            ],
        },
    },
    WireType {
        name: "LayoutRect",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("w", WireTy::Integer, None),
                WireField::new("h", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "LevelOutcomeWire",
        shape: WireShape::Object {
            fields: &[
                // R1617 — all three keys carry closed sets, and R1610 published
                // none of them. R1616 closed exactly this gap for the level a
                // caller WRITES; these are the three on the way back, and
                // knowing a match is safe is worth nothing without knowing what
                // to match. Every list is derived from the type that produces
                // it, never retyped here.
                WireField::new("kind", WireTy::String, None)
                    .accepting(&pinion_core::window_level::LevelOutcome::KINDS),
                WireField::new("declared", WireTy::String, None)
                    .accepting(&crate::window_declare::LEVEL_WIRE_NAMES),
                WireField::new("backend", WireTy::String, None)
                    .accepting(&crate::window_declare::BACKEND_WIRE_NAMES),
            ],
        },
    },
    WireType {
        name: "ListOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("font_ids", WireTy::Array, None)],
        },
    },
    WireType {
        name: "LocateRegionOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("paths", WireTy::Array, None),
                WireField::new("common_ancestor", WireTy::String, None),
                // R1591 — the question is repeated in its own answer. A
                // selection's result is only interpretable alongside the shape
                // and the fit it was taken with, and the toolkit records
                // neither: its rubber-band mode is a VIEW property, so nothing
                // says which mode a given selection used.
                WireField::new("shape", WireTy::String, None),
                WireField::new("fit", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "LogicalSizeOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("w", WireTy::Number, None),
                WireField::new("h", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "MarkRunWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("name", WireTy::String, None),
                WireField::new("start", WireTy::Integer, None),
                WireField::new("end", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "MarkStackWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("index", WireTy::Integer, None),
                WireField::new("names", WireTy::Array, None),
                // R1615 — always present when a position was named, null when
                // nothing covers it. "Nothing covers this byte" is an answer.
                WireField::new("top", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "MarksOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("kind", WireTy::String, None),
                // R1615 — the reason a `published: false` is worth reading.
                // Without it an agent cannot tell a node that could have named
                // its runs and did not from one whose kind has nothing to name.
                WireField::new("channel", WireTy::String, None),
                WireField::new("published", WireTy::Boolean, None),
                WireField::new("domain", WireTy::String, None).optional(),
                WireField::new("runs", WireTy::Array, Some("MarkRunWire")).optional(),
                WireField::new("at", WireTy::Object, Some("MarkStackWire")).optional(),
            ],
        },
    },
    WireType {
        name: "MemoryArena",
        shape: WireShape::Object {
            fields: &[
                WireField::new("arena", WireTy::String, None),
                WireField::new("window", WireTy::String, None).nullable(),
                WireField::new("bytes", WireTy::Integer, None),
                WireField::new("entries", WireTy::Integer, None),
                WireField::new("basis", WireTy::String, None),
                WireField::new("unmeasured", WireTy::Array, Some("MemoryUnmeasured")),
                WireField::new("budget_bytes", WireTy::Integer, None).nullable(),
            ],
        },
    },
    WireType {
        name: "MemoryOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("arenas", WireTy::Array, Some("MemoryArena")),
                WireField::new("total_bytes", WireTy::Integer, None),
                WireField::new("process_rss_bytes", WireTy::Integer, None).nullable(),
            ],
        },
    },
    WireType {
        name: "MemoryUnmeasured",
        shape: WireShape::Object {
            fields: &[
                WireField::new("type", WireTy::String, None),
                WireField::new("count", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "MethodEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("name", WireTy::String, None),
                WireField::new("occ", WireTy::String, Some("MethodOcc")),
                WireField::new("window", WireTy::String, Some("MethodWindow")),
            ],
        },
    },
    WireType {
        name: "MethodOcc",
        shape: WireShape::Enum {
            values: &["read", "mutate"],
        },
    },
    WireType {
        name: "MethodWindow",
        shape: WireShape::Enum {
            values: &["scope", "path"],
        },
    },
    WireType {
        name: "MetricsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("units_per_em", WireTy::Integer, None),
                WireField::new("ascender", WireTy::Integer, None),
                WireField::new("descender", WireTy::Integer, None),
                WireField::new("line_gap", WireTy::Integer, None),
                WireField::new("num_glyphs", WireTy::Integer, None),
                WireField::new("weight_class", WireTy::Integer, None),
                WireField::new("is_monospace", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "MetricsParams",
        shape: WireShape::Object {
            fields: &[WireField::new("font_id", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "MnemonicEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("key", WireTy::String, None),
                WireField::new("accel", WireTy::String, None),
                WireField::new("target", WireTy::String, None),
                WireField::new("label", WireTy::String, None),
                WireField::new("index", WireTy::Integer, None),
                WireField::new("ambiguous", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "MnemonicsOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "mnemonics",
                WireTy::Array,
                Some("MnemonicEntry"),
            )],
        },
    },
    WireType {
        name: "NormalizeForm",
        shape: WireShape::Enum {
            values: &["NFC", "NFD", "NFKC", "NFKD"],
        },
    },
    WireType {
        name: "NormalizeOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("text", WireTy::String, None)],
        },
    },
    WireType {
        name: "PaletteCatalogue",
        shape: WireShape::Object {
            fields: &[
                WireField::new("light", WireTy::Array, Some("ThemeTokenView")),
                WireField::new("dark", WireTy::Array, Some("ThemeTokenView")),
            ],
        },
    },
    WireType {
        name: "ParseOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("font_id", WireTy::Integer, None)],
        },
    },
    WireType {
        name: "ParseParams",
        shape: WireShape::Object {
            fields: &[WireField::new("bytes", WireTy::Array, None)],
        },
    },
    WireType {
        name: "PendingCommandView",
        shape: WireShape::Object {
            fields: &[
                WireField::new("kind", WireTy::String, None),
                WireField::new("payload", WireTy::Any, None),
                WireField::new("scope_id", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "PlacementOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("home", WireTy::String, None).nullable(),
                WireField::new("covering", WireTy::Array, Some("CoverageOutcome")),
                WireField::new("visible_px", WireTy::Integer, None),
                WireField::new("total_px", WireTy::Integer, None),
                WireField::new("offscreen_px", WireTy::Integer, None),
                WireField::new("fully_visible", WireTy::Boolean, None),
                WireField::new("visible_fraction", WireTy::Number, None),
                WireField::new("suggestion", WireTy::Array, None).nullable(),
            ],
        },
    },
    WireType {
        name: "PointerReachReport",
        shape: WireShape::Object {
            fields: &[
                WireField::new("deliverable", WireTy::Integer, None),
                WireField::new("inert", WireTy::Integer, None),
                WireField::new("shadows", WireTy::Array, Some("ShadowEntry")),
                WireField::new("unreachable", WireTy::Array, Some("UnreachableEntry")),
            ],
        },
    },
    WireType {
        name: "PostscriptNameOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("name", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "RenderFidelityOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("paint_seq", WireTy::Integer, None),
                WireField::new("presented_at_ms", WireTy::Integer, None),
                WireField::new("present_ok", WireTy::Boolean, None),
                WireField::new("viewport_w", WireTy::Integer, None),
                WireField::new("viewport_h", WireTy::Integer, None),
                WireField::new("displayed", WireTy::Array, Some("GridFidelityView")),
                WireField::new("state", WireTy::Array, Some("GridFidelityView")).optional(),
                WireField::new("diverged", WireTy::Boolean, None).optional(),
                WireField::new("divergences", WireTy::Array, Some("GridDivergence")),
            ],
        },
    },
    WireType {
        name: "Request",
        shape: WireShape::Object {
            fields: &[
                WireField::new("jsonrpc", WireTy::String, None),
                WireField::new("method", WireTy::String, None),
                WireField::new("params", WireTy::Any, None).optional(),
                WireField::new("id", WireTy::Any, Some("RequestId")).optional(),
            ],
        },
    },
    WireType {
        name: "RequestId",
        shape: WireShape::Scalar {
            types: &[WireTy::Integer, WireTy::String, WireTy::Null],
        },
    },
    WireType {
        name: "ResizeOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("width", WireTy::Integer, None),
                WireField::new("height", WireTy::Integer, None),
                WireField::new("requested", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "ResizeParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("width", WireTy::Integer, None),
                WireField::new("height", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "ResolvedCrossWindowDrop",
        shape: WireShape::Object {
            fields: &[
                WireField::new("window", WireTy::String, None),
                WireField::new("tag", WireTy::String, None),
                WireField::new("x_rel", WireTy::Number, None),
                WireField::new("y_rel", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "Response",
        shape: WireShape::Object {
            fields: &[
                WireField::new("jsonrpc", WireTy::String, None),
                WireField::new("result", WireTy::Any, None).optional(),
                WireField::new("error", WireTy::Object, Some("RpcError")).optional(),
                WireField::new("id", WireTy::Any, Some("RequestId")).nullable(),
            ],
        },
    },
    WireType {
        name: "RpcError",
        shape: WireShape::Object {
            fields: &[
                WireField::new("code", WireTy::Integer, None),
                WireField::new("message", WireTy::String, None),
                WireField::new("data", WireTy::Any, None).optional(),
            ],
        },
    },
    WireType {
        name: "RpcErrors",
        shape: WireShape::Object {
            fields: &[
                WireField::new("errors", WireTy::Array, Some("ErrorEntry")),
                WireField::new("count", WireTy::Integer, None),
                WireField::new("application_range", WireTy::Array, None),
                WireField::new("data_doc", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "RpcMethods",
        shape: WireShape::Object {
            fields: &[
                WireField::new("methods", WireTy::Array, Some("MethodEntry")),
                WireField::new("count", WireTy::Integer, None),
                WireField::new("occ_doc", WireTy::String, None),
                WireField::new("window_doc", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "RpcSchema",
        shape: WireShape::Object {
            fields: &[
                WireField::new("types", WireTy::Array, Some("WireType")),
                WireField::new("count", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "ScrollAxisPair",
        shape: WireShape::Object {
            fields: &[
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "ScrollEdges",
        shape: WireShape::Object {
            fields: &[
                WireField::new("at_top", WireTy::Boolean, None),
                WireField::new("at_bottom", WireTy::Boolean, None),
                WireField::new("at_left", WireTy::Boolean, None),
                WireField::new("at_right", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "ScrollStateOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("offset", WireTy::Object, Some("ScrollAxisPair")),
                WireField::new("max", WireTy::Object, Some("ScrollAxisPair")),
                WireField::new("edges", WireTy::Object, Some("ScrollEdges")),
                WireField::new("following_measured_tail", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "SetThemeModeOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("mode", WireTy::String, None),
                WireField::new("active", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "SetThemePalettesOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("mode", WireTy::String, None),
                WireField::new("system_scheme", WireTy::String, None),
                WireField::new("active", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "ShadowEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("path", WireTy::String, None),
                WireField::new("shadowed", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "SubfamilyNameOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("name", WireTy::String, None).nullable()],
        },
    },
    WireType {
        name: "SubscribeOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("subscription", WireTy::Integer, None),
                WireField::new("revision", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "SubscriptionView",
        shape: WireShape::Object {
            fields: &[
                WireField::new("subscription", WireTy::Integer, None),
                WireField::new("conn", WireTy::Integer, None),
                WireField::new("revision", WireTy::Integer, None),
                WireField::new("delivered_count", WireTy::Integer, None),
                WireField::new("armed", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "SubscriptionsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("subscriptions", WireTy::Array, Some("SubscriptionView")),
                WireField::new("published_total", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "TextBackgroundBand",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None).nullable(),
                WireField::new("start", WireTy::Integer, None),
                WireField::new("end", WireTy::Integer, None),
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("width", WireTy::Integer, None),
                WireField::new("height", WireTy::Integer, None),
                WireField::new("color", WireTy::Object, Some("ColorWire")),
                WireField::new("fg_color", WireTy::Object, Some("ColorWire")),
                // Absent for a translucent background — see `contrast_note`.
                WireField::new("contrast", WireTy::Number, None).nullable(),
                WireField::new("contrast_note", WireTy::String, None).optional(),
            ],
        },
    },
    WireType {
        name: "TextBackgroundsOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "bands",
                WireTy::Array,
                Some("TextBackgroundBand"),
            )],
        },
    },
    WireType {
        name: "TextBlockReport",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None).nullable(),
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("width", WireTy::Integer, None),
                WireField::new("height", WireTy::Integer, None),
                // Absent for a paragraph that declares only a text indent.
                WireField::new("block", WireTy::Object, Some("BlockFormatWire")).nullable(),
                WireField::new("text_indent", WireTy::Object, Some("TextIndentWire")),
                WireField::new("align", WireTy::String, None),
                WireField::new("lines", WireTy::Array, Some("TextLineWire")),
            ],
        },
    },
    WireType {
        name: "TextBlocksOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "blocks",
                WireTy::Array,
                Some("TextBlockReport"),
            )],
        },
    },
    WireType {
        name: "TextCacheStatsOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("shapes", WireTy::Integer, None),
                WireField::new("run_builds", WireTy::Integer, None),
                WireField::new("background_builds", WireTy::Integer, None),
                WireField::new("entries", WireTy::Integer, None),
                WireField::new("capacity", WireTy::Integer, None),
                WireField::new("max_capacity", WireTy::Integer, None),
                WireField::new("growths", WireTy::Integer, None),
                WireField::new("font_scans", WireTy::Integer, None),
                WireField::new("at_ceiling", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "TextCellWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("row_tag", WireTy::String, None),
                WireField::new("row", WireTy::Integer, None),
                WireField::new("column", WireTy::Integer, None),
                WireField::new("row_span", WireTy::Integer, None),
                WireField::new("column_span", WireTy::Integer, None),
                WireField::new("declared_row_span", WireTy::Integer, None),
                WireField::new("declared_column_span", WireTy::Integer, None),
                WireField::new("clamped", WireTy::Boolean, None),
                WireField::new("header", WireTy::String, None),
                WireField::new("index", WireTy::Integer, None),
                WireField::new("blocks", WireTy::Array, None),
                // Absent until the paint has laid the cell box out.
                WireField::new("x", WireTy::Integer, None).nullable(),
                WireField::new("y", WireTy::Integer, None).nullable(),
                WireField::new("width", WireTy::Integer, None).nullable(),
                WireField::new("height", WireTy::Integer, None).nullable(),
            ],
        },
    },
    WireType {
        name: "TextIndentWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("amount_px", WireTy::Integer, None),
                WireField::new("hanging", WireTy::Boolean, None),
                WireField::new("each_line", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "TextLineWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("start", WireTy::Integer, None),
                WireField::new("end", WireTy::Integer, None),
                WireField::new("x", WireTy::Number, None),
                WireField::new("y", WireTy::Number, None),
                WireField::new("advance", WireTy::Number, None),
                WireField::new("trailing_whitespace", WireTy::Number, None),
                WireField::new("height", WireTy::Number, None),
            ],
        },
    },
    WireType {
        name: "TextListItemWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                // Absent when the item's marker was painted by some other
                // composition than `view_document`'s.
                WireField::new("marker_tag", WireTy::String, None).nullable(),
                WireField::new("position", WireTy::Integer, None),
                WireField::new("ordinal", WireTy::Integer, None),
                WireField::new("marker", WireTy::String, None),
                WireField::new("rendered_as", WireTy::String, None),
                WireField::new("fell_back", WireTy::Boolean, None),
                WireField::new("marker_x", WireTy::Integer, None).nullable(),
                WireField::new("marker_y", WireTy::Integer, None).nullable(),
                WireField::new("marker_width", WireTy::Integer, None).nullable(),
                WireField::new("marker_height", WireTy::Integer, None).nullable(),
            ],
        },
    },
    WireType {
        name: "TextListWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                // Absent for a top-level list.
                WireField::new("parent_tag", WireTy::String, None).nullable(),
                WireField::new("level", WireTy::Integer, None),
                WireField::new("style", WireTy::String, None),
                WireField::new("start", WireTy::Integer, None),
                WireField::new("number_prefix", WireTy::String, None),
                WireField::new("number_suffix", WireTy::String, None),
                WireField::new("suffix_is_default", WireTy::Boolean, None),
                WireField::new("indent_px", WireTy::Integer, None),
                WireField::new("count", WireTy::Integer, None),
                // Absent until the paint has laid the container out.
                WireField::new("x", WireTy::Integer, None).nullable(),
                WireField::new("y", WireTy::Integer, None).nullable(),
                WireField::new("width", WireTy::Integer, None).nullable(),
                WireField::new("height", WireTy::Integer, None).nullable(),
                WireField::new("items", WireTy::Array, Some("TextListItemWire")),
            ],
        },
    },
    WireType {
        name: "TextListsOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new("lists", WireTy::Array, Some("TextListWire"))],
        },
    },
    WireType {
        name: "TextSelectionView",
        shape: WireShape::Object {
            fields: &[
                WireField::new("start", WireTy::Integer, None),
                WireField::new("end", WireTy::Integer, None),
                WireField::new("anchor", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "TextStateOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("text", WireTy::String, None),
                WireField::new("caret", WireTy::Integer, None),
                WireField::new("has_selection", WireTy::Boolean, None),
                WireField::new("selection", WireTy::Object, Some("TextSelectionView")).nullable(),
                WireField::new("is_composing", WireTy::Boolean, None),
                WireField::new("preedit", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "TextTableWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("rows", WireTy::Integer, None),
                WireField::new("columns", WireTy::Integer, None),
                WireField::new("cell_count", WireTy::Integer, None),
                WireField::new("header_rows", WireTy::Integer, None),
                WireField::new("header_columns", WireTy::Integer, None),
                WireField::new("column_widths", WireTy::Array, None),
                WireField::new("cell_padding_px", WireTy::Integer, None),
                WireField::new("cell_spacing_px", WireTy::Integer, None),
                WireField::new("border_px", WireTy::Integer, None),
                // Absent until the paint has laid the container out.
                WireField::new("x", WireTy::Integer, None).nullable(),
                WireField::new("y", WireTy::Integer, None).nullable(),
                WireField::new("width", WireTy::Integer, None).nullable(),
                WireField::new("height", WireTy::Integer, None).nullable(),
                WireField::new("slack", WireTy::Array, Some("GridSlotWire")),
                WireField::new("cells", WireTy::Array, Some("TextCellWire")),
            ],
        },
    },
    WireType {
        name: "TextTablesOutcome",
        shape: WireShape::Object {
            fields: &[WireField::new(
                "tables",
                WireTy::Array,
                Some("TextTableWire"),
            )],
        },
    },
    WireType {
        name: "ThemeTokenView",
        shape: WireShape::Object {
            fields: &[
                WireField::new("role", WireTy::String, None),
                WireField::new("color", WireTy::String, None),
            ],
        },
    },
    WireType {
        name: "ThemeTokensOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("mode", WireTy::String, None),
                WireField::new("system_scheme", WireTy::String, None),
                WireField::new("active", WireTy::String, None),
                WireField::new("palettes", WireTy::Object, Some("PaletteCatalogue")),
            ],
        },
    },
    WireType {
        name: "UnreachableEntry",
        shape: WireShape::Object {
            fields: &[
                WireField::new("tag", WireTy::String, None),
                WireField::new("path", WireTy::String, None),
                // `null` is a DIFFERENT fact from a tag: the centre resolves to
                // nothing at all (the widget is painted off-window), and the
                // repair is a scroll region rather than a declaration.
                WireField::new("blocked_by", WireTy::String, None).nullable(),
            ],
        },
    },
    WireType {
        name: "UnsubscribeOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("subscription", WireTy::Integer, None),
                WireField::new("delivered_count", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "UsableRegionWire",
        shape: WireShape::Object {
            fields: &[
                WireField::new("rect", WireTy::Object, Some("DisplayRectOutcome")),
                // R1616 — a closed value set says so and says which values. The
                // list is DERIVED from the arms (`UsableRegion::WIRE_NAMES`), so
                // it cannot go stale against the type it describes.
                WireField::new("provenance", WireTy::String, None)
                    .accepting(&pinion_core::display::UsableRegion::WIRE_NAMES),
            ],
        },
    },
    WireType {
        name: "ViewportSize",
        shape: WireShape::Object {
            fields: &[
                WireField::new("width", WireTy::Integer, None),
                WireField::new("height", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "WindowDeclareOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("window_id", WireTy::String, None),
                WireField::new("applied", WireTy::Array, None),
            ],
        },
    },
    WireType {
        // R1610 — the patch. Every axis but `window_id` is OPTIONAL, and the
        // two nullable ones are also NULLABLE, which is the whole point: an
        // absent key leaves the axis alone and an explicit null clears it, so
        // the census has to carry both facts per field or a client cannot tell
        // "say nothing" from "hand the window back to the window manager".
        name: "WindowDeclareParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("window_id", WireTy::String, None),
                WireField::new("title", WireTy::String, None).optional(),
                WireField::new("position", WireTy::Array, None).patch(),
                WireField::new("display", WireTy::String, None).patch(),
                WireField::new("decorations", WireTy::Boolean, None).optional(),
                WireField::new("level", WireTy::String, None)
                    .optional()
                    .accepting(&crate::window_declare::LEVEL_WIRE_NAMES),
            ],
        },
    },
    WireType {
        name: "WindowMoveOutcome",
        shape: WireShape::Object {
            fields: &[
                WireField::new("window_id", WireTy::String, None),
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
                WireField::new("requested", WireTy::Boolean, None),
            ],
        },
    },
    WireType {
        name: "WindowMoveParams",
        shape: WireShape::Object {
            fields: &[
                WireField::new("window_id", WireTy::String, None),
                WireField::new("x", WireTy::Integer, None),
                WireField::new("y", WireTy::Integer, None),
            ],
        },
    },
    WireType {
        name: "WireField",
        shape: WireShape::Object {
            fields: &[
                WireField::new("name", WireTy::String, None),
                WireField::new("optional", WireTy::Boolean, None),
                WireField::new("nullable", WireTy::Boolean, None),
                WireField::new("patch", WireTy::Boolean, None).optional(),
                WireField::new("ty", WireTy::String, Some("WireTy")),
                WireField::new("of", WireTy::String, None).optional(),
                // R1616 — the census describes itself, so the value-vocabulary
                // slot is on the wire the same way every other key it
                // publishes is.
                WireField::new("values", WireTy::Array, None).optional(),
            ],
        },
    },
    WireType {
        name: "WireShape",
        shape: WireShape::Union {
            tag: "kind",
            variants: &[
                WireVariant {
                    name: "object",
                    fields: &[WireField::new("fields", WireTy::Array, Some("WireField"))],
                },
                WireVariant {
                    name: "enum",
                    fields: &[WireField::new("values", WireTy::Array, None)],
                },
                WireVariant {
                    name: "union",
                    fields: &[
                        WireField::new("tag", WireTy::String, None),
                        WireField::new("variants", WireTy::Array, Some("WireVariant")),
                    ],
                },
                WireVariant {
                    name: "scalar",
                    fields: &[WireField::new("types", WireTy::Array, Some("WireTy"))],
                },
            ],
        },
    },
    WireType {
        name: "WireTy",
        shape: WireShape::Enum {
            values: &[
                "integer", "number", "string", "boolean", "array", "object", "null", "any",
            ],
        },
    },
    WireType {
        name: "WireType",
        shape: WireShape::Object {
            fields: &[
                WireField::new("name", WireTy::String, None),
                WireField::new("shape", WireTy::Object, Some("WireShape")),
            ],
        },
    },
    WireType {
        name: "WireVariant",
        shape: WireShape::Object {
            fields: &[
                WireField::new("name", WireTy::String, None),
                WireField::new("fields", WireTy::Array, Some("WireField")),
            ],
        },
    },
];

/// Resolve a [`WireField::of`] reference to its type.
#[must_use]
pub fn wire_type(name: &str) -> Option<&'static WireType> {
    WIRE_TYPES.iter().find(|t| t.name == name)
}

/// Response payload for `rpc/schema`: every wire type, with its shape.
///
/// Serialize-only, unlike [`crate::methods::RpcMethods`]. The census holds
/// `&'static` slices — it is a compile-time table, never something a client
/// sends back — and serde cannot derive `Deserialize` for a borrowed slice. A
/// consumer reads this as JSON; nothing round-trips a schema into Rust.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RpcSchema {
    /// Every censused type, sorted by name — [`WIRE_TYPES`].
    pub types: &'static [WireType],
    /// `types.len()`, so a client need not re-count.
    pub count: usize,
}

/// Build the `rpc/schema` response.
#[must_use]
pub fn rpc_schema() -> RpcSchema {
    RpcSchema {
        types: WIRE_TYPES,
        count: WIRE_TYPES.len(),
    }
}
