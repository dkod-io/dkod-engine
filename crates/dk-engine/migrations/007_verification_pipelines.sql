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
