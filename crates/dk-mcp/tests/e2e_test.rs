//! E2E integration test for dk-mcp.
//! Requires: dk-server running with PostgreSQL.
//! Run with: DATABASE_URL=postgres://localhost/dkod_test cargo test -p dk-mcp --test e2e_test

#[cfg(test)]
mod tests {
    use dk_mcp::grpc;
    use dk_mcp::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    #[ignore] // Only run when dk-server is up
    async fn test_full_agent_flow() {
        let addr = "http://[::1]:50051";
        let token =
            std::env::var("DKOD_AUTH_TOKEN").expect("DKOD_AUTH_TOKEN must be set for e2e test");
        let mut client = grpc::connect_with_auth(addr, token.clone())
            .await
            .expect("connect to server");

        // 1. CONNECT
        let resp = client
            .connect(ConnectRequest {
                agent_id: "test-agent".to_string(),
                auth_token: token,
                codebase: "demo/hello-world".to_string(),
                intent: "Test the full flow".to_string(),
                workspace_config: None,
                agent_name: String::new(),
                owner_id: None,
            })
            .await
            .expect("CONNECT should succeed");
        let resp = resp.into_inner();
        assert!(!resp.session_id.is_empty(), "session_id should be set");
        let session_id = resp.session_id.clone();
        let changeset_id = resp.changeset_id.clone();

        // 2. FILE_WRITE
        let write_resp = client
            .file_write(FileWriteRequest {
                session_id: session_id.clone(),
                path: "src/greet.rs".to_string(),
                content:
                    b"pub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n"
                        .to_vec(),
            })
            .await
            .expect("FILE_WRITE should succeed");
        assert!(!write_resp.into_inner().new_hash.is_empty());

        // 3. SUBMIT
        let submit_resp = client
            .submit(SubmitRequest {
                session_id: session_id.clone(),
                intent: "Added greeting function".to_string(),
                changes: vec![],
                changeset_id: changeset_id.clone(),
            })
            .await
            .expect("SUBMIT should succeed");
        assert_eq!(submit_resp.into_inner().status, 0, "should be ACCEPTED");

        // 4. VERIFY (streaming)
        let verify_resp = client
            .verify(VerifyRequest {
                session_id: session_id.clone(),
                changeset_id: changeset_id.clone(),
            })
            .await
            .expect("VERIFY should succeed");

        let mut stream = verify_resp.into_inner();
        while let Some(step) = stream.next().await {
            let step = step.expect("verify step should not error");
            eprintln!("Verify: {} — {}", step.step_name, step.status);
        }

        // 5. MERGE
        let merge_resp = client
            .merge(MergeRequest {
                session_id,
                changeset_id,
                commit_message: "Added greeting function".to_string(),
                force: false,
                author_name: String::new(),
                author_email: String::new(),
            })
            .await
            .expect("MERGE should succeed");
        let merge = merge_resp.into_inner();
        match merge.result {
            Some(dk_mcp::merge_response::Result::Success(ref s)) => {
                assert!(!s.commit_hash.is_empty(), "commit_hash should be set");
                eprintln!("Merged! Commit: {}", s.commit_hash);
            }
            Some(dk_mcp::merge_response::Result::Conflict(ref c)) => {
                panic!("Expected success but got conflict: {:?}", c.conflicts);
            }
            Some(dk_mcp::merge_response::Result::OverwriteWarning(ref w)) => {
                panic!(
                    "Expected success but got overwrite warning: {:?}",
                    w.overwrites
                );
            }
            None => {
                panic!("Expected success but got empty result");
            }
        }
    }
}
