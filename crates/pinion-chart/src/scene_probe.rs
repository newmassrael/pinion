//! Test-only scene probes: "which node carries this tag" and "what tags did
//! this chart emit".
//!
//! R1553 obligation-3b lift. Every chart builder in this crate asserts on the
//! tags it emits, so every one of them had grown its own copy of the same
//! walk — measured before lifting: **eight** copies of `find` in three
//! spellings (`then_some`, an `if`/`else`, and a leading-`tag()` restructure)
//! and **three** of `tags`, all semantically identical. There is no per-chart
//! opinion in "descend a container's children", which is what makes this
//! mechanical rather than a shared style the builders were each entitled to
//! pick — the [`crate::draw`] axis furniture was lifted on the same rule at
//! its third consumer (R1377).
//!
//! Kept `#[cfg(test)]`: these answer questions about a *built* scene that the
//! production paths never ask. `pinion-rpc`'s `find_by_tag` is the same
//! traversal over the serialized wire tree, and is not reusable here — this
//! crate does not depend on it, and a chart's unit test asserts on the
//! `Scene` it produced rather than on a snapshot of it.

use pinion_core::Scene;

/// The first node carrying `tag`, depth-first, or `None` when absent.
///
/// A container is tested before its children, so a chart root and a child
/// sharing a tag resolve to the root — the order a caller asking "where is
/// `chart.box.0`" means.
pub(crate) fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
    if scene.tag() == Some(tag) {
        return Some(scene);
    }
    match scene {
        Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
        _ => None,
    }
}

/// Whether `scene` holds a node tagged `tag`.
pub(crate) fn has(scene: &Scene, tag: &str) -> bool {
    find(scene, tag).is_some()
}

/// Every tag in `scene`, in emit order.
pub(crate) fn tags(scene: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    collect(scene, &mut out);
    out
}

/// How many of `scene`'s tags begin with `prefix` — the cardinality question
/// a per-datum mark set is asserted with (`chart.outlier.`).
pub(crate) fn count_prefix(scene: &Scene, prefix: &str) -> usize {
    tags(scene).iter().filter(|t| t.starts_with(prefix)).count()
}

/// The content of the [`Scene::Text`] carrying `tag` — `None` when nothing
/// carries it, or the node that does is not text.
///
/// R1567 lift. `donut`, `bar`, `timeline` and `treemap` each held a
/// byte-identical copy, and the candlestick chart's slot-label assertion
/// would have been the fifth — the same mechanical duplication R1553 lifted
/// [`find`] and [`tags`] for, one accessor later.
pub(crate) fn text_of<'a>(scene: &'a Scene, tag: &str) -> Option<&'a str> {
    match find(scene, tag)? {
        Scene::Text(t) => Some(t.content.as_str()),
        _ => None,
    }
}

fn collect(scene: &Scene, out: &mut Vec<String>) {
    if let Some(t) = scene.tag() {
        out.push(t.to_string());
    }
    if let Scene::Container(c) = scene {
        for ch in &c.children {
            collect(ch, out);
        }
    }
}
