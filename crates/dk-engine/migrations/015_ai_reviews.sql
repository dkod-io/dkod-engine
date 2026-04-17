-- AI-generated code review results stored by the RecordReview RPC.
-- Separate from changeset_reviews (human reviews) — no user FK required.
CREATE TABLE IF NOT EXISTS changeset_ai_reviews (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    changeset_id  UUID        NOT NULL REFERENCES changesets(id) ON DELETE CASCADE,
    tier          TEXT        NOT NULL,
    score         INTEGER,
    summary       TEXT,
    findings      JSONB       NOT NULL DEFAULT '[]',
    provider      TEXT        NOT NULL,
    model         TEXT        NOT NULL,
    duration_ms   BIGINT      NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_changeset_ai_reviews_changeset
    ON changeset_ai_reviews(changeset_id);
