//! R1512 / R1514 §5.16 §2 #6 — every renderer observes what the scene declares.
//!
//! §2 #6 is "one scene, two render dispatch paths" (three, with the PDF
//! projector). R1511 found the invariant broken in the direction nothing could
//! see: the vello adapter stroked a `BoxStyle::border` in the `Scene::Box` arm
//! alone, so a border declared on a `Scene::Container` reached the TUI walker
//! and the PDF projector but never the GUI. Nothing failed, because no test
//! compared the renderers — each asserted its own output against its own
//! fixture, which is exactly what a divergence survives.
//!
//! This is that comparison, and it is deliberately BACKEND-NEUTRAL. The three
//! renderers emit incomparable artifacts — a vello `Encoding`, a ratatui cell
//! `Buffer`, a PDF content stream — so the property asserted is not "they draw
//! the same thing" (they cannot) but whether each one OBSERVES a declaration:
//!
//!   render(R, N with the declaration) != render(R, N without it)
//!
//! The R1511 defect is precisely one cell of that matrix — vello × Container ×
//! border — having been EQUAL.
//!
//! # R1514: the comparison must see everything the renderer emits
//!
//! R1512 reduced each renderer to bytes, and two of the three reductions were
//! LOSSY in exactly the place half the declarations live. vello was serialized
//! as path geometry only (`n_paths`, `n_path_segments`, `path_data`) and the
//! TUI as `cell.symbol()` alone — so neither carried a single colour. Both
//! then reported, wrongly, that a solid `fill` and a `gradient` were IGNORED
//! by renderers whose own code paints them (measured: six cells of the matrix
//! were false negatives; only the PDF stream, serialized whole, was right).
//!
//! That is the R1511 silence one level up: had those facets been added with
//! their true observers, this file would have FAILED, and the natural repair —
//! moving vello out of `observers` — would have written *"vello ignores
//! gradients"* into the one file whose purpose is to prevent that sentence.
//!
//! So each reduction now carries the renderer's whole artifact, and the
//! instrument is checked before its verdicts are read (see
//! `r1514_each_reduction_can_see_a_change_that_moves_no_geometry`).
//!
//! # Ignoring is a claim, and so is not-yet
//!
//! Not every renderer must honour every declaration, and a cell that is not
//! observed is one of two very different things:
//!
//! * [`Observation::Ignores`] — the medium cannot carry it. A contract.
//! * [`Observation::Gap`] — the medium CAN carry it and this renderer does
//!   not. A named debt, and the number of them is a number that can go to
//!   zero.
//! * [`Observation::Declarative`] — R1674 — the facet asks for no ink at all,
//!   so there is nothing for any renderer to carry or to owe. Both words above
//!   would be false statements about it, and a false excuse and a false debt
//!   are each worse than the silence they replace.
//!
//! All three assert byte-equality, so a renderer that quietly starts honouring
//! something fails here just as loudly as one that stops. The difference is
//! what the reader is told. R1512 recorded *"the PDF projector paints fill +
//! border only"* as though it were a property of PDF; the projector's own doc
//! files gradients and shadows under "Deferred … additive when a consumer
//! arrives", which is a debt wearing a contract's clothes.
//!
//! # The table is the census, not a hand list
//!
//! `BoxStyle` is `#[non_exhaustive]`: this crate cannot destructure it, so
//! R1512's three-row table could not have known it was covering three facets
//! of five. `pinion_core::style::BoxFacet` is the census the owning crate
//! publishes for exactly this, and the rows below come from an exhaustive
//! `match` on it — a facet added to `BoxStyle` lands as a compile error in
//! `BoxStyle::facets`, and the resulting variant lands as a compile error
//! here, where its observers and reasons must be stated.
//!
//! Determinism is asserted first and is load-bearing: an inequality between
//! two renders means nothing if a renderer is not stable for a fixed scene, so
//! each probe renders the same scene twice and requires the bytes to match
//! before any comparison is trusted.
//!
//! # Why this file lives in `pinion-shell`
//!
//! It has to see all three renderers at once, and no crate did. `pinion-shell`
//! is the cheapest home that can: it already enables `pinion-runtime/vello`
//! unconditionally (so the GPU-free `to_vello` walk is present without adding
//! wgpu anywhere new), and it already hosts the cross-cutting render proofs in
//! `headless_screenshot`. `pinion-tui` and `pinion-pdf` join as dev-dependencies
//! — `pinion-pdf` depends only on `pinion-core`, and nothing depends on
//! `pinion-shell`, so neither creates a cycle.
//!
//! R1512 said a dedicated conformance crate would be the answer once a second
//! contract item landed. Three have, and the answer measured out differently:
//! moving these bytes to another crate closes nothing, because what is
//! unforced is REGISTRATION — `RENDERERS` names three backends, and a fourth
//! is not obliged to appear in any crate. The type system cannot close that
//! one (there is no framework-level declaration of "the set of scene
//! painters"; `pinion_core::external::Backend` answers a different question —
//! it lists Gui/Tui/**Rpc**, and PDF is not in it). What IS closed here: a
//! renderer that does register must be classified for every facet
//! (`r1514_every_declaration_names_every_renderer`), so it cannot join and sit
//! unverified.

use pinion_core::scene::{
    BoxNode, ContainerNode, Rect, Scene, SceneNodeKind, ScrollNode, TextNode,
};
use pinion_core::style::{
    Border, BoxFacet, BoxShadow, BoxStyle, Chrome, Color, Gradient, LayoutStyle, Overflow,
    TextStyle,
};
use pinion_runtime::paint_adapter::to_vello;
use pinion_text::LayoutCache;
use pinion_tui::ratatui::buffer::{Buffer, Cell};
use pinion_tui::ratatui::layout::Rect as TuiRect;
use vello::Scene as VelloScene;

fn rect() -> Rect {
    Rect::new(4, 2, 120, 64)
}
const FILL: Color = Color::rgb(0x20, 0x20, 0x20);
const STROKE: Color = Color::rgb(0xff, 0x30, 0x30);
const BORDER_W: u32 = 3;
/// Why every renderer is right to leave a chrome band unpainted. R1674.
const CHROME_PAINTS_NOTHING: &str = "a chrome band is a reservation, not a mark: \
     it states how much of its own box a painter kept for a caption, a header \
     or a tab strip so `containment::content_of` can subtract it. The painter \
     still draws the title itself, through the ordinary nodes every backend \
     already renders. A renderer that changed its output here would be \
     inventing ink the scene never declared";

/// What one renderer does with one declaration.
enum Observation {
    /// It must observe the declaration.
    Observes,
    /// It is right not to: the medium cannot carry the declaration. The
    /// string says why, and if it cannot be written the cell is a [`Gap`],
    /// not an exception.
    ///
    /// [`Gap`]: Observation::Gap
    Ignores(&'static str),
    /// The medium CAN carry the declaration and this renderer does not yet.
    /// A debt with a name, not a property of the target format.
    Gap(&'static str),
    /// R1674 — the declaration is not about ink at all, so no renderer is
    /// expected to observe it and none is in debt for not doing so.
    ///
    /// The three arms above were written when every facet of a `BoxStyle` was
    /// something to draw, and between them they can only say "cannot" or
    /// "does not yet" — both of which are false statements about a facet that
    /// asks nobody to draw anything. Classifying such a facet as
    /// [`Ignores`](Observation::Ignores) would enter a permanent excuse for
    /// a renderer that is not at fault, and as [`Gap`](Observation::Gap) a
    /// permanent debt nobody can ever pay.
    Declarative(&'static str),
}

impl Observation {
    /// Both non-observing kinds assert the same byte-equality — the
    /// distinction is what the reader is told, not what is checked.
    const fn must_observe(&self) -> bool {
        matches!(self, Self::Observes)
    }

    const fn label(&self) -> &'static str {
        match self {
            Self::Observes => "observes",
            Self::Ignores(_) => "ignores (the medium cannot carry it)",
            Self::Gap(_) => "has a GAP (the medium carries it; this renderer does not)",
            Self::Declarative(_) => "is not asked to observe it (the facet paints nothing)",
        }
    }

    const fn reason(&self) -> &'static str {
        match self {
            Self::Observes => "",
            Self::Ignores(why) | Self::Gap(why) | Self::Declarative(why) => why,
        }
    }
}

/// One facet of a `BoxStyle`, a value that declares it, and the verdict for
/// every renderer.
struct Declaration {
    facet: BoxFacet,
    apply: fn(BoxStyle) -> BoxStyle,
    /// Keyed by renderer name. Every renderer in [`RENDERERS`] appears
    /// exactly once — asserted, so a backend cannot register and then go
    /// unclassified for a facet.
    verdicts: &'static [(&'static str, Observation)],
}

/// The rows, derived from the census. The `match` is exhaustive, so a new
/// [`BoxFacet`] cannot be added without stating here what each renderer does
/// with it.
fn declaration(facet: BoxFacet) -> Declaration {
    let verdicts: &'static [(&'static str, Observation)] = match facet {
        // A colour and an outline are expressible in every medium — a cell
        // background, box-drawing glyphs, a vector fill or stroke — so these
        // two share a row until one of them stops being universal.
        BoxFacet::Fill | BoxFacet::Border => &[
            ("vello", Observation::Observes),
            ("tui", Observation::Observes),
            ("pdf", Observation::Observes),
        ],
        BoxFacet::CornerRadius => &[
            ("vello", Observation::Observes),
            (
                "tui",
                Observation::Gap(
                    "the walker draws the LIGHT box-drawing set and its own \
                     comment rejects `heavy / double / rounded` variants as \
                     mis-matching the single-cell thickness. That holds for \
                     heavy and double, which change weight; the light ARC set \
                     (U+256D..U+2570) is the same weight as the corners \
                     already drawn, so the medium does carry a rounded corner",
                ),
            ),
            ("pdf", Observation::Observes),
        ],
        BoxFacet::Gradient => &[
            ("vello", Observation::Observes),
            (
                "tui",
                Observation::Gap(
                    "the walker already writes a per-cell `bg` colour for the \
                     solid fill, and a ramp sampled once per cell uses that \
                     same mechanism — coarse, but not absent",
                ),
            ),
            (
                "pdf",
                Observation::Gap(
                    "PDF has shading patterns, and the projector's own doc \
                     files gradients under `Deferred (rendered as nothing, a \
                     documented carry — additive when a consumer arrives)` — \
                     its words, and they describe a debt, not the format",
                ),
            ),
        ],
        BoxFacet::Shadows => &[
            ("vello", Observation::Observes),
            (
                "tui",
                Observation::Ignores(
                    "a cell holds one background colour and no sub-cell \
                     alpha, so a blurred penumbra — the whole of what a \
                     `BoxShadow` declares beyond its offset — has nowhere to \
                     land",
                ),
            ),
            (
                "pdf",
                Observation::Gap(
                    "PDF 1.4 soft masks express a blur, and the projector \
                     lists drop-shadows beside gradients under the same \
                     `Deferred … additive when a consumer arrives` carry",
                ),
            ),
        ],
        // R1674 — chrome is a LAYOUT declaration: how much of its own box a
        // painter kept for a caption band, a header or a tab strip. Nothing
        // renders from it in any backend, by design, and byte-identical output
        // is therefore the CORRECT result rather than a shortfall. The rows
        // below say so in the one vocabulary that can say it without slandering
        // a renderer.
        BoxFacet::Chrome => &[
            ("vello", Observation::Declarative(CHROME_PAINTS_NOTHING)),
            ("tui", Observation::Declarative(CHROME_PAINTS_NOTHING)),
            ("pdf", Observation::Declarative(CHROME_PAINTS_NOTHING)),
        ],
    };
    let apply: fn(BoxStyle) -> BoxStyle = match facet {
        BoxFacet::Fill => |s| s.with_fill(Color::rgb(0x00, 0x90, 0xff)),
        BoxFacet::Border => |s| s.with_border(Border::new(STROKE, BORDER_W)),
        BoxFacet::CornerRadius => |s| s.with_corner_radius(12),
        BoxFacet::Gradient => |s| {
            s.with_gradient(
                Gradient::horizontal()
                    .with_stop(0.0, Color::rgb(0xff, 0x00, 0x00))
                    .with_stop(1.0, Color::rgb(0x00, 0x00, 0xff)),
            )
        },
        BoxFacet::Shadows => |s| {
            s.with_shadows(vec![
                BoxShadow::new(Color::rgb(0, 0, 0))
                    .with_offset(0.0, 2.0)
                    .with_blur(4.0),
            ])
        },
        BoxFacet::Chrome => |s| s.with_chrome(Chrome::caption(20)),
    };
    Declaration {
        facet,
        apply,
        verdicts,
    }
}

impl Declaration {
    fn verdict(&self, renderer: &str) -> &Observation {
        self.verdicts
            .iter()
            .find_map(|(name, obs)| (*name == renderer).then_some(obs))
            .unwrap_or_else(|| {
                panic!(
                    "renderer `{renderer}` has no verdict for `{}` — register \
                     it in this facet's row",
                    self.facet.name()
                )
            })
    }
}

/// R1516 — the node axis, from the census.
///
/// This used to be a local `enum NodeKind { Box, Container }` whose doc said
/// "the two node types that carry a `BoxStyle`". `Scene` is
/// `#[non_exhaustive]` and its own header names the variants meant to land
/// later (`Mesh` / `Camera` / `Light`), so that sentence was a claim this
/// crate had no way to check — the same shape as R1512's three-row facet
/// table, one axis over.
///
/// [`SceneNodeKind::carries_box_style`] is the claim now, and this match is
/// exhaustive: a kind added to the census arrives here as a compile error,
/// where the fixture that makes it testable has to be written. `None` marks
/// the kinds that carry no style, cross-checked against the census in
/// [`r1516_the_node_axis_is_the_census`] so a styled kind whose fixture was
/// skipped cannot shrink the matrix in silence.
///
/// The rect and style are the same for every kind, so the ONLY difference
/// between two scenes a renderer sees is which variant carries them. A label
/// child rides along on the container so the TUI walk has ink to place even
/// where a border is absent — without it the no-border container renders a
/// blank buffer and the inequality would be trivially satisfied for the
/// wrong reason.
fn styled_scene(kind: SceneNodeKind, style: BoxStyle) -> Option<Scene> {
    match kind {
        SceneNodeKind::Box => Some(Scene::Box(BoxNode::new(rect(), style))),
        SceneNodeKind::Container => {
            let label = Scene::Text(TextNode::styled(
                "ab".to_string(),
                Rect::new(rect().x + 8, rect().y + 8, 40, 16),
                TextStyle::new().with_fg(Color::rgb(0xf0, 0xf0, 0xf0)),
            ));
            let mut node = ContainerNode::new(vec![label]).with_style(style);
            node.rect = rect();
            Some(Scene::Container(node))
        }
        SceneNodeKind::Text
        | SceneNodeKind::Path
        | SceneNodeKind::Image
        | SceneNodeKind::Effect
        | SceneNodeKind::External
        | SceneNodeKind::Scroll
        | SceneNodeKind::ImmediateModeNode
        | SceneNodeKind::TextGrid => None,
    }
}

/// The kinds the [`BoxFacet`] matrix runs over — the census, filtered by the
/// census's own answer, never by a list kept here.
fn styled_kinds() -> impl Iterator<Item = SceneNodeKind> {
    SceneNodeKind::ALL
        .into_iter()
        .filter(|k| k.carries_box_style())
}

fn fixture(kind: SceneNodeKind, style: BoxStyle) -> Scene {
    styled_scene(kind, style).unwrap_or_else(|| panic!("`{}` carries a BoxStyle", kind.name()))
}

fn declaring(kind: SceneNodeKind, decl: &Declaration) -> Scene {
    fixture(kind, (decl.apply)(BoxStyle::filled(FILL)))
}

fn bare(kind: SceneNodeKind) -> Scene {
    fixture(kind, BoxStyle::filled(FILL))
}

/// One renderer, reduced to "turn a scene into bytes".
struct Renderer {
    name: &'static str,
    render: fn(&Scene) -> Vec<u8>,
}

/// R1514 — the WHOLE encoding, not the path streams alone.
///
/// `path_tags` / `path_data` carry geometry; `draw_tags` / `draw_data` carry
/// the brush (which is where a colour or a gradient ramp lives); `styles`
/// carries fill-vs-stroke and line width; `transforms` carries placement. Any
/// of these missing is a facet this file cannot see, and R1512 shipped with
/// three of the six absent.
fn render_vello(scene: &Scene) -> Vec<u8> {
    let mut out = VelloScene::new();
    let mut cache = LayoutCache::new();
    to_vello(scene, &|_: &BoxNode| None, &mut cache, &mut out);
    let enc = out.encoding();
    let mut bytes = Vec::new();
    for tag in &enc.path_tags {
        bytes.push(tag.0);
    }
    for word in &enc.path_data {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for tag in &enc.draw_tags {
        bytes.extend_from_slice(&tag.0.to_le_bytes());
    }
    for word in &enc.draw_data {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for transform in &enc.transforms {
        for f in transform.matrix {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for f in transform.translation {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    for style in &enc.styles {
        bytes.extend_from_slice(&style.flags_and_miter_limit.to_le_bytes());
        bytes.extend_from_slice(&style.line_width.to_le_bytes());
    }
    for count in [
        enc.n_paths,
        enc.n_path_segments,
        enc.n_clips,
        enc.n_open_clips,
        enc.flags,
    ] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes
}

/// R1514 — the whole cell, not its symbol.
///
/// A `ratatui` cell is a glyph AND its colours; the walker's own doc says it
/// paints a `BoxStyle`'s background fill as the cell `bg`. Serializing
/// `symbol()` alone made every colour-only declaration invisible here.
fn render_tui(scene: &Scene) -> Vec<u8> {
    let mut buf = Buffer::empty(TuiRect::new(0, 0, 60, 20));
    pinion_tui::paint::to_buffer(scene, &mut buf);
    let mut bytes = Vec::new();
    for cell in buf.content() {
        bytes.extend_from_slice(format!("{cell:?}").as_bytes());
        bytes.push(b'|');
    }
    bytes
}

/// The PDF content stream, whole — as it always was. This reduction is the
/// control that showed the other two were lossy: it alone reported `fill` as
/// observed.
fn render_pdf(scene: &Scene) -> Vec<u8> {
    let page = pinion_pdf::PageSize::from_scene_bounds(scene);
    pinion_pdf::render_scene(scene, page).as_bytes().to_vec()
}

const RENDERERS: [Renderer; 3] = [
    Renderer {
        name: "vello",
        render: render_vello,
    },
    Renderer {
        name: "tui",
        render: render_tui,
    },
    Renderer {
        name: "pdf",
        render: render_pdf,
    },
];

/// R1514 — the instrument, before its verdicts.
///
/// Every verdict below is an equality or an inequality between two byte
/// strings, which says nothing unless the bytes carry the property under
/// test. A colour change moves no geometry, so a reduction that keeps only
/// geometry answers "identical" for it — and answers it for every
/// colour-bearing facet, forever, looking exactly like a contract.
///
/// That is not hypothetical: it is what R1512 shipped. This is the check that
/// would have caught it, and it is deliberately about a change NO row in the
/// table needs to be right about — a `TextStyle` colour, which is not a
/// `BoxFacet` at all — so the guard cannot be satisfied by the same code path
/// the rows exercise.
#[test]
fn r1514_each_reduction_can_see_a_change_that_moves_no_geometry() {
    let text = |fg: Color| {
        Scene::Text(TextNode::styled(
            "ab".to_string(),
            Rect::new(8, 8, 40, 16),
            TextStyle::new().with_fg(fg),
        ))
    };
    let dark = text(Color::rgb(0x10, 0x10, 0x10));
    let light = text(Color::rgb(0xf0, 0xf0, 0xf0));

    for renderer in &RENDERERS {
        assert_ne!(
            (renderer.render)(&dark),
            (renderer.render)(&light),
            "{}'s reduction cannot distinguish two colours at identical \
             geometry, so every equality it reports below is uninformative — \
             serialize more of the artifact",
            renderer.name
        );
    }
}

/// Every registered renderer is classified for every facet. Registration
/// itself cannot be forced (see the module doc), but joining `RENDERERS` and
/// then having no verdict can be, and this is that.
#[test]
fn r1514_every_declaration_names_every_renderer() {
    for facet in BoxFacet::ALL {
        let decl = declaration(facet);
        assert_eq!(
            decl.verdicts.len(),
            RENDERERS.len(),
            "`{}` states a verdict for each of the {} registered renderers",
            facet.name(),
            RENDERERS.len()
        );
        for renderer in &RENDERERS {
            let verdict = decl.verdict(renderer.name);
            if !verdict.must_observe() {
                assert!(
                    !verdict.reason().is_empty(),
                    "{} does not observe `{}`, so the table owes a reason — \
                     an unexplained exception is how the R1511 silence began",
                    renderer.name,
                    facet.name()
                );
            }
        }
    }
}

#[test]
fn r1512_every_renderer_answers_each_declaration_the_same_way_for_every_styled_node() {
    for facet in BoxFacet::ALL {
        let decl = declaration(facet);
        for renderer in &RENDERERS {
            let verdict = decl.verdict(renderer.name);
            for kind in styled_kinds() {
                let with = declaring(kind, &decl);
                let without = bare(kind);

                // Determinism first: an inequality between two renders is
                // evidence only if the renderer is stable for a fixed scene.
                let a = (renderer.render)(&with);
                let b = (renderer.render)(&with);
                assert_eq!(
                    a,
                    b,
                    "{} is not deterministic for a Scene::{}; every \
                     comparison below would be noise",
                    renderer.name,
                    kind.name()
                );

                let undeclared = (renderer.render)(&without);
                if verdict.must_observe() {
                    assert_ne!(
                        a,
                        undeclared,
                        "{} ignores `{}` declared on a Scene::{} — the §2 #6 \
                         divergence R1511 found, in this cell",
                        renderer.name,
                        facet.name(),
                        kind.name()
                    );
                } else {
                    assert_eq!(
                        a,
                        undeclared,
                        "{} started honouring `{}` on a Scene::{}. That may \
                         well be an improvement — the table records that it \
                         {} ({}) — but an unstated change of medium is how \
                         the R1511 silence began. Move it to `Observes`.",
                        renderer.name,
                        facet.name(),
                        kind.name(),
                        verdict.label(),
                        verdict.reason()
                    );
                }
            }
        }
    }
}

/// The matrix above says each renderer *observes* the declaration. This says
/// what each one does with it, in that renderer's own vocabulary, so a failure
/// names the missing artifact instead of only reporting that two blobs matched.
///
/// These are deliberately the WEAKEST claims that are still specific — an extra
/// vello path, a box-drawing glyph, a PDF stroke operator. Anything tighter
/// (which path, which glyph, what stroke width) belongs in each renderer's own
/// suite, where the geometry is already pinned; duplicating it here would make
/// this file a second, competing source of truth for three backends at once.
#[test]
fn r1512_each_renderer_draws_a_border_in_its_own_vocabulary() {
    let border = declaration(BoxFacet::Border);
    for kind in styled_kinds() {
        let with = declaring(kind, &border);
        let without = bare(kind);

        // vello: the stroke is one more encoded path than the bare scene.
        let paths = |s: &Scene| -> u32 {
            let mut out = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(s, &|_: &BoxNode| None, &mut cache, &mut out);
            out.encoding().n_paths
        };
        assert_eq!(
            paths(&with),
            paths(&without) + 1,
            "vello encodes exactly one extra path — the stroke — for a \
             bordered Scene::{}",
            kind.name()
        );

        // TUI: cells are discrete, so the border is box-drawing glyphs.
        let tui_with = String::from_utf8(render_tui(&with)).expect("tui cells are utf-8");
        let tui_without = String::from_utf8(render_tui(&without)).expect("tui cells are utf-8");
        let corners = ['\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}'];
        assert!(
            corners.iter().all(|c| tui_with.contains(*c)),
            "the TUI walk draws all four box-drawing corners for a bordered \
             Scene::{}",
            kind.name()
        );
        assert!(
            !corners.iter().any(|c| tui_without.contains(*c)),
            "and none of them without the declaration (Scene::{})",
            kind.name()
        );

        // PDF: a stroke is the `S` operator, which the bare scene never emits.
        let pdf_with = render_pdf(&with);
        let pdf_without = render_pdf(&without);
        assert!(
            pdf_with.len() > pdf_without.len(),
            "the PDF projector emits more content for a bordered Scene::{}",
            kind.name()
        );
    }
}

// ---------------------------------------------------------------------------
// R1516 §5.45 — the clip axis
// ---------------------------------------------------------------------------

/// Content big enough to overflow [`NARROW`], in the content-intrinsic frame
/// a scroll node paints its child in.
fn clipped_content() -> Scene {
    Scene::Box(BoxNode::new(
        Rect::new(0, 0, 100, 50),
        BoxStyle::filled(STROKE),
    ))
}

/// A viewport that hides most of [`clipped_content`], and one that hides
/// none of it. Same origin, so the content lands in the same place under
/// both and the only difference a renderer sees is how much is visible — a
/// renderer that dropped the clip would emit identical bytes for the pair.
const NARROW: Rect = Rect::new(4, 2, 12, 8);
const WIDE: Rect = Rect::new(4, 2, 200, 120);

/// ★★ R1685 — the clip fixtures are rendered inside a box of CONSTANT size,
/// and without it one third of the clip matrix was vacuous.
///
/// The pair differs in the clipping node's own extent, so the SCENE BOUNDS
/// differ too — and the PDF projector derives its page size from
/// `PageSize::from_scene_bounds`. Its two artifacts therefore differed in the
/// page header whatever it did with the clip, and the inequality below was
/// satisfied by paper size. Measured by the R1685 counterfactual: deleting the
/// projector's clip entirely left this file green.
///
/// That is the exact failure this file was written for, one level down — an
/// assertion that cannot fail reads as a contract. The frame pins the bounds so
/// the only thing left that can move the bytes is the clip.
fn framed(inner: Scene) -> Scene {
    let mut outer = ContainerNode::new(vec![inner]);
    outer.rect = Rect::new(0, 0, 240, 140);
    Scene::Container(outer)
}

/// R1516 — the clip axis, from the same census as the styled-node axis.
///
/// A [`BoxFacet`] is ADDITIVE: declaring it adds ink, so "the renderer
/// observed it" reads as an inequality against the scene without it. A clip
/// is subtractive, and the two are not mirror images — measured, vello and
/// the PDF projector both ENCODE the hidden content and leave the dropping
/// to the rasteriser, so no reduction of their pre-raster artifact can show
/// absence of ink. Only the TUI walk culls at walk time. What all three must
/// do is carry the clip itself, and that is what the matrix below asserts;
/// what each does with it is the vocabulary test after it.
///
/// There is no [`Observation::Ignores`] here. A medium may genuinely be
/// unable to carry a blurred penumbra; painting content the scene declares
/// hidden is a §2 #6 divergence in any medium, so the clip row has no
/// exceptions to state.
fn clipping_scene(kind: SceneNodeKind, viewport: Rect) -> Option<Scene> {
    match kind {
        SceneNodeKind::Scroll => Some(Scene::Scroll(ScrollNode::new(viewport, clipped_content()))),
        // ★ R1685 — the second clipping kind. A container clips when it
        // declares it, so the fixture declares it: same content, same origin,
        // same pair of windows, and the only difference from the scroll above
        // is which node carries the window. A renderer that learned the clip as
        // "what a Scroll does" rather than as "what a node declares" passes the
        // row above and fails this one.
        SceneNodeKind::Container => {
            let mut node = ContainerNode::new(vec![clipped_content()])
                .with_layout(LayoutStyle::new().with_overflow(Overflow::Hidden));
            node.rect = viewport;
            Some(Scene::Container(node))
        }
        SceneNodeKind::Box
        | SceneNodeKind::Text
        | SceneNodeKind::Path
        | SceneNodeKind::Image
        | SceneNodeKind::Effect
        | SceneNodeKind::External
        | SceneNodeKind::ImmediateModeNode
        | SceneNodeKind::TextGrid => None,
    }
}

/// Both node axes are the census's answer, and a fixture is what makes an
/// answer testable. If the two disagree — a kind the census calls styled or
/// clipping that no fixture builds — the matrices above run over a quietly
/// smaller set, which is the R1511 silence with the census in place to have
/// prevented it.
#[test]
fn r1516_the_node_axis_is_the_census() {
    let mut styled = 0;
    let mut clipping = 0;
    for kind in SceneNodeKind::ALL {
        assert_eq!(
            styled_scene(kind, BoxStyle::filled(FILL)).is_some(),
            kind.carries_box_style(),
            "the census says Scene::{} carries a BoxStyle = {}; this file's \
             fixtures say otherwise",
            kind.name(),
            kind.carries_box_style()
        );
        assert_eq!(
            clipping_scene(kind, NARROW).is_some(),
            kind.can_clip_subtree(),
            "the census says Scene::{} can clip its subtree = {}; this file's \
             fixtures say otherwise",
            kind.name(),
            kind.can_clip_subtree()
        );
        styled += usize::from(kind.carries_box_style());
        clipping += usize::from(kind.can_clip_subtree());
        // ★ R1685 — the kind census answers "can", and a kind that can must
        // hold children, because a clip is something a node does to a subtree.
        // Asserted against `child_nodes` rather than restated, so the day a
        // kind gains children this file says whether it can clip them.
        let fixture = clipping_scene(kind, NARROW);
        assert_eq!(
            kind.can_clip_subtree(),
            fixture
                .as_ref()
                .is_some_and(|s| !s.child_nodes().is_empty()),
            "Scene::{} says it can clip a subtree = {}, but the fixture it \
             builds {} children to clip",
            kind.name(),
            kind.can_clip_subtree(),
            if kind.can_clip_subtree() {
                "has no"
            } else {
                "has"
            }
        );
    }
    // The axes are non-empty: a census filtered down to nothing would let
    // every matrix below pass by iterating zero cells.
    assert!(styled > 0, "the styled-node axis has members");
    assert!(clipping > 0, "the clip axis has members");
}

/// Every renderer carries the clip declaration into its artifact.
#[test]
fn r1516_every_renderer_carries_the_clip_declaration() {
    for kind in SceneNodeKind::ALL
        .into_iter()
        .filter(|k| k.can_clip_subtree())
    {
        let narrow = clipping_scene(kind, NARROW).expect("the census says this kind clips");
        let wide = clipping_scene(kind, WIDE).expect("the census says this kind clips");
        // ★ R1685 — the fixture must actually be clipping. `can_clip_subtree`
        // is the kind's answer; a container that forgot to declare
        // `Overflow::Hidden` would render narrow and wide differently anyway
        // (its own box is a different size) and the row below would pass
        // without a clip ever existing.
        assert!(
            narrow.clips_subtree() && wide.clips_subtree(),
            "the Scene::{} fixture does not declare a clip, so the rows below \
             would compare two unclipped scenes",
            kind.name()
        );
        // Rendered inside a constant frame — see `framed` for the third of this
        // matrix that was answering with its page size.
        let (narrow, wide) = (framed(narrow), framed(wide));
        for renderer in &RENDERERS {
            // Determinism first, for the reason the facet matrix asserts it.
            let a = (renderer.render)(&narrow);
            let b = (renderer.render)(&narrow);
            assert_eq!(
                a,
                b,
                "{} is not deterministic for a Scene::{}; the comparison \
                 below would be noise",
                renderer.name,
                kind.name()
            );
            assert_ne!(
                a,
                (renderer.render)(&wide),
                "{} renders a Scene::{} that hides its content and one that \
                 hides none of it identically — the viewport never reached \
                 the artifact, so whatever the rasteriser does with it is \
                 not this scene's clip",
                renderer.name,
                kind.name()
            );
        }
    }
}

/// What each renderer does with the clip, in its own vocabulary, against a
/// control that declares no clip at all — so a failure names the missing
/// artifact instead of reporting that two blobs differed.
#[test]
fn r1516_each_renderer_clips_in_its_own_vocabulary() {
    let unclipped = Scene::Container(ContainerNode::new(vec![clipped_content()]));
    let clipped = clipping_scene(SceneNodeKind::Scroll, NARROW).expect("Scroll clips");

    // vello: a clip layer, which the encoding counts.
    let clips = |s: &Scene| -> u32 {
        let mut out = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(s, &|_: &BoxNode| None, &mut cache, &mut out);
        out.encoding().n_clips
    };
    assert_eq!(
        clips(&unclipped),
        0,
        "no clip layer without a clipping node"
    );
    assert!(
        clips(&clipped) > 0,
        "vello pushes a clip layer for the scroll viewport"
    );

    // TUI: cells are the artifact, so the clip is a cull — strictly fewer
    // of them carry ink once the viewport hides most of the content.
    let ink = |s: &Scene| -> usize {
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 60, 20));
        pinion_tui::paint::to_buffer(s, &mut buf);
        buf.content()
            .iter()
            .filter(|c| **c != Cell::default())
            .count()
    };
    let unclipped_ink = ink(&unclipped);
    assert!(unclipped_ink > 0, "the content paints cells when unclipped");
    assert!(
        ink(&clipped) < unclipped_ink,
        "the TUI walk culls the cells the viewport hides ({} clipped vs {} \
         unclipped)",
        ink(&clipped),
        unclipped_ink
    );

    // PDF: `re W n` — build a rect path, intersect the clip, no paint.
    let pdf = |s: &Scene| String::from_utf8(render_pdf(s)).expect("the stream is utf-8");
    assert!(
        pdf(&clipped).contains("re W n"),
        "the PDF projector emits the clip operator for the scroll viewport"
    );
    assert!(
        !pdf(&unclipped).contains("re W n"),
        "and none without a clipping node"
    );
}
