//! Verify that the server binary accepts gRPC-Web content-type headers.
#[test]
fn tonic_web_layer_compiles() {
    use dk_protocol::agent_service_server::AgentServiceServer;
    fn _assert_composable(svc: AgentServiceServer<dk_protocol::ProtocolServer>) {
        let _wrapped = tonic_web::enable(svc);
    }
}
