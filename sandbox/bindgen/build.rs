use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/struct_ffi.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let header = manifest_dir.join("../../submodules/lz4/lib/lz4.h");
    let rust_bindings = PathBuf::from(env::var("OUT_DIR")?).join("lz4_bindings.rs");
    let moonbit_bindings = manifest_dir.join("../lz4_ffi.mbt");

    moon_bindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .moonbit_visibility(moon_bindgen::Visibility::Public)
        .generate()?
        .write_moonbit_to_file("../my_add_ffi.mbt")?;

    moon_bindgen::Builder::default()
        .input_extern_file("src/struct_ffi.rs")
        .moonbit_visibility(moon_bindgen::Visibility::Public)
        .moonbit_nullability_resolver(|function, position| match (function, position) {
            (
                "test_context_ptr" | "test_context_ptr_ptr",
                moon_bindgen::NullabilityPosition::Return,
            ) => moon_bindgen::Nullability::NonNull,
            _ => moon_bindgen::Nullability::Unspecified,
        })
        .c_stub_file_header("#include <stddef.h>")
        .generate()?
        .write_to_file("../struct_ffi.mbt", "../struct_ffi_stub.c")?;

    bindgen::Builder::default()
        .header(header.to_string_lossy())
        .generate()?
        .write_to_file(&rust_bindings)?;

    moon_bindgen::Builder::default()
        .input_bindgen_file(&rust_bindings)
        .moonbit_file_header("// Source: LZ4 1.10.0 (BSD-2-Clause)")
        .moonbit_visibility(moon_bindgen::Visibility::Public)
        .moonbit_ownership_resolver(|_, _| moon_bindgen::Ownership::Borrow)
        .generate()?
        .write_to_file(moonbit_bindings, manifest_dir.join("../lz4_ffi_stub.c"))?;

    Ok(())
}
