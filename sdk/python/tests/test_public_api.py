"""Tests for the dkod package public API and re-exports."""

import dkod


def test_public_imports():
    """All 15 public names are importable from the top-level package."""
    from dkod import (
        CallEdge,
        Change,
        ChangeType,
        CodebaseSummary,
        ContextDepth,
        ContextResult,
        DkodClient,
        DkodSession,
        Dependency,
        SubmitError,
        SubmitResult,
        SubmitStatus,
        Symbol,
        dkod_tools,
        dispatch_tool,
    )

    public_names = [
        DkodClient,
        DkodSession,
        Symbol,
        ContextResult,
        Change,
        SubmitResult,
        CallEdge,
        Dependency,
        SubmitError,
        CodebaseSummary,
        ChangeType,
        ContextDepth,
        SubmitStatus,
        dkod_tools,
        dispatch_tool,
    ]
    for name in public_names:
        assert name is not None


def test_version():
    """Package exposes __version__ == '0.1.0'."""
    assert dkod.__version__ == "0.1.0"
