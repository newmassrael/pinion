//! **A string that has to parse**, declared as data.
//!
//! A configuration form holds three kinds of string, and the difference between
//! them is not cosmetic — it is what happens downstream:
//!
//! * one of a fixed set of words, which a picker can offer
//!   ([`FieldType::Choice`](super::config_form::FieldType::Choice)),
//! * **a string with a shape**, which the thing being configured parses and
//!   dies on when it cannot,
//! * free text, which anything downstream accepts.
//!
//! The middle one had no spelling here before, so every such field was typed as
//! free text and the tool accepted values the target refuses. This module is
//! that spelling: [`TextFormat`] declares the shape, and [`TextFormat::judge`]
//! answers what a caret-level validator answers — with two things such a
//! validator does not have.
//!
//! # Three states, and why the middle one exists
//!
//! [`Judgement::Unfinished`] is not a softer refusal. It is the difference
//! between "this can still become right" and "it cannot", and a field that
//! cannot tell them apart must either reject the user's third keystroke of a
//! six-character value or accept the finished value blind. Both are wrong, and
//! the second is how a configuration reaches a target that then refuses to
//! start.
//!
//! # What this says that a pattern cannot
//!
//! A validator that answers only a state leaves the person and the agent to
//! guess. This answers, with every refusal:
//!
//! * [`wanted`](Judgement::wanted) — a sentence naming the shape, derived from
//!   the declaration rather than written beside it, so it cannot drift from
//!   what is actually checked, and
//! * [`at`](Judgement::at) — the byte offset the text first went wrong at, so a
//!   caller can point at the character rather than the field.
//!
//! And because the declaration is **data** — serialisable, comparable, walkable
//! — the shape travels: an agent can read what a field will accept before
//! writing to it, and a saved document carries the grammar it was checked
//! against. A closure could do the checking and none of that.
//!
//! # Composition
//!
//! The arms are primitives with no domain in them; a domain shape is built by
//! composing. An address of the `<scheme>/<host>:<port>` kind is
//! [`TextFormat::Then`] of a [`TextFormat::Word`] and a `Then` of a host and a
//! [`TextFormat::Number`] — no arm of this enum knows what an address is, which
//! is why the next domain does not need a new arm.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// An inclusive count bound.
///
/// A pair rather than a `Range` so it is `Copy`, `Eq` and serialisable, all of
/// which [`TextFormat`] needs and none of which the standard range types are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Fewest, inclusive. Text shorter than this is
    /// [`Judgement::Unfinished`] rather than refused.
    pub min: usize,
    /// Most, inclusive.
    pub max: usize,
}

impl Span {
    /// Exactly `n`.
    #[must_use]
    pub const fn exactly(n: usize) -> Self {
        Self { min: n, max: n }
    }

    /// From `min` to `max`, inclusive.
    #[must_use]
    pub const fn between(min: usize, max: usize) -> Self {
        Self { min, max }
    }

    /// How this reads in a sentence.
    #[must_use]
    pub fn wanted(self) -> String {
        if self.min == self.max {
            format!("{}", self.min)
        } else {
            format!("{} to {}", self.min, self.max)
        }
    }
}

/// A named set of characters.
///
/// Named rather than spelled out because the name is what a sentence needs:
/// "a lower-case hexadecimal digit" reads, and the sixteen characters do not.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, pinion_derive::VariantCensus,
)]
#[serde(rename_all = "snake_case")]
pub enum CharClass {
    /// `0`–`9`.
    Digit,
    /// `0`–`9` and `a`–`f`. Lower case only, because a value that round-trips
    /// through a document has one spelling or it compares unequal to itself.
    LowerHex,
    /// `a`–`z`.
    Lower,
    /// `A`–`Z`.
    Upper,
    /// Any ASCII letter.
    Letter,
}

impl CharClass {
    /// Whether `c` is in this class.
    #[must_use]
    pub const fn holds(self, c: char) -> bool {
        match self {
            Self::Digit => c.is_ascii_digit(),
            Self::LowerHex => c.is_ascii_digit() || matches!(c, 'a'..='f'),
            Self::Lower => c.is_ascii_lowercase(),
            Self::Upper => c.is_ascii_uppercase(),
            Self::Letter => c.is_ascii_alphabetic(),
        }
    }

    /// How this reads in a sentence.
    #[must_use]
    pub const fn wanted(self) -> &'static str {
        match self {
            Self::Digit => "digits",
            Self::LowerHex => "lower-case hexadecimal digits",
            Self::Lower => "lower-case letters",
            Self::Upper => "upper-case letters",
            Self::Letter => "letters",
        }
    }
}

/// The characters a [`TextFormat::Chars`] admits: any of the named classes, or
/// any of the extra characters spelled out.
///
/// The union is what keeps the classes small. A host label is letters, digits
/// and two punctuation marks — a `Hostname` class would be this module learning
/// a domain, which is the thing it must not do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharSet {
    /// Classes, any of which admits a character.
    pub classes: Vec<CharClass>,
    /// Further characters admitted literally.
    pub extra: Cow<'static, str>,
}

impl CharSet {
    /// Just these classes.
    #[must_use]
    pub fn of(classes: &[CharClass]) -> Self {
        Self {
            classes: classes.to_vec(),
            extra: Cow::Borrowed(""),
        }
    }

    /// These classes, and these further characters.
    #[must_use]
    pub fn and(mut self, extra: impl Into<Cow<'static, str>>) -> Self {
        self.extra = extra.into();
        self
    }

    /// Whether `c` is admitted.
    #[must_use]
    pub fn holds(&self, c: char) -> bool {
        self.classes.iter().any(|k| k.holds(c)) || self.extra.contains(c)
    }

    /// How this reads in a sentence.
    #[must_use]
    pub fn wanted(&self) -> String {
        let mut parts: Vec<String> = self
            .classes
            .iter()
            .map(|k| (*k).wanted().to_string())
            .collect();
        if !self.extra.is_empty() {
            parts.push(format!("any of {}", self.extra));
        }
        if parts.is_empty() {
            return "nothing".to_string();
        }
        parts.join(" or ")
    }
}

/// The shape a string has to have.
///
/// See the [module documentation](self) for why this is data rather than a
/// predicate, and for how domain shapes are composed from these arms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, pinion_derive::VariantCensus)]
#[serde(rename_all = "snake_case", tag = "format")]
pub enum TextFormat {
    /// Every character is admitted by the set, and the length is inside the
    /// span.
    Chars {
        /// What each character may be.
        allow: CharSet,
        /// How many there may be.
        len: Span,
    },
    /// A whole number in base ten, inside an inclusive range.
    ///
    /// Distinct from [`FieldType::Integer`](super::config_form::FieldType) —
    /// that is a field whose *document value* is a number, this is a number
    /// written inside a larger string, such as the port of an address.
    Number {
        /// Smallest accepted.
        min: i64,
        /// Largest accepted.
        max: i64,
    },
    /// Exactly one of a set of words.
    Word {
        /// The words, in the order a sentence lists them.
        of: Vec<Cow<'static, str>>,
    },
    /// `head`, then the separator, then `tail`. The **first** separator splits.
    ///
    /// First rather than last so that a partially typed value is judged
    /// left-to-right: the head is settled as soon as the separator is typed,
    /// and everything after it is the tail's business.
    Then {
        /// Before the separator.
        head: Box<TextFormat>,
        /// What separates them.
        sep: char,
        /// After it.
        tail: Box<TextFormat>,
    },
    /// The parts between separators, each of the inner shape.
    Split {
        /// What separates the parts.
        sep: char,
        /// What each part is.
        of: Box<TextFormat>,
        /// How many parts there may be.
        parts: Span,
    },
    /// Any one of the alternatives.
    ///
    /// A refusal names the alternative that got **furthest**, because that is
    /// almost always the one the person was typing.
    Either {
        /// The alternatives, in the order a sentence lists them.
        of: Vec<TextFormat>,
    },
}

/// What a text is, against a format.
///
/// Three states because two cannot express a value that is *on its way*. See
/// the [module documentation](self) for what this carries that a bare state
/// does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// The text satisfies the format.
    Acceptable,
    /// Not yet, and some continuation of it would be.
    Unfinished {
        /// The shape, as a sentence.
        wanted: String,
    },
    /// No continuation of it can satisfy the format.
    Refused {
        /// The shape, as a sentence.
        wanted: String,
        /// The byte offset the text first went wrong at.
        at: usize,
    },
}

impl Judgement {
    /// Whether the text is finished and right.
    #[must_use]
    pub const fn acceptable(&self) -> bool {
        matches!(self, Self::Acceptable)
    }

    /// Whether **no** continuation can make it right.
    ///
    /// The question a field asks before rejecting a keystroke: an unfinished
    /// value must be allowed to stand, a refused one is already wrong.
    #[must_use]
    pub const fn refused(&self) -> bool {
        matches!(self, Self::Refused { .. })
    }

    /// The shape, as a sentence — empty when there is nothing to want.
    #[must_use]
    pub fn wanted(&self) -> &str {
        match self {
            Self::Acceptable => "",
            Self::Unfinished { wanted } | Self::Refused { wanted, .. } => wanted,
        }
    }

    /// Where the text went wrong, for a refusal.
    #[must_use]
    pub const fn at(&self) -> Option<usize> {
        match self {
            Self::Refused { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// What a person reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Acceptable => String::new(),
            Self::Unfinished { wanted } => format!("not finished; wanted {wanted}"),
            Self::Refused { wanted, at } => format!("wanted {wanted}, wrong at byte {at}"),
        }
    }
}

impl TextFormat {
    /// A whole-number range, as a nested format.
    #[must_use]
    pub const fn number(min: i64, max: i64) -> Self {
        Self::Number { min, max }
    }

    /// One of these words.
    #[must_use]
    pub fn word(of: &[&'static str]) -> Self {
        Self::Word {
            of: of.iter().map(|w| Cow::Borrowed(*w)).collect(),
        }
    }

    /// `head` then `sep` then `tail`.
    #[must_use]
    pub fn then(head: Self, sep: char, tail: Self) -> Self {
        Self::Then {
            head: Box::new(head),
            sep,
            tail: Box::new(tail),
        }
    }

    /// `parts` of `of`, separated by `sep`.
    #[must_use]
    pub fn split(sep: char, of: Self, parts: Span) -> Self {
        Self::Split {
            sep,
            of: Box::new(of),
            parts,
        }
    }

    /// The shape, as a sentence.
    ///
    /// Derived from the declaration rather than carried beside it, so a format
    /// whose grammar changes cannot keep describing itself the old way.
    #[must_use]
    pub fn wanted(&self) -> String {
        match self {
            Self::Chars { allow, len } => {
                format!("{} {}", len.wanted(), allow.wanted())
            }
            Self::Number { min, max } => format!("a whole number {min} to {max}"),
            Self::Word { of } => {
                let words: Vec<&str> = of.iter().map(Cow::as_ref).collect();
                format!("one of {}", words.join(", "))
            }
            Self::Then { head, sep, tail } => {
                format!("{}{sep}{}", head.wanted(), tail.wanted())
            }
            Self::Split { sep, of, parts } => {
                format!("{} of ({}) separated by {sep}", parts.wanted(), of.wanted())
            }
            Self::Either { of } => {
                let each: Vec<String> = of.iter().map(Self::wanted).collect();
                format!("either {}", each.join(" or "))
            }
        }
    }

    /// **What `text` is**, against this format.
    ///
    /// Total: every text gets one of the three answers, and the answer for a
    /// text that could still grow into an acceptable one is never
    /// [`Judgement::Refused`].
    #[must_use]
    pub fn judge(&self, text: &str) -> Judgement {
        // One arm, one function. Each answers in this format's own words —
        // `self.wanted()` — so a composite's refusal says what the composite
        // wanted while keeping the offset only the inner walk knows.
        match self {
            Self::Chars { allow, len } => self.judge_chars(allow, *len, text),
            Self::Number { min, max } => self.judge_number(*min, *max, text),
            Self::Word { of } => self.judge_word(of, text),
            Self::Then { head, sep, tail } => self.judge_then(head, *sep, tail, text),
            Self::Split { sep, of, parts } => self.judge_split(*sep, of, *parts, text),
            Self::Either { of } => self.judge_either(of, text),
        }
    }

    /// The sentence this format describes itself with, as a refusal at `at`.
    fn refused(&self, at: usize) -> Judgement {
        Judgement::Refused {
            wanted: self.wanted(),
            at,
        }
    }

    /// The same sentence, as "not yet".
    fn unfinished(&self) -> Judgement {
        Judgement::Unfinished {
            wanted: self.wanted(),
        }
    }

    fn judge_chars(&self, allow: &CharSet, len: Span, text: &str) -> Judgement {
        for (offset, c) in text.char_indices() {
            if !allow.holds(c) {
                return self.refused(offset);
            }
        }
        let count = text.chars().count();
        if count > len.max {
            // The offset of the first character past the bound, which is the
            // one that has to go.
            let at = text
                .char_indices()
                .nth(len.max)
                .map_or(text.len(), |(i, _)| i);
            return self.refused(at);
        }
        if count < len.min {
            return self.unfinished();
        }
        Judgement::Acceptable
    }

    fn judge_number(&self, min: i64, max: i64, text: &str) -> Judgement {
        for (offset, c) in text.char_indices() {
            if !c.is_ascii_digit() {
                return self.refused(offset);
            }
        }
        if text.is_empty() {
            return self.unfinished();
        }
        let Ok(n) = text.parse::<i64>() else {
            // Longer than any accepted number: digits only ever make it bigger,
            // so nothing can bring it back.
            return self.refused(text.len());
        };
        if n > max {
            return self.refused(text.len());
        }
        if n < min {
            // Another digit multiplies it by ten, so it can still get there.
            return self.unfinished();
        }
        Judgement::Acceptable
    }

    fn judge_word(&self, of: &[Cow<'static, str>], text: &str) -> Judgement {
        if of.iter().any(|w| w == text) {
            return Judgement::Acceptable;
        }
        if of.iter().any(|w| w.starts_with(text)) {
            return self.unfinished();
        }
        // The furthest any option agreed with the text: that is where it
        // stopped being any of them.
        let at = of
            .iter()
            .map(|w| common_prefix(w, text))
            .max()
            .unwrap_or(0)
            .min(text.len());
        self.refused(at)
    }

    fn judge_then(&self, head: &Self, sep: char, tail: &Self, text: &str) -> Judgement {
        let Some(cut) = text.find(sep) else {
            // No separator yet. The head has to be able to grow into one, and
            // the whole is unfinished either way.
            return match head.judge(text) {
                Judgement::Refused { at, .. } => self.refused(at),
                _ => self.unfinished(),
            };
        };
        match head.judge(&text[..cut]) {
            Judgement::Acceptable => {}
            // The separator ended the head, so "could still grow" is no longer
            // available to it.
            Judgement::Unfinished { .. } => return self.refused(cut),
            Judgement::Refused { at, .. } => return self.refused(at),
        }
        let rest = cut + sep.len_utf8();
        match tail.judge(&text[rest..]) {
            Judgement::Acceptable => Judgement::Acceptable,
            Judgement::Unfinished { .. } => self.unfinished(),
            Judgement::Refused { at, .. } => self.refused(rest + at),
        }
    }

    fn judge_split(&self, sep: char, of: &Self, parts: Span, text: &str) -> Judgement {
        if parts.max == 0 {
            // A format that admits no parts admits no text at all; said here
            // rather than left to underflow the index below.
            return if text.is_empty() {
                Judgement::Acceptable
            } else {
                self.refused(0)
            };
        }
        let pieces: Vec<&str> = text.split(sep).collect();
        if pieces.len() > parts.max {
            // The separator that opened the part too many.
            let at = text
                .match_indices(sep)
                .nth(parts.max - 1)
                .map_or(text.len(), |(i, _)| i);
            return self.refused(at);
        }
        let mut offset = 0usize;
        let last = pieces.len() - 1;
        for (n, piece) in pieces.iter().enumerate() {
            match of.judge(piece) {
                Judgement::Acceptable => {}
                Judgement::Unfinished { .. } if n == last => return self.unfinished(),
                // A part with more after it is finished whether it is ready or
                // not.
                Judgement::Unfinished { .. } => return self.refused(offset + piece.len()),
                Judgement::Refused { at, .. } => return self.refused(offset + at),
            }
            offset += piece.len() + sep.len_utf8();
        }
        if pieces.len() < parts.min {
            return self.unfinished();
        }
        Judgement::Acceptable
    }

    fn judge_either(&self, of: &[Self], text: &str) -> Judgement {
        let judged: Vec<Judgement> = of.iter().map(|f| f.judge(text)).collect();
        if judged.iter().any(Judgement::acceptable) {
            return Judgement::Acceptable;
        }
        if judged
            .iter()
            .any(|j| matches!(j, Judgement::Unfinished { .. }))
        {
            return self.unfinished();
        }
        // The alternative that got furthest is the one being typed.
        let at = judged.iter().filter_map(Judgement::at).max().unwrap_or(0);
        self.refused(at)
    }
}

/// How many bytes of `a` and `b` agree from the start.
///
/// Counted in characters and reported in bytes, so the answer is always a
/// character boundary: it is handed out as an offset into the text, and an
/// offset in the middle of a character points at nothing a caller can show.
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.chars())
        .take_while(|((_, x), y)| x == y)
        .map(|((i, x), _)| i + x.len_utf8())
        .last()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{CharClass, CharSet, Judgement, Span, TextFormat};

    /// A lower-case hexadecimal identifier of one to four digits.
    fn ident() -> TextFormat {
        TextFormat::Chars {
            allow: CharSet::of(&[CharClass::LowerHex]),
            len: Span::between(1, 4),
        }
    }

    /// A dotted quad.
    fn quad() -> TextFormat {
        TextFormat::split('.', TextFormat::number(0, 255), Span::exactly(4))
    }

    /// `<scheme>/<host>:<port>` — the shape the screens' addresses have, built
    /// out of arms that know nothing about addresses.
    fn address() -> TextFormat {
        TextFormat::then(
            TextFormat::word(&["tcp", "tls", "quic", "udp", "ws"]),
            '/',
            TextFormat::then(
                TextFormat::Either {
                    of: vec![
                        quad(),
                        TextFormat::Chars {
                            allow: CharSet::of(&[CharClass::Letter, CharClass::Digit]).and("-."),
                            len: Span::between(1, 253),
                        },
                    ],
                },
                ':',
                TextFormat::number(0, 65535),
            ),
        )
    }

    /// ★★★★★ R1690 — **every prefix of an acceptable text is never refused.**
    ///
    /// This is the property that makes the third state mean something, and the
    /// one a two-state validator cannot have: a field that judges only
    /// right/wrong must call the third keystroke of a six-character value wrong,
    /// because at that instant it is. Typing is a walk through prefixes, so the
    /// invariant is exactly "a value can be typed into this field".
    ///
    /// Stated over a whole corpus and every prefix of every member rather than
    /// over chosen pairs, because the failures this catches are at boundaries
    /// nobody picks by hand — the byte after a separator, a part that is
    /// complete but not the last, a number that is under its minimum.
    #[test]
    fn r1690_every_prefix_of_an_acceptable_text_can_still_become_one() {
        let corpus: &[(TextFormat, &[&str])] = &[
            (ident(), &["a", "ab", "beef", "0f"]),
            (quad(), &["0.0.0.0", "10.0.0.21", "255.255.255.255"]),
            // ★★★ R1690 — a number with a minimum ABOVE zero, and it is here
            // because a counterfactual said so: every `Number` this tree
            // otherwise builds has `min: 0`, so the branch that answers
            // "under the minimum, and another digit can still get there" was
            // dead in the corpus and breaking it changed nothing. A primitive
            // whose arm no fixture reaches is a primitive nothing is checking.
            (
                TextFormat::number(100, 9999),
                &["100", "255", "9999", "1000"],
            ),
            (
                address(),
                &[
                    "tcp/0.0.0.0:7447",
                    "tls/10.0.0.21:7449",
                    "quic/host-one.local:1",
                    "ws/255.255.255.255:65535",
                ],
            ),
        ];
        let mut walked = 0;
        for (format, texts) in corpus {
            for text in *texts {
                assert_eq!(
                    format.judge(text),
                    Judgement::Acceptable,
                    "{text:?} is in the corpus because it is acceptable",
                );
                for end in 0..text.len() {
                    if !text.is_char_boundary(end) {
                        continue;
                    }
                    let prefix = &text[..end];
                    let judged = format.judge(prefix);
                    assert!(
                        !judged.refused(),
                        "{prefix:?} is on the way to {text:?} and was refused: \
                         {}. A refusal here is a field that cannot be typed \
                         into.",
                        judged.sentence(),
                    );
                    walked += 1;
                }
            }
        }
        assert!(
            walked >= 60,
            "the corpus has to walk enough prefixes to be a property and not an \
             example: {walked}",
        );
    }

    /// A refusal names the byte the text went wrong at.
    ///
    /// The offset is the half a caller cannot recompute — it is what lets a
    /// screen point at the character rather than colour the whole row.
    #[test]
    fn r1690_a_refusal_says_where() {
        // The `x` is the fifth byte; everything before it is a host.
        let judged = address().judge("tcp/host!x:1");
        assert_eq!(judged.at(), Some(8), "{}", judged.sentence());
        // A digit group over 255 cannot come back: more digits only grow it.
        let judged = quad().judge("10.300");
        assert!(judged.refused(), "{judged:?}");
        // A fifth group is one too many, refused at the separator that opened
        // it rather than at the end of the text.
        let judged = quad().judge("1.2.3.4.5");
        assert_eq!(judged.at(), Some(7), "{}", judged.sentence());
        // ★ A number under its minimum is UNFINISHED, not refused — another
        // digit multiplies it by ten and can still get there — while one over
        // its maximum is refused, because digits only ever make it bigger.
        let bounded = TextFormat::number(100, 9999);
        assert!(matches!(bounded.judge("9"), Judgement::Unfinished { .. }));
        assert!(bounded.judge("10000").refused());
        // Too long, refused at the first character past the bound.
        assert_eq!(ident().judge("abcde").at(), Some(4));
        // Not hexadecimal at all, refused where it stopped being.
        assert_eq!(ident().judge("abz").at(), Some(2));
    }

    /// The three states are exclusive, and each is reachable for one format.
    ///
    /// Without this the corpus above would pass on a judge that answered
    /// `Unfinished` to everything.
    #[test]
    fn r1690_all_three_states_are_reachable() {
        let f = ident();
        assert!(f.judge("ab").acceptable());
        assert!(matches!(f.judge(""), Judgement::Unfinished { .. }));
        assert!(f.judge("zz").refused());
        // ...and no text is two of them at once, which the enum gives, so the
        // statement worth making is that the corpus exercises the arms rather
        // than one of them: an `Either` reaches all three too.
        let e = address();
        assert!(e.judge("tcp/1.2.3.4:1").acceptable());
        assert!(matches!(
            e.judge("tcp/1.2.3.4:"),
            Judgement::Unfinished { .. }
        ));
        assert!(e.judge("zzz").refused());
    }

    /// A separator ends the part before it, whether or not that part was ready.
    ///
    /// The rule that makes `Then` left-to-right: once the separator is typed,
    /// no continuation can go back and finish the head.
    #[test]
    fn r1690_a_separator_settles_what_came_before_it() {
        let f = TextFormat::then(ident(), '/', TextFormat::number(0, 9));
        assert!(f.judge("ab/3").acceptable());
        // Empty head, then a separator: the head can never be filled in now.
        let judged = f.judge("/3");
        assert!(judged.refused(), "{judged:?}");
        assert_eq!(judged.at(), Some(0));
    }

    /// The sentence a refusal carries is derived from the declaration.
    ///
    /// Asserted by changing the declaration and reading the sentence, not by
    /// comparing to a literal written beside the format: a sentence stored
    /// beside a grammar is the drift this exists to prevent.
    #[test]
    fn r1690_the_sentence_follows_the_declaration() {
        let narrow = TextFormat::Chars {
            allow: CharSet::of(&[CharClass::LowerHex]),
            len: Span::between(1, 4),
        };
        let wide = TextFormat::Chars {
            allow: CharSet::of(&[CharClass::LowerHex]),
            len: Span::between(1, 32),
        };
        assert_ne!(
            narrow.judge("zz").wanted(),
            wide.judge("zz").wanted(),
            "two different bounds must not describe themselves the same way",
        );
        assert!(
            narrow.wanted().contains("hexadecimal"),
            "{}",
            narrow.wanted()
        );
        assert!(
            address().wanted().contains('/') && address().wanted().contains(':'),
            "a composite's sentence shows its separators: {}",
            address().wanted(),
        );
    }

    /// The grammar survives a round trip through the wire form.
    ///
    /// Load-bearing: a format is stored inside a saved document and read by an
    /// agent that wants to know what a field takes, so a shape that could not
    /// be written down would be a shape only this process knows about.
    #[test]
    fn r1690_a_format_round_trips_as_data() {
        let f = address();
        let text = serde_json::to_string(&f).expect("a format is serialisable");
        let back: TextFormat = serde_json::from_str(&text).expect("and readable");
        assert_eq!(f, back);
        assert!(back.judge("tcp/0.0.0.0:7447").acceptable());
    }

    /// An empty character set admits nothing, and says so rather than panicking.
    #[test]
    fn r1690_a_format_that_admits_nothing_refuses_at_the_first_character() {
        let f = TextFormat::Chars {
            allow: CharSet::of(&[]),
            len: Span::between(0, 4),
        };
        assert!(f.judge("").acceptable(), "nothing is still nothing");
        assert_eq!(f.judge("a").at(), Some(0));
        let none = TextFormat::split('.', TextFormat::number(0, 9), Span::exactly(0));
        assert!(none.judge("").acceptable());
        assert!(none.judge("1").refused());
    }
}
