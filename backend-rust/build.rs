// Build script to provide missing MSVC 14.43 STL symbols for ort-sys compatibility.
// The ort-sys crate's C++ wrappers are compiled with MSVC 14.43 but the
// ONNX Runtime binary was compiled with an older MSVC. This causes linker
// errors for __std_find_first_of_trivial_pos_1/2 which are new inline
// functions in MSVC 14.43's STL that the compiler may emit as out-of-line calls.

fn main() {
    // Only needed on Windows with MSVC
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows"
        && std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc"
    {
        // Compile a small stub that provides the missing symbols
        cc::Build::new()
            .file("src/ort_msvc_stub.cc")
            .compile("ort_msvc_stub");
        println!("cargo:rustc-link-lib=ort_msvc_stub");
    }
}
