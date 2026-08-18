//! R1687 — **what leaves the screen**: the configuration, and the script that
//! starts it.
//!
//! The reference groups these two under one heading and it is right to: they
//! are **one derivation rendered twice**, not two features. Doing either alone
//! would mean building the derivation and using half of it, and the half left
//! out is the half that would then drift.
//!
//! The derivation is [`plan`]. It answers, for every node that is going to be
//! started, in the order they have to be started in:
//!
//! - the **configuration document** its form describes,
//! - the **host** it starts on, which on this screen is the frame it sits in,
//! - the **program** its role runs, and
//! - the rows of its form that the document **could not carry**.
//!
//! [`as_document`] renders that as one value an agent reads; [`as_script`]
//! renders it as text a person runs. Neither knows anything the other does not.
//!
//! # ★★★ Where `unexpressed` comes from, and where it deliberately does not
//!
//! The reference has two sources for it: a row whose value cannot be encoded,
//! and a row that is *not a configuration path at all* — its inspector holds a
//! few rows that are run-time arguments, and a row whose executable has no
//! matching argument is reported rather than hidden.
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

use pinion_core::widgets::config_form::{ConfigForm, Unexpressed};
use pinion_node_graph::{Bringup, NodeId};
use serde_json::{Map, Value};

use crate::graph::Role;

/// One node's place in the plan: everything both renderings need about it.
#[derive(Debug, Clone)]
pub struct Deployed {
    /// What the canvas calls it, which is also what its configuration file is
    /// named after.
    ///
    /// ★ The name and not the [`NodeId`]: a plan is a thing that leaves the
    /// screen, and an id is minted in authoring order and means nothing outside
    /// this process. A first draft carried both and nothing ever read the id.
    pub name: String,
    /// The host it starts on — this screen's frames are machines.
    pub host: String,
    /// The program its role runs.
    pub program: &'static str,
    /// Why it sits where it does in the order.
    pub standing: Bringup,
    /// The configuration document its form describes, carrying every row that
    /// could be expressed.
    pub config: Value,
    /// The rows that could not be, each with the reason.
    pub unexpressed: Vec<Unexpressed>,
}

/// The whole plan, in the order the graph is brought up.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// One entry per node that is started, ordered.
    pub nodes: Vec<Deployed>,
}

impl Plan {
    /// The hosts this plan spreads across, in the order they are first needed.
    ///
    /// First-needed and not sorted, because the script is written in launch
    /// order and a reader following it down the page meets them this way.
    #[must_use]
    pub fn hosts(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for entry in &self.nodes {
            if !seen.contains(&entry.host) {
                seen.push(entry.host.clone());
            }
        }
        seen
    }

    /// Every row the plan could not carry, with the node it belongs to.
    #[must_use]
    pub fn unexpressed(&self) -> Vec<(&str, &Unexpressed)> {
        self.nodes
            .iter()
            .flat_map(|entry| {
                entry
                    .unexpressed
                    .iter()
                    .map(move |row| (entry.name.as_str(), row))
            })
            .collect()
    }
}

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

/// Build the plan from the ordered nodes and what is known about each.
///
/// Takes the order rather than deriving it, so that the one caller who has a
/// [`Document`](pinion_node_graph::Document) asks it for the order and this
/// stays a function of its arguments — which is what lets the tests below drive
/// it without a graph.
pub fn plan(
    order: &[(NodeId, String, Bringup)],
    host_of: impl Fn(NodeId) -> String,
    role_of: impl Fn(NodeId) -> Option<Role>,
    form_of: impl Fn(NodeId) -> Option<ConfigForm>,
) -> Plan {
    let mut nodes = Vec::with_capacity(order.len());
    for (node, name, standing) in order {
        let Some(role) = role_of(*node) else {
            continue;
        };
        let composed = form_of(*node).map(|form| form.compose());
        nodes.push(Deployed {
            name: name.clone(),
            host: host_of(*node),
            program: program_of(role),
            standing: *standing,
            config: composed
                .as_ref()
                .map_or_else(|| Value::Object(Map::new()), |c| c.document.clone()),
            unexpressed: composed.map(|c| c.unexpressed).unwrap_or_default(),
        });
    }
    Plan { nodes }
}

/// The plan as one value: what an agent reads.
///
/// ★ The four sections are the reference's own, and the order is its own too —
/// `order` before `nodes` because the order is the part that cannot be got any
/// other way, and `unexpressed` last because it is normally empty and a reader
/// should not have to scroll past it.
#[must_use]
pub fn as_document(plan: &Plan) -> Value {
    let order: Vec<Value> = plan
        .nodes
        .iter()
        .map(|entry| {
            serde_json::json!({
                "node": entry.name,
                "host": entry.host,
                "program": entry.program,
                "standing": entry.standing.wire(),
            })
        })
        .collect();
    let mut nodes = Map::new();
    for entry in &plan.nodes {
        nodes.insert(entry.name.clone(), entry.config.clone());
    }
    let mut hosts = Map::new();
    for host in plan.hosts() {
        hosts.insert(
            host.clone(),
            Value::Array(
                plan.nodes
                    .iter()
                    .filter(|entry| entry.host == host)
                    .map(|entry| Value::String(entry.name.clone()))
                    .collect(),
            ),
        );
    }
    let unexpressed: Vec<Value> = plan
        .unexpressed()
        .into_iter()
        .map(|(node, row)| {
            serde_json::json!({
                "node": node,
                "key": row.key,
                "shown": row.shown,
                "why": row.why.sentence(),
            })
        })
        .collect();
    serde_json::json!({
        "order": order,
        "nodes": Value::Object(nodes),
        "hosts": Value::Object(hosts),
        "unexpressed": unexpressed,
    })
}

/// The plan as a script: what a person runs.
///
/// ★★ **It is split by host and says so.** One file cannot start processes on
/// two machines, so a plan spread across several is written as one script that
/// runs the part belonging to whichever host it is invoked on — which is the
/// reference's choice and the honest one. The alternative, silently emitting
/// only the first host's processes, produces a script that appears to work.
///
/// ★ The rows that could not be expressed are written into it as **comments**
/// rather than left out. A script is the artifact somebody keeps; a report that
/// lived only in a toast would be gone by the time it mattered.
#[must_use]
pub fn as_script(plan: &Plan) -> String {
    let mut lines: Vec<String> = Vec::new();
    let hosts = plan.hosts();
    lines.push("#!/usr/bin/env bash".to_string());
    lines.push("# Generated from the node graph. Do not edit — regenerate.".to_string());
    lines.push("set -euo pipefail".to_string());
    lines.push("OUT=${1:-./graph-run}; mkdir -p \"$OUT\"".to_string());
    lines.push("BIN=${BIN:-.}".to_string());
    lines.push(format!(
        "HOST=${{HOST:-{}}}",
        hosts.first().map_or("localhost", String::as_str)
    ));
    lines.push(String::new());

    for entry in &plan.nodes {
        lines.push(format!("cat > \"$OUT/{}.json\" <<'CONFIG'", entry.name));
        lines.push(
            serde_json::to_string_pretty(&entry.config)
                .unwrap_or_else(|_| entry.config.to_string()),
        );
        lines.push("CONFIG".to_string());
    }
    lines.push(String::new());
    lines.push(
        "trap 'kill $(cat \"$OUT/pids\" 2>/dev/null) 2>/dev/null; rm -f \"$OUT/pids\"' EXIT"
            .to_string(),
    );

    for host in &hosts {
        lines.push(String::new());
        lines.push(format!("if [ \"$HOST\" = \"{host}\" ]; then"));
        for entry in plan.nodes.iter().filter(|entry| &entry.host == host) {
            lines.push(format!(
                "  \"$BIN/{}\" -c \"$OUT/{}.json\" & echo $! >> \"$OUT/pids\"",
                entry.program, entry.name
            ));
            // The wait the ordering cannot supply — see the module header.
            lines.push("  sleep 0.4".to_string());
        }
        lines.push("fi".to_string());
    }
    lines.push(String::new());
    lines.push("wait".to_string());

    let unexpressed = plan.unexpressed();
    if !unexpressed.is_empty() {
        lines.push(String::new());
        lines.push("# These settings are on the screen and not in any file above:".to_string());
        for (node, row) in unexpressed {
            lines.push(format!(
                "#   {node} · {} = {} — {}",
                row.key,
                row.shown,
                row.why.sentence()
            ));
        }
    }
    if hosts.len() > 1 {
        lines.push(String::new());
        lines.push(format!(
            "# {} hosts — run this on each with HOST=<name>.",
            hosts.len()
        ));
    }
    lines.join("\n")
}

/// What the toast says after an export, and what it says after a script.
///
/// ★ Here rather than at the two call sites because the sentence is part of
/// what the operation *is*: the reference's own toast reports the count, the
/// unexpressed rows and the verdict, and a person who reads only that has to be
/// able to tell a clean export from one that will not start.
#[must_use]
pub fn export_sentence(plan: &Plan, verdict: Option<&str>) -> String {
    let mut clauses = vec![format!("{} node configurations", plan.nodes.len())];
    let unexpressed = plan.unexpressed().len();
    if unexpressed > 0 {
        clauses.push(format!("{unexpressed} not expressed"));
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
        format!("{} processes", plan.nodes.len()),
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
