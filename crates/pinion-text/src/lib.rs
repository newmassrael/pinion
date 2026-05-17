//! R47.1 §5.36 `pinion-text` — backend-orthogonal text shaping &
//! glyph cache framework primitive.
//!
//! Wraps Linebender's parley (layout) + swash (shaping / glyph
//! rasterization) + fontique (font enumeration, transitive via
//! `parley::FontContext`) behind a small surface that every paint
//! backend (Vello GUI today; Headless / TUI / future paths) can
//! consume without re-implementing shaping per backend. Application
//! code never touches parley / swash / fontique directly —
//! `pinion-text` is the public contract.
//!
//! Surface roadmap (this crate evolves through the R47 build slice):
//!
//! * R47.1 — skeleton + workspace integration (this commit).
//! * R47.2 — `GlyphCache` (LRU bounded, GPU-uploadable; realizes the
//!   §5.16 R31 "glyph atlas GPU texture" intent).
//! * R47.3 — `Layout` builder + `paint_adapter` Text arm activation
//!   via `vello::Scene::draw_glyph`.
//! * R47.4+ — `TextStyle` schema extension (`font_family` / weight /
//!   decoration — parley `StyleProperty`-aligned), fontique font
//!   fallback override API, `GlyphCache` evict policy parameters.
//!
//! Lineage: R21 cosmic-text decision is partially superseded — layout
//! responsibility moves to parley for §5.16 R41 Vello ecosystem
//! consistency; R31's glyph atlas / GPU texture intent is preserved
//! through `GlyphCache` and Vello `draw_glyph` integration.
