//! R1687 — **what leaves the screen**: the configuration, and the script that
//! starts it.
//!
//! The reference groups these two under one heading and it is right to: they
//! are **one derivation rendered twice**, not two features. Doing either alone
//! would mean building the derivation and using half of it, and the half left
//! out is the half that would then drift.
//!
//! # ★★★★★ R1788 — the derivation moved to the framework, and this is what is
//! left
//!
//! Until R1788 the whole of it lived here: `Plan`, `Deployed`, the builder, the
//! document rendering and the script rendering, 371 lines inside one binary's
//! `src/`. That is a defect and not a filing question. The analysis-tool census
//! asks whether the **framework** can export a deployable configuration and
//! carried the row as a gap; a derivation living in an example cannot be
//! reached by a second consumer; and this project's standing rule is that the
//! deliverable is a crate rather than an example.
//!
//! It now lives in [`pinion_node_graph`] as [`Plan`](pinion_node_graph::Plan)
//! and its neighbours, and moving it bought the one thing it could not have
//! here: [`Document::deployment`](pinion_node_graph::Document::deployment)
//! derives the ORDER itself. While this screen read `launch_order` and handed
//! the sequence to a builder, nothing stopped the order and the artifact from
//! disagreeing — they are one derivation rendered twice and were a call apart.
//!
//! What stays is exactly what is **this screen's**: which program a role runs,
//! the seam that renders the form's own reason vocabulary into the sentence a
//! plan carries, the two toast sentences, and the latch that remembers what was
//! produced.
//!
//! # ★★★ Where the un-carried rows come from, and where they deliberately do
//! not
//!
//! The reference has two sources: a row whose value cannot be encoded, and a
//! row that is *not a configuration path at all* — its inspector holds a few
//! rows that are run-time arguments, and a row whose executable has no matching
//! argument is reported rather than hidden.
//!
//! **We have the first and cannot have the second**, and that is a property of
//! the model rather than a gap. [`ConfigField`](pinion_core::widgets::config_form::ConfigField)
//! is declared with *the key being the configuration path, verbatim* — there is
//! no second kind of row to fall out of the document. So the whole population
//! is what [`ConfigForm::compose`] names, and inventing a table of run-only
//! keys here would be inventing the very distinction the form was designed
//! without.
//!
//! # What this is not
//!
//! It is not a claim that a graph which produces a script will start. Whether a
//! node is *ready* when the next one dials it is a race no ordering closes —
//! the reference writes a wait into its own script and calls that the script's
//! business, and this does the same.

use pinion_core::widgets::config_form::ConfigForm;
use pinion_node_graph::{Configured, Uncarried};
use serde_json::Value;

use crate::graph::Role;

/// This screen's plan: the framework's, over the document a form composes.
pub type Plan = pinion_node_graph::Plan<Value>;

/// The program a role's node runs.
///
/// Two shapes, which is the reference's own split: the infrastructure roles are
/// all the same program told what to be by its configuration, and each traffic
/// role is its own program. That is not an implementation detail of theirs —
/// it is why the infrastructure roles share a configuration vocabulary and the
/// traffic ones do not.
#[must_use]
pub const fn program_of(role: Role) -> &'static str {
    match role {
        Role::Router | Role::Peer | Role::Client | Role::Store => "node",
        Role::Publisher => "publisher",
        Role::Subscriber => "subscriber",
        Role::Querier => "querier",
        Role::Responder => "responder",
    }
}

/// The seam: what one node's form composes to, in the shape a plan takes.
///
/// ★★ This is the whole of what the framework does not own, and it is one
/// mapping: [`Unexpressed::why`](pinion_core::widgets::config_form::Unexpressed)
/// is an enum whose vocabulary belongs to the **form**, and a plan needs a
/// sentence a reader can act on. Rendering it once here — rather than making a
/// pure-data crate depend on the widget layer to hold the enum — is what lets
/// [`pinion_node_graph`] own the derivation at all.
///
/// A node with no form still gets an entry: an empty document is a
/// configuration, and leaving the node out would put a hole in the plan's
/// `hosts` total.
#[must_use]
pub fn configured(form: Option<ConfigForm>) -> Configured<Value> {
    let Some(form) = form else {
        return Configured {
            document: Value::Object(serde_json::Map::new()),
            uncarried: Vec::new(),
        };
    };
    let composed = form.compose();
    Configured {
        document: composed.document,
        uncarried: composed
            .unexpressed
            .into_iter()
            .map(|row| Uncarried {
                key: row.key,
                shown: row.shown,
                why: row.why.sentence(),
            })
            .collect(),
    }
}

/// What the toast says after an export, and what it says after a script.
///
/// ★ Here rather than at the two call sites because the sentence is part of
/// what the operation *is*: the reference's own toast reports the count, the
/// un-carried rows and the verdict, and a person who reads only that has to be
/// able to tell a clean export from one that will not start.
#[must_use]
pub fn export_sentence(plan: &Plan, verdict: Option<&str>) -> String {
    let mut clauses = vec![format!("{} node configurations", plan.nodes().len())];
    let uncarried = plan.uncarried().len();
    if uncarried > 0 {
        clauses.push(format!("{uncarried} not expressed"));
    }
    clauses.push(verdict.map_or_else(|| "checks pass".to_string(), ToString::to_string));
    clauses.join(" · ")
}

/// The sentence a produced script is announced with.
#[must_use]
pub fn script_sentence(plan: &Plan) -> String {
    let hosts = plan.hosts().len();
    let mut clauses = vec![
        "launch script".to_string(),
        format!("{} processes", plan.nodes().len()),
    ];
    if hosts > 1 {
        clauses.push(format!("split across {hosts} hosts"));
    }
    clauses.join(" · ")
}

/// Both artifacts a screen has produced, or has not yet.
///
/// ★★ **Latched, not derived**, and that is the difference between an operation
/// and a read. "Produce the launch script" is a thing somebody *does*, and a
/// slot that always answered the current graph would make it a no-op with
/// nothing to witness — as well as losing the one fact a person wants from an
/// artifact, which is what the graph looked like when it was taken.
#[derive(Debug, Clone, Default)]
pub struct Produced {
    /// The last exported configuration, or `None` before any.
    pub config: Option<Value>,
    /// The last produced script.
    pub script: Option<String>,
}

impl Produced {
    /// What the wire answers for the `export` slot.
    ///
    /// Both halves are always present as keys so that a reader can tell "not
    /// produced" from "produced and empty" without knowing which operations
    /// this screen has — a null is an answer and a missing key is a question.
    #[must_use]
    pub fn wire(&self) -> Value {
        serde_json::json!({
            "config": self.config.clone().unwrap_or(Value::Null),
            "script": self
                .script
                .as_ref()
                .map_or(Value::Null, |s| Value::String(s.clone())),
        })
    }
}

/// What a node with no host frame runs on.
///
/// A node outside every frame still has to start somewhere, and calling that
/// somewhere by a name keeps the plan's `hosts` total — a plan with a hole in
/// it would be one whose script silently skipped a process.
///
/// 🟥 R1716 — this used to be a `host_lookup(frames, node)` beside it, and the
/// screen's map is keyed by the FRAME's node rather than by the card's, so
/// asking it about a card could only ever answer this default. Measured: the
/// exported plan put all eight nodes on `unplaced` while the canvas drew two
/// host frames. The walk now lives in one place (`LabState::frame_of`), and
/// what is left here is the word it falls back to.
pub const UNPLACED: &str = "unplaced";
