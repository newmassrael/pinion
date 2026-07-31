//! R1512 §5.16 §2 #6 — every renderer observes what the scene declares.
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
//! the same thing" (they cannot) but the one thing they must all do:
//!
//!   for every renderer R and every node type N,
//!     render(R, N with a declared border) != render(R, N without one)
//!
//! The R1511 defect is precisely one cell of that matrix — vello × Container —
//! having been EQUAL. A future edit that drops the declaration in any renderer,
//! for either node type, fails here rather than in nothing.
//!
//! Determinism is asserted first and is load-bearing: an inequality between two
//! renders means nothing if a renderer is not stable for a fixed scene, so each
//! probe renders the same scene twice and requires the bytes to match before
//! any comparison is trusted.
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
//! A crate whose only purpose is cross-renderer conformance would be the other
//! answer. One contract item does not justify it; when a second lands here
//! (corner radius, shadows, clip — each is the same shape) that is the signal.

use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, TextStyle};
use pinion_runtime::paint_adapter::to_vello;
use pinion_text::LayoutCache;
use pinion_tui::ratatui::buffer::Buffer;
use pinion_tui::ratatui::layout::Rect as TuiRect;
use vello::Scene as VelloScene;

fn rect() -> Rect {
    Rect::new(4, 2, 120, 64)
}
const FILL: Color = Color::rgb(0x20, 0x20, 0x20);
const STROKE: Color = Color::rgb(0xff, 0x30, 0x30);
const BORDER_W: u32 = 3;

/// The two node types that carry a `BoxStyle`. Both must observe it.
#[derive(Clone, Copy, Debug)]
enum NodeKind {
    Box,
    Container,
}

impl NodeKind {
    const ALL: [Self; 2] = [Self::Box, Self::Container];

    /// The same rect and style either way, so the ONLY difference between the
    /// two scenes a renderer sees is which variant carries them. A label child
    /// rides along on the container so the TUI walk has ink to place even where
    /// a border is absent — without it the no-border container renders a blank
    /// buffer and the inequality would be trivially satisfied for the wrong
    /// reason.
    fn scene(self, border: Option<Border>) -> Scene {
        let mut style = BoxStyle::filled(FILL);
        if let Some(b) = border {
            style = style.with_border(b);
        }
        match self {
            Self::Box => Scene::Box(BoxNode::new(rect(), style)),
            Self::Container => {
                let label = Scene::Text(TextNode::styled(
                    "ab".to_string(),
                    Rect::new(rect().x + 8, rect().y + 8, 40, 16),
                    TextStyle::new().with_fg(Color::rgb(0xf0, 0xf0, 0xf0)),
                ));
                let mut node = ContainerNode::new(vec![label]).with_style(style);
                node.rect = rect();
                Scene::Container(node)
            }
        }
    }
}

/// One renderer, reduced to "turn a scene into bytes".
struct Renderer {
    name: &'static str,
    render: fn(&Scene) -> Vec<u8>,
}

fn render_vello(scene: &Scene) -> Vec<u8> {
    let mut out = VelloScene::new();
    let mut cache = LayoutCache::new();
    to_vello(scene, &|_: &BoxNode| None, &mut cache, &mut out);
    let enc = out.encoding();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&enc.n_paths.to_le_bytes());
    bytes.extend_from_slice(&enc.n_path_segments.to_le_bytes());
    for word in &enc.path_data {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn render_tui(scene: &Scene) -> Vec<u8> {
    let mut buf = Buffer::empty(TuiRect::new(0, 0, 60, 20));
    pinion_tui::paint::to_buffer(scene, &mut buf);
    let mut bytes = Vec::new();
    for cell in buf.content() {
        bytes.extend_from_slice(cell.symbol().as_bytes());
        bytes.push(b'|');
    }
    bytes
}

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

#[test]
fn r1512_every_renderer_observes_a_declared_border() {
    let border = Border::new(STROKE, BORDER_W);
    for renderer in &RENDERERS {
        for kind in NodeKind::ALL {
            let with = kind.scene(Some(border));
            let without = kind.scene(None);

            // Determinism first: an inequality between two renders is evidence
            // only if the renderer is stable for a fixed scene.
            let a = (renderer.render)(&with);
            let b = (renderer.render)(&with);
            assert_eq!(
                a, b,
                "{} is not deterministic for {kind:?}; every comparison below \
                 would be noise",
                renderer.name
            );

            let bare = (renderer.render)(&without);
            assert_ne!(
                a, bare,
                "{} ignores the border declared on a Scene::{kind:?} — the \
                 §2 #6 divergence R1511 found, in the cell for this renderer \
                 and node type",
                renderer.name
            );
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
    let border = Border::new(STROKE, BORDER_W);
    for kind in NodeKind::ALL {
        let with = kind.scene(Some(border));
        let without = kind.scene(None);

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
             bordered Scene::{kind:?}"
        );

        // TUI: cells are discrete, so the border is box-drawing glyphs.
        let tui_with = String::from_utf8(render_tui(&with)).expect("tui cells are utf-8");
        let tui_without = String::from_utf8(render_tui(&without)).expect("tui cells are utf-8");
        let corners = ['\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}'];
        assert!(
            corners.iter().all(|c| tui_with.contains(*c)),
            "the TUI walk draws all four box-drawing corners for a bordered \
             Scene::{kind:?}"
        );
        assert!(
            !corners.iter().any(|c| tui_without.contains(*c)),
            "and none of them without the declaration (Scene::{kind:?})"
        );

        // PDF: a stroke is the `S` operator, which the bare scene never emits.
        let pdf_with = render_pdf(&with);
        let pdf_without = render_pdf(&without);
        assert!(
            pdf_with.len() > pdf_without.len(),
            "the PDF projector emits more content for a bordered \
             Scene::{kind:?}"
        );
    }
}
