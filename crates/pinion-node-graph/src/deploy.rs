//! R1687 — **the order the graph is STARTED in**, which is not the order it
//! runs in.
//!
//! [`run`](crate::run) answers *which node gets control next, inside one
//! instant*. This answers *which process has to be up before which other one
//! can reach it* — a question about a graph whose nodes are **peers on a
//! network** rather than steps in a walk. The two never meet: a control walk
//! has a single entry and a stack, and a deployment has neither.
//!
//! # The rule, and why it is the link direction
//!
//! A link here means *this end reaches out to that one*. So a node nothing
//! reaches out to has nobody to wait for, and a node everything reaches out to
//! has to be standing first. The order falls out of two questions asked of the
//! links alone:
//!
//! | is dialled | dials out | [`Bringup`] | why |
//! |---|---|---|---|
//! | yes | no | [`Bringup::First`] | somebody dials it and it dials nobody |
//! | yes | yes | [`Bringup::Between`] | it is dialled *and* it dials |
//! | no | yes | [`Bringup::Last`] | it only dials, so everything it needs is up |
//! | no | no | [`Bringup::Alone`] | nothing to wait for and nothing waiting |
//!
//! # ★★★ The order is topological, and the four words above are only the REASON
//!
//! The first draft ordered by those four buckets alone and justified it with
//! *"a topological sort would have to refuse, because two peers that dial each
//! other are a legal mesh"*. **Both halves were wrong, and the model said so:**
//! [`Document::connect`](crate::Document::connect) refuses a cycle outright
//! (`WouldCycle`), so an authored tree is acyclic by construction — mutual
//! reachability lives in the *observed* layer ([`crate::observed`]), which is
//! not authored and not deployed.
//!
//! And with the excuse gone the bucketing was simply incorrect. In a chain of
//! four, `a → b → c → d`, both `b` and `c` are "reached and reaching", so a
//! bucket puts them in the same class and the tie-break — a NAME — decides
//! their order. But `b` reaches out to `c`: `c` has to be standing first, and a
//! name cannot know that. Three links deep the buckets are right by luck;
//! four deep they are wrong.
//!
//! So the walk is Kahn's, over the reversed link direction, and the buckets
//! stay as the *reported reason* a node sits where it does — which is what a
//! reader wants and what the order alone cannot say.
//!
//! **Deterministic**: among the nodes that are ready at the same moment the
//! next one is chosen by display NAME, never by id — an id is minted in
//! authoring order, so ordering by it would make the plan a fact about the
//! sequence somebody happened to draw the graph in.
//!
//! **Total anyway.** A cycle cannot be authored, but this does not depend on
//! that: whatever the walk cannot place goes at the end in name order rather
//! than being dropped. A deployment that silently omitted a node would be worse
//! than one that ordered it badly, and a model invariant is not a reason to
//! make a second thing depend on it.
//!
//! # ★★ A node that is switched OFF is not started, and neither are its links
//!
//! [`Node::disabled`](crate::Node::disabled) means *this node produces
//! nothing* (R1682) — as against bypassing, which passes its input through and
//! leaves the graph below alive. Starting a process for it would contradict the
//! only thing the switch says, so it is not in the order at all.
//!
//! ★ **Its links go with it**, and that is not the same statement. In `a → b →
//! c` with `b` off, `a` never dialled `c` — the two links are separate and both
//! ended at `b` or began there. So with `b` gone `a` and `c` have nothing
//! between them and are ordered by name, which is exactly right: neither has to
//! wait for the other. A walk that kept the edges would invent a constraint
//! from a node that is not being started.
//!
//! What is left out is therefore **derivable and not hidden**: it is every node
//! of the tree this does not name, and a caller wanting to report the omission
//! reads `tree.nodes()` against the answer. That is a subtraction with one
//! obvious spelling, which is why there is no second call for it.
//!
//! # What it is NOT
//!
//! It is not a claim that the order is sufficient. Whether a node is *ready*
//! when the next one dials it is a race no ordering closes — the reference tool
//! this shape comes from writes a wait into the script it generates, and that
//! is the script's business. This answers only the part that is a fact about
//! the graph.

use core::fmt;

use serde::Serialize;

use crate::model::{Document, NodeId, NodeKind, TreeId};

/// Where a node stands in the order the graph is brought up.
///
/// Ordered, and the derive is the declaration: `First < Between < Last <
/// Alone`. A node nobody waits for is last on purpose — it is the only bucket
/// whose members can be brought up in any order at all, so putting it anywhere
/// else would suggest a constraint that is not there.
///
/// ★ Named `Bringup` and not `Standing`, which this crate already uses for
/// *whether an answer about the drawn graph can be trusted as an answer about
/// the world* (R1645). Two unrelated facts under one word is how a reader comes
/// to believe a deployment order says something about discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bringup {
    /// Dialled by others and dials nobody: it has to be up first.
    First,
    /// Dialled by others and dials others.
    Between,
    /// Dials others and is dialled by nobody.
    Last,
    /// Neither end of any link.
    Alone,
}

impl Bringup {
    /// Every arm, so a census counts against the type rather than a list.
    pub const ALL: [Self; 4] = [Self::First, Self::Between, Self::Last, Self::Alone];

    /// The word this standing goes onto a wire as.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Between => "between",
            Self::Last => "last",
            Self::Alone => "alone",
        }
    }

    /// The standing of a node that is dialled by others / dials others, as
    /// given.
    ///
    /// ★ The two words are `dialled` and `dials` rather than `reached` and
    /// `reaches`, which is the module header's own vocabulary — and the pair it
    /// replaces was near enough alike that the lint refused it, correctly: two
    /// booleans one letter apart, in that order, is a call nobody can read.
    #[must_use]
    pub const fn of(dialled: bool, dials: bool) -> Self {
        match (dialled, dials) {
            (true, false) => Self::First,
            (true, true) => Self::Between,
            (false, true) => Self::Last,
            (false, false) => Self::Alone,
        }
    }
}

/// R1788 — one row a node's configuration **could not carry** into the plan.
///
/// A boundary type rather than the form's own: the reason a row was left out is
/// vocabulary the *form* owns, and this crate is pure data with no widget
/// layer. What a plan needs is that there ARE such rows, which node they belong
/// to, and a sentence a reader can act on — so the caller renders its reason
/// once, at the seam, and this carries the result.
///
/// # The floor this exists against
///
/// Built and run at 6.11 across **two processes**, which is the population a
/// persistence claim has to be measured over: handed a value it cannot encode,
/// the reference's settings store reports `NoError` on write, `NoError` on
/// sync, and `NoError` on read; the file keeps the type NAME and no payload;
/// and reading it back **in the writing process appears to succeed**, because a
/// process-wide cache answers instead of the file. In a second process the
/// value is invalid and default-constructed — and `status()` is still
/// `NoError`. The only signal, in either direction, is a line on stderr.
///
/// So the row is lost silently, and the in-process test a developer would write
/// passes. A plan that says nothing about what it dropped is that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncarried {
    /// The configuration path the row is addressed by.
    pub key: String,
    /// The value verbatim, as the row shows it.
    pub shown: String,
    /// Why it could not be carried, already a sentence.
    pub why: String,
}

/// R1788 — what a caller knows about one node's configuration.
///
/// The document is whatever the caller's configuration IS — this crate never
/// looks inside it, and only asks that it can be serialised when a rendering
/// needs that. Keeping it generic is what lets a pure-data crate own the
/// derivation without taking a dependency on the widget layer that composes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Configured<D> {
    /// The configuration document, carrying every row that could be expressed.
    pub document: D,
    /// The rows that could not be, each with the reason.
    pub uncarried: Vec<Uncarried>,
}

/// R1788 — one node's place in the plan: everything both renderings need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployed<D> {
    /// What the canvas calls it, which is also what its configuration file is
    /// named after.
    ///
    /// ★ The name and not the [`NodeId`]: a plan is a thing that LEAVES the
    /// screen, and an id is minted in authoring order and means nothing outside
    /// the process that minted it.
    pub name: String,
    /// The host it starts on.
    pub host: String,
    /// The program it runs.
    pub program: String,
    /// Why it sits where it does in the order.
    pub standing: Bringup,
    /// Its configuration, and what that configuration could not carry.
    pub config: Configured<D>,
}

/// R1788 — the whole plan, in the order the graph is brought up.
///
/// # Why this is in the crate and not in a screen
///
/// It was in an example until R1788, which is a real defect and not a filing
/// question: the analysis-tool census asks whether the FRAMEWORK can export a
/// deployable configuration, a derivation living in one binary's `src/` cannot
/// be reached by a second consumer, and this project's standing rule is that
/// the deliverable is a crate rather than an example.
///
/// Moving it also bought the thing it could not have there:
/// [`Document::deployment`] derives the ORDER itself instead of being handed
/// one, so the order and the artifact cannot disagree. In the example the two
/// were a call apart and a caller could pass any sequence it liked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan<D> {
    nodes: Vec<Deployed<D>>,
}

impl<D> Default for Plan<D> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl<D> Plan<D> {
    /// One entry per node that is started, in bring-up order.
    #[must_use]
    pub fn nodes(&self) -> &[Deployed<D>] {
        &self.nodes
    }

    /// The hosts this plan spreads across, in the order they are first needed.
    ///
    /// First-needed and not sorted, because a script is written in launch order
    /// and a reader following it down the page meets them this way.
    #[must_use]
    pub fn hosts(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for entry in &self.nodes {
            if !seen.contains(&entry.host.as_str()) {
                seen.push(entry.host.as_str());
            }
        }
        seen
    }

    /// Every row the plan could not carry, with the node it belongs to.
    #[must_use]
    pub fn uncarried(&self) -> Vec<(&str, &Uncarried)> {
        self.nodes
            .iter()
            .flat_map(|entry| {
                entry
                    .config
                    .uncarried
                    .iter()
                    .map(move |row| (entry.name.as_str(), row))
            })
            .collect()
    }

    /// Whether every row of every node's configuration reached the plan.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.nodes
            .iter()
            .all(|entry| entry.config.uncarried.is_empty())
    }

    /// Names that more than one started node answers to, in first-seen order.
    ///
    /// A plan names each node's configuration file after it, so a shared name
    /// is two processes writing one file — see [`Unplannable::Shared`] for the
    /// whole of it. Readable before rendering, so a screen can offer the fix
    /// instead of only reporting the refusal.
    #[must_use]
    pub fn shared_names(&self) -> Vec<(&str, usize)> {
        let mut out: Vec<(&str, usize)> = Vec::new();
        for entry in &self.nodes {
            let name = entry.name.as_str();
            if out.iter().any(|(seen, _)| *seen == name) {
                continue;
            }
            let count = self.nodes.iter().filter(|e| e.name == name).count();
            if count > 1 {
                out.push((name, count));
            }
        }
        out
    }

    /// The first shared name as a refusal, or `Ok(())`.
    fn no_shared_name(&self) -> Result<(), Unplannable> {
        match self.shared_names().first() {
            None => Ok(()),
            Some((name, count)) => Err(Unplannable::Shared {
                name: (*name).to_owned(),
                count: *count,
            }),
        }
    }
}

/// Why a plan could not be written out, **naming what it was about**.
///
/// Both arms carry the node or the name, which is the field the floor does not
/// have: the reference's store answers no-error and drops the row, so a caller
/// is told neither that it happened nor where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unplannable {
    /// One node's configuration could not be serialised into the artifact.
    Unwritable {
        /// The node it belonged to.
        node: String,
        /// What the serializer said.
        why: String,
    },
    /// Two or more started nodes answer to the same name.
    ///
    /// ★★★★★ R1788 — **found by the crate's own first test run**, and it had
    /// been true since R1687 while the derivation lived in an example. A
    /// [`Deployed::name`] is what the node's configuration file is named after,
    /// so nodes sharing one mean two processes writing one file: the document's
    /// `nodes` map keeps whichever was inserted last, and the script writes two
    /// heredocs to the same path.
    ///
    /// ★★★★★ **And the invariant that looks like it covers this does not.**
    /// [`relabel`](Document::relabel) maintains *authored names are unique
    /// within a tree, and therefore address exactly one node*
    /// ([`EditError::LabelTaken`](crate::EditError)) — which is about the
    /// **stored label**. A node with no label still has a name:
    /// [`display_name`](crate::Node::display_name) falls back to its KIND's,
    /// and a fallback is not a stored value, so nothing can refuse a second one.
    /// Two unlabelled nodes of one kind therefore collide **while the invariant
    /// holds**. The example never saw it because that screen labels every card.
    ///
    /// Refused rather than papered over. Renaming silently would change what a
    /// person sees on the canvas; dropping one is the floor's behaviour; and
    /// export is exactly the moment a person can still fix it by labelling.
    Shared {
        /// The name more than one node answers to.
        name: String,
        /// How many nodes answer to it.
        count: usize,
    },
}

impl fmt::Display for Unplannable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unwritable { node, why } => write!(
                f,
                "{node}'s configuration could not be written into the plan: {why}"
            ),
            Self::Shared { name, count } => write!(
                f,
                "{count} nodes are called {name:?}, and a plan names each node's \
                 configuration file after it — label them apart first"
            ),
        }
    }
}

impl std::error::Error for Unplannable {}

impl<D: Serialize> Plan<D> {
    /// The plan as one JSON document: what an agent reads.
    ///
    /// ★ Four sections, and the order is deliberate — `order` before `nodes`
    /// because the order is the part that cannot be got any other way, and
    /// `uncarried` last because it is normally empty and a reader should not
    /// have to scroll past it.
    ///
    /// # Errors
    /// [`Unplannable`], naming the node whose configuration could not be
    /// serialised.
    pub fn to_document(&self) -> Result<String, Unplannable> {
        let value = self.to_value()?;
        serde_json::to_string_pretty(&value).map_err(|e| Unplannable::Unwritable {
            node: String::new(),
            why: e.to_string(),
        })
    }

    /// The plan as a script: what a person runs.
    ///
    /// ★★ **It is split by host and says so.** One file cannot start processes
    /// on two machines, so a plan spread across several is written as one
    /// script that runs the part belonging to whichever host it is invoked on.
    /// The alternative — silently emitting only the first host's processes —
    /// produces a script that appears to work.
    ///
    /// ★ The rows that could not be carried are written in as **comments**
    /// rather than left out. A script is the artifact somebody keeps; a report
    /// that lived only in a toast would be gone by the time it mattered.
    ///
    /// # Errors
    /// [`Unplannable::Shared`] when two started nodes answer to one name — the
    /// script would write two heredocs to one path. The document refuses the
    /// same input for the same reason: they are one derivation rendered twice,
    /// and a refusal only one of them made would be the drift this module
    /// exists to prevent.
    pub fn to_script(&self) -> Result<String, Unplannable> {
        self.no_shared_name()?;
        let mut lines: Vec<String> = Vec::new();
        let hosts = self.hosts();
        lines.push("#!/usr/bin/env bash".to_string());
        lines.push("# Generated from the node graph. Do not edit — regenerate.".to_string());
        lines.push("set -euo pipefail".to_string());
        lines.push("OUT=${1:-./graph-run}; mkdir -p \"$OUT\"".to_string());
        lines.push("BIN=${BIN:-.}".to_string());
        lines.push(format!(
            "HOST=${{HOST:-{}}}",
            hosts.first().copied().unwrap_or("localhost")
        ));
        lines.push(String::new());

        for entry in &self.nodes {
            lines.push(format!("cat > \"$OUT/{}.json\" <<'CONFIG'", entry.name));
            lines.push(
                serde_json::to_string_pretty(&entry.config.document)
                    // A node whose configuration will not serialise still gets
                    // its heredoc, holding the reason: a script with a hole a
                    // reader can SEE beats one silently missing a process.
                    .unwrap_or_else(|e| format!("{{\"error\": {:?}}}", e.to_string())),
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
            for entry in self.nodes.iter().filter(|entry| &entry.host == host) {
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

        let uncarried = self.uncarried();
        if !uncarried.is_empty() {
            lines.push(String::new());
            lines.push("# These settings are on the screen and not in any file above:".to_string());
            for (node, row) in uncarried {
                lines.push(format!(
                    "#   {node} · {} = {} — {}",
                    row.key, row.shown, row.why
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
        Ok(lines.join("\n"))
    }

    /// The document as a value, so both renderings read one assembly.
    fn to_value(&self) -> Result<serde_json::Value, Unplannable> {
        self.no_shared_name()?;
        let order: Vec<serde_json::Value> = self
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
        let mut nodes = serde_json::Map::new();
        for entry in &self.nodes {
            let document = serde_json::to_value(&entry.config.document).map_err(|e| {
                Unplannable::Unwritable {
                    node: entry.name.clone(),
                    why: e.to_string(),
                }
            })?;
            nodes.insert(entry.name.clone(), document);
        }
        let mut hosts = serde_json::Map::new();
        for host in self.hosts() {
            hosts.insert(
                host.to_owned(),
                serde_json::Value::Array(
                    self.nodes
                        .iter()
                        .filter(|entry| entry.host == host)
                        .map(|entry| serde_json::Value::String(entry.name.clone()))
                        .collect(),
                ),
            );
        }
        let uncarried: Vec<serde_json::Value> = self
            .uncarried()
            .into_iter()
            .map(|(node, row)| {
                serde_json::json!({
                    "node": node,
                    "key": row.key,
                    "shown": row.shown,
                    "why": row.why,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "order": order,
            "nodes": serde_json::Value::Object(nodes),
            "hosts": serde_json::Value::Object(hosts),
            "uncarried": uncarried,
        }))
    }
}

/// One node's place in the bring-up order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Which node.
    pub node: NodeId,
    /// The name it is ordered by, carried so a caller does not re-derive it.
    pub name: String,
    /// Why it sits where it does.
    pub standing: Bringup,
}

impl<K: NodeKind> Document<K> {
    /// The order this tree's nodes have to be started in, so that a node is up
    /// before anything that reaches out to it.
    ///
    /// Every node that is **started** appears exactly once, whatever the links
    /// do. A node switched off is not started and is not here — see the module
    /// header for that, for the walk, and for what the first draft got wrong.
    ///
    /// A **muted** link still counts. Muting is a semantic declaration about
    /// the value a link carries, and every structural derivation in this crate
    /// ignores it for the same reason: the wiring is still there, and a
    /// deployment is a structural question. A disabled *node* is the other
    /// case, and the difference is which of the two the switch is on.
    #[must_use]
    pub fn launch_order(&self, tree: TreeId) -> Vec<Placed> {
        let Some(held) = self.tree(tree) else {
            return Vec::new();
        };
        let started = |id: NodeId| held.node(id).is_some_and(|node| !node.disabled);
        // A link only constrains the order while BOTH of its ends are being
        // started; see the module header for why dropping the others is not the
        // same as dropping the node's own edges.
        let live = |link: &&crate::Link| started(link.from.node) && started(link.to.node);
        // Every started node, with the reason it will be reported under and the
        // count of things it still has to wait for. A node waits for whatever
        // it reaches OUT to, so the walk runs the link direction backwards.
        let mut waiting: Vec<(Placed, usize)> = held
            .nodes()
            .filter(|node| !node.disabled)
            .map(|node| {
                let id = node.id;
                let dialled = held
                    .links()
                    .iter()
                    .filter(live)
                    .any(|link| link.to.node == id);
                let waits = held
                    .links()
                    .iter()
                    .filter(live)
                    .filter(|link| link.from.node == id)
                    .count();
                (
                    Placed {
                        node: id,
                        name: node.display_name(),
                        standing: Bringup::of(dialled, waits > 0),
                    },
                    waits,
                )
            })
            .collect();

        let mut order: Vec<Placed> = Vec::with_capacity(waiting.len());
        while !waiting.is_empty() {
            // The ready ones, and among them the first by name — which is what
            // makes the same graph give the same plan twice.
            let next = waiting
                .iter()
                .enumerate()
                .filter(|(_, (_, waits))| *waits == 0)
                .min_by(|(_, (a, _)), (_, (b, _))| a.name.cmp(&b.name))
                .map(|(at, _)| at);
            let Some(at) = next else {
                // Unreachable while `connect` refuses a cycle. Kept because a
                // plan that DROPPED a node would be worse than one that ordered
                // it badly, and this does not want to depend on that invariant.
                waiting.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));
                order.extend(waiting.into_iter().map(|(placed, _)| placed));
                return order;
            };
            let (placed, _) = waiting.remove(at);
            // Whoever reached out to it has one fewer thing to wait for.
            for (other, waits) in &mut waiting {
                let feeds = held
                    .links()
                    .iter()
                    .filter(live)
                    .filter(|link| link.from.node == other.node && link.to.node == placed.node)
                    .count();
                *waits -= feeds.min(*waits);
            }
            order.push(placed);
        }
        order
    }

    /// R1788 — **the deployable configuration**: every node that is started, in
    /// the order it has to be started in, with the configuration it starts
    /// with and every row that configuration could not carry.
    ///
    /// The order is derived HERE, from [`launch_order`](Self::launch_order),
    /// which is the whole reason this lives beside it: while the caller passed
    /// a sequence in, nothing stopped the artifact and the order from
    /// disagreeing, and the two are one derivation rendered twice.
    ///
    /// `program_of` answering `None` means *this node is not started* — a
    /// palette entry, a note, a frame. Such a node is left out rather than
    /// exported with an empty program, because a script line with no executable
    /// is a failure at run time instead of at export time.
    pub fn deployment<D>(
        &self,
        tree: TreeId,
        host_of: impl Fn(NodeId) -> String,
        program_of: impl Fn(NodeId) -> Option<String>,
        config_of: impl Fn(NodeId) -> Configured<D>,
    ) -> Plan<D> {
        let nodes = self
            .launch_order(tree)
            .into_iter()
            .filter_map(|placed| {
                let program = program_of(placed.node)?;
                Some(Deployed {
                    name: placed.name,
                    host: host_of(placed.node),
                    program,
                    standing: placed.standing,
                    config: config_of(placed.node),
                })
            })
            .collect();
        Plan { nodes }
    }
}
