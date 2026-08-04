//! R1559 §5.36 pinion-forge codegen entrypoint for hello-richtext-list.
//!
//! Identical shape to hello-richtext-blocks's `build.rs`: compiles
//! `app.pinion.xml` to `$OUT_DIR/app.rs`, which `main.rs` pulls in via
//! `include!(concat!(env!("OUT_DIR"), "/app.rs"))`.

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
                input.display()
            );
        }
        Err(other) => panic!("pinion-forge: {other}"),
    }
}
