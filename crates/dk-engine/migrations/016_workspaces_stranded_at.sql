-- 016_workspaces_stranded_at.sql
-- Epic B: workspace eviction recovery. Adds lifecycle columns for the
-- strand → resume / abandon flow. All columns are nullable and additive —
-- existing rows are unaffected.

ALTER TABLE session_workspaces
    ADD COLUMN stranded_at       TIMESTAMPTZ,
    ADD COLUMN stranded_reason   TEXT,
    ADD COLUMN abandoned_at      TIMESTAMPTZ,
    ADD COLUMN abandoned_reason  TEXT,
    ADD COLUMN superseded_by     UUID REFERENCES session_workspaces(id);

-- Partial index: the stranded_sweep scans only rows where stranded_at IS NOT NULL.
CREATE INDEX idx_workspaces_stranded_at
    ON session_workspaces (stranded_at)
    WHERE stranded_at IS NOT NULL;

-- Partial index: startup_reconcile filters on non-abandoned rows missing a live session.
CREATE INDEX idx_workspaces_alive
    ON session_workspaces (session_id)
    WHERE stranded_at IS NULL AND abandoned_at IS NULL;
