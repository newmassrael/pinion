//! (R1557 §5.16 §5.18 §2 #2 §2 #7) Per-subtree attribution of a frame's draw
//! work — which part of the scene the picture cost, not just how much it cost.
//!
//! # The gap this closes
//!
//! R1538 made the frame's node census readable and R1556 made its **draw**
//! census readable — how many draw commands, paths, path segments, clip layers,
//! glyph runs and glyphs the encoded scene asks the renderer for. Both are one
//! number per frame, so both answer *how much* and neither answers *where*. A
//! frame that costs four thousand glyphs and a frame that costs four thousand
//! glyphs somewhere else are the same reading, and the fix for them is not the
//! same fix.
//!
//! That is the shape of every profiler's central artifact: a tree of frames,
//! each with an **inclusive** cost (itself and everything under it) and an
//! **exclusive** cost (itself alone). A flame graph is that tree drawn
//! sideways.
//!
//! # How the attribution is taken
//!
//! Not by a tally the walker keeps, and not by re-deriving what each node
//! *ought* to encode. The encoded streams only ever grow during a paint, so a
//! [`DrawWork`] census of the output taken **before** a subtree is walked and
//! one taken **after** differ by exactly that subtree's contribution
//! ([`DrawWork::since`]). The profiler is handed those two censuses and
//! subtracts.
//!
//! Two properties follow, and they are the reason for the choice:
//!
//! - A subtree the §5.16 fragment cache **replayed** is attributed exactly like
//!   one encoded fresh. A cache hit appends a stored fragment, `vello`'s append
//!   folds the appended counters in, and the difference sees it. A walker-side
//!   tally would attribute a replayed subtree **zero**, which is the reading a
//!   profiler must never produce: the GPU draws it either way.
//! - Nothing a node draws can escape its own attribution, because the
//!   measurement is the size of the artifact rather than a count of the calls
//!   that were meant to produce it.
//!
//! `own` is then `total` minus the sum of the children's totals — arithmetic,
//! not a second measurement, so the two cannot disagree.
//!
//! # The balance identity
//!
//! Summing `own` over every node in the tree gives the root's `total`, exactly.
//! It is the property that makes the attribution a *partition* of the frame's
//! draw work rather than a set of overlapping estimates, and it is asserted —
//! in this module's tests, and again over the wire by `scene/draw_profile`'s
//! demo. A saturating subtraction that ever clamped, an arm of the paint walk
//! that opened a frame without closing it, or a node whose work landed outside
//! the span being measured would each break it.
//!
//! # Rooting the walk somewhere else (R1558)
//!
//! Nothing here needs to know whether the [`Scene`] it is handed is a window's
//! root or a subtree of one: the walk starts where it is started, and the
//! census difference measures whatever it encodes. That is what lets
//! `scene/draw_profile` scope the **measurement** to an address instead of
//! merely trimming the reply — the caller resolves the address, hands the
//! resulting node to the profiled walk, and states the chain it resolved as
//! [`DrawProfile::scope`].
//!
//! The reason the two agree — a scoped profile equalling the corresponding
//! subtree of the whole one — is a property of the encoder, not a convention
//! of this module. A subtree's [`DrawWork`] is **independent of the context it
//! is drawn in**: the inherited transform is applied to coordinates and changes
//! no count (R1520 already relies on this, encoding every cached fragment at
//! `IDENTITY` and re-placing it), an ancestor's clip is an input to *damage*
//! reporting rather than a cull, and the clip layer a [`Scene::Scroll`] pushes
//! is charged to the scroll's own row. So the counts compose, and the
//! equality is asserted rather than assumed — over a real encode in
//! `paint_adapter`'s tests, and again over the wire.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit's floor for per-item render cost is **`QSG_RENDERER_DEBUG=render`**: an environment
//! variable that makes the toolkit Quick scene-graph renderer dump its batches
//! to `stderr` as prose, with raw `SG geometry node*` pointers for identity. quick3 D render stats
//! publishes `drawCallCount` / `drawVertexCount` / `renderPassCount` as bindable properties, but they are whole-frame
//! and 3D-only. On the widget stack — widget, painter, canvas view — there is
//! no per-item draw-work accounting of any kind.
//!
//! Four things here are past that floor:
//!
//! - **It is queryable, not printed.** `scene/draw_profile` answers a request
//!   with structured data. The toolkit's is `stderr` text behind an env var, which a
//!   running application cannot ask itself for.
//! - **A node's identity is its address.** Every row carries the
//!   `/window[main]/a/b` path `scene/locate` produces and
//!   [`Scene::lookup_path_ref`](pinion_core::Scene::lookup_path_ref) — the
//!   resolver behind `scene/query` and `scene/invoke` — consumes, derived
//!   through the one
//!   [`Scene::path_segment_at`](pinion_core::Scene::path_segment_at) rule the
//!   hit-test uses. "Which subtree is expensive" and "act on that subtree" are
//!   the same string, and the demo resolves a profile row's path through
//!   `scene/query` to prove the two derivations agree. The toolkit hands back a
//!   `SG geometry node*`.
//! - **It is deterministic, not sampled.** Tracy and the engine Insights attribute
//!   by interrupting a clock, so two runs of the same frame disagree and a
//!   cheap-but-frequent node can vanish between samples. Every number here is a
//!   count read off the artifact, so the same scene profiles identically on
//!   every host — which is also what lets a CI guard assert on it.
//! - **Text is attributed.** No the toolkit surface reports a per-item glyph count
//!   anywhere. R1531 measured the glyph-run walk at 37% of a warm frame, so on
//!   a professional 2D application this is the term that decides the answer.
//!
//! # What this does not state
//!
//! A **cost** to multiply the counts by, for [`DrawWork`]'s stated reason: what
//! a glyph or a path segment costs moves with the GPU, the driver and the
//! resolution. So there is deliberately no single "which subtree is heaviest"
//! scalar — ranking happens by a **named** unit the caller chooses, never by a
//! weight this crate invented.
//!
//! Whether a subtree was **replayed or re-encoded** is likewise not here. That
//! is the encode axis, which `FrameTiming::encode_nodes` and `scene/cache_stats`
//! answer per frame; this module is about what the renderer was asked to draw.

use crate::frame_timing::DrawWork;
use pinion_core::Scene;
use pinion_core::scene::SceneNodeKind;

/// (R1557 §5.16) One node of a [`DrawProfile`] — a scene node, what it and its
/// subtree drew, and what it drew by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawProfileNode {
    /// The container-relative path segment addressing this node within its
    /// parent — [`Scene::path_segment_at`](pinion_core::Scene::path_segment_at),
    /// so a tag when the node has one and its child index otherwise.
    ///
    /// `None` for a node that consumes **no** path segment: the profiled root,
    /// and a [`Scene::Scroll`]'s content. That is not a special case invented
    /// here — it is the addressing rule `scene/locate` and `Scene::hit_test`
    /// already follow, under which a scroll's content is reached at the
    /// scroll's own address. Keeping the exception makes a profile row's path
    /// resolvable by every other method; removing it would make this the one
    /// surface whose paths do not.
    pub segment: Option<String>,
    /// Which [`Scene`] variant this node is.
    pub kind: SceneNodeKind,
    /// The node's §5.20 tag, when it carries one. Redundant with
    /// [`Self::segment`] for a tagged node and the only name an untagged one
    /// has (`None`) — published separately so a consumer can tell an
    /// index-addressed node from a node whose tag happens to be numeric.
    pub tag: Option<String>,
    /// **Inclusive** draw work: this node and its whole subtree.
    pub total: DrawWork,
    /// **Exclusive** draw work: `total` minus the sum of the children's
    /// totals — what this node contributed by itself.
    ///
    /// A `Container`'s own work is its background, border and corner radii; a
    /// `Scroll`'s is the clip layer it pushes; a leaf's own work is all of it.
    pub own: DrawWork,
    /// Children, in paint order (later siblings draw on top).
    pub children: Vec<DrawProfileNode>,
}

impl DrawProfileNode {
    /// How many nodes this subtree holds, itself included.
    #[must_use]
    pub fn node_count(&self) -> u32 {
        self.children
            .iter()
            .fold(1_u32, |acc, c| acc.saturating_add(c.node_count()))
    }

    /// The per-field sum of every node's [`own`](Self::own) in this subtree —
    /// which equals [`total`](Self::total) exactly when the attribution is a
    /// partition.
    ///
    /// The balance identity this module's header names, computed rather than
    /// asserted so a caller (a test, the wire's demo) can compare the two.
    #[must_use]
    pub fn own_sum(&self) -> DrawWork {
        self.children
            .iter()
            .fold(self.own, |acc, c| acc.plus(c.own_sum()))
    }
}

/// (R1557 §5.16) One profiled paint: the attributed tree, or nothing when the
/// paint encoded no node at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawProfile {
    /// The profiled scene's root. `None` only when [`DrawProfiler::finish`] was
    /// called on a profiler no walk ever entered — a host with no window, not a
    /// blank frame (a painted scene has a root, so a real profile has one too).
    pub root: Option<DrawProfileNode>,
    /// (R1558) The segment chain from the window's scene root to the node this
    /// profile was **rooted at** — empty for a whole-window profile.
    ///
    /// It is what turns every row's segment chain back into the absolute
    /// `/window[id]/…` address the rest of the API speaks, so a scoped
    /// profile's rows address the same nodes an unscoped profile's rows do.
    ///
    /// Recorded here rather than taken from the request, because the two are
    /// not always the same chain: the addressing vocabulary lets a scene root
    /// be named by its own tag (`resolve::lookup_addressed`'s root alias), and
    /// a profile rooted that way sits at the **empty** chain. Seeding the
    /// projection from what was asked for would then address every row one
    /// segment too deep.
    pub scope: Vec<String>,
}

/// (R1557 §5.16) Builder the paint walk drives to produce a [`DrawProfile`].
///
/// # Contract
///
/// [`enter`](Self::enter) and [`leave`](Self::leave) are paired, one pair per
/// walked node, `leave` receiving a census of the **same** output scene the
/// matching `enter` was given one of. `paint_adapter`'s cached walk satisfies
/// this by construction: one wrapper opens the frame, delegates the whole node
/// — every arm, every early return — to the body, and closes the frame on the
/// single path back out. There is no arm that can forget.
///
/// An unpaired `leave` is a no-op rather than a panic. A profiler is
/// instrumentation, and instrumentation that can abort the frame it measures is
/// worse than instrumentation that reports a short tree — which the balance
/// identity then catches.
#[derive(Debug, Default)]
pub struct DrawProfiler {
    /// Nodes entered and not yet left, outermost first.
    open: Vec<OpenFrame>,
    /// The finished root, set when the outermost frame closes.
    root: Option<DrawProfileNode>,
}

/// A node the walk is currently inside.
#[derive(Debug)]
struct OpenFrame {
    segment: Option<String>,
    kind: SceneNodeKind,
    tag: Option<String>,
    /// The output scene's census at the moment this node was entered.
    before: DrawWork,
    /// Children closed so far.
    children: Vec<DrawProfileNode>,
    /// Their totals, summed — the subtrahend that turns `total` into `own`.
    children_total: DrawWork,
}

impl DrawProfiler {
    /// A profiler with nothing open and nothing recorded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a frame for `scene`, entered as the child at `index` of its parent.
    ///
    /// `index` is `None` for a node that consumes no path segment — see
    /// [`DrawProfileNode::segment`]. `before` is the output scene's [`DrawWork`]
    /// census at this instant.
    ///
    /// The segment `String` is allocated here and only here, which is why the
    /// walk passes an `index` rather than a formatted segment: a paint that is
    /// not being profiled must not pay for a name nobody asked for.
    pub fn enter(&mut self, scene: &Scene, index: Option<usize>, before: DrawWork) {
        self.open.push(OpenFrame {
            segment: index.map(|i| scene.path_segment_at(i)),
            kind: scene.node_kind(),
            tag: scene.tag().map(ToOwned::to_owned),
            before,
            children: Vec::new(),
            children_total: DrawWork::default(),
        });
    }

    /// Close the innermost open frame. `after` is the output scene's
    /// [`DrawWork`] census now, so `after - before` is what this node's subtree
    /// contributed.
    pub fn leave(&mut self, after: DrawWork) {
        let Some(frame) = self.open.pop() else {
            return;
        };
        let total = after.since(frame.before);
        let node = DrawProfileNode {
            segment: frame.segment,
            kind: frame.kind,
            tag: frame.tag,
            total,
            own: total.since(frame.children_total),
            children: frame.children,
        };
        match self.open.last_mut() {
            Some(parent) => {
                parent.children_total = parent.children_total.plus(node.total);
                parent.children.push(node);
            }
            // The outermost frame closed: this is the profiled root. A second
            // outermost close would be a second root; the last one wins, which
            // is the only answer that keeps the tree consistent with the walk
            // that produced it.
            None => self.root = Some(node),
        }
    }

    /// The finished profile, rooted at `scope` — the segment chain from the
    /// window's scene root to the node this walk started at, empty when it
    /// started at the root itself. Any frame still open is dropped unclosed —
    /// see the type's contract.
    ///
    /// The scope is a **parameter** rather than a field the producer sets
    /// afterwards: a profile whose rows cannot be addressed is not a profile,
    /// and the one caller who knows which subtree was walked is the one who
    /// chose it. Passing it here means there is no order of calls in which it
    /// is forgotten.
    #[must_use]
    pub fn finish(self, scope: Vec<String>) -> DrawProfile {
        DrawProfile {
            root: self.root,
            scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DrawProfile, DrawProfileNode, DrawProfiler};
    use crate::frame_timing::DrawWork;
    use pinion_core::Scene;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, SceneNodeKind};
    use pinion_core::style::Color;

    /// A census with `draws` set — the one field these unit tests vary, so the
    /// arithmetic under test is legible. `paint_adapter`'s own tests exercise
    /// all six against a real encode.
    fn work(draws: u32) -> DrawWork {
        DrawWork {
            draws,
            ..DrawWork::default()
        }
    }

    fn untagged_box() -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(0, 0, 1, 1), Color::default()))
    }

    fn tagged_box(tag: &'static str) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(0, 0, 1, 1), Color::default()).with_tag(tag))
    }

    fn container(children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children))
    }

    #[test]
    fn r1557_own_is_total_minus_children() {
        // root [0 .. 100] with one child [10 .. 60]: the child's subtree is 50
        // and the root drew 50 by itself.
        let root = container(vec![tagged_box("kid")]);
        let mut p = DrawProfiler::new();
        p.enter(&root, None, work(0));
        p.enter(&tagged_box("kid"), Some(0), work(10));
        p.leave(work(60));
        p.leave(work(100));

        let node = p
            .finish(Vec::new())
            .root
            .expect("a closed outermost frame is the root");
        assert_eq!(node.total, work(100));
        assert_eq!(node.own, work(50));
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].total, work(50));
        // A leaf's own work is all of it.
        assert_eq!(node.children[0].own, work(50));
    }

    #[test]
    fn r1557_own_sums_to_the_root_total() {
        // Three levels, two siblings at the bottom, every node contributing.
        let leaf = tagged_box("leaf");
        let mid = container(vec![tagged_box("leaf"), tagged_box("leaf")]);
        let root = container(vec![tagged_box("mid")]);
        let mut p = DrawProfiler::new();
        p.enter(&root, None, work(0));
        p.enter(&mid, Some(0), work(3));
        p.enter(&leaf, Some(0), work(5));
        p.leave(work(9));
        p.enter(&leaf, Some(1), work(9));
        p.leave(work(20));
        p.leave(work(25));
        p.leave(work(31));

        let node = p.finish(Vec::new()).root.expect("root");
        assert_eq!(node.total, work(31));
        // The identity the module header names: the attribution is a partition.
        assert_eq!(node.own_sum(), node.total);
        assert_eq!(node.node_count(), 4);
    }

    #[test]
    fn r1557_segment_is_the_tag_or_the_index() {
        let root = container(vec![untagged_box(), tagged_box("named")]);
        let mut p = DrawProfiler::new();
        p.enter(&root, None, work(0));
        p.enter(&untagged_box(), Some(0), work(0));
        p.leave(work(1));
        p.enter(&tagged_box("named"), Some(1), work(1));
        p.leave(work(2));
        p.leave(work(2));

        let node = p.finish(Vec::new()).root.expect("root");
        // The root consumes no segment, and neither does it invent one.
        assert_eq!(node.segment, None);
        assert_eq!(node.kind, SceneNodeKind::Container);
        assert_eq!(node.children[0].segment.as_deref(), Some("0"));
        assert_eq!(node.children[0].tag, None);
        assert_eq!(node.children[1].segment.as_deref(), Some("named"));
        assert_eq!(node.children[1].tag.as_deref(), Some("named"));
    }

    #[test]
    fn r1557_a_scroll_content_consumes_no_segment() {
        // `index: None` below the root is how the walk states "this node is
        // reached at its parent's address" — the scroll-content rule.
        let content = container(vec![]);
        let scroll = container(vec![container(vec![])]);
        let mut p = DrawProfiler::new();
        p.enter(&scroll, None, work(0));
        p.enter(&content, None, work(1));
        p.leave(work(4));
        p.leave(work(5));

        let node = p.finish(Vec::new()).root.expect("root");
        assert_eq!(node.children[0].segment, None);
        assert_eq!(node.children[0].total, work(3));
        assert_eq!(node.own, work(2));
    }

    #[test]
    fn r1557_unentered_profiler_has_no_root() {
        assert_eq!(
            DrawProfiler::new().finish(Vec::new()),
            DrawProfile {
                root: None,
                scope: Vec::new()
            }
        );
    }

    #[test]
    fn r1557_unpaired_leave_is_inert() {
        // The contract's stated failure mode: a `leave` with nothing open must
        // not panic and must not fabricate a root out of the census it was
        // handed. Instrumentation may under-report; it may not abort a paint.
        let mut p = DrawProfiler::new();
        p.leave(work(7));
        assert_eq!(p.finish(Vec::new()).root, None);
    }

    #[test]
    fn r1557_a_node_that_drew_nothing_is_still_a_row() {
        // A census is a census: a subtree that contributed nothing appears with
        // zeroes, rather than being pruned into invisibility. Otherwise "this
        // subtree is free" and "this subtree is missing from the profile" are
        // the same reading.
        let empty = container(vec![]);
        let root = container(vec![container(vec![])]);
        let mut p = DrawProfiler::new();
        p.enter(&root, None, work(4));
        p.enter(&empty, Some(0), work(6));
        p.leave(work(6));
        p.leave(work(6));

        let node = p.finish(Vec::new()).root.expect("root");
        assert_eq!(node.total, work(2));
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].total, DrawWork::default());
        assert_eq!(node.own_sum(), node.total);
    }

    #[test]
    fn r1557_all_six_fields_are_attributed() {
        // The R1556 counterfactual lesson: an expectation that names half the
        // fields passes for a defect in the other half. Every field is
        // distinct here, and the expected values are whole struct literals, so
        // a field added to `DrawWork` stops this compiling.
        let leaf = tagged_box("leaf");
        let root = container(vec![tagged_box("leaf")]);
        let before = DrawWork {
            draws: 1,
            paths: 2,
            path_segments: 3,
            layers: 4,
            glyph_runs: 5,
            glyphs: 6,
        };
        let child_end = DrawWork {
            draws: 11,
            paths: 22,
            path_segments: 33,
            layers: 44,
            glyph_runs: 55,
            glyphs: 66,
        };
        let root_end = DrawWork {
            draws: 111,
            paths: 222,
            path_segments: 333,
            layers: 444,
            glyph_runs: 555,
            glyphs: 666,
        };
        let mut p = DrawProfiler::new();
        p.enter(&root, None, before);
        p.enter(&leaf, Some(0), before);
        p.leave(child_end);
        p.leave(root_end);

        let node: DrawProfileNode = p.finish(Vec::new()).root.expect("root");
        assert_eq!(
            node.total,
            DrawWork {
                draws: 110,
                paths: 220,
                path_segments: 330,
                layers: 440,
                glyph_runs: 550,
                glyphs: 660,
            }
        );
        assert_eq!(
            node.children[0].total,
            DrawWork {
                draws: 10,
                paths: 20,
                path_segments: 30,
                layers: 40,
                glyph_runs: 50,
                glyphs: 60,
            }
        );
        assert_eq!(
            node.own,
            DrawWork {
                draws: 100,
                paths: 200,
                path_segments: 300,
                layers: 400,
                glyph_runs: 500,
                glyphs: 600,
            }
        );
        assert_eq!(node.own_sum(), node.total);
    }
}
