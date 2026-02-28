"""Tests for dekode.session — DekodeSession RPC wrappers."""

from __future__ import annotations

import grpc
import pytest

from dekode.client import DekodeClient
from dekode.models import (
    Change,
    ChangeType,
    ContextDepth,
    ContextResult,
    SubmitResult,
    SubmitStatus,
)
from dekode.session import DekodeSession


# ── Context ──────────────────────────────────────────────────────────


def test_session_context(grpc_server: str) -> None:
    """session.context() returns a ContextResult with symbols."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "explore") as session:
        result = session.context("parse_config")

        assert isinstance(result, ContextResult)
        assert len(result.symbols) == 1
        assert result.symbols[0].name == "parse_config"
        assert result.symbols[0].id == "sym-mock-001"
        assert result.symbols[0].source == "fn parse_config(path: &str) -> Config { todo!() }"

        # Call graph
        assert len(result.call_graph) == 1
        assert result.call_graph[0].caller_id == "sym-mock-001"
        assert result.call_graph[0].callee_id == "sym-mock-002"

        # Dependencies
        assert len(result.dependencies) == 1
        assert result.dependencies[0].package == "toml"

        # Token estimate
        assert result.estimated_tokens == 500


# ── Context with all params ──────────────────────────────────────────


def test_session_context_with_params(grpc_server: str) -> None:
    """session.context() forwards depth, include_tests, include_dependencies, max_tokens."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "deep exploration") as session:
        # Call with every non-default parameter to verify they're wired through.
        result = session.context(
            "parse_config",
            depth=ContextDepth.CALL_GRAPH,
            include_tests=True,
            include_dependencies=True,
            max_tokens=16000,
        )

        # The mock server doesn't vary its response based on params, but
        # the call should succeed without errors, proving serialisation works.
        assert isinstance(result, ContextResult)
        assert len(result.symbols) >= 1


# ── Submit ───────────────────────────────────────────────────────────


def test_session_submit(grpc_server: str) -> None:
    """session.submit() returns a SubmitResult with ACCEPTED status."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "refactor") as session:
        changes = [
            Change(
                type=ChangeType.MODIFY_FUNCTION,
                symbol_name="parse_config",
                file_path="src/config.rs",
                new_source="fn parse_config(path: &str) -> Config { /* improved */ }",
                rationale="Improve error handling in config parser.",
            ),
        ]
        result = session.submit(changes, "improve config parsing")

        assert isinstance(result, SubmitResult)
        assert result.status == SubmitStatus.ACCEPTED
        assert result.changeset_id == "cs-mock-1"
        assert result.new_version == "def456"
        assert result.errors == []


# ── Invalid session ──────────────────────────────────────────────────


def test_session_context_invalid_session(grpc_server: str) -> None:
    """Context RPC with a bad session_id raises NOT_FOUND."""
    channel = grpc.insecure_channel(grpc_server)

    try:
        from dekode.models import CodebaseSummary

        # Manually construct a session with a bogus session_id.
        session = DekodeSession(
            channel=channel,
            session_id="non-existent-session",
            codebase_version="fake",
            summary=CodebaseSummary(languages=[], total_symbols=0, total_files=0),
        )

        with pytest.raises(grpc.RpcError) as exc_info:
            session.context("anything")

        assert exc_info.value.code() == grpc.StatusCode.NOT_FOUND
    finally:
        channel.close()
