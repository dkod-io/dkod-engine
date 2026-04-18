-- 020_session_resume_redirects.sql
-- Epic B: durable dead_session → successor mapping so resume(dead_session)
-- remains idempotent after in-place session_id rotation.

CREATE TABLE IF NOT EXISTS session_resume_redirects (
    dead_session_id      UUID PRIMARY KEY,
    successor_session_id UUID NOT NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_session_resume_redirects_successor
    ON session_resume_redirects (successor_session_id);
