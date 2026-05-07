// dkod-engine repo knowledge graph schema
//
// Nodes represent source-code artefacts (repos, files, symbols, commits).
// Edges encode structural relationships between them.

// ── Nodes ────────────────────────────────────────────────────────────────────

N::Repo {
    INDEX name:           String,
    url:                  String,
    default_branch:       String,
    primary_language:     String,
    indexed_at:           I64,
}

N::File {
    INDEX path:           String,
    repo_id:              String,
    language:             String,
    size_bytes:           I64,
    last_modified:        I64,
    checksum:             String,
}

N::Symbol {
    INDEX qualified_name: String,
    name:                 String,
    kind:                 String,    // fn | struct | enum | trait | impl | mod | const | type
    signature:            String,
    doc:                  String,
    start_line:           I64,
    end_line:             I64,
    is_public:            Boolean,
}

N::Commit {
    INDEX sha:            String,
    message:              String,
    author_name:          String,
    author_email:         String,
    authored_at:          I64,
    files_changed:        I64,
    insertions:           I64,
    deletions:            I64,
}

// ── Edges ────────────────────────────────────────────────────────────────────

E::RepoHasFile {
    From: Repo,
    To:   File,
}

E::Defines {
    From: File,
    To:   Symbol,
}

E::Calls {
    From: Symbol,
    To:   Symbol,
    Properties: {
        call_count: I64,
        call_sites: String,   // JSON: [{line, col}]
    }
}

E::Imports {
    From: File,
    To:   File,
    Properties: {
        import_path: String,
    }
}

E::HasCommit {
    From: Repo,
    To:   Commit,
}

E::TouchedBy {
    From: File,
    To:   Commit,
}
