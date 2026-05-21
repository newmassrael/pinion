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
        "widgets/radio.scxml",
        "widgets/listbox_item.scxml",
        "widgets/slider.scxml",
        "widgets/scroll_bar.scxml",
        "app.scxml",
        "fixtures/multi_window.scxml",
    ];
    sce_build::compile_scxml(&scxml_inputs);

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
