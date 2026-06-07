//! R827 §2 #4 §5.20 — pinion-forge codegen entrypoint for
//! hello-immediate-intent.
//!
//! Compiles `app.pinion.xml` to `$OUT_DIR/app.rs`. `main.rs` pulls
//! the result in via `include!(concat!(env!("OUT_DIR"), "/app.rs"))`.
//!
//! Identical shape to every other hello-* `build.rs`.

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
