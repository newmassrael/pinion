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
/// ★★★★★ R2078 — **THREE shapes, and this doc said two.**
///
/// It said *"two shapes, which is the reference's own split: the infrastructure
/// roles are all the same program told what to be by its configuration, and
/// each traffic role is its own program"*. Both halves are still true and the
/// sentence was still short, because the eight-role roster it was written
/// against could not reach the third case. Re-measured this round against the
/// pristine behaviour canon, the split is:
///
/// * **the four infrastructure roles share one program**, told what to be by
///   its configuration — the [`RoleSpec::mode`](crate::graph::RoleSpec) axis is
///   exactly that telling, which is why those four are the roles that have one;
/// * **fifteen roles are each their own program**, one declared binary apiece;
/// * **two roles have no program of their own at all** and run as the test
///   harness's driver. The canon declares them in its *driven* map and gives
///   them no entry in its *run* map, so its own derivation reaches for the
///   driver by falling through — the two are the discovery pair, which are
///   harness roles rather than deployment ones.
///
/// ⇒ ★ **the third shape was unreachable, not absent.** A count that cannot go
/// above two is not evidence that there are two, and this one had held still
/// since the roster did.
///
/// ⚠ The neutral names are this screen's, per role, and the pattern is the
/// existing one: a role's own program is its name in lower case. The canon's
/// binary names are protocol vocabulary and are not written here.
/// ★★★★★ R2086 — the program that runs a role which has none of its own, named
/// once because two things now read it: the program a plan row names, and the
/// decision about whether that row needs TELLING.
pub const DRIVER: &str = "driver";

/// ★★★★★ R2086 — **what a role's program is told, beyond its configuration.**
///
/// Empty for all but two, and that is the whole shape of it: a program of one's
/// own is told everything by the configuration file the plan writes, because
/// this screen's rows ARE configuration paths (see this module's header for why
/// there is no second kind of row here).
///
/// The two exceptions are the roles with no program of their own. [`DRIVER`]
/// runs whichever traffic role it is told to be, and **nothing in a
/// configuration says which**:
///
/// * `RoleSpec::mode` is a different axis — it is the SESSION mode a node runs
///   in (`router` / `peer` / `client`), it reaches the program through the
///   configuration document, and it is `None` for both of these roles. Deriving
///   the telling from it would hand the driver an empty argument and conflate
///   two facts, which is R1716's class exactly.
/// * so the telling is derived from the ROLE, as its own name in lower case —
///   the form every other word this screen puts on a wire takes (`deployment`,
///   `pattern`, `reference`, `locator`), where the badge is the three or four
///   letters a small box wears and not a word a person reads in a command.
///
/// ⚠ Measured before it was built, and the second measurement is the sharper
/// one. The crate that renders the script had **no** assertion about that line
/// at all; this screen had **one**, per node, since R1788 — and it was blind by
/// construction of its fixture. It built the expected line as *program then
/// `-c`*, which is exactly the broken form for a role whose program needs
/// telling, and the opening graph places no such role, so the shape it pinned
/// was always the shape it got. ⇒ ★ **an assertion can pin a defect instead of
/// catching it, and what decides which is the population its fixture creates.**
/// It derives the line from the row now, arguments included.
#[must_use]
pub fn arguments_of(role: Role) -> Vec<String> {
    if program_of(role) == DRIVER {
        vec!["--mode".to_owned(), role.name().to_lowercase()]
    } else {
        Vec::new()
    }
}

#[must_use]
pub const fn program_of(role: Role) -> &'static str {
    match role {
        // One program, told what to be. `RoleSpec::mode` is the telling.
        Role::Router | Role::Peer | Role::Client | Role::Store => "node",
        // ★ R2078 — no program of their own: these two exist to be driven, and
        // the harness driver is what runs them. Sharing a program with each
        // other is not the infrastructure case above — those share a program
        // and differ by configuration, and these two ARE the driver.
        Role::Scanner | Role::Member => DRIVER,
        // Fifteen roles, fifteen programs.
        Role::Publisher => "publisher",
        Role::Subscriber => "subscriber",
        Role::Puller => "puller",
        Role::Querier => "querier",
        Role::Responder => "responder",
        Role::Put => "put",
        Role::Delete => "delete",
        Role::Get => "get",
        Role::Retainer => "retainer",
        Role::Recoverer => "recoverer",
        Role::Roster => "roster",
        Role::Beacon => "beacon",
        Role::Watcher => "watcher",
        Role::Prober => "prober",
        Role::Forwarder => "forwarder",
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
