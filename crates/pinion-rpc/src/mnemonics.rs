//! `scene/mnemonics` — the window's accelerator map, published (R1543 §5.12
//! §5.39 §2 #2 §2 #7).
//!
//! Answers the question an agent driving a keyboard-first application has to
//! ask first: **what can I press?** Every mnemonic declared in the painted
//! scene, with the tag it activates, the label it was marked in, the byte
//! offset of the marked character, and whether anything else claims the same
//! key.
//!
//! # Against the toolkit 6.11
//!
//! There is a toolkit peer to be at parity with. The toolkit's
//! accelerator state lives in shortcut map, reachable only through `qshortcutmap_p.h` — a
//! private header — so a toolkit application cannot enumerate its own
//! mnemonics, and an external driver certainly cannot. The nearest public
//! surface is `shortcut()`, one widget at a time, and only for buttons.
//!
//! Two consequences an agent gets here and cannot get there:
//!
//! - **Discovery without pixels.** The map is derived from the paint scene, so
//!   it lists exactly what a sighted user could see underlined, without a
//!   screenshot (§2 #7).
//! - **Conflicts before the keypress.** `ambiguous` is a property of the
//!   scene. The toolkit reports a contested accelerator through
//!   `isAmbiguous()` — only to the application, only at the
//!   moment the user triggered it, by which time one of the two claimants has
//!   already been activated. A window's mnemonic conflicts are assertable here
//!   in one call, which makes "no screen in this app has an accelerator
//!   collision" a CI-checkable property.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "mnemonics": [
//!       { "key": "F", "accel": "Alt+F", "target": "menu#t0",
//!         "label": "File", "index": 0, "ambiguous": false }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, which is
//! the same scene the shell's <kbd>Alt</kbd> arc resolves against, so the map
//! an agent reads and the map a keypress hits are one list built by one
//! function.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/mnemonics", "id": 1 }
//! ```
//!
//! A binding that has not painted yet answers with an empty list rather than
//! an error: "nothing is bound" is the true answer for a scene with no labels
//! in it, and it is also the true answer before the first frame.

use pinion_core::Scene;
use pinion_core::mnemonic::{Mnemonic, scene_mnemonics};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One published accelerator.
#[derive(Debug, Clone, Serialize)]
pub struct MnemonicEntry {
    /// The activation character as authored — what the user sees underlined.
    pub key: String,
    /// The platform accelerator spelling, `"Alt+F"`. The same string the §5.40
    /// tree announces as this target's `accesskey`, so the wire and the AT
    /// agree by construction.
    pub accel: String,
    /// The paint tag <kbd>Alt</kbd>+`key` activates. Composite tags
    /// (`"menu#t0"`) appear as painted, since that is what `scene/invoke` and
    /// `scene/click` address too.
    pub target: String,
    /// The label the mnemonic was marked in, as displayed (markers resolved).
    pub label: String,
    /// UTF-8 byte offset of the marked character within `label`.
    pub index: u32,
    /// Another entry in this list claims the same key, case-insensitively.
    pub ambiguous: bool,
}

/// Response payload for `scene/mnemonics`.
#[derive(Debug, Clone, Serialize)]
pub struct MnemonicsOutcome {
    /// Every mnemonic in the painted scene, in paint order — which is also the
    /// order an ambiguous key cycles through, so a driver can predict the
    /// second press from this list alone.
    pub mnemonics: Vec<MnemonicEntry>,
}

/// Build the `scene/mnemonics` response from the last painted scene.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for a `Vec` of owned strings
/// and integers is unreachable in practice; it is surfaced rather than
/// unwrapped so an RPC handler never panics the shell.
pub fn handle_scene_mnemonics(last_paint_scene: Option<&Scene>) -> Result<Value, RpcError> {
    let mnemonics = last_paint_scene
        .map(scene_mnemonics)
        .unwrap_or_default()
        .into_iter()
        .map(|b| MnemonicEntry {
            key: b.key.to_string(),
            accel: Mnemonic::accel_label(b.key),
            target: b.target,
            label: b.label,
            index: b.index,
            ambiguous: b.ambiguous,
        })
        .collect();
    let outcome = MnemonicsOutcome { mnemonics };
    serde_json::to_value(outcome).map_err(RpcError::internal_error)
}

#[cfg(test)]
mod tests {
    use super::handle_scene_mnemonics;
    use pinion_core::mnemonic::MnemonicLabel;
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};

    fn labelled(tag: &'static str, source: &str) -> Scene {
        let parsed = MnemonicLabel::parse(source);
        let mut text = TextNode::new(parsed.display, Rect::default());
        if let Some(mark) = parsed.mnemonic {
            text = text.with_mnemonic(mark);
        }
        Scene::Container(ContainerNode::new(vec![Scene::Text(text)]).with_tag(tag))
    }

    #[test]
    fn an_unpainted_binding_answers_with_an_empty_list() {
        let value = handle_scene_mnemonics(None).expect("ok");
        assert_eq!(value["mnemonics"].as_array().expect("array").len(), 0);
    }

    #[test]
    fn every_entry_carries_the_whole_binding() {
        let scene = Scene::Container(ContainerNode::new(vec![labelled("menu#t0", "&File")]));
        let value = handle_scene_mnemonics(Some(&scene)).expect("ok");
        let entry = &value["mnemonics"][0];
        assert_eq!(entry["key"], "F");
        assert_eq!(entry["accel"], "Alt+F");
        assert_eq!(entry["target"], "menu#t0");
        assert_eq!(entry["label"], "File");
        assert_eq!(entry["index"], 0);
        assert_eq!(entry["ambiguous"], false);
    }

    #[test]
    fn a_conflict_is_reported_before_anyone_presses_the_key() {
        // The property the toolkit cannot answer: both claimants say so,
        // statically.
        let scene = Scene::Container(ContainerNode::new(vec![
            labelled("save", "&Save"),
            labelled("send", "&Send"),
            labelled("quit", "&Quit"),
        ]));
        let value = handle_scene_mnemonics(Some(&scene)).expect("ok");
        let rows = value["mnemonics"].as_array().expect("array");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["ambiguous"], true);
        assert_eq!(rows[1]["ambiguous"], true);
        assert_eq!(rows[2]["ambiguous"], false);
    }

    #[test]
    fn the_index_addresses_the_published_label() {
        // An agent underlining the key in its own UI must be able to slice
        // `label` at `index` without re-parsing the authored source, which it
        // never sees.
        let scene = Scene::Container(ContainerNode::new(vec![labelled("save_as", "Save &As")]));
        let value = handle_scene_mnemonics(Some(&scene)).expect("ok");
        let entry = &value["mnemonics"][0];
        let label = entry["label"].as_str().expect("str");
        let index = usize::try_from(entry["index"].as_u64().expect("int")).expect("fits");
        assert_eq!(&label[index..=index], "A");
    }
}
