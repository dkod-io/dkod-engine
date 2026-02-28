fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = std::path::Path::new("../../proto");

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .build_transport(false)
        .compile_protos(
            &[proto_root.join("dekode/v1/agent.proto")],
            &[proto_root],
        )?;
    Ok(())
}
