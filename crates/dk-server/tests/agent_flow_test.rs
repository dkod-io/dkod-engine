//! End-to-end integration test for the agent workflow.
//!
//! Requires a running PostgreSQL instance. Run with:
//! ```
//! DATABASE_URL=postgres://localhost/dekode_test cargo test -p dk-server --test agent_flow_test -- --ignored
//! ```

use std::sync::Arc;

use dk_engine::repo::Engine;
use dk_protocol::agent_service_server::AgentService;
use dk_protocol::*;
use sqlx::PgPool;
use tonic::Request;

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_agent_flow_connect_context_submit() {
    // ── Setup ──

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/dekode_test".into());
    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("../dk-engine/migrations")
        .run(&pool)
        .await
        .unwrap();

    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new(tmp.path().to_path_buf(), pool.clone()).unwrap();

    // Create a repo with a unique name to avoid collisions.
    let repo_name = format!("test/agent-flow-{}", uuid::Uuid::new_v4());
    let repo_id = engine.create_repo(&repo_name).await.unwrap();
    let (_, git_repo) = engine.get_repo(&repo_name).await.unwrap();

    // Write a Rust source file into the repo's working directory.
    let src_dir = git_repo.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("auth.rs"),
        r#"
pub fn authenticate_user(token: &str) -> bool {
    !token.is_empty()
}

pub fn validate_token(token: &str) -> Result<String, String> {
    if token.len() > 5 {
        Ok(token.to_string())
    } else {
        Err("Token too short".to_string())
    }
}
"#,
    )
    .unwrap();

    // Index the repo.
    engine.index_repo(repo_id, &git_repo).await.unwrap();

    // Drop the git_repo handle before handing the engine to ProtocolServer
    // (GitRepository is !Sync and must not live across the async boundary).
    drop(git_repo);

    // Create the protocol server.
    let server = ProtocolServer::new(Arc::new(engine), "test-token".to_string());

    // ── 1. CONNECT ──

    let connect_resp = server
        .connect(Request::new(ConnectRequest {
            agent_id: "test-agent".into(),
            auth_token: "test-token".into(),
            codebase: repo_name.clone(),
            intent: "Testing agent flow".into(),
            workspace_config: None,
        }))
        .await
        .unwrap()
        .into_inner();

    assert!(
        !connect_resp.session_id.is_empty(),
        "Should receive a session ID"
    );

    let summary = connect_resp.summary.unwrap();
    assert!(
        summary.total_symbols > 0,
        "Should have indexed at least one symbol, got {}",
        summary.total_symbols
    );

    let session_id = connect_resp.session_id;
    let changeset_id = connect_resp.changeset_id;

    // ── 2. CONTEXT — search for "authenticate" ──

    let context_resp = server
        .context(Request::new(ContextRequest {
            session_id: session_id.clone(),
            query: "authenticate".into(),
            depth: ContextDepth::Full.into(),
            include_tests: false,
            include_dependencies: false,
            max_tokens: 0,
        }))
        .await
        .unwrap()
        .into_inner();

    assert!(
        !context_resp.symbols.is_empty(),
        "Should find the authenticate_user symbol"
    );

    // FULL depth should include source code.
    let first = &context_resp.symbols[0];
    assert!(
        first.source.is_some(),
        "FULL depth should include source code"
    );

    // ── 3. SUBMIT — add a new file ──

    let submit_resp = server
        .submit(Request::new(SubmitRequest {
            session_id: session_id.clone(),
            changeset_id: changeset_id.clone(),
            intent: "Add greeting function".into(),
            changes: vec![Change {
                r#type: ChangeType::AddFunction.into(),
                symbol_name: "greet".into(),
                file_path: "src/greet.rs".into(),
                old_symbol_id: None,
                new_source: "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n"
                    .into(),
                rationale: "Add a greeting utility".into(),
            }],
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        submit_resp.status(),
        SubmitStatus::Accepted,
        "Submit should be accepted, got {:?} with errors: {:?}",
        submit_resp.status(),
        submit_resp.errors
    );
    assert!(!submit_resp.changeset_id.is_empty());

    // ── 4. CONTEXT again — verify "greet" is now indexed ──

    let context_resp2 = server
        .context(Request::new(ContextRequest {
            session_id: session_id.clone(),
            query: "greet".into(),
            depth: ContextDepth::Signatures.into(),
            include_tests: false,
            include_dependencies: false,
            max_tokens: 0,
        }))
        .await
        .unwrap()
        .into_inner();

    assert!(
        !context_resp2.symbols.is_empty(),
        "Should find the newly submitted greet symbol"
    );

    // With SIGNATURES depth, source should not be included.
    let greet_result = &context_resp2.symbols[0];
    assert!(
        greet_result.source.is_none(),
        "SIGNATURES depth should not include source code"
    );

    // ── Cleanup ──

    sqlx::query("DELETE FROM repositories WHERE name = $1")
        .bind(&repo_name)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_connect_invalid_auth() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/dekode_test".into());
    let pool = PgPool::connect(&db_url).await.unwrap();

    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new(tmp.path().to_path_buf(), pool).unwrap();
    let server = ProtocolServer::new(Arc::new(engine), "correct-token".to_string());

    let result = server
        .connect(Request::new(ConnectRequest {
            agent_id: "test".into(),
            auth_token: "wrong-token".into(),
            codebase: "test/repo".into(),
            intent: "test".into(),
            workspace_config: None,
        }))
        .await;

    assert!(result.is_err(), "Wrong token should be rejected");

    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::Unauthenticated,
        "Should return UNAUTHENTICATED status"
    );
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_context_invalid_session() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/dekode_test".into());
    let pool = PgPool::connect(&db_url).await.unwrap();

    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new(tmp.path().to_path_buf(), pool).unwrap();
    let server = ProtocolServer::new(Arc::new(engine), "test-token".to_string());

    let result = server
        .context(Request::new(ContextRequest {
            session_id: uuid::Uuid::new_v4().to_string(),
            query: "anything".into(),
            depth: ContextDepth::Signatures.into(),
            include_tests: false,
            include_dependencies: false,
            max_tokens: 0,
        }))
        .await;

    assert!(result.is_err(), "Non-existent session should be rejected");

    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::NotFound,
        "Should return NOT_FOUND status for expired/missing session"
    );
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_submit_modify_nonexistent_file() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/dekode_test".into());
    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("../dk-engine/migrations")
        .run(&pool)
        .await
        .unwrap();

    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new(tmp.path().to_path_buf(), pool.clone()).unwrap();

    let repo_name = format!("test/submit-err-{}", uuid::Uuid::new_v4());
    engine.create_repo(&repo_name).await.unwrap();

    let server = ProtocolServer::new(Arc::new(engine), "test-token".to_string());

    // CONNECT to get a session
    let connect_resp = server
        .connect(Request::new(ConnectRequest {
            agent_id: "test-agent".into(),
            auth_token: "test-token".into(),
            codebase: repo_name.clone(),
            intent: "test submit error".into(),
            workspace_config: None,
        }))
        .await
        .unwrap()
        .into_inner();

    let session_id = connect_resp.session_id;
    let changeset_id = connect_resp.changeset_id;

    // Try to MODIFY a file that does not exist
    let submit_resp = server
        .submit(Request::new(SubmitRequest {
            session_id,
            changeset_id,
            intent: "Modify missing file".into(),
            changes: vec![Change {
                r#type: ChangeType::ModifyFunction.into(),
                symbol_name: "missing_fn".into(),
                file_path: "src/nonexistent.rs".into(),
                old_symbol_id: None,
                new_source: "fn missing_fn() {}".into(),
                rationale: "Should fail".into(),
            }],
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        submit_resp.status(),
        SubmitStatus::Rejected,
        "Modifying a nonexistent file should be rejected"
    );
    assert!(
        !submit_resp.errors.is_empty(),
        "Should report errors for the missing file"
    );

    // Cleanup
    sqlx::query("DELETE FROM repositories WHERE name = $1")
        .bind(&repo_name)
        .execute(&pool)
        .await
        .unwrap();
}
