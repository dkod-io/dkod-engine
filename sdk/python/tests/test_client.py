"""Tests for dekode.client — DekodeClient connection logic."""

from __future__ import annotations

import grpc
import pytest

from dekode.client import DekodeClient
from dekode.session import DekodeSession


# ── Connect ──────────────────────────────────────────────────────────


def test_client_connect(grpc_server: str) -> None:
    """DekodeClient.connect returns a DekodeSession with correct fields."""
    client = DekodeClient(grpc_server, auth_token="test-token")
    session = client.connect("my-repo", "explore codebase")

    try:
        assert isinstance(session, DekodeSession)
        assert session.session_id == "mock-session-1"
        assert session.codebase_version == "abc123"

        # Summary is converted from proto to the Pydantic model.
        assert session.summary.languages == ["rust", "python"]
        assert session.summary.total_symbols == 42
        assert session.summary.total_files == 10
    finally:
        session.close()


# ── Invalid auth ─────────────────────────────────────────────────────


def test_client_connect_invalid_auth(grpc_server: str) -> None:
    """Connect with a wrong token raises UNAUTHENTICATED."""
    client = DekodeClient(grpc_server, auth_token="bad-token")

    with pytest.raises(grpc.RpcError) as exc_info:
        client.connect("my-repo", "explore")

    assert exc_info.value.code() == grpc.StatusCode.UNAUTHENTICATED


# ── Context manager ──────────────────────────────────────────────────


def test_client_context_manager(grpc_server: str) -> None:
    """DekodeSession supports the ``with`` statement (context manager)."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "explore") as session:
        assert isinstance(session, DekodeSession)
        assert session.session_id == "mock-session-1"

    # After exiting the context manager the channel is closed.
    # Attempting another RPC should raise.
    with pytest.raises(Exception):
        session.context("anything")
