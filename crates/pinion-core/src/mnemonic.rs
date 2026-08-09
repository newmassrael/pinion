//! R1543 §5.20 §5.39 §5.40 — label mnemonics (the toolkit `&File`).
//!
//! A **mnemonic** is one character of a widget's *visible label*, marked in
//! the label source with `&`, that activates that widget from anywhere in the
//! window via <kbd>Alt</kbd>+char and is drawn underlined so it can be
//! discovered without documentation. Every desktop toolkit has one; the toolkit spells
//! it `&File` / [`QKeySequence::mnemonic`] / [`QLabel::setBuddy`], GTK spells
//! it `_File`, Win32 spells it `&File`, and HTML spells it `accesskey`.
//!
//! Before R1543 pinion had **none of it**. The `menu` module deferred
//! "accelerator / mnemonic keys" as an axis awaiting a real consumer, and what
//! actually happened is what an absent extension point always causes: 96
//! bindings hand-rolled a [`WidgetCore::keybinding`](crate::WidgetCore::keybinding)
//! map of *bare* characters (`hello-button`'s `d` / `e`) that has no relation
//! to any painted label, draws no underline, is invisible to assistive
//! technology, cannot be checked for conflicts, and collides with text input
//! because it does not use a modifier.
//!
//! ## The decomposition
//!
//! One declaration, three derived facts:
//!
//! | Fact | Derived by | Reaches |
//! |---|---|---|
//! | the **ink** (which character is underlined) | [`TextNode::with_mnemonic`](crate::scene::TextNode::with_mnemonic) lowering to a [`StyleRun`](crate::scene::StyleRun) | both painters, unchanged |
//! | the **binding** (what <kbd>Alt</kbd>+char activates) | [`scene_mnemonics`] over the paint scene | the shell's character-key arc |
//! | the **announcement** (`accesskey`) | the §5.40 enrichment pass | AccessKit → UIA / AT-SPI / AX |
//!
//! The declaration ([`Mnemonic`] on the label's `TextNode`) is the sole
//! authority. The underline is *derived ink*, never read back — a mnemonic is
//! dispatched from the field, not by looking for an underlined character. That
//! separation is deliberate: R1542 recorded that authority cannot be recovered
//! from the value it produced, and here the recovery would additionally be
//! ambiguous (rich text underlines characters for reasons of its own).
//!
//! ## Where this is more than the toolkit 6.11
//!
//! 1. **The map is a published fact.** [`scene_mnemonics`] enumerates every
//!    mnemonic in a window with its target, its label and its conflicts; the
//!    §5.12 `scene/mnemonics` method hands the same list to an agent. The toolkit's
//!    equivalent state lives in shortcut map, which is private
//!    (`qshortcutmap_p.h`) — an application cannot ask the toolkit what its own
//!    accelerators are.
//! 2. **Ambiguity is static and reportable, not a dispatch-time surprise.** the toolkit
//!    tells you two widgets claim <kbd>Alt</kbd>+F only when the user presses
//!    it, via `isAmbiguous()`. [`MnemonicBinding::ambiguous`]
//!    is a property of the *scene*, so a test or CI gate can assert a window
//!    has no conflicts before anyone types anything.
//! 3. **The ink and the binding cannot disagree.** the toolkit parses `&` twice through
//!    unrelated code — `mnemonic` for the shortcut and
//!    `drawItemText` for the underline, re-run on every paint. Here
//!    both come from one parse held in one field.
//! 4. **The target is structural, not wired.** A widget's mnemonic addresses
//!    the widget whose label carries it — the innermost enclosing tagged
//!    container — so nothing has to be connected. `setBuddy`'s explicit
//!    pointer survives only as the *override* ([`Mnemonic::with_buddy`]), for
//!    the one case where the label is not the widget's own.
//!
//! ## What is deliberately the toolkit's shape, not more
//!
//! The parse is the toolkit's, character for character (see [`MnemonicLabel::parse`]). A mnemonic
//! vocabulary that differed from `&`/`&&` would make every label literal a
//! porting hazard for no capability gained — [[the
//! toolkit-is-the-floor-not-the-target]] makes the *existence* of a feature
//! the toolkit's floor and leaves its *shape* a fresh choice each time, and
//! here the fresh choice is that the toolkit's shape is already right.
//!
//! [`QKeySequence::mnemonic`]: https://doc.qt.io/qt-6/qkeysequence.html
//! [`QLabel::setBuddy`]: https://doc.qt.io/qt-6/qlabel.html

use std::borrow::Cow;
use std::collections::HashMap;

use crate::scene::Scene;

/// R1543 §5.39 — one label character marked as an activation key.
///
/// Produced by [`MnemonicLabel::parse`] and carried on the label's
/// [`TextNode`](crate::scene::TextNode). `index` / `len` address the character
/// inside the **display** string (the one with the `&` markers removed), so
/// they index the same bytes the painter and the shaper see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mnemonic {
    /// The activation character **as written in the label**.
    ///
    /// Matching is case-insensitive ([`Mnemonic::matches`]); this field keeps
    /// the authored spelling so the published map and the AT announcement can
    /// show what the user actually sees underlined.
    pub key: char,
    /// UTF-8 byte offset of `key` within the display string.
    pub index: u32,
    /// UTF-8 byte length of `key` (1..=4).
    pub len: u32,
    /// The toolkit `setBuddy` — the tag this mnemonic activates instead of the
    /// widget whose label it is.
    ///
    /// `None` (the common case) targets the innermost enclosing tagged container:
    /// a button, a menu title and a checkbox all label *themselves*, so
    /// nothing has to be wired. `Some(tag)` is the standalone-label case — `&Name:` in a
    /// form focuses the field beside it — which is the only situation the
    /// toolkit's explicit buddy pointer ever described.
    pub buddy: Option<Cow<'static, str>>,
}

impl Mnemonic {
    /// Construct a mnemonic addressing `key` at display-string byte range
    /// `[index, index + len)`.
    ///
    /// Prefer [`MnemonicLabel::parse`], which derives all three from one
    /// authored string and therefore cannot disagree with the label it marks.
    #[must_use]
    pub const fn new(key: char, index: u32, len: u32) -> Self {
        Self {
            key,
            index,
            len,
            buddy: None,
        }
    }

    /// The toolkit `setBuddy` — retarget this mnemonic at another tag
    /// (builder form). See [`Self::buddy`].
    #[must_use]
    pub fn with_buddy(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.buddy = Some(tag.into());
        self
    }

    /// The canonical form two mnemonic characters share iff they denote the
    /// **same** accelerator.
    ///
    /// Full Unicode lowercase mapping (`char::to_lowercase`), not a truncated
    /// `.next()`: `'İ'` lowercases to two scalars and must not silently
    /// collapse onto `'i'`, and `'ẞ'` must fold onto `'ß'`. Allocating is
    /// acceptable here because this is the *grouping* key, computed once per
    /// binding when the map is built; the hot path ([`Self::matches`]) compares
    /// the iterators directly and allocates nothing.
    #[must_use]
    pub fn fold(key: char) -> String {
        key.to_lowercase().collect()
    }

    /// Whether a typed character activates this mnemonic — case-insensitive
    /// under the same rule [`Self::fold`] groups by.
    #[must_use]
    pub fn matches(&self, typed: char) -> bool {
        self.key.to_lowercase().eq(typed.to_lowercase())
    }

    /// The platform accelerator spelling for assistive technology and for the
    /// published map — `"Alt+F"`.
    ///
    /// An associated function over the bare character (like [`Self::fold`])
    /// rather than a method, because both a declaration ([`Mnemonic`]) and a
    /// resolved binding ([`MnemonicBinding`]) need the spelling and neither
    /// should have to build the other to get it.
    ///
    /// Upper-cased because that is the convention every AT and every menu
    /// renderer shows (UIA `AccessKey`, AT-SPI `AccessibleKeyBinding`), not
    /// because the match is case-sensitive.
    #[must_use]
    pub fn accel_label(key: char) -> String {
        let key: String = key.to_uppercase().collect();
        format!("Alt+{key}")
    }
}

/// R1543 §5.39 — the result of parsing an authored label: the string to paint
/// and the mnemonic it marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MnemonicLabel {
    /// The label with every `&` marker resolved — what the widget paints and
    /// what the §5.40 name enrichment announces.
    pub display: String,
    /// The marked character, or `None` when the source declared none.
    pub mnemonic: Option<Mnemonic>,
}

impl MnemonicLabel {
    /// Parse the toolkit's `&`-marked label vocabulary.
    ///
    /// The rules, in full — deliberately the toolkit's, so a label literal
    /// ports both ways unchanged:
    ///
    /// * `&&` is a **literal ampersand**, never a marker: `"Save && Exit"`
    ///   displays `Save & Exit` with no mnemonic.
    /// * `&` followed by any other character **marks that character** and is
    ///   removed from the display: `"&File"` displays `File`, mnemonic `F` at
    ///   byte 0.
    /// * Only the **first** marker binds. Later ones are still stripped from
    ///   the display (the toolkit strips in the style and binds in key sequence, and
    ///   the two disagree about how many exist; stripping is the behaviour a
    ///   reader sees).
    /// * A **trailing lone `&`** is dropped.
    ///
    /// A marker on a non-BMP character is fine: `index` / `len` are UTF-8 byte
    /// offsets, and `len` is that character's real encoded width.
    ///
    /// The mnemonic is dropped (the marker is still stripped) for a label whose
    /// display string exceeds `u32::MAX` bytes, since the offset would not be
    /// representable. That is 4 GiB of label.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let mut display = String::with_capacity(source.len());
        let mut mnemonic: Option<Mnemonic> = None;
        let mut chars = source.chars();
        while let Some(c) = chars.next() {
            if c != '&' {
                display.push(c);
                continue;
            }
            match chars.next() {
                // Trailing lone `&` — dropped, exactly as the toolkit drops
                // it.
                None => {}
                // `&&` — a literal ampersand, not a marker.
                Some('&') => display.push('&'),
                Some(marked) => {
                    if mnemonic.is_none()
                        && let Ok(index) = u32::try_from(display.len())
                    {
                        mnemonic = Some(Mnemonic::new(marked, index, utf8_len(marked)));
                    }
                    display.push(marked);
                }
            }
        }
        Self { display, mnemonic }
    }
}

/// `char::len_utf8` as a `u32`, total by construction.
///
/// The value is 1..=4 by definition of UTF-8, so this is a widening that
/// cannot fail — written as a match rather than an `as` cast so no lint has to
/// be silenced and no impossible fallback has to be invented.
const fn utf8_len(c: char) -> u32 {
    match c.len_utf8() {
        1 => 1,
        2 => 2,
        3 => 3,
        _ => 4,
    }
}

/// R1543 §5.39 §5.12 — one resolved mnemonic in a painted scene: which key,
/// which widget it activates, and whether anything else claims the same key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MnemonicBinding {
    /// The activation character as authored (see [`Mnemonic::key`]).
    pub key: char,
    /// The tag <kbd>Alt</kbd>+[`Self::key`] activates — the buddy override
    /// when the declaration carried one, else the widget whose label this is.
    pub target: String,
    /// The label the mnemonic was marked in, as displayed. What an agent (or a
    /// menu-discovery overlay) shows beside the key.
    pub label: String,
    /// UTF-8 byte offset of the marked character within [`Self::label`].
    pub index: u32,
    /// Another binding in the same scene claims the same key under
    /// [`Mnemonic::fold`].
    ///
    /// The toolkit surfaces this only at dispatch time, as a bool on the event
    /// the user triggered; here it is a property of the scene, so it can be
    /// asserted before anyone types. Dispatch still resolves an ambiguous key
    /// rather than refusing it — see the shell's cycling rule.
    pub ambiguous: bool,
}

/// R1543 §5.39 §5.12 — every mnemonic declared in `scene`, in paint order.
///
/// Walks the painted tree carrying the innermost tagged
/// [`Scene::Container`] seen so far. Each [`Scene::Text`] that declares a
/// [`Mnemonic`] resolves its target by precedence:
///
/// 1. [`Mnemonic::buddy`] when set (`setBuddy`),
/// 2. else the innermost enclosing tagged container — *the widget whose label
///    this is*, which is why the common case needs no wiring at all,
/// 3. else the text node's own tag, for a label that is itself the addressable
///    node.
///
/// A declaration with no resolvable target is skipped: it can activate
/// nothing, so publishing it would describe a binding that does not exist.
///
/// The walk descends into [`Scene::Scroll`], which is transparent to a tag
/// walk exactly as `Scene::rect_for_tag_with_offset`, `Scene::lookup_path_ref`
/// and (since R1536) the §5.40 name enrichment treat it. Omitting that arm is
/// how R1536 found every virtualized row unnamed to assistive technology for
/// ~760 rounds; a mnemonic inside a scrolling pane would have failed the same
/// silent way.
///
/// Ambiguity is resolved after collection: bindings whose keys share a
/// [`Mnemonic::fold`] are all marked [`MnemonicBinding::ambiguous`].
#[must_use]
pub fn scene_mnemonics(scene: &Scene) -> Vec<MnemonicBinding> {
    let mut out = Vec::new();
    collect(scene, None, &mut out);

    // One pass to count claims per folded key, one to stamp. Marking every
    // member of a contested group (rather than "all but the first") is what
    // makes the field answer the question a reader has — *is this key
    // contested* — instead of an ordering artefact.
    let mut claims: HashMap<String, usize> = HashMap::new();
    for binding in &out {
        *claims.entry(Mnemonic::fold(binding.key)).or_default() += 1;
    }
    for binding in &mut out {
        binding.ambiguous = claims
            .get(&Mnemonic::fold(binding.key))
            .copied()
            .unwrap_or(0)
            > 1;
    }
    out
}

/// DFS pre-order half of [`scene_mnemonics`], carrying the innermost tagged
/// container as the default target.
fn collect(scene: &Scene, owner: Option<&str>, out: &mut Vec<MnemonicBinding>) {
    match scene {
        Scene::Container(c) => {
            let owner = c.tag.as_deref().or(owner);
            for child in &c.children {
                collect(child, owner, out);
            }
        }
        Scene::Scroll(s) => collect(&s.content, owner, out),
        Scene::Text(t) => {
            let Some(mnemonic) = &t.mnemonic else {
                return;
            };
            let target = mnemonic
                .buddy
                .as_deref()
                .or(owner)
                .or(t.tag.as_deref())
                .map(str::to_owned);
            if let Some(target) = target {
                out.push(MnemonicBinding {
                    key: mnemonic.key,
                    target,
                    label: t.content.clone(),
                    index: mnemonic.index,
                    // Stamped by the caller once every claim is known.
                    ambiguous: false,
                });
            }
        }
        Scene::Box(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::Effect(_)
        | Scene::External(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{Mnemonic, MnemonicLabel, scene_mnemonics};
    use crate::scene::{ContainerNode, Rect, Scene, ScrollNode, TextNode};

    fn label(text: &str, source: &str) -> Scene {
        let parsed = MnemonicLabel::parse(source);
        let mut node = TextNode::new(parsed.display, Rect::default());
        if let Some(m) = parsed.mnemonic {
            node = node.with_mnemonic(m);
        }
        assert_eq!(node.content, text, "display string");
        Scene::Text(node)
    }

    #[test]
    fn parse_marks_the_first_character_after_the_ampersand() {
        let parsed = MnemonicLabel::parse("&File");
        assert_eq!(parsed.display, "File");
        let m = parsed.mnemonic.expect("marked");
        assert_eq!(m.key, 'F');
        assert_eq!(m.index, 0);
        assert_eq!(m.len, 1);
        assert!(m.buddy.is_none());
    }

    #[test]
    fn parse_marks_an_interior_character_at_its_display_offset() {
        // The offset must index the DISPLAY string, not the source — the `&`
        // is gone by the time anything paints or shapes.
        let parsed = MnemonicLabel::parse("Save &As");
        assert_eq!(parsed.display, "Save As");
        let m = parsed.mnemonic.expect("marked");
        assert_eq!(m.key, 'A');
        assert_eq!(m.index, 5, "byte offset of 'A' in \"Save As\"");
    }

    #[test]
    fn double_ampersand_is_a_literal_and_marks_nothing() {
        let parsed = MnemonicLabel::parse("Save && Exit");
        assert_eq!(parsed.display, "Save & Exit");
        assert!(parsed.mnemonic.is_none(), "`&&` is not a marker");
    }

    #[test]
    fn a_literal_ampersand_does_not_consume_a_later_marker() {
        // The `&&` must not leave the parser mid-marker: the `&E` after it is
        // still a real marker, and its offset must account for the single
        // ampersand that survived into the display string.
        let parsed = MnemonicLabel::parse("R&&D &Export");
        assert_eq!(parsed.display, "R&D Export");
        let m = parsed.mnemonic.expect("marked");
        assert_eq!(m.key, 'E');
        assert_eq!(m.index, 4);
    }

    #[test]
    fn only_the_first_marker_binds_and_later_ones_are_stripped() {
        let parsed = MnemonicLabel::parse("&Save &All");
        assert_eq!(parsed.display, "Save All", "both markers strip");
        assert_eq!(parsed.mnemonic.expect("marked").key, 'S', "first binds");
    }

    #[test]
    fn a_trailing_lone_ampersand_is_dropped() {
        let parsed = MnemonicLabel::parse("Ratio &");
        assert_eq!(parsed.display, "Ratio ");
        assert!(parsed.mnemonic.is_none());
    }

    #[test]
    fn a_label_without_a_marker_is_unchanged() {
        let parsed = MnemonicLabel::parse("Preferences");
        assert_eq!(parsed.display, "Preferences");
        assert!(parsed.mnemonic.is_none());
    }

    #[test]
    fn a_multibyte_marker_reports_its_encoded_width() {
        // `len` is a UTF-8 byte length, not a character count — the derived
        // underline run spans exactly the marked character's bytes.
        let parsed = MnemonicLabel::parse("Fichier (&É)");
        let m = parsed.mnemonic.expect("marked");
        assert_eq!(m.key, 'É');
        assert_eq!(m.len, 2, "É is two UTF-8 bytes");
        assert_eq!(m.index, 9);
        assert_eq!(&parsed.display[9..11], "É");
    }

    #[test]
    fn matching_is_case_insensitive_both_ways() {
        let m = Mnemonic::new('F', 0, 1);
        assert!(m.matches('f'));
        assert!(m.matches('F'));
        assert!(!m.matches('e'));
        let lower = Mnemonic::new('é', 0, 2);
        assert!(lower.matches('É'));
    }

    #[test]
    fn fold_uses_full_lowercase_mapping_not_a_truncation() {
        // A `.next()` truncation would collapse `İ` onto `i` and make two
        // distinct keys collide. The full mapping keeps them apart.
        assert_eq!(Mnemonic::fold('F'), "f");
        assert_eq!(Mnemonic::fold('İ'), "i\u{307}");
        assert_ne!(Mnemonic::fold('İ'), Mnemonic::fold('i'));
        assert!(!Mnemonic::new('İ', 0, 2).matches('i'));
        // ẞ folds onto ß, which IS the same key.
        assert_eq!(Mnemonic::fold('ẞ'), Mnemonic::fold('ß'));
    }

    #[test]
    fn accel_label_is_the_platform_spelling() {
        assert_eq!(Mnemonic::accel_label('f'), "Alt+F");
        assert_eq!(Mnemonic::accel_label('É'), "Alt+É");
    }

    #[test]
    fn a_mnemonic_targets_the_widget_whose_label_it_is() {
        let scene =
            Scene::Container(ContainerNode::new(vec![label("File", "&File")]).with_tag("menu#t0"));
        let found = scene_mnemonics(&scene);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, 'F');
        assert_eq!(found[0].target, "menu#t0", "no wiring needed");
        assert_eq!(found[0].label, "File");
        assert!(!found[0].ambiguous);
    }

    #[test]
    fn the_innermost_tagged_container_wins_over_an_outer_one() {
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Container(
                ContainerNode::new(vec![label("OK", "&OK")]).with_tag("ok_btn"),
            )])
            .with_tag("dialog"),
        );
        let found = scene_mnemonics(&scene);
        assert_eq!(found[0].target, "ok_btn");
    }

    #[test]
    fn a_buddy_overrides_the_enclosing_widget() {
        // `setBuddy` — a standalone label focuses the field beside it.
        let parsed = MnemonicLabel::parse("&Name:");
        let node = TextNode::new(parsed.display, Rect::default())
            .with_mnemonic(parsed.mnemonic.expect("marked").with_buddy("name_field"));
        let scene =
            Scene::Container(ContainerNode::new(vec![Scene::Text(node)]).with_tag("name_label"));
        let found = scene_mnemonics(&scene);
        assert_eq!(found[0].target, "name_field", "buddy beats the container");
    }

    #[test]
    fn a_mnemonic_inside_a_scroll_is_found() {
        // The R1536 lesson, applied before it could cost anything: a walk that
        // stops at `Scene::Scroll` silently loses everything in a scrolling
        // pane while looking structurally correct.
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::default(),
            Scene::Container(
                ContainerNode::new(vec![label("Apply", "&Apply")]).with_tag("apply_btn"),
            ),
        ));
        let found = scene_mnemonics(&scene);
        assert_eq!(found.len(), 1, "a scroll is transparent to the walk");
        assert_eq!(found[0].target, "apply_btn");
    }

    #[test]
    fn a_declaration_with_no_resolvable_target_is_not_published() {
        // Publishing it would describe a binding that activates nothing.
        let scene = label("Untagged", "&Untagged");
        assert!(scene_mnemonics(&scene).is_empty());
    }

    #[test]
    fn a_tagged_text_is_its_own_target_when_nothing_encloses_it() {
        let parsed = MnemonicLabel::parse("&Link");
        let node = TextNode::new(parsed.display, Rect::default())
            .with_mnemonic(parsed.mnemonic.expect("marked"))
            .with_tag("link");
        let found = scene_mnemonics(&Scene::Text(node));
        assert_eq!(found[0].target, "link");
    }

    #[test]
    fn contested_keys_are_all_marked_ambiguous() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![label("Save", "&Save")]).with_tag("save")),
            Scene::Container(ContainerNode::new(vec![label("Send", "&Send")]).with_tag("send")),
            Scene::Container(ContainerNode::new(vec![label("Quit", "&Quit")]).with_tag("quit")),
        ]));
        let found = scene_mnemonics(&scene);
        assert_eq!(found.len(), 3);
        assert!(found[0].ambiguous, "S is claimed twice");
        assert!(found[1].ambiguous, "so BOTH claimants say so");
        assert!(!found[2].ambiguous, "Q is claimed once");
    }

    #[test]
    fn ambiguity_is_case_insensitive() {
        // `&Save` and `&send` contest the same key; a case-sensitive grouping
        // would report a clean window and then cycle at dispatch time.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![label("Save", "&Save")]).with_tag("save")),
            Scene::Container(ContainerNode::new(vec![label("send", "&send")]).with_tag("send")),
        ]));
        let found = scene_mnemonics(&scene);
        assert!(found[0].ambiguous && found[1].ambiguous);
    }

    #[test]
    fn bindings_are_published_in_paint_order() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![label("File", "&File")]).with_tag("f")),
            Scene::Container(ContainerNode::new(vec![label("Edit", "&Edit")]).with_tag("e")),
            Scene::Container(ContainerNode::new(vec![label("View", "&View")]).with_tag("v")),
        ]));
        let keys: Vec<char> = scene_mnemonics(&scene).iter().map(|b| b.key).collect();
        assert_eq!(
            keys,
            vec!['F', 'E', 'V'],
            "paint order, so cycling is stable"
        );
    }
}
