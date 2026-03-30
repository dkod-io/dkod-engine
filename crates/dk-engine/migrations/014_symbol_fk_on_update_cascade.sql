-- Add ON UPDATE CASCADE to all FK constraints referencing symbols(id).
-- This is required because upsert_symbol uses `id = EXCLUDED.id` in the
-- ON CONFLICT clause, which changes the PK on re-submission.  Without
-- ON UPDATE CASCADE, PostgreSQL refuses the PK change when child rows
-- exist in call_edges, type_info, symbol_dependencies, or symbols.parent_id.

-- symbols.parent_id
ALTER TABLE symbols DROP CONSTRAINT symbols_parent_id_fkey;
ALTER TABLE symbols ADD CONSTRAINT symbols_parent_id_fkey
    FOREIGN KEY (parent_id) REFERENCES symbols(id)
    ON DELETE SET NULL ON UPDATE CASCADE;

-- call_edges.caller_id
ALTER TABLE call_edges DROP CONSTRAINT call_edges_caller_id_fkey;
ALTER TABLE call_edges ADD CONSTRAINT call_edges_caller_id_fkey
    FOREIGN KEY (caller_id) REFERENCES symbols(id)
    ON DELETE CASCADE ON UPDATE CASCADE;

-- call_edges.callee_id
ALTER TABLE call_edges DROP CONSTRAINT call_edges_callee_id_fkey;
ALTER TABLE call_edges ADD CONSTRAINT call_edges_callee_id_fkey
    FOREIGN KEY (callee_id) REFERENCES symbols(id)
    ON DELETE CASCADE ON UPDATE CASCADE;

-- symbol_dependencies.symbol_id
ALTER TABLE symbol_dependencies DROP CONSTRAINT symbol_dependencies_symbol_id_fkey;
ALTER TABLE symbol_dependencies ADD CONSTRAINT symbol_dependencies_symbol_id_fkey
    FOREIGN KEY (symbol_id) REFERENCES symbols(id)
    ON DELETE CASCADE ON UPDATE CASCADE;

-- type_info.symbol_id
ALTER TABLE type_info DROP CONSTRAINT type_info_symbol_id_fkey;
ALTER TABLE type_info ADD CONSTRAINT type_info_symbol_id_fkey
    FOREIGN KEY (symbol_id) REFERENCES symbols(id)
    ON DELETE CASCADE ON UPDATE CASCADE;
