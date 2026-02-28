-- Add optional org_id to repositories
ALTER TABLE repositories ADD COLUMN org_id UUID REFERENCES organizations(id) ON DELETE CASCADE;
CREATE INDEX idx_repositories_org_id ON repositories(org_id);
