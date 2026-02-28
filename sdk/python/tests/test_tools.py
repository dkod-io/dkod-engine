"""Tests for dekode.tools — LLM tool descriptors and dispatch."""

from __future__ import annotations

import json

import pytest

from dekode.client import DekodeClient
from dekode.models import ContextDepth
from dekode.tools import dekode_tools, dispatch_tool


# ── Descriptor tests ─────────────────────────────────────────────────


def test_dekode_tools_returns_six_tools() -> None:
    """dekode_tools() returns exactly 6 tools with the expected names."""
    tools = dekode_tools()

    assert len(tools) == 6
    names = {t["name"] for t in tools}
    assert names == {
        "dkod_connect",
        "dkod_context",
        "dkod_read_file",
        "dkod_write_file",
        "dkod_submit",
        "dkod_session_status",
    }


def test_tool_descriptors_have_valid_schema() -> None:
    """Each tool descriptor has name, description, and a well-formed input_schema."""
    tools = dekode_tools()

    for tool in tools:
        assert "name" in tool
        assert "description" in tool
        assert isinstance(tool["description"], str)
        assert len(tool["description"]) > 0

        schema = tool["input_schema"]
        assert schema["type"] == "object"
        assert "properties" in schema
        assert isinstance(schema["properties"], dict)
        assert "required" in schema
        assert isinstance(schema["required"], list)


def test_all_tools_have_allowed_callers() -> None:
    """Every tool includes the code_execution allowed_callers entry."""
    tools = dekode_tools()

    for tool in tools:
        assert "allowed_callers" in tool, f"{tool['name']} missing allowed_callers"
        assert "code_execution_20260120" in tool["allowed_callers"]


# ── Dispatch tests ───────────────────────────────────────────────────


def test_dispatch_search_codebase(grpc_server: str) -> None:
    """dispatch_tool('search_codebase') resolves legacy alias and returns JSON with symbols."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "search test") as session:
        raw = dispatch_tool(session, "search_codebase", {"query": "parse_config"})

    result = json.loads(raw)
    assert "symbols" in result
    assert len(result["symbols"]) >= 1
    assert result["symbols"][0]["name"] == "parse_config"


def test_dispatch_submit_changes(grpc_server: str) -> None:
    """dispatch_tool('submit_changes') resolves legacy alias and returns JSON with ACCEPTED status."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "submit test") as session:
        raw = dispatch_tool(
            session,
            "submit_changes",
            {
                "intent": "improve config parsing",
                "changes": [
                    {
                        "type": "MODIFY_FUNCTION",
                        "symbol_name": "parse_config",
                        "file_path": "src/config.rs",
                        "new_source": "fn parse_config(p: &str) -> Config { /* v2 */ }",
                        "rationale": "Simplify parameter name.",
                    },
                ],
            },
        )

    result = json.loads(raw)
    assert result["status"] == "ACCEPTED"
    assert "changeset_id" in result


def test_dispatch_unknown_tool(grpc_server: str) -> None:
    """dispatch_tool() raises ValueError for an unrecognised tool name."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "unknown test") as session:
        with pytest.raises(ValueError, match="Unknown tool: nope"):
            dispatch_tool(session, "nope", {})


def test_dispatch_search_with_optional_params(grpc_server: str) -> None:
    """dispatch_tool('search_codebase') forwards depth and max_tokens."""
    client = DekodeClient(grpc_server, auth_token="test-token")

    with client.connect("my-repo", "param test") as session:
        raw = dispatch_tool(
            session,
            "search_codebase",
            {
                "query": "parse_config",
                "depth": "SIGNATURES",
                "max_tokens": 4000,
            },
        )

    result = json.loads(raw)
    assert "symbols" in result
    assert len(result["symbols"]) >= 1
