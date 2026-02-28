"""Shared pytest fixtures that build protobuf objects for testing."""

from __future__ import annotations

from concurrent import futures

import grpc
import pytest

from dekode._generated.dekode.v1 import agent_pb2, agent_pb2_grpc, types_pb2


@pytest.fixture()
def make_symbol_ref_proto() -> agent_pb2.SymbolResult:
    """Return a ``SymbolResult`` wrapping a fully-populated ``SymbolRef``."""
    sym = types_pb2.SymbolRef(
        id="sym-001",
        name="process_request",
        qualified_name="myapp::handlers::process_request",
        kind="function",
        visibility="public",
        file_path="src/handlers.rs",
        start_byte=120,
        end_byte=450,
        signature="fn process_request(req: Request) -> Response",
        doc_comment="Handles incoming HTTP requests.",
        parent_id="sym-000",
    )
    return agent_pb2.SymbolResult(
        symbol=sym,
        source="fn process_request(req: Request) -> Response { todo!() }",
        caller_ids=["sym-010", "sym-011"],
        callee_ids=["sym-020"],
    )


@pytest.fixture()
def make_context_response_proto() -> agent_pb2.ContextResponse:
    """Return a ``ContextResponse`` with symbols, call_graph, and dependencies."""
    sym1 = types_pb2.SymbolRef(
        id="sym-001",
        name="handle",
        qualified_name="app::handle",
        kind="function",
        visibility="public",
        file_path="src/app.rs",
        start_byte=0,
        end_byte=200,
        signature="fn handle()",
    )
    sym2 = types_pb2.SymbolRef(
        id="sym-002",
        name="helper",
        qualified_name="app::helper",
        kind="function",
        visibility="private",
        file_path="src/app.rs",
        start_byte=210,
        end_byte=350,
        signature="fn helper() -> bool",
    )
    sr1 = agent_pb2.SymbolResult(
        symbol=sym1,
        source="fn handle() { helper(); }",
        caller_ids=[],
        callee_ids=["sym-002"],
    )
    sr2 = agent_pb2.SymbolResult(
        symbol=sym2,
        caller_ids=["sym-001"],
        callee_ids=[],
    )
    edge = types_pb2.CallEdgeRef(
        caller_id="sym-001",
        callee_id="sym-002",
        kind="direct",
    )
    dep = types_pb2.DependencyRef(
        package="serde",
        version_req="^1.0",
        used_by_symbol_ids=["sym-001"],
    )
    return agent_pb2.ContextResponse(
        symbols=[sr1, sr2],
        call_graph=[edge],
        dependencies=[dep],
        estimated_tokens=1500,
    )


@pytest.fixture()
def make_submit_response_proto() -> agent_pb2.SubmitResponse:
    """Return a ``SubmitResponse`` with ACCEPTED status."""
    return agent_pb2.SubmitResponse(
        status=agent_pb2.ACCEPTED,
        changeset_id="cs-abc-123",
        new_version="v2.1.0",
        errors=[],
    )


@pytest.fixture()
def make_connect_response_proto() -> agent_pb2.ConnectResponse:
    """Return a ``ConnectResponse`` with a codebase summary."""
    summary = agent_pb2.CodebaseSummary(
        languages=["rust", "python"],
        total_symbols=4200,
        total_files=150,
    )
    return agent_pb2.ConnectResponse(
        session_id="sess-xyz-789",
        codebase_version="abc1234",
        summary=summary,
    )


# ── Mock gRPC server ────────────────────────────────────────────────


class FakeAgentServicer(agent_pb2_grpc.AgentServiceServicer):
    """In-process mock of the Dekode Agent gRPC service."""

    VALID_TOKEN = "test-token"
    _MOCK_SESSION = "mock-session-1"

    # -- Connect -----------------------------------------------------------

    def Connect(
        self,
        request: agent_pb2.ConnectRequest,
        context: grpc.ServicerContext,
    ) -> agent_pb2.ConnectResponse:
        if request.auth_token != self.VALID_TOKEN:
            context.abort(grpc.StatusCode.UNAUTHENTICATED, "invalid auth token")

        summary = agent_pb2.CodebaseSummary(
            languages=["rust", "python"],
            total_symbols=42,
            total_files=10,
        )
        return agent_pb2.ConnectResponse(
            session_id=self._MOCK_SESSION,
            codebase_version="abc123",
            summary=summary,
        )

    # -- Context -----------------------------------------------------------

    def Context(
        self,
        request: agent_pb2.ContextRequest,
        context: grpc.ServicerContext,
    ) -> agent_pb2.ContextResponse:
        if request.session_id != self._MOCK_SESSION:
            context.abort(grpc.StatusCode.NOT_FOUND, "session not found")

        sym = types_pb2.SymbolRef(
            id="sym-mock-001",
            name="parse_config",
            qualified_name="config::parse_config",
            kind="function",
            visibility="public",
            file_path="src/config.rs",
            start_byte=0,
            end_byte=120,
            signature="fn parse_config(path: &str) -> Config",
        )
        sr = agent_pb2.SymbolResult(
            symbol=sym,
            source="fn parse_config(path: &str) -> Config { todo!() }",
            caller_ids=[],
            callee_ids=["sym-mock-002"],
        )
        edge = types_pb2.CallEdgeRef(
            caller_id="sym-mock-001",
            callee_id="sym-mock-002",
            kind="direct",
        )
        dep = types_pb2.DependencyRef(
            package="toml",
            version_req="^0.5",
            used_by_symbol_ids=["sym-mock-001"],
        )
        return agent_pb2.ContextResponse(
            symbols=[sr],
            call_graph=[edge],
            dependencies=[dep],
            estimated_tokens=500,
        )

    # -- Submit ------------------------------------------------------------

    def Submit(
        self,
        request: agent_pb2.SubmitRequest,
        context: grpc.ServicerContext,
    ) -> agent_pb2.SubmitResponse:
        if request.session_id != self._MOCK_SESSION:
            context.abort(grpc.StatusCode.NOT_FOUND, "session not found")

        return agent_pb2.SubmitResponse(
            status=agent_pb2.ACCEPTED,
            changeset_id="cs-mock-1",
            new_version="def456",
        )


@pytest.fixture(scope="session")
def grpc_server() -> str:
    """Start a mock gRPC server on a random port; yield ``localhost:<port>``."""
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=2))
    agent_pb2_grpc.add_AgentServiceServicer_to_server(FakeAgentServicer(), server)
    port = server.add_insecure_port("[::]:0")
    server.start()
    yield f"localhost:{port}"
    server.stop(grace=0)


@pytest.fixture()
def grpc_channel(grpc_server: str):
    """Create an insecure gRPC channel to the mock server."""
    channel = grpc.insecure_channel(grpc_server)
    yield channel
    channel.close()
