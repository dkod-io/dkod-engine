fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Proto files are copied into this crate (crates/dk-protocol/proto/) so the
    // crate is self-contained for crates.io publishing. The canonical source is
    // proto/dkod/v1/ at the workspace root. CI enforces that both copies stay in
    // sync — see the "Proto sync check" step in .github/workflows/ci.yml.
    let proto_root = std::path::Path::new("proto");

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .build_transport(false)
        .compile_protos(
            &[proto_root.join("dkod/v1/agent.proto")],
            &[proto_root],
        )?;
    Ok(())
}
