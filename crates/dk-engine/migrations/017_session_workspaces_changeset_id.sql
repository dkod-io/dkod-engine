-- 017_session_workspaces_changeset_id.sql
-- Epic B: persist the changeset_id that the in-memory SessionWorkspace already
-- carries, so resume / startup_reconcile / should_pin can JOIN session_workspaces
-- to changesets without a roundtrip through the session_id (which is nullable on
-- changesets for human-authored PRs).

ALTER TABLE session_workspaces
    ADD COLUMN IF NOT EXISTS changeset_id UUID REFERENCES changesets(id);

-- Partial index: should_pin and startup_reconcile filter on non-null changeset_id.
CREATE INDEX IF NOT EXISTS idx_workspaces_changeset
    ON session_workspaces (changeset_id)
    WHERE changeset_id IS NOT NULL;
