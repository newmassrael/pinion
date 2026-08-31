//! ★★★★★ R1925 — **a definition's ports gather into named, collapsible
//! sections, and one port of a section can be the switch that stands for it.**
//!
//! # What the reference does, measured rather than summarised
//!
//! The DCC registers three operators for this and the census carried all three
//! under one sentence, *grouping a definition's ports into collapsible
//! sections*. Read at `scripts/startup/bl_operators/node.py`, that sentence
//! covers none of them:
//!
//! * `interface_item_new_panel_toggle` — make a new boolean **input** socket,
//!   move it to position 0 of the active panel, and flag it. Refuses with
//!   *Active item is not a panel* and *Panel already has a toggle*.
//! * `interface_item_make_panel_toggle` — take the boolean input socket the
//!   author has selected and flag it. Refuses with *Only boolean input sockets
//!   are supported*, *Socket must be in a panel*, *Panel already has a toggle*.
//! * `interface_item_unlink_panel_toggle` — clear the flag, leaving the socket
//!   where it is.
//!
//! So the three are **the switch**, not the grouping: a panel's on/off is
//! carried by one boolean input *inside* it, which is what lets the section
//! header draw a checkbox. Grouping itself has no operator of its own in that
//! list — the panel is created by `interface_item_new` with `item_type='PANEL'`.
//!
//! # ★★★★★ Three ways this is better than what was measured
//!
//! 1. **A make does not destroy the port's name.** The reference's
//!    `make_panel_toggle` assigns the socket the panel's name, and its `unlink`
//!    assigns the panel's name *again*, so a socket called `Use Falloff`
//!    promoted and then demoted comes back called after its panel and the
//!    authored name is gone. Here the section supplies the header's label and
//!    the port keeps its own name, so the round trip is the identity — which is
//!    an assertion a test can make and the reference would fail.
//! 2. **"Already has a switch" is unrepresentable rather than checked.** The
//!    reference flags the socket and asks whether a panel has one by looking at
//!    its *first* item only — while its make operator does not move the socket
//!    it flags. A panel can therefore hold a flagged socket that is not first,
//!    which its unlink poll and its new operator's "already has a toggle" check
//!    both fail to see. Here the section owns [`Section::switch`], an `Option`,
//!    so a second switch cannot be written down at all and the switch is listed
//!    first without moving any port.
//! 3. **The refusal can be asked before the act.** Every message above is a
//!    poll-time string, produced while a menu is being drawn.
//!    [`Document::may_make_section_switch`] answers the same rule as a value,
//!    and [`Document::make_section_switch`] is a call site of it — one rule
//!    asked at two moments, which is the shape R1922 settled and R1924 used for
//!    a wire's end.
//!
//! # What "boolean" becomes when the taxonomy is the application's
//!
//! This crate does not own the socket type, so it cannot name a boolean. The
//! application declares which of its types is the two-state one
//! ([`NodeKind::switch_type`]), and that single declaration is read twice: it
//! is the type [`Document::new_section_switch`] *creates*, and it is what
//! [`Document::make_section_switch`] checks an existing port against. An
//! application that declares none has no section switches, and says so
//! ([`SwitchRefusal::NoSwitchType`]) rather than silently accepting any port.
//!
//! # Where the maintenance lives
//!
//! Every mutation of an interface's port lists in this crate goes through
//! [`Document::expose`] and [`Document::unexpose`] — measured, not assumed:
//! `Interface::side_mut` has exactly those two call sites. `expose` appends, so
//! it can never disturb a member index; `unexpose` is where a removal shifts
//! the members and clears a switch that went with the port. A section therefore
//! cannot dangle through this crate's API, and [`Document::validate`] reports
//! the ones that arrive from a file.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Document, EditError, Interface, InterfaceSide, NodeKind, Port, TreeId};

/// A section's identity within one interface.
///
/// Allocated from a counter rather than being a position, so removing a section
/// never renames another one — the mistake an index would make the moment a
/// document is edited twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SectionId(pub u32);

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "section {}", self.0)
    }
}

/// One port of a definition's interface, named by the half it is on and its
/// index in that half.
///
/// [`PortRef`](crate::PortRef) is the same shape for a *node's* signature and is
/// deliberately not reused: that one is a [`Side`](crate::Side) of a node and
/// this one is an [`InterfaceSide`] of a tree, and the two mean opposite things
/// at an interface node — a tree's interface *inputs* are the inside node's
/// *outputs*. One type covering both is how R1589's "one spelling, two
/// meanings" gets written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InterfacePort {
    /// Which half of the interface.
    pub side: InterfaceSide,
    /// The port's index in that half.
    pub index: u32,
}

impl InterfacePort {
    /// The interface input at `index`.
    #[must_use]
    pub const fn input(index: u32) -> Self {
        Self {
            side: InterfaceSide::Input,
            index,
        }
    }

    /// The interface output at `index`.
    #[must_use]
    pub const fn output(index: u32) -> Self {
        Self {
            side: InterfaceSide::Output,
            index,
        }
    }
}

impl fmt::Display for InterfacePort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.side.wire_word(), self.index)
    }
}

/// A named, collapsible run of a definition's interface ports.
///
/// The members are kept in the order the section shows them, which is **not**
/// the order the ports are indexed in: a port's index is the link ABI and moving
/// one would re-aim every wire at every instance. That separation is what lets
/// [`Document::make_section_switch`] put the switch first without touching a
/// single link — the thing the reference has to move a socket to achieve, and
/// then does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub(crate) id: SectionId,
    pub(crate) name: String,
    pub(crate) folded: bool,
    pub(crate) members: Vec<InterfacePort>,
    pub(crate) switch: Option<u32>,
}

impl Section {
    /// Its identity in this interface.
    #[must_use]
    pub const fn id(&self) -> SectionId {
        self.id
    }

    /// The header's label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the section shows itself closed. Presentation, kept in the
    /// document for the reason [`Appearance`](crate::Appearance) is: a
    /// definition that is copied into another document keeps the way it was
    /// arranged.
    #[must_use]
    pub const fn folded(&self) -> bool {
        self.folded
    }

    /// Its ports, in the order the section shows them — the switch first when it
    /// has one.
    #[must_use]
    pub fn members(&self) -> &[InterfacePort] {
        &self.members
    }

    /// The interface **input** index of the port that switches this section, if
    /// one does.
    #[must_use]
    pub const fn switch(&self) -> Option<u32> {
        self.switch
    }
}

/// Why a section switch could not be made, moved or removed.
///
/// Every message the reference reaches as a poll-time string is here as a value,
/// plus the three it reaches by returning false with nothing said or by
/// swallowing an attribute error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwitchRefusal {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// That interface has no such section. The reference's *Active item is not a
    /// panel*.
    NoSuchSection {
        /// The tree whose interface was addressed.
        tree: TreeId,
        /// The section asked for.
        section: SectionId,
    },
    /// That interface has no input at that index.
    NoSuchPort {
        /// The tree whose interface was addressed.
        tree: TreeId,
        /// The index asked for.
        index: u32,
        /// How many inputs the interface actually has.
        arity: u32,
    },
    /// The port is not in any section. The reference's *Socket must be in a
    /// panel*.
    NotInASection {
        /// The interface input index.
        index: u32,
    },
    /// The port does not carry the application's two-state type. The reference's
    /// *Only boolean input sockets are supported* — and the input half of that
    /// sentence is structural here, because the index this is asked with is an
    /// interface **input** index.
    NotSwitchable {
        /// The interface input index.
        index: u32,
    },
    /// The application declares no two-state socket type
    /// ([`NodeKind::switch_type`]), so it has no section switches at all.
    NoSwitchType,
    /// The section already has a switch. The reference's *Panel already has a
    /// toggle*.
    SectionHasSwitch {
        /// The section.
        section: SectionId,
        /// The input index already carrying it.
        port: u32,
    },
    /// The section has no switch to remove. The reference's unlink poll returns
    /// false here and says nothing.
    SectionHasNoSwitch {
        /// The section.
        section: SectionId,
    },
}

impl fmt::Display for SwitchRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchSection { section, .. } => write!(f, "there is no {section}"),
            Self::NoSuchPort { index, arity, .. } => {
                write!(f, "input {index} of {arity} does not exist")
            }
            Self::NotInASection { index } => write!(f, "input {index} is not in a section"),
            Self::NotSwitchable { index } => {
                write!(f, "input {index} does not carry the switchable type")
            }
            Self::NoSwitchType => {
                write!(f, "this application declares no two-state socket type")
            }
            Self::SectionHasSwitch { section, port } => {
                write!(f, "{section} is already switched by input {port}")
            }
            Self::SectionHasNoSwitch { section } => write!(f, "{section} has no switch"),
        }
    }
}

impl std::error::Error for SwitchRefusal {}

impl SwitchRefusal {
    /// The one spelling a client reads this refusal under (the shape R1920 set
    /// with [`Matched::wire_word`](crate::Matched::wire_word)).
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::NoSuchTree(_) => "no-such-tree",
            Self::NoSuchSection { .. } => "no-such-section",
            Self::NoSuchPort { .. } => "no-such-port",
            Self::NotInASection { .. } => "not-in-a-section",
            Self::NotSwitchable { .. } => "not-switchable",
            Self::NoSwitchType => "no-switch-type",
            Self::SectionHasSwitch { .. } => "already-switched",
            Self::SectionHasNoSwitch { .. } => "not-switched",
        }
    }
}

/// How a section breaks its interface's rules — the states a document that
/// arrived from a file can be in and this crate's own edits cannot produce.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionBreach {
    /// A member names a port the interface does not have.
    NoSuchMember(InterfacePort),
    /// A port is a member of this section and of another one.
    MemberShared(InterfacePort),
    /// The switch is not among the section's members.
    SwitchNotAMember(u32),
    /// The switch does not carry the application's two-state type.
    SwitchNotSwitchable(u32),
}

impl fmt::Display for SectionBreach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchMember(port) => write!(f, "{port} does not exist"),
            Self::MemberShared(port) => write!(f, "{port} is in two sections"),
            Self::SwitchNotAMember(index) => {
                write!(f, "input {index} switches a section it is not in")
            }
            Self::SwitchNotSwitchable(index) => {
                write!(f, "input {index} switches but is not switchable")
            }
        }
    }
}

impl<K: NodeKind> Interface<K> {
    /// The sections this interface groups its ports into, in the order they are
    /// shown.
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// One section by identity.
    #[must_use]
    pub fn section(&self, id: SectionId) -> Option<&Section> {
        self.sections.iter().find(|held| held.id == id)
    }

    /// Which section a port is in, if any.
    #[must_use]
    pub fn section_of(&self, port: InterfacePort) -> Option<SectionId> {
        self.sections
            .iter()
            .find(|held| held.members.contains(&port))
            .map(|held| held.id)
    }

    /// The ports of this interface that are in no section at all, in index
    /// order — what a header-less run of the definition's face shows.
    #[must_use]
    pub fn ungathered(&self) -> Vec<InterfacePort> {
        [InterfaceSide::Input, InterfaceSide::Output]
            .into_iter()
            .flat_map(|side| {
                (0..u32::try_from(self.side(side).len()).unwrap_or(u32::MAX))
                    .map(move |index| InterfacePort { side, index })
            })
            .filter(|port| self.section_of(*port).is_none())
            .collect()
    }

    /// Drop `port` from whatever section holds it and slide every higher member
    /// of the same side down one — the maintenance a removal owes.
    ///
    /// Called from [`Document::unexpose`] and nowhere else, because that is the
    /// only operation in this crate that can shorten a port list.
    pub(crate) fn forget_port(&mut self, side: InterfaceSide, index: u32) {
        let gone = InterfacePort { side, index };
        for section in &mut self.sections {
            section.members.retain(|member| *member != gone);
            for member in &mut section.members {
                if member.side == side && member.index > index {
                    member.index -= 1;
                }
            }
            if side == InterfaceSide::Input {
                section.switch = match section.switch {
                    Some(switch) if switch == index => None,
                    Some(switch) if switch > index => Some(switch - 1),
                    other => other,
                };
            }
        }
    }

    /// Take another interface's sections wholesale.
    ///
    /// Used where a definition is copied port for port in the same order into an
    /// empty interface ([`Document::insert`](crate::Document::insert)), so every
    /// member index still names the port it named. A section is part of what a
    /// definition *is* — [`Fragment`](crate::Fragment) compares two definitions
    /// by their interfaces — so a paste that dropped them would make a carried
    /// definition unequal to the one it was carried from.
    ///
    /// ★ The counter travels too, and that is why: `Interface` compares by
    /// derive, so a copy that reset it would be unequal to its origin while
    /// showing the same face. `same_definition` is conservative in the
    /// duplicate-a-definition direction by design, but a fork nobody can SEE
    /// the reason for is worse than one anybody can.
    pub(crate) fn adopt_sections(&mut self, from: &Self) {
        self.sections.clone_from(&from.sections);
        self.next_section = from.next_section;
    }

    /// Drop `port` from whatever section holds it, clearing that section's
    /// switch when the port was it.
    fn forget_membership(&mut self, port: InterfacePort) {
        for section in &mut self.sections {
            if section.members.contains(&port) {
                section.members.retain(|member| *member != port);
                if port.side == InterfaceSide::Input && section.switch == Some(port.index) {
                    section.switch = None;
                }
            }
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// Add an empty section to a definition's interface.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`].
    pub fn add_section(
        &mut self,
        tree: TreeId,
        name: impl Into<String>,
    ) -> Result<SectionId, EditError> {
        let interface = self.interface_mut(tree)?;
        let id = SectionId(interface.next_section);
        interface.next_section += 1;
        interface.sections.push(Section {
            id,
            name: name.into(),
            folded: false,
            members: Vec::new(),
            switch: None,
        });
        Ok(id)
    }

    /// Remove a section, answering the ports it let go.
    ///
    /// The ports themselves stay — a section is how the face is *arranged*, and
    /// removing an arrangement is not removing a contract. The reference's item
    /// removal takes the panel's toggle socket away with the panel, which is the
    /// opposite choice and the one that loses a wire at every instance.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchSection`].
    pub fn remove_section(
        &mut self,
        tree: TreeId,
        section: SectionId,
    ) -> Result<Vec<InterfacePort>, EditError> {
        let interface = self.interface_mut(tree)?;
        let at = interface
            .sections
            .iter()
            .position(|held| held.id == section)
            .ok_or(EditError::NoSuchSection { tree, section })?;
        Ok(interface.sections.remove(at).members)
    }

    /// Rename a section's header, answering the name it had.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchSection`].
    pub fn rename_section(
        &mut self,
        tree: TreeId,
        section: SectionId,
        name: impl Into<String>,
    ) -> Result<String, EditError> {
        let held = self.section_mut(tree, section)?;
        Ok(std::mem::replace(&mut held.name, name.into()))
    }

    /// Show a section closed, or open, answering how it was.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchSection`].
    pub fn set_section_folded(
        &mut self,
        tree: TreeId,
        section: SectionId,
        folded: bool,
    ) -> Result<bool, EditError> {
        let held = self.section_mut(tree, section)?;
        Ok(std::mem::replace(&mut held.folded, folded))
    }

    /// Put an interface port in a section, or take it out of the one it is in,
    /// answering the section it was in before.
    ///
    /// A port is in at most one section and that is not a rule this checks — it
    /// is the only writer, and it removes before it adds, so the state cannot be
    /// reached from here at all.
    ///
    /// Taking the port that switches a section out of it clears the switch: a
    /// switch outside its own section is the state the reference can reach and
    /// cannot see.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`], [`EditError::NoSuchSection`] or
    /// [`EditError::NoSuchInterfacePort`].
    pub fn assign_section(
        &mut self,
        tree: TreeId,
        port: InterfacePort,
        section: Option<SectionId>,
    ) -> Result<Option<SectionId>, EditError> {
        let interface = self.interface_mut(tree)?;
        let arity = u32::try_from(interface.side(port.side).len()).unwrap_or(u32::MAX);
        if port.index >= arity {
            return Err(EditError::NoSuchInterfacePort {
                tree,
                side: port.side,
                index: port.index,
                arity,
            });
        }
        if let Some(wanted) = section {
            if !interface.sections.iter().any(|held| held.id == wanted) {
                return Err(EditError::NoSuchSection {
                    tree,
                    section: wanted,
                });
            }
        }
        let was = interface.section_of(port);
        interface.forget_membership(port);
        if let Some(wanted) = section {
            if let Some(held) = interface.sections.iter_mut().find(|held| held.id == wanted) {
                held.members.push(port);
            }
        }
        Ok(was)
    }

    /// Whether this interface input could be made its section's switch, and
    /// which section that would be.
    ///
    /// The question [`Document::make_section_switch`] answers by acting, asked
    /// without acting. One rule at two moments: `make_section_switch` is a call
    /// site of this, so a screen that lights the ports it would accept and the
    /// edit that accepts them cannot disagree.
    ///
    /// # Errors
    ///
    /// Every arm of [`SwitchRefusal`] except [`SwitchRefusal::NoSuchSection`]
    /// and [`SwitchRefusal::SectionHasNoSwitch`], which are the section-first
    /// operations' refusals.
    pub fn may_make_section_switch(
        &self,
        tree: TreeId,
        index: u32,
    ) -> Result<SectionId, SwitchRefusal> {
        let held = self
            .tree(tree)
            .ok_or(SwitchRefusal::NoSuchTree(tree))?
            .interface();
        let inputs = held.inputs();
        let port = inputs
            .get(index as usize)
            .ok_or(SwitchRefusal::NoSuchPort {
                tree,
                index,
                arity: u32::try_from(inputs.len()).unwrap_or(u32::MAX),
            })?;
        let switchable = K::switch_type().ok_or(SwitchRefusal::NoSwitchType)?;
        if port.flow.value_type() != Some(&switchable) {
            return Err(SwitchRefusal::NotSwitchable { index });
        }
        let section = held
            .section_of(InterfacePort::input(index))
            .ok_or(SwitchRefusal::NotInASection { index })?;
        match held.section(section).and_then(Section::switch) {
            Some(taken) if taken != index => Err(SwitchRefusal::SectionHasSwitch {
                section,
                port: taken,
            }),
            _ => Ok(section),
        }
    }

    /// Make an interface input the switch for the section it is in.
    ///
    /// The port keeps its own name — the reference overwrites it with the
    /// panel's, and its unlink overwrites it again, so the authored name does
    /// not survive the round trip there. It also moves to the front of the
    /// section's shown order without moving its **index**, so no link at any
    /// instance is touched.
    ///
    /// # Errors
    ///
    /// Whatever [`Document::may_make_section_switch`] refuses.
    pub fn make_section_switch(
        &mut self,
        tree: TreeId,
        index: u32,
    ) -> Result<SectionId, SwitchRefusal> {
        let section = self.may_make_section_switch(tree, index)?;
        let held = self
            .section_mut(tree, section)
            .map_err(|_| SwitchRefusal::NoSuchSection { tree, section })?;
        held.switch = Some(index);
        let port = InterfacePort::input(index);
        held.members.retain(|member| *member != port);
        held.members.insert(0, port);
        Ok(section)
    }

    /// Whether a section could be given a **new** switch, without giving it one.
    ///
    /// ★ The order of the checks is deliberate and is what makes this askable
    /// at all: [`SwitchRefusal::NoSwitchType`] comes **before** the section
    /// lookup, because an application that declares no two-state type can never
    /// have a section switch whatever section is named. A screen with no
    /// sections yet can therefore find out that it has none to gain, which a
    /// section-first order would answer with a refusal about the wrong thing.
    ///
    /// # Errors
    ///
    /// [`SwitchRefusal::NoSuchTree`], [`SwitchRefusal::NoSwitchType`],
    /// [`SwitchRefusal::NoSuchSection`] or [`SwitchRefusal::SectionHasSwitch`].
    pub fn may_new_section_switch(
        &self,
        tree: TreeId,
        section: SectionId,
    ) -> Result<(), SwitchRefusal> {
        let face = self
            .tree(tree)
            .ok_or(SwitchRefusal::NoSuchTree(tree))?
            .interface();
        if K::switch_type().is_none() {
            return Err(SwitchRefusal::NoSwitchType);
        }
        let held = face
            .section(section)
            .ok_or(SwitchRefusal::NoSuchSection { tree, section })?;
        match held.switch {
            Some(port) => Err(SwitchRefusal::SectionHasSwitch { section, port }),
            None => Ok(()),
        }
    }

    /// Add a new switchable input to a section and make it that section's
    /// switch, answering its interface index.
    ///
    /// The new port is named after the section, because there is no authored
    /// name to lose — which is the one place the reference's naming is right and
    /// the reason [`Document::make_section_switch`] does not copy it.
    ///
    /// A call site of [`Document::may_new_section_switch`], so the question a
    /// screen asks and the edit it then makes are one rule.
    ///
    /// # Errors
    ///
    /// Whatever [`Document::may_new_section_switch`] refuses.
    pub fn new_section_switch(
        &mut self,
        tree: TreeId,
        section: SectionId,
    ) -> Result<u32, SwitchRefusal> {
        self.may_new_section_switch(tree, section)?;
        let held = self
            .tree(tree)
            .ok_or(SwitchRefusal::NoSuchTree(tree))?
            .interface()
            .section(section)
            .ok_or(SwitchRefusal::NoSuchSection { tree, section })?;
        let name = held.name.clone();
        let switchable = K::switch_type().ok_or(SwitchRefusal::NoSwitchType)?;
        let index = self
            .expose(tree, InterfaceSide::Input, Port::new(name, switchable))
            .map_err(|_| SwitchRefusal::NoSuchTree(tree))?;
        let held = self
            .section_mut(tree, section)
            .map_err(|_| SwitchRefusal::NoSuchSection { tree, section })?;
        held.switch = Some(index);
        held.members.insert(0, InterfacePort::input(index));
        Ok(index)
    }

    /// Make a section's switch an ordinary port of it again, answering the index
    /// it was.
    ///
    /// The port stays in the section — *stand-alone* in the reference's label
    /// means no longer the toggle, not removed — and keeps its name, which there
    /// it does not.
    ///
    /// # Errors
    ///
    /// [`SwitchRefusal::NoSuchTree`], [`SwitchRefusal::NoSuchSection`] or
    /// [`SwitchRefusal::SectionHasNoSwitch`].
    pub fn unlink_section_switch(
        &mut self,
        tree: TreeId,
        section: SectionId,
    ) -> Result<u32, SwitchRefusal> {
        if self.tree(tree).is_none() {
            return Err(SwitchRefusal::NoSuchTree(tree));
        }
        let held = self
            .section_mut(tree, section)
            .map_err(|_| SwitchRefusal::NoSuchSection { tree, section })?;
        held.switch
            .take()
            .ok_or(SwitchRefusal::SectionHasNoSwitch { section })
    }

    /// Every way a tree's sections break their own rules, as
    /// [`Violation`](crate::Violation)s.
    ///
    /// Its own function for the reason [`Document::validate`]'s per-link
    /// admission check is: that one asks about two ports and this asks about a
    /// section and the interface under it. `validate` is at its line budget and
    /// this is what keeps it there.
    pub(crate) fn section_violations(&self, tree: TreeId) -> Vec<crate::Violation> {
        self.section_breaches(tree)
            .into_iter()
            .map(|(section, breach)| crate::Violation::SectionBroken {
                tree,
                section,
                breach,
            })
            .collect()
    }

    /// The raw findings the wrapper above dresses as violations.
    fn section_breaches(&self, tree: TreeId) -> Vec<(SectionId, SectionBreach)> {
        let Some(interface) = self.tree(tree).map(crate::Tree::interface) else {
            return Vec::new();
        };
        let switchable = K::switch_type();
        let mut found = Vec::new();
        for section in interface.sections() {
            for member in section.members() {
                let arity = u32::try_from(interface.side(member.side).len()).unwrap_or(u32::MAX);
                if member.index >= arity {
                    found.push((section.id, SectionBreach::NoSuchMember(*member)));
                } else if interface
                    .sections()
                    .iter()
                    .filter(|other| other.members.contains(member))
                    .count()
                    > 1
                {
                    found.push((section.id, SectionBreach::MemberShared(*member)));
                }
            }
            if let Some(index) = section.switch {
                let port = InterfacePort::input(index);
                if section.members.contains(&port) {
                    let carries = interface
                        .inputs()
                        .get(index as usize)
                        .is_some_and(|held| held.flow.value_type() == switchable.as_ref());
                    if !carries {
                        found.push((section.id, SectionBreach::SwitchNotSwitchable(index)));
                    }
                } else {
                    found.push((section.id, SectionBreach::SwitchNotAMember(index)));
                }
            }
        }
        found
    }

    /// A tree's interface, mutably.
    fn interface_mut(&mut self, tree: TreeId) -> Result<&mut Interface<K>, EditError> {
        self.interface_of_mut(tree)
            .ok_or(EditError::NoSuchTree(tree))
    }

    /// One section of one tree, mutably.
    fn section_mut(&mut self, tree: TreeId, section: SectionId) -> Result<&mut Section, EditError> {
        self.interface_mut(tree)?
            .sections
            .iter_mut()
            .find(|held| held.id == section)
            .ok_or(EditError::NoSuchSection { tree, section })
    }
}
