//! R46.3 §5.16 pinion-forge codegen entrypoint for the ai-introspect-demo.
//!
//! Compiles `app.pinion.xml` to `$OUT_DIR/demo_renderer.rs`. `main.rs`
//! pulls the result in via
//! `include!(concat!(env!("OUT_DIR"), "/demo_renderer.rs"))`.
//!
//! On DSL-level failure every diagnostic is printed on its own line
//! before the build aborts — `pinion-forge` accumulates all failures
//! per pass so a typo in the manifest surfaces all related diagnostics
//! at once. Same pattern as `examples/forge-counter/build.rs` (R38.2e
//! first dogfood).

use std::path::Path;

fn main() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("cargo populates CARGO_MANIFEST_DIR for build scripts");
    let out_dir = std::env::var_os("OUT_DIR").expect("cargo populates OUT_DIR");

    let input = Path::new(&manifest_dir).join("app.pinion.xml");

    // Rerun the codegen only when the manifest or this build script
    // changes — without these hints cargo runs the script every build.
    println!("cargo:rerun-if-changed={}", input.display());
    println!("cargo:rerun-if-changed=build.rs");

    match pinion_forge::compile_file(&input, Path::new(&out_dir)) {
        // Silent success path — see hello-button build.rs for the
        // rationale. Codegen alerts on the happy path conflict with the
        // workspace `warnings = "deny"` floor.
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
