fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Proto files are copied into this crate (crates/dk-protocol/proto/) so the
    // crate is self-contained for crates.io publishing. The canonical source is
    // proto/dkod/v1/ at the workspace root. CI enforces that both copies stay in
    // sync — see the "Proto sync check" step in .github/workflows/ci.yml.
    //
    // Generated layout mirrors sdk/python/dkod/_generated/:
    //   src/generated/dkod/v1/agent.rs   — messages + gRPC stubs from agent.proto
    //   src/generated/dkod/v1/types.rs   — shared types from types.proto
    let proto_root = std::path::Path::new("proto");
    let v1_dir = std::path::Path::new("src/generated/dkod/v1");
    std::fs::create_dir_all(v1_dir)?;

    // Use a temp dir so tonic-build writes dkod.v1.rs there; we then rename.
    let tmp = std::env::var("OUT_DIR").unwrap();
    let tmp = std::path::Path::new(&tmp);

    // Compile agent.proto (imports types.proto automatically).
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .build_transport(false)
        .out_dir(tmp)
        .compile_protos(
            &[proto_root.join("dkod/v1/agent.proto")],
            &[proto_root],
        )?;
    std::fs::copy(tmp.join("dkod.v1.rs"), v1_dir.join("agent.rs"))?;

    // Compile types.proto alone to get only the shared types.
    tonic_build::configure()
        .build_server(false)
        .build_client(false)
        .build_transport(false)
        .out_dir(tmp)
        .compile_protos(
            &[proto_root.join("dkod/v1/types.proto")],
            &[proto_root],
        )?;
    std::fs::copy(tmp.join("dkod.v1.rs"), v1_dir.join("types.rs"))?;

    Ok(())
}
