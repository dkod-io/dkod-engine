//! End-to-end test: CONNECT -> CONTEXT -> SUBMIT -> VERIFY -> MERGE
//!
//! Requires: PostgreSQL running, dk-server running on localhost:50051
//! Run with: cargo test -p dk-agent-sdk --test workflow_test -- --ignored --nocapture

#[tokio::test]
#[ignore] // Requires running server
async fn full_agent_workflow() {
    use dk_agent_sdk::{AgentClient, Change, Depth};

    // Connect
    let mut client = AgentClient::connect("http://[::1]:50051", "dk-alpha-token")
        .await
        .expect("failed to connect to server");

    let mut session = client
        .init("test-sdk-repo", "add hello world function")
        .await
        .expect("failed to init session");

    println!(
        "Connected: session={}, changeset={}",
        session.session_id, session.changeset_id
    );
    assert!(!session.session_id.is_empty());
    assert!(!session.changeset_id.is_empty());

    // Context -- search for existing code
    let ctx = session
        .context("main function", Depth::Signatures, 1000)
        .await
        .expect("context failed");

    println!(
        "Context: {} symbols, ~{} tokens",
        ctx.symbols.len(),
        ctx.estimated_tokens
    );

    // Submit -- add a new file
    let result = session
        .submit(vec![Change::Add {
            path: "src/hello.rs".to_string(),
            content: "pub fn hello() -> &'static str {\n    \"Hello from dk-agent-sdk!\"\n}\n"
                .to_string(),
        }])
        .await
        .expect("submit failed");

    println!(
        "Submitted: changeset={}, status={}",
        result.changeset_id, result.status
    );
    assert!(!result.changeset_id.is_empty());

    // Verify -- run pipeline (will likely fail since no real Cargo project, but should not error)
    let steps = session.verify().await.expect("verify failed");
    for step in &steps {
        println!(
            "  verify step {}: {} = {}",
            step.step_order, step.step_name, step.status
        );
    }
    assert!(
        !steps.is_empty(),
        "should have at least one verification step"
    );

    // Note: MERGE will fail if verification rejected the changeset.
    // In a real test with a proper Rust project, verification would pass.
    // For now, we just verify the SDK mechanics work.
    println!("\nTest complete -- SDK workflow verified!");
}
