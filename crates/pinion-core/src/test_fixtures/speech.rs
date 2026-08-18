//! R1718 §5.12 §2 #7 — **a type that speaks to a person says every sentence it
//! can say, and a reader can tell them apart.**
//!
//! # The defect this exists for, measured
//!
//! R1716 added a third situation to a screen's launch gate and wrote no wording
//! for it. The situation was carried inside another type's variant, so it fell
//! through to *that* type's wording, and the panel told a person:
//!
//! > R-01 · R-01 · connect.endpoints · tcp/10.0.0.21:7449 is outside this graph
//! > is not a key the target knows; it starts and ignores it
//!
//! — the card named twice, and a sentence about unknown keys glued onto a fact
//! that is not about one. It shipped for a whole round with **every gate
//! green**, because every check over it asked whether the ADDRESS was named.
//! R1717 found it by photographing the panel and gave the fact a type; this is
//! the half that stops the next one, and R1718 measured how much room there was
//! for a next one: of the wordings this workspace can put in front of a person,
//! **11 of 39 were read by any test or demo at all**, and most of those eleven
//! were short fragments a search matched for other reasons. The launch verdict
//! a screen paints, the take-over toast, and two of three configuration defects
//! were among the unread.
//!
//! # The properties, and the defect each one is earned by
//!
//! | property | what it would have caught |
//! |---|---|
//! | every arm is DRIVEN | the third situation was never driven |
//! | silence is DECLARED | see below — two sibling types disagreed about it |
//! | all are DISTINCT | the third arm read as the first arm's wording |
//! | a clause omits its subject | the card was named twice |
//!
//! The last one is the pair R1717 established and did not check: a clause
//! producer's output is prefixed with a subject by its caller, so the clause
//! itself must never name it. That was written in a doc comment and nothing
//! read the doc.
//!
//! ★★★ The second one is the split this file was **corrected into**, by the
//! first two types it was pointed at. A launch verdict's good case says
//! "nothing to fix — launch is open"; a text judgement's good case returns the
//! empty string. Both are defensible and they cannot both be the default, so
//! neither is: an arm that says nothing is named in `silent`, and a name in
//! `silent` whose arm speaks is a **dead declaration** and fails too. That is
//! the same split the analysis-tool screens already draw between the regions
//! that owe a reader a voice and the ones that declare a silence — a census
//! with nothing on one side is satisfiable by putting everything on the other.
//!
//! # What the floor does here, measured rather than read
//!
//! A probe was built against the mature toolkit at 6.11.1 and **run**. Its
//! meta-object enumerates a speaking enum's arms — 4 of 4 — so *counting* the
//! arms is parity. Four things it does not do:
//!
//! * **Nothing reachable from the type enumerates the sentences it can say.**
//!   The meta-object holds methods, properties and enums — four methods on the
//!   probe's own class — and has no notion of a string an object can produce.
//!   Driving each arm by hand is the only way, which is what nobody did.
//! * **The message census that exists is keyed by the SOURCE TEXT.** Two
//!   situations whose wording is identical merge into one catalogue entry, so
//!   the census collapses exactly the defect a census would be built to find.
//! * **It counts sources, not sentences produced.** With no translation loaded
//!   the lookup answers the source text verbatim, so an arm that never runs is
//!   indistinguishable from one that does.
//! * **A refusal carries no sentence at all.** A bounded integer validator
//!   judging `70000` answers a state number; its type has five methods and none
//!   of them returns a reason.
//!
//! Six capabilities here are compile errors there: asking a type for every
//! sentence it can say, asking whether two of its messages read alike, asking a
//! refusal for its reason, asserting a clause omits its subject, asking how
//! many arms have been SAID as opposed to declared, and a census keyed by the
//! situation rather than by the wording.

/// One arm of a speaking type, and what it said when it was driven.
///
/// The arm's NAME travels with the sentence so a failure names the variant a
/// reader would have to go and look at, rather than an index into a list.
pub type Said<'a> = (&'a str, String);

/// Assert `what` says every sentence it can say, and that a reader can tell
/// them apart.
///
/// `arms` is the **type's own** count — `Self::ARMS` where the type derives the
/// variant census, or the number of situations the producer matches on where it
/// is not an enum. Passing `said.len()` makes this a tautology and is the one
/// way to hold it wrong; the doc says so here because nothing can check it.
///
/// `silent` names the arms that say nothing **on purpose**. It is a
/// declaration, so it is checked in both directions: an arm that came back
/// empty must be in it, and a name in it whose arm speaks is a dead
/// declaration.
///
/// # Panics
///
/// When an arm is missing, silent without saying so, declared silent and not,
/// or reads the same as another one.
pub fn assert_speaks(what: &str, arms: usize, said: &[Said<'_>], silent: &[&str]) {
    assert_eq!(
        said.len(),
        arms,
        "{what} has {arms} arm(s) and {} was/were driven — an arm nobody drives \
         is an arm that can say anything, which is how a whole situation came \
         to read as another one's wording",
        said.len()
    );
    for (arm, sentence) in said {
        let quiet = sentence.trim().is_empty();
        assert_eq!(
            quiet,
            silent.contains(arm),
            "{what}::{arm} {} — silence is a decision this type has to declare, \
             because two producers next to each other in this workspace \
             disagreed about whether a good case speaks",
            if quiet {
                "says nothing and is not in the silent list"
            } else {
                "is declared silent and speaks"
            }
        );
    }
    for name in silent {
        assert!(
            said.iter().any(|(arm, _)| arm == name),
            "{what} declares {name} silent and never drove it — a declaration \
             over an arm nobody ran is a claim, not a check"
        );
    }
    let heard: Vec<&Said<'_>> = said.iter().filter(|(_, s)| !s.trim().is_empty()).collect();
    for (i, (arm, sentence)) in heard.iter().enumerate() {
        for (other, other_sentence) in &heard[i + 1..] {
            assert_ne!(
                sentence, other_sentence,
                "{what}::{arm} and {what}::{other} read alike ({sentence:?}) — \
                 two arms a reader cannot tell apart are one arm, and the \
                 second one is then free to be about anything"
            );
        }
    }
}

/// The same, for a producer whose output is a **clause** that a caller prefixes
/// with a subject.
///
/// Every arm must be driven with data naming `subject`, and no clause may
/// contain it: the caller puts it in front, once. Drive with a subject that
/// could not occur by accident — the screen's own card name is ideal, a bare
/// letter is not.
///
/// # Panics
///
/// Everything [`assert_speaks`] panics on, plus a clause that names its own
/// subject.
pub fn assert_speaks_of(
    what: &str,
    subject: &str,
    arms: usize,
    said: &[Said<'_>],
    silent: &[&str],
) {
    assert!(
        !subject.trim().is_empty(),
        "{what} was asked about an empty subject, so the check below cannot \
         fail and is not a check"
    );
    assert_speaks(what, arms, said, silent);
    for (arm, sentence) in said {
        assert!(
            !sentence.contains(subject),
            "{what}::{arm} names its own subject ({subject:?}) in {sentence:?} \
             — the caller puts that in front, so a reader is told twice"
        );
    }
}

/// The census that keeps this gate from shrinking: **every type that can put a
/// sentence in front of a person is driven by something.**
///
/// A gate nobody points at a new type is a gate that gets smaller with the
/// tree, which is how the fourth situation of a launch vocabulary was added
/// with no wording at all. So both sides are read out of a crate's own source —
/// the types that CAN speak, and the names [`assert_speaks`] is actually driven
/// with — and set equality is the assertion. There is no roster to keep in
/// step, because a roster is itself a census a new module can silently miss.
///
/// Point it at a crate's `src/` from that crate's own tests. It reads source
/// text, and it is **loud rather than lenient**: a header shape it does not
/// model panics instead of being dropped, because a census that quietly skips
/// what it cannot parse is worse than none. Three shapes taught it that in the
/// round it was written — a primitive subject (`impl … for f32`), a macro
/// template whose subject is a metavariable, and a call the formatter had
/// wrapped onto its own line.
pub mod census {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Every `.rs` under `src`, minus this gate's own file.
    ///
    /// ★★ The gate is not a subject of itself. Its file holds the signatures —
    /// `assert_speaks(what: &str, …)`, with no name after the paren — and its
    /// own counterfactuals, which drive FABRICATED types on purpose. Counting
    /// those as drives would let the gate satisfy the census, and counting the
    /// fabrications as speakers would report types that are not there. The same
    /// exclusion the reference-name ratchet takes for the file that IS the term
    /// list, and for the same reason.
    fn sources(src: &Path) -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("the source directory is readable") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    out.push((
                        path.to_string_lossy().into_owned(),
                        std::fs::read_to_string(&path).expect("source is UTF-8"),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        walk(src, &mut out);
        out.retain(|(path, _)| !path.ends_with("test_fixtures/speech.rs"));
        out.sort();
        assert!(
            !out.is_empty(),
            "no source under {} — the census would pass on nothing",
            src.display()
        );
        out
    }

    /// The type an `impl` block is for, or `None` for a macro template whose
    /// subject is not a type until it expands.
    ///
    /// ★ The first draft demanded an upper-case initial and a crate answered
    /// `impl Animatable for f32`, so the rule was wrong rather than the tree: a
    /// subject is a **type name**, and a primitive's is lower case. What the
    /// check is for is a header shape the parser did not model.
    fn impl_subject(header: &str) -> Option<String> {
        let body = header
            .trim()
            .strip_prefix("impl")
            .expect("called on an impl line")
            .trim();
        // ★ Depth-counted, not the first `>`. An `impl<E: From<Wrapped>> Ty<E>`
        // closes its INNER bound first, and taking that as the end left the
        // subject starting with `>` — which the census then refused, correctly,
        // as a shape it could not read. It could not: this is the fix.
        let body = if body.starts_with('<') {
            let mut depth = 0usize;
            let close = body
                .char_indices()
                .find_map(|(i, c)| match c {
                    '<' => {
                        depth += 1;
                        None
                    }
                    '>' => {
                        depth -= 1;
                        (depth == 0).then_some(i)
                    }
                    _ => None,
                })
                .expect("an impl generic list closes");
            body[close + 1..].trim()
        } else {
            body
        };
        let subject = body.rsplit(" for ").next().unwrap_or(body).trim();
        let subject = subject.trim_start_matches(['&', '*']).trim();
        if subject.starts_with('$') {
            return None;
        }
        let name = subject
            .split(['<', '{', ' ', ','])
            .next()
            .expect("a subject before the brace")
            .rsplit("::")
            .next()
            .expect("a path ends in a name")
            .trim();
        assert!(
            name.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_'),
            "this census cannot read the impl header {header:?} — teach it \
             rather than letting a speaking type slip past"
        );
        Some(name.to_owned())
    }

    /// Every type under `src` that can put a sentence in front of a person.
    ///
    /// The convention this workspace speaks by: `fn sentence()` / `fn message()`.
    fn speakers(src: &Path) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for (file, text) in sources(src) {
            found.extend(speakers_in(&file, &text));
        }
        found
    }

    /// The same, over ONE source text.
    ///
    /// ★★★★★ R1718.1 — split out so the parser's own arms can be driven. The
    /// counterfactuals for this census PASSED at first: breaking its undriven-
    /// speaker report, its stray-drive report and its refusal of an unreadable
    /// header changed nothing, because at HEAD every speaker is driven and
    /// every header readable, so the failing branches were never taken. That is
    /// this round's own thesis pointed at itself — a check whose failure path
    /// nobody drives is a check that can say anything — and the repair is the
    /// one the round is about: feed it the failing input and read what it says.
    pub(super) fn speakers_in(file: &str, text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        {
            // `None` = not inside any impl; `Some(None)` = a macro template.
            let mut current: Option<Option<String>> = None;
            for line in text.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
                    current = Some(impl_subject(trimmed));
                }
                if trimmed.starts_with("pub fn sentence(&self")
                    || trimmed.starts_with("pub fn message(&self")
                    || trimmed.starts_with("fn sentence(&self")
                    || trimmed.starts_with("fn message(&self")
                {
                    match current.clone() {
                        Some(Some(owner)) => {
                            found.insert(owner);
                        }
                        Some(None) => panic!(
                            "{file}: a speaking method inside a MACRO template, \
                             whose subject is not a type until it expands — this \
                             census cannot attribute it, and a speaker it cannot \
                             attribute is a speaker nothing drives: {trimmed}"
                        ),
                        None => {
                            panic!("{file}: a speaking method outside any impl block: {trimmed}")
                        }
                    }
                }
            }
        }
        found
    }

    /// Every name the gate is driven with, read out of the same source.
    ///
    /// The first argument of [`super::assert_speaks`] is the name a failure
    /// prints, and it is what ties a drive to a type. A qualifier —
    /// `"Unavailable (with detail)"` — counts for the type before the
    /// parenthesis, because two situations of one type are still that type
    /// being driven.
    ///
    /// ★ The name is the first STRING argument, wherever the formatter put it.
    /// The first draft demanded it be adjacent to the paren, `cargo fmt`
    /// wrapped one call onto its own line, and the census reported that drive
    /// as absent — a parser that reads a LAYOUT rather than a value is a gate
    /// that answers to the formatter.
    fn driven(src: &Path) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for (file, text) in sources(src) {
            found.extend(driven_in(&file, &text));
        }
        found
    }

    /// The same, over ONE source text — see [`speakers_in`] for why the split.
    pub(super) fn driven_in(file: &str, text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        {
            for call in ["assert_speaks(", "assert_speaks_of("] {
                let mut at = 0;
                while let Some(i) = text[at..].find(call) {
                    let after = at + i + call.len();
                    let rest = &text[after..];
                    let open = rest
                        .find('"')
                        .unwrap_or_else(|| panic!("{file}: a speech drive with no name: {call}"));
                    assert!(
                        !rest[..open].contains(';'),
                        "{file}: a speech drive whose first argument is not a \
                         literal name — a failure could not then say which type \
                         it was about"
                    );
                    let start = after + open + 1;
                    let end = start + text[start..].find('"').expect("the name closes");
                    let name = text[start..end].split(" (").next().unwrap_or("").trim();
                    assert!(
                        !name.is_empty(),
                        "{file}: a speech drive was given an empty name, so \
                         nothing ties it to a type"
                    );
                    found.insert(name.to_owned());
                    at = end;
                }
            }
        }
        found
    }

    /// Assert every speaking type a crate declares is driven, and every drive
    /// names a type that can speak.
    ///
    /// Point it at a crate's **root** — the directory holding `Cargo.toml`.
    /// Speakers are read from `src/`, and drives from `src/` *and* `tests/`,
    /// because a drive lives legitimately in either: a framework type is driven
    /// beside its definition and a surface type is driven from an integration
    /// test. Reading only one of them would report a driven type as silent,
    /// which is a census that fails for the wrong reason.
    ///
    /// `least` is how many speakers the caller expects at minimum — the guard
    /// that keeps a passing census from being a census of nothing when a scan
    /// silently reads the wrong directory.
    ///
    /// # Panics
    ///
    /// When a speaker is undriven, a drive names a non-speaker, or the scan
    /// found less than `least`.
    pub fn assert_every_speaker_is_driven(crate_dir: impl AsRef<Path>, least: usize) {
        let root = PathBuf::from(crate_dir.as_ref());
        let src = root.join("src");
        let speakers = speakers(&src);
        let mut driven = driven(&src);
        let tests = root.join("tests");
        if tests.is_dir() {
            driven.extend(self::driven(&tests));
        }

        assert!(
            speakers.len() >= least,
            "expected at least {least} speaking type(s) under {}, found {speakers:?} \
             — a census that reads too little passes on nothing",
            src.display()
        );
        judge(&speakers, &driven);
    }

    /// The two set comparisons, apart from the walking, so that **their own
    /// failure paths can be driven** — see [`speakers_in`].
    pub(super) fn judge(speakers: &BTreeSet<String>, driven: &BTreeSet<String>) {
        let silent: Vec<&String> = speakers.difference(driven).collect();
        assert!(
            silent.is_empty(),
            "these type(s) can put a sentence in front of a person and nothing \
             drives what they SAY: {silent:?}\n\
             Add a `speech::assert_speaks` drive naming each one. An arm nobody \
             drives is an arm that can say anything — which is how a whole \
             situation came to read as another one's wording for a round."
        );

        let stray: Vec<&String> = driven.difference(speakers).collect();
        assert!(
            stray.is_empty(),
            "these drive(s) name something with no speaking method here: \
             {stray:?} — a drive over a type that cannot speak is a check that \
             cannot fail"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{assert_speaks, assert_speaks_of};

    fn caught(run: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = std::panic::catch_unwind(run);
        std::panic::set_hook(hushed);
        let err = out.expect_err("the gate was supposed to refuse this");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
    }

    /// ★★★★★ R1718 — the gate's own counterfactuals. A gate nothing tests is
    /// a claim, and this one exists because a claim in a doc comment is what
    /// let the defect through.
    #[test]
    fn r1718_the_gate_refuses_each_of_the_four_ways_a_type_can_fail_a_reader() {
        let good = [
            ("Blocking", "3 error(s) block launch".to_owned()),
            ("Warning", "5 warning(s) stand".to_owned()),
        ];
        assert_speaks("Verdict", 2, &good, &[]);

        let missing = caught(|| assert_speaks("Verdict", 3, &good, &[]));
        assert!(
            missing.contains("3 arm(s) and 2"),
            "★ an undriven arm is named: {missing}"
        );

        let quiet = [("Blocking", "  ".to_owned()), ("Warning", "w".to_owned())];
        let undeclared = caught(|| assert_speaks("Verdict", 2, &quiet, &[]));
        assert!(
            undeclared.contains("says nothing and is not in the silent list"),
            "★ an undeclared silence is named: {undeclared}"
        );
        assert_speaks("Verdict", 2, &quiet, &["Blocking"]);

        // ★★ And the other direction: a declaration over an arm that speaks.
        let dead = caught(|| assert_speaks("Verdict", 2, &good, &["Blocking"]));
        assert!(
            dead.contains("is declared silent and speaks"),
            "★★ a dead silence declaration is named: {dead}"
        );
        let absent = caught(|| assert_speaks("Verdict", 2, &good, &["Nowhere"]));
        assert!(
            absent.contains("never drove it"),
            "★★ and one over an arm nobody drove: {absent}"
        );

        let alike = [
            ("Blocking", "it is wrong".to_owned()),
            ("Warning", "it is wrong".to_owned()),
        ];
        let twice = caught(|| assert_speaks("Verdict", 2, &alike, &[]));
        assert!(
            twice.contains("read alike"),
            "★★ two arms a reader cannot tell apart: {twice}"
        );

        // ★★★★★ And the one the round is named for: a clause that names the
        // subject its caller prefixes.
        let doubled = [
            ("Value", "R-01 is outside this graph".to_owned()),
            ("Graph", "nothing is listening".to_owned()),
        ];
        let subject = caught(|| assert_speaks_of("Finding", "R-01", 2, &doubled, &[]));
        assert!(
            subject.contains("names its own subject"),
            "★★★★★ the R1716 shape: {subject}"
        );
    }

    /// ★★★ R1718 — and the subject check cannot pass vacuously.
    #[test]
    fn r1718_a_subject_that_could_not_occur_is_refused() {
        let any = [("One", "a sentence".to_owned())];
        let empty = caught(|| assert_speaks_of("Thing", "  ", 1, &any, &[]));
        assert!(
            empty.contains("cannot fail"),
            "★ an empty subject would make the clause check a no-op: {empty}"
        );
    }

    /// ★★★★★ R1718.1 — **the CENSUS's own failure paths, driven.**
    ///
    /// Its counterfactuals passed the first time they were run: breaking the
    /// undriven-speaker report, the stray-drive report and the refusal of an
    /// unreadable header all changed nothing, because at HEAD every speaker is
    /// driven and every header is readable, so those branches were never taken.
    /// That is exactly the defect this whole round is about, pointed at the
    /// round's own work — a check whose failing branch nobody drives can say
    /// anything — and it is why the parser was split into functions that take a
    /// source TEXT.
    #[test]
    fn r1718_the_census_refuses_a_speaker_nobody_drives_and_a_drive_with_no_speaker() {
        use super::census::{driven_in, judge, speakers_in};
        use std::collections::BTreeSet;

        let speaking =
            "impl Verdict {\n    pub fn sentence(&self) -> String { String::new() }\n}\n";
        let speakers = speakers_in("fixture.rs", speaking);
        assert_eq!(
            speakers.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Verdict"],
            "★ the parser finds the speaker at all"
        );

        let undriven = caught(move || judge(&speakers_in("f.rs", speaking), &BTreeSet::default()));
        assert!(
            undriven.contains("nothing \n         drives what they SAY")
                || undriven.contains("drives what they SAY"),
            "★★★★★ a speaker nobody drives is named: {undriven}"
        );

        let drives = driven_in("t.rs", "assert_speaks(\"Nowhere\", 1, &said, &[]);\n");
        assert_eq!(
            drives.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Nowhere"],
            "★ and the drive parser finds a name"
        );
        let stray = caught(move || {
            judge(
                &BTreeSet::default(),
                &driven_in("t.rs", "assert_speaks(\"Nowhere\", 1, &said, &[]);\n"),
            );
        });
        assert!(
            stray.contains("no speaking method here"),
            "★★ and a drive over a type that cannot speak is named: {stray}"
        );
    }

    /// ★★★★ R1718.1 — and the parser refuses what it cannot read, which is the
    /// whole trade. Each input below taught it something in the round it was
    /// written.
    #[test]
    fn r1718_the_census_parser_refuses_what_it_cannot_attribute() {
        use super::census::{driven_in, speakers_in};

        // A macro template: the subject is not a type until it expands, so a
        // speaker inside one is one this census cannot attribute at all.
        let macro_body = "impl $crate::external::External for $t {\n    \
                          fn message(&self) -> &'static str { \"x\" }\n}\n";
        let refused = caught(move || {
            speakers_in("m.rs", macro_body);
        });
        assert!(
            refused.contains("MACRO template"),
            "★★★ a speaker inside a macro template is refused by name: {refused}"
        );

        // A primitive subject is a TYPE NAME, not an unreadable header — the
        // first draft demanded an upper-case initial and this crate answered
        // `impl Animatable for f32`.
        let primitive = "impl Animatable for f32 {\n    \
                         pub fn sentence(&self) -> String { String::new() }\n}\n";
        assert_eq!(
            speakers_in("p.rs", primitive)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["f32"],
            "★★ a primitive subject reads"
        );

        // A nested generic bound closes before the impl's own list does.
        let nested = "impl<E: From<Wrapped>> Refusal<E> {\n    \
                      pub fn sentence(&self) -> String { String::new() }\n}\n";
        assert_eq!(
            speakers_in("n.rs", nested)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Refusal"],
            "★★ and the generic stripper counts depth rather than taking the first `>`"
        );

        // ★★★★ And a header it genuinely cannot read is REFUSED rather than
        // guessed at. A tuple subject has no name to attribute a speaker to,
        // and the whole trade this parser makes is loud-over-lenient: a census
        // that quietly skips what it cannot parse is worse than none. Driven
        // here because the counterfactual for this refusal PASSED without it —
        // every other input in this test is one the parser now reads.
        let tuple = "impl Speaks for (A, B) {\n    \
                     pub fn sentence(&self) -> String { String::new() }\n}\n";
        let unreadable = caught(move || {
            speakers_in("t.rs", tuple);
        });
        assert!(
            unreadable.contains("cannot read the impl header"),
            "★★★★ a header with no name to attribute is refused: {unreadable}"
        );

        // A call the formatter wrapped is still a drive. This is the one that
        // made the census report a real drive as absent.
        let wrapped = concat!(
            "        assert_speaks(\n",
            "            \"Unavailable (no detail)\",\n",
            "            7,\n",
            "        );\n"
        );
        assert_eq!(
            driven_in("w.rs", wrapped)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Unavailable"],
            "★★★★ a wrapped call is found, and its qualifier folds to the type"
        );
    }
}
