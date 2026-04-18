-- 018_superseded_by_drop_fk.sql
-- Epic B follow-up: superseded_by stores the session_id of the session that
-- took over a stranded workspace, NOT a workspace PK. The original FK to
-- session_workspaces(id) was incorrect; session_id is not the PK. Drop the FK
-- so writes succeed. Column remains a plain UUID pointer (no enforced integrity).

-- Postgres names inline FK constraints as `<table>_<column>_fkey` by default.
ALTER TABLE session_workspaces
    DROP CONSTRAINT IF EXISTS session_workspaces_superseded_by_fkey;
