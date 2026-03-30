fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use the canonical proto files from the repo root (shared with dk-protocol).
    let proto_root = std::path::Path::new("../../proto");

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .build_transport(false)
        .compile_protos(&[proto_root.join("dkod/v1/agent.proto")], &[proto_root])?;
    Ok(())
}
