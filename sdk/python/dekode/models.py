"""Pydantic models and enums wrapping the Dekode Agent Protocol protobuf types.

Each model provides a ``from_proto()`` (or ``from_symbol_result()``) classmethod
for converting *from* protobuf messages, and ``Change`` additionally exposes a
``to_proto()`` method for building outgoing protobuf messages.
"""

from __future__ import annotations

from enum import StrEnum
from typing import Optional

from pydantic import BaseModel

from dekode._generated.dekode.v1 import agent_pb2, types_pb2

# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------


class ChangeType(StrEnum):
    """Kinds of code changes an agent can submit."""

    MODIFY_FUNCTION = "MODIFY_FUNCTION"
    ADD_FUNCTION = "ADD_FUNCTION"
    DELETE_FUNCTION = "DELETE_FUNCTION"
    MODIFY_TYPE = "MODIFY_TYPE"
    ADD_TYPE = "ADD_TYPE"
    ADD_DEPENDENCY = "ADD_DEPENDENCY"


class ContextDepth(StrEnum):
    """How much detail the server should return for a context query."""

    SIGNATURES = "SIGNATURES"
    FULL = "FULL"
    CALL_GRAPH = "CALL_GRAPH"


class SubmitStatus(StrEnum):
    """Outcome of a submit request."""

    ACCEPTED = "ACCEPTED"
    REJECTED = "REJECTED"
    CONFLICT = "CONFLICT"


# ---------------------------------------------------------------------------
# Enum ↔ Proto mapping dicts
# ---------------------------------------------------------------------------

_CHANGE_TYPE_TO_PROTO: dict[ChangeType, int] = {
    ChangeType.MODIFY_FUNCTION: agent_pb2.MODIFY_FUNCTION,
    ChangeType.ADD_FUNCTION: agent_pb2.ADD_FUNCTION,
    ChangeType.DELETE_FUNCTION: agent_pb2.DELETE_FUNCTION,
    ChangeType.MODIFY_TYPE: agent_pb2.MODIFY_TYPE,
    ChangeType.ADD_TYPE: agent_pb2.ADD_TYPE,
    ChangeType.ADD_DEPENDENCY: agent_pb2.ADD_DEPENDENCY,
}

_CHANGE_TYPE_FROM_PROTO: dict[int, ChangeType] = {
    v: k for k, v in _CHANGE_TYPE_TO_PROTO.items()
}

_CONTEXT_DEPTH_TO_PROTO: dict[ContextDepth, int] = {
    ContextDepth.SIGNATURES: agent_pb2.SIGNATURES,
    ContextDepth.FULL: agent_pb2.FULL,
    ContextDepth.CALL_GRAPH: agent_pb2.CALL_GRAPH,
}

_SUBMIT_STATUS_FROM_PROTO: dict[int, SubmitStatus] = {
    agent_pb2.ACCEPTED: SubmitStatus.ACCEPTED,
    agent_pb2.REJECTED: SubmitStatus.REJECTED,
    agent_pb2.CONFLICT: SubmitStatus.CONFLICT,
}

# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------


class CodebaseSummary(BaseModel):
    """High-level stats about the codebase returned after a ``Connect`` call."""

    languages: list[str]
    total_symbols: int
    total_files: int

    @classmethod
    def from_proto(cls, pb: agent_pb2.CodebaseSummary) -> CodebaseSummary:
        return cls(
            languages=list(pb.languages),
            total_symbols=pb.total_symbols,
            total_files=pb.total_files,
        )


class Symbol(BaseModel):
    """A single code symbol with optional source and graph edges."""

    id: str
    name: str
    qualified_name: str
    kind: str
    visibility: str
    file_path: str
    start_byte: int
    end_byte: int
    signature: str
    doc_comment: Optional[str] = None
    parent_id: Optional[str] = None
    source: Optional[str] = None
    caller_ids: list[str] = []
    callee_ids: list[str] = []

    @classmethod
    def from_symbol_result(cls, pb: agent_pb2.SymbolResult) -> Symbol:
        """Build a ``Symbol`` from a ``SymbolResult`` proto (which nests a ``SymbolRef``)."""
        ref: types_pb2.SymbolRef = pb.symbol
        return cls(
            id=ref.id,
            name=ref.name,
            qualified_name=ref.qualified_name,
            kind=ref.kind,
            visibility=ref.visibility,
            file_path=ref.file_path,
            start_byte=ref.start_byte,
            end_byte=ref.end_byte,
            signature=ref.signature,
            doc_comment=ref.doc_comment if ref.HasField("doc_comment") else None,
            parent_id=ref.parent_id if ref.HasField("parent_id") else None,
            source=pb.source if pb.HasField("source") else None,
            caller_ids=list(pb.caller_ids),
            callee_ids=list(pb.callee_ids),
        )


class CallEdge(BaseModel):
    """A directed edge in the call graph."""

    caller_id: str
    callee_id: str
    kind: str

    @classmethod
    def from_proto(cls, pb: types_pb2.CallEdgeRef) -> CallEdge:
        return cls(
            caller_id=pb.caller_id,
            callee_id=pb.callee_id,
            kind=pb.kind,
        )


class Dependency(BaseModel):
    """An external package dependency referenced by one or more symbols."""

    package: str
    version_req: str
    used_by_symbol_ids: list[str] = []

    @classmethod
    def from_proto(cls, pb: types_pb2.DependencyRef) -> Dependency:
        return cls(
            package=pb.package,
            version_req=pb.version_req,
            used_by_symbol_ids=list(pb.used_by_symbol_ids),
        )


class SubmitError(BaseModel):
    """An error reported by the server when a submit is rejected or conflicted."""

    message: str
    symbol_id: Optional[str] = None
    file_path: Optional[str] = None

    @classmethod
    def from_proto(cls, pb: agent_pb2.SubmitError) -> SubmitError:
        return cls(
            message=pb.message,
            symbol_id=pb.symbol_id if pb.HasField("symbol_id") else None,
            file_path=pb.file_path if pb.HasField("file_path") else None,
        )


class ContextResult(BaseModel):
    """The full context payload returned by the server for a ``Context`` call."""

    symbols: list[Symbol] = []
    call_graph: list[CallEdge] = []
    dependencies: list[Dependency] = []
    estimated_tokens: int = 0

    @classmethod
    def from_proto(cls, pb: agent_pb2.ContextResponse) -> ContextResult:
        return cls(
            symbols=[Symbol.from_symbol_result(s) for s in pb.symbols],
            call_graph=[CallEdge.from_proto(e) for e in pb.call_graph],
            dependencies=[Dependency.from_proto(d) for d in pb.dependencies],
            estimated_tokens=pb.estimated_tokens,
        )


class Change(BaseModel):
    """A single code change to be submitted to the server."""

    type: ChangeType
    symbol_name: str
    file_path: str
    new_source: str
    rationale: str
    old_symbol_id: Optional[str] = None

    def to_proto(self) -> agent_pb2.Change:
        """Serialize this change into a protobuf ``Change`` message."""
        kwargs: dict = dict(
            type=_CHANGE_TYPE_TO_PROTO[self.type],
            symbol_name=self.symbol_name,
            file_path=self.file_path,
            new_source=self.new_source,
            rationale=self.rationale,
        )
        if self.old_symbol_id is not None:
            kwargs["old_symbol_id"] = self.old_symbol_id
        return agent_pb2.Change(**kwargs)


class SubmitResult(BaseModel):
    """The outcome of a ``Submit`` call."""

    status: SubmitStatus
    changeset_id: str
    new_version: Optional[str] = None
    errors: list[SubmitError] = []

    @classmethod
    def from_proto(cls, pb: agent_pb2.SubmitResponse) -> SubmitResult:
        return cls(
            status=_SUBMIT_STATUS_FROM_PROTO[pb.status],
            changeset_id=pb.changeset_id,
            new_version=pb.new_version if pb.HasField("new_version") else None,
            errors=[SubmitError.from_proto(e) for e in pb.errors],
        )
