//! R46.5 §5.16 pinion-forge codegen entrypoint for hello-button.
//!
//! Compiles `app.pinion.xml` to `$OUT_DIR/app.rs`. `main.rs` pulls the
//! result in via `include!(concat!(env!("OUT_DIR"), "/app.rs"))`.
//!
//! On DSL-level failure every diagnostic is printed on its own line
//! before the build aborts — `pinion-forge` accumulates all failures
//! per pass so a typo in the manifest surfaces all related diagnostics
//! at once. Identical to the ai-introspect-demo build.rs (R46.3) and
//! the forge-counter build.rs (R382e first dogfood).

use std::path::Path;

fn main() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("cargo populates CARGO_MANIFEST_DIR for build scripts");
    let out_dir = std::env::var_os("OUT_DIR").expect("cargo populates OUT_DIR");

    let input = Path::new(&manifest_dir).join("app.pinion.xml");

    println!("cargo:rerun-if-changed={}", input.display());
    println!("cargo:rerun-if-changed=build.rs");

    match pinion_forge::compile_file(&input, Path::new(&out_dir)) {
        // Codegen success is the steady state on every clean build,
        // so a silent path is the cargo-idiomatic shape. `cargo:warning=`
        // is reserved for actual warnings (missing dep, deprecated API);
        // emitting it from the happy path pollutes `cargo run` output
        // and prevents the `warnings = "deny"` workspace policy from
        // having a clean signal-to-noise floor.
        Ok(_) => {}
        Err(pinion_forge::CompileError::Diagnostics(diags)) => {
            for d in &diags {
                eprintln!("{d}");
            }
            panic!(
                "pinion-forge: {} diagnostic(s) in {}",
                diags.len(),
                input.display()
            );
        }
        Err(other) => panic!("pinion-forge: {other}"),
    }
}
