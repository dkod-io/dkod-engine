"""High-level client for connecting to a dkod Agent Protocol server."""

from __future__ import annotations

import grpc

from dkod._generated.dkod.v1 import agent_pb2, agent_pb2_grpc
from dkod.models import CodebaseSummary
from dkod.session import DkodSession


class DkodClient:
    """Entry point for AI agents interacting with a dkod codebase.

    Create a client with server address and credentials, then call
    :meth:`connect` to obtain a :class:`~dkod.session.DkodSession`::

        client = DkodClient("localhost:50051", auth_token="tok-abc")
        with client.connect("my-repo", "refactor auth module") as session:
            ctx = session.context("login handler")
            ...

    Parameters
    ----------
    address:
        ``host:port`` of the dkod Agent Protocol gRPC server.
    auth_token:
        Bearer / API token used for authentication.
    agent_id:
        Unique identifier for this agent (defaults to ``"default-agent"``).
    """

    def __init__(
        self,
        address: str,
        auth_token: str,
        agent_id: str = "default-agent",
    ) -> None:
        self._address = address
        self._auth_token = auth_token
        self._agent_id = agent_id

    def connect(self, codebase: str, intent: str) -> DkodSession:
        """Open a stateful session against *codebase*.

        Creates an insecure gRPC channel, sends a ``ConnectRequest``, and
        returns a :class:`~dkod.session.DkodSession` that owns the
        channel.  The caller is responsible for closing the session (or using
        it as a context manager).

        Parameters
        ----------
        codebase:
            Name or identifier of the target codebase on the server.
        intent:
            Human-readable description of what the agent intends to do.

        Raises
        ------
        grpc.RpcError
            If the server rejects the connection (e.g. ``UNAUTHENTICATED``).
        """
        channel = grpc.insecure_channel(self._address)
        stub = agent_pb2_grpc.AgentServiceStub(channel)
        request = agent_pb2.ConnectRequest(
            agent_id=self._agent_id,
            auth_token=self._auth_token,
            codebase=codebase,
            intent=intent,
        )
        response = stub.Connect(request)
        return DkodSession(
            channel=channel,
            session_id=response.session_id,
            codebase_version=response.codebase_version,
            summary=CodebaseSummary.from_proto(response.summary),
        )
