//! ★★★★★ R2001 — **a class of port that is folded away behind one control,
//! and who is allowed to say which ports are in it.**
//!
//! # What the reference does, measured rather than summarised
//!
//! Three separate things carry this capability there, and the census row this
//! module closes names only the third:
//!
//! * a bit on the **pin**, saying the pin is in the advanced class;
//! * a **tri-state on the node** — *no advanced pins* / *shown* / *hidden* —
//!   which the chevron on the node's frame writes;
//! * a virtual on the node class, *may a person edit a pin's advanced flag*,
//!   whose base answers no and which exactly **two** classes in the whole tree
//!   override.
//!
//! Read at its one consumer, that third one is not about a menu at all. It sits
//! in the routine that carries a pin's persistent data across a **rebuild** of
//! the node's pins, and it decides whether the old pin's advanced bit is copied
//! forward — with the comment *"Otherwise we don't want to copy this, or we'd
//! be ignoring new metadata that tries to hide old pins."* So the flag exists
//! because there a declaration and a person's choice are **the same storage**,
//! and something has to say which of the two a rebuild keeps.
//!
//! The two overriders say what the yes-answer means. One is a switch over an
//! enumeration whose *remove pin* does not delete a case — the cases belong to
//! the enumeration — but moves that pin into the advanced class and breaks its
//! links, while *add pin* takes the first hidden one back. Its advanced set is
//! therefore a record of what the person has been doing, and the class
//! re-deriving it on the next rebuild would undo their work.
//!
//! # ★★★★★ Four ways this is better than what was measured
//!
//! 1. **The rebuild question is unrepresentable rather than answered.** The
//!    kind's declaration is [`Port::advanced`] and the person's disagreement is
//!    [`Appearance::reclassified`], two different places, so a rebuild re-reads
//!    one and the other still stands. Nothing has to choose a survivor, and the
//!    reference's defect — a person's classification silently dropped, or new
//!    metadata silently ignored — has no state to occur in.
//! 2. **"This node has nothing advanced" is DERIVED and never stored.** The
//!    reference keeps that as the third member of its stored tri-state, and
//!    pays for it: across its tree, twenty assignments in twenty-one files
//!    promote that member to *hidden* by hand right after creating an advanced
//!    pin, and **five** ever write it back. A node that stops having advanced
//!    pins therefore goes on drawing the control that folds them, because the
//!    control's visibility asks the stored member and not the pins.
//!    [`Document::advanced_view`] answers all three from the ports themselves.
//! 3. **A person's answer has three values and is a type.** *Advanced*,
//!    *plain*, and *say nothing and let the kind answer again* are three
//!    genuinely different requests, and the third is the one the reference
//!    cannot express at all: with one bit per pin, the declaration is
//!    overwritten the moment a person touches it and there is nothing left to
//!    go back to. [`Classify::Declared`] is that going back.
//! 4. **The refusal can be asked before the act.** The reference's permission
//!    is read by the code doing the copying, so a person finds out what it
//!    said by watching a pin lose its class. [`Document::may_classify_port`]
//!    answers the same rule as a value and [`Document::classify_port`] is a
//!    call site of it — one rule asked at two moments, the shape R1920 settled.
//!
//! # What is NOT better, stated rather than hidden
//!
//! The reference's flag does a second job this one does not have to do, and
//! that is a *consequence* of the split above rather than a capability we
//! grew: it is a permission here and nothing else. And a taxonomy that
//! declares [`NodeKind::advanced_ports_are_authored`] still owes its own
//! answer to *what a person's classification means when the kind's ports
//! change shape underneath it* — [`Appearance::reclassified`] is keyed by
//! [`PortRef`], which is an index, so a variadic run that grows re-points an
//! override the same way it re-points a value. That is the standing property
//! of this crate's port addressing and not a new hole.
//!
//! [`Port::advanced`]: crate::Port::advanced
//! [`Appearance::reclassified`]: crate::Appearance::reclassified
//! [`NodeKind::advanced_ports_are_authored`]: crate::NodeKind::advanced_ports_are_authored

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::model::{Document, NodeBody, NodeId, NodeKind, PortRef, Side, TreeId};

/// ★★★★★ R2001 — which class a port is in.
///
/// Two arms and a named type rather than the reference's bare bit, because the
/// class is a fact a screen shows, a wire publishes and a person changes — and
/// [`Classified`] carries it beside *who said so*, which a bool has nowhere to
/// put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PortClass {
    /// Always on the frame. The class an ordinary port is in.
    Plain,
    /// Folded away behind the node's advanced control unless something is
    /// wired to it.
    Advanced,
}

impl PortClass {
    /// The word this class is published under, for a client reading the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Advanced => "advanced",
        }
    }
}

/// ★★★★★ R2001 — who put a port in the class it is in.
///
/// The half a bare [`PortClass`] cannot carry, and the same shape R1923 gave a
/// description and R1928 gave a name: an editor offering *put this back the way
/// your kind declares it* has to know whether there is anything to put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClassSource {
    /// The kind's own declaration — [`Port::advanced`](crate::Port::advanced).
    Kind,
    /// A person said so on this node, and
    /// [`Classify::Declared`] takes it back.
    Person,
}

/// ★★★★★ R2001 — a port's class as a reader gets it, and who chose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Classified {
    /// The class the port is in.
    pub class: PortClass,
    /// Where that answer came from.
    pub source: ClassSource,
}

impl Classified {
    /// Whether this port is folded away when the node's advanced group is and
    /// nothing is wired to it.
    #[must_use]
    pub const fn is_advanced(&self) -> bool {
        matches!(self.class, PortClass::Advanced)
    }
}

/// ★★★★★ R2001 — what a person is saying about one port's class.
///
/// Three arms and not an `Option<PortClass>`, by R1928's rule: *say nothing* is
/// a real third request here and not an absent answer. It is also the request
/// the reference cannot make — with the declaration and the choice sharing one
/// bit, touching a pin's class destroys what the kind said and there is nothing
/// to return to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Classify {
    /// Put this port in the advanced class, whatever the kind declared.
    Advanced,
    /// Take this port out of the advanced class, whatever the kind declared.
    Plain,
    /// Say nothing about this port: the kind's declaration answers again.
    #[default]
    Declared,
}

impl Classify {
    /// The word this request arrives under on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::Plain => "plain",
            Self::Declared => "declared",
        }
    }

    /// The request this word names, or `None` for a word this vocabulary does
    /// not have.
    ///
    /// The closed set a wire may send, derived from the arms rather than
    /// re-spelled beside them: a fourth arm arrives here without an edit.
    #[must_use]
    pub fn from_wire_word(word: &str) -> Option<Self> {
        [Self::Advanced, Self::Plain, Self::Declared]
            .into_iter()
            .find(|how| how.wire_word() == word)
    }
}

/// ★★★★★ R2001 — what a node's advanced group is doing.
///
/// Three states, one of which is **derived from the ports and never stored**:
/// see this module's header for the count of hand-maintained promotions the
/// reference pays for storing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdvancedView {
    /// No port of this node is in the advanced class, so there is nothing to
    /// fold and no control to draw.
    Nothing,
    /// This node has advanced ports and they are folded away — except any that
    /// something is wired to, which stay on the frame.
    Folded,
    /// This node has advanced ports and they are all on the frame.
    Unfolded,
}

impl AdvancedView {
    /// Whether a renderer draws the fold control at all.
    ///
    /// The reference asks its stored tri-state for exactly this and gets a
    /// stale answer on any node that has stopped having advanced pins.
    #[must_use]
    pub const fn has_control(self) -> bool {
        !matches!(self, Self::Nothing)
    }

    /// The word this state is published under, for a client reading the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::Folded => "folded",
            Self::Unfolded => "unfolded",
        }
    }
}

/// ★★★★★ R2001 — why a port's class could not be changed.
///
/// Its own type rather than an arm on [`EditError`](crate::EditError), for the
/// reason [`SwapError`](crate::SwapError) is: each of these is repaired by a
/// different act — find the node, find the port, or stop asking because this
/// kind's ports are the kind's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyError {
    /// No such node in that tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The node has no port there.
    NoSuchPort {
        /// The side asked about.
        side: Side,
        /// The index asked about.
        index: u32,
        /// How many ports the node has on that side, so a caller can say what
        /// the range is instead of guessing.
        of: u32,
    },
    /// This kind does not let a person classify its ports —
    /// [`NodeKind::advanced_ports_are_authored`] answers `false`.
    KindDecides {
        /// What the node is called, so a refusal a person reads names it.
        kind: String,
    },
    /// The node has no kind to ask: a frame or a reroute is not a taxonomy's
    /// node, so nothing there declares whether its ports are a person's.
    NotAKind {
        /// The node asked about.
        node: NodeId,
    },
}

impl core::fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => write!(f, "no node {node:?} in tree {tree:?}"),
            Self::NoSuchPort { side, index, of } => {
                write!(f, "no {side:?} port at {index}; this node has {of} of them")
            }
            Self::KindDecides { kind } => write!(
                f,
                "`{kind}` declares which of its ports are advanced; a person may not"
            ),
            Self::NotAKind { node } => write!(
                f,
                "node {node:?} has no kind, so nothing declares whether its ports are a person's"
            ),
        }
    }
}

impl std::error::Error for ClassifyError {}

/// The class a port is in, given the node's overrides and what its kind
/// declared for that port.
///
/// A free function taking the two facts rather than a method taking an address,
/// because [`Document::visible_ports`] already holds the resolved signature and
/// asking the document per port would walk the kind once per port — the
/// quadratic shape R1717 recorded.
pub(crate) fn classified_in(appearance: &Appearance, declared: bool, at: PortRef) -> Classified {
    appearance.reclassified.get(&at).map_or(
        Classified {
            class: if declared {
                PortClass::Advanced
            } else {
                PortClass::Plain
            },
            source: ClassSource::Kind,
        },
        |&class| Classified {
            class,
            source: ClassSource::Person,
        },
    )
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R2001 — **which class the port at `at` is in, and who put it
    /// there** — or `None` when the node has no port there.
    ///
    /// The one resolution point, and the reason there is one: a person's
    /// override and the kind's declaration live in different places, so
    /// everything that draws, wires or reports a port has to compose them the
    /// same way. It is composed here and nowhere else.
    #[must_use]
    pub fn classified(&self, tree: TreeId, node: NodeId, at: PortRef) -> Option<Classified> {
        let signature = self.signature(tree, node)?;
        let ports = match at.side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let declared = ports.get(at.index as usize)?.advanced;
        let appearance = &self.tree(tree)?.node(node)?.appearance;
        Some(classified_in(appearance, declared, at))
    }

    /// ★★★★★ R2001 — the class of the port at `at`, without who chose it.
    ///
    /// [`classified`](Document::classified) when the answer to *is there
    /// anything to put back* matters.
    #[must_use]
    pub fn port_class(&self, tree: TreeId, node: NodeId, at: PortRef) -> Option<PortClass> {
        self.classified(tree, node, at).map(|it| it.class)
    }

    /// ★★★★★ R2001 — every port of `node` that is in the advanced class,
    /// ascending by side then index.
    ///
    /// What [`advanced_view`](Document::advanced_view) is derived from, and
    /// published because a screen drawing the fold control wants to say how
    /// many ports it folds.
    #[must_use]
    pub fn advanced_ports(&self, tree: TreeId, node: NodeId) -> Option<Vec<PortRef>> {
        let signature = self.signature(tree, node)?;
        let appearance = &self.tree(tree)?.node(node)?.appearance;
        let mut found = Vec::new();
        for (side, ports) in [
            (Side::Input, &signature.inputs),
            (Side::Output, &signature.outputs),
        ] {
            for (index, port) in ports.iter().enumerate() {
                let at = PortRef::new(side, u32::try_from(index).unwrap_or(u32::MAX));
                if classified_in(appearance, port.advanced, at).is_advanced() {
                    found.push(at);
                }
            }
        }
        Some(found)
    }

    /// ★★★★★ R2001 — **what this node's advanced group is doing**, or `None`
    /// when the node is not there.
    ///
    /// [`AdvancedView::Nothing`] is derived from the ports on every read, which
    /// is the whole of what this passes the reference by: there the same three
    /// states are a stored field, twenty sites promote it by hand and five
    /// demote it, so a node that has stopped having advanced ports keeps
    /// drawing a control that folds none.
    #[must_use]
    pub fn advanced_view(&self, tree: TreeId, node: NodeId) -> Option<AdvancedView> {
        let any = !self.advanced_ports(tree, node)?.is_empty();
        let shown = self.tree(tree)?.node(node)?.appearance.advanced_shown;
        Some(if !any {
            AdvancedView::Nothing
        } else if shown {
            AdvancedView::Unfolded
        } else {
            AdvancedView::Folded
        })
    }

    /// ★★★★★ R2001 — **fold this node's advanced ports away, or put them back
    /// on the frame**, answering the state that results.
    ///
    /// Answering [`AdvancedView`] rather than nothing is the difference from
    /// the reference's own handler, which guards on its stored tri-state and
    /// silently does nothing when it says *no advanced pins*: here the caller
    /// is told it asked about a node with nothing to fold
    /// ([`AdvancedView::Nothing`]) instead of watching for a picture that does
    /// not change.
    ///
    /// `None` when the node is not there.
    pub fn show_advanced_ports(
        &mut self,
        tree: TreeId,
        node: NodeId,
        shown: bool,
    ) -> Option<AdvancedView> {
        self.tree_mut(tree)?
            .node_mut(node)?
            .appearance
            .advanced_shown = shown;
        self.advanced_view(tree, node)
    }

    /// ★★★★★ R2001 — **what classifying this port would do**, asked before
    /// anything moves.
    ///
    /// The rule [`classify_port`](Document::classify_port) is a call site of.
    /// A screen greys the gesture with this and announces the reason, so the
    /// refusal a person reads and the refusal the edit makes cannot drift
    /// apart — R1920's shape, and the answer the reference has no way to give
    /// because its permission is read by the code doing the copying.
    ///
    /// # Errors
    ///
    /// [`ClassifyError`] — an absent node or port, a node with no kind, or a
    /// kind that declares its ports' classes itself.
    pub fn may_classify_port(
        &self,
        tree: TreeId,
        node: NodeId,
        at: PortRef,
        how: Classify,
    ) -> Result<Classified, ClassifyError> {
        let held = self
            .tree(tree)
            .and_then(|t| t.node(node))
            .ok_or(ClassifyError::NoSuchNode { tree, node })?;
        let NodeBody::Kind(kind) = &held.body else {
            return Err(ClassifyError::NotAKind { node });
        };
        let signature = self
            .signature(tree, node)
            .ok_or(ClassifyError::NoSuchNode { tree, node })?;
        let ports = match at.side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let declared = ports
            .get(at.index as usize)
            .ok_or_else(|| ClassifyError::NoSuchPort {
                side: at.side,
                index: at.index,
                of: u32::try_from(ports.len()).unwrap_or(u32::MAX),
            })?
            .advanced;
        if !kind.advanced_ports_are_authored() {
            return Err(ClassifyError::KindDecides { kind: kind.name() });
        }
        Ok(match how {
            Classify::Advanced => Classified {
                class: PortClass::Advanced,
                source: ClassSource::Person,
            },
            Classify::Plain => Classified {
                class: PortClass::Plain,
                source: ClassSource::Person,
            },
            Classify::Declared => classified_in(&Appearance::default(), declared, at),
        })
    }

    /// ★★★★★ R2001 — **say which class a port is in**, on this node, as a
    /// person.
    ///
    /// [`Classify::Declared`] removes the override rather than writing the
    /// kind's current answer into it, which is what keeps *going back* meaning
    /// *going back*: a kind that later re-declares the port then moves it, as
    /// it would for a node nobody had ever touched.
    ///
    /// Answers the resulting classification, so a caller undoing this knows
    /// what it is undoing to.
    ///
    /// # Errors
    ///
    /// [`ClassifyError`] — see [`may_classify_port`](Document::may_classify_port),
    /// which this asks first.
    pub fn classify_port(
        &mut self,
        tree: TreeId,
        node: NodeId,
        at: PortRef,
        how: Classify,
    ) -> Result<Classified, ClassifyError> {
        let after = self.may_classify_port(tree, node, at, how)?;
        let appearance = &mut self
            .tree_mut(tree)
            .and_then(|t| t.node_mut(node))
            .ok_or(ClassifyError::NoSuchNode { tree, node })?
            .appearance;
        match how {
            Classify::Advanced => {
                appearance.reclassified.insert(at, PortClass::Advanced);
            }
            Classify::Plain => {
                appearance.reclassified.insert(at, PortClass::Plain);
            }
            Classify::Declared => {
                appearance.reclassified.remove(&at);
            }
        }
        Ok(after)
    }
}
