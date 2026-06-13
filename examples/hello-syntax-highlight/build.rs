//! R904 §5.36 pinion-forge codegen entrypoint for hello-syntax-highlight.
//!
//! Identical shape to hello-toggle's `build.rs` (R51.29): compiles
//! `app.pinion.xml` to `$OUT_DIR/app.rs`, which `main.rs` pulls in via
//! `include!(concat!(env!("OUT_DIR"), "/app.rs"))`. On DSL-level
//! failure every diagnostic is printed on its own line before the
//! build aborts — `pinion-forge` accumulates failures per pass so a
//! typo in the manifest surfaces all related diagnostics at once.

use std::path::Path;

fn main() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("cargo populates CARGO_MANIFEST_DIR for build scripts");
    let out_dir = std::env::var_os("OUT_DIR").expect("cargo populates OUT_DIR");

    let input = Path::new(&manifest_dir).join("app.pinion.xml");

    println!("cargo:rerun-if-changed={}", input.display());
    println!("cargo:rerun-if-changed=build.rs");

    match pinion_forge::compile_file(&input, Path::new(&out_dir)) {
        Ok(_) => {}
        Err(pinion_forge::CompileError::Diagnostics(diags)) => {
            for d in &diags {
                eprintln!("{d}");
            }
            panic!("pinion-forge: {} diagnostic(s) in {}", diags.len(), input.display());
        }
        Err(other) => panic!("pinion-forge: {other}"),
    }
}
