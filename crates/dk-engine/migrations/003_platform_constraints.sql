-- Add CHECK constraints on state columns to prevent invalid values
-- at the database level.
ALTER TABLE issues
    ADD CONSTRAINT chk_issues_state CHECK (state IN ('open', 'closed'));

ALTER TABLE changesets
    ADD CONSTRAINT chk_changesets_state CHECK (state IN ('open', 'merged', 'closed'));

ALTER TABLE changeset_reviews
    ADD CONSTRAINT chk_reviews_verdict CHECK (verdict IN ('approved', 'changes_requested'));

ALTER TABLE org_members
    ADD CONSTRAINT chk_org_members_role CHECK (role IN ('owner', 'admin', 'member'));

-- Add owner_id to repositories for per-user authorization.
ALTER TABLE repositories
    ADD COLUMN owner_id UUID REFERENCES users(id);

-- Set owner_id as NOT NULL for future rows once existing data is handled.
-- For now we leave it nullable so existing repos (created before this
-- migration) do not fail the migration. The API layer always sets owner_id.
CREATE INDEX idx_repositories_owner ON repositories(owner_id);
