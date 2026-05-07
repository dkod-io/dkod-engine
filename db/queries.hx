// dkod-engine code-intelligence queries
//
// All mutation queries return "ok" on success.
// All read queries return the requested nodes/edges.

// ── Ingest mutations ─────────────────────────────────────────────────────────

QUERY UpsertRepo(name: String, url: String, default_branch: String, primary_language: String, indexed_at: I64) =>
    repo <- AddN<Repo>({ name: name, url: url, default_branch: default_branch, primary_language: primary_language, indexed_at: indexed_at })
    RETURN repo

QUERY AddFile(repo_id: ID, path: String, language: String, size_bytes: I64, last_modified: I64, checksum: String) =>
    repo <- N<Repo>(repo_id)
    file <- AddN<File>({ path: path, repo_id: repo_id, language: language, size_bytes: size_bytes, last_modified: last_modified, checksum: checksum })
    AddE<RepoHasFile>::From(repo)::To(file)
    RETURN file

QUERY AddSymbol(file_id: ID, qualified_name: String, name: String, kind: String, signature: String, doc: String, start_line: I64, end_line: I64, is_public: Boolean) =>
    file   <- N<File>(file_id)
    symbol <- AddN<Symbol>({ qualified_name: qualified_name, name: name, kind: kind, signature: signature, doc: doc, start_line: start_line, end_line: end_line, is_public: is_public })
    AddE<Defines>::From(file)::To(symbol)
    RETURN symbol

QUERY AddCallEdge(caller_id: ID, callee_id: ID, call_count: I64, call_sites: String) =>
    caller <- N<Symbol>(caller_id)
    callee <- N<Symbol>(callee_id)
    AddE<Calls>({ call_count: call_count, call_sites: call_sites })::From(caller)::To(callee)
    RETURN "ok"

QUERY AddImportEdge(from_file_id: ID, to_file_id: ID, import_path: String) =>
    from_file <- N<File>(from_file_id)
    to_file   <- N<File>(to_file_id)
    AddE<Imports>({ import_path: import_path })::From(from_file)::To(to_file)
    RETURN "ok"

QUERY AddCommit(repo_id: ID, sha: String, message: String, author_name: String, author_email: String, authored_at: I64, files_changed: I64, insertions: I64, deletions: I64) =>
    repo   <- N<Repo>(repo_id)
    commit <- AddN<Commit>({ sha: sha, message: message, author_name: author_name, author_email: author_email, authored_at: authored_at, files_changed: files_changed, insertions: insertions, deletions: deletions })
    AddE<HasCommit>::From(repo)::To(commit)
    RETURN commit

QUERY LinkFileTouched(file_id: ID, commit_id: ID) =>
    file   <- N<File>(file_id)
    commit <- N<Commit>(commit_id)
    AddE<TouchedBy>::From(file)::To(commit)
    RETURN "ok"

// ── Lookup queries ────────────────────────────────────────────────────────────

QUERY GetRepoByName(name: String) =>
    repo <- N<Repo>(name)
    RETURN repo

QUERY GetRepoFiles(repo_id: ID) =>
    files <- N<Repo>(repo_id)::Out<RepoHasFile>
    RETURN files

QUERY GetFileSymbols(file_id: ID) =>
    symbols <- N<File>(file_id)::Out<Defines>
    RETURN symbols

QUERY GetSymbolByQualifiedName(qualified_name: String) =>
    symbol <- N<Symbol>(qualified_name)
    RETURN symbol

QUERY GetSymbolCallees(caller_id: ID) =>
    callees <- N<Symbol>(caller_id)::Out<Calls>
    RETURN callees

QUERY GetSymbolCallers(callee_id: ID) =>
    callers <- N<Symbol>(callee_id)::In<Calls>
    RETURN callers

QUERY GetRepoCommits(repo_id: ID) =>
    commits <- N<Repo>(repo_id)::Out<HasCommit>
    RETURN commits

QUERY GetFileImports(file_id: ID) =>
    imports <- N<File>(file_id)::Out<Imports>
    RETURN imports

QUERY GetFileImportedBy(file_id: ID) =>
    imported_by <- N<File>(file_id)::In<Imports>
    RETURN imported_by

QUERY GetFilesChangedInCommit(commit_id: ID) =>
    files <- N<Commit>(commit_id)::In<TouchedBy>
    RETURN files
