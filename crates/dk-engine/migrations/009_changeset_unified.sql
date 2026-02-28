-- Reconcile changesets schema: merge agent-protocol (006) and platform v2.
-- Alpha-only: drops and recreates with unified schema.

DROP TABLE IF EXISTS changeset_comments CASCADE;
DROP TABLE IF EXISTS changeset_reviews CASCADE;
DROP TABLE IF EXISTS verification_results CASCADE;
DROP TABLE IF EXISTS verification_pipelines CASCADE;
DROP TABLE IF EXISTS changeset_symbols CASCADE;
DROP TABLE IF EXISTS changeset_files CASCADE;
DROP TABLE IF EXISTS changesets CASCADE;

-- Unified changesets table: works for both human PRs and agent submissions
CREATE TABLE changesets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    intent_summary TEXT,
    source_branch TEXT NOT NULL DEFAULT 'main',
    target_branch TEXT NOT NULL DEFAULT 'main',
    state TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'submitted', 'verifying', 'approved', 'rejected', 'merged', 'closed')),
    author_id UUID REFERENCES users(id),
    agent_name TEXT,
    -- Agent protocol fields (kept for dual-layer compat)
    session_id UUID,
    agent_id TEXT,
    base_version TEXT,
    merged_version TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    merged_at TIMESTAMPTZ,
    UNIQUE (repo_id, number)
);

CREATE INDEX idx_changesets_repo_id ON changesets(repo_id);
CREATE INDEX idx_changesets_state ON changesets(state);

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

-- Reviews on changesets
CREATE TABLE changeset_reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    changeset_id UUID NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    reviewer_id UUID NOT NULL REFERENCES users(id),
    verdict TEXT NOT NULL CHECK (verdict IN ('approve', 'request_changes', 'comment')),
    body TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_changeset_reviews_changeset ON changeset_reviews(changeset_id);

-- Changeset comments (general + inline diff)
CREATE TABLE changeset_comments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    changeset_id UUID NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    author_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    side TEXT CHECK (side IS NULL OR side IN ('left', 'right')),
    parent_id UUID REFERENCES changeset_comments(id) ON DELETE CASCADE,
    resolved BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_changeset_comments_changeset ON changeset_comments(changeset_id);

-- Configurable verification pipeline per repository
CREATE TABLE verification_pipelines (
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    step_order INT NOT NULL,
    step_type TEXT NOT NULL
        CHECK (step_type IN ('typecheck', 'test', 'lint', 'agent-review', 'human-approve', 'custom')),
    config JSONB NOT NULL DEFAULT '{}',
    required BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (repo_id, step_order)
);

-- Results of running verification steps on a changeset
CREATE TABLE verification_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    changeset_id UUID NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    step_order INT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'pass', 'fail', 'skip')),
    output TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE INDEX idx_verification_results_changeset ON verification_results(changeset_id);
