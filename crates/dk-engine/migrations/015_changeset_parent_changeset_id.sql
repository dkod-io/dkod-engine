-- Additive-only migration for PR1 of the release-locks-at-submit feature.
-- The column is defined but has no consumers in this PR — populating it and
-- enforcing merge-order on the chain lands in PR2. Declared here so PR2 can
-- ship without coordinating a schema change against the testbed.
--
-- Self-referential FK; children deleted-set-null (not CASCADE) so a rolled-
-- back parent does not silently take its children with it.

ALTER TABLE changesets
    ADD COLUMN IF NOT EXISTS parent_changeset_id UUID NULL REFERENCES changesets(id)
        ON DELETE SET NULL;

-- Lookup index for "find all children of this parent" — used by PR2's
-- changeset.parent_rollback_invalidated emission path. Partial index skips
-- the long tail of root changesets where the column is null.
CREATE INDEX IF NOT EXISTS idx_changesets_parent
    ON changesets (parent_changeset_id)
    WHERE parent_changeset_id IS NOT NULL;
