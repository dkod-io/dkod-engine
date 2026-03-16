-- Compound index for the activity endpoint query:
--   SELECT ... FROM changesets WHERE repo_id = $1 ORDER BY updated_at DESC LIMIT $2
CREATE INDEX IF NOT EXISTS idx_changesets_repo_updated ON changesets (repo_id, updated_at DESC);
