fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_dir = format!("{manifest_dir}/lib");

    // Point chdb-rust's build script at the pre-bundled libchdb.so so it
    // doesn't attempt a network download during CI or offline builds.
    println!("cargo:rustc-env=CHDB_LIB_DIR={lib_dir}");

    // Add the lib dir to the native linker search path.
    println!("cargo:rustc-link-search=native={lib_dir}");

    println!("cargo:rerun-if-changed=lib/libchdb.so");
}
