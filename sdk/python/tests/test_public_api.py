"""Tests for the dekode package public API and re-exports."""

import dekode


def test_public_imports():
    """All 15 public names are importable from the top-level package."""
    from dekode import (
        CallEdge,
        Change,
        ChangeType,
        CodebaseSummary,
        ContextDepth,
        ContextResult,
        DekodeClient,
        DekodeSession,
        Dependency,
        SubmitError,
        SubmitResult,
        SubmitStatus,
        Symbol,
        dekode_tools,
        dispatch_tool,
    )

    public_names = [
        DekodeClient,
        DekodeSession,
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
        dekode_tools,
        dispatch_tool,
    ]
    for name in public_names:
        assert name is not None


def test_version():
    """Package exposes __version__ == '0.1.0'."""
    assert dekode.__version__ == "0.1.0"
