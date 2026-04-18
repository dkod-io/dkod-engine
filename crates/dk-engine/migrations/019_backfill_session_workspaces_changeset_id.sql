-- 019_backfill_session_workspaces_changeset_id.sql
-- Backfill changeset_id for session_workspaces rows that existed before
-- migration 017 added the column.  For each workspace whose changeset_id is
-- still NULL, we find the most-recently created changeset whose session_id
-- matches the workspace's session_id and copy its id in.
--
-- Rows with no matching changeset (e.g. human-authored PRs or workspaces
-- created before changesets were linked to sessions) remain NULL and are
-- safely filtered out by should_pin / startup_reconcile / resume, which
-- already handle the NULL case.

UPDATE session_workspaces w
   SET changeset_id = (
       SELECT c.id
         FROM changesets c
        WHERE c.session_id = w.session_id
        ORDER BY c.created_at DESC, c.id DESC
        LIMIT 1
   )
 WHERE w.changeset_id IS NULL;
