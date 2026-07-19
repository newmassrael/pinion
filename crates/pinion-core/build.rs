fn main() {
    // SCXML inputs compiled by sce-build (§5.4 + §5.19):
    //   widgets/button.scxml         -> button_sm.rs       (R12 widget statechart)
    //   widgets/toggle.scxml         -> toggle_sm.rs       (R51.2 §5.38 Toggle widget)
    //   app.scxml                    -> app_sm.rs          (R16 §5.19 window topology)
    //   fixtures/multi_window.scxml  -> multi_window_sm.rs (R16 §5.17 parallel-root fixture)
    let scxml_inputs = [
        "widgets/button.scxml",
        "widgets/toggle.scxml",
        "widgets/checkbox.scxml",
        "widgets/disclosure.scxml",
        "widgets/radio.scxml",
        "widgets/listbox_item.scxml",
        "widgets/slider.scxml",
        "widgets/color_area.scxml",
        "widgets/scroll_bar.scxml",
        "widgets/text_field.scxml",
        "widgets/dock_panel.scxml",
        "app.scxml",
        "fixtures/multi_window.scxml",
    ];
    // Inject caller-supplied derives onto every generated `{widget}State` /
    // `{widget}Event` enum via the sce-build derive hook (rust_derive_policy
    // SSOT):
    //
    // * SCE-004 — `serde::{Serialize, Deserialize}` on the State enum lets the
    //   state back a `Signal<T>` directly (`Signal<T>` bounds `T: Clone +
    //   PartialEq + Serialize + DeserializeOwned`), retiring the per-widget
    //   u8-tag mirror.
    // * SCE-002 — `pinion_derive::WidgetStateName` on the State enum and
    //   `pinion_derive::WidgetEventName` on the Event enum reconstruct the
    //   `Self ↔ &'static str` statechart-name mapping from the markers the sce
    //   codegen emits (the State's `#[default]` SCXML-initial variant + the
    //   Event's `EXTERNALLY_DRIVABLE_EVENTS` associated const), retiring the
    //   per-widget `widget_state_name!` / `widget_event_name!` hand-written
    //   macros. `pinion-core` normal-deps `pinion-derive` (no cycle — the
    //   derive crate only dev-deps back) and aliases itself as `pinion_core`
    //   via `extern crate self as` so the derive-emitted `::pinion_core::…`
    //   trait paths resolve inside this crate.
    sce_build::compile_scxml_with_derives(
        &scxml_inputs,
        &[
            "serde::Serialize".to_string(),
            "serde::Deserialize".to_string(),
            "pinion_derive::WidgetStateName".to_string(),
        ],
        &["pinion_derive::WidgetEventName".to_string()],
    );

    // Post-process: strip inner attributes (#![...]) and inner doc comments (//!).
    // include!() in Rust does not allow inner attributes in its expanded content,
    // so we move them to module-level outer attributes on the wrapping mod block.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    for scxml_path in &scxml_inputs {
        let stem = std::path::Path::new(scxml_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("invalid SCXML filename");
        let path = std::path::Path::new(&out_dir).join(format!("{stem}_sm.rs"));
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read generated {}: {e}", path.display()));
        let cleaned: String = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("#![") && !trimmed.starts_with("//!")
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, cleaned)
            .unwrap_or_else(|e| panic!("write cleaned {}: {e}", path.display()));
    }
}
