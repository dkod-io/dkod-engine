-- Drop the platform-era changesets schema (002_platform) so the agent-protocol
-- schema below can take its place.  Dependents must be dropped first.
DROP TABLE IF EXISTS changeset_reviews CASCADE;
DROP TABLE IF EXISTS changesets CASCADE;

-- Changesets accumulate file changes before being merged to a Git commit
CREATE TABLE changesets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    session_id UUID,
    agent_id TEXT NOT NULL,
    intent TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'submitted', 'verifying', 'approved', 'rejected', 'merged')),
    base_version TEXT,
    merged_version TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_changesets_repo_id ON changesets(repo_id);
CREATE INDEX idx_changesets_status ON changesets(status);

-- Individual file changes within a changeset
CREATE TABLE changeset_files (
    changeset_id UUID NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('add', 'modify', 'delete')),
    content TEXT,
    PRIMARY KEY (changeset_id, file_path)
);

-- Track which symbols a changeset affects (for conflict detection)
CREATE TABLE changeset_symbols (
    changeset_id UUID NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    symbol_id UUID NOT NULL,
    symbol_qualified_name TEXT NOT NULL,
    PRIMARY KEY (changeset_id, symbol_id)
);
