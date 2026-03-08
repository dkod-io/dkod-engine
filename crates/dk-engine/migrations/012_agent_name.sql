-- Add agent_name to session_workspaces for distinguishing agents per repo
ALTER TABLE session_workspaces ADD COLUMN agent_name TEXT NOT NULL DEFAULT '';
