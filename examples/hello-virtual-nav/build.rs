//! R776 §5.27 pinion-forge codegen entrypoint for hello-virtual-nav.
//! Identical shape to the other hello-* binary build scripts — the
//! renderer manifest in `app.pinion.xml` is the only divergence (struct
//! name = `HelloVirtualNavRenderer`).

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
