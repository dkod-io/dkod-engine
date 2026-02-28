"""Stateful session wrapping Connect/Context/Submit Agent Protocol RPCs."""

from __future__ import annotations

from typing import TYPE_CHECKING

import grpc

from dekode._generated.dekode.v1 import agent_pb2, agent_pb2_grpc
from dekode.models import (
    Change,
    CodebaseSummary,
    ContextDepth,
    ContextResult,
    SubmitResult,
    _CONTEXT_DEPTH_TO_PROTO,
)

if TYPE_CHECKING:
    pass


class DekodeSession:
    """Stateful session wrapping Connect/Context/Submit RPCs.

    A session is obtained via :meth:`DekodeClient.connect` and represents an
    active agent connection to a specific codebase.  It holds the gRPC channel,
    a stub, the server-assigned ``session_id``, the current ``codebase_version``
    and the initial :class:`~dekode.models.CodebaseSummary`.

    Usage::

        with client.connect("my-repo", "refactor auth") as session:
            ctx = session.context("login handler")
            result = session.submit(changes, "improve error handling")
    """

    def __init__(
        self,
        channel: grpc.Channel,
        session_id: str,
        codebase_version: str,
        summary: CodebaseSummary,
    ) -> None:
        self._channel = channel
        self._stub = agent_pb2_grpc.AgentServiceStub(channel)
        self.session_id = session_id
        self.codebase_version = codebase_version
        self.summary = summary

    # -- Context ---------------------------------------------------------------

    def context(
        self,
        query: str,
        depth: ContextDepth = ContextDepth.FULL,
        include_tests: bool = False,
        include_dependencies: bool = False,
        max_tokens: int = 8000,
    ) -> ContextResult:
        """Search for symbols in the current codebase.

        Builds a ``ContextRequest``, calls the ``Context`` RPC on the server,
        and returns a :class:`~dekode.models.ContextResult` containing matched
        symbols, call-graph edges, dependencies, and a token estimate.

        Parameters
        ----------
        query:
            Free-text or qualified-name query sent to the server.
        depth:
            How much detail to request (signatures only, full source, or
            call-graph).
        include_tests:
            Whether test symbols should be included in the response.
        include_dependencies:
            Whether external dependency info should be included.
        max_tokens:
            Soft cap on estimated tokens the server should aim for.
        """
        request = agent_pb2.ContextRequest(
            session_id=self.session_id,
            query=query,
            depth=_CONTEXT_DEPTH_TO_PROTO[depth],
            include_tests=include_tests,
            include_dependencies=include_dependencies,
            max_tokens=max_tokens,
        )
        response = self._stub.Context(request)
        return ContextResult.from_proto(response)

    # -- Submit ----------------------------------------------------------------

    def submit(self, changes: list[Change], intent: str) -> SubmitResult:
        """Submit code changes to the server for verification and merge.

        Each :class:`~dekode.models.Change` is serialised via its ``to_proto()``
        method before being sent over gRPC.

        Parameters
        ----------
        changes:
            List of individual code changes to apply.
        intent:
            Human-readable description of what these changes accomplish.
        """
        request = agent_pb2.SubmitRequest(
            session_id=self.session_id,
            intent=intent,
            changes=[c.to_proto() for c in changes],
        )
        response = self._stub.Submit(request)
        return SubmitResult.from_proto(response)

    # -- File Read --------------------------------------------------------------

    def file_read(self, path: str) -> dict:
        """Read a file through the session workspace overlay."""
        request = agent_pb2.FileReadRequest(
            session_id=self.session_id,
            path=path,
        )
        response = self._stub.FileRead(request)
        return {
            "content": response.content.decode("utf-8", errors="replace"),
            "hash": response.hash,
            "modified_in_session": response.modified_in_session,
        }

    # -- File Write -------------------------------------------------------------

    def file_write(self, path: str, content: str) -> dict:
        """Write a file to the session workspace overlay."""
        request = agent_pb2.FileWriteRequest(
            session_id=self.session_id,
            path=path,
            content=content.encode("utf-8"),
        )
        response = self._stub.FileWrite(request)
        return {
            "new_hash": response.new_hash,
            "detected_changes": [
                {"symbol_name": sc.symbol_name, "change_type": sc.change_type}
                for sc in response.detected_changes
            ],
        }

    # -- Session Status ---------------------------------------------------------

    def session_status(self) -> dict:
        """Get the current state of this session's workspace."""
        request = agent_pb2.SessionStatusRequest(
            session_id=self.session_id,
        )
        response = self._stub.GetSessionStatus(request)
        return {
            "session_id": response.session_id,
            "base_commit": response.base_commit,
            "files_modified": list(response.files_modified),
            "symbols_modified": list(response.symbols_modified),
            "overlay_size_bytes": response.overlay_size_bytes,
            "active_other_sessions": response.active_other_sessions,
        }

    # -- Lifecycle -------------------------------------------------------------

    def close(self) -> None:
        """Close the underlying gRPC channel."""
        self._channel.close()

    def __enter__(self) -> DekodeSession:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()
