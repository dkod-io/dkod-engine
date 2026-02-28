-- Repositories
CREATE TABLE repositories (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Symbols
CREATE TABLE symbols (
    id UUID PRIMARY KEY,
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    visibility TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_byte INT NOT NULL,
    end_byte INT NOT NULL,
    signature TEXT,
    doc_comment TEXT,
    parent_id UUID REFERENCES symbols(id) ON DELETE SET NULL,
    last_modified_by TEXT,
    last_modified_intent TEXT,
    UNIQUE(repo_id, qualified_name)
);

CREATE INDEX idx_symbols_repo_kind ON symbols(repo_id, kind);
CREATE INDEX idx_symbols_repo_file ON symbols(repo_id, file_path);
CREATE INDEX idx_symbols_name ON symbols(name);

-- Call Graph
CREATE TABLE call_edges (
    id UUID PRIMARY KEY,
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    caller_id UUID NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    callee_id UUID NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    UNIQUE(repo_id, caller_id, callee_id, kind)
);

CREATE INDEX idx_calls_caller ON call_edges(caller_id);
CREATE INDEX idx_calls_callee ON call_edges(callee_id);

-- Dependencies
CREATE TABLE dependencies (
    id UUID PRIMARY KEY,
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    package TEXT NOT NULL,
    version_req TEXT NOT NULL,
    UNIQUE(repo_id, package)
);

-- Symbol <-> Dependency usage
CREATE TABLE symbol_dependencies (
    symbol_id UUID NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    dependency_id UUID NOT NULL REFERENCES dependencies(id) ON DELETE CASCADE,
    PRIMARY KEY (symbol_id, dependency_id)
);

-- Type Information
CREATE TABLE type_info (
    symbol_id UUID PRIMARY KEY REFERENCES symbols(id) ON DELETE CASCADE,
    params JSONB,
    return_type TEXT,
    fields JSONB,
    implements TEXT[]
);
