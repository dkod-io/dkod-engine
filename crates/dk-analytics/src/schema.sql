-- dk-analytics ClickHouse schema.
--
-- Kept as a plain `.sql` file so operators can also apply it by hand with
-- `clickhouse-client < schema.sql` without needing a Rust build. The DDL
-- statements are also parsed and executed by `dk_analytics::schema::migrate`
-- so `dk analytics migrate` produces the same result.
--
-- Column comments follow pytorch/test-infra's convention of inline
-- `COMMENT 'doc'` so `DESCRIBE TABLE` documents the semantics. The parser
-- in `schema.rs` keeps them intact (it only strips `-- ...` line comments,
-- not the inline ClickHouse `COMMENT` clause).

CREATE TABLE IF NOT EXISTS session_events (
    event_id UUID COMMENT 'Unique id for this event row',
    event_type String COMMENT 'Symbolic type, e.g. connect, submit, verify_started',
    session_id UUID COMMENT 'dkod session id',
    agent_id String COMMENT 'Agent name / identity that produced the event',
    repo_id UUID COMMENT 'dkod repo id the event pertains to',
    changeset_id Nullable(UUID) COMMENT 'Changeset id if applicable, NULL for session-level events',
    details String COMMENT 'Free-form JSON payload',
    affected_symbols Array(String) COMMENT 'Qualified names of symbols this event touched',
    created_at DateTime64(3) COMMENT 'Wall-clock event time (ms precision, UTC)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (repo_id, session_id, created_at);

CREATE TABLE IF NOT EXISTS changeset_lifecycle (
    changeset_id UUID COMMENT 'dkod changeset id',
    repo_id UUID COMMENT 'dkod repo id',
    session_id UUID COMMENT 'Owning session id',
    agent_id String COMMENT 'Agent name / identity',
    state String COMMENT 'New state (draft, submitted, verified, approved, merged, abandoned)',
    previous_state Nullable(String) COMMENT 'Prior state, NULL on initial creation',
    transition_at DateTime64(3) COMMENT 'When the transition happened (ms, UTC)',
    duration_ms Nullable(UInt64) COMMENT 'Milliseconds spent in previous_state, NULL if unknown'
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(transition_at)
ORDER BY (repo_id, changeset_id, transition_at);

CREATE TABLE IF NOT EXISTS verification_runs (
    run_id UUID COMMENT 'Unique id for this verification step run',
    changeset_id UUID COMMENT 'Changeset the step ran against',
    step_name String COMMENT 'Step identifier, e.g. clippy, pytest, pytorch-ci:lint',
    status String COMMENT 'pass, fail, skip, error, or a provider-specific conclusion',
    duration_ms UInt64 COMMENT 'Step wall-clock duration in milliseconds',
    stdout String COMMENT 'Captured stdout (truncated by the runner)',
    findings_count UInt32 COMMENT 'Number of findings / failures the step reported',
    created_at DateTime64(3) COMMENT 'Step completion time (ms, UTC)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (changeset_id, created_at);

CREATE TABLE IF NOT EXISTS review_results (
    review_id UUID COMMENT 'Unique id for this review invocation',
    changeset_id UUID COMMENT 'Changeset the review targeted',
    provider String COMMENT 'Review provider id (anthropic, openai, llm-judge, ...)',
    model String COMMENT 'Model identifier returned by the provider',
    score Nullable(Int32) COMMENT 'Numeric verdict score if the provider emits one',
    findings_count UInt32 COMMENT 'Number of findings reported by the reviewer',
    verdict String COMMENT 'Symbolic verdict (approve, needs_iteration, reject, ...)',
    duration_ms UInt64 COMMENT 'Review wall-clock duration in milliseconds',
    created_at DateTime64(3) COMMENT 'Review completion time (ms, UTC)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (changeset_id, created_at);
