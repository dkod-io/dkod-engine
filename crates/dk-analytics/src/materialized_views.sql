-- Optional refreshable materialized views for dk-analytics.
--
-- These follow the pattern pytorch/test-infra uses for HUD dashboards: a
-- target `MergeTree` table plus a `REFRESH EVERY N MINUTE` materialized
-- view that recomputes the aggregate on a schedule.
--
-- Applied separately from the base schema because `REFRESH EVERY` requires
-- ClickHouse 24.3+. Operators on older ClickHouse should skip this file.
-- `dk analytics migrate --with-materialized-views` applies it alongside the
-- base schema; plain `dk analytics migrate` does not.
--
-- Force an immediate refresh (mirrors pytorch/test-infra docs):
--   SYSTEM REFRESH VIEW dk_mv_changesets_merged_7d;
--   SYSTEM WAIT    VIEW dk_mv_changesets_merged_7d;

CREATE TABLE IF NOT EXISTS changesets_merged_7d (
    repo_id UUID COMMENT 'dkod repo id',
    agent_id String COMMENT 'Agent that owned the changeset',
    merged_count UInt64 COMMENT 'Number of changesets merged in the trailing 7 days',
    window_end DateTime64(3) COMMENT 'When this aggregation was recomputed'
) ENGINE = ReplacingMergeTree(window_end)
ORDER BY (repo_id, agent_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS dk_mv_changesets_merged_7d
REFRESH EVERY 5 MINUTE
TO changesets_merged_7d
AS
SELECT
    repo_id,
    agent_id,
    count() AS merged_count,
    now64(3) AS window_end
FROM changeset_lifecycle
WHERE state = 'merged'
  AND transition_at >= (now64(3) - INTERVAL 7 DAY)
GROUP BY repo_id, agent_id;

CREATE TABLE IF NOT EXISTS verification_step_daily (
    step_name String COMMENT 'Verification step name',
    day Date COMMENT 'Calendar day (UTC)',
    run_count UInt64 COMMENT 'Number of runs that day',
    fail_count UInt64 COMMENT 'Number of runs whose status was not pass/skip',
    avg_duration_ms Float64 COMMENT 'Arithmetic mean duration across all runs'
) ENGINE = ReplacingMergeTree
ORDER BY (step_name, day);

CREATE MATERIALIZED VIEW IF NOT EXISTS dk_mv_verification_step_daily
REFRESH EVERY 5 MINUTE
TO verification_step_daily
AS
SELECT
    step_name,
    toDate(created_at) AS day,
    count() AS run_count,
    countIf(status NOT IN ('pass', 'skip')) AS fail_count,
    avg(duration_ms) AS avg_duration_ms
FROM verification_runs
WHERE created_at >= (now64(3) - INTERVAL 30 DAY)
GROUP BY step_name, day;
