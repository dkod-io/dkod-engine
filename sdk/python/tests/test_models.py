"""Tests for dekode.models — Pydantic models and enum conversions."""

from __future__ import annotations

from dekode._generated.dekode.v1 import agent_pb2, types_pb2
from dekode.models import (
    CallEdge,
    Change,
    ChangeType,
    CodebaseSummary,
    ContextDepth,
    ContextResult,
    Dependency,
    SubmitError,
    SubmitResult,
    SubmitStatus,
    Symbol,
)


# ── Enum value tests ─────────────────────────────────────────────────


def test_change_type_enum() -> None:
    """ChangeType enum contains all six expected members."""
    assert ChangeType.MODIFY_FUNCTION == "MODIFY_FUNCTION"
    assert ChangeType.ADD_FUNCTION == "ADD_FUNCTION"
    assert ChangeType.DELETE_FUNCTION == "DELETE_FUNCTION"
    assert ChangeType.MODIFY_TYPE == "MODIFY_TYPE"
    assert ChangeType.ADD_TYPE == "ADD_TYPE"
    assert ChangeType.ADD_DEPENDENCY == "ADD_DEPENDENCY"
    assert len(ChangeType) == 6


def test_context_depth_enum() -> None:
    """ContextDepth enum contains three depth levels."""
    assert ContextDepth.SIGNATURES == "SIGNATURES"
    assert ContextDepth.FULL == "FULL"
    assert ContextDepth.CALL_GRAPH == "CALL_GRAPH"
    assert len(ContextDepth) == 3


def test_submit_status_enum() -> None:
    """SubmitStatus enum contains three status values."""
    assert SubmitStatus.ACCEPTED == "ACCEPTED"
    assert SubmitStatus.REJECTED == "REJECTED"
    assert SubmitStatus.CONFLICT == "CONFLICT"
    assert len(SubmitStatus) == 3


# ── Symbol ────────────────────────────────────────────────────────────


def test_symbol_from_proto(make_symbol_ref_proto: agent_pb2.SymbolResult) -> None:
    """Convert a fully-populated SymbolResult proto to a Symbol model."""
    sym = Symbol.from_symbol_result(make_symbol_ref_proto)

    assert sym.id == "sym-001"
    assert sym.name == "process_request"
    assert sym.qualified_name == "myapp::handlers::process_request"
    assert sym.kind == "function"
    assert sym.visibility == "public"
    assert sym.file_path == "src/handlers.rs"
    assert sym.start_byte == 120
    assert sym.end_byte == 450
    assert sym.signature == "fn process_request(req: Request) -> Response"
    assert sym.doc_comment == "Handles incoming HTTP requests."
    assert sym.parent_id == "sym-000"
    assert sym.source == "fn process_request(req: Request) -> Response { todo!() }"
    assert sym.caller_ids == ["sym-010", "sym-011"]
    assert sym.callee_ids == ["sym-020"]


def test_symbol_optional_fields() -> None:
    """Optional fields default to None/empty list when absent in proto."""
    ref = types_pb2.SymbolRef(
        id="sym-099",
        name="bare",
        qualified_name="mod::bare",
        kind="function",
        visibility="public",
        file_path="src/lib.rs",
        start_byte=0,
        end_byte=10,
        signature="fn bare()",
    )
    result = agent_pb2.SymbolResult(symbol=ref)
    sym = Symbol.from_symbol_result(result)

    assert sym.doc_comment is None
    assert sym.parent_id is None
    assert sym.source is None
    assert sym.caller_ids == []
    assert sym.callee_ids == []


# ── Change (to_proto) ────────────────────────────────────────────────


def test_change_to_proto() -> None:
    """Change.to_proto() produces a valid protobuf Change message."""
    change = Change(
        type=ChangeType.ADD_FUNCTION,
        symbol_name="new_handler",
        file_path="src/handlers.rs",
        new_source="fn new_handler() {}",
        rationale="Add a new request handler.",
        old_symbol_id="sym-old-001",
    )
    pb = change.to_proto()

    assert isinstance(pb, agent_pb2.Change)
    assert pb.type == agent_pb2.ADD_FUNCTION
    assert pb.symbol_name == "new_handler"
    assert pb.file_path == "src/handlers.rs"
    assert pb.new_source == "fn new_handler() {}"
    assert pb.rationale == "Add a new request handler."
    assert pb.HasField("old_symbol_id")
    assert pb.old_symbol_id == "sym-old-001"


def test_change_to_proto_without_optional() -> None:
    """Change.to_proto() omits old_symbol_id when not set."""
    change = Change(
        type=ChangeType.MODIFY_FUNCTION,
        symbol_name="existing",
        file_path="src/lib.rs",
        new_source="fn existing() { /* updated */ }",
        rationale="Refactor.",
    )
    pb = change.to_proto()

    assert not pb.HasField("old_symbol_id")


# ── ContextResult ─────────────────────────────────────────────────────


def test_context_result_from_proto(
    make_context_response_proto: agent_pb2.ContextResponse,
) -> None:
    """Full ContextResponse converts to ContextResult with all sub-models."""
    ctx = ContextResult.from_proto(make_context_response_proto)

    # Symbols
    assert len(ctx.symbols) == 2
    assert ctx.symbols[0].name == "handle"
    assert ctx.symbols[1].name == "helper"
    assert ctx.symbols[0].source == "fn handle() { helper(); }"
    assert ctx.symbols[1].source is None  # not set in fixture

    # Call graph
    assert len(ctx.call_graph) == 1
    edge = ctx.call_graph[0]
    assert edge.caller_id == "sym-001"
    assert edge.callee_id == "sym-002"
    assert edge.kind == "direct"

    # Dependencies
    assert len(ctx.dependencies) == 1
    dep = ctx.dependencies[0]
    assert dep.package == "serde"
    assert dep.version_req == "^1.0"
    assert dep.used_by_symbol_ids == ["sym-001"]

    # Tokens
    assert ctx.estimated_tokens == 1500


# ── SubmitResult ──────────────────────────────────────────────────────


def test_submit_result_from_proto(
    make_submit_response_proto: agent_pb2.SubmitResponse,
) -> None:
    """SubmitResponse with ACCEPTED status converts correctly."""
    result = SubmitResult.from_proto(make_submit_response_proto)

    assert result.status == SubmitStatus.ACCEPTED
    assert result.changeset_id == "cs-abc-123"
    assert result.new_version == "v2.1.0"
    assert result.errors == []


def test_submit_result_with_errors() -> None:
    """SubmitResult correctly surfaces a list of SubmitError objects."""
    err1 = agent_pb2.SubmitError(
        message="Symbol not found",
        symbol_id="sym-missing",
    )
    err2 = agent_pb2.SubmitError(
        message="File conflict",
        file_path="src/conflict.rs",
    )
    pb = agent_pb2.SubmitResponse(
        status=agent_pb2.CONFLICT,
        changeset_id="cs-fail-001",
        errors=[err1, err2],
    )
    result = SubmitResult.from_proto(pb)

    assert result.status == SubmitStatus.CONFLICT
    assert result.new_version is None
    assert len(result.errors) == 2

    assert result.errors[0].message == "Symbol not found"
    assert result.errors[0].symbol_id == "sym-missing"
    assert result.errors[0].file_path is None

    assert result.errors[1].message == "File conflict"
    assert result.errors[1].symbol_id is None
    assert result.errors[1].file_path == "src/conflict.rs"


# ── CodebaseSummary ───────────────────────────────────────────────────


def test_codebase_summary_from_proto(
    make_connect_response_proto: agent_pb2.ConnectResponse,
) -> None:
    """CodebaseSummary converts from the nested ConnectResponse.summary."""
    summary = CodebaseSummary.from_proto(make_connect_response_proto.summary)

    assert summary.languages == ["rust", "python"]
    assert summary.total_symbols == 4200
    assert summary.total_files == 150
