//! `scene/containment` — R1656 §5.12 §5.32 §5.36 §2 #7: **every painted mark
//! that left the box that owns it.**
//!
//! The read that makes "the label is drawn outside the card" askable. Its
//! reason for existing, and the measurement behind every sentence here, is in
//! [`pinion_core::containment`]; this module is the wire form.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "escapes": [
//!       {
//!         "tag": null,
//!         "path": "4/0/content/2231",
//!         "owner": "lab.node.T-01",
//!         "content": "data, express",
//!         "promised": { "x": 1740, "y": 1801, "w": 62, "h": 11 },
//!         "painted":  { "x": 1740, "y": 1801, "w": 56, "h": 14 },
//!         "owner_rect": { "x": 1688, "y": 1730, "w": 122, "h": 77 },
//!         "over": { "left": 0, "top": 0, "right": 0, "bottom": 5 },
//!         "fate": "smeared"
//!       }
//!     ],
//!     "smeared": 15,
//!     "clipped": 0,
//!     "marks": 331
//!   }
//! }
//! ```
//!
//! Request — no parameters; it reads the last painted scene, so what it reports
//! is what is on the screen right now.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/containment", "id": 1 }
//! ```
//!
//! `marks` is beside the two counts for the reason `deliverable` sits beside
//! `pointer_reach`'s lists: an empty `escapes` on a surface that painted
//! nothing is not the same answer as an empty one on a surface that painted
//! three hundred marks, and a caller reading only the list cannot tell them
//! apart.
//!
//! `over` is four numbers rather than a boolean because the boolean was
//! measured useless — see the core module. `fate` separates a mark drawn over
//! its neighbour from one an enclosing clip cut away; both lose the reader
//! something, and the repairs differ.

use pinion_core::Scene;
use pinion_core::containment::{Escape, Fate};
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// How far a mark reached past its owner, per edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OverhangReport {
    /// Pixels past the owner's left edge.
    pub left: u32,
    /// Pixels past the owner's top edge.
    pub top: u32,
    /// Pixels past the owner's right edge.
    pub right: u32,
    /// Pixels past the owner's bottom edge.
    pub bottom: u32,
}

/// A window-absolute rectangle, in the spelling every other read here uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RectReport {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl From<pinion_core::scene::Rect> for RectReport {
    fn from(r: pinion_core::scene::Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }
    }
}

/// One painted mark that did not stay inside the box that owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EscapeReport {
    /// The mark's own tag, when it has one. A text run usually does not.
    pub tag: Option<String>,
    /// The mark's address, `/`-joined the way `scene/locate` reports it.
    pub path: String,
    /// The box it escaped — its tag, or `<untagged>` when it has none.
    pub owner: String,
    /// What a text run holds: the string a reader is losing.
    pub content: Option<String>,
    /// The box the scene promised the mark.
    pub promised: RectReport,
    /// What was actually painted — shaped ink for a text run.
    pub painted: RectReport,
    /// The owner's box.
    pub owner_rect: RectReport,
    /// How far past each of the owner's edges the paint reached.
    pub over: OverhangReport,
    /// `smeared` (drawn over the neighbour) or `clipped` (cut away silently).
    pub fate: &'static str,
    /// R1674 — **which parts of the owner** the mark landed on, in the order a
    /// painter lays them: `outside`, `border`, and `chrome:<role>` for a band
    /// the owner reserved for itself. Never empty, and more than one when a
    /// mark crosses several: a header label drawn full-bleed reports
    /// `["border", "chrome:header"]`.
    ///
    /// This is the field the reference toolkit has no form for. Probed at 6.11,
    /// a widget's reservation is four integers with the reason discarded, so
    /// "over the title" and "out of bounds" arrive as the same answer.
    pub trespass: Vec<String>,
}

/// The `scene/containment` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainmentOutcome {
    /// Every escape, in paint order.
    pub escapes: Vec<EscapeReport>,
    /// How many are drawn over a neighbour.
    pub smeared: usize,
    /// How many have their overhang cut away by a clip.
    pub clipped: usize,
    /// How many painted marks were examined — what keeps an empty list from
    /// reading as coverage on a surface that painted nothing.
    pub marks: usize,
}

/// Typed errors [`handle_scene_containment`] can return.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainmentError {
    /// The embedder installed no report.
    ///
    /// Distinct from an empty list, and the distinction is the point: empty
    /// means "nothing escaped", while this means "this host cannot answer" —
    /// a host that never shapes, or a fixture with no shape cache. A caller
    /// asserting "the screen is contained" must not read the second as the
    /// first.
    ContainmentUnavailable,
}

impl ContainmentError {
    /// The word that rides in `error.data`.
    #[must_use]
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::ContainmentUnavailable => "ContainmentUnavailable",
        }
    }
}

/// Turn the core report into its wire form.
#[must_use]
pub fn report(escapes: &[Escape], marks: usize) -> ContainmentOutcome {
    ContainmentOutcome {
        escapes: escapes
            .iter()
            .map(|e| EscapeReport {
                tag: e.tag.clone(),
                path: e.path.join("/"),
                owner: e.owner.clone(),
                content: e.content.clone(),
                promised: e.promised.into(),
                painted: e.painted.into(),
                owner_rect: e.owner_rect.into(),
                over: OverhangReport {
                    left: e.over.left,
                    top: e.over.top,
                    right: e.over.right,
                    bottom: e.over.bottom,
                },
                fate: e.fate.wire_word(),
                trespass: e.trespass.iter().map(|t| t.wire_word()).collect(),
            })
            .collect(),
        smeared: escapes.iter().filter(|e| e.fate == Fate::Smeared).count(),
        clipped: escapes.iter().filter(|e| e.fate == Fate::Clipped).count(),
        marks,
    }
}

/// Measure the painted scene and build the wire report.
///
/// The ink of a text run is asked of the same [`LayoutCache`] the frame shaped
/// with, so this reports what was drawn and not a second opinion about it —
/// asking and then painting costs one shape, as it does for
/// [`crate::text_painted`].
#[must_use]
pub fn collect(scene: &Scene, cache: &mut LayoutCache) -> ContainmentOutcome {
    let mut marks = 0usize;
    scene.for_each_node(&mut |_| marks += 1);
    let escapes = pinion_core::containment::escapes(scene, &mut |t| {
        let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
        cache.ink_size(&t.content, &t.style, &t.runs, max_width)
    });
    report(&escapes, marks)
}

/// `scene/containment` dispatcher entry.
///
/// # Errors
///
/// [`ContainmentError::ContainmentUnavailable`] when the embedder installed no
/// report.
pub fn handle_scene_containment(outcome: Option<&ContainmentOutcome>) -> Result<Value, RpcError> {
    let Some(outcome) = outcome else {
        return Err(RpcError::invalid_params(
            ContainmentError::ContainmentUnavailable.wire_tag(),
        ));
    };
    serde_json::to_value(outcome).map_err(|err| RpcError::internal_error(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::containment::{Fate, Overhang, Trespass};
    use pinion_core::scene::Rect;
    use pinion_core::style::ChromeRole;

    fn escape(fate: Fate) -> Escape {
        Escape {
            tag: None,
            path: vec!["4".into(), "0".into()],
            owner: "card".into(),
            content: Some("data, express".into()),
            promised: Rect::new(1740, 1801, 62, 11),
            painted: Rect::new(1740, 1801, 56, 14),
            owner_rect: Rect::new(1688, 1730, 122, 77),
            over: Overhang {
                left: 0,
                top: 0,
                right: 0,
                bottom: 5,
            },
            fate,
            trespass: vec![Trespass::Chrome(ChromeRole::Header)],
        }
    }

    /// ★ The counts are derived from the list, so a caller cannot be told
    /// "nothing is smeared" beside a list of smears.
    #[test]
    fn r1656_the_counts_are_derived_from_the_list() {
        let out = report(&[escape(Fate::Smeared), escape(Fate::Clipped)], 331);
        assert_eq!(out.escapes.len(), 2);
        assert_eq!(out.smeared, 1);
        assert_eq!(out.clipped, 1);
        assert_eq!(out.marks, 331);
        assert_eq!(out.escapes[0].fate, "smeared");
        assert_eq!(out.escapes[1].fate, "clipped");
    }

    /// ★★★ R1674 — the trespass survives the trip, with the words it was
    /// given.
    ///
    /// Written because a counterfactual passed: emptying the list left the
    /// whole of `pinion-rpc` green. The field is this round's answer to the
    /// question the floor has no form for — probed at 6.11, "over the title"
    /// and "out of bounds" arrive there as the same four integers — so a wire
    /// form that silently drops it withholds exactly what was built.
    ///
    /// The words are compared against the type's own `wire_word`, not against
    /// literals, so this and `box_style_to_json`'s `role` cannot drift into two
    /// spellings of one vocabulary.
    #[test]
    fn r1674_the_trespass_survives_the_wire_with_its_words() {
        let mut hit_border = escape(Fate::Smeared);
        hit_border.trespass = vec![
            Trespass::Border,
            Trespass::Chrome(ChromeRole::Caption),
            Trespass::Outside,
        ];
        let out = report(&[hit_border], 1);
        assert_eq!(
            out.escapes[0].trespass,
            vec![
                Trespass::Border.wire_word(),
                Trespass::Chrome(ChromeRole::Caption).wire_word(),
                Trespass::Outside.wire_word(),
            ],
            "in the order the parts are laid, and spelled by the type",
        );
        assert_eq!(
            out.escapes[0].trespass,
            vec!["border", "chrome:caption", "outside"],
            "and those words are these words — pinned, because they are an \
             AI client's keys",
        );

        // ★ The negative control: two escapes that differ ONLY in which band
        // they hit must not serialize identically. Without this, a `trespass`
        // emitted as a constant would read as a pass.
        let mut on_header = escape(Fate::Smeared);
        on_header.trespass = vec![Trespass::Chrome(ChromeRole::Header)];
        let mut on_caption = escape(Fate::Smeared);
        on_caption.trespass = vec![Trespass::Chrome(ChromeRole::Caption)];
        assert_ne!(
            report(&[on_header], 1).escapes[0].trespass,
            report(&[on_caption], 1).escapes[0].trespass,
            "a mark on the header and a mark on the caption are different facts",
        );
    }

    /// ★ An absent report is not an empty one, and the wire says which.
    #[test]
    fn r1656_an_unavailable_report_is_not_a_clean_screen() {
        let err = handle_scene_containment(None).expect_err("no report installed");
        assert!(
            format!("{err:?}").contains("ContainmentUnavailable"),
            "{err:?}"
        );
        let clean = handle_scene_containment(Some(&report(&[], 0))).expect("empty is an answer");
        assert_eq!(clean["escapes"].as_array().map(Vec::len), Some(0));
        assert_eq!(clean["marks"], 0);
    }

    /// ★ The path is `/`-joined the way every sibling read spells an address,
    /// so a caller can hand it straight to `scene/snapshot`.
    #[test]
    fn r1656_the_path_is_the_address_the_other_reads_accept() {
        let out = report(&[escape(Fate::Smeared)], 1);
        assert_eq!(out.escapes[0].path, "4/0");
    }
}
