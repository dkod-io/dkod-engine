"""Integration tests against a real dk-server.

Run with: DEKODE_SERVER_URL=localhost:50051 DEKODE_AUTH_TOKEN=secret pytest -m integration -v
"""

import os

import pytest

from dekode import (
    Change,
    ChangeType,
    ContextDepth,
    DekodeClient,
    SubmitStatus,
)

SERVER_URL = os.environ.get("DEKODE_SERVER_URL", "")
AUTH_TOKEN = os.environ.get("DEKODE_AUTH_TOKEN", "")

pytestmark = pytest.mark.integration


@pytest.fixture
def session():
    if not SERVER_URL:
        pytest.skip("DEKODE_SERVER_URL not set")
    client = DekodeClient(SERVER_URL, auth_token=AUTH_TOKEN)
    session = client.connect(codebase="test-repo", intent="integration test")
    yield session
    session.close()


def test_connect_and_summary(session):
    """Connect returns valid session with codebase summary."""
    assert session.session_id
    assert session.codebase_version
    assert session.summary.total_files > 0


def test_context_search(session):
    """Context search returns symbols."""
    result = session.context("parse", depth=ContextDepth.FULL)
    assert len(result.symbols) > 0
    for sym in result.symbols:
        assert sym.name
        assert sym.file_path


def test_submit_change(session):
    """Submit a change and verify it's accepted."""
    result = session.context("parse", depth=ContextDepth.FULL)
    if not result.symbols:
        pytest.skip("No symbols found to modify")

    sym = result.symbols[0]
    change = Change(
        type=ChangeType.MODIFY_FUNCTION,
        symbol_name=sym.name,
        file_path=sym.file_path,
        new_source=sym.source or "// modified",
        rationale="integration test",
        old_symbol_id=sym.id,
    )
    submit_result = session.submit(changes=[change], intent="integration test")
    assert submit_result.status == SubmitStatus.ACCEPTED
