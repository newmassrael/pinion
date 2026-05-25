//! R670 §5.16 — pinion-forge codegen entrypoint for hello-popover.
//!
//! Compiles `app.pinion.xml` to `$OUT_DIR/app.rs`. `main.rs` pulls the
//! result in via `include!(concat!(env!("OUT_DIR"), "/app.rs"))`.
//!
//! Identical shape to every other hello-* `build.rs` — codegen success
//! is silent (a `cargo:warning=` on the happy path would pollute
//! `cargo run` output and erode the workspace `warnings = "deny"`
//! signal-to-noise floor); diagnostic failure prints every accumulated
//! diagnostic on its own line so a single typo surfaces with full
//! context.

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
            panic!(
                "pinion-forge: {} diagnostic(s) in {}",
                diags.len(),
                input.display(),
            );
        }
        Err(other) => panic!("pinion-forge: {other}"),
    }
}
