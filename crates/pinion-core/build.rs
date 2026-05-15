fn main() {
    sce_build::compile_scxml(&["widgets/button.scxml"]);

    // Post-process: strip inner attributes (#![...]) and inner doc comments (//!).
    // include!() in Rust does not allow inner attributes in its expanded content,
    // so we move them to module-level outer attributes on the wrapping mod block.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let path = std::path::Path::new(&out_dir).join("button_sm.rs");
    let content = std::fs::read_to_string(&path).expect("read generated button_sm.rs");
    let cleaned: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("#![") && !trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, cleaned).expect("write cleaned button_sm.rs");
}
