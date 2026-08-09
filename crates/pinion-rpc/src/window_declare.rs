//! `scene/window_declare` RPC method — the general WRITE peer of the
//! `scene/windows` read (§5.16 §5.41 §2 #7).
//!
//! # Why this is one method and not five
//!
//! `scene/windows` reads a window's whole declaration in one message: title,
//! position, declared size, display, decorations, and (R1610) level. Before
//! this method exactly **one** of those axes could be written back —
//! `scene/window_move`, for position. Title, decorations and display were
//! readable and not writable, and R1610's level would have been the fifth axis
//! and the second method, which is the shape that ends in one method per field.
//!
//! [[wire-form-read-write-symmetry]] is the standing rule: every readable state
//! of an axis must be writable when the write wire claims the axis. The read is
//! one method over the whole declaration, so the write is too.
//!
//! # Absent, null, and a value are three different things
//!
//! A patch has to distinguish "leave this alone" from "clear this", and for
//! `position` / `display` — the two nullable axes — a bare `Option` cannot.
//! Those two carry a [`Patch`]: the key **absent** leaves the axis untouched,
//! an explicit **`null`** clears it, and a value sets it. Three named arms
//! rather than two stacked `Option`s, for the reason that type documents.
//!
//! Clearing is not a rounding error in this design, it is the state every
//! window boots in: a `null` position hands the window back to the window
//! manager, and R1576 made the same point on the binding side
//! (`WindowSpec::with_placement(None)` exists precisely because the builders
//! could only ever add).
//!
//! # What is deliberately NOT here
//!
//! `strategy` (and therefore `declared_size`) is create-time intent, read once
//! when the window is made. It is absent from this patch because writing it
//! would be a lie — the shell has no pass that re-applies it — and a write wire
//! that silently does nothing is the failure mode this whole axis exists to
//! remove. The patch's field set IS the set of live, reconcilable axes.
//!
//! # Relationship to `scene/window_move`
//!
//! One write path, not two. `scene/window_move` predates this and keeps its
//! wire shape (`{window_id, x, y}`), but is now expressed as a position-only
//! patch through the same closure, so the pinning semantics R1088 gave it and
//! the semantics of a `position` patch cannot drift apart.
//!
//! Asynchrony mirrors `scene/window_move` and `scene/resize`: the signal write
//! fires the reconcile effect and the OS calls land on the next event-loop
//! iteration. Clients pair the write with `scene/windows` to confirm what took.

use pinion_core::window_level::WindowLevel;
use serde::{Deserialize, Deserializer, Serialize};

/// R1610 — what a patch says about ONE nullable axis: nothing, clear it, or
/// set it.
///
/// Three named arms rather than `Option<Option<T>>`. The draft was the nested
/// pair and `clippy::option_option` refused it, pointing at "a custom enum if
/// you need to distinguish all 3 cases" — which is the same argument this
/// round makes for [`WindowLevel`] against a two-flag encoding, so writing the
/// level as one value with three arms and the patch as two stacked `Option`s
/// would have been inconsistent within a single round. A reader of
/// `Some(None)` has to know which layer means what; a reader of
/// [`Patch::Clear`] does not.
///
/// Only the two NULLABLE axes use it. `title`, `decorations` and `level` stay
/// plain `Option`s, because those axes have no cleared state — that is a real
/// distinction between the axes and the types now carry it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Patch<T> {
    /// The key was absent: leave the axis exactly as it is.
    #[default]
    Untouched,
    /// The key was an explicit `null`: clear the axis. For `position` this
    /// hands the window back to the window manager, which is the state every
    /// window boots in and the one a builder can never reach.
    Clear,
    /// The key carried a value: set the axis to it.
    Set(T),
}

impl<T> Patch<T> {
    /// Does this patch say nothing about its axis?
    ///
    /// The `skip_serializing_if` predicate, and load-bearing: [`Self::Clear`]
    /// and [`Self::Untouched`] both serialize as an absent-or-null shape, so
    /// without the skip a re-sent patch would CLEAR every axis it never
    /// mentioned.
    #[must_use]
    pub const fn is_untouched(&self) -> bool {
        matches!(self, Self::Untouched)
    }

    /// The value this patch sets, if it sets one.
    #[must_use]
    pub const fn set(&self) -> Option<&T> {
        match self {
            Self::Set(v) => Some(v),
            Self::Untouched | Self::Clear => None,
        }
    }

    /// Apply this patch to a slot, leaving it alone when untouched.
    ///
    /// The whole reason the type exists, in one place: every consumer that
    /// hand-matched the three arms would be a chance to treat `Untouched` as
    /// `Clear`, which is the defect the nested `Option` invited.
    pub fn apply_to(self, slot: &mut Option<T>) {
        match self {
            Self::Untouched => {}
            Self::Clear => *slot = None,
            Self::Set(v) => *slot = Some(v),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    /// A present key — INCLUDING `null` — reaches here; an absent one is
    /// filled in by `#[serde(default)]` and never does. That is the whole
    /// mechanism by which "do not touch the position" and "hand the window
    /// back to the window manager" stay different messages.
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(de)? {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        })
    }
}

impl<T: Serialize> Serialize for Patch<T> {
    /// [`Self::Untouched`] is never reached in practice — the field's
    /// `skip_serializing_if` drops it before serde sees it — and serializing
    /// it as `null` here is the conservative answer for a caller who
    /// serializes a bare `Patch`, since `null` is at worst a redundant clear
    /// where a value would be a fabricated one.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Set(value) => serializer.serialize_some(value),
            Self::Untouched | Self::Clear => serializer.serialize_none(),
        }
    }
}

/// Request params for `scene/window_declare`: a patch over one declared
/// window's LIVE axes.
///
/// Every axis is optional and an omitted axis is untouched, so a client that
/// wants to pin a panel on top sends `{"window_id": "panel", "level":
/// "always_on_top"}` and says nothing about the other four.
///
/// The target is named `window_id` for the reason
/// [`crate::WindowMoveParams`] documents: `window` is the reserved
/// per-dispatch SCOPE key, and naming the target `window` would route the
/// request through the unknown-window scope gate before this handler could
/// produce its own precise miss.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowDeclareParams {
    /// The declared window id (the `scene/windows` `id` field).
    pub window_id: String,
    /// New OS window title. Absent leaves the title alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New declared outer position in logical pixels, `null` to hand the
    /// window back to the window manager, absent to leave it alone.
    #[serde(default, skip_serializing_if = "Patch::is_untouched")]
    pub position: Patch<(i32, i32)>,
    /// New display id that `position` is measured from, `null` to go back to
    /// an absolute virtual-desktop coordinate, absent to leave it alone.
    #[serde(default, skip_serializing_if = "Patch::is_untouched")]
    pub display: Patch<String>,
    /// Whether the OS draws this window's chrome. Absent leaves it alone.
    /// Not nullable — a window's chrome state is always known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorations: Option<bool>,
    /// R1610 — where the window sits in the window manager's front-to-back
    /// order. Absent leaves it alone; not nullable, because
    /// [`WindowLevel::Normal`] is the "no stacking request" value rather than
    /// an absent one.
    ///
    /// The wire SPELLING rather than the domain enum, for the reason
    /// [`crate::DeclaredWindow::display`] is a `String`: this crate owns the
    /// wire shape, and the census that keeps `rpc/schema` honest reads only
    /// this crate, so a field typed with another crate's vocabulary publishes
    /// as `any`. [`WindowLevel::from_wire`] parses it and an unrecognised
    /// spelling is [`WindowDeclareError::UnknownLevel`] — never a silent
    /// fallback to `normal`, which would make a typo indistinguishable from
    /// asking for ordinary stacking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// R1616 §5.12 §2 #7 — every wire spelling `WindowDeclareParams::level`
/// accepts, DERIVED from the domain enum's own census.
///
/// The valid set had no home on the framework wire. R1610.1 projected the
/// level onto a `String` for good reasons — this crate owns the wire shape,
/// and the axis must be parsed before any other axis in the same message is
/// applied, which a deserialization-time enum cannot do without aborting the
/// whole frame — and the cost it named in its own carry was exactly this: the
/// earlier `Option<WindowLevel>` draft got serde's unknown-variant error,
/// which lists the valid values, and the projection threw that away. An agent
/// was left with `UnknownLevel` and no way to ask what a right spelling looks
/// like short of reading pinion's source.
///
/// Computed rather than retyped. A hand list here would be a second copy of a
/// closed set, and a second copy of a closed set goes stale in silence — the
/// failure this whole census exists to make impossible.
pub const LEVEL_WIRE_NAMES: [&str; WindowLevel::ALL.len()] = {
    let mut names = [""; WindowLevel::ALL.len()];
    let mut i = 0;
    while i < WindowLevel::ALL.len() {
        names[i] = WindowLevel::ALL[i].as_str();
        i += 1;
    }
    names
};

impl WindowDeclareParams {
    /// The axes this patch actually names, in wire spelling.
    ///
    /// Used to refuse an empty patch by name and echoed in the outcome so a
    /// caller sees what the request was understood to touch — a typo in an
    /// axis key otherwise arrives as a cheerful success that changed nothing.
    #[must_use]
    pub fn declared_axes(&self) -> Vec<&'static str> {
        let mut axes = Vec::new();
        if self.title.is_some() {
            axes.push("title");
        }
        if !self.position.is_untouched() {
            axes.push("position");
        }
        if !self.display.is_untouched() {
            axes.push("display");
        }
        if self.decorations.is_some() {
            axes.push("decorations");
        }
        if self.level.is_some() {
            axes.push("level");
        }
        axes
    }

    /// R1610 — the position-only patch `scene/window_move` is.
    ///
    /// The two methods share one closure so a move and a `position` patch
    /// cannot come to mean different things; this is where that sharing is
    /// stated.
    #[must_use]
    pub fn moving(window_id: String, x: i32, y: i32) -> Self {
        Self {
            window_id,
            position: Patch::Set((x, y)),
            ..Self::default()
        }
    }
}

/// Response payload for `scene/window_declare`.
///
/// Echoes the window and the axes the patch was understood to name — the
/// confirmation anchor `WindowMoveOutcome` establishes, made informative:
/// `applied` is what the request MEANT, so a client that misspelled an axis
/// key sees it missing here instead of reading a success and wondering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowDeclareOutcome {
    /// Echoes the window id the patch addressed.
    pub window_id: String,
    /// The axes the patch named, in wire spelling.
    pub applied: Vec<String>,
}

/// Reasons `scene/window_declare` can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowDeclareError {
    /// The dispatcher was invoked without a `declare_request` closure — a
    /// single-window or TUI binding that declares no `windows_signal`.
    ClosureUnavailable,
    /// No declared window carries the requested id.
    UnknownWindow,
    /// `level` carried a string that is not one of the window levels. Refused
    /// by name rather than defaulted, because "the level you asked for is not
    /// one" and "you asked for normal" are different facts. The valid
    /// spellings are [`WindowLevel::ALL`].
    UnknownLevel,
    /// The patch named no axis at all. Refused rather than answered with a
    /// successful no-op, because the only ways to send one are a client bug
    /// (every axis key misspelled) and a client asking for nothing — and a
    /// silent success teaches the first that it worked.
    NoAxisDeclared,
}

/// Write a declared window's live axes through the registered closure.
///
/// The closure returns `true` when it found a declared window with the
/// requested id and wrote it; `false` becomes
/// [`WindowDeclareError::UnknownWindow`] so the wire surfaces a precise miss
/// rather than a silent no-op.
///
/// # Errors
///
/// See [`WindowDeclareError`].
pub fn window_declare<F>(
    params: WindowDeclareParams,
    declare_request: Option<&mut F>,
) -> Result<WindowDeclareOutcome, WindowDeclareError>
where
    F: FnMut(&WindowDeclareParams) -> bool + ?Sized,
{
    let axes = params.declared_axes();
    if axes.is_empty() {
        return Err(WindowDeclareError::NoAxisDeclared);
    }
    // Parsed BEFORE the closure runs, so a bad spelling cannot half-apply a
    // patch: every other axis in the same message would already be written.
    if let Some(level) = params.level.as_deref()
        && WindowLevel::from_wire(level).is_none()
    {
        return Err(WindowDeclareError::UnknownLevel);
    }
    let closure = declare_request.ok_or(WindowDeclareError::ClosureUnavailable)?;
    if closure(&params) {
        Ok(WindowDeclareOutcome {
            window_id: params.window_id,
            applied: axes.into_iter().map(str::to_owned).collect(),
        })
    } else {
        Err(WindowDeclareError::UnknownWindow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn parse(json: &str) -> WindowDeclareParams {
        serde_json::from_str(json).expect("params must parse")
    }

    #[test]
    fn r1610_an_absent_axis_is_not_a_cleared_one() {
        // The distinction the whole patch shape exists for.
        let untouched = parse(r#"{"window_id":"panel"}"#);
        assert_eq!(
            untouched.position,
            Patch::Untouched,
            "absent leaves it alone"
        );
        assert_eq!(
            untouched.display,
            Patch::Untouched,
            "absent leaves it alone"
        );

        let cleared = parse(r#"{"window_id":"panel","position":null,"display":null}"#);
        assert_eq!(
            cleared.position,
            Patch::Clear,
            "explicit null hands the window back to the WM",
        );
        assert_eq!(
            cleared.display,
            Patch::Clear,
            "explicit null clears display"
        );

        let set = parse(r#"{"window_id":"panel","position":[40,80],"display":"HDMI-1"}"#);
        assert_eq!(set.position, Patch::Set((40, 80)));
        assert_eq!(set.display, Patch::Set("HDMI-1".to_owned()));

        // All three are distinct — a bare Option would collapse two of them.
        assert_ne!(untouched.position, cleared.position);
        assert_ne!(cleared.position, set.position);

        // And apply_to is where that distinction has to survive contact with
        // a slot: the untouched arm must leave a declared value in place.
        let mut slot = Some((1, 2));
        untouched.position.apply_to(&mut slot);
        assert_eq!(slot, Some((1, 2)), "untouched leaves the slot alone");
        cleared.position.apply_to(&mut slot);
        assert_eq!(slot, None, "clear empties it");
        set.position.apply_to(&mut slot);
        assert_eq!(slot, Some((40, 80)), "set writes it");
    }

    #[test]
    fn r1610_declared_axes_names_only_what_the_patch_carries() {
        assert_eq!(
            parse(r#"{"window_id":"p"}"#).declared_axes(),
            Vec::<&str>::new()
        );
        assert_eq!(
            parse(r#"{"window_id":"p","level":"always_on_top"}"#).declared_axes(),
            vec!["level"],
        );
        // A CLEAR is an axis the patch names, not an absence.
        assert_eq!(
            parse(r#"{"window_id":"p","position":null}"#).declared_axes(),
            vec!["position"],
        );
        assert_eq!(
            parse(
                r#"{"window_id":"p","title":"T","position":[1,2],"display":null,
                    "decorations":false,"level":"always_on_bottom"}"#
            )
            .declared_axes(),
            vec!["title", "position", "display", "decorations", "level"],
        );
    }

    #[test]
    fn r1610_an_empty_patch_is_refused_before_the_closure_runs() {
        // A misspelled axis key arrives as an empty patch. Answering it with a
        // success would teach the client that the typo worked.
        let calls = RefCell::new(0_usize);
        let mut closure = |_p: &WindowDeclareParams| {
            *calls.borrow_mut() += 1;
            true
        };
        let err = window_declare(
            parse(r#"{"window_id":"panel","levl":"always_on_top"}"#),
            Some(&mut closure),
        )
        .unwrap_err();
        assert_eq!(err, WindowDeclareError::NoAxisDeclared);
        assert_eq!(*calls.borrow(), 0, "the closure must not have been asked");
    }

    #[test]
    fn r1610_declare_requires_a_closure() {
        let err = window_declare::<dyn FnMut(&WindowDeclareParams) -> bool>(
            parse(r#"{"window_id":"panel","level":"always_on_top"}"#),
            None,
        )
        .unwrap_err();
        assert_eq!(err, WindowDeclareError::ClosureUnavailable);
    }

    #[test]
    fn r1610_a_closure_miss_is_an_unknown_window() {
        let mut closure = |_p: &WindowDeclareParams| false;
        let err = window_declare(
            parse(r#"{"window_id":"ghost","level":"always_on_top"}"#),
            Some(&mut closure),
        )
        .unwrap_err();
        assert_eq!(err, WindowDeclareError::UnknownWindow);
    }

    #[test]
    fn r1610_the_outcome_echoes_the_axes_the_patch_named() {
        let seen: RefCell<Option<WindowDeclareParams>> = RefCell::new(None);
        let mut closure = |p: &WindowDeclareParams| {
            *seen.borrow_mut() = Some(p.clone());
            true
        };
        let outcome = window_declare(
            parse(r#"{"window_id":"panel","level":"always_on_top","decorations":false}"#),
            Some(&mut closure),
        )
        .unwrap();
        assert_eq!(outcome.window_id, "panel");
        assert_eq!(outcome.applied, vec!["decorations", "level"]);
        let got = seen.into_inner().expect("closure ran");
        assert_eq!(got.level.as_deref(), Some("always_on_top"));
        assert_eq!(got.decorations, Some(false));
        assert_eq!(got.title, None, "an axis the patch did not name stays None");
        assert!(
            got.position.is_untouched(),
            "and a nullable one stays untouched"
        );
    }

    #[test]
    fn r1610_a_move_is_a_position_only_patch() {
        // The two methods share ONE write path, so the pinning semantics
        // R1088 gave a move and the semantics of a position patch cannot
        // drift apart. This is that claim as a test.
        let moving = WindowDeclareParams::moving("panel".to_owned(), 240, 160);
        assert_eq!(moving.declared_axes(), vec!["position"]);
        assert_eq!(moving.position, Patch::Set((240, 160)));
        assert_eq!(moving.title, None);
        assert_eq!(moving.display, Patch::Untouched);
        assert_eq!(moving.decorations, None);
        assert_eq!(moving.level, None);
    }

    #[test]
    fn r1610_an_unknown_level_is_refused_before_anything_is_written() {
        // The parse happens ahead of the closure, so a patch carrying a bad
        // level AND a good title cannot half-apply — every other axis in the
        // same message would already be in the signal.
        let calls = RefCell::new(0_usize);
        let mut closure = |_p: &WindowDeclareParams| {
            *calls.borrow_mut() += 1;
            true
        };
        for bad in ["always_on_topp", "top", "AlwaysOnTop", ""] {
            let err = window_declare(
                parse(&format!(
                    r#"{{"window_id":"p","title":"T","level":"{bad}"}}"#
                )),
                Some(&mut closure),
            )
            .unwrap_err();
            assert_eq!(err, WindowDeclareError::UnknownLevel, "{bad:?}");
        }
        assert_eq!(*calls.borrow(), 0, "nothing was written");
        // And every valid spelling passes the same gate.
        for valid in WindowLevel::ALL {
            let outcome = window_declare(
                parse(&format!(
                    r#"{{"window_id":"p","level":"{}"}}"#,
                    valid.as_str()
                )),
                Some(&mut closure),
            )
            .expect("a valid level");
            assert_eq!(outcome.applied, vec!["level"]);
        }
        assert_eq!(*calls.borrow(), WindowLevel::ALL.len());
    }

    #[test]
    fn r1610_params_round_trip_the_wire_without_growing_absent_keys() {
        // Serializing a sparse patch must not turn absent axes into nulls —
        // that would make a re-sent patch CLEAR what it never mentioned.
        let sparse = parse(r#"{"window_id":"panel","level":"always_on_top"}"#);
        let json = serde_json::to_string(&sparse).unwrap();
        assert_eq!(json, r#"{"window_id":"panel","level":"always_on_top"}"#);
        assert_eq!(parse(&json), sparse);

        let clearing = parse(r#"{"window_id":"panel","position":null}"#);
        let json = serde_json::to_string(&clearing).unwrap();
        assert_eq!(json, r#"{"window_id":"panel","position":null}"#);
        assert_eq!(parse(&json), clearing);
    }

    #[test]
    fn r1616_the_published_level_vocabulary_is_exactly_what_the_axis_accepts() {
        // ★ The point of publishing a value set is that a client can act on
        // it without guessing. That only holds if the published set and the
        // accepted set are one set, so every published spelling is driven
        // through the real parse path and a non-member is refused by it.
        assert_eq!(
            LEVEL_WIRE_NAMES.len(),
            WindowLevel::ALL.len(),
            "derived from the domain census, so it cannot go short",
        );
        for (name, level) in LEVEL_WIRE_NAMES.iter().zip(WindowLevel::ALL) {
            assert_eq!(*name, level.as_str());
        }

        for name in LEVEL_WIRE_NAMES {
            let params = parse(&format!(r#"{{"window_id":"main","level":"{name}"}}"#));
            let mut seen = false;
            let mut closure = |_: &WindowDeclareParams| {
                seen = true;
                true
            };
            let outcome = window_declare(params, Some(&mut closure))
                .unwrap_or_else(|e| panic!("published level {name:?} refused: {e:?}"));
            assert_eq!(outcome.applied, vec!["level".to_owned()]);
            assert!(seen, "{name:?} reached the closure");
        }

        // ...and a spelling outside the published set is refused by NAME, so
        // a client that matched the word learns the axis and reads the set
        // from `rpc/schema` rather than guessing again.
        let bad = parse(r#"{"window_id":"main","level":"floating"}"#);
        let mut closure = |_: &WindowDeclareParams| panic!("must not reach the closure");
        assert_eq!(
            window_declare(bad, Some(&mut closure)),
            Err(WindowDeclareError::UnknownLevel),
        );
        assert!(
            !LEVEL_WIRE_NAMES.contains(&"floating"),
            "the negative control is genuinely outside the set",
        );
    }
}
