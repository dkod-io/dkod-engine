-- Explicit changeset state machine: replace 'open' with 'draft', add reason column.

-- Add reason column for recording why each state transition happened.
ALTER TABLE changesets ADD COLUMN IF NOT EXISTS reason TEXT NOT NULL DEFAULT '';

-- Migrate existing 'open' state rows to 'draft'.
UPDATE changesets SET state = 'draft' WHERE state = 'open';

-- Drop the old constraint and add an updated one with 'draft' instead of 'open'.
ALTER TABLE changesets DROP CONSTRAINT IF EXISTS changesets_state_check;
ALTER TABLE changesets ADD CONSTRAINT changesets_state_check
    CHECK (state IN ('draft', 'submitted', 'verifying', 'approved', 'rejected', 'merged', 'closed'));
