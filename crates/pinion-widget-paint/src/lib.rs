//! R657 §5.16 §5.38 — backend-agnostic widget paint composition.
//!
//! This crate is the framework-side home for **per-widget paint
//! helpers** — backend-agnostic `Scene` composition routines that
//! consume the SCXML statechart state + reactive substrate
//! (`TextEditState`, `CaretBlink`, etc.) and produce the visual
//! `Scene` fragment a binding inserts into its view-fn output.
//!
//! ## Why a separate crate
//!
//! Pre-R657, the only consumer of `TextField` paint composition was
//! `examples/hello-textfield`. R655 added `examples/todomvc` as the
//! 2nd consumer — and 17 out of the binding's 19 view-fn diff-lines
//! were the same caret/selection/preedit/field-fill composition,
//! copy-pasted verbatim. The R51.113 [[substrate-incompleteness-signal]]
//! and the [[abstraction-needs-second-consumer]] gate both fire at
//! 2nd consumer; R657 lands the substrate in this crate so further
//! composed apps (R658 toggle, R660 edit, R663 settings panel)
//! don't re-duplicate the same ~280 LOC.
//!
//! ## Why not pinion-core
//!
//! The seed-prompt's first design pass placed the helper in
//! `pinion-core/src/widgets/text_field.rs`. That route is blocked by
//! the crate dep graph: the lift needs
//! [`pinion_text::cache::LayoutCache`] +
//! [`pinion_text::caret::caret_rect_for_byte_offset`] (parley-backed
//! text shaping), and `pinion-text` depends on `pinion-core` rather
//! than the reverse. Pulling parley/swash/fontique into pinion-core
//! would pollute the foundational layer with text-shaping deps,
//! violating the §6 layered architecture. Hence a new crate one tier
//! above pinion-text (and parallel to pinion-shell / pinion-tui),
//! cleanly placed for the substrate it composes.
//!
//! ## Why not pinion-shell
//!
//! `pinion-shell` is the **Vello/winit GUI** shell — anchoring widget
//! paint helpers there would couple them to the GUI backend, and the
//! same widget rendering primitive would not be reachable from the
//! ratatui `pinion-tui` backend. §2 invariant #6 (GUI/TUI dual from
//! one scene structure) requires the paint composition helpers to
//! live outside both per-backend shells so both can consume them
//! identically.
//!
//! ## Dep graph
//!
//! ```text
//!         pinion-core (Scene / Style / Theme / widget statecharts)
//!              ▲
//!              │
//!         pinion-text (LayoutCache / caret_rect / shaping)
//!              ▲
//!              │
//!   pinion-widget-paint (THIS CRATE — view_field, future view_toggle, etc.)
//!              ▲
//!              ├──── pinion-shell (Vello/winit GUI)
//!              └──── pinion-tui   (ratatui TUI)
//! ```
//!
//! ## First consumer
//!
//! R657 — [`text_field`] module surfaces the previously-duplicated
//! `TextField` paint composition: [`text_field::TextFieldStyle`]
//! (M3-tuned dimensions + alpha constants), [`text_field::view_field`]
//! (the 280-LOC caret/selection/preedit/field-fill composition),
//! [`text_field::ime_caret_rect_for`] (the IME platform bridge caret
//! rect derivation), plus the SCXML name lookup helpers.
//!
//! ## Future consumers (per [[abstraction-needs-second-consumer]])
//!
//! Per-widget paint modules land in this crate only after the
//! 2nd-consumer signal fires. R657 ships only the `text_field` module.
//! Toggle, Slider, Checkbox, Radio, `ListBox` bindings stay
//! single-consumer until a 2nd composed app exercises each.

#![forbid(unsafe_code)]

pub mod barrier;
pub mod button;
pub mod checkbox;
pub mod chip;
pub mod datepicker;
pub mod devtools;
pub mod disclosure;
pub mod dialog;
pub mod elevation;
pub mod dock;
pub mod drawer;
pub mod menu;
pub mod radio_composite;
pub mod scrim;
pub mod scrollbar;
pub mod slider;
pub mod splitter;
pub mod state_layer;
pub mod table;
pub mod tabs;
pub mod text_field;
pub mod toolbar;
pub mod tooltip;
pub mod tree_view;
pub mod virtual_list;
