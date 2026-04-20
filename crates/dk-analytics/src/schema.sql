-- dk-analytics ClickHouse schema.
--
-- Kept as a plain `.sql` file so operators can also apply it by hand with
-- `clickhouse-client < schema.sql` without needing a Rust build. The DDL
-- statements are also parsed and executed by `dk_analytics::schema::migrate`
-- so `dk analytics migrate` produces the same result.

CREATE TABLE IF NOT EXISTS session_events (
    event_id UUID,
    event_type String,
    session_id UUID,
    agent_id String,
    repo_id UUID,
    changeset_id Nullable(UUID),
    details String,
    affected_symbols Array(String),
    created_at DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (repo_id, session_id, created_at);

CREATE TABLE IF NOT EXISTS changeset_lifecycle (
    changeset_id UUID,
    repo_id UUID,
    session_id UUID,
    agent_id String,
    state String,
    previous_state Nullable(String),
    transition_at DateTime64(3),
    duration_ms Nullable(UInt64)
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(transition_at)
ORDER BY (repo_id, changeset_id, transition_at);

CREATE TABLE IF NOT EXISTS verification_runs (
    run_id UUID,
    changeset_id UUID,
    step_name String,
    status String,
    duration_ms UInt64,
    stdout String,
    findings_count UInt32,
    created_at DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (changeset_id, created_at);

CREATE TABLE IF NOT EXISTS review_results (
    review_id UUID,
    changeset_id UUID,
    provider String,
    model String,
    score Nullable(Int32),
    findings_count UInt32,
    verdict String,
    duration_ms UInt64,
    created_at DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (changeset_id, created_at);
