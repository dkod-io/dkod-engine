"""Tests that exercise the mock gRPC server (FakeAgentServicer)."""

from __future__ import annotations

import grpc
import pytest

from dekode._generated.dekode.v1 import agent_pb2, agent_pb2_grpc


# ── Connect ──────────────────────────────────────────────────────────


def test_mock_server_connect(grpc_channel: grpc.Channel) -> None:
    """Connect RPC returns a valid session_id and codebase summary."""
    stub = agent_pb2_grpc.AgentServiceStub(grpc_channel)
    response = stub.Connect(
        agent_pb2.ConnectRequest(
            agent_id="test-agent",
            auth_token="test-token",
            codebase="my-repo",
            intent="explore",
        )
    )

    assert response.session_id == "mock-session-1"
    assert response.codebase_version == "abc123"
    assert list(response.summary.languages) == ["rust", "python"]
    assert response.summary.total_symbols == 42
    assert response.summary.total_files == 10


# ── Context ──────────────────────────────────────────────────────────


def test_mock_server_context(grpc_channel: grpc.Channel) -> None:
    """Context RPC returns symbols, call graph, and dependencies."""
    stub = agent_pb2_grpc.AgentServiceStub(grpc_channel)
    response = stub.Context(
        agent_pb2.ContextRequest(
            session_id="mock-session-1",
            query="parse_config",
        )
    )

    assert len(response.symbols) == 1
    assert response.symbols[0].symbol.name == "parse_config"
    assert response.symbols[0].source == "fn parse_config(path: &str) -> Config { todo!() }"

    assert len(response.call_graph) == 1
    assert response.call_graph[0].caller_id == "sym-mock-001"
    assert response.call_graph[0].callee_id == "sym-mock-002"

    assert len(response.dependencies) == 1
    assert response.dependencies[0].package == "toml"

    assert response.estimated_tokens == 500


# ── Submit ───────────────────────────────────────────────────────────


def test_mock_server_submit(grpc_channel: grpc.Channel) -> None:
    """Submit RPC returns ACCEPTED status with changeset id."""
    stub = agent_pb2_grpc.AgentServiceStub(grpc_channel)
    response = stub.Submit(
        agent_pb2.SubmitRequest(
            session_id="mock-session-1",
            intent="refactor parse_config",
            changes=[],
        )
    )

    assert response.status == agent_pb2.ACCEPTED
    assert response.changeset_id == "cs-mock-1"
    assert response.new_version == "def456"


# ── Invalid auth ─────────────────────────────────────────────────────


def test_mock_server_invalid_auth(grpc_channel: grpc.Channel) -> None:
    """Connect with a wrong token raises UNAUTHENTICATED."""
    stub = agent_pb2_grpc.AgentServiceStub(grpc_channel)

    with pytest.raises(grpc.RpcError) as exc_info:
        stub.Connect(
            agent_pb2.ConnectRequest(
                agent_id="bad-agent",
                auth_token="wrong-token",
                codebase="my-repo",
                intent="explore",
            )
        )

    assert exc_info.value.code() == grpc.StatusCode.UNAUTHENTICATED
