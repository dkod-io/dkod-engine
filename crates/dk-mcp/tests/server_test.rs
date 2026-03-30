//! Unit tests for dk-mcp MCP server logic.
//!
//! Tests the DkodMcp server's MCP-layer behavior without a running gRPC backend.
//! Uses an in-process MCP client-server pair via `tokio::io::duplex` to exercise
//! `list_resources`, `read_resource`, and `get_info` through the real MCP protocol.

use rmcp::{
    model::{ClientInfo, ReadResourceRequestParams},
    ClientHandler, ServiceExt,
};

// ---------------------------------------------------------------------------
// Helper: spin up an in-process MCP server+client pair
// ---------------------------------------------------------------------------

/// Minimal MCP client handler required by rmcp to complete the handshake.
#[derive(Debug, Clone, Default)]
struct DummyClient;

impl ClientHandler for DummyClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

/// Starts a `DkodMcp` server and a dummy client connected via an in-memory
/// duplex transport. Returns the running client handle (which derefs to
/// `Peer<RoleClient>`) and a join handle for the server task.
async fn start_pair(
    server: dk_mcp::server::DkodMcp,
) -> (
    rmcp::service::RunningService<rmcp::RoleClient, DummyClient>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let server_handle = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });

    let client = DummyClient
        .serve(client_transport)
        .await
        .expect("client handshake should succeed");

    (client, server_handle)
}

// ===========================================================================
// 1. DkodMcp construction
// ===========================================================================

#[tokio::test]
async fn new_creates_valid_instance() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let sessions = server.sessions.read().await;
    assert!(
        sessions.is_empty(),
        "freshly-created server should have no sessions"
    );
}

#[tokio::test]
async fn new_state_has_default_server_addr() {
    // Unless env vars are set, default address should be present.
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let conn = server.connection.read().await;
    assert!(
        !conn.server_addr.is_empty(),
        "server_addr should have a default value"
    );
}

// ===========================================================================
// 2. Session map connected / disconnected
// ===========================================================================

#[tokio::test]
async fn session_map_tracks_sessions() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let sessions = server.sessions.read().await;
        assert!(sessions.is_empty(), "should start empty");
    }

    // Insert a session
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "sess-1".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-1".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
    }
    let sessions = server.sessions.read().await;
    assert_eq!(sessions.len(), 1);
    assert!(sessions.contains_key("sess-1"));
}

#[tokio::test]
async fn session_removal_clears_entry() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "sess-1".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-1".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
    }
    {
        let mut sessions = server.sessions.write().await;
        sessions.remove("sess-1");
    }
    let sessions = server.sessions.read().await;
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn connection_display_contains_server_addr() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let conn = server.connection.read().await;
    let text = conn.to_string();
    assert!(text.contains("server_addr:"), "should contain server_addr");
}

// ===========================================================================
// 3. get_info
// ===========================================================================

#[tokio::test]
async fn get_info_returns_tools_and_resources_capabilities() {
    use rmcp::ServerHandler;

    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let info = server.get_info();

    // Instructions should be non-empty.
    let instructions = info
        .instructions
        .as_deref()
        .expect("instructions should be present");
    assert!(!instructions.is_empty(), "instructions should not be empty");
    assert!(
        instructions.contains("dk_connect"),
        "instructions should mention the connect tool"
    );

    // Capabilities should enable tools and resources.
    let caps = info.capabilities;
    assert!(caps.tools.is_some(), "tools capability should be enabled");
    assert!(
        caps.resources.is_some(),
        "resources capability should be enabled"
    );
}

// ===========================================================================
// 4. list_resources via in-process MCP
// ===========================================================================

#[tokio::test]
async fn list_resources_disconnected_returns_session_only() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .list_resources(None)
        .await
        .expect("list_resources should succeed");

    assert_eq!(
        result.resources.len(),
        1,
        "disconnected: only session resource"
    );
    assert_eq!(result.resources[0].uri, "dkod://session");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn list_resources_connected_returns_three() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    // Simulate a connected session.
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "test-session".to_string(),
            dk_mcp::state::SessionData {
                session_id: "test-session".into(),
                workspace_id: "test-workspace".into(),
                changeset_id: "test-changeset".into(),
                repo_name: "test/repo".into(),
            },
        );
    }

    let (client, server_handle) = start_pair(server).await;

    let result = client
        .list_resources(None)
        .await
        .expect("list_resources should succeed");

    assert_eq!(
        result.resources.len(),
        3,
        "connected: session + symbols + changeset"
    );

    let uris: Vec<&str> = result.resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(
        uris.contains(&"dkod://session"),
        "should contain session resource"
    );
    assert!(
        uris.contains(&"dkod://symbols"),
        "should contain symbols resource"
    );
    assert!(
        uris.contains(&"dkod://changeset"),
        "should contain changeset resource"
    );

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

// ===========================================================================
// 5. read_resource via in-process MCP
// ===========================================================================

#[tokio::test]
async fn read_resource_session_always_works() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://session".into(),
        })
        .await
        .expect("read_resource session should succeed");

    assert!(
        !result.contents.is_empty(),
        "should return at least one content block"
    );

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_symbols_errors_when_disconnected() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://symbols".into(),
        })
        .await;

    assert!(result.is_err(), "symbols should error when disconnected");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_changeset_errors_when_disconnected() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://changeset".into(),
        })
        .await;

    assert!(result.is_err(), "changeset should error when disconnected");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_unknown_uri_errors() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://nonexistent".into(),
        })
        .await;

    assert!(result.is_err(), "unknown URI should error");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_symbols_succeeds_when_connected() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "test-session".to_string(),
            dk_mcp::state::SessionData {
                session_id: "test-session".into(),
                workspace_id: "test-workspace".into(),
                changeset_id: "test-changeset".into(),
                repo_name: "test/repo".into(),
            },
        );
    }
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://symbols".into(),
        })
        .await
        .expect("symbols should succeed when connected");

    assert!(!result.contents.is_empty(), "should return content");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_changeset_succeeds_when_connected() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "test-session".to_string(),
            dk_mcp::state::SessionData {
                session_id: "test-session".into(),
                workspace_id: "test-workspace".into(),
                changeset_id: "test-changeset".into(),
                repo_name: "test/repo".into(),
            },
        );
    }
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://changeset".into(),
        })
        .await
        .expect("changeset should succeed when connected");

    assert!(!result.contents.is_empty(), "should return content");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

#[tokio::test]
async fn read_resource_changeset_contains_ids() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "test-session".to_string(),
            dk_mcp::state::SessionData {
                session_id: "test-session".into(),
                workspace_id: "ws-42".into(),
                changeset_id: "cs-99".into(),
                repo_name: "acme/widgets".into(),
            },
        );
    }
    let (client, server_handle) = start_pair(server).await;

    let result = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "dkod://changeset".into(),
        })
        .await
        .expect("changeset should succeed");

    // Extract text from the response.
    let text = match &result.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
        _ => panic!("expected text resource contents"),
    };

    assert!(text.contains("cs-99"), "should contain changeset_id");
    assert!(text.contains("ws-42"), "should contain workspace_id");
    assert!(text.contains("acme/widgets"), "should contain repo name");

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

// ===========================================================================
// 6. list_tools via in-process MCP (sanity check)
// ===========================================================================

#[tokio::test]
async fn list_tools_returns_expected_tools() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let tools = client
        .list_all_tools()
        .await
        .expect("list_tools should succeed");

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(
        tool_names.contains(&"dk_status"),
        "should have dk_status tool"
    );
    assert!(
        tool_names.contains(&"dk_connect"),
        "should have dk_connect tool"
    );
    assert!(
        tool_names.contains(&"dk_context"),
        "should have dk_context tool"
    );
    assert!(
        tool_names.contains(&"dk_file_read"),
        "should have dk_file_read tool"
    );
    assert!(
        tool_names.contains(&"dk_file_write"),
        "should have dk_file_write tool"
    );
    assert!(
        tool_names.contains(&"dk_file_list"),
        "should have dk_file_list tool"
    );
    assert!(
        tool_names.contains(&"dk_submit"),
        "should have dk_submit tool"
    );
    assert!(
        tool_names.contains(&"dk_verify"),
        "should have dk_verify tool"
    );
    assert!(
        tool_names.contains(&"dk_merge"),
        "should have dk_merge tool"
    );

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}

// ===========================================================================
// 7. resolve_session logic
// ===========================================================================

#[tokio::test]
async fn resolve_session_returns_error_when_no_sessions() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let result = server.resolve_session(None).await;
    assert!(result.is_err(), "should error when no sessions exist");
}

#[tokio::test]
async fn resolve_session_auto_resolves_single_session() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "only-session".to_string(),
            dk_mcp::state::SessionData {
                session_id: "only-session".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
    }
    let result = server.resolve_session(None).await;
    assert!(
        result.is_ok(),
        "should auto-resolve when exactly one session"
    );
    assert_eq!(result.unwrap().session_id, "only-session");
}

#[tokio::test]
async fn resolve_session_errors_on_ambiguous_multiple_sessions() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "sess-1".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-1".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
        sessions.insert(
            "sess-2".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-2".into(),
                workspace_id: "ws-2".into(),
                changeset_id: "cs-2".into(),
                repo_name: "acme/gadgets".into(),
            },
        );
    }
    let result = server.resolve_session(None).await;
    assert!(
        result.is_err(),
        "should error when multiple sessions and no session_id provided"
    );
}

#[tokio::test]
async fn resolve_session_finds_by_explicit_id() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "sess-1".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-1".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
        sessions.insert(
            "sess-2".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-2".into(),
                workspace_id: "ws-2".into(),
                changeset_id: "cs-2".into(),
                repo_name: "acme/gadgets".into(),
            },
        );
    }
    let result = server.resolve_session(Some("sess-2")).await;
    assert!(result.is_ok(), "should find session by explicit ID");
    assert_eq!(result.unwrap().session_id, "sess-2");
}

#[tokio::test]
async fn resolve_session_errors_on_unknown_id() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    {
        let mut sessions = server.sessions.write().await;
        sessions.insert(
            "sess-1".to_string(),
            dk_mcp::state::SessionData {
                session_id: "sess-1".into(),
                workspace_id: "ws-1".into(),
                changeset_id: "cs-1".into(),
                repo_name: "acme/widgets".into(),
            },
        );
    }
    let result = server.resolve_session(Some("nonexistent")).await;
    assert!(result.is_err(), "should error for unknown session ID");
}

// ===========================================================================
// 8. dk_watch tool registration
// ===========================================================================

#[tokio::test]
async fn list_tools_returns_dk_watch() {
    let server = dk_mcp::server::DkodMcp::new_for_testing();
    let (client, server_handle) = start_pair(server).await;

    let tools = client
        .list_all_tools()
        .await
        .expect("list_tools should succeed");

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(
        tool_names.contains(&"dk_watch"),
        "should have dk_watch tool, got: {:?}",
        tool_names,
    );

    client.cancel().await.expect("cancel client");
    let _ = server_handle.await;
}
