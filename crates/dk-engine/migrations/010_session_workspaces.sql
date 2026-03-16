-- Native Session Isolation: workspace metadata + overlay file storage

CREATE TABLE IF NOT EXISTS session_workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID NOT NULL,
    repo_id UUID NOT NULL REFERENCES repositories(id),
    base_commit_hash TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active'
        CHECK (state IN ('active', 'submitted', 'merged', 'expired', 'abandoned')),
    mode TEXT NOT NULL DEFAULT 'ephemeral'
        CHECK (mode IN ('ephemeral', 'persistent')),
    agent_id TEXT NOT NULL,
    intent TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    files_modified INT NOT NULL DEFAULT 0,
    symbols_modified INT NOT NULL DEFAULT 0,
    overlay_size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS session_overlay_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES session_workspaces(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    content BYTEA NOT NULL,
    content_hash TEXT NOT NULL,
    change_type TEXT NOT NULL
        CHECK (change_type IN ('modified', 'added', 'deleted')),
    base_content_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_id, file_path)
);

CREATE INDEX IF NOT EXISTS idx_workspaces_repo_state ON session_workspaces(repo_id, state);
CREATE INDEX IF NOT EXISTS idx_workspaces_session ON session_workspaces(session_id);
CREATE INDEX IF NOT EXISTS idx_overlay_workspace ON session_overlay_files(workspace_id);
