//! `scene/accelerators` RPC method dispatch — R1569 §5.39 §5.20 + §5.7 + §5.12.
//!
//! The **resolution** peer of `scene/mnemonics`. That method answers what a
//! window *declares* — a pure function of the painted scene, which is what lets
//! R1543 promise the accelerator and the underline can never disagree. This one
//! answers what a chord would *do*, which is a different question with three
//! inputs: the declarations, who has focus, and whether that focused widget
//! claims the chord for itself (R1569's
//! [`External::shadows_accelerator`](pinion_core::external::External::shadows_accelerator)).
//!
//! Keeping them apart is the design, not an accident of layering: folding the
//! shadow into `scene/mnemonics` would make that method stop being a function
//! of the scene.
//!
//! ## Where this is more than Qt 6.11
//!
//! 1. **It exists.** Qt's shortcut state lives in `QShortcutMap`, a private
//!    header (`qshortcutmap_p.h`), and `QEvent::ShortcutOverride` is a
//!    transient event delivered per press. So no Qt application — let alone an
//!    external driver — can ask what <kbd>Alt</kbd>+<kbd>F</kbd> does right
//!    now, and there is no moment at which the override is a readable fact.
//! 2. **A shadow is attributed.** The row names the widget taking the chord,
//!    where Qt's override is anonymous: `accept()` leaves no record of who
//!    accepted.
//! 3. **A chord can be asked about before it is pressed.** `params.chord`
//!    answers "would `Ctrl+S` collide" — the question a keymap editor must ask
//!    to be usable, and the one `QKeySequenceEdit` cannot: it records the chord
//!    and the collision surfaces later, at dispatch, as
//!    `QShortcutEvent::isAmbiguous()`.
//! 4. **The probe's domain is published.** The `keybinding` layer is a
//!    *function*, so its claims are discovered by calling it; `probed` states
//!    the character range that was called rather than letting a list read as
//!    complete when it is bounded.
//!
//! A binding that has not painted yet answers with an empty `accelerators`
//! list: "nothing is bound" is the true answer both for a window with no
//! labels and for one before its first frame.

use pinion_core::accelerator::{Chord, ChordParseError};
use pinion_runtime::{ASCII_PROBE_RANGE, AcceleratorRow};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One live accelerator.
#[derive(Debug, Clone, Serialize)]
pub struct AcceleratorEntry {
    /// Portable chord spelling — `"Alt+f"`, `"d"`.
    pub accel: String,
    /// `"mnemonic"` or `"keybinding"`.
    pub layer: &'static str,
    /// The paint tag a mnemonic activates; empty for a `keybinding`, which
    /// maps to a typed event rather than to a node.
    pub target: String,
    /// The displayed label a mnemonic was marked in; empty for a `keybinding`.
    pub label: String,
    /// Whether the focused widget is currently taking this chord.
    pub shadowed: bool,
    /// Which widget is taking it. `null` when nothing is.
    pub shadowed_by: Option<String>,
}

/// What an asked-about chord would do right now.
#[derive(Debug, Clone, Serialize)]
pub struct ChordVerdict {
    /// The chord as asked, re-spelled canonically — so a caller can see the
    /// spelling its request was understood as.
    pub accel: String,
    /// The layer that claims it, or `null` when the chord is free.
    pub claimed_by: Option<&'static str>,
    /// Whether the focused widget currently takes it ahead of that layer.
    pub shadowed: bool,
    /// Which widget takes it. `null` when nothing does.
    pub shadowed_by: Option<String>,
}

impl ChordVerdict {
    /// Build the verdict from what `CoreShell::accelerator_for_chord` answered.
    ///
    /// The construction lives here rather than at the two embedders because it
    /// is the wire type's own field mapping: written inline it is four lines
    /// per backend, and the failure a second copy permits — a GUI that reports
    /// `shadowed` from one fact and a TUI from another — is exactly the §2 #6
    /// divergence `scene/accelerators` exists to make visible.
    #[must_use]
    pub fn resolve(
        chord: &Chord,
        claimed: Option<pinion_core::accelerator::AcceleratorLayer>,
        shadowed_by: Option<String>,
    ) -> Self {
        Self {
            accel: chord.portable(),
            claimed_by: claimed.map(pinion_core::accelerator::AcceleratorLayer::as_name),
            shadowed: shadowed_by.is_some(),
            shadowed_by,
        }
    }
}

/// Response payload for `scene/accelerators`.
#[derive(Debug, Clone, Serialize)]
pub struct AcceleratorsOutcome {
    /// Every live accelerator, mnemonics in paint order then probed
    /// `keybinding` claims in code-point order.
    pub accelerators: Vec<AcceleratorEntry>,
    /// The focus the shadows were resolved against. `null` when nothing has
    /// focus, in which case no row can be shadowed.
    pub focused: Option<String>,
    /// The one widget currently shadowing anything, if any. At most one, and
    /// it is always the focused one — the mechanism is bounded by construction.
    pub shadowing: Option<String>,
    /// The character range the `keybinding` layer was probed over, as
    /// `"U+0020..=U+007E"`. Named rather than implied: a binding that maps a
    /// non-ASCII key is out of this scope, and a bounded list that reads as
    /// complete is worse than a bounded list that says so.
    pub probed: String,
    /// Present only when the request asked about a chord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chord: Option<ChordVerdict>,
}

/// Build the `scene/accelerators` response.
///
/// `rows` and `verdict` are resolved by the embedder from
/// `CoreShell::accelerator_map_for_window` / `CoreShell::accelerator_for_chord`
/// — the [`crate::DispatchContext`] `input_state` pattern, because only the
/// shell can see the paint scene, the focus and the binding's `keybinding`
/// function at once.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for owned strings and bools is
/// unreachable in practice; it is surfaced rather than unwrapped so an RPC
/// handler never panics the shell.
pub fn handle_scene_accelerators(
    rows: &[AcceleratorRow],
    focused: Option<&str>,
    verdict: Option<ChordVerdict>,
) -> Result<Value, RpcError> {
    let accelerators: Vec<AcceleratorEntry> = rows
        .iter()
        .map(|row| AcceleratorEntry {
            accel: row.accel.clone(),
            layer: row.layer.as_name(),
            target: row.target.clone(),
            label: row.label.clone(),
            shadowed: row.shadowed_by.is_some(),
            shadowed_by: row.shadowed_by.clone(),
        })
        .collect();
    let shadowing = rows
        .iter()
        .find_map(|row| row.shadowed_by.clone())
        .or_else(|| verdict.as_ref().and_then(|v| v.shadowed_by.clone()));
    let outcome = AcceleratorsOutcome {
        accelerators,
        focused: focused.map(str::to_owned),
        shadowing,
        probed: format!(
            "U+{:04X}..=U+{:04X}",
            *ASCII_PROBE_RANGE.start(),
            *ASCII_PROBE_RANGE.end(),
        ),
        chord: verdict,
    };
    serde_json::to_value(outcome).map_err(RpcError::internal_error)
}

/// Resolve `scene/accelerators`' optional `chord` parameter into a verdict.
///
/// The whole embedder-side pre-resolve in one place: the method gate, the
/// parameter read, the tolerance of a malformed spelling (which falls through
/// to the refusal `scene/accelerators` itself produces), and the verdict's
/// construction. `resolve` is the only per-backend part — the shell call that
/// answers what the chord would do.
///
/// Lifted at its SECOND consumer rather than its third. The rule that waits
/// for a third exists to avoid abstracting a shape that has not settled, and
/// here the shape cannot gain a third: there are exactly two backends. What a
/// second copy buys instead is the §2 #6 divergence R1569's own counterfactual
/// found — removing the TUI gate left every GUI assertion green, so the two
/// embedders are precisely where a window and a terminal drift unobserved.
#[must_use]
pub fn resolve_chord_param(
    method: &str,
    params: Option<&Value>,
    resolve: impl FnOnce(
        &Chord,
    ) -> (
        Option<pinion_core::accelerator::AcceleratorLayer>,
        Option<String>,
    ),
) -> Option<ChordVerdict> {
    if method != "scene/accelerators" {
        return None;
    }
    let chord = parse_chord_param(params).ok().flatten()?;
    let (claimed, shadowed_by) = resolve(&chord);
    Some(ChordVerdict::resolve(&chord, claimed, shadowed_by))
}

/// Read the optional `chord` request parameter.
///
/// # Errors
///
/// [`RpcError::invalid_params`] naming which part of the spelling failed, so a
/// typo is a refusal rather than a silently different chord — Qt's
/// `QKeySequence::fromString` substitutes `Qt::Key_unknown` and reports
/// nothing.
pub fn parse_chord_param(params: Option<&Value>) -> Result<Option<Chord>, RpcError> {
    let Some(raw) = params.and_then(|p| p.get("chord")) else {
        return Ok(None);
    };
    let Some(text) = raw.as_str() else {
        return Err(RpcError::invalid_params("chord must be a string"));
    };
    Chord::parse(text)
        .map(Some)
        .map_err(|err: ChordParseError| RpcError::invalid_params(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ChordVerdict, handle_scene_accelerators, parse_chord_param};
    use pinion_core::accelerator::AcceleratorLayer;
    use pinion_runtime::AcceleratorRow;
    use serde_json::json;

    fn row(accel: &str, layer: AcceleratorLayer, shadowed_by: Option<&str>) -> AcceleratorRow {
        AcceleratorRow {
            accel: accel.to_owned(),
            layer,
            target: "menu#t0".to_owned(),
            label: "File".to_owned(),
            shadowed_by: shadowed_by.map(str::to_owned),
        }
    }

    #[test]
    fn a_shadow_is_attributed_not_merely_reported() {
        // Qt's `ShortcutOverride` is anonymous: `accept()` leaves no record of
        // who accepted, so "something took this key" is the most a Qt
        // application could ever learn.
        let out = handle_scene_accelerators(
            &[
                row("Alt+f", AcceleratorLayer::Mnemonic, Some("kseq")),
                row("d", AcceleratorLayer::Keybinding, None),
            ],
            Some("kseq"),
            None,
        )
        .expect("serializes");
        assert_eq!(out["accelerators"][0]["shadowed"], json!(true));
        assert_eq!(out["accelerators"][0]["shadowed_by"], json!("kseq"));
        assert_eq!(out["accelerators"][1]["shadowed"], json!(false));
        assert_eq!(out["accelerators"][1]["shadowed_by"], json!(null));
        assert_eq!(out["shadowing"], json!("kseq"));
        assert_eq!(out["focused"], json!("kseq"));
        assert_eq!(out["probed"], json!("U+0020..=U+007E"));
        assert!(out.get("chord").is_none(), "absent unless asked");
    }

    #[test]
    fn an_unasked_chord_leaves_the_key_out_rather_than_answering_null() {
        // `null` would read as "asked, and it does nothing"; absent reads as
        // "not asked". They are different facts.
        let out = handle_scene_accelerators(&[], None, None).expect("serializes");
        assert!(out.get("chord").is_none());
        assert_eq!(out["shadowing"], json!(null));
        let asked = handle_scene_accelerators(
            &[],
            None,
            Some(ChordVerdict {
                accel: "Ctrl+s".to_owned(),
                claimed_by: Some("keybinding"),
                shadowed: false,
                shadowed_by: None,
            }),
        )
        .expect("serializes");
        assert_eq!(asked["chord"]["claimed_by"], json!("keybinding"));
        assert_eq!(asked["chord"]["accel"], json!("Ctrl+s"));
    }

    /// The method gate, reached DIRECTLY because that is the only shape that
    /// reaches it: the verdict is read by one arm, so removing the gate costs
    /// work and changes no response — R1569's counterfactual removed it and
    /// every wire assertion still passed. The lift inherited that gap from
    /// both embedders' inline copies, neither of which was ever tested either.
    #[test]
    fn a_verdict_is_resolved_for_this_method_and_no_other() {
        let params = json!({"chord": "Ctrl+s"});
        let asked = std::cell::Cell::new(0_u32);
        let probe = |_: &_| {
            asked.set(asked.get() + 1);
            (Some(AcceleratorLayer::Keybinding), None)
        };
        assert!(super::resolve_chord_param("scene/accelerators", Some(&params), probe).is_some(),);
        assert_eq!(asked.get(), 1, "the shell was asked exactly once");
        for other in ["scene/snapshot", "scene/mnemonics", "rpc/methods"] {
            assert!(
                super::resolve_chord_param(other, Some(&params), probe).is_none(),
                "{other} must not resolve a chord even when one is supplied",
            );
        }
        assert_eq!(asked.get(), 1, "and every other dispatch pays NOTHING");
        // A malformed spelling is tolerated here and refused by the method
        // itself, so a bad param cannot make an unrelated dispatch fail.
        let bad = json!({"chord": "Ctrl+Frobnicate+P"});
        assert!(super::resolve_chord_param("scene/accelerators", Some(&bad), probe).is_none());
        assert_eq!(
            asked.get(),
            1,
            "an unreadable chord never reaches the shell"
        );
    }

    #[test]
    fn a_malformed_chord_param_is_refused_by_name() {
        let err = parse_chord_param(Some(&json!({"chord": "Ctrl+Frobnicate+P"})))
            .expect_err("unreadable");
        assert!(format!("{err:?}").contains("Frobnicate"), "{err:?}");
        assert!(
            parse_chord_param(Some(&json!({"chord": 7}))).is_err(),
            "not a string"
        );
        assert_eq!(parse_chord_param(Some(&json!({}))).expect("absent"), None);
        assert_eq!(parse_chord_param(None).expect("no params"), None);
    }
}
