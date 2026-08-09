//! R1569 §5.39 §5.20 — chords, and which layer gets them first.
//!
//! A window has **accelerator layers**: bindings that fire from anywhere in
//! the window regardless of what has focus. pinion has two — the §5.20
//! mnemonic map (R1543, <kbd>Alt</kbd>+char derived from painted labels) and
//! the binding's own [`WidgetCore::keybinding`](crate::WidgetCore::keybinding)
//! character map. Both run **ahead** of the focused widget, which is right for
//! an accelerator and wrong for a widget that is being typed into.
//!
//! Every toolkit needs the escape hatch, because the extreme case is real: a
//! text field must keep the letter `d` even in a window where `d` is a
//! shortcut, and a *key-sequence editor* must be able to record
//! <kbd>Alt</kbd>+<kbd>F</kbd> in a window whose File menu claims it. The
//! toolkit spells the hatch `ShortcutOverride` — an event delivered to the
//! focus widget before shortcut processing, which the widget `accept()`s to
//! claim the key.
//!
//! Before R1569 pinion had **none of it**, and the consequence shipped: in
//! `hello-textfield`, which binds `d` → `Disable`, typing `d` into the focused field disabled the
//! field and the character never arrived. Four bindings in the tree carried
//! that defect. The toolkit does not have it — line edit accepts `ShortcutOverride` for any
//! unmodified printable key — so this was one of the places the tree sat
//! *below* the floor rather than above it.
//!
//! ## The shape, and why it is not the toolkit's
//!
//! [`External::shadows_accelerator`](crate::external::External::shadows_accelerator)
//! is a **question the router asks**, not an event a widget must remember to
//! accept. The difference is not stylistic:
//!
//! * the toolkit's override must be accepted on *every* press. A widget that handles a
//!   key in `keyPressEvent` but forgets the `ShortcutOverride` arm in
//!   `event()` loses exactly the presses that collide with a shortcut — a
//!   defect that is invisible until someone adds the colliding shortcut, in a
//!   different file, possibly years later. Here the widget cannot be asked
//!   too late, because the router asks before it dispatches.
//! * the toolkit's override leaves **no record**. shortcut map is private and the
//!   event is transient, so a toolkit application can answer "what does
//!   <kbd>Alt</kbd>+<kbd>F</kbd> do right now". `scene/accelerators` answers
//!   it, because the shadow is a function the router can evaluate against the
//!   published map without anyone pressing anything.
//!
//! The question is **per chord**, and the two consumers in this round give
//! different answers to it, which is why it is not a bool on the widget:
//!
//! | Widget | shadows a bare `d` | shadows <kbd>Alt</kbd>+<kbd>F</kbd> |
//! |---|---|---|
//! | [`TextFieldExternal`](crate::widgets::text_field::TextFieldExternal) | yes — it is text | **no** — a mnemonic still works while typing, as in the toolkit |
//! | [`KeySequenceEditExternal`](crate::widgets::key_sequence::KeySequenceEditExternal) | yes | **yes** — recording a chord means recording *that* chord |
//!
//! ## Spelling
//!
//! [`Chord::portable`] renders the toolkit's `PortableText` vocabulary (`Ctrl`
//! / `Alt` / `Shift` / `Meta`) in a fixed order, and [`Chord::parse`] reads it
//! back. The round trip is a **guarantee** here and is not one in the toolkit:
//! `fromString` maps an unrecognised name to `Key_unknown` and reports
//! nothing, so `key sequence("Ctrl+Frobnicate")` is a silently wrong shortcut.
//! [`ChordParseError`] names which part failed instead.
//!
//! The modifier *order* is fixed here rather than borrowed: the toolkit's
//! lives in a private table in `qkeysequence.cpp`, so there is nothing to be compatible with
//! — only something to be canonical about, and canonical is what makes the
//! round trip a property rather than a coincidence.
//!
//! ## Stated limit: a chord's identity is the W3C `key`, not the physical key
//!
//! key sequence stores a **key code** plus modifier bits, so `Ctrl+K` and `Ctrl+Shift+K` share
//! one `Key_K`. A [`Chord`] stores the W3C `KeyboardEvent.key`, which is the *interpreted* value — `"k"`
//! unmodified and `"K"` with <kbd>Shift</kbd> — so those are two distinct chords
//! with two distinct spellings. Both round-trip, and both describe what the
//! user actually pressed; what they cannot do is identify the physical key
//! across layouts, which needs W3C's `KeyboardEvent.code` (`"KeyK"`). This framework's key wire
//! carries `key` and not `code`, so that axis is absent upstream of this type
//! rather than declined by it.

use std::fmt;

use crate::input::Modifiers;

/// R1569 §5.39 — one chord as the accelerator layers see it.
///
/// `key` is a W3C [`KeyboardEvent.key`] string — a single printable codepoint
/// (`"d"`, `"漢"`) or a named key (`"Enter"`, `"F5"`, `"ArrowLeft"`) — paired
/// with the modifier state held when it arrived.
///
/// [`KeyboardEvent.key`]: https://www.w3.org/TR/uievents-key/
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chord {
    key: String,
    modifiers: Modifiers,
}

/// The modifier names, in the order [`Chord::portable`] emits them.
///
/// Fixed rather than borrowed — see the module docs. Names only: the pairing
/// with the [`Modifiers`] field each one addresses lives in [`modifier_bit`],
/// so the renderer and the parser read ONE mapping and cannot drift into
/// disagreeing about which bit `"Alt"` means.
const MODIFIER_ORDER: [&str; 4] = ["Ctrl", "Alt", "Shift", "Meta"];

/// The one name -> [`Modifiers`] field mapping, as a place.
///
/// `&mut` in both directions because a single accessor is the point: a reader
/// and a writer that matched on the name separately would be two tables, and
/// the failure they permit — `"Alt"` setting one bit and reading another — is
/// silent. Callers that only read hand it a local copy.
fn modifier_bit<'a>(modifiers: &'a mut Modifiers, name: &str) -> Option<&'a mut bool> {
    Some(match name {
        "Ctrl" => &mut modifiers.ctrl,
        "Alt" => &mut modifiers.alt,
        "Shift" => &mut modifiers.shift,
        "Meta" => &mut modifiers.meta,
        _ => return None,
    })
}

impl Chord {
    /// A chord from a W3C key string and the modifier state it arrived with.
    #[must_use]
    pub fn new(key: impl Into<String>, modifiers: Modifiers) -> Self {
        Self {
            key: key.into(),
            modifiers,
        }
    }

    /// The W3C `KeyboardEvent.key` string.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The modifier state held when the chord arrived.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Whether `key` names a modifier rather than a key pressed *with* one.
    ///
    /// The toolkit's `keyPressEvent` returns early on exactly this set and records
    /// nothing, so a held <kbd>Ctrl</kbd> is invisible to a toolkit keymap
    /// editor. [`KeySequenceEdit`](crate::widgets::key_sequence::KeySequenceEdit) publishes
    /// it as a pending prefix instead — the same fact, kept rather than
    /// dropped.
    #[must_use]
    pub fn is_modifier_only(&self) -> bool {
        matches!(
            self.key.as_str(),
            "Control" | "Shift" | "Alt" | "Meta" | "AltGraph" | "CapsLock" | "Super" | "Hyper"
        )
    }

    /// Whether this chord would produce **text** in an editable field.
    ///
    /// This is the toolkit's own rule, from `processShortcutOverrideEvent`: an unmodified (or
    /// <kbd>Shift</kbd>-only) printable key is text, and text belongs to the
    /// field rather than to a shortcut. A single-codepoint W3C `key` string *is*
    /// the printable test — the R666 auto-discriminator routes multi-codepoint
    /// names (`"Enter"`, `"F5"`) down the named-key arc, which never reaches an
    /// accelerator layer at all.
    ///
    /// <kbd>Alt</kbd> is excluded deliberately: <kbd>Alt</kbd>+char is the
    /// mnemonic vocabulary (R1543), so treating it as text would make a text
    /// field swallow every accelerator in the window — which is neither the
    /// toolkit's behaviour nor any toolkit's.
    ///
    /// **Stated limit**: on layouts where `AltGr` composes a character, W3C sets
    /// `ctrlKey` *and* `altKey` alongside the composed `key`, so such a
    /// keystroke reads as a command chord here and is not claimed as text.
    /// Distinguishing it needs the `AltGraph` modifier the pointer/key wire
    /// does not carry ([`Modifiers`] has four bits, W3C's `getModifierState`
    /// has more).
    #[must_use]
    pub fn is_text_bearing(&self) -> bool {
        self.key.chars().count() == 1
            && !self.modifiers.ctrl
            && !self.modifiers.alt
            && !self.modifiers.meta
    }

    /// Whether this chord is a **command** chord — <kbd>Ctrl</kbd> or
    /// <kbd>Meta</kbd> held.
    ///
    /// R879 named the distinction in `edit_field_keymap` ("a Ctrl/Meta chord
    /// is a *command* (select-all, clipboard), not text input"); this is the
    /// same predicate reached from the chord rather than from loose
    /// [`Modifiers`], so the two cannot disagree.
    #[must_use]
    pub fn is_command(&self) -> bool {
        self.modifiers.command_key()
    }

    /// The toolkit's `PortableText` spelling — `"Ctrl+Shift+P"`.
    ///
    /// Round-trips through [`Chord::parse`] for every chord this type can
    /// hold, which is the property the toolkit does not have.
    #[must_use]
    pub fn portable(&self) -> String {
        let mut out = String::new();
        let mut probe = self.modifiers;
        for name in MODIFIER_ORDER {
            if modifier_bit(&mut probe, name).is_some_and(|bit| *bit) {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&self.key);
        out
    }

    /// Read a [`Chord::portable`] spelling back.
    ///
    /// # Errors
    ///
    /// Returns the [`ChordParseError`] naming which part failed. The toolkit's
    /// `fromString` has no error channel at all — an
    /// unrecognised key name becomes `Key_unknown` and the caller is told
    /// nothing, so a typo in a config file becomes a shortcut that silently
    /// never fires.
    pub fn parse(source: &str) -> Result<Self, ChordParseError> {
        if source.is_empty() {
            return Err(ChordParseError::Empty);
        }
        let mut modifiers = Modifiers::empty();
        let mut rest = source;
        // A `+` is both the separator and a legal key, so the split walks
        // prefixes rather than splitting the whole string: `"Ctrl++"` is
        // Ctrl plus the `+` key, and `"+"` alone is the bare `+` key.
        loop {
            let Some(sep) = rest.find('+') else { break };
            if sep == 0 {
                // `rest` starts with `+`: this is the key, not a separator.
                break;
            }
            let (head, tail) = rest.split_at(sep);
            let Some(bit) = modifier_bit(&mut modifiers, head) else {
                // Not a modifier name — `head` is the key and a `+` follows,
                // which no chord spelling permits.
                return Err(ChordParseError::UnknownModifier(head.to_owned()));
            };
            *bit = true;
            rest = &tail[1..];
        }
        if rest.is_empty() {
            return Err(ChordParseError::MissingKey);
        }
        Ok(Self::new(rest, modifiers))
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.portable())
    }
}

/// R1569 §5.39 — why a [`Chord::portable`] spelling could not be read back.
///
/// The toolkit's `fromString` answers a key sequence in every case,
/// substituting `Key_unknown` for anything it did not recognise, so a
/// caller cannot tell a valid chord from a typo without comparing the
/// round trip itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordParseError {
    /// The spelling was empty.
    Empty,
    /// A `+`-separated segment ahead of the key was not a modifier name.
    UnknownModifier(String),
    /// The spelling ended on a separator, so no key was named.
    MissingKey,
}

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty chord"),
            Self::UnknownModifier(name) => {
                write!(
                    f,
                    "unknown modifier {name:?} (expected Ctrl / Alt / Shift / Meta)"
                )
            }
            Self::MissingKey => f.write_str("chord names modifiers but no key"),
        }
    }
}

impl std::error::Error for ChordParseError {}

/// R1569 §5.39 — which of a window's two accelerator layers claims a chord.
///
/// A property of the **window**, not of any widget: whether `Ctrl+S` is already
/// spoken for is decided by what the window paints and what its binding maps,
/// so a keymap editor asking "would this collide" is asking about the window.
/// The toolkit cannot answer it at all — shortcut map is private, so a toolkit
/// application cannot enumerate its own accelerators, and key-sequence editor
/// will record a chord that is already a shortcut with the collision surfacing
/// later, at dispatch, as `isAmbiguous()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorLayer {
    /// The §5.20 mnemonic map — some painted label declares this
    /// <kbd>Alt</kbd>+char (R1543).
    Mnemonic,
    /// The binding's `WidgetCore::keybinding` character map.
    ///
    /// That map is **modifier-blind** — the shell consults it with the
    /// character alone — so `Ctrl+S` collides with a `keybinding("s")` just as
    /// a bare `s` does. Reporting it any other way would describe a dispatch
    /// order the framework does not have.
    Keybinding,
}

impl AcceleratorLayer {
    /// Stable wire name.
    #[must_use]
    pub const fn as_name(self) -> &'static str {
        match self {
            Self::Mnemonic => "mnemonic",
            Self::Keybinding => "keybinding",
        }
    }
}

/// R1569 §5.39 — an ordered run of [`Chord`]s, the toolkit's key sequence.
///
/// The toolkit fixes the capacity at four (`MaxKeyCount`) and key-sequence editor
/// exposes it as `maximumSequenceLength`. Here the bound is carried by the value rather than by
/// the type, because a keymap editor and a chord *display* want different ones
/// and the toolkit's constant is the reason key sequence silently truncates a
/// longer sequence rather than reporting it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeySequence {
    chords: Vec<Chord>,
}

/// The toolkit's `MaxKeyCount`, which key-sequence editor also
/// takes as its default `maximumSequenceLength`.
pub const QT_MAX_SEQUENCE_LENGTH: usize = 4;

impl KeySequence {
    /// The empty sequence.
    #[must_use]
    pub const fn new() -> Self {
        Self { chords: Vec::new() }
    }

    /// The chords, in the order they were recorded.
    #[must_use]
    pub fn chords(&self) -> &[Chord] {
        &self.chords
    }

    /// How many chords the sequence holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chords.len()
    }

    /// Whether the sequence holds no chords.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chords.is_empty()
    }

    /// Append `chord`, refusing rather than truncating once `max` is reached.
    ///
    /// # Errors
    ///
    /// [`SequenceFull`] when the sequence already holds `max` chords. The toolkit's `operator=` /
    /// `setKeySequence` **truncate** silently — the documented behaviour is "if the
    /// sequence is longer than `maximumSequenceLength()`, it is truncated" — so a toolkit caller
    /// cannot tell a sequence that fit from one that was cut down to fit.
    pub fn push(&mut self, chord: Chord, max: usize) -> Result<usize, SequenceFull> {
        if self.chords.len() >= max {
            return Err(SequenceFull {
                max,
                dropped: chord,
            });
        }
        self.chords.push(chord);
        Ok(self.chords.len() - 1)
    }

    /// Drop every chord.
    pub fn clear(&mut self) {
        self.chords.clear();
    }

    /// The toolkit's `PortableText` spelling — chords joined by `", "`,
    /// which is the toolkit's own separator for a multi-chord sequence.
    ///
    /// Round-trips through [`KeySequence::parse`].
    #[must_use]
    pub fn portable(&self) -> String {
        self.chords
            .iter()
            .map(Chord::portable)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Read a [`KeySequence::portable`] spelling back.
    ///
    /// # Errors
    ///
    /// The first [`ChordParseError`] any chord in the run produced.
    pub fn parse(source: &str) -> Result<Self, ChordParseError> {
        if source.is_empty() {
            return Ok(Self::new());
        }
        let mut chords = Vec::new();
        for part in source.split(", ") {
            chords.push(Chord::parse(part)?);
        }
        Ok(Self { chords })
    }
}

/// R1569 §5.39 — a chord could not be appended because the sequence was full.
///
/// Carries the chord that was **dropped**, so the refusal names the fact
/// rather than merely reporting that one happened — the R1564 / R1565 shape.
/// The toolkit has no channel here at all: the sequence is truncated and
/// nothing is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceFull {
    /// The declared maximum the sequence had already reached.
    pub max: usize,
    /// The chord that did not fit.
    pub dropped: Chord,
}

impl fmt::Display for SequenceFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sequence already holds its maximum of {} chords; {} was not recorded",
            self.max,
            self.dropped.portable(),
        )
    }
}

impl std::error::Error for SequenceFull {}

#[cfg(test)]
mod tests {
    use super::{Chord, ChordParseError, KeySequence, QT_MAX_SEQUENCE_LENGTH, SequenceFull};
    use crate::input::Modifiers;

    /// Build a modifier state from its own portable NAMES.
    ///
    /// Not four positional bools: `mods(false, true, ..)` and
    /// `mods(true, false, ..)` read identically, and clippy's four-bool limit
    /// is pointing at exactly that. Going through [`modifier_bit`] also means a
    /// test cannot assert against a mapping the production code does not have.
    fn mods(names: &[&str]) -> Modifiers {
        let mut m = Modifiers::empty();
        for name in names {
            *super::modifier_bit(&mut m, name).expect("a declared modifier name") = true;
        }
        m
    }

    /// The `bits`-th subset of [`MODIFIER_ORDER`], for the exhaustive lattice.
    fn mods_from_bits(bits: u8) -> Modifiers {
        let names: Vec<&str> = super::MODIFIER_ORDER
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & (1 << i) != 0)
            .map(|(_, name)| *name)
            .collect();
        mods(&names)
    }

    #[test]
    fn portable_emits_the_declared_order() {
        let c = Chord::new("P", mods(&["Ctrl", "Alt", "Shift", "Meta"]));
        assert_eq!(c.portable(), "Ctrl+Alt+Shift+Meta+P");
    }

    #[test]
    fn every_representable_chord_round_trips() {
        // The property the toolkit does not have. Exhaustive over the modifier
        // lattice rather than sampled, because the failure this guards is a
        // single arm of the table disagreeing with its parser.
        for bits in 0..16u8 {
            let m = mods_from_bits(bits);
            for key in ["a", "F5", "Enter", "+", "漢"] {
                let c = Chord::new(key, m);
                let back = Chord::parse(&c.portable()).expect("round trip");
                assert_eq!(
                    back,
                    c,
                    "{:?} did not survive its own spelling",
                    c.portable()
                );
            }
        }
    }

    #[test]
    fn an_unknown_modifier_is_named_not_swallowed() {
        // The toolkit answers `Key_unknown` here and reports nothing.
        assert_eq!(
            Chord::parse("Ctrl+Frobnicate+P"),
            Err(ChordParseError::UnknownModifier("Frobnicate".to_owned())),
        );
        assert_eq!(Chord::parse(""), Err(ChordParseError::Empty));
        assert_eq!(Chord::parse("Ctrl+"), Err(ChordParseError::MissingKey));
    }

    #[test]
    fn text_bearing_is_qts_line_edit_rule() {
        // Unmodified or Shift-only printable = text.
        assert!(Chord::new("d", Modifiers::empty()).is_text_bearing());
        assert!(Chord::new("D", mods(&["Shift"])).is_text_bearing());
        // A command chord is not text (Ctrl+C is clipboard, not a `c`).
        assert!(!Chord::new("c", mods(&["Ctrl"])).is_text_bearing());
        // Alt+char is the mnemonic vocabulary, never text.
        assert!(!Chord::new("f", mods(&["Alt"])).is_text_bearing());
        // A named key is not a printable codepoint.
        assert!(!Chord::new("Enter", Modifiers::empty()).is_text_bearing());
    }

    #[test]
    fn modifier_only_is_recognised_by_name() {
        assert!(Chord::new("Control", Modifiers::empty()).is_modifier_only());
        assert!(Chord::new("Shift", Modifiers::empty()).is_modifier_only());
        assert!(!Chord::new("a", Modifiers::empty()).is_modifier_only());
    }

    #[test]
    fn a_full_sequence_refuses_and_names_the_dropped_chord() {
        // The toolkit truncates here and returns nothing, so a caller cannot
        // tell a sequence that fit from one that was cut down to fit.
        let mut seq = KeySequence::new();
        for i in 0..QT_MAX_SEQUENCE_LENGTH {
            assert_eq!(
                seq.push(Chord::new("a", Modifiers::empty()), QT_MAX_SEQUENCE_LENGTH),
                Ok(i),
            );
        }
        let overflow = Chord::new("z", mods(&["Ctrl"]));
        assert_eq!(
            seq.push(overflow.clone(), QT_MAX_SEQUENCE_LENGTH),
            Err(SequenceFull {
                max: QT_MAX_SEQUENCE_LENGTH,
                dropped: overflow,
            }),
        );
        assert_eq!(
            seq.len(),
            QT_MAX_SEQUENCE_LENGTH,
            "the refusal changed nothing"
        );
    }

    #[test]
    fn a_sequence_round_trips_through_its_own_spelling() {
        let mut seq = KeySequence::new();
        seq.push(Chord::new("k", mods(&["Ctrl"])), 4).expect("fits");
        seq.push(Chord::new("S", mods(&["Ctrl", "Shift"])), 4)
            .expect("fits");
        assert_eq!(seq.portable(), "Ctrl+k, Ctrl+Shift+S");
        assert_eq!(KeySequence::parse(&seq.portable()), Ok(seq));
        assert_eq!(KeySequence::parse(""), Ok(KeySequence::new()));
    }

    /// The spelling carries the W3C `key` VERBATIM. Pinned because the tempting
    /// alternative — upper-casing a letter the way the toolkit's keycode model
    /// renders it — would break the round trip that is this type's whole claim
    /// over `fromString`, and would do so only for letters, which is the shape of a
    /// defect nobody notices until a config file stops loading.
    #[test]
    fn case_is_the_wires_own_and_shift_is_not_folded_into_it() {
        let plain = Chord::new("k", mods(&["Ctrl"]));
        let shifted = Chord::new("K", mods(&["Ctrl", "Shift"]));
        assert_eq!(plain.portable(), "Ctrl+k");
        assert_eq!(shifted.portable(), "Ctrl+Shift+K");
        assert_ne!(plain, shifted, "two different keystrokes, two chords");
        for c in [plain, shifted] {
            assert_eq!(Chord::parse(&c.portable()), Ok(c));
        }
    }
}
