//! R1349 §5.20 PR-58 — the **commit-channel intent vocabulary**: the wire
//! words a settled gesture emits on the `onChangeEnd` half of the
//! live/commit intent pair.
//!
//! Every drag-driven widget in the workspace publishes two §5.20 channels:
//! a **live** one (`value_changing`, the `Signal` preview stream) and a
//! **commit** one — "the gesture ended, that was the final value, persist
//! it now". A binding that mirrors state to a host process / disk listens
//! on the commit word rather than heuristically inferring a settle from
//! frame cadence (R1346 `commit_drag_state`).
//!
//! ## Why the words live here and not next to their emitters
//!
//! The established precedent for a symbolic event name is module-local: a
//! `pinion-widget-paint` `dock::TEAR_OFF_EVENT` sits in the module that
//! emits it, one emitter per word. **The commit family does not have that
//! shape.** Two of its five words have several emitters:
//!
//! * `value_committed` — [`slider`](crate::widgets::slider),
//!   [`color_area`](crate::widgets::color_area), and
//!   [`range_slider`](crate::widgets::range_slider);
//! * `text_committed` — two paths inside
//!   [`text_field`](crate::widgets::text_field) (the state-transition edge
//!   and the IME composition commit), with *different payloads*.
//!
//! A per-module constant would therefore mint several definitions of one
//! wire word, which is the very duplication the export exists to end. One
//! word, one definition — so the family shares a module, and
//! `pinion-widget-paint`'s splitter reaches across the crate boundary for
//! [`RATIO_COMMITTED_EVENT`].
//!
//! This is a claim about *this* family, not a law: `"click"` is legitimately
//! spelled by two constants elsewhere (`tree_view::TREE_ROW_CLICK_EVENT`,
//! `devtools::CLICK_ROUTER_EVENT`) because those are two unrelated routers
//! that happen to share a common English word, not one channel with several
//! emitters. The test is whether a rename must move both, not whether the
//! strings match today.
//!
//! ## Composing the dotted wire form
//!
//! An arriving intent's tag is the dotted `"<widget_tag>.<event>"` form
//! ([[intent-tag-dotted-wire-form]]), so a reducer match arm needs the
//! pair joined. The [`intent_tag!`](crate::intent_tag) macro does that at
//! compile time but is **literal-only** — stable `concat!` does not accept
//! a `const` ref, so `intent_tag!(MY_TAG, RATIO_COMMITTED_EVENT)` does not
//! compile (R1349 corrected three rustdocs and two test comments that
//! claimed otherwise). Join these constants at runtime instead:
//!
//! ```
//! use pinion_core::widgets::commit::RATIO_COMMITTED_EVENT;
//!
//! let split_tag = "main_splitter";
//! let arm = format!("{split_tag}.{RATIO_COMMITTED_EVENT}");
//! assert_eq!(arm, "main_splitter.ratio_committed");
//! ```
//!
//! Referencing the symbol (rather than re-typing the literal) is what makes
//! drift loud: a renamed word arrives through the reference, and a *removed*
//! one is a compile error at the consumer. A hand-mirrored literal fails the
//! other way — it compiles, its tests pass (they feed the consumer's own
//! constant in at both ends), and the user's setting simply stops persisting.
//!
//! ## Honest bound: a `const`-context consumer cannot use these
//!
//! Because the join is a runtime `format!`, a consumer whose reducer arm is a
//! `const` (`const ARM: &str = intent_tag!("main_splitter", "ratio_committed")`
//! — the shape `examples/hello-dock-panels` and `examples/settings-panel` use)
//! cannot adopt the symbol without giving up the compile-time `&'static str`.
//! Those sites keep their literals deliberately; that is the macro's
//! literal-only limit showing through, not an oversight. The constants serve
//! the runtime-join consumers (which is what a binding matching an arriving
//! `Intent::tag_str()` actually is) and, regardless of any consumer, make the
//! emitters themselves reference one definition per word — so a rename is one
//! edit here rather than a hunt through seven emitters.

/// A settled value gesture, emitted on the `Dragging → Hover` activate by
/// **three** widgets:
///
/// * [`SliderExternal`](crate::widgets::slider::SliderExternal) — payload
///   [`IntrospectValue::Float`](crate::external::IntrospectValue::Float), the
///   final value;
/// * [`RangeSliderExternal`](crate::widgets::range_slider::RangeSliderExternal)
///   — `Float`, the *active thumb's* committed value (which thumb is active is
///   context `WidgetTransition::detect`'s snapshot tuple cannot carry, so it
///   emits from its own send path);
/// * [`ColorAreaExternal`](crate::widgets::color_area::ColorAreaExternal) —
///   [`Json`](crate::external::IntrospectValue::Json), the final `{x, y}`.
///
/// One word, two payload shapes: the channel is "a value settled", and what a
/// value *is* differs per widget. `PointerCancel` stays silent on all three.
pub const VALUE_COMMITTED_EVENT: &str = "value_committed";

/// A settled splitter drag — emitted by
/// `pinion_widget_paint::splitter::SplitterExternal` on the `PointerUp` of a
/// drag whose ratio actually moved, carrying the settled ratio
/// ([`IntrospectValue::Float`](crate::external::IntrospectValue::Float)).
/// A bare click is silent (R1346: the press-time cursor forward arms the
/// anchor without moving the ratio), as is `PointerCancel`.
pub const RATIO_COMMITTED_EVENT: &str = "ratio_committed";

/// A settled scrollbar drag — emitted by
/// [`ScrollBarExternal`](crate::widgets::scrollbar::ScrollBarExternal) on the
/// `Dragging → Hover` activate. A pure drag-end marker: payload is
/// [`IntrospectValue::Null`](crate::external::IntrospectValue::Null), because
/// the offset the binding wants is already readable on the scroll state it
/// owns. Drag-cancel (pointer leave / touch cancel) stays silent.
pub const SCROLL_COMMITTED_EVENT: &str = "scroll_committed";

/// A settled column-resize drag — emitted by
/// [`ColumnResizeExternal`](crate::widgets::column_widths::ColumnResizeExternal)
/// on the `PointerUp` of a drag whose width actually moved, carrying the
/// settled width in logical pixels
/// ([`IntrospectValue::Int`](crate::external::IntrospectValue::Int)). The
/// column peer of [`RATIO_COMMITTED_EVENT`], gated by the same "settled !=
/// press snapshot" predicate (R1347); `PointerCancel` is silent.
pub const WIDTH_COMMITTED_EVENT: &str = "width_committed";

/// A settled text edit, emitted by
/// [`text_field`](crate::widgets::text_field) on **two** paths that carry
/// **different payloads** — the one word in this family whose payload is not a
/// single shape:
///
/// * the state-transition edge (`WidgetTransition::detect`), when a
///   commit-bearing event exits the `Editing` state (Enter / blur, not Escape)
///   — payload [`IntrospectValue::Null`](crate::external::IntrospectValue::Null),
///   because the committed text is readable on the field's own state, so the
///   word carries only the edge;
/// * `TextFieldExternal::apply_composition_commit` (the IME path) — payload
///   [`IntrospectValue::Text`](crate::external::IntrospectValue::Text), the
///   committed string. A documented, deliberate payload upgrade (R56.1.a): an
///   IME commit's text is the thing the client did not otherwise have.
///
/// So a reducer on this channel must match the payload, not assume `Null`.
pub const TEXT_COMMITTED_EVENT: &str = "text_committed";

#[cfg(test)]
mod tests {
    use super::*;

    /// R1349 PR-58 — the wire ABI pin. These literals are the words already
    /// on the wire (asserted emitter-side by each widget's own drag-cycle
    /// test); a consumer's reducer arm and a recorded RPC trace both spell
    /// them. Changing one is an ABI break for every binding, so the literal
    /// lives here deliberately rather than being derived from the constant —
    /// a test that compared each constant to itself would pin nothing.
    #[test]
    fn r1349_commit_vocabulary_pins_the_wire_words() {
        assert_eq!(VALUE_COMMITTED_EVENT, "value_committed");
        assert_eq!(RATIO_COMMITTED_EVENT, "ratio_committed");
        assert_eq!(SCROLL_COMMITTED_EVENT, "scroll_committed");
        assert_eq!(WIDTH_COMMITTED_EVENT, "width_committed");
        assert_eq!(TEXT_COMMITTED_EVENT, "text_committed");
    }

    /// R1349.1 PR-58 — every emitter of a commit word references this module,
    /// so the constants are the family's only definition.
    ///
    /// This is a SOURCE-TEXT check, which is unusual and deliberate. The
    /// property is "no bare literal survives anywhere", and no runtime
    /// assertion can see a literal that a widget never emits during a test. The
    /// first cut of R1349 shipped this module while `range_slider` and
    /// `text_field`'s IME path still spelled their words by hand — missed
    /// because the enumerating grep was truncated, and invisible to the whole
    /// suite because both emitters were green either way. A doc claiming the
    /// family is unified while two emitters are not is exactly the confidently
    /// wrong doc this file warns about, so the claim gets a test.
    #[test]
    fn r1349_1_no_emitter_spells_a_commit_word_by_hand() {
        // Walk this crate's widget sources; `pinion-widget-paint`'s splitter is
        // out of reach from here and is covered by its own crate's build (it
        // references `commit::RATIO_COMMITTED_EVENT` directly).
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/widgets");
        let words = [
            "value_committed",
            "ratio_committed",
            "scroll_committed",
            "width_committed",
            "text_committed",
        ];
        let mut offenders: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(dir).expect("widgets dir readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|f| f == "commit.rs")
            {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("source readable");
            for (n, line) in src.lines().enumerate() {
                // Stop at the file's test module. A test SHOULD spell the word
                // as a literal — that is the wire-ABI pin (see
                // `r1349_commit_vocabulary_pins_the_wire_words`): an assertion
                // that fed the constant in at both ends would pin nothing. Only
                // production emitters must go through the constant. Every
                // widget module in this crate puts `#[cfg(test)]` after its
                // production code, so a prefix scan is exact rather than
                // heuristic; a file that ever inverts that order fails loudly
                // here rather than silently skipping a real emitter.
                if line.trim_start().starts_with("#[cfg(test)]") {
                    break;
                }
                // Docs/comments legitimately name the words.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for w in words {
                    if line.contains(&format!("\"{w}\"")) {
                        offenders.push(format!(
                            "{}:{} spells {w:?} by hand",
                            path.file_name().unwrap().to_string_lossy(),
                            n + 1,
                        ));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "every emitter must reference widgets::commit, not a literal:\n{}",
            offenders.join("\n"),
        );
    }

    /// R1349 PR-58 — the words are distinct. A copy-paste slip that pointed
    /// two constants at one word would route two channels to one reducer arm
    /// and stay green under the per-constant assertions above.
    #[test]
    fn r1349_commit_words_are_distinct() {
        let words = [
            VALUE_COMMITTED_EVENT,
            RATIO_COMMITTED_EVENT,
            SCROLL_COMMITTED_EVENT,
            WIDTH_COMMITTED_EVENT,
            TEXT_COMMITTED_EVENT,
        ];
        let mut seen: Vec<&str> = Vec::new();
        for w in words {
            assert!(!seen.contains(&w), "duplicate commit word {w:?}");
            seen.push(w);
        }
    }
}
